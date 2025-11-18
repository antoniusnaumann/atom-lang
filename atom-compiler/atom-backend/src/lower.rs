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
/// - Value ID counter for SSA form
/// - Block ID counter for basic blocks
/// - Current function being lowered
pub struct Lower {
    /// Type environment for type resolution
    type_env: TypeEnvironment,
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
            next_value_id: 0,
            next_block_id: 0,
            next_local_id: 0,
            variables: HashMap::new(),
            params: HashMap::new(),
        }
    }

    /// Lower a complete AST program to IR.
    pub fn lower_program(&mut self, ast: Vec<atom_ast::TopLevel>) -> LowerResult<IrProgram> {
        let mut program = IrProgram::new();

        // First pass: collect all type definitions
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

        // Second pass: lower global variables
        for item in &ast {
            if let atom_ast::TopLevel::Variable(decl) = item {
                let ir_global = self.lower_global_var(decl)?;
                program.add_global(ir_global);
            }
        }

        // Third pass: lower functions
        for item in &ast {
            if let atom_ast::TopLevel::Function(func_def) = item {
                let ir_func = self.lower_function(func_def)?;
                program.add_function(ir_func);
            }
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
            None
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
            if let Some(value) = result_value {
                entry_block.set_terminator(IrTerminator::Return { value: Some(value) });
            } else if return_type.is_none() || return_type.as_ref().unwrap().is_void() {
                entry_block.set_terminator(IrTerminator::Return { value: None });
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
        if decl.names.len() != 1 {
            return Err(LowerError::Unsupported(
                "Tuple destructuring in local variables".to_string(),
            ));
        }

        let var_name = decl.names[0].name.clone();
        let init_value = if let Some(init_expr) = &decl.init {
            self.lower_expr(init_expr, ir_block, func)?
        } else {
            return Err(LowerError::Internal(
                "Local variable must have initializer".to_string(),
            ));
        };

        let var_type = if let Some(ty_ast) = &decl.ty {
            self.lower_type(ty_ast)?
        } else {
            // Type inference - would need type checker integration
            return Err(LowerError::Internal(
                "Type inference not yet implemented".to_string(),
            ));
        };

        // For now, all variables are immutable values in SSA form
        // In the future, support mutable locals with Load/Store
        self.variables
            .insert(var_name, VarBinding::Value(init_value, var_type));

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
            atom_ast::Expr::Closure { .. } => {
                Err(LowerError::Unsupported("Closures".to_string()))
            }
            atom_ast::Expr::MethodCall { .. } => {
                Err(LowerError::Unsupported("Method calls".to_string()))
            }
            atom_ast::Expr::Comptime { .. } => {
                Err(LowerError::Unsupported("Comptime expressions".to_string()))
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
        _ir_block: &mut IrBlock,
        _func: &mut IrFunction,
    ) -> LowerResult<ValueId> {
        // Clone the binding to avoid borrow checker issues
        let binding = self.variables.get(name).cloned();
        
        match binding {
            Some(VarBinding::Value(value_id, _)) => Ok(value_id),
            Some(VarBinding::Local(local_id, ty)) => {
                // Need to load from local
                let value_id = self.fresh_value_id();
                _ir_block.add_instruction(IrInstruction {
                    result: value_id,
                    ty,
                    kind: IrInstructionKind::Load {
                        source: IrMemoryLocation::Local(local_id),
                    },
                });
                Ok(value_id)
            }
            None => Err(LowerError::UndefinedVariable(name.to_string())),
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
        ) {
            return Err(LowerError::Unsupported("Assignment operators".to_string()));
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

        // Lower arguments
        let mut arg_values = Vec::new();
        for arg in args {
            let value = self.lower_expr(arg, ir_block, func)?;
            arg_values.push(value);
        }

        // Determine return type (simplified - should query type environment)
        let return_type = IrType::Int(64);

        let value_id = self.fresh_value_id();
        ir_block.add_instruction(IrInstruction {
            result: value_id,
            ty: return_type,
            kind: IrInstructionKind::Call {
                function: func_name,
                args: arg_values,
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

        for (i, arm) in arms.iter().enumerate() {
            let arm_block_id = self.fresh_block_id();
            let mut arm_block = IrBlock::new(arm_block_id);

            // Lower the arm body
            let arm_value = self.lower_expr(&arm.body, &mut arm_block, func)?;
            case_values.push(arm_value);

            // Jump to merge block
            arm_block.set_terminator(IrTerminator::Jump {
                target: merge_block_id,
            });

            case_blocks.push((i as u32, arm_block_id, arm_block));
        }

        // Create switch terminator on current block
        let default_block_id = case_blocks.last().unwrap().1;
        let cases: Vec<(u32, BlockId)> = case_blocks
            .iter()
            .take(case_blocks.len() - 1)
            .map(|(tag, block_id, _)| (*tag, *block_id))
            .collect();

        ir_block.set_terminator(IrTerminator::Switch {
            value: match_value,
            cases,
            default: default_block_id,
        });

        // Add all case blocks to function
        for (_, _, block) in case_blocks {
            func.add_block(block);
        }

        // Create merge block with phi node
        let mut merge_block = IrBlock::new(merge_block_id);
        let result_value = self.fresh_value_id();

        let incoming: Vec<(BlockId, ValueId)> = arms
            .iter()
            .enumerate()
            .map(|(i, _)| {
                let block_id = BlockId(merge_block_id.0 - (arms.len() - i) as u32);
                (block_id, case_values[i])
            })
            .collect();

        merge_block.add_instruction(IrInstruction {
            result: result_value,
            ty: IrType::Int(64), // Simplified
            kind: IrInstructionKind::Phi { incoming },
        });

        // This becomes the new current block
        *ir_block = merge_block;

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
                } else {
                    Err(LowerError::UndefinedStruct(name.to_string()))
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
