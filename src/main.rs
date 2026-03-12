mod compiler;
mod parser;
mod vm;

use compiler::Compiler;
use parser::{ASTParser, Parser};
use vm::VM;

use crate::{vm::VMError, vm::Value};

fn run_prog(prog: &str) -> Result<Option<Value>, VMError> {
    let mut parser = Parser::new(prog.to_string());

    if let Ok(tokens) = parser.parse() {
        let mut ast = ASTParser::new(tokens);
        if let Ok(expressions) = ast.parse() {
            let mut compiler = Compiler::new(expressions);
            if let Ok(chunks) = compiler.compile() {
                let mut vm = VM::new(chunks);
                return vm.execute();
            }
        }
    }
    Ok(None)
}

fn main() {}

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
}
