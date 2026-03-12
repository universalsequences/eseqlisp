use crate::compiler::{Chunk, Compiler, OpCode};
use crate::parser::{ASTParser, Parser};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

#[derive(Debug, PartialEq)]
pub enum VMError {
    UnknownConstant,
    StackUnderflow,
    IncorrectType,
    UnknownVariable,
    ExpectedFunction,
    ParseError,
    CompileError,
}

pub type NativeFn = Rc<dyn Fn(Vec<Value>) -> Value>;

pub enum Value {
    Number(f64),
    Bool(bool),
    String(String),
    Symbol(String),
    Keyword(String),
    List(Vec<Rc<RefCell<Value>>>),
    Map(HashMap<String, Rc<RefCell<Value>>>),
    Closure(usize, Vec<Rc<RefCell<Value>>>),
    Function(usize),
    NativeFunction(NativeFn),
}

impl std::fmt::Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Number(n) => write!(f, "{n}"),
            Self::Bool(b) => write!(f, "{b}"),
            Self::String(s) => write!(f, "{s}"),
            Self::Symbol(s) => write!(f, "'{s}"),
            Self::Keyword(s) => write!(f, ":{s}"),
            Self::List(l) => write!(f, "{l:?}"),
            Self::Map(m) => write!(f, "{m:?}"),
            Self::Closure(i, _) => write!(f, "<closure:{i}>"),
            Self::Function(i) => write!(f, "<fn:{i}>"),
            Self::NativeFunction(_) => write!(f, "<native>"),
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Number(a), Self::Number(b)) => a == b,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Symbol(a), Self::Symbol(b)) => a == b,
            (Self::Keyword(a), Self::Keyword(b)) => a == b,
            (Self::List(a), Self::List(b)) => {
                a.len() == b.len()
                    && a.iter().zip(b.iter()).all(|(x, y)| *x.borrow() == *y.borrow())
            }
            (Self::Closure(a, _), Self::Closure(b, _)) => a == b,
            (Self::Function(a), Self::Function(b)) => a == b,
            _ => false,
        }
    }
}

impl Clone for Value {
    fn clone(&self) -> Self {
        match self {
            Self::Number(n) => Self::Number(*n),
            Self::Bool(b) => Self::Bool(*b),
            Self::String(s) => Self::String(s.clone()),
            Self::Symbol(s) => Self::Symbol(s.clone()),
            Self::Keyword(s) => Self::Keyword(s.clone()),
            Self::List(l) => Self::List(l.clone()),
            Self::Map(m) => Self::Map(m.clone()),
            Self::Closure(i, u) => Self::Closure(*i, u.clone()),
            Self::Function(i) => Self::Function(*i),
            Self::NativeFunction(f) => Self::NativeFunction(f.clone()),
        }
    }
}

struct Frame {
    locals: Vec<Option<Rc<RefCell<Value>>>>,
    upvalues: Vec<Rc<RefCell<Value>>>,
    pc: usize,
    chunk_idx: usize,
}

pub struct VM {
    pub chunks: Vec<Chunk>,
    current_chunk: usize,
    globals: Vec<Option<Rc<RefCell<Value>>>>,
    pub global_names: Vec<String>,
}

/// Register built-in functions available in all contexts.
pub fn register_core_natives(vm: &mut VM) {
    // (dict :key val :key val ...) → Map
    vm.register_native("dict", |args| {
        let mut map = HashMap::new();
        let mut i = 0;
        while i + 1 < args.len() {
            if let Value::Keyword(k) = &args[i] {
                map.insert(k.clone(), Rc::new(RefCell::new(args[i + 1].clone())));
            }
            i += 2;
        }
        Value::Map(map)
    });

    // (get map :key) → value, or false if missing
    vm.register_native("get", |args| {
        if let (Some(Value::Map(m)), Some(Value::Keyword(k))) = (args.first(), args.get(1)) {
            m.get(k).map(|v| v.borrow().clone()).unwrap_or(Value::Bool(false))
        } else {
            Value::Bool(false)
        }
    });

    // (merge map :key val ...) → new map with overrides
    vm.register_native("merge", |args| {
        let mut map = if let Some(Value::Map(m)) = args.first() {
            m.iter().map(|(k, v)| (k.clone(), v.clone())).collect::<HashMap<_, _>>()
        } else {
            HashMap::new()
        };
        let mut i = 1;
        while i + 1 < args.len() {
            if let Value::Keyword(k) = &args[i] {
                map.insert(k.clone(), Rc::new(RefCell::new(args[i + 1].clone())));
            }
            i += 2;
        }
        Value::Map(map)
    });

    // (keys map) → List of keywords
    vm.register_native("keys", |args| {
        if let Some(Value::Map(m)) = args.first() {
            Value::List(
                m.keys()
                    .map(|k| Rc::new(RefCell::new(Value::Keyword(k.clone()))))
                    .collect(),
            )
        } else {
            Value::List(vec![])
        }
    });

    // (first list) → first element or false
    vm.register_native("first", |args| {
        if let Some(Value::List(l)) = args.first() {
            l.first().map(|v| v.borrow().clone()).unwrap_or(Value::Bool(false))
        } else {
            Value::Bool(false)
        }
    });

    // (rest list) → tail of list or empty list
    vm.register_native("rest", |args| {
        if let Some(Value::List(l)) = args.first() {
            Value::List(l[1..].to_vec())
        } else {
            Value::List(vec![])
        }
    });

    // (cons val list) → new list with val prepended
    vm.register_native("cons", |args| {
        if let (Some(head), Some(Value::List(tail))) = (args.first(), args.get(1)) {
            let mut new = vec![Rc::new(RefCell::new(head.clone()))];
            new.extend(tail.iter().cloned());
            Value::List(new)
        } else {
            Value::List(vec![])
        }
    });

    // (len list-or-string) → number
    vm.register_native("len", |args| match args.first() {
        Some(Value::List(l)) => Value::Number(l.len() as f64),
        Some(Value::String(s)) => Value::Number(s.len() as f64),
        _ => Value::Number(0.0),
    });

    // (append list ...) → concatenated list
    vm.register_native("append", |args| {
        let mut result = vec![];
        for arg in &args {
            if let Value::List(l) = arg {
                result.extend(l.iter().cloned());
            }
        }
        Value::List(result)
    });

    // (not val) → bool
    vm.register_native("not", |args| {
        Value::Bool(matches!(args.first(), Some(Value::Bool(false)) | None))
    });

    // (str val ...) → concatenated string representation
    vm.register_native("str", |args| {
        Value::String(args.iter().map(|v| format!("{v:?}")).collect::<Vec<_>>().join(""))
    });
}

impl VM {
    pub fn new(chunks: Vec<Chunk>) -> Self {
        VM {
            chunks,
            current_chunk: 0,
            globals: vec![None; 512],
            global_names: vec![],
        }
    }

    /// Register a Rust function as a named global callable from Lisp.
    pub fn register_native(&mut self, name: &str, f: impl Fn(Vec<Value>) -> Value + 'static) {
        let idx = self.ensure_global(name);
        self.globals[idx] = Some(Rc::new(RefCell::new(Value::NativeFunction(Rc::new(f)))));
    }

    /// Compile and run `code` in this VM's existing context (globals persist).
    pub fn eval_str(&mut self, code: &str) -> Result<Option<Value>, VMError> {
        let tokens = Parser::new(code.to_string())
            .parse()
            .map_err(|_| VMError::ParseError)?;
        let exprs = ASTParser::new(tokens)
            .parse()
            .map_err(|_| VMError::ParseError)?;

        let entry_idx = self.chunks.len();
        let existing = self.chunks.clone();
        let names = self.global_names.clone();

        let mut compiler = Compiler::new_repl(exprs, existing, names);
        match compiler.compile() {
            Ok(chunks) => {
                self.chunks = chunks;
                self.global_names = compiler.into_global_names();
                self.execute_from(entry_idx)
            }
            Err(_) => Err(VMError::CompileError),
        }
    }

    fn ensure_global(&mut self, name: &str) -> usize {
        if let Some(idx) = self.global_names.iter().position(|n| n == name) {
            return idx;
        }
        let idx = self.global_names.len();
        self.global_names.push(name.to_string());
        idx
    }

    fn execute_from(&mut self, entry_chunk: usize) -> Result<Option<Value>, VMError> {
        self.current_chunk = entry_chunk;
        self.execute()
    }

    fn chunk(&self) -> &Chunk {
        self.chunks.get(self.current_chunk).unwrap()
    }

    fn new_frame(&self) -> Frame {
        Frame {
            locals: vec![None; self.chunk().symbols.len()],
            upvalues: vec![],
            pc: 0,
            chunk_idx: self.current_chunk,
        }
    }

    pub fn execute(&mut self) -> Result<Option<Value>, VMError> {
        let mut stack: Vec<Rc<RefCell<Value>>> = vec![];
        let mut frames: Vec<Frame> = vec![self.new_frame()];

        while frames.last().unwrap().pc < self.chunks[self.current_chunk].ops.len() {
            let op = self.chunks[self.current_chunk].ops[frames.last().unwrap().pc].clone();
            match op {
                OpCode::PushConst(x) => {
                    if let Some(constant) = self.chunks[self.current_chunk].constants.get(x) {
                        stack.push(Rc::new(RefCell::new(Value::Number(*constant))));
                        frames.last_mut().unwrap().pc += 1;
                    } else {
                        return Err(VMError::UnknownConstant);
                    }
                }
                OpCode::PushStr(x) => {
                    if let Some(s) = self.chunks[self.current_chunk].strings.get(x) {
                        stack.push(Rc::new(RefCell::new(Value::String(s.clone()))));
                        frames.last_mut().unwrap().pc += 1;
                    } else {
                        return Err(VMError::UnknownConstant);
                    }
                }
                OpCode::Add(arity) => {
                    if stack.len() < arity {
                        return Err(VMError::StackUnderflow);
                    }
                    let mut sum: f64 = 0.0;
                    for _ in 0..arity {
                        if let Some(val) = stack.pop() {
                            match *val.borrow() {
                                Value::Number(val) => sum += val,
                                _ => return Err(VMError::IncorrectType),
                            }
                        }
                    }
                    stack.push(Rc::new(RefCell::new(Value::Number(sum))));
                    frames.last_mut().unwrap().pc += 1;
                }
                OpCode::Sub(arity) => {
                    if stack.len() < arity {
                        return Err(VMError::StackUnderflow);
                    }
                    let mut nums: Vec<f64> = vec![];
                    for _ in 0..arity {
                        if let Some(val) = stack.pop() {
                            match *val.borrow() {
                                Value::Number(val) => nums.push(val),
                                _ => return Err(VMError::IncorrectType),
                            }
                        }
                    }
                    nums.reverse();
                    let diff = nums[1..].iter().fold(nums[0], |acc, x| acc - x);
                    stack.push(Rc::new(RefCell::new(Value::Number(diff))));
                    frames.last_mut().unwrap().pc += 1;
                }
                OpCode::Mul(arity) => {
                    if stack.len() < arity {
                        return Err(VMError::StackUnderflow);
                    }
                    let mut product: f64 = 1.0;
                    for _ in 0..arity {
                        if let Some(val) = stack.pop() {
                            match *val.borrow() {
                                Value::Number(val) => product *= val,
                                _ => return Err(VMError::IncorrectType),
                            }
                        }
                    }
                    stack.push(Rc::new(RefCell::new(Value::Number(product))));
                    frames.last_mut().unwrap().pc += 1;
                }
                OpCode::Eq => {
                    if stack.len() < 2 {
                        return Err(VMError::StackUnderflow);
                    }
                    let mut result: bool = false;
                    if let (Some(a), Some(b)) = (stack.pop(), stack.pop()) {
                        match (&*a.borrow(), &*b.borrow()) {
                            (Value::Number(a), Value::Number(b)) => result = a == b,
                            (Value::Bool(a), Value::Bool(b)) => result = a == b,
                            (Value::String(a), Value::String(b)) => result = a == b,
                            _ => return Err(VMError::IncorrectType),
                        }
                    }
                    stack.push(Rc::new(RefCell::new(Value::Bool(result))));
                    frames.last_mut().unwrap().pc += 1;
                }
                OpCode::Lt => {
                    if stack.len() < 2 {
                        return Err(VMError::StackUnderflow);
                    }
                    if let (Some(a), Some(b)) = (stack.pop(), stack.pop()) {
                        match (&*a.borrow(), &*b.borrow()) {
                            (Value::Number(a), Value::Number(b)) => {
                                stack.push(Rc::new(RefCell::new(Value::Bool(b < a))));
                            }
                            _ => return Err(VMError::IncorrectType),
                        }
                    }
                    frames.last_mut().unwrap().pc += 1;
                }
                OpCode::Gt => {
                    if stack.len() < 2 {
                        return Err(VMError::StackUnderflow);
                    }
                    if let (Some(a), Some(b)) = (stack.pop(), stack.pop()) {
                        match (&*a.borrow(), &*b.borrow()) {
                            (Value::Number(a), Value::Number(b)) => {
                                stack.push(Rc::new(RefCell::new(Value::Bool(b > a))));
                            }
                            _ => return Err(VMError::IncorrectType),
                        }
                    }
                    frames.last_mut().unwrap().pc += 1;
                }
                OpCode::Lte => {
                    if stack.len() < 2 {
                        return Err(VMError::StackUnderflow);
                    }
                    if let (Some(a), Some(b)) = (stack.pop(), stack.pop()) {
                        match (&*a.borrow(), &*b.borrow()) {
                            (Value::Number(a), Value::Number(b)) => {
                                stack.push(Rc::new(RefCell::new(Value::Bool(b <= a))));
                            }
                            _ => return Err(VMError::IncorrectType),
                        }
                    }
                    frames.last_mut().unwrap().pc += 1;
                }
                OpCode::Gte => {
                    if stack.len() < 2 {
                        return Err(VMError::StackUnderflow);
                    }
                    if let (Some(a), Some(b)) = (stack.pop(), stack.pop()) {
                        match (&*a.borrow(), &*b.borrow()) {
                            (Value::Number(a), Value::Number(b)) => {
                                stack.push(Rc::new(RefCell::new(Value::Bool(b >= a))));
                            }
                            _ => return Err(VMError::IncorrectType),
                        }
                    }
                    frames.last_mut().unwrap().pc += 1;
                }
                OpCode::MakeList(arity) => {
                    if stack.len() < arity {
                        return Err(VMError::StackUnderflow);
                    }
                    let mut list: Vec<_> = (0..arity).filter_map(|_| stack.pop()).collect();
                    list.reverse();
                    stack.push(Rc::new(RefCell::new(Value::List(list))));
                    frames.last_mut().unwrap().pc += 1;
                }
                OpCode::Jump(pc) => {
                    if let Some(frame) = frames.last_mut() {
                        frame.pc += pc;
                    }
                }
                OpCode::JumpIfFalse(pc) => {
                    if stack.is_empty() {
                        return Err(VMError::StackUnderflow);
                    }
                    if let Some(result) = stack.pop()
                        && let Some(frame) = frames.last_mut()
                    {
                        let is_false = match &*result.borrow() {
                            Value::Bool(r) => !r,
                            Value::Number(r) => *r == 0.0,
                            Value::String(r) => r.is_empty(),
                            _ => return Err(VMError::IncorrectType),
                        };
                        if is_false {
                            frame.pc += pc;
                        } else {
                            frame.pc += 1;
                        }
                    }
                }
                OpCode::StoreLocal(idx) => {
                    if let Some(frame) = frames.last_mut() {
                        frame.locals[idx] = stack.pop();
                        frame.pc += 1;
                    }
                }
                OpCode::LoadLocal(idx) => {
                    if let Some(frame) = frames.last_mut() {
                        if let Some(Some(val)) = frame.locals.get(idx) {
                            stack.push(Rc::clone(val));
                            frame.pc += 1;
                        } else {
                            return Err(VMError::UnknownVariable);
                        }
                    }
                }
                OpCode::StoreUpvalue(idx) => {
                    if let Some(frame) = frames.last_mut() {
                        frame.upvalues[idx] = stack.pop().unwrap();
                        frame.pc += 1;
                    }
                }
                OpCode::LoadUpvalue(idx) => {
                    if let Some(frame) = frames.last_mut() {
                        if let Some(val) = frame.upvalues.get(idx) {
                            stack.push(Rc::clone(val));
                            frame.pc += 1;
                        } else {
                            return Err(VMError::UnknownVariable);
                        }
                    }
                }
                OpCode::StoreGlobal(idx) => {
                    if let Some(frame) = frames.last_mut() {
                        self.globals[idx] = stack.pop();
                        frame.pc += 1;
                    }
                }
                OpCode::LoadGlobal(idx) => {
                    if let Some(frame) = frames.last_mut() {
                        if let Some(Some(val)) = self.globals.get(idx) {
                            stack.push(Rc::clone(val));
                            frame.pc += 1;
                        } else {
                            return Err(VMError::UnknownVariable);
                        }
                    }
                }
                OpCode::MakeClosure(chunk_idx, num_upvalues) => {
                    if let Some(frame) = frames.last_mut() {
                        let mut upvalues = vec![];
                        for _ in 0..num_upvalues {
                            upvalues.push(stack.pop().unwrap());
                        }
                        upvalues.reverse();
                        stack.push(Rc::new(Value::Closure(chunk_idx, upvalues).into()));
                        frame.pc += 1;
                    }
                }
                OpCode::Call(arity) => {
                    if let Some(v) = stack.pop() {
                        let borrowed = v.borrow();
                        match &*borrowed {
                            Value::Closure(chunk_idx, upvalues) => {
                                let chunk_idx = *chunk_idx;
                                let upvalues = upvalues.clone();
                                drop(borrowed);
                                self.current_chunk = chunk_idx;
                                let mut frame = self.new_frame();
                                frame.upvalues = upvalues;
                                for i in 0..arity {
                                    frame.locals[arity - i - 1] = stack.pop();
                                }
                                frames.last_mut().unwrap().pc += 1;
                                frames.push(frame);
                            }
                            Value::NativeFunction(f) => {
                                // Clone the Rc so we can drop the borrow before touching the stack
                                let f = f.clone();
                                drop(borrowed);
                                let mut args: Vec<Value> = (0..arity)
                                    .filter_map(|_| stack.pop())
                                    .map(|v| v.borrow().clone())
                                    .collect();
                                args.reverse();
                                let result = f(args);
                                stack.push(Rc::new(RefCell::new(result)));
                                frames.last_mut().unwrap().pc += 1;
                            }
                            _ => {
                                return Err(VMError::ExpectedFunction);
                            }
                        }
                    }
                }
                // TODO(human): implement OpCode::Eval
                //
                // At this point in the execute loop you have &mut self, so eval_str is safe to call.
                // Steps:
                //   1. Pop the top of the stack — it should be a Value::String(code)
                //   2. Save self.current_chunk (eval_str will overwrite it)
                //   3. Call self.eval_str(&code) — this compiles + runs the string, returns a Value
                //   4. Restore self.current_chunk to the saved value
                //   5. Push the result onto the stack (or Value::Bool(false) on None/error)
                //   6. Advance pc by 1
                OpCode::Eval => match stack.pop() {
                    Some(val) => {
                        if let Value::String(code) = &*(val.borrow()) {
                            let current_chunk = self.current_chunk;
                            match (self.eval_str(code)?, frames.last_mut()) {
                                (result, Some(frame)) => {
                                    self.current_chunk = current_chunk;
                                    stack.push(Rc::new(RefCell::new(
                                        result.unwrap_or(Value::Bool(false)),
                                    )));
                                    frame.pc += 1;
                                }
                                _ => {
                                    return Err(VMError::IncorrectType);
                                }
                            }
                        }
                    }
                    None => {
                        return Err(VMError::StackUnderflow);
                    }
                },
                OpCode::PushKeyword(idx) => {
                    let kw = self.chunks[self.current_chunk].strings[idx].clone();
                    stack.push(Rc::new(RefCell::new(Value::Keyword(kw))));
                    frames.last_mut().unwrap().pc += 1;
                }
                OpCode::PushSymbol(idx) => {
                    let sym = self.chunks[self.current_chunk].strings[idx].clone();
                    stack.push(Rc::new(RefCell::new(Value::Symbol(sym))));
                    frames.last_mut().unwrap().pc += 1;
                }
                OpCode::GetField(idx) => {
                    let key = self.chunks[self.current_chunk].strings[idx].clone();
                    match stack.pop() {
                        Some(val) => {
                            let result = match &*val.borrow() {
                                Value::Map(m) => m.get(&key).cloned()
                                    .unwrap_or_else(|| Rc::new(RefCell::new(Value::Bool(false)))),
                                _ => return Err(VMError::IncorrectType),
                            };
                            stack.push(result);
                            frames.last_mut().unwrap().pc += 1;
                        }
                        None => return Err(VMError::StackUnderflow),
                    }
                }
                OpCode::Return => match stack.pop() {
                    Some(return_value) => {
                        frames.pop();
                        if let Some(caller_frame) = frames.last() {
                            self.current_chunk = caller_frame.chunk_idx;
                            stack.push(return_value);
                        } else {
                            // Last frame returned — this is the final result
                            return Ok(Some(return_value.borrow().clone()));
                        }
                    }
                    None => return Err(VMError::StackUnderflow),
                },
                _ => {}
            }
        }

        if let Some(result) = stack.last() {
            return Ok(Some(result.borrow().clone()));
        }
        Ok(None)
    }
}
