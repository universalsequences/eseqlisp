use crate::parser::Expression;

// represents all the chunks (functionsa nd global
#[derive(Debug)]
pub struct Chunk {
    pub ops: Vec<OpCode>,
    pub constants: Vec<f64>,
    pub symbols: Vec<String>,
}

#[derive(Debug)]
enum SymbolResolution {
    Global(usize),
    Upvalue(usize),
    Local(usize),
}

struct Scope {
    pub chunk_idx: usize,
    pub symbols: Vec<String>,
    pub upvalues: Vec<String>,
}

pub enum CompilerError {
    UnknownOperator,
    InvalidArg,
}

#[derive(Debug)]
pub enum OpCode {
    Push,
    PushConst(usize), // const idx
    Pop,
    Load(usize), // load from symbol idx
    LoadGlobal(usize),
    LoadLocal(usize),
    LoadUpvalue(usize),
    StoreGlobal(usize),
    StoreLocal(usize),
    StoreUpvalue(usize),
    Store(usize), // store into symbol idx
    Add(usize),   // aritiy
    Mul(usize),   // aritiy
    Sub(usize),   // aritiy
    Div(usize),   // aritiy
    Eq,           // aritiy
    Lt,           // aritiy
    Gt,           // aritiy
    Lte,          // aritiy
    Gte,          // aritiy
    MakeList(usize),
    Call(usize),               // arity
    MakeFunc(usize),           // defines function with fn chunk idx
    MakeClosure(usize, usize), // chunk_idx + upvalues_len
    Return,
    Jump(usize),
    JumpIfFalse(usize),
}

pub struct Compiler {
    expressions: Vec<Expression>,
    chunks: Vec<Chunk>,
    scopes: Vec<Scope>,
    current_chunk: usize,
    global_symbols: Vec<String>,
}

fn extract_function_definition(
    list: &[Expression],
) -> Option<(Option<String>, Vec<Expression>, Vec<Expression>)> {
    match (list.first(), list.get(1), list.get(2), list.get(3)) {
        (
            Some(Expression::Symbol(s)),
            Some(Expression::Symbol(name)),
            Some(Expression::List(args)),
            Some(Expression::List(body)),
        ) if s == "def" => Some((Some(name.to_string()), args.clone(), body.clone())),
        (
            Some(Expression::Symbol(s)),
            Some(Expression::List(args)),
            Some(Expression::List(body)),
            _,
        ) if s == "lambda" => Some((None, args.clone(), body.clone())),
        _ => None,
    }
}

fn extract_if_statement(list: &[Expression]) -> Option<(Expression, Expression, Expression)> {
    match (list.first(), list.get(1), list.get(2), list.get(3)) {
        (Some(Expression::Symbol(s)), Some(condition), Some(then_body), Some(else_body))
            if s == "if" =>
        {
            Some((condition.clone(), then_body.clone(), else_body.clone()))
        }
        _ => None,
    }
}

impl Compiler {
    pub fn new(expressions: Vec<Expression>) -> Self {
        Compiler {
            expressions,
            chunks: vec![],
            scopes: vec![],
            current_chunk: 0,
            global_symbols: vec![],
        }
    }

    fn chunk(&self) -> Option<&Chunk> {
        self.chunks.get(self.current_chunk)
    }

    fn chunk_mut(&mut self) -> Option<&mut Chunk> {
        self.chunks.get_mut(self.current_chunk)
    }

    fn use_constant(&mut self, num: f64) -> usize {
        let chunk = self.chunk_mut().unwrap();
        if let Some(index) = chunk.constants.iter().position(|r| *r == num) {
            return index;
        }
        let idx = chunk.constants.len();
        chunk.constants.push(num);
        idx
    }

    fn use_symbol(&mut self, symbol: String) -> usize {
        let chunk = self.chunk_mut().unwrap();
        if let Some(index) = chunk.symbols.iter().position(|r| *r == symbol) {
            return index;
        }
        let idx = chunk.symbols.len();
        chunk.symbols.push(symbol);
        idx
    }

    fn get_scope_mut(&mut self) -> &mut Scope {
        self.scopes.last_mut().unwrap()
    }

    fn resolve_symbol(&mut self, name: &str) -> SymbolResolution {
        if let Some(idx) = self.get_scope_mut().symbols.iter().position(|s| *s == name) {
            return SymbolResolution::Local(idx);
        }

        if let Some(parent) = self.scopes.iter().rev().nth(1)
            && let Some(_) = parent
                .symbols
                .iter()
                .position(|s| *s == name)
                .or_else(|| parent.upvalues.iter().position(|s| *s == name))
        {
            let current = self.get_scope_mut();
            let upvalues_idx = current.upvalues.len();
            current.upvalues.push(name.to_string());
            return SymbolResolution::Upvalue(upvalues_idx);
        }

        // global
        let idx = self.use_global(name);
        SymbolResolution::Global(idx)
    }

    fn use_global(&mut self, name: &str) -> usize {
        if let Some(index) = self.global_symbols.iter().position(|r| *r == name) {
            return index;
        }
        let idx = self.global_symbols.len();
        self.global_symbols.push(name.to_string());
        idx
    }

    pub fn new_chunk(&mut self, chunk: Chunk) -> (usize, usize) {
        let symbols = chunk.symbols.clone();
        let prev_chunk_idx = self.current_chunk;
        let new_chunk_idx = self.chunks.len();
        self.chunks.push(chunk);
        self.current_chunk = new_chunk_idx;

        self.scopes.push(Scope {
            chunk_idx: new_chunk_idx,
            symbols,
            upvalues: vec![],
        });

        (new_chunk_idx, prev_chunk_idx)
    }

    fn emit(&mut self, op: OpCode) {
        self.chunk_mut().unwrap().ops.push(op);
    }

    fn emit_symbol_load(&mut self, name: &str) {
        match self.resolve_symbol(name) {
            SymbolResolution::Local(idx) => self.emit(OpCode::LoadLocal(idx)),
            SymbolResolution::Global(idx) => self.emit(OpCode::LoadGlobal(idx)),
            SymbolResolution::Upvalue(idx) => self.emit(OpCode::LoadUpvalue(idx)),
        }
    }

    fn emit_symbol_store(&mut self, name: &str) {
        match self.resolve_symbol(name) {
            SymbolResolution::Local(idx) => self.emit(OpCode::StoreLocal(idx)),
            SymbolResolution::Global(idx) => self.emit(OpCode::StoreGlobal(idx)),
            SymbolResolution::Upvalue(idx) => self.emit(OpCode::StoreUpvalue(idx)),
        }
    }

    pub fn compile_function(
        &mut self,
        name: Option<String>,
        args: Vec<Expression>,
        body: Vec<Expression>,
    ) -> Result<(), CompilerError> {
        let symbols: Vec<String> = args
            .iter()
            .map(|arg| {
                if let Expression::Symbol(s) = arg {
                    Ok(s.to_string())
                } else {
                    Err(CompilerError::InvalidArg)
                }
            })
            .collect::<Result<_, _>>()?;
        let (new_chunk_idx, previous_chunk_idx) = self.new_chunk(Chunk {
            ops: vec![],
            constants: vec![],
            symbols,
        });
        self.compile_list(&body)?;

        // finished compiling body, pop the scope to collect upvalues to pass to closure
        let scope = self.scopes.pop().unwrap();

        self.emit(OpCode::Return);

        // bring back original chunk
        self.current_chunk = previous_chunk_idx;

        // the magic of closures -> loop thru upvalues generated in function definition (at
        // compile time) and "load them" so that they get placed on the VM stack
        for upvalue_name in &scope.upvalues {
            let resolved = self.resolve_symbol(upvalue_name);
            match resolved {
                SymbolResolution::Local(idx) => self.emit(OpCode::LoadLocal(idx)),
                SymbolResolution::Upvalue(idx) => self.emit(OpCode::LoadUpvalue(idx)),
                _ => {}
            }
        }
        self.emit(OpCode::MakeClosure(new_chunk_idx, scope.upvalues.len()));
        if let Some(name) = name {
            // is named function
            self.emit_symbol_store(&name);
        }

        Ok(())
    }

    pub fn op_idx(&self) -> usize {
        self.chunk().unwrap().ops.len()
    }

    pub fn compile_if_statement(
        &mut self,
        condition: Expression,
        then_body: Expression,
        else_body: Expression,
    ) -> Result<(), CompilerError> {
        self.compile_expression(&condition)?;
        let jump_op_idx = self.op_idx();
        self.emit(OpCode::JumpIfFalse(0));
        self.compile_expression(&then_body)?;
        let then_end_idx = self.op_idx();
        self.emit(OpCode::Jump(0));
        let else_begin_idx = self.op_idx();
        let jump_false_increment = else_begin_idx - jump_op_idx;
        self.chunk_mut().unwrap().ops[jump_op_idx] = OpCode::JumpIfFalse(jump_false_increment);
        self.compile_expression(&else_body)?;
        let else_end_idx = self.op_idx();
        self.chunk_mut().unwrap().ops[then_end_idx] = OpCode::Jump(else_end_idx - then_end_idx);
        Ok(())
    }

    pub fn compile_list(&mut self, list: &[Expression]) -> Result<(), CompilerError> {
        if let Some((name, args, body)) = extract_function_definition(list) {
            return self.compile_function(name, args, body);
        }

        if let Some((cond, then_body, else_body)) = extract_if_statement(list) {
            return self.compile_if_statement(cond, then_body, else_body);
        }

        let op = list.first();

        if let Some(op) = op {
            for (i, elem) in list.iter().skip(1).enumerate() {
                match elem {
                    Expression::Number(c) => {
                        let constant_idx = self.use_constant(*c);
                        self.emit(OpCode::PushConst(constant_idx));
                    }
                    Expression::Symbol(c) => match op {
                        Expression::Symbol(s) if s == "def" && i == 0 => continue,
                        _ => {
                            self.emit_symbol_load(c);
                        }
                    },
                    Expression::List(l) => {
                        // (def sq (x) (* x x)) -> in this case we need to define a function
                        self.compile_list(l)?;
                    }
                    _ => {}
                }
            }
            let arity = list.len() - 1;
            match op {
                Expression::Symbol(s) if s == "+" => self.emit(OpCode::Add(arity)),
                Expression::Symbol(s) if s == "*" => self.emit(OpCode::Mul(arity)),
                Expression::Symbol(s) if s == "-" => self.emit(OpCode::Sub(arity)),
                Expression::Symbol(s) if s == "/" => self.emit(OpCode::Div(arity)),
                Expression::Symbol(s) if s == "list" => self.emit(OpCode::MakeList(arity)),
                Expression::Symbol(s) if s == "=" => self.emit(OpCode::Eq),
                Expression::Symbol(s) if s == "<" => self.emit(OpCode::Lt),
                Expression::Symbol(s) if s == ">" => self.emit(OpCode::Gt),
                Expression::Symbol(s) if s == "<=" => self.emit(OpCode::Lte),
                Expression::Symbol(s) if s == ">=" => self.emit(OpCode::Gte),
                Expression::Symbol(s) if s == "def" => {
                    if let Some(Expression::Symbol(s)) = list.get(1) {
                        // builtin def eg: (def x 5)
                        self.emit_symbol_store(s);
                    }
                }
                _ => {
                    // TODO - add other builtins before doing CALL
                    // Handle function call
                    self.compile_expression(op)?;
                    self.emit(OpCode::Call(arity));
                }
            }
        }
        Ok(())
    }

    fn compile_expression(&mut self, expression: &Expression) -> Result<(), CompilerError> {
        match expression {
            Expression::List(l) => {
                self.compile_list(l)?;
            }
            Expression::Symbol(s) => {
                self.emit_symbol_load(s);
            }
            Expression::Number(n) => {
                let constant_idx = self.use_constant(*n);
                self.chunk_mut()
                    .unwrap()
                    .ops
                    .push(OpCode::PushConst(constant_idx));
            }
            _ => {}
        }

        Ok(())
    }

    pub fn compile(&mut self) -> Result<Vec<Chunk>, CompilerError> {
        _ = self.new_chunk(Chunk {
            ops: vec![],
            constants: vec![],
            symbols: vec![],
        });
        let expressions = std::mem::take(&mut self.expressions);
        for expression in &expressions {
            self.compile_expression(expression)?;
        }
        Ok(std::mem::take(&mut self.chunks))
    }
}
