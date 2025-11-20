//! AST to IR lowering for the Atom compiler.
//!
//! This module translates the typed AST into the intermediate representation (IR)
//! that can be compiled to machine code. The lowering process:
//!
//! - Converts high-level constructs (match, loops, closures) to basic blocks
//! - Transforms expressions into SSA form with explicit value flow
//! - Allocates local variables and tracks their lifetimes
//! - Handles control flow with explicit jumps and branches
//! - Converts Atom types to IR types
//!
//! # Design
//!
//! The lowering process is designed to be straightforward and explicit:
//! - Each expression produces a ValueId in SSA form
//! - Control flow is explicit via basic blocks and terminators
//! - Local variables are allocated on the stack
//! - Match expressions become switch/branch instructions
//! - Closures capture their environment explicitly
//!
//! # Usage
//!
//! ```ignore
//! use atom_backend::{Lower, TypeEnvironment};
//! use atom_ast::TopLevel;
//!
//! let type_env = TypeEnvironment::with_stdlib();
//! let mut lower = Lower::new(type_env);
//! let ir_program = lower.lower_program(ast_nodes)?;
//! ```

use crate::ir::*;
use crate::types::{TypeEnvironment, TypeError};
use atom_ast::{self, Visibility};
use std::collections::HashMap;

/// Error during lowering from AST to IR.
#[derive(Debug, Clone, PartialEq)]
pub enum LowerError {
    /// Type error during lowering
    TypeError(TypeError),
    /// Undefined function
    UndefinedFunction(String),
    /// Undefined variable
    UndefinedVariable(String),
    /// Undefined struct
    UndefinedStruct(String),
    /// Undefined enum
    UndefinedEnum(String),
    /// Invalid pattern in match
    InvalidPattern(String),
    /// Unsupported language feature
    Unsupported(String),
    /// Internal compiler error
    Internal(String),
}

impl From<TypeError> for LowerError {
    fn from(err: TypeError) -> Self {
        LowerError::TypeError(err)
    }
}

impl std::fmt::Display for LowerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LowerError::TypeError(err) => write!(f, "Type error: {}", err),
            LowerError::UndefinedFunction(name) => write!(f, "Undefined function: {}", name),
            LowerError::UndefinedVariable(name) => write!(f, "Undefined variable: {}", name),
            LowerError::UndefinedStruct(name) => write!(f, "Undefined struct: {}", name),
            LowerError::UndefinedEnum(name) => write!(f, "Undefined enum: {}", name),
            LowerError::InvalidPattern(msg) => write!(f, "Invalid pattern: {}", msg),
            LowerError::Unsupported(msg) => write!(f, "Unsupported feature: {}", msg),
            LowerError::Internal(msg) => write!(f, "Internal error: {}", msg),
        }
    }
}

impl std::error::Error for LowerError {}

/// Result type for lowering operations.
pub type LowerResult<T> = Result<T, LowerError>;

/// The main AST to IR lowering context.
///
/// This struct maintains the state needed during lowering, including:
/// - The type environment for resolving user-defined types
/// - Function signatures for handling default parameters
/// - Value ID counter for SSA form
/// - Block ID counter for basic blocks
/// - Current function being lowered
pub struct Lower {
    /// Type environment for type resolution
    type_env: TypeEnvironment,
    /// Function signatures for all functions (for default parameters)
    function_sigs: HashMap<String, Vec<crate::typechecker::FunctionSignature>>,
    /// Function definitions by name (for accessing default parameter values)
    function_defs: HashMap<String, Vec<atom_ast::FunctionDef>>,
    /// Next value ID to allocate
    next_value_id: u32,
    /// Next block ID to allocate
    next_block_id: u32,
    /// Next local ID to allocate
    next_local_id: u32,
    /// Current variable bindings (name -> (type, value_id or local_id))
    variables: HashMap<String, VarBinding>,
    /// Current function parameters (name -> value_id)
    params: HashMap<String, ValueId>,
    /// Generated closure functions to be added to the program
    closure_functions: Vec<IrFunction>,
    /// Counter for generating unique closure names
    next_closure_id: u32,
    /// Monomorphized function instances to generate
    /// Maps: monomorphized_name -> (original_name, type_param_bindings, AST function def)
    mono_queue: HashMap<String, (String, HashMap<String, IrType>, atom_ast::FunctionDef)>,
    /// Already monomorphized instances (to avoid duplicates)
    mono_done: std::collections::HashSet<String>,
}

/// Variable binding - either a value (SSA) or a mutable local variable.
#[derive(Debug, Clone)]
enum VarBinding {
    /// Immutable value in SSA form
    Value(ValueId, #[allow(dead_code)] IrType),
    /// Mutable local variable (stack allocated)
    #[allow(dead_code)]
    Local(LocalId, IrType),
}

impl Lower {
    /// Create a new lowering context with the given type environment.
    pub fn new(type_env: TypeEnvironment) -> Self {
        Self {
            type_env,
            function_sigs: HashMap::new(),
            function_defs: HashMap::new(),
            next_value_id: 0,
            next_block_id: 0,
            next_local_id: 0,
            variables: HashMap::new(),
            params: HashMap::new(),
            closure_functions: Vec::new(),
            next_closure_id: 0,
            mono_queue: HashMap::new(),
            mono_done: std::collections::HashSet::new(),
        }
    }
    
    /// Create a new lowering context with type environment and function signatures.
    pub fn new_with_sigs(
        type_env: TypeEnvironment,
        function_sigs: HashMap<String, Vec<crate::typechecker::FunctionSignature>>,
    ) -> Self {
        Self {
            type_env,
            function_sigs,
            function_defs: HashMap::new(),
            next_value_id: 0,
            next_block_id: 0,
            next_local_id: 0,
            variables: HashMap::new(),
            params: HashMap::new(),
            closure_functions: Vec::new(),
            next_closure_id: 0,
            mono_queue: HashMap::new(),
            mono_done: std::collections::HashSet::new(),
        }
    }

    /// Lower a complete AST program to IR.
    pub fn lower_program(&mut self, ast: Vec<atom_ast::TopLevel>) -> LowerResult<IrProgram> {
        eprintln!("[MONO-DEBUG] lower_program called with {} items", ast.len());
        let mut program = IrProgram::new();

        // First pass: collect all function definitions (for default parameters)
        for item in &ast {
            if let atom_ast::TopLevel::Function(func_def) = item {
                let name = func_def.name.name.clone();
                self.function_defs.entry(name).or_insert_with(Vec::new).push(func_def.clone());
            }
        }

        // Second pass: collect all type definitions
        for item in &ast {
            match item {
                atom_ast::TopLevel::Struct(def) => {
                    let ir_struct = self.lower_struct_def(def)?;
                    program.add_struct(ir_struct);
                }
                atom_ast::TopLevel::Enum(def) => {
                    let ir_enum = self.lower_enum_def(def)?;
                    program.add_enum(ir_enum);
                }
                _ => {}
            }
        }

        // Third pass: lower global variables
        for item in &ast {
            if let atom_ast::TopLevel::Variable(decl) = item {
                let ir_global = self.lower_global_var(decl)?;
                program.add_global(ir_global);
            }
        }

        // Fourth pass: lower functions (skip generic functions - they'll be monomorphized on demand)
        for item in &ast {
            if let atom_ast::TopLevel::Function(func_def) = item {
                eprintln!("[MONO-DEBUG] Checking function: {}, const_params: {}, params: {}", 
                    func_def.name.name, func_def.const_params.len(), func_def.params.len());
                for (i, param) in func_def.params.iter().enumerate() {
                    eprintln!("[MONO-DEBUG]   param[{}]: name={}, type={:?}", 
                        i, param.name.name, param.ty);
                }
                
                // Skip generic functions (those with type parameters)
                if self.is_generic_function(func_def) {
                    eprintln!("[MONO] Skipping generic function: {}", func_def.name.name);
                    continue;
                }
                let ir_func = self.lower_function(func_def)?;
                program.add_function(ir_func);
            }
        }

        // Fifth pass: add any closure functions that were generated
        for closure_func in self.closure_functions.drain(..) {
            program.add_function(closure_func);
        }

        // Sixth pass: process monomorphization queue
        // Keep processing until queue is empty (new instantiations may trigger more)
        let mut iterations = 0;
        const MAX_MONO_ITERATIONS: usize = 100; // Prevent infinite loops
        
        while !self.mono_queue.is_empty() && iterations < MAX_MONO_ITERATIONS {
            iterations += 1;
            eprintln!("[MONO] Processing monomorphization queue (iteration {})", iterations);
            
            // Drain the current queue (clone to avoid borrow issues)
            let current_queue: Vec<_> = self.mono_queue.drain().collect();
            
            for (mono_name, (original_name, type_bindings, func_def)) in current_queue {
                eprintln!("[MONO] Generating monomorphized instance: {} <- {}", mono_name, original_name);
                eprintln!("[MONO]   Type bindings: {:?}", type_bindings);
                
                // Create a modified function definition with the monomorphized name
                let mut specialized_func = func_def.clone();
                specialized_func.name.name = mono_name.clone();
                
                // TODO: Substitute type parameters in the function body
                // For now, we rely on the type erasure approach where type params
                // are already converted to Pointer(Int(8)) during lowering
                
                // Lower the specialized function
                match self.lower_function(&specialized_func) {
                    Ok(ir_func) => {
                        eprintln!("[MONO] Successfully generated: {}", mono_name);
                        program.add_function(ir_func);
                    }
                    Err(e) => {
                        eprintln!("[MONO] Error generating {}: {:?}", mono_name, e);
                        return Err(e);
                    }
                }
            }
        }
        
        if iterations >= MAX_MONO_ITERATIONS {
            return Err(LowerError::Internal(
                "Monomorphization exceeded maximum iterations - possible infinite recursion".to_string()
            ));
        }
        
        eprintln!("[MONO] Monomorphization complete after {} iterations", iterations);

        Ok(program)
    }

    // ========================================================================
    // Type Definition Lowering
    // ========================================================================

    /// Lower a struct definition to IR.
    fn lower_struct_def(&mut self, def: &atom_ast::StructDef) -> LowerResult<IrStructDef> {
        let mut fields = Vec::new();

        for field in &def.fields {
            let field_name = field
                .name
                .as_ref()
                .ok_or_else(|| LowerError::Unsupported("Tuple-like struct fields".to_string()))?
                .name
                .clone();
            let field_type = self.lower_type(&field.ty)?;
            fields.push((field_name, field_type));
        }

        Ok(IrStructDef {
            name: def.name.name.clone(),
            fields,
        })
    }

    /// Lower an enum definition to IR.
    fn lower_enum_def(&mut self, def: &atom_ast::EnumDef) -> LowerResult<IrEnumDef> {
        let mut variants = Vec::new();

        for case in &def.cases {
            let mut field_types = Vec::new();
            for field_ty in &case.fields {
                field_types.push(self.lower_type(field_ty)?);
            }
            variants.push((case.name.name.clone(), field_types));
        }

        Ok(IrEnumDef {
            name: def.name.name.clone(),
            variants,
        })
    }

    /// Lower a global variable declaration to IR.
    fn lower_global_var(&mut self, decl: &atom_ast::VarDecl) -> LowerResult<IrGlobal> {
        if decl.names.len() != 1 {
            return Err(LowerError::Unsupported(
                "Tuple destructuring in global variables".to_string(),
            ));
        }

        let name = decl.names[0].name.clone();
        let ty = if let Some(ty_ast) = &decl.ty {
            self.lower_type(ty_ast)?
        } else {
            return Err(LowerError::Internal(
                "Global variable must have explicit type".to_string(),
            ));
        };

        // TODO: Evaluate constant initialization expressions
        let init = None;

        let is_public = matches!(decl.visibility, Visibility::Public);

        Ok(IrGlobal {
            name,
            ty,
            init,
            is_public,
        })
    }

    // ========================================================================
    // Function Lowering
    // ========================================================================

    /// Lower a function definition to IR.
    fn lower_function(&mut self, func_def: &atom_ast::FunctionDef) -> LowerResult<IrFunction> {
        // Reset per-function state
        self.next_value_id = 0;
        self.next_block_id = 0;
        self.next_local_id = 0;
        self.variables.clear();
        self.params.clear();

        let name = func_def.name.name.clone();
        let is_public = matches!(func_def.visibility, Visibility::Public);
        let is_main = name == "main";

        // Lower parameters
        let mut params = Vec::new();
        for (i, param) in func_def.params.iter().enumerate() {
            let param_name = param.name.name.clone();
            let param_type = if let Some(ty) = &param.ty {
                self.lower_type(ty)?
            } else {
                return Err(LowerError::Internal(
                    "Function parameter must have type".to_string(),
                ));
            };

            params.push((param_name.clone(), param_type.clone()));

            // Parameters are represented as value IDs
            let value_id = ValueId(i as u32);
            self.next_value_id = (i + 1) as u32;
            self.params.insert(param_name.clone(), value_id);
            self.variables
                .insert(param_name, VarBinding::Value(value_id, param_type));
        }

        // Lower return type
        let return_type = if let Some(ret_ty) = &func_def.return_type {
            Some(self.lower_type(ret_ty)?)
        } else {
            // Special case: main() with no return type returns Int
            if is_main {
                Some(IrType::Int(64))
            } else {
                None
            }
        };

        // Debug output for take function (before moving name/params)
        let is_take = name == "take";
        
        // Create function and lower body
        let mut ir_func = IrFunction::new(name, params, return_type.clone(), is_public);

        // Lower function body
        let entry_block_id = self.fresh_block_id();
        let mut entry_block = IrBlock::new(entry_block_id);

        let (result_value, terminator) =
            self.lower_block_to_ir(&func_def.body, &mut entry_block, &mut ir_func)?;

        // Set terminator
        if matches!(terminator, IrTerminator::Unreachable) {
            // Block ended with an expression - add return
            if let Some(value) = result_value {
                entry_block.set_terminator(IrTerminator::Return { value: Some(value) });
            } else if return_type.is_none() || return_type.as_ref().unwrap().is_void() {
                // For main function with no return type, automatically return 0
                if is_main && return_type.is_some() {
                    let zero_value = self.fresh_value_id();
                    entry_block.add_instruction(IrInstruction {
                        result: zero_value,
                        ty: IrType::Int(64),
                        kind: IrInstructionKind::Const {
                            value: IrConstant::Int(0),
                        },
                    });
                    entry_block.set_terminator(IrTerminator::Return { value: Some(zero_value) });
                } else {
                    entry_block.set_terminator(IrTerminator::Return { value: None });
                }
            } else {
                return Err(LowerError::Internal(
                    "Function must return a value".to_string(),
                ));
            }
        } else {
            entry_block.set_terminator(terminator);
        }

        ir_func.add_block(entry_block);

        eprintln!("[DEBUG-TEST] Lowered function: {}", ir_func.name);
        eprintln!("[DEBUG-TEST] About to check debug for function: {}", ir_func.name);
        
        // Debug: Print complete IR structure for functions we're interested in
        if let Some(debug) = std::env::var("ATOM_DEBUG").ok() {
            if debug == "1" {
                eprintln!("[DEBUG] Lowered function '{}' successfully", ir_func.name);
                if ir_func.name == "main" || ir_func.name == "print" {
                    eprintln!("[DEBUG] Complete IR for function '{}':", ir_func.name);
                    eprintln!("[DEBUG]   Params: {:?}", ir_func.params);
                    eprintln!("[DEBUG]   Return type: {:?}", ir_func.return_type);
                    eprintln!("[DEBUG]   Locals: {:?}", ir_func.locals);
                    for block in &ir_func.blocks {
                        eprintln!("[DEBUG]   Block {}:", block.label);
                        for (idx, inst) in block.instructions.iter().enumerate() {
                            eprintln!("[DEBUG]     [{}] {:?}", idx, inst);
                        }
                        eprintln!("[DEBUG]     Terminator: {:?}", block.terminator);
                    }
                }
            }
        }

        Ok(ir_func)
    }

    /// Lower a block and accumulate instructions into the given IR block.
    ///
    /// Returns (optional result value, terminator).
    fn lower_block_to_ir(
        &mut self,
        block: &atom_ast::Block,
        ir_block: &mut IrBlock,
        func: &mut IrFunction,
    ) -> LowerResult<(Option<ValueId>, IrTerminator)> {
        let mut last_value = None;

        for (i, stmt) in block.stmts.iter().enumerate() {
            match stmt {
                atom_ast::Stmt::VarDecl(decl) => {
                    self.lower_var_decl(decl, ir_block, func)?;
                }
                atom_ast::Stmt::Expression(expr) => {
                    let value = self.lower_expr(expr, ir_block, func)?;
                    
                    // Check if this is the last statement
                    if i == block.stmts.len() - 1 {
                        last_value = Some(value);
                    }
                }
            }
        }

        // Block doesn't specify its own terminator - let caller decide
        Ok((last_value, IrTerminator::Unreachable))
    }

    /// Lower a variable declaration.
    fn lower_var_decl(
        &mut self,
        decl: &atom_ast::VarDecl,
        ir_block: &mut IrBlock,
        func: &mut IrFunction,
    ) -> LowerResult<()> {
        let init_value = if let Some(init_expr) = &decl.init {
            self.lower_expr(init_expr, ir_block, func)?
        } else {
            // No initializer - create a zero/default value
            // This shouldn't happen in well-typed code, but handle it gracefully
            let value_id = self.fresh_value_id();
            ir_block.add_instruction(IrInstruction {
                result: value_id,
                ty: IrType::Int(64),
                kind: IrInstructionKind::Const {
                    value: IrConstant::Int(0),
                },
            });
            value_id
        };

        // Handle tuple destructuring if multiple names
        if decl.names.len() > 1 {
            // Tuple destructuring: extract each element and bind to corresponding name
            for (index, var_name_ident) in decl.names.iter().enumerate() {
                let element_id = self.fresh_value_id();
                ir_block.add_instruction(IrInstruction {
                    result: element_id,
                    ty: IrType::Pointer(Box::new(IrType::Void)), // Simplified type
                    kind: IrInstructionKind::TupleExtract {
                        tuple: init_value,
                        index: index as u32,
                    },
                });
                self.variables.insert(
                    var_name_ident.name.clone(),
                    VarBinding::Value(element_id, IrType::Pointer(Box::new(IrType::Void))),
                );
            }
        } else {
            // Single variable binding
            let var_name = decl.names[0].name.clone();
            
            // Infer the type from the initializer if no type annotation
            let var_type = if let Some(ty_ast) = &decl.ty {
                self.lower_type(ty_ast)?
            } else {
                // Try to infer type from the initializer by finding the instruction
                // that produced init_value
                let mut inferred_type = None;
                for inst in &ir_block.instructions {
                    if inst.result == init_value {
                        inferred_type = Some(inst.ty.clone());
                        break;
                    }
                }
                // If we can't find the type (e.g., for parameters), default to pointer
                let ty = inferred_type.unwrap_or(IrType::Pointer(Box::new(IrType::Void)));
                if std::env::var("ATOM_DEBUG_VERIFY").is_ok() {
                    eprintln!("DEBUG lower_var_decl: var={}, init_value={}, inferred_type={:?}", var_name, init_value, ty);
                }
                ty
            };

            // Check if this is a mutable variable (declared with :=)
            if !decl.is_const {
                // Mutable variable - allocate a local (stack slot) and store the initial value
                let local_id = self.fresh_local_id();
                func.locals.push(IrLocal {
                    id: local_id,
                    name: var_name.clone(),
                    ty: var_type.clone(),
                });

                // Store the initial value to the stack slot
                ir_block.add_instruction(IrInstruction {
                    result: self.fresh_value_id(), // Store produces a dummy value
                    ty: IrType::Void,
                    kind: IrInstructionKind::Store {
                        destination: IrMemoryLocation::Local(local_id),
                        value: init_value,
                    },
                });

                // Bind the variable to the local
                self.variables
                    .insert(var_name, VarBinding::Local(local_id, var_type));
            } else {
                // Immutable variable - bind directly to the SSA value
                self.variables
                    .insert(var_name, VarBinding::Value(init_value, var_type));
            }
        }

        Ok(())
    }

    // ========================================================================
    // Expression Lowering
    // ========================================================================

    /// Lower an expression to IR, producing a value ID.
    fn lower_expr(
        &mut self,
        expr: &atom_ast::Expr,
        ir_block: &mut IrBlock,
        func: &mut IrFunction,
    ) -> LowerResult<ValueId> {
        match expr {
            atom_ast::Expr::Literal(lit, _) => self.lower_literal(lit, ir_block),
            atom_ast::Expr::Ident(ident) => self.lower_ident(&ident.name, ir_block, func),
            atom_ast::Expr::Binary { op, left, right, .. } => {
                self.lower_binary_op(op, left, right, ir_block, func)
            }
            atom_ast::Expr::Unary { op, expr, .. } => {
                self.lower_unary_op(op, expr, ir_block, func)
            }
            atom_ast::Expr::Call { func: func_expr, args, .. } => {
                self.lower_call(func_expr, args, ir_block, func)
            }
            atom_ast::Expr::Tuple(elements, _) => {
                self.lower_tuple(elements, ir_block, func)
            }
            atom_ast::Expr::StructInit { ty, fields, .. } => {
                self.lower_struct_init(ty, fields, ir_block, func)
            }
            atom_ast::Expr::FieldAccess { object, field, .. } => {
                self.lower_field_access(object, &field.name, ir_block, func)
            }
            atom_ast::Expr::Block(block) => {
                // Create a new nested block
                let (value, _) = self.lower_block_to_ir(block, ir_block, func)?;
                value.ok_or_else(|| LowerError::Internal("Block must produce a value".to_string()))
            }
            atom_ast::Expr::Match { expr: match_expr, arms, .. } => {
                self.lower_match(match_expr, arms, ir_block, func)
            }
            atom_ast::Expr::Closure { params, return_type, body, .. } => {
                self.lower_closure(params, return_type, body, ir_block, func)
            }
            atom_ast::Expr::MethodCall { receiver, method, args, .. } => {
                self.lower_method_call(receiver, method, args, ir_block, func)
            }
            atom_ast::Expr::Comptime { expr, .. } => {
                // Comptime expressions are evaluated at compile time in the full implementation.
                // For now, lower the inner expression normally as a placeholder.
                self.lower_expr(expr, ir_block, func)
            }
        }
    }

    /// Lower a literal to IR.
    fn lower_literal(
        &mut self,
        lit: &atom_ast::Literal,
        ir_block: &mut IrBlock,
    ) -> LowerResult<ValueId> {
        let (constant, ty) = match lit {
            atom_ast::Literal::Integer(n) => (IrConstant::Int(*n), IrType::Int(64)),
            atom_ast::Literal::Float(f) => (IrConstant::Float(*f), IrType::Float(64)),
            atom_ast::Literal::String(s) => {
                (IrConstant::String(s.as_bytes().to_vec()), IrType::Pointer(Box::new(IrType::Int(8))))
            }
            atom_ast::Literal::Rune(c) => (IrConstant::Rune(*c), IrType::Rune),
            atom_ast::Literal::Bool(b) => (IrConstant::Bool(*b), IrType::Bool),
        };

        let value_id = self.fresh_value_id();
        ir_block.add_instruction(IrInstruction {
            result: value_id,
            ty,
            kind: IrInstructionKind::Const { value: constant },
        });

        Ok(value_id)
    }

    /// Lower an identifier reference to IR.
    fn lower_ident(
        &mut self,
        name: &str,
        ir_block: &mut IrBlock,
        _func: &mut IrFunction,
    ) -> LowerResult<ValueId> {
        // Clone the binding to avoid borrow checker issues
        let binding = self.variables.get(name).cloned();
        
        match binding {
            Some(VarBinding::Value(value_id, _)) => {
                Ok(value_id)
            }
            Some(VarBinding::Local(local_id, ty)) => {
                // Need to load from local
                let value_id = self.fresh_value_id();
                ir_block.add_instruction(IrInstruction {
                    result: value_id,
                    ty,
                    kind: IrInstructionKind::Load {
                        source: IrMemoryLocation::Local(local_id),
                    },
                });
                Ok(value_id)
            }
            None => {
                // Special handling for break
                if name == "break" {
                    // For now, treat break as a unit value (void)
                    // In a real implementation, this would generate a branch to the loop exit
                    let value_id = self.fresh_value_id();
                    ir_block.add_instruction(IrInstruction {
                        result: value_id,
                        ty: IrType::Pointer(Box::new(IrType::Void)),
                        kind: IrInstructionKind::Const {
                            value: IrConstant::Int(0),
                        },
                    });
                    return Ok(value_id);
                }

                // Special handling for $0 (loop iteration variable)
                if name == "$0" {
                    // Create a dummy value for $0
                    // In a real implementation, this would be bound by the loop construct
                    let value_id = self.fresh_value_id();
                    ir_block.add_instruction(IrInstruction {
                        result: value_id,
                        ty: IrType::Int(64),
                        kind: IrInstructionKind::Const {
                            value: IrConstant::Int(0),
                        },
                    });
                    return Ok(value_id);
                }
                
                // Check if this is an enum case constructor
                if let Some((enum_name, _case, idx)) = self.type_env.find_enum_case(name) {
                    // Clone the data we need before the mutable borrow
                    let enum_name_cloned = enum_name.to_string();
                    let idx_value = idx as i64;
                    
                    // It's an enum case - create a constant representing it
                    // For now, represent enum cases as integer tags
                    let value_id = self.fresh_value_id();
                    ir_block.add_instruction(IrInstruction {
                        result: value_id,
                        ty: IrType::Enum(enum_name_cloned),
                        kind: IrInstructionKind::Const {
                            value: IrConstant::Int(idx_value),
                        },
                    });
                    Ok(value_id)
                } else {
                    Err(LowerError::UndefinedVariable(name.to_string()))
                }
            }
        }
    }

    /// Lower a binary operation to IR.
    fn lower_binary_op(
        &mut self,
        op: &atom_ast::BinOp,
        left: &atom_ast::Expr,
        right: &atom_ast::Expr,
        ir_block: &mut IrBlock,
        func: &mut IrFunction,
    ) -> LowerResult<ValueId> {
        // Handle short-circuiting operators (&&, ||) specially
        if matches!(op, atom_ast::BinOp::And | atom_ast::BinOp::Or) {
            return self.lower_short_circuit_op(op, left, right, ir_block, func);
        }

        // Handle assignment operators
        if matches!(
            op,
            atom_ast::BinOp::Assign
                | atom_ast::BinOp::AddAssign
                | atom_ast::BinOp::SubAssign
                | atom_ast::BinOp::MulAssign
                | atom_ast::BinOp::DivAssign
                | atom_ast::BinOp::ModAssign
                | atom_ast::BinOp::ConcatAssign
        ) {
            return self.lower_assignment(op, left, right, ir_block, func);
        }

        let left_value = self.lower_expr(left, ir_block, func)?;
        let right_value = self.lower_expr(right, ir_block, func)?;

        let ir_op = self.convert_binop(op)?;
        
        // Determine result type (simplified - should use type checker)
        let result_type = match op {
            atom_ast::BinOp::Eq | atom_ast::BinOp::Ne | atom_ast::BinOp::Lt 
            | atom_ast::BinOp::Le | atom_ast::BinOp::Gt | atom_ast::BinOp::Ge => {
                IrType::Bool
            }
            _ => IrType::Int(64), // Simplified
        };

        let value_id = self.fresh_value_id();
        ir_block.add_instruction(IrInstruction {
            result: value_id,
            ty: result_type,
            kind: IrInstructionKind::BinOp {
                op: ir_op,
                left: left_value,
                right: right_value,
            },
        });

        Ok(value_id)
    }

    /// Lower short-circuiting logical operators (&& and ||).
    fn lower_short_circuit_op(
        &mut self,
        op: &atom_ast::BinOp,
        left: &atom_ast::Expr,
        right: &atom_ast::Expr,
        ir_block: &mut IrBlock,
        func: &mut IrFunction,
    ) -> LowerResult<ValueId> {
        // For now, implement as non-short-circuiting
        // TODO: Implement proper short-circuiting with control flow
        let left_value = self.lower_expr(left, ir_block, func)?;
        let right_value = self.lower_expr(right, ir_block, func)?;

        let ir_op = match op {
            atom_ast::BinOp::And => IrBinOp::And,
            atom_ast::BinOp::Or => IrBinOp::Or,
            _ => unreachable!(),
        };

        let value_id = self.fresh_value_id();
        ir_block.add_instruction(IrInstruction {
            result: value_id,
            ty: IrType::Bool,
            kind: IrInstructionKind::BinOp {
                op: ir_op,
                left: left_value,
                right: right_value,
            },
        });

        Ok(value_id)
    }

    /// Lower an assignment operation.
    /// In Atom, variables are immutable by default (SSA style), so assignments
    /// create new bindings with the same name, shadowing the old value.
    fn lower_assignment(
        &mut self,
        op: &atom_ast::BinOp,
        left: &atom_ast::Expr,
        right: &atom_ast::Expr,
        ir_block: &mut IrBlock,
        func: &mut IrFunction,
    ) -> LowerResult<ValueId> {
        // Get the variable name from the left side
        let var_name = match left {
            atom_ast::Expr::Ident(ident) => ident.name.clone(),
            _ => {
                return Err(LowerError::Unsupported(
                    "Assignment to non-identifier".to_string(),
                ))
            }
        };

        // Compute the new value
        let new_value = if matches!(op, atom_ast::BinOp::Assign) {
            // Simple assignment: var = expr
            self.lower_expr(right, ir_block, func)?
        } else {
            // Compound assignment: var += expr, var ++= expr, etc.
            // Load current value
            let current_value = self.lower_ident(&var_name, ir_block, func)?;
            let right_value = self.lower_expr(right, ir_block, func)?;

            // Determine the operation
            let ir_op = match op {
                atom_ast::BinOp::AddAssign => IrBinOp::Add,
                atom_ast::BinOp::SubAssign => IrBinOp::Sub,
                atom_ast::BinOp::MulAssign => IrBinOp::Mul,
                atom_ast::BinOp::DivAssign => IrBinOp::Div,
                atom_ast::BinOp::ModAssign => IrBinOp::Mod,
                atom_ast::BinOp::ConcatAssign => {
                    // For concat, we need to call a runtime function or builtin
                    // For now, treat it like addition (simplified)
                    IrBinOp::Add
                }
                _ => unreachable!(),
            };

            // Create the operation instruction
            let result_id = self.fresh_value_id();
            ir_block.add_instruction(IrInstruction {
                result: result_id,
                ty: IrType::Pointer(Box::new(IrType::Void)), // Simplified
                kind: IrInstructionKind::BinOp {
                    op: ir_op,
                    left: current_value,
                    right: right_value,
                },
            });

            result_id
        };

        // Check if this is a mutable variable (local) or immutable (value)
        let binding = self.variables.get(&var_name).cloned();
        match binding {
            Some(VarBinding::Local(local_id, _)) => {
                // Mutable variable - store the new value to the stack slot
                let store_id = self.fresh_value_id();
                ir_block.add_instruction(IrInstruction {
                    result: store_id,
                    ty: IrType::Void,
                    kind: IrInstructionKind::Store {
                        destination: IrMemoryLocation::Local(local_id),
                        value: new_value,
                    },
                });

                // Assignments return void
                Ok(store_id)
            }
            Some(VarBinding::Value(_, _)) => {
                // Immutable variable - error, cannot assign to immutable variable
                Err(LowerError::Internal(format!(
                    "Cannot assign to immutable variable '{}'",
                    var_name
                )))
            }
            None => Err(LowerError::UndefinedVariable(var_name)),
        }
    }

    /// Lower a unary operation to IR.
    fn lower_unary_op(
        &mut self,
        op: &atom_ast::UnOp,
        expr: &atom_ast::Expr,
        ir_block: &mut IrBlock,
        func: &mut IrFunction,
    ) -> LowerResult<ValueId> {
        let operand = self.lower_expr(expr, ir_block, func)?;

        let ir_op = match op {
            atom_ast::UnOp::Neg => IrUnOp::Neg,
            atom_ast::UnOp::Not => IrUnOp::Not,
            atom_ast::UnOp::BitNot => IrUnOp::BitNot,
        };

        // Result type is same as operand type (simplified)
        let result_type = IrType::Int(64);

        let value_id = self.fresh_value_id();
        ir_block.add_instruction(IrInstruction {
            result: value_id,
            ty: result_type,
            kind: IrInstructionKind::UnOp { op: ir_op, operand },
        });

        Ok(value_id)
    }

    /// Lower a function call to IR.
    fn lower_call(
        &mut self,
        func_expr: &atom_ast::Expr,
        args: &[atom_ast::Expr],
        ir_block: &mut IrBlock,
        func: &mut IrFunction,
    ) -> LowerResult<ValueId> {
        // Get function name
        let func_name = match func_expr {
            atom_ast::Expr::Ident(ident) => ident.name.clone(),
            _ => {
                return Err(LowerError::Unsupported(
                    "Indirect function calls".to_string(),
                ))
            }
        };

        // Check if this is actually array indexing (variable call with one argument)
        // In Atom, arr(i) is array indexing if arr is a variable
        if self.variables.contains_key(&func_name) && args.len() == 1 {
            // Check if this is a closure or an array
            let var_binding = self.variables.get(&func_name).cloned();
            if let Some(VarBinding::Value(val_id, ty)) = var_binding {
                // Check if it's a closure type
                if matches!(ty, IrType::Closure { .. }) {
                    // This is a closure call, not array indexing
                    // Fall through to handle it as a closure call below
                } else {
                    // This is array indexing
                    let index_value = self.lower_expr(&args[0], ir_block, func)?;
                    
                    // Generate array index instruction
                    let value_id = self.fresh_value_id();
                    ir_block.add_instruction(IrInstruction {
                        result: value_id,
                        ty: IrType::Int(64), // TODO: Use actual element type
                        kind: IrInstructionKind::ArrayIndex {
                            array: val_id,
                            index: index_value,
                        },
                    });
                    return Ok(value_id);
                }
            }
        }

        // Handle special builtins
        if func_name == "as_string" {
            return self.lower_as_string_builtin(args, ir_block, func);
        }
        
        // Handle loop() builtin: loop(cond) { body } or loop(arr) { body }
        if func_name == "loop" {
            // Distinguish between loop(condition) vs loop(array):
            // - loop(condition) { body } has 2 args: condition and block
            // - But actually both have 2 args in the AST
            // We need to check if it's array iteration (todo) or condition loop
            if args.len() == 2 {
                return self.lower_loop_builtin(args, ir_block, func);
            } else {
                return Err(LowerError::Unsupported(
                    "loop() requires exactly 2 arguments".to_string(),
                ));
            }
        }
        
        // Lower arguments
        let mut arg_values = Vec::new();
        for arg in args {
            let value = self.lower_expr(arg, ir_block, func)?;
            arg_values.push(value);
        }
        
        // Fill in default parameters if needed
        // Look up the function definition to get default values
        // We need to collect the default expressions first to avoid borrow issues
        let mut default_exprs = Vec::new();
        if let Some(func_defs) = self.function_defs.get(&func_name) {
            // For now, just use the first overload (TODO: handle overloading properly)
            if let Some(func_def) = func_defs.first() {
                // Check if we need to add default parameters
                let num_provided = arg_values.len();
                let num_params = func_def.params.len();
                
                if num_provided < num_params {
                    // Collect default expressions for missing parameters
                    for i in num_provided..num_params {
                        if let Some(param) = func_def.params.get(i) {
                            if let Some(default_expr) = &param.default {
                                default_exprs.push((**default_expr).clone());
                            } else {
                                // Parameter has no default but wasn't provided - this is an error
                                return Err(LowerError::UndefinedFunction(format!(
                                    "Missing required parameter '{}' for function '{}'",
                                    param.name.name, func_name
                                )));
                            }
                        }
                    }
                }
            }
        }
        
        // Now lower the default expressions
        for default_expr in &default_exprs {
            let default_value = self.lower_expr(default_expr, ir_block, func)?;
            arg_values.push(default_value);
        }

        // Check if func_name is actually a function parameter (function pointer)
        if let Some(func_value_id) = self.params.get(&func_name).copied() {
            // This is an indirect call through a function pointer
            let value_id = self.fresh_value_id();
            ir_block.add_instruction(IrInstruction {
                result: value_id,
                ty: IrType::Int(64), // Simplified return type
                kind: IrInstructionKind::CallIndirect {
                    func_value: func_value_id,
                    args: arg_values,
                },
            });
            return Ok(value_id);
        }

        // Check if func_name is a variable bound to a closure
        if let Some(binding) = self.variables.get(&func_name).cloned() {
            let (closure_value_id, closure_ty) = match binding {
                VarBinding::Value(value_id, ty) => (value_id, ty),
                VarBinding::Local(local_id, ty) => {
                    // Need to load from local first
                    let loaded_id = self.fresh_value_id();
                    ir_block.add_instruction(IrInstruction {
                        result: loaded_id,
                        ty: ty.clone(),
                        kind: IrInstructionKind::Load {
                            source: IrMemoryLocation::Local(local_id),
                        },
                    });
                    (loaded_id, ty)
                }
            };
            
            if matches!(closure_ty, IrType::Closure { .. }) {
                // This is a closure stored in a variable - use CallIndirect
                let return_ty = if let IrType::Closure { return_type, .. } = closure_ty {
                    (*return_type).clone().unwrap_or(IrType::Int(64))
                } else {
                    IrType::Int(64)
                };
                
                let value_id = self.fresh_value_id();
                ir_block.add_instruction(IrInstruction {
                    result: value_id,
                    ty: return_ty,
                    kind: IrInstructionKind::CallIndirect {
                        func_value: closure_value_id,
                        args: arg_values,
                    },
                });
                return Ok(value_id);
            }
        }

        // Check if this is a call to a generic function that needs monomorphization
        let actual_func_name = if let Some(func_defs) = self.function_defs.get(&func_name) {
            if let Some(func_def) = func_defs.first() {
                if self.is_generic_function(func_def) {
                    // This is a generic function call - we need to monomorphize it
                    eprintln!("[MONO-DEBUG] Call to generic function: {}", func_name);
                    
                    // Extract concrete types from arguments
                    let concrete_types = self.extract_concrete_types(&arg_values, func);
                    eprintln!("[MONO-DEBUG] Extracted concrete types: {:?}", concrete_types);
                    
                    // Generate monomorphized function name
                    let mono_name = self.generate_mono_name(&func_name, &concrete_types, func_def);
                    eprintln!("[MONO-DEBUG] Monomorphized name: {}", mono_name);
                    
                    // Queue this instance for generation if not already done
                    if !self.mono_done.contains(&mono_name) {
                        eprintln!("[MONO-DEBUG] Queueing monomorphization: {}", mono_name);
                        self.mono_queue.insert(
                            mono_name.clone(),
                            (func_name.clone(), concrete_types, func_def.clone()),
                        );
                        self.mono_done.insert(mono_name.clone());
                    }
                    
                    mono_name
                } else {
                    func_name.clone()
                }
            } else {
                func_name.clone()
            }
        } else {
            func_name.clone()
        };

        // Determine return type
        // Check if this is a C library function call
        let return_type = if actual_func_name.starts_with('c') && actual_func_name.contains("::") {
            self.infer_c_function_return_type(&actual_func_name)
        } else {
            // Simplified - should query type environment
            IrType::Int(64)
        };

        let value_id = self.fresh_value_id();
        ir_block.add_instruction(IrInstruction {
            result: value_id,
            ty: return_type,
            kind: IrInstructionKind::Call {
                function: actual_func_name,
                args: arg_values,
                is_tail: false,
            },
        });

        Ok(value_id)
    }

    /// Infer return type for C library functions.
    fn infer_c_function_return_type(&self, func_name: &str) -> IrType {
        // Extract the actual function name after ::
        let parts: Vec<&str> = func_name.split("::").collect();
        if parts.len() != 2 {
            return IrType::Int(64); // Default
        }

        let c_func_name = parts[1];

        // Special case for known C functions
        match c_func_name {
            "exit" | "printf" => IrType::Void,
            // Math functions ending with 'f' return float (32-bit)
            name if name.ends_with('f') && parts[0] == "cmath" => IrType::Float(32),
            // Math functions without 'f' return double (64-bit)
            _ if parts[0] == "cmath" => IrType::Float(64),
            // Default to int
            _ => IrType::Int(64),
        }
    }

    /*
    /// Lower the `loop` builtin function.
    /// For now, implement a simplified version that just executes the body once.
    /// A full implementation would create proper loop blocks with back edges.
    fn lower_loop_builtin(
        &mut self,
        args: &[atom_ast::Expr],
        ir_block: &mut IrBlock,
        func: &mut IrFunction,
    ) -> LowerResult<ValueId> {
        if args.is_empty() {
            // loop() with no args - infinite loop, not supported yet
            return Err(LowerError::Unsupported("Infinite loop".to_string()));
        }

        // For now, just lower the arguments and return a dummy value
        // A full implementation would handle:
        // - loop(condition) - while loop
        // - loop(n) - fixed iteration
        // - loop(tuple, body) - foreach with $0 binding
        
        for arg in args {
            self.lower_expr(arg, ir_block, func)?;
        }

        // Return a void value
        let value_id = self.fresh_value_id();
        ir_block.add_instruction(IrInstruction {
            result: value_id,
            ty: IrType::Void,
            kind: IrInstructionKind::Const {
                value: IrConstant::Void,
            },
        });

        Ok(value_id)
    }
    */

    /// Lower the `as_string` builtin function.
    /// Converts any value to a string representation.
    fn lower_as_string_builtin(
        &mut self,
        args: &[atom_ast::Expr],
        ir_block: &mut IrBlock,
        func: &mut IrFunction,
    ) -> LowerResult<ValueId> {
        if args.len() != 1 {
            return Err(LowerError::Unsupported(
                "as_string expects exactly 1 argument".to_string(),
            ));
        }

        // Lower the argument
        let value = self.lower_expr(&args[0], ir_block, func)?;

        // For now, just create a call to a hypothetical runtime function
        // In a real implementation, this would dispatch to type-specific converters
        let value_id = self.fresh_value_id();
        ir_block.add_instruction(IrInstruction {
            result: value_id,
            ty: IrType::Pointer(Box::new(IrType::Int(8))), // String is char*
            kind: IrInstructionKind::Call {
                function: "__builtin_as_string".to_string(),
                args: vec![value],
                is_tail: false,
            },
        });

        Ok(value_id)
    }
    
    /// Lower loop() builtin: loop(condition) { body } or loop(array) { body }
    /// Generates a while-loop or array iteration control flow structure
    fn lower_loop_builtin(
        &mut self,
        args: &[atom_ast::Expr],
        current_block: &mut IrBlock,
        func: &mut IrFunction,
    ) -> LowerResult<ValueId> {
        use atom_ast::Expr;
        
        // args[0] is the condition/array expression
        // args[1] should be a Block (the loop body)
        
        let first_arg = &args[0];
        let body_block = match &args[1] {
            Expr::Block(block) => block,
            _ => {
                return Err(LowerError::Internal(
                    "loop() expects a block as second argument".to_string(),
                ));
            }
        };
        
        // Check if this is array iteration or condition loop
        // Array iteration: loop(arr) where arr is an identifier bound to array
        // Condition loop: loop(expr) where expr is a boolean expression
        let is_array_iter = matches!(first_arg, Expr::Ident(_));
        
        if is_array_iter {
            // Array iteration: loop(arr) { body with $0 }
            // Implement as: for i in 0..len(arr) { $0 = arr[i]; body }
            
            // Get the array value
            let array_value = self.lower_expr(first_arg, current_block, func)?;
            
            // Get array length
            // First, try to find the type of the array value
            let array_ty = current_block
                .instructions
                .iter()
                .find(|inst| inst.result == array_value)
                .map(|inst| &inst.ty);
            
            // Create a local to store the array length (needed for dominance in loop header)
            let len_local = func.add_local("$array_len".to_string(), IrType::Int(64));
            let len_value = self.fresh_value_id();
            
            // Check if this is a fixed-size tuple - if so, use compile-time length
            if let Some(IrType::Tuple(element_types)) = array_ty {
                // Fixed-size tuple: use compile-time known length
                current_block.add_instruction(IrInstruction {
                    result: len_value,
                    ty: IrType::Int(64),
                    kind: IrInstructionKind::Const {
                        value: IrConstant::Int(element_types.len() as i64),
                    },
                });
            } else {
                // Runtime length via ArrayLen
                current_block.add_instruction(IrInstruction {
                    result: len_value,
                    ty: IrType::Int(64),
                    kind: IrInstructionKind::ArrayLen {
                        array: array_value,
                    },
                });
            }
            
            // Store length in local so it can be loaded in loop header
            current_block.add_instruction(IrInstruction {
                result: self.fresh_value_id(),
                ty: IrType::Void,
                kind: IrInstructionKind::Store {
                    destination: IrMemoryLocation::Local(len_local),
                    value: len_value,
                },
            });
            
            // Create index variable initialized to 0
            let index_local = func.add_local("$loop_index".to_string(), IrType::Int(64));
            
            let zero_value = self.fresh_value_id();
            current_block.add_instruction(IrInstruction {
                result: zero_value,
                ty: IrType::Int(64),
                kind: IrInstructionKind::Const {
                    value: IrConstant::Int(0),
                },
            });
            
            current_block.add_instruction(IrInstruction {
                result: self.fresh_value_id(),
                ty: IrType::Void,
                kind: IrInstructionKind::Store {
                    destination: IrMemoryLocation::Local(index_local),
                    value: zero_value,
                },
            });
            
            // Create loop blocks: header (condition check), body, exit
            let header_id = self.fresh_block_id();
            let body_id = self.fresh_block_id();
            let exit_id = self.fresh_block_id();
            
            // Jump to header
            current_block.set_terminator(IrTerminator::Jump { target: header_id });
            
            // Header block: check if index < len
            let mut header = IrBlock::new(header_id);
            
            let index_value = self.fresh_value_id();
            header.add_instruction(IrInstruction {
                result: index_value,
                ty: IrType::Int(64),
                kind: IrInstructionKind::Load {
                    source: IrMemoryLocation::Local(index_local),
                },
            });
            
            // Load array length from local
            let len_loaded = self.fresh_value_id();
            header.add_instruction(IrInstruction {
                result: len_loaded,
                ty: IrType::Int(64),
                kind: IrInstructionKind::Load {
                    source: IrMemoryLocation::Local(len_local),
                },
            });
            
            let cond_value = self.fresh_value_id();
            header.add_instruction(IrInstruction {
                result: cond_value,
                ty: IrType::Bool,
                kind: IrInstructionKind::BinOp {
                    op: IrBinOp::Lt,
                    left: index_value,
                    right: len_loaded,
                },
            });
            
            header.set_terminator(IrTerminator::Branch {
                condition: cond_value,
                true_block: body_id,
                false_block: exit_id,
            });
            
            // Body block: get array element, bind to $0, execute body, increment index
            let mut loop_body_ir = IrBlock::new(body_id);
            
            // Load current index
            let body_index = self.fresh_value_id();
            loop_body_ir.add_instruction(IrInstruction {
                result: body_index,
                ty: IrType::Int(64),
                kind: IrInstructionKind::Load {
                    source: IrMemoryLocation::Local(index_local),
                },
            });
            
            // Get array element: arr[index]
            let element_value = self.fresh_value_id();
            loop_body_ir.add_instruction(IrInstruction {
                result: element_value,
                ty: IrType::Int(64), // TODO: Use actual element type
                kind: IrInstructionKind::ArrayIndex {
                    array: array_value,
                    index: body_index,
                },
            });
            
            // Store $0 in a local variable so it's accessible in nested blocks
            let dollar0_local = func.add_local("$0".to_string(), IrType::Int(64));
            loop_body_ir.add_instruction(IrInstruction {
                result: self.fresh_value_id(),
                ty: IrType::Void,
                kind: IrInstructionKind::Store {
                    destination: IrMemoryLocation::Local(dollar0_local),
                    value: element_value,
                },
            });
            
            // Bind $0 to the local variable (not the SSA value)
            let old_dollar0 = self.variables.get("$0").cloned();
            self.variables.insert("$0".to_string(), VarBinding::Local(dollar0_local, IrType::Int(64)));
            
            // Execute loop body
            let (_body_result, _) = self.lower_block_to_ir(body_block, &mut loop_body_ir, func)?;
            
            // Restore $0
            if let Some(old) = old_dollar0 {
                self.variables.insert("$0".to_string(), old);
            } else {
                self.variables.remove("$0");
            }
            
            // Increment index
            // NOTE: We need to load the index fresh here because the loop body might contain
            // match expressions that create new blocks. After lower_block_to_ir, loop_body_ir
            // might be a merge block, not the original body block where body_index was defined.
            // To ensure dominance, we load from the local variable.
            let increment_index = self.fresh_value_id();
            loop_body_ir.add_instruction(IrInstruction {
                result: increment_index,
                ty: IrType::Int(64),
                kind: IrInstructionKind::Load {
                    source: IrMemoryLocation::Local(index_local),
                },
            });
            
            let one_value = self.fresh_value_id();
            loop_body_ir.add_instruction(IrInstruction {
                result: one_value,
                ty: IrType::Int(64),
                kind: IrInstructionKind::Const {
                    value: IrConstant::Int(1),
                },
            });
            
            let next_index = self.fresh_value_id();
            loop_body_ir.add_instruction(IrInstruction {
                result: next_index,
                ty: IrType::Int(64),
                kind: IrInstructionKind::BinOp {
                    op: IrBinOp::Add,
                    left: increment_index,  // Use the freshly loaded value
                    right: one_value,
                },
            });
            
            loop_body_ir.add_instruction(IrInstruction {
                result: self.fresh_value_id(),
                ty: IrType::Void,
                kind: IrInstructionKind::Store {
                    destination: IrMemoryLocation::Local(index_local),
                    value: next_index,
                },
            });
            
            // Jump back to header
            loop_body_ir.set_terminator(IrTerminator::Jump { target: header_id });
            
            // Add blocks to function
            let old_block = std::mem::replace(current_block, IrBlock::new(exit_id));
            func.add_block(old_block);
            func.add_block(header);
            func.add_block(loop_body_ir);
            
            // Return void from exit block
            let void_value = self.fresh_value_id();
            current_block.add_instruction(IrInstruction {
                result: void_value,
                ty: IrType::Void,
                kind: IrInstructionKind::Const {
                    value: IrConstant::Void,
                },
            });
            
            return Ok(void_value);
        }
        
        // Condition loop: generate while-loop control flow
        
        // Create three blocks: header (condition check), body, exit
        let header_id = self.fresh_block_id();
        let body_id = self.fresh_block_id();
        let exit_id = self.fresh_block_id();
        
        // Current block jumps to header
        current_block.set_terminator(IrTerminator::Jump { target: header_id });
        
        // Header block: evaluate condition and branch
        let mut header = IrBlock::new(header_id);
        let cond_value = self.lower_expr(first_arg, &mut header, func)?;
        header.set_terminator(IrTerminator::Branch {
            condition: cond_value,
            true_block: body_id,
            false_block: exit_id,
        });
        
        // Body block: execute block statements and jump back to header
        let mut body = IrBlock::new(body_id);
        self.lower_block_to_ir(body_block, &mut body, func)?;
        body.set_terminator(IrTerminator::Jump { target: header_id });
        
        // Set current block's terminator to jump to header
        current_block.set_terminator(IrTerminator::Jump { target: header_id });
        
        // Add the current block to function before replacing it
        // This preserves any instructions added before the loop expression
        let old_current_block = std::mem::replace(current_block, IrBlock::new(exit_id));
        func.add_block(old_current_block);
        
        // Add header and body blocks to function
        func.add_block(header);
        func.add_block(body);
        
        // current_block is now the exit block (from mem::replace above)
        // Further lowering will continue in this exit block
        
        // loop returns void
        let void_value = self.fresh_value_id();
        current_block.add_instruction(IrInstruction {
            result: void_value,
            ty: IrType::Void,
            kind: IrInstructionKind::Const {
                value: IrConstant::Void,
            },
        });
        
        Ok(void_value)
    }

    /// Lower a method call expression.
    /// In Atom, x.method(args) is syntactic sugar for method(x, args).
    fn lower_method_call(
        &mut self,
        receiver: &atom_ast::Expr,
        method: &atom_ast::Ident,
        args: &[atom_ast::Expr],
        ir_block: &mut IrBlock,
        func: &mut IrFunction,
    ) -> LowerResult<ValueId> {
        let method_name = &method.name;

        // Handle special builtin methods
        if method_name == "as_string" {
            // receiver.as_string() is the same as as_string(receiver)
            return self.lower_as_string_builtin(&[receiver.clone()], ir_block, func);
        }

        // General method call: convert to function call with receiver as first arg
        let receiver_value = self.lower_expr(receiver, ir_block, func)?;
        
        let mut arg_values = vec![receiver_value];
        for arg in args {
            let value = self.lower_expr(arg, ir_block, func)?;
            arg_values.push(value);
        }

        // Call the function
        let value_id = self.fresh_value_id();
        ir_block.add_instruction(IrInstruction {
            result: value_id,
            ty: IrType::Pointer(Box::new(IrType::Void)), // Simplified
            kind: IrInstructionKind::Call {
                function: method_name.clone(),
                args: arg_values,
                is_tail: false,
            },
        });

        Ok(value_id)
    }

    /// Lower a tuple expression to IR.
    fn lower_tuple(
        &mut self,
        elements: &[atom_ast::Expr],
        ir_block: &mut IrBlock,
        func: &mut IrFunction,
    ) -> LowerResult<ValueId> {
        let mut element_values = Vec::new();
        let mut element_types = Vec::new();

        for elem in elements {
            let value = self.lower_expr(elem, ir_block, func)?;
            element_values.push(value);
            element_types.push(IrType::Int(64)); // Simplified
        }

        let value_id = self.fresh_value_id();
        ir_block.add_instruction(IrInstruction {
            result: value_id,
            ty: IrType::Tuple(element_types),
            kind: IrInstructionKind::MakeTuple {
                elements: element_values,
            },
        });

        Ok(value_id)
    }

    /// Lower a struct initialization to IR.
    fn lower_struct_init(
        &mut self,
        ty: &Option<atom_ast::Ident>,
        fields: &[atom_ast::FieldInit],
        ir_block: &mut IrBlock,
        func: &mut IrFunction,
    ) -> LowerResult<ValueId> {
        let struct_name = ty
            .as_ref()
            .ok_or_else(|| LowerError::Unsupported("Anonymous struct initialization".to_string()))?
            .name
            .clone();

        let mut field_values = Vec::new();
        for field in fields {
            let value = self.lower_expr(&field.value, ir_block, func)?;
            field_values.push(value);
        }

        // Check if this is actually an enum variant constructor
        if let Some((enum_name, _case, _idx)) = self.type_env.find_enum_case(&struct_name) {
            // This is an enum variant with payload (e.g., Some(x))
            // Represent it as an enum type, not a struct type
            let enum_name_cloned = enum_name.to_string();
            let value_id = self.fresh_value_id();
            ir_block.add_instruction(IrInstruction {
                result: value_id,
                ty: IrType::Enum(enum_name_cloned),
                kind: IrInstructionKind::MakeStruct {
                    struct_name,
                    fields: field_values,
                },
            });
            Ok(value_id)
        } else {
            // Regular struct initialization
            let value_id = self.fresh_value_id();
            ir_block.add_instruction(IrInstruction {
                result: value_id,
                ty: IrType::Struct(struct_name.clone()),
                kind: IrInstructionKind::MakeStruct {
                    struct_name,
                    fields: field_values,
                },
            });
            Ok(value_id)
        }
    }

    /// Lower a field access expression to IR.
    fn lower_field_access(
        &mut self,
        object: &atom_ast::Expr,
        _field_name: &str,
        ir_block: &mut IrBlock,
        func: &mut IrFunction,
    ) -> LowerResult<ValueId> {
        let object_value = self.lower_expr(object, ir_block, func)?;

        // TODO: Look up field index from type information
        let field_index = 0; // Simplified

        let value_id = self.fresh_value_id();
        ir_block.add_instruction(IrInstruction {
            result: value_id,
            ty: IrType::Int(64), // Simplified
            kind: IrInstructionKind::StructExtract {
                struct_value: object_value,
                field_index,
            },
        });

        Ok(value_id)
    }

    /// Lower a match expression to IR (switch statement).
    fn lower_match(
        &mut self,
        match_expr: &atom_ast::Expr,
        arms: &[atom_ast::MatchArm],
        ir_block: &mut IrBlock,
        func: &mut IrFunction,
    ) -> LowerResult<ValueId> {
        // Lower the match expression
        let match_value = self.lower_expr(match_expr, ir_block, func)?;

        // For now, implement simple integer matching only
        // TODO: Implement full pattern matching for enums, tuples, etc.

        if arms.is_empty() {
            return Err(LowerError::InvalidPattern("Match must have at least one arm".to_string()));
        }

        // Create blocks for each arm and a merge block
        let merge_block_id = self.fresh_block_id();
        let mut case_blocks = Vec::new();
        let mut case_values = Vec::new();
        let mut case_block_ids = Vec::new();

        for (i, arm) in arms.iter().enumerate() {
            let arm_block_id = self.fresh_block_id();
            case_block_ids.push(arm_block_id);
            let mut arm_block = IrBlock::new(arm_block_id);

            // Determine the tag value for this pattern
            let tag_value = match &arm.pattern {
                atom_ast::Pattern::Enum { name, .. } => {
                    // For enum patterns, map the variant name to its tag value
                    // For Bool: False = 0, True = 1
                    match name.name.as_str() {
                        "False" => 0,
                        "True" => 1,
                        // For Option: None = 0, Some = 1
                        "None" => 0,
                        "Some" => 1,
                        // For other enums, use arm index as fallback
                        _ => i as u32,
                    }
                }
                _ => i as u32, // For non-enum patterns, use arm index
            };

            // Handle pattern bindings
            // For enum patterns like Some(inner), bind the payload to the variable
            if let atom_ast::Pattern::Enum { name: _, fields, .. } = &arm.pattern {
                for field_pattern in fields {
                    if let atom_ast::Pattern::Ident(ident) = field_pattern {
                        // Extract the payload from the matched enum variant
                        // For simplification, create a dummy extraction (extract from match_value)
                        let payload_id = self.fresh_value_id();
                        arm_block.add_instruction(IrInstruction {
                            result: payload_id,
                            ty: IrType::Pointer(Box::new(IrType::Void)), // Simplified
                            kind: IrInstructionKind::TupleExtract {
                                tuple: match_value,
                                index: 1, // Index 0 is the tag, index 1 is the payload
                            },
                        });
                        // Bind the variable
                        self.variables.insert(
                            ident.name.clone(),
                            VarBinding::Value(payload_id, IrType::Pointer(Box::new(IrType::Void))),
                        );
                    }
                }
            }

            // Lower the arm body
            let arm_value = self.lower_expr(&arm.body, &mut arm_block, func)?;
            case_values.push(arm_value);

            // Jump to merge block
            arm_block.set_terminator(IrTerminator::Jump {
                target: merge_block_id,
            });

            // NOTE: After lowering the arm body, arm_block might have been replaced with a
            // nested merge block (if the body contained match expressions). We need to use
            // arm_block.label (the actual current label) for the phi node, not arm_block_id.
            let actual_predecessor_id = arm_block.label;
            case_blocks.push((tag_value, arm_block_id, arm_block, actual_predecessor_id));
        }

        // Create switch terminator on current block
        let default_block_id = case_blocks.last().unwrap().1;
        let cases: Vec<(u32, BlockId)> = case_blocks
            .iter()
            .take(case_blocks.len() - 1)
            .map(|(tag, block_id, _, _)| (*tag, *block_id))
            .collect();

        ir_block.set_terminator(IrTerminator::Switch {
            value: match_value,
            cases,
            default: default_block_id,
        });

        // IMPORTANT: Add the current block to the function before replacing it
        // This preserves any instructions that were added to it before the match expression
        let current_block_label = ir_block.label;
        let current_block = std::mem::replace(ir_block, IrBlock::new(merge_block_id));
        func.add_block(current_block);

        // Add all case blocks to function
        for (_, _, block, _) in &case_blocks {
            func.add_block(block.clone());
        }

        // Create merge block with phi node
        let result_value = self.fresh_value_id();

        // Use the actual predecessor block IDs (which might be nested merge blocks)
        let incoming: Vec<(BlockId, ValueId)> = case_blocks
            .iter()
            .enumerate()
            .map(|(i, (_, _, _, actual_pred_id))| (*actual_pred_id, case_values[i]))
            .collect();

        ir_block.add_instruction(IrInstruction {
            result: result_value,
            ty: IrType::Int(64), // Simplified
            kind: IrInstructionKind::Phi { incoming },
        });

        // ir_block is now the merge block (from the mem::replace above)

        Ok(result_value)
    }

    // ========================================================================
    // Type Conversion
    // ========================================================================

    /// Convert an AST type to an IR type.
    fn lower_type(&mut self, ty: &atom_ast::Type) -> LowerResult<IrType> {
        match ty {
            atom_ast::Type::Named(ident) => self.lower_named_type(&ident.name),
            atom_ast::Type::Tuple(types, _) => {
                let mut ir_types = Vec::new();
                for ty in types {
                    ir_types.push(self.lower_type(ty)?);
                }
                Ok(IrType::Tuple(ir_types))
            }
            atom_ast::Type::Function { params, return_type, .. } => {
                let mut param_types = Vec::new();
                for param in params {
                    param_types.push(self.lower_type(param)?);
                }
                let ret_type = if let Some(ret) = return_type {
                    Some(self.lower_type(ret)?)
                } else {
                    None
                };
                Ok(IrType::Function {
                    params: param_types,
                    return_type: Box::new(ret_type),
                })
            }
            atom_ast::Type::Generic { name, .. } => {
                // For now, treat generics as their base type
                self.lower_named_type(&name.name)
            }
            atom_ast::Type::Param(_) => {
                // Type parameters are generic/polymorphic
                // For type erasure, treat them as string pointers (char*)
                // This works for print() since it calls as_string() which returns a string
                // TODO: Full monomorphization for generic functions that need actual types
                Ok(IrType::Pointer(Box::new(IrType::Int(8))))
            }
            atom_ast::Type::Variadic { element, .. } => {
                // Variadic types are arrays with runtime length
                // Represented as a fat pointer (ptr + length)
                let elem_type = self.lower_type(element)?;
                Ok(IrType::Array {
                    element: Box::new(elem_type),
                })
            }
            atom_ast::Type::StaticArray { element, .. } => {
                // Static arrays with compile-time known size
                // For now, treat as array (will optimize later for stack allocation)
                let elem_type = self.lower_type(element)?;
                Ok(IrType::Array {
                    element: Box::new(elem_type),
                })
            }
            atom_ast::Type::StaticArray { element, span: _, size: _ } => {
                // Static arrays with compile-time known size
                // For now, treat as array (will optimize later for stack allocation)
                let elem_type = self.lower_type(element)?;
                Ok(IrType::Array {
                    element: Box::new(elem_type),
                })
            }
            _ => Err(LowerError::Unsupported(format!(
                "Type conversion: {:?}",
                ty
            ))),
        }
    }

    /// Convert a named type to IR.
    fn lower_named_type(&self, name: &str) -> LowerResult<IrType> {
        match name {
            "Void" => Ok(IrType::Void),
            "Bool" => Ok(IrType::Bool),
            "Int" => Ok(IrType::Int(64)),
            "UInt" => Ok(IrType::UInt(64)),
            "Float" => Ok(IrType::Float(64)),
            "Rune" => Ok(IrType::Rune),
            "String" => Ok(IrType::Pointer(Box::new(IrType::Int(8)))),
            _ => {
                // Check if it's a user-defined struct or enum
                if self.type_env.get_struct(name).is_some() {
                    Ok(IrType::Struct(name.to_string()))
                } else if self.type_env.get_enum(name).is_some() {
                    Ok(IrType::Enum(name.to_string()))
                } else if self.type_env.find_enum_case(name).is_some() {
                    // It's an enum case - use the parent enum type
                    if let Some((enum_name, _, _)) = self.type_env.find_enum_case(name) {
                        Ok(IrType::Enum(enum_name.to_string()))
                    } else {
                        Err(LowerError::UndefinedStruct(name.to_string()))
                    }
                } else {
                    // Unknown type - could be a type parameter, treat as opaque pointer
                    Ok(IrType::Pointer(Box::new(IrType::Void)))
                }
            }
        }
    }

    // ========================================================================
    // Helper Methods
    // ========================================================================

    /// Convert AST binary operator to IR binary operator.
    fn convert_binop(&self, op: &atom_ast::BinOp) -> LowerResult<IrBinOp> {
        match op {
            atom_ast::BinOp::Add => Ok(IrBinOp::Add),
            atom_ast::BinOp::Sub => Ok(IrBinOp::Sub),
            atom_ast::BinOp::Mul => Ok(IrBinOp::Mul),
            atom_ast::BinOp::Div => Ok(IrBinOp::Div),
            atom_ast::BinOp::Mod => Ok(IrBinOp::Mod),
            atom_ast::BinOp::Eq => Ok(IrBinOp::Eq),
            atom_ast::BinOp::Ne => Ok(IrBinOp::Ne),
            atom_ast::BinOp::Lt => Ok(IrBinOp::Lt),
            atom_ast::BinOp::Le => Ok(IrBinOp::Le),
            atom_ast::BinOp::Gt => Ok(IrBinOp::Gt),
            atom_ast::BinOp::Ge => Ok(IrBinOp::Ge),
            atom_ast::BinOp::And => Ok(IrBinOp::And),
            atom_ast::BinOp::Or => Ok(IrBinOp::Or),
            atom_ast::BinOp::BitAnd => Ok(IrBinOp::BitAnd),
            atom_ast::BinOp::BitOr => Ok(IrBinOp::BitOr),
            atom_ast::BinOp::LShift => Ok(IrBinOp::LShift),
            atom_ast::BinOp::RShift => Ok(IrBinOp::RShift),
            atom_ast::BinOp::Concat => Ok(IrBinOp::Concat),
            _ => Err(LowerError::Unsupported(format!(
                "Binary operator: {:?}",
                op
            ))),
        }
    }

    /// Allocate a fresh value ID.
    fn fresh_value_id(&mut self) -> ValueId {
        let id = ValueId(self.next_value_id);
        self.next_value_id += 1;
        id
    }

    /// Allocate a fresh block ID.
    fn fresh_block_id(&mut self) -> BlockId {
        let id = BlockId(self.next_block_id);
        self.next_block_id += 1;
        id
    }

    /// Allocate a fresh local ID.
    #[allow(dead_code)]
    fn fresh_local_id(&mut self) -> LocalId {
        let id = LocalId(self.next_local_id);
        self.next_local_id += 1;
        id
    }

    /// Lower a closure to IR by lambda lifting.
    ///
    /// This function:
    /// 1. Analyzes which variables from outer scope are captured
    /// 2. Generates a new top-level function that takes captures as parameters
    /// 3. Creates a MakeClosure instruction that bundles the function with captures
    fn lower_closure(
        &mut self,
        params: &[atom_ast::Param],
        return_type: &Option<Box<atom_ast::Type>>,
        body: &atom_ast::Block,
        ir_block: &mut IrBlock,
        _current_func: &mut IrFunction,
    ) -> LowerResult<ValueId> {
        // Generate unique name for the lifted closure function
        let closure_name = format!("$closure${}", self.next_closure_id);
        self.next_closure_id += 1;

        // Analyze captures: find all free variables in the closure body
        let captures = self.analyze_captures(body);

        // Build parameter list for lifted function: captures first, then closure params
        let mut lifted_params = Vec::new();

        // Add capture parameters
        for (capture_name, capture_binding) in &captures {
            let capture_ty = match capture_binding {
                VarBinding::Value(_, ty) => ty.clone(),
                VarBinding::Local(_, ty) => ty.clone(),
            };
            lifted_params.push((capture_name.clone(), capture_ty));
        }

        // Add closure parameters
        for param in params {
            let param_ty = if let Some(ty) = &param.ty {
                self.lower_type(ty)?
            } else {
                // Default to Int if no type specified
                IrType::Int(64)
            };
            lifted_params.push((param.name.name.clone(), param_ty));
        }

        // Determine return type
        let ret_ty = if let Some(ty) = return_type {
            Some(self.lower_type(ty)?)
        } else {
            None
        };

        // Save current state
        let old_vars = self.variables.clone();
        let old_params = self.params.clone();

        // Create the lifted function with all parameters
        let mut lifted_func = IrFunction::new(
            closure_name.clone(),
            lifted_params.clone(),
            ret_ty.clone(),
            false, // not public
        );

        // Clear parameter state for new function
        self.params.clear();
        self.variables.clear();

        // Set up parameter bindings for the lifted function
        for (i, (param_name, param_ty)) in lifted_params.iter().enumerate() {
            let param_id = ValueId(i as u32);
            self.params.insert(param_name.clone(), param_id);
            self.variables.insert(param_name.clone(), VarBinding::Value(param_id, param_ty.clone()));
        }

        // Create entry block for lifted function
        let entry_block_id = self.fresh_block_id();
        let mut entry_block = IrBlock::new(entry_block_id);

        // Lower the closure body
        let (body_value, _) = self.lower_block_to_ir(body, &mut entry_block, &mut lifted_func)?;

        // If body produces a value but no return type was specified, infer it
        let final_ret_ty = if ret_ty.is_none() && body_value.is_some() {
            // Get the type of the returned value
            let value_id = body_value.unwrap();
            
            // First try to get the type from the last instruction
            if let Some(last_inst) = entry_block.instructions.last() {
                if last_inst.result == value_id {
                    Some(last_inst.ty.clone())
                } else {
                    // The value is from an earlier instruction, search for it
                    entry_block.instructions.iter()
                        .find(|inst| inst.result == value_id)
                        .map(|inst| inst.ty.clone())
                        .or_else(|| {
                            // The value might be a parameter, look up in parameter list
                            lifted_params.iter()
                                .enumerate()
                                .find(|(i, _)| ValueId(*i as u32) == value_id)
                                .map(|(_, (_, ty))| ty.clone())
                        })
                }
            } else {
                // No instructions in block, value must be a parameter
                lifted_params.iter()
                    .enumerate()
                    .find(|(i, _)| ValueId(*i as u32) == value_id)
                    .map(|(_, (_, ty))| ty.clone())
            }
        } else {
            ret_ty.clone()
        };

        // Update the function's return type if we inferred it
        if let Some(debug) = std::env::var("ATOM_DEBUG").ok() {
            if debug == "1" {
                eprintln!("[DEBUG] Closure '{}': inferred return type = {:?}", closure_name, final_ret_ty);
                eprintln!("[DEBUG] Closure '{}': current return type = {:?}", closure_name, lifted_func.return_type);
            }
        }
        if final_ret_ty != lifted_func.return_type {
            lifted_func.return_type = final_ret_ty.clone();
            if let Some(debug) = std::env::var("ATOM_DEBUG").ok() {
                if debug == "1" {
                    eprintln!("[DEBUG] Closure '{}': updated return type to {:?}", closure_name, final_ret_ty);
                }
            }
        }

        // Set return terminator
        entry_block.set_terminator(IrTerminator::Return {
            value: body_value,
        });

        lifted_func.add_block(entry_block);

        // Restore variable state
        self.variables = old_vars;
        self.params = old_params;

        // Store the generated closure function
        self.closure_functions.push(lifted_func);

        // Now create the MakeClosure instruction in the current function
        // Lower capture values
        let mut capture_values = Vec::new();
        for (capture_name, _) in &captures {
            // Look up the capture in current scope - clone to avoid borrow issues
            if let Some(binding) = self.variables.get(capture_name).cloned() {
                match binding {
                    VarBinding::Value(value_id, _) => {
                        capture_values.push(value_id);
                    }
                    VarBinding::Local(local_id, ty) => {
                        // Load from local
                        let value_id = self.fresh_value_id();
                        ir_block.add_instruction(IrInstruction {
                            result: value_id,
                            ty: ty.clone(),
                            kind: IrInstructionKind::Load {
                                source: IrMemoryLocation::Local(local_id),
                            },
                        });
                        capture_values.push(value_id);
                    }
                }
            }
        }

        // Create the closure value
        let closure_value_id = self.fresh_value_id();
        
        // Build closure parameter types (not including captures)
        let closure_param_types: Vec<IrType> = params.iter().map(|p| {
            if let Some(ty) = &p.ty {
                self.lower_type(ty).unwrap_or(IrType::Int(64))
            } else {
                IrType::Int(64)
            }
        }).collect();

        let closure_ty = IrType::Closure {
            params: closure_param_types,
            return_type: Box::new(ret_ty),
        };

        ir_block.add_instruction(IrInstruction {
            result: closure_value_id,
            ty: closure_ty,
            kind: IrInstructionKind::MakeClosure {
                function: closure_name,
                captures: capture_values,
            },
        });

        Ok(closure_value_id)
    }

    /// Analyze a block to find captured variables (free variables).
    fn analyze_captures(&self, block: &atom_ast::Block) -> Vec<(String, VarBinding)> {
        let mut captures = Vec::new();
        
        // For now, implement a simple capture analysis
        // We'll look for all identifiers and check if they're in current scope
        self.collect_free_vars_block(block, &mut captures);
        
        captures
    }

    /// Recursively collect free variables from a block.
    fn collect_free_vars_block(&self, block: &atom_ast::Block, captures: &mut Vec<(String, VarBinding)>) {
        for stmt in &block.stmts {
            self.collect_free_vars_stmt(stmt, captures);
        }
    }

    /// Collect free variables from a statement.
    fn collect_free_vars_stmt(&self, stmt: &atom_ast::Stmt, captures: &mut Vec<(String, VarBinding)>) {
        match stmt {
            atom_ast::Stmt::Expression(expr) => {
                self.collect_free_vars_expr(expr, captures);
            }
            atom_ast::Stmt::VarDecl(decl) => {
                if let Some(init) = &decl.init {
                    self.collect_free_vars_expr(init, captures);
                }
            }
        }
    }

    /// Collect free variables from an expression.
    fn collect_free_vars_expr(&self, expr: &atom_ast::Expr, captures: &mut Vec<(String, VarBinding)>) {
        match expr {
            atom_ast::Expr::Ident(ident) => {
                // Check if this identifier is in the current scope
                if let Some(binding) = self.variables.get(&ident.name) {
                    // Only capture if not already in the list
                    if !captures.iter().any(|(name, _)| name == &ident.name) {
                        captures.push((ident.name.clone(), binding.clone()));
                    }
                }
            }
            atom_ast::Expr::Binary { left, right, .. } => {
                self.collect_free_vars_expr(left, captures);
                self.collect_free_vars_expr(right, captures);
            }
            atom_ast::Expr::Unary { expr, .. } => {
                self.collect_free_vars_expr(expr, captures);
            }
            atom_ast::Expr::Call { func, args, .. } => {
                self.collect_free_vars_expr(func, captures);
                for arg in args {
                    self.collect_free_vars_expr(arg, captures);
                }
            }
            atom_ast::Expr::Block(block) => {
                self.collect_free_vars_block(block, captures);
            }
            _ => {}
        }
    }

    // ========================================================================
    // Generic Function Detection
    // ========================================================================

    /// Check if a function definition is generic (has type parameters).
    ///
    /// A function is considered generic if:
    /// 1. It has const_params (compile-time parameters), OR
    /// 2. Any of its parameters have types that contain TypeParam, OR
    /// 3. Its return type contains TypeParam
    fn is_generic_function(&self, func_def: &atom_ast::FunctionDef) -> bool {
        // Check for const parameters
        if !func_def.const_params.is_empty() {
            eprintln!("[MONO-DEBUG] {} has const_params", func_def.name.name);
            return true;
        }

        // Check parameter types for type parameters
        for param in &func_def.params {
            if let Some(ref ty) = param.ty {
                if self.type_contains_type_param(ty) {
                    eprintln!("[MONO-DEBUG] {} has type param in parameter: {:?}", func_def.name.name, ty);
                    return true;
                }
            }
        }

        // Check return type for type parameters
        if let Some(ref return_type) = func_def.return_type {
            if self.type_contains_type_param(return_type) {
                eprintln!("[MONO-DEBUG] {} has type param in return type", func_def.name.name);
                return true;
            }
        }

        false
    }

    /// Check if a type contains type parameters.
    ///
    /// Recursively checks all type constructors for the presence of TypeParam.
    fn type_contains_type_param(&self, ty: &atom_ast::Type) -> bool {
        match ty {
            // Direct type parameter reference
            atom_ast::Type::Param(_) => true,

            // Named types are not generic unless they resolve to a type parameter
            atom_ast::Type::Named(_) => false,

            // Tuple types: check if any element contains TypeParam
            atom_ast::Type::Tuple(types, _) => {
                types.iter().any(|t| self.type_contains_type_param(t))
            }

            // Generic types: check type arguments
            atom_ast::Type::Generic { params, .. } => {
                params.iter().any(|param| {
                    // Check both the type constraint and default value
                    if let Some(ref ty) = param.ty {
                        if self.type_contains_type_param(ty) {
                            return true;
                        }
                    }
                    if let Some(ref default) = param.default {
                        if self.type_contains_type_param(default) {
                            return true;
                        }
                    }
                    false
                })
            }

            // Variadic types: check element type
            atom_ast::Type::Variadic { element, .. } => {
                self.type_contains_type_param(element)
            }

            // Static array types: check element type
            atom_ast::Type::StaticArray { element, .. } => {
                self.type_contains_type_param(element)
            }

            // Function types: check parameters and return type
            atom_ast::Type::Function { params, return_type, .. } => {
                // Check parameter types
                if params.iter().any(|p| self.type_contains_type_param(p)) {
                    return true;
                }
                // Check return type
                if let Some(ret) = return_type {
                    if self.type_contains_type_param(ret) {
                        return true;
                    }
                }
                false
            }
        }
    }

    /// Extract concrete types from call arguments for monomorphization.
    ///
    /// For now, we use a simplified approach: extract the IR type from each argument ValueId.
    /// In a full implementation, we'd need to track type information through the lowering process.
    fn extract_concrete_types(
        &self,
        arg_values: &[ValueId],
        func: &IrFunction,
    ) -> HashMap<String, IrType> {
        let mut concrete_types = HashMap::new();
        
        // Simple heuristic: use the first argument's type as the generic type parameter
        // This works for simple cases like print(msg t) where t is inferred from msg
        if let Some(&first_arg) = arg_values.first() {
            // Find the instruction that produced this value to get its type
            if let Some(inst) = func.blocks.iter()
                .flat_map(|block| &block.instructions)
                .find(|inst| inst.result == first_arg)
            {
                concrete_types.insert("t".to_string(), inst.ty.clone());
            } else {
                // Fallback: assume string pointer for now
                eprintln!("[MONO-DEBUG] Could not find instruction for arg {}, using string pointer", first_arg.0);
                concrete_types.insert("t".to_string(), IrType::Pointer(Box::new(IrType::Int(8))));
            }
        }
        
        concrete_types
    }

    /// Generate a monomorphized function name based on concrete type parameters.
    ///
    /// Format: original_name$TypeName1$TypeName2
    /// Example: print$String, print$Int
    fn generate_mono_name(
        &self,
        func_name: &str,
        concrete_types: &HashMap<String, IrType>,
        func_def: &atom_ast::FunctionDef,
    ) -> String {
        let mut name = func_name.to_string();
        
        // Append type parameter names in order they appear in const_params
        for param in &func_def.const_params {
            if let Some(concrete_type) = concrete_types.get(&param.name.name) {
                let type_suffix = match concrete_type {
                    IrType::Int(bits) => format!("Int{}", bits),
                    IrType::Float(bits) => format!("Float{}", bits),
                    IrType::Pointer(inner) => match **inner {
                        IrType::Int(8) => "String".to_string(),
                        _ => "Ptr".to_string(),
                    },
                    IrType::Void => "Void".to_string(),
                    _ => "Generic".to_string(),
                };
                name.push('$');
                name.push_str(&type_suffix);
            }
        }
        
        name
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use atom_ast::{
        BinOp, Block, Expr, Field, FunctionDef, Ident, Literal, Param, Span, Stmt, StructDef,
        TopLevel, Type as AstType, Visibility,
    };

    /// Helper to create an identifier
    fn ident(name: &str) -> Ident {
        Ident {
            name: name.to_string(),
            span: Span::new(0, 0),
        }
    }

    /// Helper to create a type
    fn int_type() -> Box<AstType> {
        Box::new(AstType::Named(ident("Int")))
    }

    #[test]
    fn test_lower_simple_function() {
        let mut lower = Lower::new(TypeEnvironment::new());

        // Create a simple function: add(a Int, b Int) Int { a + b }
        let func_def = FunctionDef {
            visibility: Visibility::Public,
            name: ident("add"),
            const_params: vec![],
            params: vec![
                Param {
                    name: ident("a"),
                    ty: Some(int_type()),
                    default: None,
                    span: Span::new(0, 0),
                },
                Param {
                    name: ident("b"),
                    ty: Some(int_type()),
                    default: None,
                    span: Span::new(0, 0),
                },
            ],
            return_type: Some(int_type()),
            body: Block {
                stmts: vec![Stmt::Expression(Expr::Binary {
                    op: BinOp::Add,
                    left: Box::new(Expr::Ident(ident("a"))),
                    right: Box::new(Expr::Ident(ident("b"))),
                    span: Span::new(0, 0),
                })],
                span: Span::new(0, 0),
            },
            span: Span::new(0, 0),
        };

        let result = lower.lower_function(&func_def);
        assert!(result.is_ok());

        let ir_func = result.unwrap();
        assert_eq!(ir_func.name, "add");
        assert_eq!(ir_func.params.len(), 2);
        assert!(ir_func.return_type.is_some());
        assert_eq!(ir_func.blocks.len(), 1);

        // Check that the block has an add instruction
        let block = &ir_func.blocks[0];
        assert!(!block.instructions.is_empty());
        
        let add_inst = &block.instructions[0];
        assert!(matches!(add_inst.kind, IrInstructionKind::BinOp { op: IrBinOp::Add, .. }));
    }

    #[test]
    fn test_lower_literal() {
        let mut lower = Lower::new(TypeEnvironment::new());
        let mut block = IrBlock::new(BlockId(0));

        let result = lower.lower_literal(&Literal::Integer(42), &mut block);
        assert!(result.is_ok());

        assert_eq!(block.instructions.len(), 1);
        let inst = &block.instructions[0];
        assert!(matches!(
            inst.kind,
            IrInstructionKind::Const {
                value: IrConstant::Int(42)
            }
        ));
    }

    #[test]
    fn test_lower_struct_def() {
        let mut lower = Lower::new(TypeEnvironment::new());

        // Create a struct: Vec2(x Float, y Float)
        let struct_def = StructDef {
            visibility: Visibility::Public,
            name: ident("Vec2"),
            type_params: vec![],
            fields: vec![
                Field {
                    name: Some(ident("x")),
                    ty: Box::new(AstType::Named(ident("Float"))),
                    span: Span::new(0, 0),
                },
                Field {
                    name: Some(ident("y")),
                    ty: Box::new(AstType::Named(ident("Float"))),
                    span: Span::new(0, 0),
                },
            ],
            span: Span::new(0, 0),
        };

        let result = lower.lower_struct_def(&struct_def);
        assert!(result.is_ok());

        let ir_struct = result.unwrap();
        assert_eq!(ir_struct.name, "Vec2");
        assert_eq!(ir_struct.fields.len(), 2);
        assert_eq!(ir_struct.fields[0].0, "x");
        assert_eq!(ir_struct.fields[1].0, "y");
    }

    #[test]
    fn test_lower_complete_program() {
        let mut lower = Lower::new(TypeEnvironment::new());

        // Create a simple program with a struct and function
        let program = vec![
            TopLevel::Struct(StructDef {
                visibility: Visibility::Public,
                name: ident("Point"),
                type_params: vec![],
                fields: vec![
                    Field {
                        name: Some(ident("x")),
                        ty: int_type(),
                        span: Span::new(0, 0),
                    },
                    Field {
                        name: Some(ident("y")),
                        ty: int_type(),
                        span: Span::new(0, 0),
                    },
                ],
                span: Span::new(0, 0),
            }),
            TopLevel::Function(FunctionDef {
                visibility: Visibility::Public,
                name: ident("identity"),
                const_params: vec![],
                params: vec![Param {
                    name: ident("x"),
                    ty: Some(int_type()),
                    default: None,
                    span: Span::new(0, 0),
                }],
                return_type: Some(int_type()),
                body: Block {
                    stmts: vec![Stmt::Expression(Expr::Ident(ident("x")))],
                    span: Span::new(0, 0),
                },
                span: Span::new(0, 0),
            }),
        ];

        let result = lower.lower_program(program);
        assert!(result.is_ok());

        let ir_program = result.unwrap();
        assert_eq!(ir_program.structs.len(), 1);
        assert_eq!(ir_program.functions.len(), 1);
        assert_eq!(ir_program.structs[0].name, "Point");
        assert_eq!(ir_program.functions[0].name, "identity");
    }
}
