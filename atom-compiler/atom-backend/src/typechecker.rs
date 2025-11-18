//! Type checker for the Atom language compiler backend.
//!
//! This module performs type checking and type inference on Atom ASTs,
//! ensuring type safety while handling all language features including:
//! - Structural typing and implicit conversions
//! - Generic types with const/type parameters
//! - Variadic tuples and static arrays
//! - Pattern matching with type checking
//! - Closures and first-class functions
//! - Operator overloading through field-wise application
//!
//! The type checker builds a typed AST along with a type environment containing
//! all user-defined types (structs, enums) for later compilation stages.

use crate::types::{
    BinaryOp, ConstArg, EnumCase, EnumType, FunctionType, StructField, StructType, SymbolTable,
    TupleField, TupleType, Type, TypeEnvironment, TypeError, TypeParameter, TypeResult,
};
use atom_ast::{
    self, BinOp, Block, Expr, FieldInit, FunctionDef, Ident, Literal, MatchArm,
    Param, Pattern, Stmt, StructDef, TestBlock, TopLevel, UnOp, VarDecl,
};
use std::collections::HashMap;

/// Result of type checking a complete program
#[derive(Debug, Clone)]
pub struct TypedProgram {
    /// The original AST (for now, until we build a full typed AST)
    pub ast: Vec<TopLevel>,
    /// Type environment with all defined types
    pub type_env: TypeEnvironment,
    /// Global variable types
    pub globals: HashMap<String, Type>,
    /// Function signatures by name (supports overloading)
    pub functions: HashMap<String, Vec<FunctionSignature>>,
}

/// Signature of a function for type checking
#[derive(Debug, Clone)]
pub struct FunctionSignature {
    /// Const/type parameters
    pub const_params: Vec<TypeParameter>,
    /// Regular parameters with names
    pub params: Vec<(String, Type)>,
    /// Return type (None for Void)
    pub return_type: Option<Type>,
}

/// Type checker for Atom programs
pub struct TypeChecker {
    /// Type environment (tracks structs, enums, type aliases)
    type_env: TypeEnvironment,
    /// Symbol table for variable types (stack of scopes)
    symbols: SymbolTable,
    /// Function signatures (for checking calls)
    functions: HashMap<String, Vec<FunctionSignature>>,
    /// Global variable types
    globals: HashMap<String, Type>,
    /// Current function's return type (for checking returns)
    current_return_type: Option<Type>,
}

impl TypeChecker {
    /// Create a new type checker with standard library types
    pub fn new() -> Self {
        Self {
            type_env: TypeEnvironment::with_stdlib(),
            symbols: SymbolTable::new(),
            functions: HashMap::new(),
            globals: HashMap::new(),
            current_return_type: None,
        }
    }

    /// Type check a complete program (Vec of top-level items)
    pub fn check_program(&mut self, ast: Vec<TopLevel>) -> TypeResult<TypedProgram> {
        // First pass: collect all type definitions
        for item in &ast {
            match item {
                TopLevel::Struct(struct_def) => {
                    self.collect_struct(struct_def)?;
                }
                TopLevel::Enum(enum_def) => {
                    self.collect_enum(enum_def)?;
                }
                _ => {}
            }
        }

        // Second pass: collect function signatures
        for item in &ast {
            if let TopLevel::Function(func_def) = item {
                self.collect_function_signature(func_def)?;
            }
        }

        // Third pass: type check all items
        for item in &ast {
            match item {
                TopLevel::Import(_) => {
                    // Imports are handled by earlier stages
                }
                TopLevel::Struct(_) | TopLevel::Enum(_) => {
                    // Already processed in first pass
                }
                TopLevel::Function(func_def) => {
                    self.check_function(func_def)?;
                }
                TopLevel::Variable(var_decl) => {
                    self.check_global_variable(var_decl)?;
                }
                TopLevel::TestBlock(test_block) => {
                    self.check_test_block(test_block)?;
                }
                TopLevel::Statement(stmt) => {
                    // Top-level statements (only in test files)
                    self.check_stmt(stmt)?;
                }
            }
        }

        Ok(TypedProgram {
            ast,
            type_env: self.type_env.clone(),
            globals: self.globals.clone(),
            functions: self.functions.clone(),
        })
    }

    // ========================================================================
    // Type Definition Collection
    // ========================================================================

    fn collect_struct(&mut self, struct_def: &StructDef) -> TypeResult<()> {
        let mut fields = Vec::new();
        let mut type_params = Vec::new();

        // Process type parameters
        for param in &struct_def.type_params {
            let type_param = self.ast_type_param_to_type_param(param)?;
            type_params.push(type_param);
        }

        // Process fields
        for field in &struct_def.fields {
            let field_name = field
                .name
                .as_ref()
                .ok_or_else(|| TypeError::Other("Struct fields must be named".to_string()))?
                .name
                .clone();

            let field_type = self.resolve_ast_type(&field.ty)?;

            fields.push(StructField {
                name: field_name,
                ty: Box::new(field_type),
            });
        }

        let struct_type = StructType {
            name: struct_def.name.name.clone(),
            params: type_params,
            fields,
            visibility: struct_def.visibility,
        };

        self.type_env.add_struct(struct_type);
        Ok(())
    }

    fn collect_enum(&mut self, enum_def: &atom_ast::EnumDef) -> TypeResult<()> {
        let mut cases = Vec::new();
        let mut type_params = Vec::new();

        // Process type parameters
        for param in &enum_def.type_params {
            let type_param = self.ast_type_param_to_type_param(param)?;
            type_params.push(type_param);
        }

        // Process cases
        for case in &enum_def.cases {
            let case_name = case.name.name.clone();

            // Check that case names start with uppercase
            if !case_name.chars().next().unwrap_or('a').is_uppercase() {
                return Err(TypeError::Other(format!(
                    "Enum case '{}' must start with uppercase letter",
                    case_name
                )));
            }

            let mut case_fields = Vec::new();
            for field_ty in &case.fields {
                let ty = self.resolve_ast_type(field_ty)?;
                case_fields.push(Box::new(ty));
            }

            cases.push(EnumCase {
                name: case_name,
                fields: case_fields,
            });
        }

        let enum_type = EnumType {
            name: enum_def.name.name.clone(),
            params: type_params,
            cases,
            visibility: enum_def.visibility,
        };

        self.type_env.add_enum(enum_type);
        Ok(())
    }

    fn collect_function_signature(&mut self, func_def: &FunctionDef) -> TypeResult<()> {
        let func_name = func_def.name.name.clone();

        let mut const_params = Vec::new();
        for param in &func_def.const_params {
            let ty = if let Some(ty_ast) = &param.ty {
                self.resolve_ast_type(ty_ast)?
            } else {
                Type::TypeMeta // Default to Type for const params
            };

            const_params.push(TypeParameter {
                name: param.name.name.clone(),
                constraint: Some(Box::new(ty)),
                default: None, // TODO: handle defaults
            });
        }

        let mut params = Vec::new();
        for param in &func_def.params {
            let param_name = param.name.name.clone();
            let param_ty = if let Some(ty_ast) = &param.ty {
                self.resolve_ast_type(ty_ast)?
            } else {
                // Infer from default or error
                return Err(TypeError::Other(format!(
                    "Parameter '{}' must have an explicit type",
                    param_name
                )));
            };

            params.push((param_name, param_ty));
        }

        let return_type = if let Some(ret_ty_ast) = &func_def.return_type {
            Some(self.resolve_ast_type(ret_ty_ast)?)
        } else {
            None
        };

        let signature = FunctionSignature {
            const_params,
            params,
            return_type,
        };

        self.functions
            .entry(func_name)
            .or_default()
            .push(signature);

        Ok(())
    }

    // ========================================================================
    // Type Checking
    // ========================================================================

    fn check_function(&mut self, func_def: &FunctionDef) -> TypeResult<()> {
        // Create new scope for function
        self.symbols.push_scope();

        // Add parameters to scope
        for param in &func_def.params {
            let param_name = param.name.name.clone();
            let param_ty = if let Some(ty_ast) = &param.ty {
                self.resolve_ast_type(ty_ast)?
            } else {
                return Err(TypeError::Other(format!(
                    "Parameter '{}' requires explicit type",
                    param_name
                )));
            };
            self.symbols.add_variable(param_name, param_ty);
        }

        // Set current return type
        let return_type = if let Some(ret_ty_ast) = &func_def.return_type {
            Some(self.resolve_ast_type(ret_ty_ast)?)
        } else {
            None
        };
        self.current_return_type = return_type.clone();

        // Check function body
        let body_type = self.check_block(&func_def.body)?;

        // Verify return type matches
        let expected_return = return_type.unwrap_or(Type::Void);
        if !body_type.can_convert_to(&expected_return) {
            return Err(TypeError::Incompatible {
                expected: Box::new(expected_return),
                found: Box::new(body_type),
                reason: "Function return type mismatch".to_string(),
            });
        }

        // Restore scope
        self.symbols.pop_scope();
        self.current_return_type = None;

        Ok(())
    }

    fn check_global_variable(&mut self, var_decl: &VarDecl) -> TypeResult<()> {
        if var_decl.names.len() > 1 {
            return Err(TypeError::Other(
                "Tuple destructuring not yet supported for global variables".to_string(),
            ));
        }

        let var_name = var_decl.names[0].name.clone();

        let var_type = if let Some(init) = &var_decl.init {
            let init_type = self.check_expr(init)?;

            if let Some(explicit_ty) = &var_decl.ty {
                let expected_ty = self.resolve_ast_type(explicit_ty)?;
                if !init_type.can_convert_to(&expected_ty) {
                    return Err(TypeError::Incompatible {
                        expected: Box::new(expected_ty),
                        found: Box::new(init_type),
                        reason: format!("Cannot initialize variable '{}'", var_name),
                    });
                }
                expected_ty
            } else {
                init_type
            }
        } else if let Some(explicit_ty) = &var_decl.ty {
            self.resolve_ast_type(explicit_ty)?
        } else {
            return Err(TypeError::Other(format!(
                "Variable '{}' must have a type or initializer",
                var_name
            )));
        };

        self.globals.insert(var_name, var_type);
        Ok(())
    }

    fn check_test_block(&mut self, test_block: &TestBlock) -> TypeResult<()> {
        self.symbols.push_scope();
        self.check_block(&test_block.body)?;
        self.symbols.pop_scope();
        Ok(())
    }

    fn check_block(&mut self, block: &Block) -> TypeResult<Type> {
        let mut last_type = Type::Void;

        for stmt in &block.stmts {
            last_type = self.check_stmt(stmt)?;
        }

        Ok(last_type)
    }

    fn check_stmt(&mut self, stmt: &Stmt) -> TypeResult<Type> {
        match stmt {
            Stmt::VarDecl(var_decl) => self.check_var_decl(var_decl),
            Stmt::Expression(expr) => self.check_expr(expr),
        }
    }

    fn check_var_decl(&mut self, var_decl: &VarDecl) -> TypeResult<Type> {
        if var_decl.names.len() > 1 {
            // Tuple destructuring
            if let Some(init) = &var_decl.init {
                let init_type = self.check_expr(init)?;

                // Extract tuple fields
                let tuple_fields = match &init_type {
                    Type::Tuple(tuple_type) => &tuple_type.fields,
                    _ => {
                        return Err(TypeError::Other(
                            "Tuple destructuring requires tuple initializer".to_string(),
                        ))
                    }
                };

                if tuple_fields.len() != var_decl.names.len() {
                    return Err(TypeError::Other(format!(
                        "Expected {} values for tuple destructuring, got {}",
                        var_decl.names.len(),
                        tuple_fields.len()
                    )));
                }

                // Add each variable to symbol table
                for (i, name) in var_decl.names.iter().enumerate() {
                    self.symbols
                        .add_variable(name.name.clone(), (*tuple_fields[i].ty).clone());
                }
            } else {
                return Err(TypeError::Other(
                    "Tuple destructuring requires initializer".to_string(),
                ));
            }

            Ok(Type::Void)
        } else {
            // Single variable declaration
            let var_name = var_decl.names[0].name.clone();

            let var_type = if let Some(init) = &var_decl.init {
                let init_type = self.check_expr(init)?;

                if let Some(explicit_ty) = &var_decl.ty {
                    let expected_ty = self.resolve_ast_type(explicit_ty)?;
                    if !init_type.can_convert_to(&expected_ty) {
                        return Err(TypeError::Incompatible {
                            expected: Box::new(expected_ty),
                            found: Box::new(init_type),
                            reason: format!("Cannot initialize variable '{}'", var_name),
                        });
                    }
                    expected_ty
                } else {
                    init_type
                }
            } else if let Some(explicit_ty) = &var_decl.ty {
                self.resolve_ast_type(explicit_ty)?
            } else {
                return Err(TypeError::Other(format!(
                    "Variable '{}' must have a type or initializer",
                    var_name
                )));
            };

            self.symbols.add_variable(var_name, var_type);
            Ok(Type::Void)
        }
    }

    fn check_expr(&mut self, expr: &Expr) -> TypeResult<Type> {
        match expr {
            Expr::Literal(lit, _) => Ok(self.type_of_literal(lit)),

            Expr::Ident(ident) => {
                // Check for C library function references (e.g., cstdlib::printf, cmath::sin)
                if ident.name.starts_with('c') && ident.name.contains("::") {
                    // C library functions are treated as function types
                    // For simplicity, we create a generic function type that accepts any args
                    // In a real implementation, we'd have proper signatures for each C function
                    
                    // Determine return type based on function name
                    let return_type = if ident.name.contains("::exit") {
                        Type::Void
                    } else if ident.name.starts_with("cmath::") {
                        if ident.name.ends_with('f') {
                            Type::Float(Some(32))
                        } else {
                            Type::Float(Some(64))
                        }
                    } else if ident.name.contains("::printf") {
                        Type::Int(None)
                    } else {
                        Type::Int(None)
                    };
                    
                    // Return a function type - for now we use an empty param list
                    // The actual type checking happens in check_call
                    return Ok(Type::Function(FunctionType {
                        const_params: vec![],
                        params: vec![], // Variadic - will be checked at call site
                        return_type: Some(Box::new(return_type)),
                    }));
                }
                
                // Look up in symbol table
                if let Some(ty) = self.symbols.lookup(&ident.name) {
                    Ok(ty.clone())
                } else if let Some(ty) = self.globals.get(&ident.name) {
                    Ok(ty.clone())
                } else if let Some((enum_name, _case, _idx)) = self.type_env.find_enum_case(&ident.name) {
                    // It's an enum case - treat it as a value of that enum type
                    // For now, return the enum type directly
                    // TODO: Handle enum cases with fields (which are constructors)
                    self.type_env.resolve_type(enum_name)
                } else {
                    Err(TypeError::UndefinedVariable {
                        name: ident.name.clone(),
                    })
                }
            }

            Expr::Binary {
                op,
                left,
                right,
                span: _,
            } => self.check_binary(op, left, right),

            Expr::Unary { op, expr, span: _ } => self.check_unary(op, expr),

            Expr::Call { func, args, span: _ } => self.check_call(func, args),

            Expr::MethodCall {
                receiver,
                method,
                args,
                span: _,
            } => self.check_method_call(receiver, method, args),

            Expr::FieldAccess {
                object,
                field,
                span: _,
            } => self.check_field_access(object, field),

            Expr::Tuple(elements, _) => self.check_tuple(elements),

            Expr::StructInit { ty, fields, span: _ } => self.check_struct_init(ty, fields),

            Expr::Closure {
                params,
                return_type,
                body,
                span: _,
            } => self.check_closure(params, return_type, body),

            Expr::Block(block) => self.check_block_expr(block),

            Expr::Match { expr, arms, span: _ } => self.check_match(expr, arms),

            Expr::Comptime { expr, span: _ } => {
                // For now, just check the inner expression
                // Full comptime evaluation would happen in a later stage
                self.check_expr(expr)
            }
        }
    }

    fn check_binary(&mut self, op: &BinOp, left: &Expr, right: &Expr) -> TypeResult<Type> {
        let left_ty = self.check_expr(left)?;
        let right_ty = self.check_expr(right)?;

        // Convert AST BinOp to Type BinaryOp
        let type_op = match op {
            BinOp::Add => BinaryOp::Add,
            BinOp::Sub => BinaryOp::Sub,
            BinOp::Mul => BinaryOp::Mul,
            BinOp::Div => BinaryOp::Div,
            BinOp::Mod => BinaryOp::Mod,
            BinOp::Eq => BinaryOp::Eq,
            BinOp::Ne => BinaryOp::Ne,
            BinOp::Lt => BinaryOp::Lt,
            BinOp::Le => BinaryOp::Le,
            BinOp::Gt => BinaryOp::Gt,
            BinOp::Ge => BinaryOp::Ge,
            BinOp::And => BinaryOp::And,
            BinOp::Or => BinaryOp::Or,
            BinOp::BitAnd => BinaryOp::BitAnd,
            BinOp::BitOr => BinaryOp::BitOr,
            BinOp::LShift => BinaryOp::LShift,
            BinOp::RShift => BinaryOp::RShift,
            BinOp::Concat => BinaryOp::Concat,
            BinOp::Assign | BinOp::AddAssign | BinOp::SubAssign | BinOp::MulAssign
            | BinOp::DivAssign | BinOp::ModAssign | BinOp::ConcatAssign => {
                // Assignment operators: check that left and right are compatible
                if !right_ty.can_convert_to(&left_ty) {
                    return Err(TypeError::Incompatible {
                        expected: Box::new(left_ty),
                        found: Box::new(right_ty),
                        reason: "Assignment type mismatch".to_string(),
                    });
                }
                return Ok(Type::Void);
            }
        };

        // Check operator support
        if !left_ty.supports_operator(&type_op) {
            return Err(TypeError::Other(format!(
                "Type {} does not support operator {:?}",
                left_ty, op
            )));
        }

        // Check type compatibility
        // Special case for Concat: allow Rune to be concatenated with String
        if type_op == BinaryOp::Concat {
            // Helper to check if a type is the String struct
            let is_string_type = |ty: &Type| -> bool {
                if let Type::Struct(s) = ty {
                    s.name == "String"
                } else {
                    false
                }
            };
            
            let is_left_string = is_string_type(&left_ty);
            let is_right_string = is_string_type(&right_ty);
            let is_left_rune = matches!(left_ty, Type::Rune);
            let is_right_rune = matches!(right_ty, Type::Rune);
            
            if is_left_string && is_right_rune {
                // Allow String ++ Rune -> returns String
                return Ok(left_ty);
            }
            
            if is_right_string && is_left_rune {
                // Allow Rune ++ String -> returns String
                return Ok(right_ty);
            }
        }
        
        if !left_ty.structurally_equal(&right_ty) {
            return Err(TypeError::Incompatible {
                expected: Box::new(left_ty.clone()),
                found: Box::new(right_ty),
                reason: format!("Binary operator {:?} requires matching types", op),
            });
        }

        // Determine result type
        match type_op {
            BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt
            | BinaryOp::Ge | BinaryOp::And | BinaryOp::Or => {
                // Comparison and logical operators return Bool enum
                Ok(self.type_env.resolve_type("Bool").unwrap())
            }
            BinaryOp::Concat => Ok(left_ty), // Concatenation returns same type
            _ => Ok(left_ty),                // Arithmetic returns same type
        }
    }

    fn check_unary(&mut self, op: &UnOp, expr: &Expr) -> TypeResult<Type> {
        let expr_ty = self.check_expr(expr)?;

        match op {
            UnOp::Neg => {
                if !expr_ty.is_numeric() {
                    return Err(TypeError::Other(format!(
                        "Unary negation requires numeric type, found {}",
                        expr_ty
                    )));
                }
                Ok(expr_ty)
            }
            UnOp::Not => {
                // Check if expression is Bool enum
                let bool_ty = self.type_env.resolve_type("Bool").unwrap();
                if !expr_ty.can_convert_to(&bool_ty) {
                    return Err(TypeError::Other(format!(
                        "Logical NOT requires Bool type, found {}",
                        expr_ty
                    )));
                }
                Ok(bool_ty)
            }
            UnOp::BitNot => {
                let bool_ty = self.type_env.resolve_type("Bool").ok();
                let is_bool = bool_ty.as_ref().map_or(false, |b| expr_ty.can_convert_to(b));
                
                if !expr_ty.is_integer() && !is_bool {
                    return Err(TypeError::Other(format!(
                        "Bitwise NOT requires integer or Bool type, found {}",
                        expr_ty
                    )));
                }
                Ok(expr_ty)
            }
        }
    }

    fn check_call(&mut self, func: &Expr, args: &[Expr]) -> TypeResult<Type> {
        // Special case: if func is an identifier, check if it's a function
        if let Expr::Ident(ident) = func {
            let func_name = ident.name.clone();
            
            // Check for C library function calls (e.g., cstdlib::printf, cmath::sin)
            if func_name.starts_with('c') && func_name.contains("::") {
                // This is a C library function call - type check the arguments and assume it returns Void
                // (or Int for most C functions - this is a simplified implementation)
                for arg in args {
                    self.check_expr(arg)?;
                }
                
                // Most C functions return Int, but printf and similar return Int
                // For now, we'll assume they all return Int (exit returns Void though)
                // This is a simplified type system for C interop
                if func_name.contains("::exit") || func_name.contains("::printf") {
                    return Ok(Type::Void);
                }
                
                // Math functions typically return Float or Float(32) depending on the suffix
                if func_name.starts_with("cmath::") {
                    // Functions ending in 'f' return Float(32), others return Float(64)
                    if func_name.ends_with('f') {
                        return Ok(Type::Float(Some(32)));
                    } else {
                        return Ok(Type::Float(Some(64)));
                    }
                }
                
                return Ok(Type::Int(None));
            }
            
            if let Some(signatures) = self.functions.get(&func_name).cloned() {
                // Try to find matching signature
                let arg_types: Result<Vec<_>, _> =
                    args.iter().map(|arg| self.check_expr(arg)).collect();
                let arg_types = arg_types?;

                for sig in &signatures {
                    if sig.params.len() == arg_types.len() {
                        let mut matches = true;
                        for (i, (_, param_ty)) in sig.params.iter().enumerate() {
                            if !arg_types[i].can_convert_to(param_ty) {
                                matches = false;
                                break;
                            }
                        }

                        if matches {
                            return Ok(sig.return_type.clone().unwrap_or(Type::Void));
                        }
                    }
                }

                return Err(TypeError::Other(format!(
                    "No matching overload for function '{}' with {} arguments",
                    func_name,
                    args.len()
                )));
            }

            // Check if it's an enum case constructor
            // We need to iterate over all known enums to find matching cases
            // Since we can't directly access the private enums field, we'll need to
            // try to resolve the identifier as a type and check if it's an enum
            // For now, we'll skip this optimization and handle it in a later pass
        }

        // General case: check if func is a function type
        let func_ty = self.check_expr(func)?;

        match func_ty {
            Type::Function(func_type) => {
                let arg_types: Result<Vec<_>, _> =
                    args.iter().map(|arg| self.check_expr(arg)).collect();
                let arg_types = arg_types?;

                if func_type.params.len() != arg_types.len() {
                    return Err(TypeError::Other(format!(
                        "Function expects {} arguments, got {}",
                        func_type.params.len(),
                        args.len()
                    )));
                }

                for (i, param_ty) in func_type.params.iter().enumerate() {
                    if !arg_types[i].can_convert_to(param_ty) {
                        return Err(TypeError::Incompatible {
                            expected: param_ty.clone(),
                            found: Box::new(arg_types[i].clone()),
                            reason: format!("Function argument {} type mismatch", i + 1),
                        });
                    }
                }

                Ok(func_type.return_type.map(|t| *t).unwrap_or(Type::Void))
            }
            _ => Err(TypeError::Other(format!(
                "Cannot call non-function type {}",
                func_ty
            ))),
        }
    }

    fn check_method_call(
        &mut self,
        receiver: &Expr,
        method: &Ident,
        args: &[Expr],
    ) -> TypeResult<Type> {
        // Uniform call syntax: receiver.method(args) is equivalent to method(receiver, args)
        let _receiver_ty = self.check_expr(receiver)?;

        // Build argument list with receiver as first argument
        let mut all_args = vec![receiver.clone()];
        all_args.extend_from_slice(args);

        // Try to find matching function
        let method_name = method.name.clone();
        if let Some(signatures) = self.functions.get(&method_name).cloned() {
            let arg_types: Result<Vec<_>, _> =
                all_args.iter().map(|arg| self.check_expr(arg)).collect();
            let arg_types = arg_types?;

            for sig in &signatures {
                if sig.params.len() == arg_types.len() {
                    let mut matches = true;
                    for (i, (_, param_ty)) in sig.params.iter().enumerate() {
                        if !arg_types[i].can_convert_to(param_ty) {
                            matches = false;
                            break;
                        }
                    }

                    if matches {
                        return Ok(sig.return_type.clone().unwrap_or(Type::Void));
                    }
                }
            }
        }

        Err(TypeError::Other(format!(
            "No method '{}' found",
            method.name
        )))
    }

    fn check_field_access(&mut self, object: &Expr, field: &Ident) -> TypeResult<Type> {
        let obj_ty = self.check_expr(object)?;

        match obj_ty {
            Type::Struct(struct_type) => {
                for struct_field in &struct_type.fields {
                    if struct_field.name == field.name {
                        return Ok((*struct_field.ty).clone());
                    }
                }
                Err(TypeError::Other(format!(
                    "Struct {} has no field '{}'",
                    struct_type.name, field.name
                )))
            }
            Type::Tuple(tuple_type) => {
                // Check for named field
                for tuple_field in &tuple_type.fields {
                    if let Some(name) = &tuple_field.name && name == &field.name {
                        return Ok((*tuple_field.ty).clone());
                    }
                }
                Err(TypeError::Other(format!(
                    "Tuple has no field '{}'",
                    field.name
                )))
            }
            _ => Err(TypeError::Other(format!(
                "Cannot access field on non-struct/tuple type {}",
                obj_ty
            ))),
        }
    }

    fn check_tuple(&mut self, elements: &[Expr]) -> TypeResult<Type> {
        let mut fields = Vec::new();

        for elem in elements {
            let elem_ty = self.check_expr(elem)?;
            fields.push(TupleField {
                name: None,
                ty: Box::new(elem_ty),
            });
        }

        Ok(Type::Tuple(TupleType {
            fields,
            variadic: None,
        }))
    }

    fn check_struct_init(
        &mut self,
        ty_name: &Option<Ident>,
        fields: &[FieldInit],
    ) -> TypeResult<Type> {
        if let Some(type_ident) = ty_name {
            // Named struct initialization
            let struct_type = self
                .type_env
                .get_struct(&type_ident.name)
                .ok_or_else(|| TypeError::Undefined {
                    name: type_ident.name.clone(),
                })?
                .clone();

            // Check that all fields match
            if fields.len() != struct_type.fields.len() {
                return Err(TypeError::Other(format!(
                    "Struct {} expects {} fields, got {}",
                    struct_type.name,
                    struct_type.fields.len(),
                    fields.len()
                )));
            }

            for (i, field_init) in fields.iter().enumerate() {
                let value_ty = self.check_expr(&field_init.value)?;
                let expected_ty = &struct_type.fields[i].ty;

                if let Some(field_name) = &field_init.name {
                    // Named field: find matching field
                    let mut found = false;
                    for struct_field in &struct_type.fields {
                        if struct_field.name == field_name.name {
                            if !value_ty.can_convert_to(&struct_field.ty) {
                                return Err(TypeError::Incompatible {
                                    expected: struct_field.ty.clone(),
                                    found: Box::new(value_ty),
                                    reason: format!("Field '{}' type mismatch", field_name.name),
                                });
                            }
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        return Err(TypeError::Other(format!(
                            "Struct {} has no field '{}'",
                            struct_type.name, field_name.name
                        )));
                    }
                } else {
                    // Positional field
                    if !value_ty.can_convert_to(expected_ty) {
                        return Err(TypeError::Incompatible {
                            expected: expected_ty.clone(),
                            found: Box::new(value_ty),
                            reason: format!("Field {} type mismatch", i + 1),
                        });
                    }
                }
            }

            Ok(Type::Struct(struct_type))
        } else {
            // Anonymous struct (tuple with named fields)
            let mut tuple_fields = Vec::new();

            for field_init in fields {
                let value_ty = self.check_expr(&field_init.value)?;
                tuple_fields.push(TupleField {
                    name: field_init.name.as_ref().map(|n| n.name.clone()),
                    ty: Box::new(value_ty),
                });
            }

            Ok(Type::Tuple(TupleType {
                fields: tuple_fields,
                variadic: None,
            }))
        }
    }

    fn check_closure(
        &mut self,
        params: &[Param],
        return_type: &Option<Box<atom_ast::Type>>,
        body: &Block,
    ) -> TypeResult<Type> {
        // Enter new scope for closure
        self.symbols.push_scope();

        // Add parameters to scope
        let mut param_types = Vec::new();
        for param in params {
            let param_ty = if let Some(ty_ast) = &param.ty {
                self.resolve_ast_type(ty_ast)?
            } else {
                return Err(TypeError::Other(
                    "Closure parameters must have explicit types".to_string(),
                ));
            };
            self.symbols
                .add_variable(param.name.name.clone(), param_ty.clone());
            param_types.push(Box::new(param_ty));
        }

        // Check body
        let body_ty = self.check_block(body)?;

        // Check return type
        let expected_return = if let Some(ret_ty_ast) = return_type {
            Some(Box::new(self.resolve_ast_type(ret_ty_ast)?))
        } else {
            None
        };

        if let Some(expected) = &expected_return && !body_ty.can_convert_to(expected) {
            return Err(TypeError::Incompatible {
                expected: expected.clone(),
                found: Box::new(body_ty),
                reason: "Closure return type mismatch".to_string(),
            });
        }

        self.symbols.pop_scope();

        Ok(Type::Function(FunctionType {
            const_params: vec![],
            params: param_types,
            return_type: expected_return.or_else(|| Some(Box::new(body_ty))),
        }))
    }

    fn check_block_expr(&mut self, block: &Block) -> TypeResult<Type> {
        self.symbols.push_scope();
        let result = self.check_block(block);
        self.symbols.pop_scope();
        result
    }

    fn check_match(&mut self, expr: &Expr, arms: &[MatchArm]) -> TypeResult<Type> {
        let expr_ty = self.check_expr(expr)?;

        if arms.is_empty() {
            return Err(TypeError::Other("Match expression must have at least one arm".to_string()));
        }

        // Check all arms and ensure they return compatible types
        let mut result_ty: Option<Type> = None;

        for arm in arms {
            // Check pattern matches expression type
            self.check_pattern(&arm.pattern, &expr_ty)?;

            // Enter new scope for pattern bindings
            self.symbols.push_scope();
            self.add_pattern_bindings(&arm.pattern, &expr_ty)?;

            let arm_ty = self.check_expr(&arm.body)?;

            self.symbols.pop_scope();

            // Check type compatibility
            if let Some(expected) = &result_ty {
                if !arm_ty.can_convert_to(expected) && !expected.can_convert_to(&arm_ty) {
                    return Err(TypeError::Incompatible {
                        expected: Box::new(expected.clone()),
                        found: Box::new(arm_ty),
                        reason: "Match arms have incompatible types".to_string(),
                    });
                }
                // TODO: find common type instead of using first arm's type
            } else {
                result_ty = Some(arm_ty);
            }
        }

        Ok(result_ty.unwrap())
    }

    fn check_pattern(&self, pattern: &Pattern, expected_ty: &Type) -> TypeResult<()> {
        match pattern {
            Pattern::Wildcard(_) => Ok(()), // Always matches

            Pattern::Literal(lit, _) => {
                let lit_ty = self.type_of_literal(lit);
                if !lit_ty.structurally_equal(expected_ty) {
                    return Err(TypeError::Incompatible {
                        expected: Box::new(expected_ty.clone()),
                        found: Box::new(lit_ty),
                        reason: "Pattern literal type mismatch".to_string(),
                    });
                }
                Ok(())
            }

            Pattern::Ident(ident) => {
                // Check if this is an enum case (zero-field constructor)
                if let Some((enum_name, case, _idx)) = self.type_env.find_enum_case(&ident.name) {
                    // It's an enum case - verify it matches the expected type
                    if let Type::Enum(enum_ty) = expected_ty {
                        if enum_ty.name == enum_name && case.fields.is_empty() {
                            Ok(())
                        } else {
                            Err(TypeError::Incompatible {
                                expected: Box::new(expected_ty.clone()),
                                found: Box::new(self.type_env.resolve_type(enum_name)?),
                                reason: format!("Pattern '{}' is an enum case", ident.name),
                            })
                        }
                    } else {
                        Err(TypeError::Incompatible {
                            expected: Box::new(expected_ty.clone()),
                            found: Box::new(self.type_env.resolve_type(enum_name)?),
                            reason: format!("Pattern '{}' is an enum case, not a binding", ident.name),
                        })
                    }
                } else {
                    // It's a regular binding variable
                    Ok(())
                }
            }

            Pattern::Tuple(patterns, _) => {
                if let Type::Tuple(tuple_ty) = expected_ty {
                    if patterns.len() != tuple_ty.fields.len() {
                        return Err(TypeError::Other(format!(
                            "Pattern expects {} elements, tuple has {}",
                            patterns.len(),
                            tuple_ty.fields.len()
                        )));
                    }

                    for (i, pattern) in patterns.iter().enumerate() {
                        self.check_pattern(pattern, &tuple_ty.fields[i].ty)?;
                    }

                    Ok(())
                } else {
                    Err(TypeError::Incompatible {
                        expected: Box::new(expected_ty.clone()),
                        found: Box::new(Type::Tuple(TupleType {
                            fields: vec![],
                            variadic: None,
                        })),
                        reason: "Pattern expects tuple".to_string(),
                    })
                }
            }

            Pattern::Enum { name, fields, .. } => {
                if let Type::Enum(enum_ty) = expected_ty {
                    // Find matching case
                    for case in &enum_ty.cases {
                        if case.name == name.name {
                            if fields.len() != case.fields.len() {
                                return Err(TypeError::Other(format!(
                                    "Enum case '{}' expects {} fields, pattern has {}",
                                    case.name,
                                    case.fields.len(),
                                    fields.len()
                                )));
                            }

                            for (i, pattern) in fields.iter().enumerate() {
                                self.check_pattern(pattern, &case.fields[i])?;
                            }

                            return Ok(());
                        }
                    }

                    Err(TypeError::Other(format!(
                        "Enum {} has no case '{}'",
                        enum_ty.name, name.name
                    )))
                } else {
                    Err(TypeError::Incompatible {
                        expected: Box::new(expected_ty.clone()),
                        found: Box::new(Type::Error), // Placeholder for unknown enum type
                        reason: "Pattern expects enum".to_string(),
                    })
                }
            }

            Pattern::Expr(_expr) => {
                // Expression patterns (guards) must evaluate to Bool
                // For now, we just accept them
                // TODO: type check the expression
                Ok(())
            }
        }
    }

    fn add_pattern_bindings(&mut self, pattern: &Pattern, ty: &Type) -> TypeResult<()> {
        match pattern {
            Pattern::Wildcard(_) | Pattern::Literal(_, _) => Ok(()),

            Pattern::Ident(ident) => {
                self.symbols.add_variable(ident.name.clone(), ty.clone());
                Ok(())
            }

            Pattern::Tuple(patterns, _) => {
                if let Type::Tuple(tuple_ty) = ty {
                    for (i, pattern) in patterns.iter().enumerate() {
                        self.add_pattern_bindings(pattern, &tuple_ty.fields[i].ty)?;
                    }
                }
                Ok(())
            }

            Pattern::Enum { name, fields, .. } => {
                if let Type::Enum(enum_ty) = ty {
                    for case in &enum_ty.cases {
                        if case.name == name.name {
                            for (i, pattern) in fields.iter().enumerate() {
                                self.add_pattern_bindings(pattern, &case.fields[i])?;
                            }
                            break;
                        }
                    }
                }
                Ok(())
            }

            Pattern::Expr(_) => Ok(()),
        }
    }

    // ========================================================================
    // Type Resolution
    // ========================================================================

    fn resolve_ast_type(&self, ast_type: &atom_ast::Type) -> TypeResult<Type> {
        match ast_type {
            atom_ast::Type::Named(ident) => self.type_env.resolve_type(&ident.name),

            atom_ast::Type::Param(ident) => Ok(Type::TypeParam(ident.name.clone())),

            atom_ast::Type::Tuple(types, _) => {
                let mut fields = Vec::new();
                for ty in types {
                    let resolved = self.resolve_ast_type(ty)?;
                    fields.push(TupleField {
                        name: None,
                        ty: Box::new(resolved),
                    });
                }
                Ok(Type::Tuple(TupleType {
                    fields,
                    variadic: None,
                }))
            }

            atom_ast::Type::Generic {
                name,
                params,
                span: _,
            } => {
                let base = self.type_env.resolve_type(&name.name)?;
                let mut args = Vec::new();

                for param in params {
                    // Type parameters in AST are TypeParam, need to convert
                    if let Some(ty) = &param.ty {
                        let resolved = self.resolve_ast_type(ty)?;
                        args.push(ConstArg::Type(Box::new(resolved)));
                    }
                }

                Ok(Type::Generic {
                    base: Box::new(base),
                    args,
                })
            }

            atom_ast::Type::Variadic {
                element,
                non_empty,
                span: _,
            } => {
                let elem_ty = self.resolve_ast_type(element)?;
                Ok(Type::Tuple(TupleType {
                    fields: vec![],
                    variadic: Some((Box::new(elem_ty), *non_empty)),
                }))
            }

            atom_ast::Type::StaticArray {
                element,
                size: _,
                span: _,
            } => {
                // For now, treat static arrays as tuples
                // TODO: properly handle static arrays with compile-time size
                let elem_ty = self.resolve_ast_type(element)?;
                Ok(Type::Tuple(TupleType {
                    fields: vec![],
                    variadic: Some((Box::new(elem_ty), false)),
                }))
            }

            atom_ast::Type::Function {
                params,
                return_type,
                span: _,
            } => {
                let mut param_types = Vec::new();
                for param_ty in params {
                    let resolved = self.resolve_ast_type(param_ty)?;
                    param_types.push(Box::new(resolved));
                }

                let ret_ty = if let Some(ret) = return_type {
                    Some(Box::new(self.resolve_ast_type(ret)?))
                } else {
                    None
                };

                Ok(Type::Function(FunctionType {
                    const_params: vec![],
                    params: param_types,
                    return_type: ret_ty,
                }))
            }
        }
    }

    fn ast_type_param_to_type_param(
        &self,
        param: &atom_ast::TypeParam,
    ) -> TypeResult<TypeParameter> {
        let name = param
            .name
            .as_ref()
            .map(|n| n.name.clone())
            .unwrap_or_else(|| "_".to_string());

        let constraint = if let Some(ty) = &param.ty {
            Some(Box::new(self.resolve_ast_type(ty)?))
        } else {
            Some(Box::new(Type::TypeMeta)) // Default to Type
        };

        let default = if let Some(default_ty) = &param.default {
            let resolved = self.resolve_ast_type(default_ty)?;
            Some(ConstArg::Type(Box::new(resolved)))
        } else {
            None
        };

        Ok(TypeParameter {
            name,
            constraint,
            default,
        })
    }

    fn type_of_literal(&self, lit: &Literal) -> Type {
        match lit {
            Literal::Integer(_) => Type::Int(None),
            Literal::Float(_) => Type::Float(None),
            Literal::String(_) => {
                // String literals use the String struct from stdlib
                // Safe to unwrap because stdlib should be loaded
                self.type_env.resolve_type("String").unwrap()
            }
            Literal::Rune(_) => Type::Rune,
            Literal::Bool(_) => {
                // Bool literals become the Bool enum type
                // Safe to unwrap because stdlib should be loaded
                self.type_env.resolve_type("Bool").unwrap()
            }
        }
    }
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use atom_ast::{Span, Visibility};

    fn make_span() -> Span {
        Span { start: 0, end: 0 }
    }

    fn make_ident(name: &str) -> Ident {
        Ident {
            name: name.to_string(),
            span: make_span(),
        }
    }

    #[test]
    fn test_simple_function() {
        let mut checker = TypeChecker::new();

        let func_def = FunctionDef {
            visibility: Visibility::Internal,
            name: make_ident("add"),
            const_params: vec![],
            params: vec![
                Param {
                    name: make_ident("a"),
                    ty: Some(Box::new(atom_ast::Type::Named(make_ident("Int")))),
                    default: None,
                    span: make_span(),
                },
                Param {
                    name: make_ident("b"),
                    ty: Some(Box::new(atom_ast::Type::Named(make_ident("Int")))),
                    default: None,
                    span: make_span(),
                },
            ],
            return_type: Some(Box::new(atom_ast::Type::Named(make_ident("Int")))),
            body: Block {
                stmts: vec![Stmt::Expression(Expr::Binary {
                    op: BinOp::Add,
                    left: Box::new(Expr::Ident(make_ident("a"))),
                    right: Box::new(Expr::Ident(make_ident("b"))),
                    span: make_span(),
                })],
                span: make_span(),
            },
            span: make_span(),
        };

        let program = vec![TopLevel::Function(func_def)];
        let result = checker.check_program(program);
        assert!(result.is_ok());
    }

    #[test]
    fn test_struct_definition() {
        let mut checker = TypeChecker::new();

        let struct_def = StructDef {
            visibility: Visibility::Public,
            name: make_ident("Vec2"),
            type_params: vec![],
            fields: vec![
                atom_ast::Field {
                    name: Some(make_ident("x")),
                    ty: Box::new(atom_ast::Type::Named(make_ident("Float"))),
                    span: make_span(),
                },
                atom_ast::Field {
                    name: Some(make_ident("y")),
                    ty: Box::new(atom_ast::Type::Named(make_ident("Float"))),
                    span: make_span(),
                },
            ],
            span: make_span(),
        };

        let program = vec![TopLevel::Struct(struct_def)];
        let result = checker.check_program(program);
        assert!(result.is_ok());

        // Verify struct was added to type environment
        assert!(checker.type_env.get_struct("Vec2").is_some());
    }

    #[test]
    fn test_enum_definition() {
        let mut checker = TypeChecker::new();

        let enum_def = atom_ast::EnumDef {
            visibility: Visibility::Public,
            name: make_ident("Option"),
            type_params: vec![atom_ast::TypeParam {
                name: Some(make_ident("t")),
                ty: None,
                default: None,
                span: make_span(),
            }],
            cases: vec![
                atom_ast::EnumCase {
                    name: make_ident("Some"),
                    fields: vec![Box::new(atom_ast::Type::Param(make_ident("t")))],
                    span: make_span(),
                },
                atom_ast::EnumCase {
                    name: make_ident("None"),
                    fields: vec![],
                    span: make_span(),
                },
            ],
            span: make_span(),
        };

        let program = vec![TopLevel::Enum(enum_def)];
        let result = checker.check_program(program);
        assert!(result.is_ok());
    }

    #[test]
    fn test_type_mismatch_error() {
        let mut checker = TypeChecker::new();

        // Function that returns Int but body is String
        let func_def = FunctionDef {
            visibility: Visibility::Internal,
            name: make_ident("bad_func"),
            const_params: vec![],
            params: vec![],
            return_type: Some(Box::new(atom_ast::Type::Named(make_ident("Int")))),
            body: Block {
                stmts: vec![Stmt::Expression(Expr::Literal(
                    Literal::String("hello".to_string()),
                    make_span(),
                ))],
                span: make_span(),
            },
            span: make_span(),
        };

        let program = vec![TopLevel::Function(func_def)];
        let result = checker.check_program(program);
        assert!(result.is_err());
    }

    #[test]
    fn test_match_expression() {
        let mut checker = TypeChecker::new();

        // match(True) { True { 1 } False { 0 } }
        let match_expr = Expr::Match {
            expr: Box::new(Expr::Literal(Literal::Bool(true), make_span())),
            arms: vec![
                MatchArm {
                    pattern: Pattern::Literal(Literal::Bool(true), make_span()),
                    body: Box::new(Expr::Literal(Literal::Integer(1), make_span())),
                    span: make_span(),
                },
                MatchArm {
                    pattern: Pattern::Literal(Literal::Bool(false), make_span()),
                    body: Box::new(Expr::Literal(Literal::Integer(0), make_span())),
                    span: make_span(),
                },
            ],
            span: make_span(),
        };

        let result = checker.check_expr(&match_expr);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Type::Int(None));
    }

    #[test]
    fn test_tuple_destructuring() {
        let mut checker = TypeChecker::new();

        // a, b := 1, 2
        let var_decl = VarDecl {
            visibility: Visibility::Internal,
            is_const: false,
            names: vec![make_ident("a"), make_ident("b")],
            ty: None,
            init: Some(Box::new(Expr::Tuple(
                vec![
                    Expr::Literal(Literal::Integer(1), make_span()),
                    Expr::Literal(Literal::Integer(2), make_span()),
                ],
                make_span(),
            ))),
            span: make_span(),
        };

        let result = checker.check_var_decl(&var_decl);
        assert!(result.is_ok());

        // Check that both variables are in symbol table
        assert!(checker.symbols.lookup("a").is_some());
        assert!(checker.symbols.lookup("b").is_some());
    }

    #[test]
    fn test_closure_type_checking() {
        let mut checker = TypeChecker::new();

        // (x Int, y Int) Int { x + y }
        let closure = Expr::Closure {
            params: vec![
                Param {
                    name: make_ident("x"),
                    ty: Some(Box::new(atom_ast::Type::Named(make_ident("Int")))),
                    default: None,
                    span: make_span(),
                },
                Param {
                    name: make_ident("y"),
                    ty: Some(Box::new(atom_ast::Type::Named(make_ident("Int")))),
                    default: None,
                    span: make_span(),
                },
            ],
            return_type: Some(Box::new(atom_ast::Type::Named(make_ident("Int")))),
            body: Box::new(Block {
                stmts: vec![Stmt::Expression(Expr::Binary {
                    op: BinOp::Add,
                    left: Box::new(Expr::Ident(make_ident("x"))),
                    right: Box::new(Expr::Ident(make_ident("y"))),
                    span: make_span(),
                })],
                span: make_span(),
            }),
            span: make_span(),
        };

        let result = checker.check_expr(&closure);
        assert!(result.is_ok());

        if let Type::Function(func_ty) = result.unwrap() {
            assert_eq!(func_ty.params.len(), 2);
            assert_eq!(*func_ty.return_type.unwrap(), Type::Int(None));
        } else {
            panic!("Expected function type");
        }
    }
}
