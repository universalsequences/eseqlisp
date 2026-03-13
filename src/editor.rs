use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};

use crate::buffer::Buffer;
use crate::host::{BufferId, CompileKind, HostCommand, HostEvent};
use crate::runtime::Runtime;
use crate::text::sexp_at_cursor;
use crate::vm::{Value, format_lisp_value};

#[derive(Default, Clone)]
pub struct EditorConfig {
    pub init_source: Option<String>,
}

#[derive(Debug)]
pub enum EditorError {
    Io(std::io::Error),
    Message(String),
}

impl From<std::io::Error> for EditorError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorExit {
    Cancelled,
    Closed,
    SavedAndClosed,
}

type LispBindings = HashMap<String, String>;

struct SavePrompt {
    input: String,
    quit_after_save: bool,
}

pub struct Editor {
    pub buffers: Vec<Buffer>,
    pub active: usize,
    pub minibuffer: Option<String>,

    pending_key: Option<KeyEvent>,
    builtins: HashMap<KeyEvent, String>,
    lisp_bindings: LispBindings,
    runtime: Runtime,
    needs_redraw: bool,
    should_quit: bool,
    last_exit: EditorExit,
    next_buffer_id: BufferId,
    save_prompt: Option<SavePrompt>,
}

impl Editor {
    pub fn new(mut runtime: Runtime, config: EditorConfig) -> Self {
        register_editor_natives(&mut runtime);

        let mut editor = Editor {
            buffers: vec![Buffer::new(0, "*scratch*")],
            active: 0,
            minibuffer: None,
            pending_key: None,
            builtins: HashMap::new(),
            lisp_bindings: HashMap::new(),
            runtime,
            needs_redraw: true,
            should_quit: false,
            last_exit: EditorExit::Closed,
            next_buffer_id: 1,
            save_prompt: None,
        };
        editor.bind_defaults();
        editor.load_init(config.init_source.as_deref());
        editor.refresh_runtime_side_effects();
        editor.sync_runtime_context();
        editor
    }

    pub fn needs_redraw(&self) -> bool {
        self.needs_redraw
    }

    pub fn clear_needs_redraw(&mut self) {
        self.needs_redraw = false;
    }

    pub fn mark_needs_redraw(&mut self) {
        self.needs_redraw = true;
    }

    pub fn should_quit(&self) -> bool {
        self.should_quit
    }

    pub fn clear_quit_request(&mut self) {
        self.should_quit = false;
        self.mark_needs_redraw();
    }

    pub fn prompt_text(&self) -> Option<String> {
        self.save_prompt
            .as_ref()
            .map(|prompt| format!(" Save as: {}", prompt.input))
    }

    pub fn active_buffer(&self) -> &Buffer {
        &self.buffers[self.active]
    }

    pub fn active_buffer_mut(&mut self) -> &mut Buffer {
        &mut self.buffers[self.active]
    }

    pub fn open_scratch_buffer(&mut self, name: &str, initial: &str) -> BufferId {
        let id = self.alloc_buffer_id();
        let buffer = Buffer::from_text(id, name, initial);
        self.buffers.push(buffer);
        self.active = self.buffers.len() - 1;
        self.mark_needs_redraw();
        self.sync_runtime_context();
        id
    }

    pub fn open_file_buffer(&mut self, path: impl Into<PathBuf>) -> Result<BufferId, EditorError> {
        let id = self.alloc_buffer_id();
        let buffer = Buffer::from_file(id, path)?;
        self.buffers.push(buffer);
        self.active = self.buffers.len() - 1;
        self.mark_needs_redraw();
        self.sync_runtime_context();
        Ok(id)
    }

    pub fn open_or_create_file_buffer(
        &mut self,
        path: impl Into<PathBuf>,
        initial: &str,
    ) -> Result<BufferId, EditorError> {
        let path = path.into();
        if Path::new(&path).exists() {
            self.open_file_buffer(path)
        } else {
            let id = self.alloc_buffer_id();
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| path.display().to_string());
            let mut buffer = Buffer::from_text(id, name, initial);
            buffer.set_path(path);
            buffer.dirty = false;
            self.buffers.push(buffer);
            self.active = self.buffers.len() - 1;
            self.mark_needs_redraw();
            self.sync_runtime_context();
            Ok(id)
        }
    }

    pub fn set_active_buffer(&mut self, id: BufferId) {
        if let Some(index) = self.buffers.iter().position(|buffer| buffer.id == id) {
            self.active = index;
            self.mark_needs_redraw();
            self.sync_runtime_context();
        }
    }

    pub fn handle_host_event(&mut self, event: HostEvent) {
        let message = match event {
            HostEvent::Status(msg) => msg,
            HostEvent::Error(msg) => format!("Error: {msg}"),
            HostEvent::CommandStarted { label } => format!("{label}..."),
            HostEvent::CommandFinished {
                label,
                success,
                message,
            } => {
                let outcome = if success { "finished" } else { "failed" };
                match message {
                    Some(message) => format!("{label} {outcome}: {message}"),
                    None => format!("{label} {outcome}"),
                }
            }
            HostEvent::CompileFinished {
                kind,
                success,
                name,
                diagnostics,
            } => {
                let label = match kind {
                    CompileKind::Instrument => "instrument",
                    CompileKind::Effect => "effect",
                };
                if success {
                    match name {
                        Some(name) => format!("Compiled {label} '{name}'"),
                        None => format!("Compiled {label}"),
                    }
                } else {
                    match diagnostics {
                        Some(diag) => format!("Compile failed ({label}): {diag}"),
                        None => format!("Compile failed ({label})"),
                    }
                }
            }
            HostEvent::BufferSaved { buffer_id, path } => {
                if let Some(buffer) = self.buffers.iter_mut().find(|buffer| buffer.id == buffer_id) {
                    buffer.set_path(path.clone());
                    buffer.dirty = false;
                }
                format!("Saved {}", path.display())
            }
        };
        self.minibuffer = Some(message);
        self.mark_needs_redraw();
        self.sync_runtime_context();
    }

    pub fn drain_host_commands(&mut self) -> Vec<HostCommand> {
        self.runtime.drain_host_commands()
    }

    pub fn runtime_mut(&mut self) -> &mut Runtime {
        &mut self.runtime
    }

    pub fn into_runtime(self) -> Runtime {
        self.runtime
    }

    pub fn run_embedded(&mut self) -> Result<EditorExit, EditorError> {
        loop {
            if event::poll(std::time::Duration::from_millis(16))? {
                match event::read()? {
                    Event::Key(key) => self.handle_key(key),
                    Event::Resize(_, _) => self.mark_needs_redraw(),
                    _ => {}
                }
            }
            if self.should_quit {
                return Ok(self.last_exit);
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        self.mark_needs_redraw();

        if self.handle_save_prompt_key(key) {
            return;
        }

        if let Some(prefix) = self.pending_key.take() {
            let chord = format!("{} {}", key_str(prefix), key_str(key));
            if let Some(handler) = self.lisp_bindings.get(&chord).cloned() {
                self.call_lisp_handler(&handler);
            }
            return;
        }

        if self.binding_has_prefix(&key_str(key)) {
            self.pending_key = Some(key);
            return;
        }

        if let Some(cmd) = self.builtins.get(&key).cloned() {
            self.run_command(&cmd);
            return;
        }

        if let Some(handler) = self.lisp_bindings.get(&key_str(key)).cloned() {
            self.call_lisp_handler(&handler);
            return;
        }

        match key.code {
            KeyCode::Char(c)
                if key.modifiers == KeyModifiers::NONE
                    || key.modifiers == KeyModifiers::SHIFT =>
            {
                self.minibuffer = None;
                self.active_buffer_mut().insert_char(c);
                self.sync_runtime_context();
            }
            KeyCode::Enter => {
                self.minibuffer = None;
                self.active_buffer_mut().insert_char('\n');
                self.sync_runtime_context();
            }
            _ => {}
        }
    }

    fn bind_defaults(&mut self) {
        let binds: &[(KeyCode, KeyModifiers, &str)] = &[
            (KeyCode::Char('q'), KeyModifiers::CONTROL, "quit"),
            (KeyCode::Char('s'), KeyModifiers::CONTROL, "save-buffer"),
            (KeyCode::Char('a'), KeyModifiers::CONTROL, "move-line-start"),
            (KeyCode::Char('e'), KeyModifiers::CONTROL, "move-line-end"),
            (KeyCode::Char('w'), KeyModifiers::CONTROL, "delete-word-before"),
            (KeyCode::Char('k'), KeyModifiers::CONTROL, "delete-to-line-end"),
            (KeyCode::Left, KeyModifiers::CONTROL, "move-word-left"),
            (KeyCode::Right, KeyModifiers::CONTROL, "move-word-right"),
            (KeyCode::Char('b'), KeyModifiers::ALT, "move-word-left"),
            (KeyCode::Char('f'), KeyModifiers::ALT, "move-word-right"),
            (KeyCode::Left, KeyModifiers::NONE, "move-left"),
            (KeyCode::Right, KeyModifiers::NONE, "move-right"),
            (KeyCode::Up, KeyModifiers::NONE, "move-up"),
            (KeyCode::Down, KeyModifiers::NONE, "move-down"),
            (KeyCode::Backspace, KeyModifiers::NONE, "delete-char-before"),
        ];
        for (code, mods, cmd) in binds {
            self.builtins
                .insert(KeyEvent::new(*code, *mods), cmd.to_string());
        }
    }

    fn load_init(&mut self, override_source: Option<&str>) {
        let init_src = override_source
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| std::fs::read_to_string("init.lisp").unwrap_or_default());
        if init_src.trim().is_empty() {
            return;
        }
        let _ = self.runtime.eval_str(&init_src);
        self.refresh_runtime_side_effects();
        if let Some(status) = self.runtime.take_status_message() {
            self.minibuffer = Some(status);
        }
    }

    fn run_command(&mut self, cmd: &str) {
        match cmd {
            "quit" => {
                if self.needs_save_as_prompt() {
                    self.open_save_prompt(true);
                } else {
                    self.should_quit = true;
                    self.last_exit = EditorExit::Closed;
                }
            }
            "move-left" => {
                self.minibuffer = None;
                self.active_buffer_mut().move_left();
            }
            "move-right" => {
                self.minibuffer = None;
                self.active_buffer_mut().move_right();
            }
            "move-up" => {
                self.minibuffer = None;
                self.active_buffer_mut().move_up();
            }
            "move-down" => {
                self.minibuffer = None;
                self.active_buffer_mut().move_down();
            }
            "move-line-start" => {
                self.minibuffer = None;
                self.active_buffer_mut().move_to_line_start();
            }
            "move-line-end" => {
                self.minibuffer = None;
                self.active_buffer_mut().move_to_line_end();
            }
            "move-word-left" => {
                self.minibuffer = None;
                self.active_buffer_mut().move_word_left();
            }
            "move-word-right" => {
                self.minibuffer = None;
                self.active_buffer_mut().move_word_right();
            }
            "delete-char-before" => {
                self.minibuffer = None;
                self.active_buffer_mut().delete_char_before();
            }
            "delete-word-before" => {
                self.minibuffer = None;
                self.active_buffer_mut().delete_word_before();
            }
            "delete-to-line-end" => {
                self.minibuffer = None;
                self.active_buffer_mut().delete_to_line_end();
            }
            "save-buffer" => {
                if self.needs_save_as_prompt() {
                    self.open_save_prompt(false);
                } else {
                    match self.save_active_buffer() {
                        Ok(path) => self.minibuffer = Some(format!("Saved {}", path.display())),
                        Err(error) => self.minibuffer = Some(format!("Error: {error:?}")),
                    }
                }
            }
            _ => {}
        }
        self.sync_runtime_context();
    }

    fn call_lisp_handler(&mut self, fn_name: &str) {
        self.sync_runtime_context();
        self.minibuffer = None;
        let code = format!("({fn_name})");
        match self.runtime.eval_str(&code) {
            Ok(Some(result)) => self.minibuffer = Some(format_value_for_minibuffer(&result)),
            Ok(None) => self.minibuffer = Some("No result".to_string()),
            Err(e) => self.minibuffer = Some(format!("Error: {e:?}")),
        }
        if let Some(status) = self.runtime.take_status_message() {
            self.minibuffer = Some(status);
        }
        self.refresh_runtime_side_effects();
        self.sync_runtime_context();
    }

    fn save_active_buffer(&mut self) -> Result<PathBuf, EditorError> {
        let path = self.active_buffer_mut().save()?;
        self.last_exit = EditorExit::SavedAndClosed;
        Ok(path)
    }

    fn sync_runtime_context(&mut self) {
        let active = self.active_buffer();
        let mut shared = self.runtime.shared.borrow_mut();
        shared.current_buffer_id = Some(active.id);
        shared.current_buffer_name = active.name.clone();
        shared.current_buffer_path = active.path.clone();
        shared.current_buffer_text = active.text();
        shared.current_sexp = sexp_at_cursor(&active.lines, active.cursor);
    }

    fn alloc_buffer_id(&mut self) -> BufferId {
        let id = self.next_buffer_id;
        self.next_buffer_id += 1;
        id
    }

    fn binding_has_prefix(&self, prefix: &str) -> bool {
        self.lisp_bindings.keys().any(|binding| {
            binding
                .strip_prefix(prefix)
                .map(|rest| rest.starts_with(' '))
                .unwrap_or(false)
        })
    }

    fn needs_save_as_prompt(&self) -> bool {
        self.active_buffer()
            .path
            .as_ref()
            .and_then(|path| path.file_stem())
            .map(|stem| stem == "untitled")
            .unwrap_or(true)
    }

    fn open_save_prompt(&mut self, quit_after_save: bool) {
        let default_name = self
            .active_buffer()
            .path
            .as_ref()
            .and_then(|path| path.file_stem())
            .and_then(|stem| {
                let stem = stem.to_string_lossy().to_string();
                if stem == "untitled" { None } else { Some(stem) }
            })
            .unwrap_or_default();
        self.save_prompt = Some(SavePrompt {
            input: default_name,
            quit_after_save,
        });
        self.sync_runtime_context();
    }

    fn handle_save_prompt_key(&mut self, key: KeyEvent) -> bool {
        let Some(prompt) = self.save_prompt.as_mut() else {
            return false;
        };

        match key.code {
            KeyCode::Esc => {
                self.save_prompt = None;
                self.minibuffer = Some("Save cancelled".to_string());
            }
            KeyCode::Enter => {
                let quit_after_save = prompt.quit_after_save;
                let input = prompt.input.trim().to_string();
                if input.is_empty() {
                    self.minibuffer = Some("Filename required".to_string());
                    return true;
                }
                let mut target = self
                    .active_buffer()
                    .path
                    .as_ref()
                    .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
                    .unwrap_or_default();
                let filename = if input.ends_with(".lisp") {
                    input
                } else {
                    format!("{input}.lisp")
                };
                target.push(filename);
                match self.active_buffer_mut().save_as(target) {
                    Ok(path) => {
                        self.minibuffer = Some(format!("Saved {}", path.display()));
                        self.save_prompt = None;
                        if quit_after_save {
                            self.should_quit = true;
                            self.last_exit = EditorExit::SavedAndClosed;
                        }
                    }
                    Err(error) => {
                        self.minibuffer = Some(format!("Error: {error}"));
                    }
                }
            }
            KeyCode::Backspace => {
                prompt.input.pop();
            }
            KeyCode::Char(c)
                if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT =>
            {
                prompt.input.push(c);
            }
            _ => {}
        }
        self.mark_needs_redraw();
        self.sync_runtime_context();
        true
    }

    fn refresh_runtime_side_effects(&mut self) {
        self.lisp_bindings = self.runtime.lisp_bindings();

        if let Some(path) = self.runtime.take_pending_save_as() {
            match self.active_buffer_mut().save_as(path) {
                Ok(path) => self.minibuffer = Some(format!("Saved {}", path.display())),
                Err(error) => self.minibuffer = Some(format!("Error: {error}")),
            }
        } else if self.runtime.take_pending_save() {
            match self.save_active_buffer() {
                Ok(path) => self.minibuffer = Some(format!("Saved {}", path.display())),
                Err(error) => self.minibuffer = Some(format!("Error: {error:?}")),
            }
        }
    }
}

fn format_value_for_minibuffer(value: &Value) -> String {
    let mut s = format_lisp_value(value);
    if s.len() > 240 {
        s.truncate(237);
        s.push_str("...");
    }
    s
}

fn register_editor_natives(runtime: &mut Runtime) {
    runtime.register_native("bind-key", |args, ctx| {
        let (Some(Value::String(key)), Some(Value::String(handler))) = (args.first(), args.get(1))
        else {
            return Err("bind-key expects (string string)".to_string());
        };
        ctx.bind_key(key.clone(), handler.clone());
        Ok(Value::Bool(true))
    });

    runtime.register_native("status", |args, ctx| {
        let Some(Value::String(message)) = args.first() else {
            return Err("status expects a string".to_string());
        };
        ctx.set_status(message.clone());
        Ok(Value::Bool(true))
    });

    runtime.register_native("s-expression-at-cursor", |_args, ctx| {
        Ok(ctx
            .current_sexp()
            .map(Value::String)
            .unwrap_or(Value::String(String::new())))
    });

    runtime.register_native("current-buffer-text", |_args, ctx| {
        Ok(Value::String(ctx.current_buffer_text()))
    });

    runtime.register_native("current-buffer-name", |_args, ctx| {
        Ok(Value::String(ctx.current_buffer_name()))
    });

    runtime.register_native("current-buffer-path", |_args, ctx| {
        Ok(match ctx.current_buffer_path() {
            Some(path) => Value::String(path.display().to_string()),
            None => Value::Bool(false),
        })
    });

    runtime.register_native("host-command", |args, ctx| {
        let Some(Value::String(name)) = args.first() else {
            return Err("host-command expects a command name".to_string());
        };
        let payload = args.get(1).cloned().unwrap_or(Value::Bool(true));
        let buffer_id = ctx.current_buffer_id();
        let path = ctx.current_buffer_path();
        let source = ctx.current_buffer_text();

        match name.as_str() {
            "compile-instrument" => {
                ctx.enqueue_command(HostCommand::CompileInstrument {
                    source,
                    suggested_name: extract_suggested_name(&payload),
                    buffer_id: buffer_id.unwrap_or(0),
                    path,
                });
            }
            "compile-effect" => {
                ctx.enqueue_command(HostCommand::CompileEffect {
                    source,
                    suggested_name: extract_suggested_name(&payload),
                    buffer_id: buffer_id.unwrap_or(0),
                    path,
                });
            }
            _ => {
                ctx.enqueue_command(HostCommand::Custom {
                    name: name.clone(),
                    payload,
                });
            }
        }
        Ok(Value::Bool(true))
    });

    runtime.register_native("save-buffer", |_args, ctx| {
        ctx.request_save();
        Ok(Value::Bool(true))
    });

    runtime.register_native("save-buffer-as", |args, ctx| {
        let Some(Value::String(path)) = args.first() else {
            return Err("save-buffer-as expects a path string".to_string());
        };
        ctx.request_save_as(path.clone());
        Ok(Value::Bool(true))
    });

    runtime.register_native("eval-selection-or-sexp", |_args, ctx| {
        Ok(ctx
            .current_sexp()
            .map(Value::String)
            .unwrap_or(Value::Bool(false)))
    });

    runtime.register_native("eval-buffer", |_args, ctx| {
        Ok(Value::String(ctx.current_buffer_text()))
    });
}

fn extract_suggested_name(payload: &Value) -> Option<String> {
    let Value::Map(map) = payload else {
        return None;
    };
    let value = map.get("suggested-name").or_else(|| map.get("name"))?;
    match &*value.borrow() {
        Value::String(name) if !name.is_empty() => Some(name.clone()),
        _ => None,
    }
}

fn key_str(key: KeyEvent) -> String {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let prefix = match (ctrl, alt) {
        (true, true) => "C-M-",
        (true, false) => "C-",
        (false, true) => "M-",
        (false, false) => "",
    };
    match key.code {
        KeyCode::Char(c) => format!("{prefix}{}", c.to_ascii_lowercase()),
        KeyCode::Enter => format!("{prefix}RET"),
        KeyCode::Backspace => format!("{prefix}BS"),
        KeyCode::Esc => "ESC".to_string(),
        KeyCode::Up => "UP".to_string(),
        KeyCode::Down => "DOWN".to_string(),
        KeyCode::Left => "LEFT".to_string(),
        KeyCode::Right => "RIGHT".to_string(),
        _ => format!("{:?}", key.code),
    }
}

#[cfg(test)]
mod tests {
    use super::{Editor, EditorConfig, key_str};
    use crate::host::HostCommand;
    use crate::runtime::Runtime;
    use crate::vm::Value;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;

    #[test]
    fn ctrl_char_keys_are_normalized_to_lowercase() {
        let key = KeyEvent::new(KeyCode::Char('C'), KeyModifiers::CONTROL);
        assert_eq!(key_str(key), "C-c");
    }

    #[test]
    fn ctrl_c_ctrl_c_binding_enqueues_host_command() {
        let init = r#"
            (def compile-current ()
              (host-command "compile-current" (dict :source (current-buffer-text))))
            (bind-key "C-c C-c" "compile-current")
        "#;
        let runtime = Runtime::with_init_source(init);
        let mut editor = Editor::new(
            runtime,
            EditorConfig {
                init_source: Some(init.to_string()),
            },
        );
        editor.open_scratch_buffer("*test*", "(+ 1 2)");

        editor.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        editor.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));

        let commands = editor.drain_host_commands();
        assert_eq!(commands.len(), 1);
        assert!(matches!(
            &commands[0],
            HostCommand::Custom { name, .. } if name == "compile-current"
        ));
    }

    #[test]
    fn preloaded_runtime_bindings_are_visible_to_editor() {
        let init = r#"
            (def compile-current ()
              (host-command "compile-current" (dict :source (current-buffer-text))))
            (bind-key "C-c C-c" "compile-current")
        "#;
        let runtime = Runtime::with_init_source(init);
        let mut editor = Editor::new(runtime, EditorConfig::default());
        editor.open_scratch_buffer("*test*", "(+ 1 2)");

        editor.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        editor.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));

        let commands = editor.drain_host_commands();
        assert_eq!(commands.len(), 1);
        assert!(matches!(
            &commands[0],
            HostCommand::Custom { name, .. } if name == "compile-current"
        ));
    }

    #[test]
    fn ctrl_a_moves_to_start_of_line() {
        let runtime = Runtime::new();
        let mut editor = Editor::new(runtime, EditorConfig::default());
        editor.open_scratch_buffer("*test*", "abcdef");
        editor.active_buffer_mut().cursor = (0, 4);

        editor.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL));

        assert_eq!(editor.active_buffer().cursor, (0, 0));
    }

    #[test]
    fn ctrl_e_moves_to_end_of_line() {
        let runtime = Runtime::new();
        let mut editor = Editor::new(runtime, EditorConfig::default());
        editor.open_scratch_buffer("*test*", "abcdef");
        editor.active_buffer_mut().cursor = (0, 1);

        editor.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));

        assert_eq!(editor.active_buffer().cursor, (0, 6));
    }

    #[test]
    fn ctrl_left_moves_to_previous_word() {
        let runtime = Runtime::new();
        let mut editor = Editor::new(runtime, EditorConfig::default());
        editor.open_scratch_buffer("*test*", "abc def ghi");
        editor.active_buffer_mut().cursor = (0, 10);

        editor.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL));

        assert_eq!(editor.active_buffer().cursor, (0, 8));
    }

    #[test]
    fn ctrl_right_moves_to_next_word() {
        let runtime = Runtime::new();
        let mut editor = Editor::new(runtime, EditorConfig::default());
        editor.open_scratch_buffer("*test*", "abc def ghi");
        editor.active_buffer_mut().cursor = (0, 0);

        editor.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL));

        assert_eq!(editor.active_buffer().cursor, (0, 3));
    }

    #[test]
    fn ctrl_w_deletes_previous_word() {
        let runtime = Runtime::new();
        let mut editor = Editor::new(runtime, EditorConfig::default());
        editor.open_scratch_buffer("*test*", "abc def ghi");
        editor.active_buffer_mut().cursor = (0, 8);

        editor.handle_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));

        assert_eq!(editor.active_buffer().text(), "abc ghi");
        assert_eq!(editor.active_buffer().cursor, (0, 4));
    }

    #[test]
    fn ctrl_k_deletes_rest_of_line() {
        let runtime = Runtime::new();
        let mut editor = Editor::new(runtime, EditorConfig::default());
        editor.open_scratch_buffer("*test*", "abc def ghi");
        editor.active_buffer_mut().cursor = (0, 4);

        editor.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL));

        assert_eq!(editor.active_buffer().text(), "abc ");
        assert_eq!(editor.active_buffer().cursor, (0, 4));
    }

    #[test]
    fn map_results_are_shown_in_minibuffer() {
        let init = r#"
            (def eval-sexp ()
              (eval (s-expression-at-cursor)))
            (bind-key "C-x C-e" "eval-sexp")
        "#;
        let mut runtime = Runtime::new();
        runtime.register_native("return-map", |_args, _ctx| {
            let mut map = HashMap::new();
            map.insert(
                "step".to_string(),
                Rc::new(RefCell::new(Value::Number(1.0))),
            );
            Ok(Value::Map(map))
        });
        let mut editor = Editor::new(
            runtime,
            EditorConfig {
                init_source: Some(init.to_string()),
            },
        );
        editor.open_scratch_buffer("*test*", "(return-map)");
        editor.active_buffer_mut().cursor = (0, "(return-map)".len());

        editor.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL));
        editor.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));

        let minibuffer = editor.minibuffer.unwrap_or_default();
        assert!(minibuffer.contains("step"));
    }

    #[test]
    fn eval_updates_after_buffer_contents_change() {
        let init = r#"
            (def eval-sexp ()
              (eval (s-expression-at-cursor)))
            (bind-key "C-x C-e" "eval-sexp")
        "#;
        let runtime = Runtime::new();
        let mut editor = Editor::new(
            runtime,
            EditorConfig {
                init_source: Some(init.to_string()),
            },
        );
        editor.open_scratch_buffer("*test*", "(+ 5 10)");
        editor.active_buffer_mut().cursor = (0, "(+ 5 10)".len());

        editor.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL));
        editor.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));
        assert_eq!(editor.minibuffer.as_deref(), Some("15"));

        editor.active_buffer_mut().set_text("(+ 100 100)");
        editor.active_buffer_mut().cursor = (0, "(+ 100 100)".len());

        editor.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL));
        editor.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));
        assert_eq!(editor.minibuffer.as_deref(), Some("200"));
    }

    #[test]
    fn movement_clears_minibuffer_message() {
        let runtime = Runtime::new();
        let mut editor = Editor::new(runtime, EditorConfig::default());
        editor.open_scratch_buffer("*test*", "abc");
        editor.minibuffer = Some("15".to_string());
        editor.active_buffer_mut().cursor = (0, 1);

        editor.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));

        assert_eq!(editor.minibuffer, None);
    }
}
