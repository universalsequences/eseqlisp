use crate::compiler::{Chunk, OpCode};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Debug, PartialEq)]
pub enum VMError {
    UnknownConstant,
    StackUnderflow,
    IncorrectType,
    UnknownVariable,
    ExpectedFunction,
}

pub struct VM {
    chunks: Vec<Chunk>,
    current_chunk: usize,
    globals: Vec<Option<Rc<RefCell<Value>>>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(f64),
    Bool(bool),
    String(String),
    List(Vec<Rc<RefCell<Value>>>),
    Closure(usize, Vec<Rc<RefCell<Value>>>),
    Function(usize), // chunk idx
                     //
}

struct Frame {
    locals: Vec<Option<Rc<RefCell<Value>>>>,
    upvalues: Vec<Rc<RefCell<Value>>>,
    pc: usize,
    chunk_idx: usize, // chunk this frame executes on
}

impl VM {
    pub fn new(chunks: Vec<Chunk>) -> Self {
        VM {
            chunks,
            current_chunk: 0,
            // todo - compiler must pass number of globals
            globals: vec![None; 100],
        }
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
        while frames.last().unwrap().pc < self.chunk().ops.len() {
            if let Some(op) = self.chunk().ops.get(frames.last().unwrap().pc) {
                println!("PC={} op={:?}", frames.last().unwrap().pc, op);
                match op {
                    OpCode::PushConst(x) => {
                        if let Some(constant) = self.chunk().constants.get(*x) {
                            stack.push(Rc::new(RefCell::new(Value::Number(*constant))));
                            frames.last_mut().unwrap().pc += 1;
                        } else {
                            // error
                            println!("unknown constnats {}", x);
                            return Err(VMError::UnknownConstant);
                        }
                    }
                    OpCode::Add(arity) => {
                        if stack.len() < *arity {
                            println!("stack underflow arity={} len={}", arity, stack.len());
                            return Err(VMError::StackUnderflow);
                        }
                        let mut sum: f64 = 0.0;
                        for _ in 0..*arity {
                            if let Some(val) = stack.pop() {
                                println!("[sum] popped the following={:?}", val);
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
                        if stack.len() < *arity {
                            println!("stack underflow arity={} len={}", arity, stack.len());
                            return Err(VMError::StackUnderflow);
                        }
                        let mut nums: Vec<f64> = vec![];
                        for _ in 0..*arity {
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
                        if stack.len() < *arity {
                            println!("stack underflow arity={} len={}", arity, stack.len());
                            return Err(VMError::StackUnderflow);
                        }
                        let mut product: f64 = 1.0;
                        for _ in 0..*arity {
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
                        if let Some(frame) = frames.last_mut() {
                            if stack.len() < *arity {
                                return Err(VMError::StackUnderflow);
                            }
                            let mut list = vec![];
                            for _ in 0..*arity {
                                if let Some(val) = stack.pop() {
                                    list.push(val);
                                }
                            }
                            stack.push(Rc::new(RefCell::new(Value::List(list))));
                            frame.pc += 1;
                        }
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
                            frame.locals[*idx] = stack.pop();
                            println!("STORING local {:?}", frame.locals.get(*idx));
                            frame.pc += 1;
                        }
                    }
                    OpCode::LoadLocal(idx) => {
                        if let Some(frame) = frames.last_mut() {
                            if let Some(Some(val)) = frame.locals.get(*idx) {
                                println!("LoadLocal loading {:?}", val);
                                stack.push(Rc::clone(val));
                                frame.pc += 1;
                            } else {
                                return Err(VMError::UnknownVariable);
                            }
                        }
                    }
                    OpCode::StoreUpvalue(idx) => {
                        if let Some(frame) = frames.last_mut() {
                            frame.upvalues[*idx] = stack.pop().unwrap();
                            frame.pc += 1;
                        }
                    }
                    OpCode::LoadUpvalue(idx) => {
                        if let Some(frame) = frames.last_mut() {
                            if let Some(val) = frame.upvalues.get(*idx) {
                                println!("LoadUpvalue loading {:?}", val);
                                stack.push(Rc::clone(val));
                                frame.pc += 1;
                            } else {
                                return Err(VMError::UnknownVariable);
                            }
                        }
                    }

                    OpCode::StoreGlobal(idx) => {
                        if let Some(frame) = frames.last_mut() {
                            let idx = *idx;
                            self.globals[idx] = stack.pop();
                            frame.pc += 1;
                        }
                    }
                    OpCode::LoadGlobal(idx) => {
                        if let Some(frame) = frames.last_mut() {
                            if let Some(Some(val)) = self.globals.get(*idx) {
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
                            for _ in 0..*num_upvalues {
                                upvalues.push(stack.pop().unwrap());
                            }
                            upvalues.reverse();
                            stack.push(Rc::new(Value::Closure(*chunk_idx, upvalues).into()));
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
                                    let arity = *arity;
                                    self.current_chunk = chunk_idx;
                                    let mut frame = self.new_frame();
                                    frame.upvalues = upvalues.clone();
                                    for i in 0..arity {
                                        frame.locals[arity - i - 1] = stack.pop();
                                    }
                                    frames.last_mut().unwrap().pc += 1;
                                    frames.push(frame);
                                }
                                _ => {
                                    return Err(VMError::ExpectedFunction);
                                }
                            }
                        }
                    }
                    OpCode::Return => match stack.pop() {
                        Some(return_value) => {
                            frames.pop();
                            if let Some(caller_frame) = frames.last() {
                                self.current_chunk = caller_frame.chunk_idx;
                                stack.push(return_value);
                            } else {
                                return Err(VMError::StackUnderflow);
                            }
                        }
                        _ => return Err(VMError::StackUnderflow),
                    },
                    _ => {}
                }
            }
        }
        if let Some(result) = stack.iter().last() {
            let value: Value = result.borrow().clone();
            return Ok(Some(value));
        }
        Ok(None)
    }
}
