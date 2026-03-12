use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use crate::vm::register_core_natives;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::buffer::Buffer;
use crate::text::sexp_at_cursor;
use crate::vm::{VM, Value};

/// Shared context updated before each Lisp handler call so natives can read buffer state.
type BufContext = Rc<RefCell<Option<(Vec<String>, (usize, usize))>>>;

/// Shared table populated by `(bind-key key fn-name)` calls in init.lisp.
type LispBindings = Rc<RefCell<HashMap<String, String>>>;

pub struct Editor {
    pub buffers: Vec<Buffer>,
    pub active: usize,
    pub needs_redraw: bool,
    pub should_quit: bool,
    pub minibuffer: Option<String>,

    // Chord support: tracks first key of a two-key sequence (e.g. C-x)
    pending_key: Option<KeyEvent>,

    // Built-in Rust key→command bindings (movement, quit, etc.)
    builtins: HashMap<KeyEvent, String>,

    // Lisp-defined bindings: "C-x C-e" → "eval-sexp"
    lisp_bindings: LispBindings,

    // Buffer state snapshot for natives that need to read the buffer
    buf_context: BufContext,

    vm: VM,
}

impl Editor {
    pub fn new() -> Self {
        let lisp_bindings: LispBindings = Rc::new(RefCell::new(HashMap::new()));
        let buf_context: BufContext = Rc::new(RefCell::new(None));

        let mut vm = VM::new(vec![]);
        register_core_natives(&mut vm);
        register_natives(&mut vm, lisp_bindings.clone(), buf_context.clone());

        let init_src = std::fs::read_to_string("init.lisp").unwrap_or_default();
        if !init_src.is_empty() {
            let _ = vm.eval_str(&init_src);
        }

        let mut e = Editor {
            buffers: vec![Buffer::new("*scratch*")],
            active: 0,
            needs_redraw: true,
            should_quit: false,
            minibuffer: None,
            pending_key: None,
            builtins: HashMap::new(),
            lisp_bindings,
            buf_context,
            vm,
        };
        e.bind_defaults();
        e
    }

    fn bind_defaults(&mut self) {
        let binds: &[(KeyCode, KeyModifiers, &str)] = &[
            (KeyCode::Char('q'), KeyModifiers::CONTROL, "quit"),
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

    pub fn active_buffer(&self) -> &Buffer {
        &self.buffers[self.active]
    }

    pub fn active_buffer_mut(&mut self) -> &mut Buffer {
        &mut self.buffers[self.active]
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        self.needs_redraw = true;

        // If a chord prefix is pending, resolve the full chord
        if let Some(prefix) = self.pending_key.take() {
            let chord = format!("{} {}", key_str(prefix), key_str(key));
            let handler = self.lisp_bindings.borrow().get(&chord).cloned();
            if let Some(handler) = handler {
                self.call_lisp_handler(&handler);
            }
            return;
        }

        // C-x starts a chord
        if key == KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL) {
            self.pending_key = Some(key);
            return;
        }

        if let Some(cmd) = self.builtins.get(&key).cloned() {
            self.run_command(&cmd);
            return;
        }

        let lisp_handler = self
            .lisp_bindings
            .clone()
            .borrow()
            .get(&key_str(key))
            .cloned();
        if let Some(handler) = lisp_handler {
            self.call_lisp_handler(&handler);
            return;
        }

        if let KeyCode::Char(c) = key.code {
            if key.modifiers == KeyModifiers::NONE || key.modifiers == KeyModifiers::SHIFT {
                self.active_buffer_mut().insert_char(c);
            }
        } else if key.code == KeyCode::Enter {
            self.active_buffer_mut().insert_char('\n');
        }
    }

    fn call_lisp_handler(&mut self, fn_name: &str) {
        *self.buf_context.borrow_mut() = Some((
            self.active_buffer().lines.clone(),
            self.active_buffer().cursor,
        ));

        let code = format!("({})", fn_name);
        match self.vm.eval_str(&code) {
            Ok(Some(result)) => self.minibuffer = Some(format!("{result:?}")),
            Ok(None) => {}
            Err(e) => self.minibuffer = Some(format!("Error: {e:?}")),
        }
    }

    fn run_command(&mut self, cmd: &str) {
        match cmd {
            "quit" => self.should_quit = true,
            "move-left" => self.active_buffer_mut().move_left(),
            "move-right" => self.active_buffer_mut().move_right(),
            "move-up" => self.active_buffer_mut().move_up(),
            "move-down" => self.active_buffer_mut().move_down(),
            "delete-char-before" => self.active_buffer_mut().delete_char_before(),
            _ => {}
        }
    }
}

fn register_natives(vm: &mut VM, lisp_bindings: LispBindings, buf_context: BufContext) {
    // (bind-key "C-x C-e" "eval-sexp") — registers a Lisp key binding
    vm.register_native("bind-key", {
        let lisp_bindings = lisp_bindings.clone();
        move |args| {
            if let (Some(Value::String(key)), Some(Value::String(handler))) =
                (args.first(), args.get(1))
            {
                lisp_bindings
                    .borrow_mut()
                    .insert(key.clone(), handler.clone());
            }
            Value::Bool(true)
        }
    });

    // (s-expression-at-cursor) — returns the sexp under the cursor as a string
    vm.register_native("s-expression-at-cursor", {
        let buf_context = buf_context.clone();
        move |_args| {
            if let Some((lines, cursor)) = &*buf_context.borrow() {
                match sexp_at_cursor(lines, *cursor) {
                    Some(s) => Value::String(s),
                    None => Value::String(String::new()),
                }
            } else {
                Value::String(String::new())
            }
        }
    });
}

fn key_str(key: KeyEvent) -> String {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let prefix = if ctrl { "C-" } else { "" };
    match key.code {
        KeyCode::Char(c) => format!("{prefix}{c}"),
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
