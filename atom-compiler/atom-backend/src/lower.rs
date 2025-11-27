#![allow(unused)]
#![allow(clippy::all)]

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
        if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
            eprintln!("DEBUG lower_program: received {} top-level items", ast.len());
        }
        
                let mut program = IrProgram::new();

        // First pass: collect all function definitions (for default parameters)
        for item in &ast {
            if let atom_ast::TopLevel::Function(func_def) = item {
                let name = func_def.name.name.clone();
                
                if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
                    if name == "first" || name == "unwrap" {
                        eprintln!("DEBUG lower_program: collecting function {}", name);
                        eprintln!("  params: {:?}", func_def.params.iter().map(|p| (&p.name.name, &p.ty)).collect::<Vec<_>>());
                        eprintln!("  return_type: {:?}", func_def.return_type);
                    }
                }
                
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
                                for (i, param) in func_def.params.iter().enumerate() {
                                    }
                
                // Skip generic functions (those with type parameters)
                if self.is_generic_function(func_def) {
                    if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
                        eprintln!("DEBUG: Skipping generic function: {}", func_def.name.name);
                    }
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
                        // Drain the current queue (clone to avoid borrow issues)
            let current_queue: Vec<_> = self.mono_queue.drain().collect();
            
            for (mono_name, (original_name, type_bindings, func_def)) in current_queue {
                                                // Create a modified function definition with the monomorphized name
                let mut specialized_func = func_def.clone();
                specialized_func.name.name = mono_name.clone();
                
                // Substitute type parameters in the AST before lowering
                specialized_func = self.substitute_type_params_in_ast(&specialized_func, &type_bindings)?;
                                // Lower the specialized function
                match self.lower_function(&specialized_func) {
                    Ok(ir_func) => {
                        // DEBUG: Dump IR for reduce monomorphizations (only when --debug flag is set)
                        if let Ok(debug_val) = std::env::var("ATOM_DEBUG") {
                            eprintln!("DEBUG: Checking function: name={}, mono_name={}", ir_func.name, mono_name);
                            if debug_val == "1" && (
                                mono_name.contains("unwrap") ||
                                (mono_name.contains("reduce") && (mono_name.contains("Float64") || mono_name.contains("Int64"))) || 
                                ir_func.name == "main"
                            ) {
                                eprintln!("\n========== IR FOR {} ==========", mono_name);
                                eprintln!("Function: {}", ir_func.name);
                                eprintln!("Params: {:?}", ir_func.params);
                                eprintln!("Return type: {:?}", ir_func.return_type);
                                eprintln!("Locals:");
                                for local in &ir_func.locals {
                                    eprintln!("  {:?}: name={}, ty={:?}", local.id, local.name, local.ty);
                                }
                                eprintln!("Blocks:");
                                for block in &ir_func.blocks {
                                    eprintln!("  Block {:?}:", block.label);
                                    for (idx, inst) in block.instructions.iter().enumerate() {
                                        eprintln!("    [{}] {:?}: {:?} = {:?}", idx, inst.result, inst.ty, inst.kind);
                                    }
                                    eprintln!("    Terminator: {:?}", block.terminator);
                                }
                                eprintln!("========================================\n");
                            }
                        }
                                                program.add_function(ir_func);
                    }
                    Err(e) => {
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

        // Evaluate constant initialization expression if present
        let init = if let Some(init_expr) = &decl.init {
            Some(self.eval_const_expr(init_expr)?)
        } else {
            None
        };

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

        let name = self.mangle_function_name(func_def);
        let is_public = matches!(func_def.visibility, Visibility::Public);
        let is_main = func_def.name.name == "main";

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
            // IMPORTANT: Check void return type FIRST
            if return_type.is_none() || return_type.as_ref().map_or(false, |rt| rt.is_void()) {
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
                    if ir_func.name.contains("print") {
                                            }
                    entry_block.set_terminator(IrTerminator::Return { value: None });
                }
            } else if let Some(value) = result_value {
                if ir_func.name.contains("print") {
                                    }
                entry_block.set_terminator(IrTerminator::Return { value: Some(value) });
            } else {
                return Err(LowerError::Internal(
                    "Function must return a value".to_string(),
                ));
            }
        } else {
            entry_block.set_terminator(terminator);
        }

        ir_func.add_block(entry_block);

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
            // Get the tuple type to extract element types
            let tuple_type = self.get_value_type(init_value, ir_block, func)
                .unwrap_or(IrType::Tuple(vec![]));
            
            let element_types = if let IrType::Tuple(types) = tuple_type {
                types
            } else {
                // Fallback: use Pointer(Void) for each element if we can't determine the type
                vec![IrType::Pointer(Box::new(IrType::Void)); decl.names.len()]
            };
            
            // Tuple destructuring: extract each element and bind to corresponding name
            for (index, var_name_ident) in decl.names.iter().enumerate() {
                let element_id = self.fresh_value_id();
                let element_type = element_types.get(index)
                    .cloned()
                    .unwrap_or(IrType::Pointer(Box::new(IrType::Void)));
                
                ir_block.add_instruction(IrInstruction {
                    result: element_id,
                    ty: element_type.clone(),
                    kind: IrInstructionKind::TupleExtract {
                        tuple: init_value,
                        index: index as u32,
                    },
                });
                self.variables.insert(
                    var_name_ident.name.clone(),
                    VarBinding::Value(element_id, element_type),
                );
            }
        } else {
            // Single variable binding
            let var_name = decl.names[0].name.clone();
            
            // Infer the type from the initializer if no type annotation
            let var_type = if let Some(ty_ast) = &decl.ty {
                self.lower_type(ty_ast)?
            } else {
                // Try to infer type from the initializer
                // First check if it's an instruction in the current block
                let mut inferred_type = None;
                for inst in &ir_block.instructions {
                    if inst.result == init_value {
                        inferred_type = Some(inst.ty.clone());
                        break;
                    }
                }
                // If not found in current block, try to get from parameters or other blocks
                if std::env::var("ATOM_DEBUG_VERIFY").is_ok() {
                                    }
                // CRITICAL FIX: Variable type MUST be inferred correctly for type safety.
                // If we cannot determine the type, this is an internal error that should be caught.
                inferred_type.or_else(|| {
                    let t = self.get_value_type(init_value, ir_block, func);
                    if std::env::var("ATOM_DEBUG_VERIFY").is_ok() {
                                            }
                    t
                }).ok_or_else(|| LowerError::Internal(
                    format!("Cannot infer type for variable '{}' from initializer (value {:?}). \
                            Type annotation required or initializer must have determinable type.",
                            var_name, init_value)
                ))?
            };

            // Check if this is a mutable variable (declared with :=)
            if !decl.is_const {
                // Mutable variable - allocate a local (stack slot) and store the initial value
                let local_id = self.fresh_local_id();
                
                if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
                    eprintln!("DEBUG lower_var_decl: '{}' is_const=false, creating Local({:?})", var_name, local_id);
                }
                
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
                if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
                    eprintln!("DEBUG lower_var_decl: '{}' is_const=true, creating Value({:?})", var_name, init_value);
                }
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
                // Check if block uses implicit closure parameters ($0, $1, etc.)
                let implicit_params = self.has_implicit_params(block);
                
                if !implicit_params.is_empty() {
                    if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
                        eprintln!("DEBUG lower_expr Block: treating as closure, implicit_params={:?}", implicit_params);
                    }
                    // Block with implicit params is treated as a closure
                    // Convert implicit params to explicit Param nodes
                    let params: Vec<atom_ast::Param> = implicit_params
                        .iter()
                        .map(|name| atom_ast::Param {
                            name: atom_ast::Ident {
                                name: name.clone(),
                                span: atom_ast::Span { start: 0, end: 0 },
                            },
                            ty: Some(Box::new(atom_ast::Type::Named(atom_ast::Ident {
                                name: "Int".to_string(),
                                span: atom_ast::Span { start: 0, end: 0 },
                            }))),
                            default: None,
                            span: atom_ast::Span { start: 0, end: 0 },
                        })
                        .collect();
                    
                    // Lower as a closure
                    self.lower_closure(&params, &None, block, ir_block, func)
                } else {
                    if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
                        eprintln!("DEBUG lower_expr Block: lowering as regular inline block");
                    }
                    // Regular block - lower inline
                    let (value, _) = self.lower_block_to_ir(block, ir_block, func)?;
                    value.ok_or_else(|| LowerError::Internal("Block must produce a value".to_string()))
                }
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
            atom_ast::Expr::Reference { expr, .. } => {
                // TODO: Implement proper reference semantics
                // For now, treat reference as the value itself (pointer/address)
                // This is a placeholder until full reference implementation
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
        
        if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
            eprintln!("DEBUG lower_literal: constant={:?}, value_id={:?}", constant, value_id);
        }
        
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
        
        if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") && name.starts_with('$') {
            eprintln!("DEBUG lower_ident: looking up '{}', found: {:?}", name, binding.is_some());
            eprintln!("DEBUG lower_ident: all variables: {:?}", self.variables.keys().collect::<Vec<_>>());
        }
        
        match binding {
            Some(VarBinding::Value(value_id, _)) => {
                if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") && name.starts_with('$') {
                    eprintln!("DEBUG lower_ident: '{}' found as Value({:?})", name, value_id);
                }
                Ok(value_id)
            }
            Some(VarBinding::Local(local_id, ty)) => {
                if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") && name.starts_with('$') {
                    eprintln!("DEBUG lower_ident: '{}' found as Local({:?})", name, local_id);
                }
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

                // Special handling for $0 (loop iteration variable) - only if not already bound
                // This is a fallback for when $0 is used but not properly bound by loop
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
                    
                    // FIX: Simple enums (without payload) are represented as plain integers (the tag value)
                    // This allows them to be passed by value and compared directly
                    // Create the enum tag as a constant with the Enum type
                    let tag_id = self.fresh_value_id();
                    ir_block.add_instruction(IrInstruction {
                        result: tag_id,
                        ty: IrType::Enum(enum_name_cloned),
                        kind: IrInstructionKind::Const {
                            value: IrConstant::Int(idx_value),
                        },
                    });
                    
                    Ok(tag_id)
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

        if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
            eprintln!("DEBUG lower_binary_op: op={:?}, left_value={:?}, right_value={:?}", op, left_value, right_value);
        }

        // Handle Concat operator specially for String ++ String and String ++ Rune
        if matches!(op, atom_ast::BinOp::Concat) {
            let left_type = self.get_value_type(left_value, ir_block, func)
                .unwrap_or(IrType::Pointer(Box::new(IrType::Void)));
            let right_type = self.get_value_type(right_value, ir_block, func)
                .unwrap_or(IrType::Pointer(Box::new(IrType::Void)));
            
            // Helper: check if a type is a string (either Pointer(Int(8)) or Struct("String"))
            let is_string_type = |ty: &IrType| -> bool {
                matches!(ty, IrType::Pointer(inner) if matches!(**inner, IrType::Int(8)))
                    || matches!(ty, IrType::Struct(name) if name == "String")
            };
            
            // String ++ String: call __builtin_string_concat
            if is_string_type(&left_type) && is_string_type(&right_type) {
                let result_id = self.fresh_value_id();
                ir_block.add_instruction(IrInstruction {
                    result: result_id,
                    ty: IrType::Pointer(Box::new(IrType::Int(8))),
                    kind: IrInstructionKind::Call {
                        function: "__builtin_string_concat".to_string(),
                        args: vec![left_value, right_value],
                        is_tail: false,
                    },
                });
                return Ok(result_id);
            }
            
            // String ++ Rune: call __builtin_append_rune_to_string
            if is_string_type(&left_type) && matches!(right_type, IrType::Rune) {
                let result_id = self.fresh_value_id();
                ir_block.add_instruction(IrInstruction {
                    result: result_id,
                    ty: IrType::Pointer(Box::new(IrType::Int(8))),
                    kind: IrInstructionKind::Call {
                        function: "__builtin_append_rune_to_string".to_string(),
                        args: vec![left_value, right_value],
                        is_tail: false,
                    },
                });
                return Ok(result_id);
            }
            
            // Rune ++ String: convert rune to string first, then concat
            if matches!(left_type, IrType::Rune) && is_string_type(&right_type) {
                // Convert rune to string first
                let rune_str_id = self.fresh_value_id();
                ir_block.add_instruction(IrInstruction {
                    result: rune_str_id,
                    ty: IrType::Pointer(Box::new(IrType::Int(8))),
                    kind: IrInstructionKind::Call {
                        function: "__builtin_rune_to_string".to_string(),
                        args: vec![left_value],
                        is_tail: false,
                    },
                });
                // Then concat the two strings
                let result_id = self.fresh_value_id();
                ir_block.add_instruction(IrInstruction {
                    result: result_id,
                    ty: IrType::Pointer(Box::new(IrType::Int(8))),
                    kind: IrInstructionKind::Call {
                        function: "__builtin_string_concat".to_string(),
                        args: vec![rune_str_id, right_value],
                        is_tail: false,
                    },
                });
                return Ok(result_id);
            }
            
            // Variadic tuple ++ variadic tuple (Array ++ Array): Int* ++ Int*
            if matches!(left_type, IrType::Array { .. }) && matches!(right_type, IrType::Array { .. }) {
                let result_id = self.fresh_value_id();
                ir_block.add_instruction(IrInstruction {
                    result: result_id,
                    ty: left_type.clone(),
                    kind: IrInstructionKind::ArrayConcat {
                        left: left_value,
                        right: right_value,
                    },
                });
                return Ok(result_id);
            }
            
            // Variadic tuple ++ element (Array ++ Element): Int* ++ Int
            if matches!(left_type, IrType::Array { .. }) {
                // For appending an element, we need to create a single-element array first
                // then concat it with the main array
                if let IrType::Array { element } = &left_type {
                    // Create a single-element array from the right value
                    let single_elem_array_id = self.fresh_value_id();
                    let single_elem_array_ty = IrType::Array { element: element.clone() };
                    ir_block.add_instruction(IrInstruction {
                        result: single_elem_array_id,
                        ty: single_elem_array_ty.clone(),
                        kind: IrInstructionKind::MakeTuple {
                            elements: vec![right_value],
                        },
                    });
                    
                    // Now concatenate the two arrays
                    let result_id = self.fresh_value_id();
                    ir_block.add_instruction(IrInstruction {
                        result: result_id,
                        ty: left_type.clone(),
                        kind: IrInstructionKind::ArrayConcat {
                            left: left_value,
                            right: single_elem_array_id,
                        },
                    });
                    return Ok(result_id);
                }
            }
            
            // Fixed tuple ++ fixed tuple: (Int, Float) ++ (String, Bool)
            if matches!(left_type, IrType::Tuple(_)) && matches!(right_type, IrType::Tuple(_)) {
                // For fixed tuples, concatenation means creating a new tuple with all elements
                // This requires extracting all elements from both tuples and making a new one
                if let (IrType::Tuple(left_types), IrType::Tuple(right_types)) = (&left_type, &right_type) {
                    let mut all_elements = Vec::new();
                    let mut all_types = Vec::new();
                    
                    // Extract all elements from left tuple
                    for (i, elem_ty) in left_types.iter().enumerate() {
                        let elem_id = self.fresh_value_id();
                        ir_block.add_instruction(IrInstruction {
                            result: elem_id,
                            ty: elem_ty.clone(),
                            kind: IrInstructionKind::TupleExtract {
                                tuple: left_value,
                                index: i as u32,
                            },
                        });
                        all_elements.push(elem_id);
                        all_types.push(elem_ty.clone());
                    }
                    
                    // Extract all elements from right tuple
                    for (i, elem_ty) in right_types.iter().enumerate() {
                        let elem_id = self.fresh_value_id();
                        ir_block.add_instruction(IrInstruction {
                            result: elem_id,
                            ty: elem_ty.clone(),
                            kind: IrInstructionKind::TupleExtract {
                                tuple: right_value,
                                index: i as u32,
                            },
                        });
                        all_elements.push(elem_id);
                        all_types.push(elem_ty.clone());
                    }
                    
                    // Create a new tuple with all elements
                    let result_id = self.fresh_value_id();
                    ir_block.add_instruction(IrInstruction {
                        result: result_id,
                        ty: IrType::Tuple(all_types),
                        kind: IrInstructionKind::MakeTuple {
                            elements: all_elements,
                        },
                    });
                    return Ok(result_id);
                }
            }
        }

        let ir_op = self.convert_binop(op)?;
        
        // Determine result type
        let result_type = match op {
            // Comparison operators always return Bool
            atom_ast::BinOp::Eq | atom_ast::BinOp::Ne | atom_ast::BinOp::Lt 
            | atom_ast::BinOp::Le | atom_ast::BinOp::Gt | atom_ast::BinOp::Ge => {
                IrType::Bool
            }
            // Arithmetic operators - infer from left operand type
            _ => {
                // CRITICAL FIX: Type MUST be known for arithmetic operations to ensure type safety.
                // Failing to determine the type could lead to incorrect codegen or runtime errors.
                self.get_value_type(left_value, ir_block, func)
                    .ok_or_else(|| LowerError::Internal(
                        format!("Cannot determine type for binary operation {:?} on value {:?}. \
                                Type information is required for correct code generation.", 
                                op, left_value)
                    ))?
            }
        };

        let value_id = self.fresh_value_id();
        
        if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
            eprintln!("DEBUG lower_binary_op: creating BinOp {:?}, left={:?}, right={:?}", ir_op, left_value, right_value);
        }
        
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
        // Implement proper short-circuit evaluation with control flow
        // For &&: if left is false, result is false (left_value), else evaluate right
        // For ||: if left is true, result is true (left_value), else evaluate right
        
        // Evaluate left operand in current block
        let left_value = self.lower_expr(left, ir_block, func)?;
        
        // Create basic blocks for control flow
        let right_block_id = self.fresh_block_id();
        let merge_block_id = self.fresh_block_id();
        
        // Set up the branch based on operator type
        match op {
            atom_ast::BinOp::And => {
                // For &&: if left is true, evaluate right; if false, skip to merge with left_value
                ir_block.set_terminator(IrTerminator::Branch {
                    condition: left_value,
                    true_block: right_block_id,
                    false_block: merge_block_id,
                });
            }
            atom_ast::BinOp::Or => {
                // For ||: if left is true, skip to merge with left_value; if false, evaluate right
                ir_block.set_terminator(IrTerminator::Branch {
                    condition: left_value,
                    true_block: merge_block_id,
                    false_block: right_block_id,
                });
            }
            _ => unreachable!(),
        }
        
        // Save and add the current block to the function
        let left_block_id = ir_block.label;
        let current_block = std::mem::replace(ir_block, IrBlock::new(merge_block_id));
        func.add_block(current_block);
        
        // Right block: evaluate right operand
        let mut right_block = IrBlock::new(right_block_id);
        let right_value = self.lower_expr(right, &mut right_block, func)?;
        let right_block_actual_id = right_block.label;
        right_block.set_terminator(IrTerminator::Jump { target: merge_block_id });
        func.add_block(right_block);
        
        // Merge block: use phi node to select result
        // ir_block is now the merge block (from mem::replace above)
        let result_id = self.fresh_value_id();
        
        // Create phi node with incoming values from both paths
        let phi_incoming = match op {
            atom_ast::BinOp::And => {
                // If left was false, use left_value (from left block)
                // If left was true, use right_value (from right block)
                vec![
                    (left_block_id, left_value),           // false path (skipped right)
                    (right_block_actual_id, right_value),  // true path (evaluated right)
                ]
            }
            atom_ast::BinOp::Or => {
                // If left was true, use left_value (from left block)
                // If left was false, use right_value (from right block)
                vec![
                    (left_block_id, left_value),           // true path (skipped right)
                    (right_block_actual_id, right_value),  // false path (evaluated right)
                ]
            }
            _ => unreachable!(),
        };
        
        ir_block.add_instruction(IrInstruction {
            result: result_id,
            ty: IrType::Bool,
            kind: IrInstructionKind::Phi { incoming: phi_incoming },
        });
        
        Ok(result_id)
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
        } else if matches!(op, atom_ast::BinOp::ConcatAssign) {
            // Compound concat assignment: var ++= expr
            // This is string/array concatenation
            let current_value = self.lower_ident(&var_name, ir_block, func)?;
            let right_value = self.lower_expr(right, ir_block, func)?;
            
            // Get the types to determine how to concat
            let left_type = self.get_value_type(current_value, ir_block, func)
                .unwrap_or(IrType::Pointer(Box::new(IrType::Void)));
            let right_type = self.get_value_type(right_value, ir_block, func)
                .unwrap_or(IrType::Pointer(Box::new(IrType::Void)));
            
            // Handle String ++= Rune
            if matches!(left_type, IrType::Struct(ref name) if name == "String") 
                && matches!(right_type, IrType::Int(32)) { // Rune is i32
                // Use __builtin_append_rune_to_string to append the rune's UTF-8 bytes
                let concat_id = self.fresh_value_id();
                ir_block.add_instruction(IrInstruction {
                    result: concat_id,
                    ty: IrType::Struct("String".to_string()),
                    kind: IrInstructionKind::Call {
                        function: "__builtin_append_rune_to_string".to_string(),
                        args: vec![current_value, right_value],
                        is_tail: false,
                    },
                });
                
                concat_id
            } else {
                // For other concat operations, treat as addition for now
                let result_id = self.fresh_value_id();
                ir_block.add_instruction(IrInstruction {
                    result: result_id,
                    ty: left_type,
                    kind: IrInstructionKind::BinOp {
                        op: IrBinOp::Add,
                        left: current_value,
                        right: right_value,
                    },
                });
                result_id
            }
        } else {
            // Compound assignment: var += expr, etc.
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

        // CRITICAL FIX: Result type must match operand type for correct semantics.
        // For example, negating a Float(32) should return Float(32), not Int(64).
        // Not operator returns Bool for boolean operands, preserves type for bitwise not.
        let result_type = if matches!(op, atom_ast::UnOp::Not) {
            // Logical not on boolean -> returns Bool
            // Get operand type to check if it's boolean
            let operand_type = self.get_value_type(operand, ir_block, func);
            match operand_type {
                Some(IrType::Bool) => IrType::Bool,
                Some(ty) => ty, // BitNot preserves type
                None => {
                    return Err(LowerError::Internal(
                        format!("Cannot determine type for unary operation {:?} on value {:?}. \
                                Type information is required for correct code generation.",
                                op, operand)
                    ));
                }
            }
        } else {
            // Neg and BitNot preserve the operand type
            self.get_value_type(operand, ir_block, func)
                .ok_or_else(|| LowerError::Internal(
                    format!("Cannot determine type for unary operation {:?} on value {:?}. \
                            Type information is required for correct code generation.",
                            op, operand)
                ))?
        };

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
        if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
            if let atom_ast::Expr::Ident(ident) = func_expr {
                if ident.name == "first" || ident.name == "unwrap" {
                    eprintln!("DEBUG lower_call: func={}, args.len={}", ident.name, args.len());
                }
            }
        }
        
        // Handle field access followed by call (e.g., s.bytes(i))
        // This is typically array/pointer indexing, not a function call
        if let atom_ast::Expr::FieldAccess { object, field, .. } = func_expr {
            if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
                eprintln!("DEBUG lower_call: FieldAccess detected, field={}, args.len={}", field.name, args.len());
            }
            if args.len() == 1 && field.name == "bytes" {
                if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
                    eprintln!("DEBUG lower_call: Handling s.bytes(i) as field indexing");
                }
                // This is s.bytes(i) - field access followed by array indexing
                let object_value = self.lower_expr(object, ir_block, func)?;
                
                // Extract the bytes field (field 0 of String tuple)
                let field_value = self.fresh_value_id();
                ir_block.add_instruction(IrInstruction {
                    result: field_value,
                    ty: IrType::Pointer(Box::new(IrType::Int(8))),
                    kind: IrInstructionKind::TupleExtract {
                        tuple: object_value,
                        index: 0,
                    },
                });
                
                // Index into the array
                let index_value = self.lower_expr(&args[0], ir_block, func)?;
                
                let value_id = self.fresh_value_id();
                ir_block.add_instruction(IrInstruction {
                    result: value_id,
                    ty: IrType::Int(8),
                    kind: IrInstructionKind::ArrayIndex {
                        array: field_value,
                        index: index_value,
                    },
                });
                return Ok(value_id);
            }
        }
        
        // Get function name
        let func_name = match func_expr {
            atom_ast::Expr::Ident(ident) => {
                if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") && ident.name == "bytes" {
                    eprintln!("DEBUG lower_call: Found direct 'bytes' function call with {} args", args.len());
                }
                ident.name.clone()
            }
            _ => {
                return Err(LowerError::Unsupported(
                    "Indirect function calls".to_string(),
                ))
            }
        };

        // Check if this is actually array indexing (variable call with one argument)
        // In Atom, arr(i) is array indexing if arr is a variable/parameter holding an array
        if args.len() == 1 {
            if func_name == "rune_arr" {
                            }
            // Try to find the identifier as a variable or parameter
            let mut is_indexable = false;
            let mut array_value = None;
            let mut element_type = IrType::Int(64); // Default
            
            // Check in variables (both SSA values and locals)
            match self.variables.get(&func_name).cloned() {
                Some(VarBinding::Value(val_id, ty)) => {
                    // Check if it's an indexable type (not a closure, not a function)
                    match &ty {
                        IrType::Closure { .. } | IrType::Function { .. } => {
                            // This is a function/closure call, not array indexing
                            // Fall through to handle it normally
                        }
                        IrType::Array { element } => {
                            is_indexable = true;
                            array_value = Some(val_id);
                            element_type = (**element).clone();
                        }
                        IrType::Pointer(inner) => {
                            // Pointers can be indexed
                            is_indexable = true;
                            array_value = Some(val_id);
                            element_type = (**inner).clone();
                        }
                        IrType::Tuple(elements) if !elements.is_empty() => {
                            // Tuples can be indexed
                            is_indexable = true;
                            array_value = Some(val_id);
                            // For tuples, assume all elements are the same type (first element type)
                            element_type = elements[0].clone();
                        }
                        _ => {
                            // Other types might be indexable, try it
                            is_indexable = true;
                            array_value = Some(val_id);
                        }
                    }
                }
                Some(VarBinding::Local(local_id, ty)) => {
                    // Variable stored in a local - need to load it first
                    if func_name == "rune_arr" {
                                            }
                    match &ty {
                        IrType::Closure { .. } | IrType::Function { .. } => {
                            // This is a function/closure call, not array indexing
                            // Fall through to handle it normally
                            if func_name == "rune_arr" {
                                                            }
                        }
                        IrType::Array { element } => {
                            is_indexable = true;
                            // Load from local
                            let loaded_val = self.fresh_value_id();
                            ir_block.add_instruction(IrInstruction {
                                result: loaded_val,
                                ty: ty.clone(),
                                kind: IrInstructionKind::Load {
                                    source: IrMemoryLocation::Local(local_id),
                                },
                            });
                            array_value = Some(loaded_val);
                            element_type = (**element).clone();
                        }
                        IrType::Pointer(inner) => {
                            // Pointers can be indexed
                            is_indexable = true;
                            // Load from local
                            let loaded_val = self.fresh_value_id();
                            ir_block.add_instruction(IrInstruction {
                                result: loaded_val,
                                ty: ty.clone(),
                                kind: IrInstructionKind::Load {
                                    source: IrMemoryLocation::Local(local_id),
                                },
                            });
                            array_value = Some(loaded_val);
                            element_type = (**inner).clone();
                        }
                        IrType::Tuple(elements) if !elements.is_empty() => {
                            // Tuples can be indexed
                            is_indexable = true;
                            // Load from local
                            let loaded_val = self.fresh_value_id();
                            ir_block.add_instruction(IrInstruction {
                                result: loaded_val,
                                ty: ty.clone(),
                                kind: IrInstructionKind::Load {
                                    source: IrMemoryLocation::Local(local_id),
                                },
                            });
                            array_value = Some(loaded_val);
                            // For tuples, assume all elements are the same type (first element type)
                            element_type = elements[0].clone();
                        }
                        _ => {
                            // Try indexing anyway
                            is_indexable = true;
                            let loaded_val = self.fresh_value_id();
                            ir_block.add_instruction(IrInstruction {
                                result: loaded_val,
                                ty: ty.clone(),
                                kind: IrInstructionKind::Load {
                                    source: IrMemoryLocation::Local(local_id),
                                },
                            });
                            array_value = Some(loaded_val);
                        }
                    }
                }
                None => {
                    // Check if it's a function parameter
                    for (param_idx, (param_name, param_ty)) in func.params.iter().enumerate() {
                        if param_name == &func_name {
                            // Found it as a parameter
                            match param_ty {
                                IrType::Closure { .. } | IrType::Function { .. } => {
                                    // Function/closure parameter, not array indexing
                                }
                                IrType::Array { element } => {
                                    is_indexable = true;
                                    // Parameters are the first ValueIds
                                    let param_value = ValueId(param_idx as u32);
                                    array_value = Some(param_value);
                                    element_type = (**element).clone();
                                }
                                IrType::Pointer(inner) => {
                                    is_indexable = true;
                                    let param_value = ValueId(param_idx as u32);
                                    array_value = Some(param_value);
                                    element_type = (**inner).clone();
                                }
                                _ => {
                                    // Try indexing anyway
                                    is_indexable = true;
                                    let param_value = ValueId(param_idx as u32);
                                    array_value = Some(param_value);
                                }
                            }
                            break;
                        }
                    }
                }
            }
            
            // If we found an indexable array/pointer, emit ArrayIndex
            if is_indexable {
                if let Some(arr_val) = array_value {
                    let index_value = self.lower_expr(&args[0], ir_block, func)?;
                    
                    let value_id = self.fresh_value_id();
                    ir_block.add_instruction(IrInstruction {
                        result: value_id,
                        ty: element_type,
                        kind: IrInstructionKind::ArrayIndex {
                            array: arr_val,
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
        
        // Save the original argument count before adding defaults
        // This is needed for proper overload resolution throughout
        let original_arg_count = arg_values.len();
        
        // Fill in default parameters if needed
        // Look up the function definition to get default values
        // We need to collect the default expressions first to avoid borrow issues
        let mut default_exprs = Vec::new();
        if let Some(func_defs) = self.function_defs.get(&func_name) {
            // Select the correct overload based on ORIGINAL argument count
            if let Some(func_def) = self.select_overload(func_defs, original_arg_count) {
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
            // Get the function parameter's type to determine the return type
            let return_ty = func.params.iter()
                .enumerate()
                .find(|(i, _)| ValueId(*i as u32) == func_value_id)
                .and_then(|(_, (_, param_ty))| match param_ty {
                    IrType::Function { return_type, .. } => {
                        (**return_type).clone().or(Some(IrType::Void))
                    }
                    IrType::Closure { return_type, .. } => {
                        (**return_type).clone().or(Some(IrType::Void))
                    }
                    _ => None,
                })
                .unwrap_or(IrType::Int(64)); // Fallback
            
            let value_id = self.fresh_value_id();
            ir_block.add_instruction(IrInstruction {
                result: value_id,
                ty: return_ty,
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
        // Also handle function overloading by mangling names
        // Store concrete types for later return type substitution
        let mut type_substitutions: HashMap<String, IrType> = HashMap::new();
        
        let actual_func_name = if let Some(func_defs) = self.function_defs.get(&func_name) {
            if let Some(func_def) = self.select_overload(func_defs, original_arg_count) {
                if self.is_generic_function(func_def) {
                    // This is a generic function call - we need to monomorphize it
                                        // Extract concrete types from arguments
                    let concrete_types = self.extract_concrete_types(&arg_values, ir_block, func, func_def);
                    
                    // DEBUG: Show what we extracted
                    if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
                        eprintln!("DEBUG MONO: func={}, concrete_types={:?}", func_name, concrete_types);
                    }
                    
                                        // Generate monomorphized function name
                                        let mono_name = self.generate_mono_name(&func_name, &concrete_types, func_def);
                    
                    // DEBUG: Show the generated name
                    if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
                        eprintln!("DEBUG MONO: func={}, mono_name={}", func_name, mono_name);
                    }
                    
                    // Store type substitutions for return type processing
                    type_substitutions = concrete_types.clone();
                    
                                                            // Queue this instance for generation if not already done
                    if !self.mono_done.contains(&mono_name) {
                                                self.mono_queue.insert(
                            mono_name.clone(),
                            (func_name.clone(), concrete_types, func_def.clone()),
                        );
                        self.mono_done.insert(mono_name.clone());
                    }
                    
                    mono_name
                } else {
                    // Not generic, but may need name mangling for overloading
                    self.mangle_function_name(func_def)
                }
            } else {
                func_name.clone()
            }
        } else {
            func_name.clone()
        };

        // Determine return type
        // Check if this is a C library function call
        let (adjusted_func_name, return_type) = if actual_func_name.starts_with('c') && actual_func_name.contains("::") {
            // For C math functions, adjust the function name based on argument types
            let parts: Vec<&str> = actual_func_name.split("::").collect();
            if parts.len() == 2 && parts[0] == "cmath" {
                let c_func_name = parts[1];
                
                // Check if this is a float-polymorphic function
                // For math.h functions, use 'f' suffix for f32 (e.g., sinf)
                // For our runtime wrappers, use '_f32' suffix (e.g., __atom_isnan_f32)
                let needs_variant_suffix = matches!(
                    c_func_name,
                    "sin" | "cos" | "tan" | 
                    "asin" | "acos" | "atan" | "sinh" | "cosh" | "tanh" |
                    "exp" | "log" | "log10" | "sqrt" | "ceil" | "floor" | "fabs" |
                    "isnan" | "isinf" | "isfinite"
                );
                
                if needs_variant_suffix && !arg_values.is_empty() {
                    // Check the first argument type
                    let first_arg_type = self.get_value_type(arg_values[0], ir_block, func);
                    let use_f32_variant = matches!(first_arg_type, Some(IrType::Float(32)));
                    
                    if use_f32_variant {
                        // For is* functions from our runtime, use _f32 suffix
                        // For standard math.h functions, use 'f' suffix
                        let suffix = if matches!(c_func_name, "isnan" | "isinf" | "isfinite") {
                            "_f32"
                        } else {
                            "f"
                        };
                        let adjusted_name = format!("{}::{}{}", parts[0], c_func_name, suffix);
                        (adjusted_name, self.infer_c_function_return_type(&actual_func_name))
                    } else {
                        // Use the regular version
                        (actual_func_name.clone(), self.infer_c_function_return_type(&actual_func_name))
                    }
                } else {
                    (actual_func_name.clone(), self.infer_c_function_return_type(&actual_func_name))
                }
            } else {
                (actual_func_name.clone(), self.infer_c_function_return_type(&actual_func_name))
            }
        } else {
            // Look up the function definition to get the actual return type
            // NOTE: Use original func_name, not actual_func_name (which may be monomorphized)
            let ret_ty_ast_opt = if let Some(func_defs) = self.function_defs.get(&func_name) {
                if let Some(func_def) = self.select_overload(func_defs, original_arg_count) {
                    func_def.return_type.clone()
                } else {
                    None
                }
            } else {
                None
            };
            
            let return_ty = if let Some(ret_ty_ast) = ret_ty_ast_opt {
                // If this was a generic function, substitute type params in return type
                if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
                    eprintln!("[MONO-RET] func_name={}, type_substitutions={:?}, ret_ty_ast={:?}", 
                        func_name, type_substitutions, ret_ty_ast);
                }
                let substituted_ast = if !type_substitutions.is_empty() {
                    if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
                        eprintln!("[MONO] Substituting return type AST with bindings: {:?}", type_substitutions);
                        eprintln!("[MONO] Original return type AST: {:?}", ret_ty_ast);
                    }
                    let result = self.substitute_ast_type(&ret_ty_ast, &type_substitutions)?;
                    if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
                        eprintln!("[MONO] Substituted return type AST: {:?}", result);
                    }
                    result
                } else {
                    (*ret_ty_ast).clone()
                };
                // Convert the return type from AST to IR type
                let lowered = self.lower_type(&substituted_ast)?;
                if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
                    eprintln!("[MONO-RET] func_name={}, lowered return type={:?}", func_name, lowered);
                }
                lowered
            } else {
                // No return type specified or function not found, use fallback
                IrType::Int(64)
            };
            
            (actual_func_name, return_ty)
        };

        let value_id = self.fresh_value_id();
        ir_block.add_instruction(IrInstruction {
            result: value_id,
            ty: return_type,
            kind: IrInstructionKind::Call {
                function: adjusted_func_name,
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
            // isnan, isinf, isfinite return int (32-bit), not float
            "isnan" | "isnanf" | "isinf" | "isinff" | "isfinite" | "isfinitef" => IrType::Int(32),
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

        // Get the type of the value being converted
        let value_type = self.get_value_type(value, ir_block, func)
            .ok_or_else(|| LowerError::Internal(
                format!("Could not determine type for as_string argument (ValueId({}))", value.0)
            ))?;

        // Dispatch to appropriate conversion function based on type
        match &value_type {
            // All integer types -> call __builtin_int_to_string
            // The C function accepts int64_t and Cranelift will handle the type widening
            IrType::Int(_) | IrType::UInt(_) => {
                // Call __builtin_int_to_string
                let result_id = self.fresh_value_id();
                ir_block.add_instruction(IrInstruction {
                    result: result_id,
                    ty: IrType::Pointer(Box::new(IrType::Int(8))),
                    kind: IrInstructionKind::Call {
                        function: "__builtin_int_to_string".to_string(),
                        args: vec![value],
                        is_tail: false,
                    },
                });
                Ok(result_id)
            }
            
            // Floating point -> call __builtin_float_to_string
            // The C function accepts double and Cranelift will handle the type widening
            IrType::Float(_) => {
                // Call __builtin_float_to_string
                let result_id = self.fresh_value_id();
                ir_block.add_instruction(IrInstruction {
                    result: result_id,
                    ty: IrType::Pointer(Box::new(IrType::Int(8))),
                    kind: IrInstructionKind::Call {
                        function: "__builtin_float_to_string".to_string(),
                        args: vec![value],
                        is_tail: false,
                    },
                });
                Ok(result_id)
            }
            
            // Boolean -> call __builtin_bool_to_string
            IrType::Bool => {
                let result_id = self.fresh_value_id();
                ir_block.add_instruction(IrInstruction {
                    result: result_id,
                    ty: IrType::Pointer(Box::new(IrType::Int(8))),
                    kind: IrInstructionKind::Call {
                        function: "__builtin_bool_to_string".to_string(),
                        args: vec![value],
                        is_tail: false,
                    },
                });
                Ok(result_id)
            }
            
            // Rune -> call __builtin_rune_to_string
            IrType::Rune => {
                let result_id = self.fresh_value_id();
                ir_block.add_instruction(IrInstruction {
                    result: result_id,
                    ty: IrType::Pointer(Box::new(IrType::Int(8))),
                    kind: IrInstructionKind::Call {
                        function: "__builtin_rune_to_string".to_string(),
                        args: vec![value],
                        is_tail: false,
                    },
                });
                Ok(result_id)
            }
            
            // String (already a string - no conversion needed, just return it)
            IrType::Pointer(inner) if matches!(**inner, IrType::Int(8)) => {
                // It's already a char*, just return the value
                Ok(value)
            }
            
            // Arrays of i8 (also strings)
            IrType::Array { element } if matches!(**element, IrType::Int(8)) => {
                // Array of char - already a string
                Ok(value)
            }
            
            // Tuples: (elem1, elem2, ...)
            IrType::Tuple(element_types) => {
                self.lower_as_string_tuple(value, element_types, ir_block, func)
            }
            
            // Structs: StructName(field1: value1, field2: value2, ...)
            IrType::Struct(struct_name) => {
                self.lower_as_string_struct(value, struct_name, ir_block, func)
            }
            
            // Enums: CaseName or CaseName(field1, field2, ...)
            IrType::Enum(enum_name) => {
                self.lower_as_string_enum(value, enum_name, ir_block, func)
            }
            
            // TODO: Implement composite types (struct, enum, tuple)
            _ => {
                Err(LowerError::Unsupported(
                    format!("as_string not yet implemented for type: {}", value_type)
                ))
            }
        }
    }

    /// Helper: Create a string literal as a ValueId
    fn make_string_literal(&mut self, s: &str, ir_block: &mut IrBlock) -> ValueId {
        let const_id = self.fresh_value_id();
        ir_block.add_instruction(IrInstruction {
            result: const_id,
            ty: IrType::Pointer(Box::new(IrType::Int(8))),
            kind: IrInstructionKind::Const {
                value: IrConstant::String(s.as_bytes().to_vec()),
            },
        });
        const_id
    }
    
    /// Helper: Concatenate two string ValueIds
    fn concat_strings(&mut self, left: ValueId, right: ValueId, ir_block: &mut IrBlock) -> ValueId {
        let result_id = self.fresh_value_id();
        ir_block.add_instruction(IrInstruction {
            result: result_id,
            ty: IrType::Pointer(Box::new(IrType::Int(8))),
            kind: IrInstructionKind::Call {
                function: "__builtin_string_concat".to_string(),
                args: vec![left, right],
                is_tail: false,
            },
        });
        result_id
    }
    
    /// Lower as_string for tuple types: (elem1, elem2, ...)
    fn lower_as_string_tuple(
        &mut self,
        tuple_value: ValueId,
        element_types: &[IrType],
        ir_block: &mut IrBlock,
        func: &mut IrFunction,
    ) -> LowerResult<ValueId> {
        // Start with "("
        let mut result = self.make_string_literal("(", ir_block);
        
        for (index, elem_type) in element_types.iter().enumerate() {
            // Add ", " separator for elements after the first
            if index > 0 {
                let separator = self.make_string_literal(", ", ir_block);
                result = self.concat_strings(result, separator, ir_block);
            }
            
            // Extract the element
            let elem_value = self.fresh_value_id();
            ir_block.add_instruction(IrInstruction {
                result: elem_value,
                ty: elem_type.clone(),
                kind: IrInstructionKind::TupleExtract {
                    tuple: tuple_value,
                    index: index as u32,
                },
            });
            
            // Recursively convert element to string
            let elem_str = self.lower_as_string_value(elem_value, elem_type, ir_block, func)?;
            
            // Concatenate to result
            result = self.concat_strings(result, elem_str, ir_block);
        }
        
        // Close with ")"
        let close_paren = self.make_string_literal(")", ir_block);
        result = self.concat_strings(result, close_paren, ir_block);
        
        Ok(result)
    }
    
    /// Lower as_string for struct types: StructName(field1: value1, field2: value2, ...)
    fn lower_as_string_struct(
        &mut self,
        struct_value: ValueId,
        struct_name: &str,
        ir_block: &mut IrBlock,
        func: &mut IrFunction,
    ) -> LowerResult<ValueId> {
        // Look up the struct definition
        let struct_def = self.type_env.get_struct(struct_name)
            .ok_or_else(|| LowerError::UndefinedStruct(struct_name.to_string()))?
            .clone();
        
        // Special case: String type should just return the bytes field directly
        if struct_name == "String" && struct_def.fields.len() == 1 && struct_def.fields[0].name == "bytes" {
            // Extract the bytes field (char*)
            let bytes_value = self.fresh_value_id();
            let bytes_ir_type = IrType::Pointer(Box::new(IrType::Int(8)));
            ir_block.add_instruction(IrInstruction {
                result: bytes_value,
                ty: bytes_ir_type,
                kind: IrInstructionKind::StructExtract {
                    struct_value,
                    field_index: 0,
                },
            });
            return Ok(bytes_value);
        }
        
        // Start with "StructName("
        let open_str = format!("{}(", struct_name);
        let mut result = self.make_string_literal(&open_str, ir_block);
        
        for (index, field) in struct_def.fields.iter().enumerate() {
            // Add ", " separator for fields after the first
            if index > 0 {
                let separator = self.make_string_literal(", ", ir_block);
                result = self.concat_strings(result, separator, ir_block);
            }
            
            // Add "fieldname: "
            let field_label = format!("{}: ", field.name);
            let field_label_str = self.make_string_literal(&field_label, ir_block);
            result = self.concat_strings(result, field_label_str, ir_block);
            
            // Extract the field value
            // TODO: Convert backend Type to IrType properly - for now use Pointer as fallback
            let field_value = self.fresh_value_id();
            let field_ir_type = IrType::Pointer(Box::new(IrType::Int(8)));
            ir_block.add_instruction(IrInstruction {
                result: field_value,
                ty: field_ir_type.clone(),
                kind: IrInstructionKind::StructExtract {
                    struct_value,
                    field_index: index as u32,
                },
            });
            
            // Recursively convert field to string
            let field_str = self.lower_as_string_value(field_value, &field_ir_type, ir_block, func)?;
            
            // Concatenate to result
            result = self.concat_strings(result, field_str, ir_block);
        }
        
        // Close with ")"
        let close_paren = self.make_string_literal(")", ir_block);
        result = self.concat_strings(result, close_paren, ir_block);
        
        Ok(result)
    }
    
    /// Lower as_string for enum types: CaseName or CaseName(field1, field2, ...)
    fn lower_as_string_enum(
        &mut self,
        enum_value: ValueId,
        enum_name: &str,
        ir_block: &mut IrBlock,
        func: &mut IrFunction,
    ) -> LowerResult<ValueId> {
        // Look up the enum definition
        let enum_def = self.type_env.get_enum(enum_name)
            .ok_or_else(|| LowerError::UndefinedEnum(enum_name.to_string()))?
            .clone();
        
        // FIX: Enums are now plain integers (the tag value), not tuples
        // So we can use the enum_value directly as the tag
        let tag_value = enum_value;
        
        // Create a block for each variant and a merge block
        let merge_block_id = self.fresh_block_id();
        let mut variant_blocks = Vec::new();
        let mut variant_strings = Vec::new();
        
        for (variant_index, case) in enum_def.cases.iter().enumerate() {
            let variant_name = &case.name;
            let variant_block_id = self.fresh_block_id();
            let mut variant_block = IrBlock::new(variant_block_id);
            
            // For now, just create a string with the variant name
            // TODO: Extract and format variant fields if they exist
            let variant_str = self.make_string_literal(variant_name, &mut variant_block);
            
            // Jump to merge block
            variant_block.set_terminator(IrTerminator::Jump { target: merge_block_id });
            let variant_block_label = variant_block.label;
            
            variant_blocks.push((variant_index as u32, variant_block_id, variant_block, variant_block_label));
            variant_strings.push(variant_str);
        }
        
        // Create switch on the tag
        let cases: Vec<(u32, BlockId)> = variant_blocks
            .iter()
            .map(|(tag, block_id, _, _)| (*tag, *block_id))
            .collect();
        
        let default_block_id = variant_blocks.first()
            .map(|(_, block_id, _, _)| *block_id)
            .unwrap_or(merge_block_id);
        
        ir_block.set_terminator(IrTerminator::Switch {
            value: tag_value,
            cases,
            default: default_block_id,
        });
        
        // Save and add the current block
        let current_block = std::mem::replace(ir_block, IrBlock::new(merge_block_id));
        func.add_block(current_block);
        
        // Add all variant blocks
        for (_, _, block, _) in &variant_blocks {
            func.add_block(block.clone());
        }
        
        // Create merge block with phi node
        let result_value = self.fresh_value_id();
        let incoming: Vec<(BlockId, ValueId)> = variant_blocks
            .iter()
            .enumerate()
            .map(|(i, (_, _, _, block_label))| (*block_label, variant_strings[i]))
            .collect();
        
        ir_block.add_instruction(IrInstruction {
            result: result_value,
            ty: IrType::Pointer(Box::new(IrType::Int(8))),
            kind: IrInstructionKind::Phi { incoming },
        });
        
        Ok(result_value)
    }
    
    /// Helper: Convert a value of known type to string (used for recursive calls)
    fn lower_as_string_value(
        &mut self,
        value: ValueId,
        value_type: &IrType,
        ir_block: &mut IrBlock,
        func: &mut IrFunction,
    ) -> LowerResult<ValueId> {
        // Similar to lower_as_string_builtin but takes a ValueId and IrType directly
        match value_type {
            IrType::Int(_) | IrType::UInt(_) => {
                let result_id = self.fresh_value_id();
                ir_block.add_instruction(IrInstruction {
                    result: result_id,
                    ty: IrType::Pointer(Box::new(IrType::Int(8))),
                    kind: IrInstructionKind::Call {
                        function: "__builtin_int_to_string".to_string(),
                        args: vec![value],
                        is_tail: false,
                    },
                });
                Ok(result_id)
            }
            
            IrType::Float(_) => {
                let result_id = self.fresh_value_id();
                ir_block.add_instruction(IrInstruction {
                    result: result_id,
                    ty: IrType::Pointer(Box::new(IrType::Int(8))),
                    kind: IrInstructionKind::Call {
                        function: "__builtin_float_to_string".to_string(),
                        args: vec![value],
                        is_tail: false,
                    },
                });
                Ok(result_id)
            }
            
            IrType::Bool => {
                let result_id = self.fresh_value_id();
                ir_block.add_instruction(IrInstruction {
                    result: result_id,
                    ty: IrType::Pointer(Box::new(IrType::Int(8))),
                    kind: IrInstructionKind::Call {
                        function: "__builtin_bool_to_string".to_string(),
                        args: vec![value],
                        is_tail: false,
                    },
                });
                Ok(result_id)
            }
            
            IrType::Rune => {
                let result_id = self.fresh_value_id();
                ir_block.add_instruction(IrInstruction {
                    result: result_id,
                    ty: IrType::Pointer(Box::new(IrType::Int(8))),
                    kind: IrInstructionKind::Call {
                        function: "__builtin_rune_to_string".to_string(),
                        args: vec![value],
                        is_tail: false,
                    },
                });
                Ok(result_id)
            }
            
            IrType::Pointer(inner) if matches!(**inner, IrType::Int(8)) => {
                Ok(value)
            }
            
            IrType::Tuple(element_types) => {
                self.lower_as_string_tuple(value, element_types, ir_block, func)
            }
            
            IrType::Struct(struct_name) => {
                self.lower_as_string_struct(value, struct_name, ir_block, func)
            }
            
            IrType::Enum(enum_name) => {
                self.lower_as_string_enum(value, enum_name, ir_block, func)
            }
            
            _ => Err(LowerError::Unsupported(
                format!("as_string not yet implemented for nested type: {}", value_type)
            ))
        }
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
        // Array iteration: loop(expr) where expr evaluates to an array/variadic
        // Condition loop: loop(expr) where expr is a boolean expression
        // For now, we use a simple heuristic: if the first arg is an Ident or MethodCall,
        // we treat it as array iteration; otherwise it's a condition loop
        let is_array_iter = match first_arg {
            Expr::Ident(_) => true,
            Expr::MethodCall { .. } => true,
            _ => false,
        };
        
        if let Some(debug) = std::env::var("ATOM_DEBUG").ok() {
            if debug == "1" {
                eprintln!("DEBUG loop_builtin: is_array_iter={}, first_arg={:?}", is_array_iter, first_arg);
            }
        }
        
        if is_array_iter {
            // Array iteration: loop(arr) { body with $0 }
            // Implement as: for i in 0..len(arr) { $0 = arr[i]; body }
            
            if let Some(debug) = std::env::var("ATOM_DEBUG").ok() {
                if debug == "1" {
                    eprintln!("DEBUG loop: is_array_iter=true, first_arg={:?}", first_arg);
                }
            }
            
            // Get the array value
            let array_value = self.lower_expr(first_arg, current_block, func)?;
            
            // Get array length
            // Try to find the type of the array value
            // First check if it's a variable (including parameters) in self.variables
            let array_ty = if let Expr::Ident(ident) = first_arg {
                // Look up the variable to get its type
                let ty = self.variables.get(&ident.name).and_then(|binding| {
                    match binding {
                        VarBinding::Value(_, ty) | VarBinding::Local(_, ty) => Some(ty.clone()),
                    }
                });
                if let Some(debug) = std::env::var("ATOM_DEBUG").ok() {
                    if debug == "1" {
                        eprintln!("DEBUG loop: ident={}, found_type={:?}", ident.name, ty);
                    }
                }
                ty
            } else {
                // Otherwise, find the instruction that produced this value
                current_block
                    .instructions
                    .iter()
                    .find(|inst| inst.result == array_value)
                    .map(|inst| inst.ty.clone())
            };
            
            // Create a local to store the array length (needed for dominance in loop header)
            let len_local = func.add_local("$array_len".to_string(), IrType::Int(64));
            let len_value = self.fresh_value_id();
            
            // Check if this is a fixed-size tuple - if so, use compile-time length
            if let Some(IrType::Tuple(ref element_types)) = array_ty {
                if let Some(debug) = std::env::var("ATOM_DEBUG").ok() {
                    if debug == "1" {
                        eprintln!("DEBUG loop: Using compile-time tuple length = {}", element_types.len());
                    }
                }

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
            // Determine the element type from the array type
            let element_type = if let Some(arr_ty) = array_ty {
                match arr_ty {
                    IrType::Array { element } => (*element).clone(),
                    IrType::Tuple(elements) if !elements.is_empty() => {
                        // For tuples, use the first element type
                        elements[0].clone()
                    }
                    IrType::Pointer(inner) => (*inner).clone(),
                    _ => IrType::Int(64), // Fallback
                }
            } else {
                // Try to get the type from the array value itself
                self.get_value_type(array_value, current_block, func)
                    .and_then(|ty| match ty {
                        IrType::Array { element } => Some((*element).clone()),
                        IrType::Tuple(ref elements) if !elements.is_empty() => Some(elements[0].clone()),
                        IrType::Pointer(inner) => Some((*inner).clone()),
                        _ => None,
                    })
                    .unwrap_or(IrType::Int(64)) // Fallback
            };
            
            let element_value = self.fresh_value_id();
            loop_body_ir.add_instruction(IrInstruction {
                result: element_value,
                ty: element_type.clone(),
                kind: IrInstructionKind::ArrayIndex {
                    array: array_value,
                    index: body_index,
                },
            });
            
            // Store $0 in a local variable so it's accessible in nested blocks
            let dollar0_local = func.add_local("$0".to_string(), element_type.clone());
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
            self.variables.insert("$0".to_string(), VarBinding::Local(dollar0_local, element_type.clone()));
            
            if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
                eprintln!("DEBUG loop: bound $0 as Local({:?}), all variables: {:?}", dollar0_local, self.variables.keys().collect::<Vec<_>>());
            }
            
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

        // Check if this is field access followed by indexing (e.g., s.bytes(i))
        // This pattern occurs when:
        // 1. Method call has exactly one argument (the index)
        // 2. The "method" name corresponds to a field of the receiver's struct type
        //
        // IMPORTANT: We check the type first WITHOUT lowering to avoid double-lowering
        // the receiver expression. If it's not field indexing, we fall through to the
        // general method call case which will lower the receiver once.
        if args.len() == 1 {
            // First, try to determine if this could be field indexing by checking
            // if the receiver is a simple identifier we can look up
            let mut is_field_indexing = false;
            let mut receiver_type_for_check: Option<IrType> = None;
            
            // Only attempt the optimization if receiver is a simple identifier
            // This avoids lowering complex expressions twice
            if let atom_ast::Expr::Ident(ident) = receiver {
                // Look up the variable type without lowering
                // Check locals first
                if let Some(local) = func.locals.iter()
                    .find(|local| local.name == ident.name) 
                {
                    receiver_type_for_check = Some(local.ty.clone());
                }
                // If not found in locals, check function parameters
                else if let Some((_, param_ty)) = func.params.iter()
                    .find(|(param_name, _)| param_name == &ident.name)
                {
                    receiver_type_for_check = Some(param_ty.clone());
                }
            }
            
            // If we found a type, check if method_name is a field
            if let Some(receiver_type) = receiver_type_for_check {
                if let IrType::Struct(struct_name) = receiver_type {
                    if let Some(struct_def) = self.type_env.get_struct(&struct_name) {
                        if let Some((field_index, field)) = struct_def.fields.iter().enumerate()
                            .find(|(_, f)| f.name == *method_name) 
                        {
                            is_field_indexing = true;
                            
                            // NOW we can safely lower the receiver since we know it's field indexing
                            let receiver_value = self.lower_expr(receiver, ir_block, func)?;
                            
                            // It's field access + indexing!
                            // First, extract the field (which should be an array/pointer)
                            let field_value = self.fresh_value_id();
                            
                            // Get field type - for now use pointer as simplified
                            let field_type = IrType::Pointer(Box::new(IrType::Int(8)));
                            
                            ir_block.add_instruction(IrInstruction {
                                result: field_value,
                                ty: field_type.clone(),
                                kind: IrInstructionKind::TupleExtract {
                                    tuple: receiver_value,
                                    index: field_index as u32,
                                },
                            });
                            
                            // Now index into the field
                            let index_value = self.lower_expr(&args[0], ir_block, func)?;
                            let result_value = self.fresh_value_id();
                            
                            // Determine element type based on field type
                            let element_type = match field_type {
                                IrType::Pointer(inner) => *inner,
                                IrType::Array { element } => *element,
                                _ => IrType::Int(8), // Fallback
                            };
                            
                            ir_block.add_instruction(IrInstruction {
                                result: result_value,
                                ty: element_type,
                                kind: IrInstructionKind::ArrayIndex {
                                    array: field_value,
                                    index: index_value,
                                },
                            });
                            
                            return Ok(result_value);
                        }
                    }
                }
            }
            
            // If we didn't handle it as field indexing, fall through to regular method call
        }

        // General method call: convert to function call with receiver as first arg
        // Build a call expression for the method with receiver as first argument
        let mut all_args = vec![receiver.clone()];
        all_args.extend_from_slice(args);
        
        if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
            eprintln!("DEBUG lower_method_call: method={}, receiver={:?}, args.len={}", method_name, receiver, args.len());
            if method_name == "bytes" {
                eprintln!("DEBUG: Found bytes method call!");
            }
            if method_name == "first" || method_name == "unwrap" {
                eprintln!("DEBUG lower_method_call: {} - about to call lower_call with {} args", method_name, all_args.len());
            }
        }
        
        // Create an identifier expression for the method name
        let func_expr = atom_ast::Expr::Ident(method.clone());
        
        // Delegate to lower_call which handles generic functions and monomorphization
        self.lower_call(&func_expr, &all_args, ir_block, func)
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
            // Get the actual type of the element
            let elem_type = self.get_value_type(value, ir_block, func)
                .unwrap_or(IrType::Int(64)); // Fallback to Int(64)
            element_types.push(elem_type);
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
        if let Some((enum_name, _case, idx)) = self.type_env.find_enum_case(&struct_name) {
            // This is an enum variant with payload (e.g., Some(x))
            // Enums are represented as tuples: (tag, payload...)
            let enum_name_cloned = enum_name.to_string();
            let idx_value = idx as i64;
            
            // Create the tag constant
            let tag_id = self.fresh_value_id();
            ir_block.add_instruction(IrInstruction {
                result: tag_id,
                ty: IrType::Int(32),
                kind: IrInstructionKind::Const {
                    value: IrConstant::Int(idx_value),
                },
            });
            
            // Create tuple with tag + payload fields
            let mut tuple_elements = vec![tag_id];
            tuple_elements.extend(field_values);
            
            let value_id = self.fresh_value_id();
            ir_block.add_instruction(IrInstruction {
                result: value_id,
                ty: IrType::Enum(enum_name_cloned),
                kind: IrInstructionKind::MakeTuple {
                    elements: tuple_elements,
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
        field_name: &str,
        ir_block: &mut IrBlock,
        func: &mut IrFunction,
    ) -> LowerResult<ValueId> {
        let object_value = self.lower_expr(object, ir_block, func)?;

        // Get the type of the object to determine field index and type
        let object_type = self.get_value_type(object_value, ir_block, func)
            .ok_or_else(|| LowerError::Internal("Cannot determine type of field access object".to_string()))?;

        // Extract struct name from object type
        let struct_name = match &object_type {
            IrType::Struct(name) => name,
            _ => return Err(LowerError::Internal(format!(
                "Field access on non-struct type: {:?}",
                object_type
            ))),
        };

        // Look up the struct definition in type environment
        let struct_def = self.type_env.get_struct(struct_name)
            .ok_or_else(|| LowerError::UndefinedStruct(struct_name.clone()))?;

        // Find the field by name and get its index
        let (field_index, field_type) = struct_def.fields.iter()
            .enumerate()
            .find(|(_, field)| field.name == field_name)
            .ok_or_else(|| LowerError::Internal(format!(
                "Field '{}' not found in struct '{}'",
                field_name, struct_name
            )))?;

        // Convert backend Type to IrType
        let field_ir_type = self.backend_type_to_ir(&field_type.ty)?;

        // Debug output for field access
        if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
            eprintln!("FieldAccess: struct={}, field={}, index={}, type={:?}", 
                      struct_name, field_name, field_index, field_ir_type);
        }

        let value_id = self.fresh_value_id();
        ir_block.add_instruction(IrInstruction {
            result: value_id,
            ty: field_ir_type,
            kind: IrInstructionKind::StructExtract {
                struct_value: object_value,
                field_index: field_index as u32,
            },
        });

        Ok(value_id)
    }

    /// Extract tag values from a pattern, handling alternative patterns.
    /// Returns a vector of tag values - most patterns return a single value,
    /// but alternative patterns return multiple values.
    fn extract_tag_values(&self, pattern: &atom_ast::Pattern, fallback_index: usize) -> LowerResult<Vec<u32>> {
        match pattern {
            atom_ast::Pattern::Literal(lit, _) => {
                // For literal patterns, use the actual literal value
                let tag = match lit {
                    atom_ast::Literal::Integer(val) => *val as u32,
                    atom_ast::Literal::Bool(b) => if *b { 1 } else { 0 },
                    atom_ast::Literal::Rune(r) => *r as u32,
                    _ => fallback_index as u32, // Fallback for other literal types
                };
                Ok(vec![tag])
            }
            atom_ast::Pattern::Enum { name, .. } => {
                // For enum patterns, look up the actual tag value from the enum definition
                if let Some((_, _, idx)) = self.type_env.find_enum_case(&name.name) {
                    Ok(vec![idx as u32])
                } else {
                    // Fallback if not found
                    Ok(vec![fallback_index as u32])
                }
            }
            atom_ast::Pattern::Ident(name) => {
                // For ident patterns (enum cases without fields), look up the tag value
                if let Some((_, _, idx)) = self.type_env.find_enum_case(&name.name) {
                    Ok(vec![idx as u32])
                } else {
                    // Not an enum case, use fallback
                    Ok(vec![fallback_index as u32])
                }
            }
            atom_ast::Pattern::Alternative(patterns, _) => {
                // For alternative patterns, collect all tag values from all alternatives
                let mut tags = Vec::new();
                for pattern in patterns {
                    let mut pattern_tags = self.extract_tag_values(pattern, fallback_index)?;
                    tags.append(&mut pattern_tags);
                }
                Ok(tags)
            }
            _ => {
                // For non-enum/non-literal patterns, use fallback index
                Ok(vec![fallback_index as u32])
            }
        }
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

        // Determine what we're matching on and extract the tag if it's an enum
        let match_value_type = self.get_value_type(match_value, ir_block, func);
        if let Some(debug) = std::env::var("ATOM_DEBUG").ok() {
            if debug == "1" {
                eprintln!("Match value type: {:?}", match_value_type);
            }
        }
        // Check if we're matching on an enum pattern (has Enum patterns in arms)
        let has_enum_patterns = arms.iter().any(|arm| {
            matches!(arm.pattern, atom_ast::Pattern::Enum { .. })
        });
        
        let switch_value = if has_enum_patterns {
            // Check if any arm has fields (payload) - if so, enum is a tuple (tag, payload...)
            // and we need to extract the tag from index 0
            let has_payload = arms.iter().any(|arm| {
                if let atom_ast::Pattern::Enum { fields, .. } = &arm.pattern {
                    !fields.is_empty()
                } else {
                    false
                }
            });
            
            if has_payload {
                // Enum with payload: extract tag from tuple at index 0
                let tag_id = self.fresh_value_id();
                ir_block.add_instruction(IrInstruction {
                    result: tag_id,
                    ty: IrType::Int(32),
                    kind: IrInstructionKind::TupleExtract {
                        tuple: match_value,
                        index: 0,
                    },
                });
                if let Some(debug) = std::env::var("ATOM_DEBUG").ok() {
                    if debug == "1" {
                        eprintln!("Extracting tag from enum tuple, tag_id={:?}, match_value_type={:?}", tag_id, match_value_type);
                    }
                }
                tag_id
            } else {
                // Simple enum without payload: use value directly as tag
                if let Some(debug) = std::env::var("ATOM_DEBUG").ok() {
                    if debug == "1" {
                        eprintln!("Simple enum without payload, using value directly");
                    }
                }
                match_value
            }
        } else {
            // For non-enum types (literals, bools, etc.), switch directly on the value
            match_value
        };

        // Create blocks for each arm and a merge block
        let merge_block_id = self.fresh_block_id();
        let mut case_blocks = Vec::new();
        let mut case_values = Vec::new();
        let mut case_block_ids = Vec::new();

        for (i, arm) in arms.iter().enumerate() {
            let arm_block_id = self.fresh_block_id();
            case_block_ids.push(arm_block_id);
            let mut arm_block = IrBlock::new(arm_block_id);

            // Determine the tag values for this pattern
            // Alternative patterns can produce multiple tag values
            let tag_values = self.extract_tag_values(&arm.pattern, i)?;

            // Handle pattern bindings
            // For enum patterns like Some(inner), bind the payload to the variable
            if let atom_ast::Pattern::Enum { name, fields, .. } = &arm.pattern {
                if let Some(debug) = std::env::var("ATOM_DEBUG").ok() {
                    if debug == "1" {
                        eprintln!("Processing enum pattern: {}, match_value_type={:?}", name.name, match_value_type);
                    }
                }
                
                // Determine the payload type from the actual match value
                // Enums with payloads are stored as tuples (tag, payload...)
                // Use get_value_type to find the actual IR type
                let payload_type = {
                    // Get the actual type of the match value
                    let val_type = self.get_value_type(match_value, ir_block, func);
                    if let Some(debug) = std::env::var("ATOM_DEBUG").ok() {
                        if debug == "1" {
                            eprintln!("Match value {} type for {}: {:?}", match_value.0, name.name, val_type);
                        }
                    }
                    
                    // Extract payload type from GenericEnum type args
                    match val_type {
                        Some(IrType::GenericEnum { type_args, .. }) if !type_args.is_empty() => {
                            // For Option(t), type_args = [t]
                            // The payload type is the first (and usually only) type arg
                            Some(type_args[0].clone())
                        }
                        Some(IrType::GenericStruct { type_args, .. }) if !type_args.is_empty() => {
                            Some(type_args[0].clone())
                        }
                        _ => {
                            // Default to Int64 for non-generic enums
                            Some(IrType::Int(64))
                        }
                    }
                };

                for field_pattern in fields {
                    if let atom_ast::Pattern::Ident(ident) = field_pattern {
                        // Extract the payload from the matched enum variant
                        let payload_id = self.fresh_value_id();
                        let actual_payload_type = payload_type.clone().unwrap_or(IrType::Int(64));
                        if let Some(debug) = std::env::var("ATOM_DEBUG").ok() {
                            if debug == "1" {
                                eprintln!("Binding {} to payload type {:?}", ident.name, actual_payload_type);
                            }
                        }
                        arm_block.add_instruction(IrInstruction {
                            result: payload_id,
                            ty: actual_payload_type.clone(),
                            kind: IrInstructionKind::TupleExtract {
                                tuple: match_value,
                                index: 1, // Index 0 is the tag, index 1 is the payload
                            },
                        });
                        if let Some(debug) = std::env::var("ATOM_DEBUG").ok() {
                            if debug == "1" {
                                eprintln!("TupleExtract payload: match_value={:?}, index=1, payload_id={:?}, payload_type={:?}", 
                                    match_value, payload_id, actual_payload_type);
                            }
                        }
                        // Bind the variable
                        self.variables.insert(
                            ident.name.clone(),
                            VarBinding::Value(payload_id, actual_payload_type),
                        );
                    }
                }
            }

            // Lower the arm body
            let arm_value = self.lower_expr(&arm.body, &mut arm_block, func)?;
            
            // Check if this arm ends with a diverging call (like `error`)
            // If so, mark it as unreachable instead of jumping to merge
            let is_diverging = {
                // Check if the last instruction is a call to a known diverging function
                arm_block.instructions.last()
                    .map(|inst| {
                        if let IrInstructionKind::Call { function, .. } = &inst.kind {
                            // Known diverging functions
                            function == "error" || function == "exit" || function == "panic"
                        } else {
                            false
                        }
                    })
                    .unwrap_or(false)
            };
            
            if is_diverging {
                // This arm diverges (never returns), use Unreachable terminator
                arm_block.set_terminator(IrTerminator::Unreachable);
                // Don't add to case_values since it won't contribute to the phi
            } else {
                // Normal case: jump to merge block and add value for phi
                case_values.push(arm_value);
                arm_block.set_terminator(IrTerminator::Jump {
                    target: merge_block_id,
                });
            }

            // NOTE: After lowering the arm body, arm_block might have been replaced with a
            // nested merge block (if the body contained match expressions). We need to use
            // arm_block.label (the actual current label) for the phi node, not arm_block_id.
            let actual_predecessor_id = arm_block.label;
            case_blocks.push((tag_values, arm_block_id, arm_block, actual_predecessor_id, is_diverging));
        }

        // Create switch terminator on current block
        // Check if the last pattern is a wildcard (should be used as default)
        let last_is_wildcard = arms.last()
            .map(|arm| matches!(arm.pattern, atom_ast::Pattern::Wildcard(_)))
            .unwrap_or(false);
        
        let (cases, default_block_id) = if last_is_wildcard {
            // Last pattern is wildcard: use it as default, all others as cases
            let default_block_id = case_blocks.last().unwrap().1;
            let mut cases: Vec<(u32, BlockId)> = Vec::new();
            for (tags, block_id, _, _, _) in case_blocks.iter().take(case_blocks.len() - 1) {
                // Expand alternative patterns: each tag value becomes a separate case
                for tag in tags {
                    cases.push((*tag, *block_id));
                }
            }
            (cases, default_block_id)
        } else {
            // No wildcard: all patterns are explicit cases, use first as default (will be overridden by cases)
            let default_block_id = case_blocks.first().unwrap().1;
            let mut cases: Vec<(u32, BlockId)> = Vec::new();
            for (tags, block_id, _, _, _) in case_blocks.iter() {
                // Expand alternative patterns: each tag value becomes a separate case
                for tag in tags {
                    cases.push((*tag, *block_id));
                }
            }
            (cases, default_block_id)
        };

        ir_block.set_terminator(IrTerminator::Switch {
            value: switch_value,
            cases,
            default: default_block_id,
        });

        // IMPORTANT: Add the current block to the function before replacing it
        // This preserves any instructions that were added to it before the match expression
        let current_block_label = ir_block.label;
        let current_block = std::mem::replace(ir_block, IrBlock::new(merge_block_id));
        func.add_block(current_block);

        // Create merge block with phi node
        let result_value = self.fresh_value_id();

        // Infer phi type from all incoming values, preferring non-default types
        // This handles cases where diverging functions (like `error`) return a default type
        let phi_type = {
            let mut candidate_types = Vec::new();
            
            // Collect types from all non-diverging case values
            let mut value_idx = 0;
            for (i, (_, _, case_block, _, is_diverging)) in case_blocks.iter().enumerate() {
                if *is_diverging {
                    // Skip diverging branches - they don't contribute to phi
                    continue;
                }
                
                let value_id = case_values[value_idx];
                value_idx += 1;
                let mut found_type = None;
                
                // Search in case blocks
                for inst in &case_block.instructions {
                    if inst.result == value_id {
                        found_type = Some(inst.ty.clone());
                        break;
                    }
                }
                
                // Also search in function blocks if not found
                if found_type.is_none() {
                    for block in &func.blocks {
                        for inst in &block.instructions {
                            if inst.result == value_id {
                                found_type = Some(inst.ty.clone());
                                break;
                            }
                        }
                        if found_type.is_some() { break; }
                    }
                }
                
                if let Some(ty) = found_type {
                    candidate_types.push((i, ty));
                }
            }
            
            // Prefer non-Int(64) types, as Int(64) is the default for void/diverging functions
            // If we have any non-Int(64) type, use the first one
            let preferred_type = candidate_types.iter()
                .find(|(_, ty)| !matches!(ty, IrType::Int(64)))
                .map(|(_, ty)| ty.clone())
                .or_else(|| candidate_types.first().map(|(_, ty)| ty.clone()))
                .unwrap_or(IrType::Int(64));
            
            if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
                eprintln!("PHI type inference: candidate_types={:?}, preferred={:?}", candidate_types, preferred_type);
            }
            
            preferred_type
        };
        
        // Build incoming list, skipping diverging branches
        let mut incoming: Vec<(BlockId, ValueId)> = Vec::new();
        let mut value_idx = 0;
        for (i, (_, _, case_block, actual_pred_id, is_diverging)) in case_blocks.iter().enumerate() {
            if *is_diverging {
                // Skip diverging branches
                continue;
            }
            
            let value_id = case_values[value_idx];
            value_idx += 1;
            incoming.push((*actual_pred_id, value_id));
        }

        // Add all case blocks to function
        for (_, _, block, _, _) in &case_blocks {
            func.add_block(block.clone());
        }

        ir_block.add_instruction(IrInstruction {
            result: result_value,
            ty: phi_type,
            kind: IrInstructionKind::Phi { incoming },
        });

        // ir_block is now the merge block (from the mem::replace above)

        Ok(result_value)
    }

    // ========================================================================
    // Constant Evaluation
    // ========================================================================

    /// Evaluate a constant expression for global variable initialization.
    /// Only supports simple literal expressions for now.
    fn eval_const_expr(&mut self, expr: &atom_ast::Expr) -> LowerResult<IrConstant> {
        match expr {
            atom_ast::Expr::Literal(lit, _) => {
                match lit {
                    atom_ast::Literal::Integer(n) => Ok(IrConstant::Int(*n)),
                    atom_ast::Literal::Float(f) => Ok(IrConstant::Float(*f)),
                    atom_ast::Literal::String(s) => Ok(IrConstant::String(s.as_bytes().to_vec())),
                    atom_ast::Literal::Rune(c) => Ok(IrConstant::Rune(*c)),
                    atom_ast::Literal::Bool(b) => Ok(IrConstant::Bool(*b)),
                }
            }
            _ => Err(LowerError::Unsupported(
                "Non-literal constant expressions in global variables not yet supported".to_string(),
            )),
        }
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
            atom_ast::Type::Generic { name, params, .. } => {
                // Handle parameterized types like UInt(8), Int(32), Float(32)
                let base_name = &name.name;
                
                // Try to extract bit width from first parameter (for Int(64), Float(32), etc.)
                if let Some(first_param) = params.first() {
                    if let Some(param_name) = &first_param.name {
                        // Try to parse the bit width
                        if let Ok(bits) = param_name.name.parse::<u16>() {
                            return match base_name.as_str() {
                                "Int" => Ok(IrType::Int(bits)),
                                "UInt" => Ok(IrType::UInt(bits)),
                                "Float" => Ok(IrType::Float(bits)),
                                _ => self.lower_named_type(base_name),
                            };
                        }
                    }
                    
                    // Check if this has actual type arguments (not bit widths)
                    // E.g., Option(t) or Result(Int, String)
                    let has_type_args = params.iter().any(|p| p.ty.is_some());
                    
                    if has_type_args {
                        // Lower each type argument
                        let mut type_args = Vec::new();
                        for param in params {
                            if let Some(ty) = &param.ty {
                                type_args.push(self.lower_type(ty)?);
                            }
                        }
                        
                        // Check if this is an enum or struct
                        let base_ir_ty = self.lower_named_type(base_name)?;
                        return match base_ir_ty {
                            IrType::Enum(_) => Ok(IrType::GenericEnum {
                                name: base_name.clone(),
                                type_args,
                            }),
                            IrType::Struct(_) => Ok(IrType::GenericStruct {
                                name: base_name.clone(),
                                type_args,
                            }),
                            _ => Ok(base_ir_ty), // Not enum/struct, use base type
                        };
                    }
                }
                
                // Fallback to base type without parameters
                self.lower_named_type(base_name)
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
            "String" => Ok(IrType::Struct("String".to_string())),
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

    /// Convert a backend Type to IrType.
    /// This is used when we have type information from the type environment
    /// and need to convert it to IR types.
    fn backend_type_to_ir(&self, ty: &crate::types::Type) -> LowerResult<IrType> {
        use crate::types::Type;
        
        match ty {
            Type::Void => Ok(IrType::Void),
            Type::Int(Some(bits)) => Ok(IrType::Int(*bits as u16)),
            Type::Int(None) => Ok(IrType::Int(64)),
            Type::UInt(Some(bits)) => Ok(IrType::UInt(*bits as u16)),
            Type::UInt(None) => Ok(IrType::UInt(64)),
            Type::Float(Some(bits)) => Ok(IrType::Float(*bits as u16)),
            Type::Float(None) => Ok(IrType::Float(64)),
            Type::Rune => Ok(IrType::Rune),
            Type::TypeMeta => Ok(IrType::Void), // Type values are compile-time only
            Type::Tuple(tuple_ty) => {
                let mut field_types = Vec::new();
                for field in &tuple_ty.fields {
                    field_types.push(self.backend_type_to_ir(&field.ty)?);
                }
                Ok(IrType::Tuple(field_types))
            }
            Type::Struct(struct_ty) => Ok(IrType::Struct(struct_ty.name.clone())),
            Type::Enum(enum_ty) => Ok(IrType::Enum(enum_ty.name.clone())),
            Type::Function(func_ty) => {
                let mut param_types = Vec::new();
                for param_ty in &func_ty.params {
                    param_types.push(self.backend_type_to_ir(param_ty)?);
                }
                let return_type = if let Some(ret_ty) = &func_ty.return_type {
                    Some(self.backend_type_to_ir(ret_ty)?)
                } else {
                    None
                };
                Ok(IrType::Function {
                    params: param_types,
                    return_type: Box::new(return_type),
                })
            }
            Type::TypeParam(_) => {
                // Type parameters are polymorphic - treat as opaque pointer
                Ok(IrType::Pointer(Box::new(IrType::Void)))
            }
            Type::Generic { base, .. } => {
                // For generic instantiations, convert the base type
                // TODO: Handle type arguments properly for full monomorphization
                self.backend_type_to_ir(base)
            }
            Type::Infer(_) | Type::Error => {
                // Inference types should be resolved by now
                Err(LowerError::Internal(format!(
                    "Unresolved type during lowering: {:?}",
                    ty
                )))
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
        if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
            if self.next_value_id < 20 {  // Log first 20
                eprintln!("DEBUG fresh_value_id: allocated {:?}, next={}", id, self.next_value_id);
            }
        }
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
        if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
            eprintln!("DEBUG lower_closure: clearing variables (was {:?})", self.variables.keys().collect::<Vec<_>>());
        }
        self.variables.clear();

        // Set up parameter bindings for the lifted function
        for (i, (param_name, param_ty)) in lifted_params.iter().enumerate() {
            let param_id = ValueId(i as u32);
            self.params.insert(param_name.clone(), param_id);
            self.variables.insert(param_name.clone(), VarBinding::Value(param_id, param_ty.clone()));
        }
        
        // Reset value ID counter to start after parameters
        // This ensures the first value generated in the closure body doesn't conflict with parameter IDs
        self.next_value_id = lifted_params.len() as u32;

        // Create entry block for lifted function
        let entry_block_id = self.fresh_block_id();
        let mut entry_block = IrBlock::new(entry_block_id);

        // Lower the closure body
        let (body_value, _) = self.lower_block_to_ir(body, &mut entry_block, &mut lifted_func)?;

        // If body produces a value but no return type was specified, infer it
        let final_ret_ty = if ret_ty.is_none() && body_value.is_some() {
            // Get the type of the returned value - safe to unwrap here since we checked is_some() above
            let value_id = match body_value {
                Some(v) => v,
                None => {
                    return Err(LowerError::Internal(
                        "Expected body value for closure return type inference".to_string()
                    ));
                }
            };
            
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
                                            }
        }
        if final_ret_ty != lifted_func.return_type {
            lifted_func.return_type = final_ret_ty.clone();
            if let Some(debug) = std::env::var("ATOM_DEBUG").ok() {
                if debug == "1" {
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
            return_type: Box::new(final_ret_ty),
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

    /// Detect if a block uses implicit closure parameters ($0, $1, etc.)
    fn has_implicit_params(&self, block: &atom_ast::Block) -> Vec<String> {
        let mut params = std::collections::HashSet::new();
        self.find_implicit_params_in_block(block, &mut params);
        
        if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") && !params.is_empty() {
            eprintln!("DEBUG has_implicit_params: found params {:?} before filtering", params);
            eprintln!("DEBUG has_implicit_params: current variables: {:?}", self.variables.keys().collect::<Vec<_>>());
        }
        
        // Filter out parameters that are already bound in the current scope
        // This prevents treating blocks that reference already-bound variables (like $0 in a loop)
        // as closures with implicit parameters
        let mut param_list: Vec<String> = params
            .into_iter()
            .filter(|param_name| !self.variables.contains_key(param_name))
            .collect();
        param_list.sort();
        
        if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") && !param_list.is_empty() {
            eprintln!("DEBUG has_implicit_params: after filtering: {:?}", param_list);
        }
        
        param_list
    }
    
    fn find_implicit_params_in_block(&self, block: &atom_ast::Block, params: &mut std::collections::HashSet<String>) {
        for stmt in &block.stmts {
            self.find_implicit_params_in_stmt(stmt, params);
        }
    }
    
    fn find_implicit_params_in_stmt(&self, stmt: &atom_ast::Stmt, params: &mut std::collections::HashSet<String>) {
        match stmt {
            atom_ast::Stmt::Expression(expr) => {
                self.find_implicit_params_in_expr(expr, params);
            }
            atom_ast::Stmt::VarDecl(decl) => {
                if let Some(init) = &decl.init {
                    self.find_implicit_params_in_expr(init, params);
                }
            }
        }
    }
    
    fn find_implicit_params_in_expr(&self, expr: &atom_ast::Expr, params: &mut std::collections::HashSet<String>) {
        use atom_ast::Expr;
        match expr {
            Expr::Ident(ident) if ident.name.starts_with('$') && ident.name[1..].chars().all(|c| c.is_ascii_digit()) => {
                params.insert(ident.name.clone());
            }
            Expr::Binary { left, right, .. } => {
                self.find_implicit_params_in_expr(left, params);
                self.find_implicit_params_in_expr(right, params);
            }
            Expr::Unary { expr, .. } => {
                self.find_implicit_params_in_expr(expr, params);
            }
            Expr::Call { func, args, .. } => {
                // Scan function expression
                self.find_implicit_params_in_expr(func, params);
                // Scan arguments, but don't scan into Block or Closure arguments
                // as they have their own parameter scope
                for arg in args {
                    if !matches!(arg, Expr::Block(_) | Expr::Closure { .. }) {
                        self.find_implicit_params_in_expr(arg, params);
                    }
                }
            }
            Expr::Block(block) => {
                // Don't scan into nested blocks - they have their own parameter scope
            }
            Expr::Match { expr,  .. } => {
                self.find_implicit_params_in_expr(expr, params);
                // Don't scan match arm bodies - they have their own scope
            }
            _ => {}
        }
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

    /// Generate a mangled function name for overloaded functions.
    ///
    /// When multiple functions have the same name but different signatures,
    /// we need to generate unique names for the IR/codegen layer.
    /// The mangling scheme is: name_{param_count} for overloads with different arities.
    fn mangle_function_name(&self, func_def: &atom_ast::FunctionDef) -> String {
        let base_name = &func_def.name.name;
        let param_count = func_def.params.len();
        
        // Check if this function has overloads
        if let Some(overloads) = self.function_defs.get(base_name) {
            if overloads.len() > 1 {
                // Multiple overloads - need to mangle
                return format!("{}_{}", base_name, param_count);
            }
        }
        
        // No overloading, use base name
        base_name.clone()
    }

    /// Select the correct function overload based on the number of arguments provided.
    ///
    /// This handles function overloading by matching the number of provided arguments
    /// against the number of required and optional parameters in each overload.
    /// 
    /// Returns the first overload where:
    /// - arg_count >= required_params (all required params are provided)
    /// - arg_count <= total_params (not too many args, considering defaults)
    fn select_overload<'a>(
        &self,
        func_defs: &'a [atom_ast::FunctionDef],
        arg_count: usize,
    ) -> Option<&'a atom_ast::FunctionDef> {
        for func_def in func_defs.iter() {
            // Count required parameters (those without defaults)
            let required_params = func_def
                .params
                .iter()
                .take_while(|p| p.default.is_none())
                .count();
            let total_params = func_def.params.len();
            
            // Check if this overload matches the argument count
            if arg_count >= required_params && arg_count <= total_params {
                return Some(func_def);
            }
        }
        
        // No matching overload found
        None
    }

    /// Check if a function definition is generic (has type parameters).
    ///
    /// A function is considered generic if:
    /// 1. It has const_params (compile-time parameters), OR
    /// 2. Any of its parameters have types that contain TypeParam, OR
    /// 3. Its return type contains TypeParam
    fn is_generic_function(&self, func_def: &atom_ast::FunctionDef) -> bool {
        let debug = std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1");
        
        if debug && (func_def.name.name == "first" || func_def.name.name == "unwrap") {
            eprintln!("DEBUG is_generic_function: checking {}", func_def.name.name);
            eprintln!("  const_params: {}", func_def.const_params.len());
            eprintln!("  params: {:?}", func_def.params.iter().map(|p| (&p.name.name, &p.ty)).collect::<Vec<_>>());
            eprintln!("  return_type: {:?}", func_def.return_type);
        }
        
        // Check for const parameters
        if !func_def.const_params.is_empty() {
            if debug {
                eprintln!("DEBUG is_generic_function: {} has const_params", func_def.name.name);
            }
            return true;
        }

        // Check parameter types for type parameters
        for param in &func_def.params {
            if let Some(ref ty) = param.ty {
                if self.type_contains_type_param(ty) {
                    if debug {
                        eprintln!("DEBUG is_generic_function: {} has param type param: {:?}", func_def.name.name, ty);
                    }
                    return true;
                }
            }
        }

        // Check return type for type parameters
        if let Some(ref return_type) = func_def.return_type {
            if self.type_contains_type_param(return_type) {
                if debug {
                    eprintln!("DEBUG is_generic_function: {} has return type param: {:?}", func_def.name.name, return_type);
                }
                return true;
            }
        }
        
        if debug && (func_def.name.name == "first" || func_def.name.name == "unwrap") {
            eprintln!("DEBUG is_generic_function: {} is NOT generic", func_def.name.name);
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

            // Reference types: check inner type
            atom_ast::Type::Reference { inner, .. } => {
                self.type_contains_type_param(inner)
            }
        }
    }

    /// Extract concrete types from call arguments for monomorphization.
    ///
    /// Maps type parameter names (like "t") to their concrete types based on
    /// the argument types provided at the call site.
    fn extract_concrete_types(
        &self,
        arg_values: &[ValueId],
        ir_block: &IrBlock,
        func: &IrFunction,
        func_def: &atom_ast::FunctionDef,
    ) -> HashMap<String, IrType> {
        let mut bindings = HashMap::new();
        
        if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
            eprintln!("DEBUG extract_concrete_types: func={}, arg_values.len={}", func_def.name.name, arg_values.len());
        }
        
                // Match each argument with its corresponding parameter
        for (i, param) in func_def.params.iter().enumerate() {
            if i >= arg_values.len() {
                                break;
            }
            
            let arg_value = arg_values[i];
            
            // Get the concrete type of this argument
            let arg_type = match self.get_value_type(arg_value, ir_block, func) {
                Some(ty) => {
                    if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
                        eprintln!("DEBUG extract_concrete_types: param[{}]={}, arg_value={:?}, arg_type={:?}", 
                                  i, param.name.name, arg_value, ty);
                    }
                    ty
                }
                None => {
                    if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
                        eprintln!("DEBUG extract_concrete_types: param[{}]={}, arg_value={:?}, type NOT FOUND", 
                                  i, param.name.name, arg_value);
                    }
                    continue;
                }
            };
            
            // Check if this parameter's type contains type parameters
            if let Some(param_ty) = &param.ty {
                if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
                    eprintln!("DEBUG extract_concrete_types: collecting bindings for param_ty={:?}, arg_type={:?}", 
                              param_ty, arg_type);
                }
                self.collect_type_bindings(param_ty, &arg_type, &mut bindings);
            }
        }
        
        if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
            eprintln!("DEBUG extract_concrete_types: func={}, bindings={:?}", func_def.name.name, bindings);
        }
        
                bindings
    }

    /// Get the IrType of a ValueId by searching through the IR.
    fn get_value_type(
        &self,
        value_id: ValueId,
        ir_block: &IrBlock,
        func: &IrFunction,
    ) -> Option<IrType> {
        // Case 1: Value is a function parameter
        // Parameters are ValueId(0), ValueId(1), etc. matching their position
        if let Some((_, param_type)) = func.params.get(value_id.0 as usize) {
                        return Some(param_type.clone());
        }
        
        // Case 2: Value was created in the current block
        for inst in &ir_block.instructions {
            if inst.result == value_id {
                                return Some(inst.ty.clone());
            }
        }
        
        // Case 3: Value was created in a previous block
        for block in &func.blocks {
            for inst in &block.instructions {
                if inst.result == value_id {
                                        return Some(inst.ty.clone());
                }
            }
        }
        
                None
    }

    /// Get the return type of a function by name.
    /// Looks up the function in function_defs and converts its return type to IrType.
    fn get_function_return_type(&mut self, func_name: &str) -> Option<IrType> {
        // Check function definitions - clone the return type AST to avoid borrow issues
        let return_type_ast = self.function_defs.get(func_name)
            .and_then(|func_defs| func_defs.first())
            .and_then(|func_def| func_def.return_type.clone())?;
        
        // Try to lower the return type
        if let Ok(ir_type) = self.lower_type(&return_type_ast) {
            return Some(ir_type);
        }
        
        // If lowering fails, default to Void
        Some(IrType::Void)
    }

    /// Recursively collect type parameter bindings by matching AST type with concrete IR type.
    fn collect_type_bindings(
        &self,
        param_type: &atom_ast::Type,
        concrete_type: &IrType,
        bindings: &mut HashMap<String, IrType>,
    ) {
        match param_type {
            // Direct type parameter reference - this is what we're looking for!
            atom_ast::Type::Param(ident) => {
                                bindings.insert(ident.name.clone(), concrete_type.clone());
            }
            
            // Named type - no type parameters to extract
            atom_ast::Type::Named(_) => {}
            
            // Tuple type - recursively match elements
            atom_ast::Type::Tuple(param_types, _) => {
                if let IrType::Tuple(concrete_types) = concrete_type {
                    for (param_elem, concrete_elem) in param_types.iter().zip(concrete_types.iter()) {
                        self.collect_type_bindings(param_elem, concrete_elem, bindings);
                    }
                }
            }
            
            // Variadic/Array types
            atom_ast::Type::Variadic { element, .. } |
            atom_ast::Type::StaticArray { element, .. } => {
                // Variadic types can be lowered to either Array or Tuple in IR
                match concrete_type {
                    IrType::Array { element: concrete_elem } => {
                        self.collect_type_bindings(element, concrete_elem, bindings);
                    }
                    IrType::Tuple(concrete_elems) => {
                        // Variadic tuple lowered to IR Tuple
                        // All elements should have the same type, so use the first one
                        if let Some(first_elem) = concrete_elems.first() {
                            if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
                                eprintln!("DEBUG collect_type_bindings: Variadic matched to Tuple, extracting element type from first element: {:?}", first_elem);
                            }
                            self.collect_type_bindings(element, first_elem, bindings);
                        }
                    }
                    _ => {}
                }
            }
            
            // Function types
            atom_ast::Type::Function { params, return_type, .. } => {
                if let IrType::Function { params: concrete_params, return_type: concrete_ret } = concrete_type {
                    // Match parameter types
                    for (param, concrete) in params.iter().zip(concrete_params.iter()) {
                        self.collect_type_bindings(param, concrete, bindings);
                    }
                    // Match return type
                    if let (Some(param_ret), Some(concrete_ret)) = (return_type.as_ref(), concrete_ret.as_ref()) {
                        self.collect_type_bindings(param_ret, concrete_ret, bindings);
                    }
                }
            }
            
            // Generic types - handle type parameter extraction from generic instantiations
            // E.g., Option(t) matched with GenericEnum { name: "Option", type_args: [Int(64)] }
            atom_ast::Type::Generic { params, .. } => {
                match concrete_type {
                    IrType::GenericEnum { type_args: concrete_args, .. } |
                    IrType::GenericStruct { type_args: concrete_args, .. } => {
                        // Match each type parameter with its concrete argument
                        for (param, concrete_arg) in params.iter().zip(concrete_args.iter()) {
                            if let Some(param_ty) = &param.ty {
                                self.collect_type_bindings(param_ty, concrete_arg, bindings);
                            }
                        }
                    }
                    _ => {}
                }
            }
            
            // Other types - no type parameters to extract
            _ => {}
        }
    }

    /// Generate a monomorphized function name based on concrete type parameters.
    ///
    /// Format: original_name$TypeName1$TypeName2
    /// Example: print$String, print$Int
    fn generate_mono_name(
        &self,
        func_name: &str,
        concrete_types: &HashMap<String, IrType>,
        _func_def: &atom_ast::FunctionDef,
    ) -> String {
                if concrete_types.is_empty() {
                        return func_name.to_string();
        }
        
        let mut name = func_name.to_string();
        
        // Append type parameter bindings in sorted order for consistency
        let mut sorted_bindings: Vec<_> = concrete_types.iter().collect();
        sorted_bindings.sort_by_key(|(k, _)| k.as_str());
        
        for (_param_name, concrete_type) in sorted_bindings {
            let type_suffix = match concrete_type {
                IrType::Int(bits) => format!("Int{}", bits),
                IrType::Float(bits) => format!("Float{}", bits),
                IrType::Pointer(inner) => match **inner {
                    IrType::Int(8) => "String".to_string(),
                    _ => "Ptr".to_string(),
                },
                IrType::Void => "Void".to_string(),
                IrType::Struct(name) => name.clone(),
                IrType::Enum(name) => name.clone(),
                _ => "Generic".to_string(),
            };
            name.push('$');
            name.push_str(&type_suffix);
        }
        
                name
    }

    /// Substitute type parameters in a function definition with concrete types.
    ///
    /// This creates a specialized version of the function where all type parameters
    /// are replaced with their concrete instantiations.
    fn substitute_type_params_in_ast(
        &self,
        func_def: &atom_ast::FunctionDef,
        type_bindings: &HashMap<String, IrType>,
    ) -> LowerResult<atom_ast::FunctionDef> {
        let mut result = func_def.clone();
        
        // Substitute in parameter types
        result.params = result.params.iter()
            .map(|param| {
                let substituted_ty = param.ty.as_ref()
                    .map(|t| self.substitute_ast_type(t, type_bindings))
                    .transpose()?;
                Ok(atom_ast::Param {
                    name: param.name.clone(),
                    ty: substituted_ty.map(Box::new),
                    default: param.default.clone(), // Don't traverse defaults for now
                    span: param.span,
                })
            })
            .collect::<LowerResult<Vec<_>>>()?;
        
        // Substitute in return type
        result.return_type = result.return_type.as_ref()
            .map(|rt| self.substitute_ast_type(rt, type_bindings).map(Box::new))
            .transpose()?;
        
        // Note: We don't substitute in the body because expressions don't
        // contain type annotations in Atom (type inference handles that)
        // Only function signatures need substitution
        
        Ok(result)
    }

    /// Recursively substitute type parameters in an AST type.
    fn substitute_ast_type(
        &self,
        ast_type: &atom_ast::Type,
        type_bindings: &HashMap<String, IrType>,
    ) -> LowerResult<atom_ast::Type> {
        match ast_type {
            // BASE CASE: Direct substitution of type parameter
            atom_ast::Type::Param(ident) => {
                if let Some(concrete_type) = type_bindings.get(&ident.name) {
                                        self.ir_type_to_ast_type(concrete_type)
                } else {
                    // Unbound type parameter - leave as is (or error?)
                                        Ok(ast_type.clone())
                }
            }
            
            // PASS-THROUGH: No type params to substitute
            atom_ast::Type::Named(_) => Ok(ast_type.clone()),
            
            // RECURSIVE CASES: Traverse and substitute
            atom_ast::Type::Tuple(types, span) => {
                let substituted_types = types.iter()
                    .map(|t| self.substitute_ast_type(t, type_bindings).map(Box::new))
                    .collect::<LowerResult<Vec<_>>>()?;
                Ok(atom_ast::Type::Tuple(substituted_types, *span))
            }
            
            atom_ast::Type::Variadic { element, non_empty, span } => {
                let substituted_element = self.substitute_ast_type(element, type_bindings)?;
                Ok(atom_ast::Type::Variadic {
                    element: Box::new(substituted_element),
                    non_empty: *non_empty,
                    span: *span,
                })
            }
            
            atom_ast::Type::StaticArray { element, size, span } => {
                let substituted_element = self.substitute_ast_type(element, type_bindings)?;
                Ok(atom_ast::Type::StaticArray {
                    element: Box::new(substituted_element),
                    size: size.clone(),
                    span: *span,
                })
            }
            
            atom_ast::Type::Function { params, return_type, span } => {
                let substituted_params = params.iter()
                    .map(|p| self.substitute_ast_type(p, type_bindings).map(Box::new))
                    .collect::<LowerResult<Vec<_>>>()?;
                let substituted_return = return_type.as_ref()
                    .map(|rt| self.substitute_ast_type(rt, type_bindings).map(Box::new))
                    .transpose()?;
                Ok(atom_ast::Type::Function {
                    params: substituted_params,
                    return_type: substituted_return,
                    span: *span,
                })
            }
            
            // Generic types: substitute in type arguments
            atom_ast::Type::Generic { name, params, span } => {
                let substituted_params = params.iter()
                    .map(|tp| {
                        let substituted_ty = tp.ty.as_ref()
                            .map(|t| self.substitute_ast_type(t, type_bindings).map(Box::new))
                            .transpose()?;
                        let substituted_default = tp.default.as_ref()
                            .map(|d| self.substitute_ast_type(d, type_bindings).map(Box::new))
                            .transpose()?;
                        Ok(Box::new(atom_ast::TypeParam {
                            name: tp.name.clone(),
                            ty: substituted_ty,
                            default: substituted_default,
                            span: tp.span,
                        }))
                    })
                    .collect::<LowerResult<Vec<_>>>()?;
                Ok(atom_ast::Type::Generic {
                    name: name.clone(),
                    params: substituted_params,
                    span: *span,
                })
            }

            // Reference types: substitute inner type
            atom_ast::Type::Reference { inner, span } => {
                let substituted_inner = self.substitute_ast_type(inner, type_bindings)?;
                Ok(atom_ast::Type::Reference {
                    inner: Box::new(substituted_inner),
                    span: *span,
                })
            }
        }
    }

    /// Convert IrType back to AST Type for type substitution.
    fn ir_type_to_ast_type(&self, ir_type: &IrType) -> LowerResult<atom_ast::Type> {
        let span = atom_ast::Span::new(0, 0); // Synthetic span for generated types
        
        match ir_type {
            IrType::Void => Ok(atom_ast::Type::Named(atom_ast::Ident {
                name: "Void".to_string(),
                span,
            })),
            IrType::Bool => Ok(atom_ast::Type::Named(atom_ast::Ident {
                name: "Bool".to_string(),
                span,
            })),
            IrType::Int(64) => Ok(atom_ast::Type::Named(atom_ast::Ident {
                name: "Int".to_string(),
                span,
            })),
            IrType::Int(32) => Ok(atom_ast::Type::Named(atom_ast::Ident {
                name: "Int32".to_string(),
                span,
            })),
            IrType::Int(8) => Ok(atom_ast::Type::Named(atom_ast::Ident {
                name: "Int8".to_string(),
                span,
            })),
            IrType::Float(64) => Ok(atom_ast::Type::Named(atom_ast::Ident {
                name: "Float".to_string(),
                span,
            })),
            IrType::Float(32) => Ok(atom_ast::Type::Named(atom_ast::Ident {
                name: "Float32".to_string(),
                span,
            })),
            IrType::Rune => Ok(atom_ast::Type::Named(atom_ast::Ident {
                name: "Rune".to_string(),
                span,
            })),
            IrType::Pointer(inner) if matches!(**inner, IrType::Int(8)) => {
                Ok(atom_ast::Type::Named(atom_ast::Ident {
                    name: "String".to_string(),
                    span,
                }))
            }
            IrType::Pointer(inner) if matches!(**inner, IrType::Void) => {
                // Opaque pointer type - use generic "Pointer" name
                Ok(atom_ast::Type::Named(atom_ast::Ident {
                    name: "Pointer".to_string(),
                    span,
                }))
            }
            IrType::Struct(name) | IrType::Enum(name) => {
                Ok(atom_ast::Type::Named(atom_ast::Ident {
                    name: name.clone(),
                    span,
                }))
            }
            IrType::Tuple(elements) => {
                let ast_elements = elements.iter()
                    .map(|e| self.ir_type_to_ast_type(e).map(Box::new))
                    .collect::<LowerResult<Vec<_>>>()?;
                Ok(atom_ast::Type::Tuple(ast_elements, span))
            }
            _ => Err(LowerError::Unsupported(format!(
                "Cannot convert IrType to AST Type: {:?}",
                ir_type
            ))),
        }
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
