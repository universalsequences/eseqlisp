mod buffer;
mod compiler;
mod editor;
mod parser;
mod text;
mod tui;
mod vm;

use crossterm::event::{self, Event};
use ratatui::DefaultTerminal;
use std::{io, time::Duration};

use compiler::Compiler;
use parser::{ASTParser, Parser};
use vm::{VM, VMError, Value, register_core_natives};

#[allow(dead_code)]
fn run_prog(prog: &str) -> Result<Option<Value>, VMError> {
    let mut vm = VM::new(vec![]);
    register_core_natives(&mut vm);
    vm.eval_str(prog)
}

fn run_editor(terminal: &mut DefaultTerminal) -> io::Result<()> {
    let mut editor = editor::Editor::new();

    loop {
        // Poll for input with a short timeout so we could fire timers later.
        if event::poll(Duration::from_millis(16))? {
            match event::read()? {
                Event::Key(key) => editor.handle_key(key),
                Event::Resize(_, _) => editor.needs_redraw = true,
                _ => {}
            }
        }

        if editor.needs_redraw {
            terminal.draw(|frame| tui::render(frame, &mut editor))?;
            editor.needs_redraw = false;
        }

        if editor.should_quit {
            break;
        }
    }
    Ok(())
}

fn main() -> io::Result<()> {
    // Ensure the terminal is restored even if the editor panics (e.g. todo!())
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        ratatui::restore();
        default_hook(info);
    }));

    let mut terminal = ratatui::init();
    let result = run_editor(&mut terminal);
    ratatui::restore();
    result
}

#[cfg(test)]
mod tests {
    use super::{Value, run_prog};

    #[test]
    fn test_basic_sum() {
        assert_eq!(run_prog("(+ 1 2)"), Ok(Some(Value::Number(3.0))));
    }

    #[test]
    fn test_var_set() {
        assert_eq!(
            run_prog("(def x 5) (* x 10)"),
            Ok(Some(Value::Number(50.0)))
        );
    }

    #[test]
    fn test_function_def() {
        assert_eq!(
            run_prog("(def sq (x) (* x x)) (sq 10)"),
            Ok(Some(Value::Number(100.0)))
        );
    }

    #[test]
    fn test_function_closure() {
        assert_eq!(
            run_prog(
                "(def sq (x) (* x x)) (def make-fn (fn a) (lambda (y) (fn (+ a y)))) ((make-fn sq 2) 10)"
            ),
            Ok(Some(Value::Number(144.0)))
        );
    }

    #[test]
    fn test_lambda_shorthand_as_argument() {
        assert_eq!(
            run_prog("(def use-fn (val fn) (fn val)) (use-fn 5 |x| (+ x 1))"),
            Ok(Some(Value::Number(6.0)))
        );
    }

    #[test]
    fn test_lambda_shorthand_with_multiple_args() {
        assert_eq!(
            run_prog("(def use-fn (a b fn) (fn a b)) (use-fn 5 7 |x y| (+ x y))"),
            Ok(Some(Value::Number(12.0)))
        );
    }

    #[test]
    fn test_if_statement_false() {
        assert_eq!(
            run_prog("(if (= 5 4) 10 15)"),
            Ok(Some(Value::Number(15.0)))
        );
    }

    #[test]
    fn test_if_statement_true() {
        assert_eq!(
            run_prog("(if (= 5 5) 10 15)"),
            Ok(Some(Value::Number(10.0)))
        );
    }

    #[test]
    fn test_recursion() {
        assert_eq!(
            run_prog("(def gauss (n) (if (= n 0) 0 (+ n (gauss (- n 1))))) (gauss 5)"),
            Ok(Some(Value::Number(15.0)))
        );
    }

    #[test]
    fn test_dict_get() {
        assert_eq!(
            run_prog("(def p (dict :name \"Alec\" :age 25)) (get p :age)"),
            Ok(Some(Value::Number(25.0)))
        );
    }

    #[test]
    fn test_merge() {
        assert_eq!(
            run_prog("(def p (dict :age 25)) (def p2 (merge p :age 30)) (get p2 :age)"),
            Ok(Some(Value::Number(30.0)))
        );
    }

    #[test]
    fn test_dot_syntax() {
        assert_eq!(
            run_prog("(def p (dict :age 25)) p.age"),
            Ok(Some(Value::Number(25.0)))
        );
    }

    #[test]
    fn test_quote_symbol() {
        assert_eq!(
            run_prog("'hello"),
            Ok(Some(Value::Symbol("hello".to_string())))
        );
    }

    #[test]
    fn test_quote_list() {
        assert_eq!(
            run_prog("'(1 2 3)"),
            Ok(Some(Value::List(vec![
                std::rc::Rc::new(std::cell::RefCell::new(Value::Number(1.0))),
                std::rc::Rc::new(std::cell::RefCell::new(Value::Number(2.0))),
                std::rc::Rc::new(std::cell::RefCell::new(Value::Number(3.0))),
            ])))
        );
    }
}
