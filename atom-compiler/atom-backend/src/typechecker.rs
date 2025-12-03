#![allow(unused)]
#![allow(clippy::all)]

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
    /// Regular parameters with names and whether they have defaults
    pub params: Vec<(String, Type, bool)>, // (name, type, has_default)
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
    /// Module names for each top-level item (None for user code, Some("module") for deps)
    item_modules: Vec<Option<String>>,
    /// Current item index being processed
    current_item_index: usize,
    /// Imported symbols: maps unqualified name -> fully qualified name
    imports: HashMap<String, String>,
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
            item_modules: Vec::new(),
            current_item_index: 0,
            imports: HashMap::new(),
        }
    }

    /// Create a new type checker with module information for each item
    pub fn new_with_modules(modules: Vec<Option<String>>) -> Self {
        Self {
            type_env: TypeEnvironment::with_stdlib(),
            symbols: SymbolTable::new(),
            functions: HashMap::new(),
            globals: HashMap::new(),
            current_return_type: None,
            item_modules: modules,
            current_item_index: 0,
            imports: HashMap::new(),
        }
    }

    /// Get the current item's module name (if any)
    fn current_module(&self) -> Option<&str> {
        self.item_modules.get(self.current_item_index)
            .and_then(|opt| opt.as_deref())
    }

    /// Qualify a name with the current module if applicable
    fn qualify_name(&self, name: &str) -> String {
        if let Some(module) = self.current_module() {
            format!("{}::{}", module, name)
        } else {
            name.to_string()
        }
    }

    /// Process an import declaration
    fn process_import(&mut self, import: &atom_ast::ImportDecl) -> TypeResult<()> {
        let namespace = &import.namespace.name;
        
        match &import.items {
            atom_ast::ImportItems::All => {
                // Import all public items from the namespace
                // For now, we'll handle this by allowing lookups to check both qualified and unqualified names
                // We add a wildcard marker
                self.imports.insert(format!("*::{}", namespace), namespace.clone());
            }
            atom_ast::ImportItems::Named(items) => {
                // Import specific items
                for item in items {
                    let unqualified = item.name.clone();
                    let qualified = format!("{}::{}", namespace, unqualified);
                    self.imports.insert(unqualified, qualified);
                }
            }
        }
        Ok(())
    }

    /// Resolve a name, considering imports
    fn resolve_name(&self, name: &str) -> String {
        // If name is already qualified (contains ::), use it as-is
        if name.contains("::") {
            return name.to_string();
        }

        // Check if this name was explicitly imported
        if let Some(qualified) = self.imports.get(name) {
            return qualified.clone();
        }

        // Check if there's a wildcard import that might match
        for (key, namespace) in &self.imports {
            if key.starts_with("*::") {
                let ns = &key[3..];
                let qualified_name = format!("{}::{}", ns, name);
                // Check if this name exists in functions or types
                if self.functions.contains_key(&qualified_name) {
                    return qualified_name;
                }
                // We could also check type_env here but for now this is sufficient
            }
        }

        // Otherwise use the unqualified name
        name.to_string()
    }

    /// Type check a complete program (Vec of top-level items)
    pub fn check_program(&mut self, ast: Vec<TopLevel>) -> TypeResult<TypedProgram> {
        // First pass: process imports
        for (index, item) in ast.iter().enumerate() {
            self.current_item_index = index;
            if let TopLevel::Import(import_decl) = item {
                self.process_import(import_decl)?;
            }
        }

        // Second pass: collect all type definitions
        for (index, item) in ast.iter().enumerate() {
            self.current_item_index = index;
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

        // Third pass: collect function signatures
        for (index, item) in ast.iter().enumerate() {
            self.current_item_index = index;
            if let TopLevel::Function(func_def) = item {
                self.collect_function_signature(func_def)?;
            }
        }

        // Fourth pass: type check all items
        for (index, item) in ast.iter().enumerate() {
            self.current_item_index = index;
            match item {
                TopLevel::Import(_) => {
                    // Already processed in first pass
                }
                TopLevel::Struct(_) | TopLevel::Enum(_) => {
                    // Already processed in second pass
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

            // Validate that the field type is not a reference type
            if let atom_ast::Type::Reference { .. } = &*field.ty {
                return Err(TypeError::Other(format!(
                    "Reference types are not allowed in struct fields (field '{}' in struct '{}')",
                    field_name, struct_def.name.name
                )));
            }

            let field_type = self.resolve_ast_type(&field.ty)?;

            fields.push(StructField {
                name: field_name,
                ty: Box::new(field_type),
            });
        }

        let struct_type = StructType {
            name: self.qualify_name(&struct_def.name.name),
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
            name: self.qualify_name(&enum_def.name.name),
            params: type_params,
            cases,
            visibility: enum_def.visibility,
        };

        self.type_env.add_enum(enum_type);
        Ok(())
    }

    fn collect_function_signature(&mut self, func_def: &FunctionDef) -> TypeResult<()> {
        let func_name = self.qualify_name(&func_def.name.name);

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

            let has_default = param.default.is_some();
            params.push((param_name, param_ty, has_default));
        }

        let return_type = if let Some(ret_ty_ast) = &func_def.return_type {
            Some(self.resolve_ast_type(ret_ty_ast)?)
        } else {
            None
        };

        let signature = FunctionSignature {
            const_params,
            params: params.clone(),
            return_type: return_type.clone(),
        };

        if func_name == "reduce" {
                                    for (i, (name, ty, has_default)) in params.iter().enumerate() {
                            }
                    }

        self.functions
            .entry(func_name.clone())
            .or_default()
            .push(signature);

        if func_name == "reduce" {
                    }

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
            
            // When adding to scope, unwrap reference types since variables hold values
            let scope_ty = match &param_ty {
                Type::Reference(inner) => (**inner).clone(),
                _ => param_ty.clone(),
            };
            self.symbols.add_variable(param_name, scope_ty);
        }

        // Set current return type and validate it doesn't contain references
        let return_type = if let Some(ret_ty_ast) = &func_def.return_type {
            let resolved = self.resolve_ast_type(ret_ty_ast)?;
            
            // Validate return type doesn't contain reference types
            if self.contains_reference_type(&resolved) {
                return Err(TypeError::Other(format!(
                    "Reference types are not allowed in return types (function '{}')",
                    func_def.name.name
                )));
            }
            
            Some(resolved)
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
                // Resolve name considering imports
                let resolved_name = self.resolve_name(&ident.name);
                
                // Check for C library function references (e.g., cstdlib::printf, cmath::sin)
                if resolved_name.starts_with('c') && resolved_name.contains("::") {
                    // C library functions are treated as function types
                    // For simplicity, we create a generic function type that accepts any args
                    // In a real implementation, we'd have proper signatures for each C function
                    
                    // Determine return type based on function name
                    let return_type = if resolved_name.contains("::exit") {
                        Type::Void
                    } else if resolved_name.starts_with("cmath::") {
                        if resolved_name.ends_with('f') {
                            Type::Float(Some(32))
                        } else {
                            Type::Float(Some(64))
                        }
                    } else if resolved_name.contains("::printf") {
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
                
                // Check if it's a user-defined function
                if self.functions.contains_key(&resolved_name) {
                    // Return a function type - actual signatures will be resolved at call site
                    // We return a generic function type here
                    return Ok(Type::Function(FunctionType {
                        const_params: vec![],
                        params: vec![],
                        return_type: None, // Will be determined at call site
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

            Expr::MultiComparison {
                operands,
                operators,
                span: _,
            } => self.check_multi_comparison(operands, operators),

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

            Expr::Reference { expr, .. } => {
                // Validate that & is only applied to lvalues
                if !self.is_lvalue(expr) {
                    return Err(TypeError::Other(
                        "The & operator can only be applied to lvalues (variables, array elements, or struct fields)".to_string()
                    ));
                }
                
                // Check the inner expression type and wrap it in a Reference type
                let inner_ty = self.check_expr(expr)?;
                Ok(Type::Reference(Box::new(inner_ty)))
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
            | BinOp::DivAssign | BinOp::ModAssign => {
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
            BinOp::ConcatAssign => {
                // Special handling for ++=: can append element to variadic tuple or concat strings
                // Check if left is variadic tuple and right is element type
                if let Type::Tuple(left_tuple) = &left_ty {
                    if let Some((element_ty, _)) = &left_tuple.variadic {
                        // Allow appending element to variadic tuple
                        if right_ty.can_convert_to(element_ty) {
                            return Ok(Type::Void);
                        }
                    }
                }
                
                // Check if both are String (for string concatenation)
                let is_left_string = if let Type::Struct(s) = &left_ty {
                    s.name == "String"
                } else {
                    false
                };
                let is_right_string = if let Type::Struct(s) = &right_ty {
                    s.name == "String"
                } else {
                    false
                };
                // Check if right is Rune or can convert to Rune (like UInt(8))
                let is_right_rune_compatible = matches!(right_ty, Type::Rune) || right_ty.can_convert_to(&Type::Rune);
                
                if is_left_string && (is_right_string || is_right_rune_compatible) {
                    return Ok(Type::Void);
                }
                
                // Otherwise, check normal assignment compatibility
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
        // Special case for Concat: allow various concatenation operations
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
            
            // String ++ Rune concatenation
            if is_left_string && is_right_rune {
                return Ok(left_ty);
            }
            
            // Rune ++ String concatenation
            if is_right_string && is_left_rune {
                return Ok(right_ty);
            }
            
            // Handle tuple concatenation
            match (&left_ty, &right_ty) {
                // Variadic tuple ++ element: Int* ++ Int
                (Type::Tuple(left_tuple), _) if left_tuple.variadic.is_some() => {
                    if let Some((elem_ty, _)) = &left_tuple.variadic {
                        // Check if right type matches the variadic element type
                        if right_ty.can_convert_to(elem_ty) {
                            return Ok(left_ty);
                        }
                    }
                }
                
                // Variadic tuple ++ variadic tuple: Int* ++ Int*
                (Type::Tuple(left_tuple), Type::Tuple(right_tuple)) 
                    if left_tuple.variadic.is_some() && right_tuple.variadic.is_some() => {
                    if let (Some((left_elem, _)), Some((right_elem, _))) = 
                        (&left_tuple.variadic, &right_tuple.variadic) {
                        // Element types must be compatible
                        if left_elem.structurally_equal(right_elem) {
                            return Ok(left_ty);
                        }
                    }
                }
                
                // Fixed tuple ++ fixed tuple: (Int, Float) ++ (String, Bool)
                (Type::Tuple(left_tuple), Type::Tuple(right_tuple))
                    if left_tuple.variadic.is_none() && right_tuple.variadic.is_none() => {
                    // Concatenate the field types
                    let mut combined_fields = left_tuple.fields.clone();
                    combined_fields.extend(right_tuple.fields.clone());
                    
                    return Ok(Type::Tuple(TupleType {
                        fields: combined_fields,
                        variadic: None,
                    }));
                }
                
                _ => {}
            }
        }
        
        if !left_ty.structurally_equal(&right_ty) {
            // Allow type conversions in binary operations:
            // - Int to Float (for parser bug where 1.0 is parsed as Int)
            // - Int to UInt (for binary literals in bitwise operations)
            // - Rune with Int/UInt (for rune comparisons with literals)
            let compatible = match (&left_ty, &right_ty) {
                (Type::Int(_), Type::Float(_)) | (Type::Float(_), Type::Int(_)) => true,
                (Type::Int(_), Type::UInt(_)) | (Type::UInt(_), Type::Int(_)) => true,
                (Type::Rune, Type::Int(_)) | (Type::Int(_), Type::Rune) => true,
                (Type::Rune, Type::UInt(_)) | (Type::UInt(_), Type::Rune) => true,
                _ => false,
            };
            
            if !compatible {
                return Err(TypeError::Incompatible {
                    expected: Box::new(left_ty.clone()),
                    found: Box::new(right_ty),
                    reason: format!("Binary operator {:?} requires matching types", op),
                });
            }
            
            // For comparison and logical operators, don't promote - let result type determination handle it
            if !matches!(type_op, BinaryOp::Eq | BinaryOp::Ne | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge | BinaryOp::And | BinaryOp::Or) {
                // Promote to more specific type for arithmetic/bitwise operations:
                // - Float over Int
                // - Rune over Int/UInt
                // - Sized type over unsized (UInt(8) over Int(None))
                if matches!(left_ty, Type::Float(_)) {
                    return Ok(left_ty);
                } else if matches!(right_ty, Type::Float(_)) {
                    return Ok(right_ty);
                } else if matches!(left_ty, Type::Rune) {
                    return Ok(left_ty);
                } else if matches!(right_ty, Type::Rune) {
                    return Ok(right_ty);
                } else if matches!(left_ty, Type::UInt(Some(_))) {
                    return Ok(left_ty);
                } else if matches!(right_ty, Type::UInt(Some(_))) {
                    return Ok(right_ty);
                } else {
                    return Ok(left_ty);
                }
            }
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

    fn check_multi_comparison(
        &mut self,
        operands: &[Expr],
        operators: &[BinOp],
    ) -> TypeResult<Type> {
        // Multi-way comparisons like a < b < c or x == y == True
        // Requirements:
        // 1. All operators must be comparison operators
        // 2. All operands must be compatible with the comparison operators
        // 3. For == and !=, all operands must have the same type (or compatible types)
        // 4. For ordering comparisons (<, <=, >, >=), all operands must be numeric or orderable
        
        if operands.len() < 2 {
            return Err(TypeError::Other(
                "Multi-way comparison requires at least 2 operands".to_string(),
            ));
        }
        
        if operands.len() != operators.len() + 1 {
            return Err(TypeError::Other(
                "Multi-way comparison must have n+1 operands for n operators".to_string(),
            ));
        }
        
        // Check that all operators are comparison operators
        for op in operators {
            if !matches!(op, BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge) {
                return Err(TypeError::Other(format!(
                    "Multi-way comparison can only use comparison operators, found {:?}",
                    op
                )));
            }
        }
        
        // Type-check all operands
        let mut operand_types = Vec::new();
        for operand in operands {
            operand_types.push(self.check_expr(operand)?);
        }
        
        // Check type compatibility based on the operator type
        let first_op = &operators[0];
        let is_equality = matches!(first_op, BinOp::Eq | BinOp::Ne);
        let is_ordering = matches!(first_op, BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge);
        
        // All operators must be of the same category (all equality or all ordering)
        for op in operators {
            let op_is_equality = matches!(op, BinOp::Eq | BinOp::Ne);
            let op_is_ordering = matches!(op, BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge);
            
            if is_equality && !op_is_equality {
                return Err(TypeError::Other(
                    "Cannot mix equality and ordering operators in multi-way comparison".to_string(),
                ));
            }
            if is_ordering && !op_is_ordering {
                return Err(TypeError::Other(
                    "Cannot mix equality and ordering operators in multi-way comparison".to_string(),
                ));
            }
        }
        
        if is_equality {
            // For equality comparisons (a == b == c), all operands must have the same type
            // This implements the requirement that a == b == True means both a and b are True
            let first_type = &operand_types[0];
            
            for (i, operand_type) in operand_types.iter().enumerate().skip(1) {
                if !operand_type.can_convert_to(first_type) && !first_type.can_convert_to(operand_type) {
                    return Err(TypeError::Incompatible {
                        expected: Box::new(first_type.clone()),
                        found: Box::new(operand_type.clone()),
                        reason: format!(
                            "Multi-way equality comparison requires all operands to have matching types (operand {} has incompatible type)",
                            i
                        ),
                    });
                }
            }
        } else if is_ordering {
            // For ordering comparisons (a < b < c), all operands must support comparison
            for (i, operand_type) in operand_types.iter().enumerate() {
                if !operand_type.supports_comparison() {
                    return Err(TypeError::Other(format!(
                        "Multi-way ordering comparison requires all operands to support comparison, operand {} has type {}",
                        i, operand_type
                    )));
                }
            }
            
            // Also check that all types are compatible (can be compared)
            let first_type = &operand_types[0];
            for (i, operand_type) in operand_types.iter().enumerate().skip(1) {
                if !operand_type.can_convert_to(first_type) && !first_type.can_convert_to(operand_type) {
                    return Err(TypeError::Incompatible {
                        expected: Box::new(first_type.clone()),
                        found: Box::new(operand_type.clone()),
                        reason: format!(
                            "Multi-way ordering comparison requires all operands to have compatible types (operand {} has incompatible type)",
                            i
                        ),
                    });
                }
            }
        }
        
        // Multi-way comparison always returns Bool
        Ok(self.type_env.resolve_type("Bool").unwrap())
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
            let func_name = self.resolve_name(&ident.name);
            
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
                    // Math functions ending in 'f' suffix (like cosf, sinf) return Float(32)
                    // But not functions whose name ends in 'f' (like erf, erfc)
                    if func_name.ends_with("cosf") || func_name.ends_with("sinf") || 
                       func_name.ends_with("tanf") || func_name.ends_with("acosf") ||
                       func_name.ends_with("asinf") || func_name.ends_with("atanf") ||
                       func_name.ends_with("coshf") || func_name.ends_with("sinhf") ||
                       func_name.ends_with("tanhf") || func_name.ends_with("acoshf") ||
                       func_name.ends_with("asinhf") || func_name.ends_with("atanhf") ||
                       func_name.ends_with("expf") || func_name.ends_with("exp2f") ||
                       func_name.ends_with("expm1f") || func_name.ends_with("logf") ||
                       func_name.ends_with("log10f") || func_name.ends_with("log2f") ||
                       func_name.ends_with("powf") || func_name.ends_with("sqrtf") ||
                       func_name.ends_with("cbrtf") || func_name.ends_with("hypotf") ||
                       func_name.ends_with("ceilf") || func_name.ends_with("floorf") ||
                       func_name.ends_with("truncf") || func_name.ends_with("roundf") ||
                       func_name.ends_with("fmodf") || func_name.ends_with("remainderf") ||
                       func_name.ends_with("fabsf") || func_name.ends_with("copysignf") ||
                       func_name.ends_with("fminf") || func_name.ends_with("fmaxf") ||
                       func_name.ends_with("erff") || func_name.ends_with("erfcf") ||
                       func_name.ends_with("tgammaf") || func_name.ends_with("lgammaf") ||
                       func_name.ends_with("atan2f") {
                        return Ok(Type::Float(Some(32)));
                    } else {
                        return Ok(Type::Float(None));
                    }
                }
                
                return Ok(Type::Int(None));
            }
            
            // Handle loop builtin
            if func_name == "loop" {
                return self.check_loop(args);
            }
            
            // Handle as_string builtin
            if func_name == "as_string" {
                return self.check_as_string(args);
            }
            
            if let Some(signatures) = self.functions.get(&func_name).cloned() {
                // Try to find matching signature by type-checking args with expected param types
                // This allows proper type inference for implicit closure parameters
                
                for sig in signatures.iter() {
                    // Count required (non-default) parameters
                    let required_params = sig.params.iter()
                        .take_while(|(_, _, has_default)| !has_default)
                        .count();
                    
                    // Check if arg count is valid (between required and total params)
                    if args.len() >= required_params && args.len() <= sig.params.len() {
                        // Type check arguments with expected parameter types for context
                        let mut arg_types = Vec::new();
                        let mut type_bindings = std::collections::HashMap::new();
                        let mut matches = true;
                        
                        for (i, arg) in args.iter().enumerate() {
                            let expected_param_ty = &sig.params[i].1;
                            
                            // Validate reference parameter usage
                            if let Type::Reference(_) = expected_param_ty {
                                // Parameter expects a reference, ensure arg uses &
                                if !matches!(arg, Expr::Reference { .. }) {
                                    return Err(TypeError::Other(format!(
                                        "Function '{}' parameter {} expects a reference. Use '&' at the call site",
                                        func_name, i + 1
                                    )));
                                }
                            } else {
                                // Parameter expects a value, ensure arg doesn't use &
                                if matches!(arg, Expr::Reference { .. }) {
                                    return Err(TypeError::Other(format!(
                                        "Function '{}' parameter {} does not expect a reference. Remove '&' from the argument",
                                        func_name, i + 1
                                    )));
                                }
                            }
                            
                            // Check if this argument is a block with implicit parameters
                            let arg_ty = if let Expr::Block(block) = arg {
                                self.check_block_expr_with_context(block, Some(expected_param_ty))?
                            } else {
                                self.check_expr(arg)?
                            };
                            
                            if !self.try_unify_type(&arg_ty, expected_param_ty, &mut type_bindings) {
                                matches = false;
                                break;
                            }
                            arg_types.push(arg_ty);
                        }

                        if matches {
                            // Substitute type parameters in return type
                            let return_ty = sig.return_type.clone().unwrap_or(Type::Void);
                            let substituted = self.substitute_type_params(&return_ty, &type_bindings);
                            return Ok(substituted);
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
            Type::Tuple(tuple_ty) => {
                // Tuple indexing: arr(i) returns the element type
                // For now, we require exactly one argument (the index)
                if args.len() != 1 {
                    return Err(TypeError::Other(format!(
                        "Tuple indexing expects 1 argument, got {}",
                        args.len()
                    )));
                }

                let index_ty = self.check_expr(&args[0])?;
                // Index should be Int
                if !matches!(index_ty, Type::Int(_)) {
                    return Err(TypeError::Incompatible {
                        expected: Box::new(Type::Int(None)),
                        found: Box::new(index_ty),
                        reason: "Tuple index must be Int".to_string(),
                    });
                }

                // Return the element type
                // For variadic tuples, return the variadic element type
                // For fixed tuples, we'd need compile-time index knowledge (simplified here)
                if let Some((elem_ty, _)) = &tuple_ty.variadic {
                    Ok((**elem_ty).clone())
                } else if !tuple_ty.fields.is_empty() {
                    // For non-variadic tuples, return the first field type as approximation
                    // (ideally we'd check the index is a compile-time constant)
                    Ok((*tuple_ty.fields[0].ty).clone())
                } else {
                    Err(TypeError::Other("Cannot index into empty tuple".to_string()))
                }
            }
            Type::Function(func_type) => {
                // Type check arguments with expected parameter types from function signature
                if func_type.params.len() != args.len() {
                    return Err(TypeError::Other(format!(
                        "Function expects {} arguments, got {}",
                        func_type.params.len(),
                        args.len()
                    )));
                }
                
                let mut arg_types = Vec::new();
                for (i, arg) in args.iter().enumerate() {
                    let expected_param_ty = &func_type.params[i];
                    
                    // Validate reference parameter usage
                    if let Type::Reference(_) = **expected_param_ty {
                        // Parameter expects a reference, ensure arg uses &
                        if !matches!(arg, Expr::Reference { .. }) {
                            return Err(TypeError::Other(format!(
                                "Function parameter {} expects a reference. Use '&' at the call site",
                                i + 1
                            )));
                        }
                    } else {
                        // Parameter expects a value, ensure arg doesn't use &
                        if matches!(arg, Expr::Reference { .. }) {
                            return Err(TypeError::Other(format!(
                                "Function parameter {} does not expect a reference. Remove '&' from the argument",
                                i + 1
                            )));
                        }
                    }
                    
                    // Check if this argument is a block with implicit parameters
                    let arg_ty = if let Expr::Block(block) = arg {
                        self.check_block_expr_with_context(block, Some(expected_param_ty))?
                    } else {
                        self.check_expr(arg)?
                    };
                    
                    if !arg_ty.can_convert_to(expected_param_ty) {
                        return Err(TypeError::Incompatible {
                            expected: expected_param_ty.clone(),
                            found: Box::new(arg_ty.clone()),
                            reason: format!("Function argument {} type mismatch", i + 1),
                        });
                    }
                    arg_types.push(arg_ty);
                }

                Ok(func_type.return_type.map(|t| *t).unwrap_or(Type::Void))
            }
            _ => Err(TypeError::Other(format!(
                "Cannot call non-function type {} (expression: {:?})",
                func_ty, func
            ))),
        }
    }

    fn check_method_call(
        &mut self,
        receiver: &Expr,
        method: &Ident,
        args: &[Expr],
    ) -> TypeResult<Type> {
        let method_name = method.name.clone();
        
        // Check for C library method calls (e.g., x.cmath::cos())
        if method_name.starts_with('c') && method_name.contains("::") {
            // Type check receiver and arguments
            let _receiver_ty = self.check_expr(receiver)?;
            for arg in args {
                self.check_expr(arg)?;
            }
            
            // Determine return type based on C library namespace and function name
            if method_name.starts_with("cmath::") {
                // Math functions ending in 'f' suffix (like cosf, sinf) return Float(32)
                // But not functions whose name ends in 'f' (like erf, erfc)
                // The pattern is: known_function_name + 'f'
                if method_name.ends_with("cosf") || method_name.ends_with("sinf") || 
                   method_name.ends_with("tanf") || method_name.ends_with("acosf") ||
                   method_name.ends_with("asinf") || method_name.ends_with("atanf") ||
                   method_name.ends_with("coshf") || method_name.ends_with("sinhf") ||
                   method_name.ends_with("tanhf") || method_name.ends_with("acoshf") ||
                   method_name.ends_with("asinhf") || method_name.ends_with("atanhf") ||
                   method_name.ends_with("expf") || method_name.ends_with("exp2f") ||
                   method_name.ends_with("expm1f") || method_name.ends_with("logf") ||
                   method_name.ends_with("log10f") || method_name.ends_with("log2f") ||
                   method_name.ends_with("powf") || method_name.ends_with("sqrtf") ||
                   method_name.ends_with("cbrtf") || method_name.ends_with("hypotf") ||
                   method_name.ends_with("ceilf") || method_name.ends_with("floorf") ||
                   method_name.ends_with("truncf") || method_name.ends_with("roundf") ||
                   method_name.ends_with("fmodf") || method_name.ends_with("remainderf") ||
                   method_name.ends_with("fabsf") || method_name.ends_with("copysignf") ||
                   method_name.ends_with("fminf") || method_name.ends_with("fmaxf") ||
                   method_name.ends_with("erff") || method_name.ends_with("erfcf") ||
                   method_name.ends_with("tgammaf") || method_name.ends_with("lgammaf") ||
                   method_name.ends_with("atan2f") {
                    return Ok(Type::Float(Some(32)));
                } else {
                    return Ok(Type::Float(None));
                }
            } else if method_name.starts_with("cstdio::") {
                // Most stdio functions return Int
                return Ok(Type::Int(None));
            } else if method_name.starts_with("cstdlib::") {
                // Most stdlib functions return Int or Void
                if method_name.contains("::exit") || method_name.contains("::abort") {
                    return Ok(Type::Void);
                }
                return Ok(Type::Int(None));
            } else if method_name.starts_with("cstring::") {
                // String functions typically return pointers (we'll use Int for now)
                return Ok(Type::Int(None));
            } else {
                // Default for other C libraries
                return Ok(Type::Int(None));
            }
        }
        
        // Check if this is actually a field access followed by indexing
        // In Atom, s.bytes(i) means: access field 'bytes', then index it with 'i'
        let receiver_ty = self.check_expr(receiver)?;
        
        // Try to get field from receiver type
        let field_ty = match &receiver_ty {
            Type::Struct(struct_type) => {
                struct_type.fields.iter()
                    .find(|f| f.name == method_name)
                    .map(|f| (*f.ty).clone())
            }
            Type::Tuple(tuple_type) => {
                tuple_type.fields.iter()
                    .find(|f| f.name.as_ref() == Some(&method_name))
                    .map(|f| (*f.ty).clone())
            }
            _ => None
        };
        
        // If we found a field and have exactly 1 argument, treat as indexing
        if let Some(field_type) = field_ty {
            if args.len() == 1 {
                // This is field access followed by indexing: field(index)
                // Check that field_type is indexable (Tuple/Array)
                match &field_type {
                    Type::Tuple(tuple_type) => {
                        // Check the index
                        let index_ty = self.check_expr(&args[0])?;
                        if !index_ty.is_integer() {
                            return Err(TypeError::Other(format!(
                                "Tuple index must be integer, found {}",
                                index_ty
                            )));
                        }
                        
                        // Return the element type
                        if let Some((elem_ty, _)) = &tuple_type.variadic {
                            return Ok((**elem_ty).clone());
                        } else if !tuple_type.fields.is_empty() {
                            // For non-variadic tuples, return first field type
                            // (we can't determine statically which field will be accessed)
                            return Ok((*tuple_type.fields[0].ty).clone());
                        } else {
                            return Err(TypeError::Other("Cannot index empty tuple".to_string()));
                        }
                    }
                    _ => {
                        // Field is not indexable, fall through to normal method call
                    }
                }
            }
        }
        
        // Handle as_string() method call: value.as_string()
        if method_name == "as_string" {
            if args.is_empty() {
                // Method call with no args: receiver.as_string()
                let _receiver_ty = self.check_expr(receiver)?;
                return self.type_env.resolve_type("String");
            } else {
                return Err(TypeError::Other(
                    "as_string method expects no arguments".to_string(),
                ));
            }
        }
        
        // Uniform call syntax: receiver.method(args) is equivalent to method(receiver, args)

        // Build argument list with receiver as first argument
        let mut all_args = vec![receiver.clone()];
        all_args.extend_from_slice(args);

        // Try to find matching function
        if let Some(signatures) = self.functions.get(&method_name).cloned() {
            // Type check arguments
            let mut arg_exprs_and_types = Vec::new();
            for arg in &all_args {
                let ty = self.check_expr(arg)?;
                arg_exprs_and_types.push((arg, ty));
            }

            for sig in &signatures {
                // Count required (non-default) parameters
                let required_params = sig.params.iter()
                    .take_while(|(_, _, has_default)| !has_default)
                    .count();
                
                // Check if arg count is valid (between required and total params)
                if arg_exprs_and_types.len() >= required_params && arg_exprs_and_types.len() <= sig.params.len() {
                    // Validate reference parameter usage and try to unify type parameters
                    let mut type_bindings = std::collections::HashMap::new();
                    let mut matches = true;
                    
                    for (i, (_, param_ty, _)) in sig.params.iter().take(arg_exprs_and_types.len()).enumerate() {
                        let (arg_expr, arg_ty) = &arg_exprs_and_types[i];
                        
                        // Validate reference parameter usage
                        if let Type::Reference(_) = param_ty {
                            // Parameter expects a reference, ensure arg uses &
                            if !matches!(arg_expr, Expr::Reference { .. }) {
                                // For method calls, the first parameter is the receiver
                                let param_desc = if i == 0 {
                                    "receiver".to_string()
                                } else {
                                    format!("parameter {}", i)
                                };
                                return Err(TypeError::Other(format!(
                                    "Method '{}' {} expects a reference. Use '&' at the call site",
                                    method_name, param_desc
                                )));
                            }
                        } else {
                            // Parameter expects a value, ensure arg doesn't use &
                            if matches!(arg_expr, Expr::Reference { .. }) {
                                let param_desc = if i == 0 {
                                    "receiver".to_string()
                                } else {
                                    format!("parameter {}", i)
                                };
                                return Err(TypeError::Other(format!(
                                    "Method '{}' {} does not expect a reference. Remove '&' from the argument",
                                    method_name, param_desc
                                )));
                            }
                        }
                        
                        if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") && method_name == "unwrap" {
                            eprintln!("DEBUG unify param {}: actual={:?}, param={:?}", i, arg_ty, param_ty);
                        }
                        if !self.try_unify_type(arg_ty, param_ty, &mut type_bindings) {
                            matches = false;
                            break;
                        }
                        if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") && method_name == "unwrap" {
                            eprintln!("DEBUG after unify: bindings={:?}", type_bindings);
                        }
                    }

                    if matches {
                        // Substitute type parameters in return type
                        let return_ty = sig.return_type.clone().unwrap_or(Type::Void);
                        if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") && (method_name == "unwrap" || method_name == "first") {
                            eprintln!("DEBUG check_method_call {}: return_ty={:?}, type_bindings={:?}", method_name, return_ty, type_bindings);
                        }
                        let substituted = self.substitute_type_params(&return_ty, &type_bindings);
                        if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") && (method_name == "unwrap" || method_name == "first") {
                            eprintln!("DEBUG check_method_call {}: substituted={:?}", method_name, substituted);
                        }
                        return Ok(substituted);
                    }
                }
            }
        }

        Err(TypeError::Other(format!(
            "No method '{}' found",
            method.name
        )))
    }
    
    /// Try to unify an actual type with a parameter type (which may contain type parameters)
    /// Returns true if unification succeeds, updating type_bindings with any new bindings
    fn try_unify_type(
        &self,
        actual: &Type,
        param: &Type,
        bindings: &mut std::collections::HashMap<String, Type>,
    ) -> bool {
        match param {
            Type::TypeParam(name) => {
                // If we've already bound this type parameter, check consistency
                if let Some(bound_ty) = bindings.get(name) {
                    // Allow bidirectional conversion for numeric types
                    // This handles cases like: u is bound to Int, but we're trying to unify Float
                    // where Int (from literal 0) should be able to become Float
                    actual.can_convert_to(bound_ty) || bound_ty.can_convert_to(actual)
                } else {
                    // Bind the type parameter
                    bindings.insert(name.clone(), actual.clone());
                    true
                }
            }
            Type::Tuple(param_tuple) => {
                // Handle variadic tuples specially
                if let Type::Tuple(actual_tuple) = actual {
                    // Check if param has variadic tail
                    if let Some((var_ty, _)) = &param_tuple.variadic {
                        // For variadic, just check the element type
                        if let Some((actual_var_ty, _)) = &actual_tuple.variadic {
                            self.try_unify_type(actual_var_ty, var_ty, bindings)
                        } else {
                            // Fixed tuple -> variadic tuple
                            // All fixed fields (beyond param's fixed fields) must unify with variadic type
                            // Example: (Int, Int, Int) unifies with t* where t=Int
                            let fixed_to_check = if param_tuple.fields.is_empty() {
                                // Param is pure variadic (t*), check all actual fields
                                &actual_tuple.fields[..]
                            } else {
                                // Param has fixed fields + variadic, check remaining actual fields
                                if actual_tuple.fields.len() < param_tuple.fields.len() {
                                    return false;
                                }
                                &actual_tuple.fields[param_tuple.fields.len()..]
                            };
                            
                            // All fields must unify with the variadic element type
                            fixed_to_check.iter().all(|f| self.try_unify_type(&f.ty, var_ty, bindings))
                        }
                    } else {
                        // Non-variadic tuple: check each field
                        if param_tuple.fields.len() != actual_tuple.fields.len() {
                            return false;
                        }
                        for (actual_field, param_field) in
                            actual_tuple.fields.iter().zip(param_tuple.fields.iter())
                        {
                            if !self.try_unify_type(&actual_field.ty, &param_field.ty, bindings) {
                                return false;
                            }
                        }
                        true
                    }
                } else {
                    false
                }
            }
            Type::Function(param_fn) => {
                // Handle function types - unify parameters and return type
                if let Type::Function(actual_fn) = actual {
                    // Check parameter count
                    if param_fn.params.len() != actual_fn.params.len() {
                        return false;
                    }
                    
                    // Unify each parameter
                    for (actual_param, param_param) in actual_fn.params.iter().zip(param_fn.params.iter()) {
                        if !self.try_unify_type(actual_param, param_param, bindings) {
                            return false;
                        }
                    }
                    
                    // Unify return types
                    match (&actual_fn.return_type, &param_fn.return_type) {
                        (Some(actual_ret), Some(param_ret)) => {
                            self.try_unify_type(actual_ret, param_ret, bindings)
                        }
                        (None, None) => true,
                        _ => false,
                    }
                } else {
                    false
                }
            }
            Type::Generic { base: param_base, args: param_args } => {
                // Handle generic types like Option(t), Result(t, e)
                // Unify Option(Int) with Option(t) to bind t = Int
                if let Type::Generic { base: actual_base, args: actual_args } = actual {
                    // Bases must unify (e.g., both are Option)
                    if !self.try_unify_type(actual_base, param_base, bindings) {
                        return false;
                    }
                    // Type arguments must match in count
                    if param_args.len() != actual_args.len() {
                        return false;
                    }
                    // Unify each type argument
                    for (actual_arg, param_arg) in actual_args.iter().zip(param_args.iter()) {
                        match (actual_arg, param_arg) {
                            (ConstArg::Type(actual_ty), ConstArg::Type(param_ty)) => {
                                if !self.try_unify_type(actual_ty, param_ty, bindings) {
                                    return false;
                                }
                            }
                            _ => return false,  // Non-type args not supported yet
                        }
                    }
                    true
                } else {
                    false
                }
            }
            _ => {
                // For non-generic types, just check convertibility
                actual.can_convert_to(param)
            }
        }
    }
    
    /// Substitute type parameters in a type with their bindings
    fn substitute_type_params(
        &self,
        ty: &Type,
        bindings: &std::collections::HashMap<String, Type>,
    ) -> Type {
        match ty {
            Type::TypeParam(name) => bindings.get(name).cloned().unwrap_or(ty.clone()),
            Type::Tuple(tuple_ty) => {
                let fields = tuple_ty
                    .fields
                    .iter()
                    .map(|f| TupleField {
                        name: f.name.clone(),
                        ty: Box::new(self.substitute_type_params(&f.ty, bindings)),
                    })
                    .collect();
                let variadic = tuple_ty.variadic.as_ref().map(|(var_ty, non_empty)| {
                    (
                        Box::new(self.substitute_type_params(var_ty, bindings)),
                        *non_empty,
                    )
                });
                Type::Tuple(TupleType { fields, variadic })
            }
            Type::Generic { base, args } => {
                // Recursively substitute in base and args
                let substituted_base = Box::new(self.substitute_type_params(base, bindings));
                let substituted_args = args.iter().map(|arg| {
                    match arg {
                        ConstArg::Type(ty) => {
                            ConstArg::Type(Box::new(self.substitute_type_params(ty, bindings)))
                        }
                        _ => arg.clone(),  // Non-type args unchanged
                    }
                }).collect();
                Type::Generic {
                    base: substituted_base,
                    args: substituted_args,
                }
            }
            _ => ty.clone(),
        }
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
            // Check if this is an enum case constructor
            if let Some((enum_name, case, _idx)) = self.type_env.find_enum_case(&type_ident.name) {
                // This is an enum case constructor
                // Clone the data we need before we return
                let enum_name = enum_name.to_string();
                let expected_fields: Vec<Box<Type>> = case.fields.clone();
                
                if fields.len() != expected_fields.len() {
                    return Err(TypeError::Other(format!(
                        "Enum case {} expects {} fields, got {}",
                        type_ident.name,
                        expected_fields.len(),
                        fields.len()
                    )));
                }

                // Get the full enum definition to access type parameters
                let enum_def = self.type_env.get_enum(&enum_name).cloned();
                
                // Infer type parameters from constructor arguments
                let mut type_param_bindings = std::collections::HashMap::new();

                // Type check the fields and infer type parameters
                for (i, field_init) in fields.iter().enumerate() {
                    let value_ty = self.check_expr(&field_init.value)?;
                    let expected_ty = &expected_fields[i];

                    // Try to unify to infer type parameters
                    if !self.try_unify_type(&value_ty, expected_ty, &mut type_param_bindings) {
                        return Err(TypeError::Incompatible {
                            expected: expected_ty.clone(),
                            found: Box::new(value_ty),
                            reason: format!("Enum case {} field {} type mismatch", type_ident.name, i + 1),
                        });
                    }
                }

                // If the enum has type parameters, return a generic instantiation
                if let Some(enum_def) = enum_def {
                    if !enum_def.params.is_empty() {
                        // Build generic type arguments
                        let mut args = Vec::new();
                        for param in &enum_def.params {
                            if let Some(inferred_ty) = type_param_bindings.get(&param.name) {
                                args.push(ConstArg::Type(Box::new(inferred_ty.clone())));
                            } else if let Some(default) = &param.default {
                                args.push(default.clone());
                            } else {
                                return Err(TypeError::Other(format!(
                                    "Cannot infer type parameter {} for enum {} case {}",
                                    param.name, enum_name, type_ident.name
                                )));
                            }
                        }

                        // Return instantiated generic type
                        return Ok(Type::Generic {
                            base: Box::new(Type::Enum(enum_def)),
                            args,
                        });
                    }
                }

                // No type parameters, return the base enum type
                return self.type_env.resolve_type(&enum_name);
            }

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

        // If closure has no explicit parameters, check if body uses implicit $N parameters
        if params.is_empty() {
            let implicit_params = self.find_implicit_params(body);
            if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
                eprintln!("DEBUG check_closure: found implicit params: {:?}", implicit_params);
            }
            if !implicit_params.is_empty() {
                // Add implicit parameters with placeholder types
                // These will be refined during type inference from context
                for param_name in &implicit_params {
                    // Use Int as placeholder type for implicit parameters
                    // TODO: Ideally this should be inferred from context/usage
                    if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
                        eprintln!("DEBUG check_closure: adding implicit param {} with type Int", param_name);
                    }
                    self.symbols.add_variable(param_name.clone(), Type::Int(None));
                    param_types.push(Box::new(Type::Int(None)));
                }
            }
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
    
    /// Find implicit closure parameters ($0, $1, etc.) used in a block
    fn find_implicit_params(&self, block: &Block) -> Vec<String> {
        let mut params = std::collections::HashSet::new();
        if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
            eprintln!("DEBUG find_implicit_params: scanning block with {} stmts", block.stmts.len());
        }
        for stmt in &block.stmts {
            self.find_implicit_params_in_stmt(stmt, &mut params);
        }
        let mut param_list: Vec<String> = params.into_iter().collect();
        param_list.sort(); // Ensure consistent ordering: $0, $1, $10, $2, ...
        if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
            eprintln!("DEBUG find_implicit_params: found params: {:?}", param_list);
        }
        param_list
    }
    
    fn find_implicit_params_in_stmt(&self, stmt: &Stmt, params: &mut std::collections::HashSet<String>) {
        if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
            eprintln!("DEBUG find_implicit_params_in_stmt: stmt type={:?}", std::mem::discriminant(stmt));
        }
        match stmt {
            Stmt::Expression(expr) => {
                self.find_implicit_params_in_expr(expr, params);
            }
            Stmt::VarDecl(var_decl) => {
                if let Some(init) = &var_decl.init {
                    self.find_implicit_params_in_expr(init, params);
                }
            }
        }
    }
    
    fn find_implicit_params_in_expr(&self, expr: &Expr, params: &mut std::collections::HashSet<String>) {
        if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
            eprintln!("DEBUG find_implicit_params_in_expr: expr type={:?}", std::mem::discriminant(expr));
        }
        match expr {
            Expr::Ident(ident) if ident.name.starts_with('$') => {
                // Only treat as implicit param if not already bound in current scope
                let in_symbols = self.symbols.lookup(&ident.name);
                let in_globals = self.globals.get(&ident.name);
                if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
                    eprintln!("DEBUG find_implicit_params_in_expr: checking '{}' (len={}) - in_symbols={:?}, in_globals={:?}", 
                              ident.name, ident.name.len(), in_symbols.is_some(), in_globals.is_some());
                }
                if in_symbols.is_none() && in_globals.is_none() {
                    if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
                        eprintln!("DEBUG find_implicit_params_in_expr: found unbound '{}' ident - treating as implicit param", ident.name);
                    }
                    params.insert(ident.name.clone());
                } else if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
                    eprintln!("DEBUG find_implicit_params_in_expr: found '{}' ident but it's already bound - skipping", ident.name);
                }
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
            Expr::MethodCall { receiver, args, .. } => {
                self.find_implicit_params_in_expr(receiver, params);
                for arg in args {
                    self.find_implicit_params_in_expr(arg, params);
                }
            }
            Expr::FieldAccess { object, .. } => {
                self.find_implicit_params_in_expr(object, params);
            }
            Expr::Tuple(exprs, _) => {
                for expr in exprs {
                    self.find_implicit_params_in_expr(expr, params);
                }
            }
            Expr::Block(block) => {
                // Don't scan into nested blocks - they have their own parameter scope
                // Only direct $0 references in THIS block count as implicit params
            }
            Expr::Match { expr: match_expr, arms, .. } => {
                // Don't scan match expressions or their arms
                // The match expression is evaluated in the parent scope where $0 may already be bound
                // (e.g., inside a loop body where $0 is the loop variable)
                // Match arm bodies also have their own scope
            }
            Expr::Closure {  .. } => {
                // Don't traverse into nested closures - they have their own scope
            }
            _ => {}
        }
    }

    fn check_block_expr(&mut self, block: &Block) -> TypeResult<Type> {
        self.check_block_expr_with_context(block, None)
    }
    
    /// Check a block expression with optional expected type context for implicit parameters
    /// 
    /// When expected_type is provided and is a function type, implicit parameters ($0, $1, etc.)
    /// will be typed according to the function's parameter types instead of defaulting to Int.
    fn check_block_expr_with_context(&mut self, block: &Block, expected_type: Option<&Type>) -> TypeResult<Type> {
        // Check if block uses implicit closure parameters ($0, $1, etc.)
        let implicit_params = self.find_implicit_params(block);
        
        // Only push a new scope if we have implicit parameters (making this a closure)
        // Otherwise, use the current scope to allow access to outer variables
        if !implicit_params.is_empty() {
            self.symbols.push_scope();
            if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
                eprintln!("DEBUG check_block_expr_with_context: found implicit params: {:?}", implicit_params);
                eprintln!("DEBUG check_block_expr_with_context: expected_type={:?}", expected_type);
            }
            
            // Determine types for implicit parameters from context
            let param_types: Vec<Type> = if let Some(Type::Function(func_type)) = expected_type {
                // Extract parameter types from the expected function type
                if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
                    eprintln!("DEBUG check_block_expr_with_context: inferring from function type with {} params", func_type.params.len());
                }
                implicit_params
                    .iter()
                    .enumerate()
                    .map(|(idx, _)| {
                        if idx < func_type.params.len() {
                            (*func_type.params[idx]).clone()
                        } else {
                            Type::Int(None) // Fallback for extra params
                        }
                    })
                    .collect()
            } else {
                // No context or non-function context: default to Int
                if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
                    eprintln!("DEBUG check_block_expr_with_context: no function type context, defaulting to Int");
                }
                vec![Type::Int(None); implicit_params.len()]
            };
            
            // Add implicit parameters to scope with inferred types
            for (param_name, param_ty) in implicit_params.iter().zip(param_types.iter()) {
                if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1") {
                    eprintln!("DEBUG check_block_expr_with_context: adding implicit param {} with type {:?}", param_name, param_ty);
                }
                self.symbols.add_variable(param_name.clone(), param_ty.clone());
            }
            
            // Check the block body
            let body_ty = self.check_block(block)?;
            self.symbols.pop_scope();
            
            // Return a function type instead of the block's return type
            // Block with implicit params is treated as a closure
            let boxed_param_types: Vec<Box<Type>> = param_types
                .into_iter()
                .map(Box::new)
                .collect();
            
            Ok(Type::Function(FunctionType {
                const_params: vec![],
                params: boxed_param_types,
                return_type: Some(Box::new(body_ty)),
            }))
        } else {
            // No implicit parameters - check block in current scope
            self.check_block(block)
        }
    }

    fn check_loop(&mut self, args: &[Expr]) -> TypeResult<Type> {
        // loop has several forms:
        // - loop() - infinite loop, returns Void
        // - loop(condition) where condition is Bool - conditional loop, returns Void
        // - loop(n) where n is Int - loop n times, returns Void or collects results
        // - loop(iterable) where iterable is a variadic tuple - iterate over elements
        //
        // If there's a second argument (a block), loop becomes an expression that collects results
        
        match args.len() {
            0 => {
                // loop() - infinite loop, returns Void
                Ok(Type::Void)
            }
            1 => {
                // loop(expr) - could be condition, count, or iterable
                let _arg_ty = self.check_expr(&args[0])?;
                
                // For now, just accept any type and return Void
                // The actual semantics depend on the argument type:
                // - Bool: loop while condition
                // - Int: loop n times
                // - Variadic tuple: iterate over elements
                Ok(Type::Void)
            }
            2 => {
                // loop(arg, block) - loop with body that produces values
                let arg_ty = self.check_expr(&args[0])?;
                
                // The second argument should be a block
                // We need to type check it in a special scope where $0 is available
                if let Expr::Block(block) = &args[1] {
                    // Enter new scope for loop body
                    self.symbols.push_scope();
                    
                    // Add $0 binding for iteration variable
                    // Determine the type of $0 based on arg_ty
                    let element_ty = match &arg_ty {
                        Type::Tuple(tuple_ty) => {
                            if let Some((var_ty, _)) = &tuple_ty.variadic {
                                (**var_ty).clone()
                            } else if !tuple_ty.fields.is_empty() {
                                // For non-variadic tuples, $0 would be each element
                                // This is a simplification - in reality we'd need union types
                                (*tuple_ty.fields[0].ty).clone()
                            } else {
                                Type::Void
                            }
                        }
                        Type::Int(_) => Type::Int(None), // loop(n) gives Int iteration count
                        _ => Type::Void,
                    };
                    
                    // Add $0 to the symbol table
                    self.symbols.add_variable("$0".to_string(), element_ty.clone());
                    
                    // Check the loop body
                    let body_ty = self.check_block(block)?;
                    
                    self.symbols.pop_scope();
                    
                    // If body returns a value, loop collects into variadic tuple
                    if !matches!(body_ty, Type::Void) {
                        Ok(Type::Tuple(TupleType {
                            fields: vec![],
                            variadic: Some((Box::new(body_ty), false)),
                        }))
                    } else {
                        Ok(Type::Void)
                    }
                } else {
                    // Second arg is not a block - type check it normally
                    self.check_expr(&args[1])?;
                    Ok(Type::Void)
                }
            }
            _ => {
                Err(TypeError::Other(format!(
                    "loop expects 0-2 arguments, got {}",
                    args.len()
                )))
            }
        }
    }

    fn check_as_string(&mut self, args: &[Expr]) -> TypeResult<Type> {
        // as_string(value) - converts any value to a String
        if args.len() != 1 {
            return Err(TypeError::Other(format!(
                "as_string expects exactly 1 argument, got {}",
                args.len()
            )));
        }
        
        // Type check the argument (accept any type)
        let _arg_ty = self.check_expr(&args[0])?;
        
        // Return String type from stdlib
        self.type_env.resolve_type("String")
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
                if !lit_ty.can_convert_to(expected_ty) && !expected_ty.can_convert_to(&lit_ty) {
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
                    let enum_matches = match expected_ty {
                        Type::Enum(enum_ty) => {
                            enum_ty.name == enum_name && case.fields.is_empty()
                        }
                        Type::Generic { base, .. } => {
                            if let Type::Enum(enum_ty) = base.as_ref() {
                                enum_ty.name == enum_name && case.fields.is_empty()
                            } else {
                                false
                            }
                        }
                        _ => false,
                    };

                    if enum_matches {
                        Ok(())
                    } else {
                        Err(TypeError::Incompatible {
                            expected: Box::new(expected_ty.clone()),
                            found: Box::new(self.type_env.resolve_type(enum_name)?),
                            reason: format!("Pattern '{}' is an enum case", ident.name),
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
                // Extract the enum type and generic args if present
                let (enum_ty, type_bindings) = match expected_ty {
                    Type::Enum(e) => (e, std::collections::HashMap::new()),
                    Type::Generic { base, args } => {
                        if let Type::Enum(e) = base.as_ref() {
                            // Build type parameter bindings from generic args
                            let mut bindings = std::collections::HashMap::new();
                            for (i, param) in e.params.iter().enumerate() {
                                if i < args.len() {
                                    if let ConstArg::Type(ty) = &args[i] {
                                        bindings.insert(param.name.clone(), (**ty).clone());
                                    }
                                }
                            }
                            (e, bindings)
                        } else {
                            return Err(TypeError::Incompatible {
                                expected: Box::new(expected_ty.clone()),
                                found: Box::new(Type::Error),
                                reason: "Pattern expects enum".to_string(),
                            });
                        }
                    }
                    _ => {
                        return Err(TypeError::Incompatible {
                            expected: Box::new(expected_ty.clone()),
                            found: Box::new(Type::Error),
                            reason: "Pattern expects enum".to_string(),
                        });
                    }
                };

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
                            // Substitute type parameters in field type
                            let field_ty = self.substitute_type_params(&case.fields[i], &type_bindings);
                            self.check_pattern(pattern, &field_ty)?;
                        }

                        return Ok(());
                    }
                }

                Err(TypeError::Other(format!(
                    "Enum {} has no case '{}'",
                    enum_ty.name, name.name
                )))
            }

            Pattern::Alternative(patterns, _) => {
                // All alternatives must match the expected type
                for pattern in patterns {
                    self.check_pattern(pattern, expected_ty)?;
                }
                Ok(())
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
                // Extract the enum type and generic args if present
                let (enum_ty, type_bindings) = match ty {
                    Type::Enum(e) => (e, std::collections::HashMap::new()),
                    Type::Generic { base, args } => {
                        if let Type::Enum(e) = base.as_ref() {
                            // Build type parameter bindings from generic args
                            let mut bindings = std::collections::HashMap::new();
                            for (i, param) in e.params.iter().enumerate() {
                                if i < args.len() {
                                    if let ConstArg::Type(ty) = &args[i] {
                                        bindings.insert(param.name.clone(), (**ty).clone());
                                    }
                                }
                            }
                            (e, bindings)
                        } else {
                            return Ok(());
                        }
                    }
                    _ => return Ok(()),
                };

                for case in &enum_ty.cases {
                    if case.name == name.name {
                        for (i, pattern) in fields.iter().enumerate() {
                            // Substitute type parameters in field type
                            let field_ty = self.substitute_type_params(&case.fields[i], &type_bindings);
                            self.add_pattern_bindings(pattern, &field_ty)?;
                        }
                        break;
                    }
                }
                Ok(())
            }

            Pattern::Alternative(patterns, _) => {
                // Alternative patterns cannot bind variables
                // Ensure all alternatives produce the same bindings
                // For simplicity, we check that none of them bind variables
                for pattern in patterns {
                    // We only allow alternatives that don't introduce bindings
                    // (e.g., enum cases, literals, wildcards)
                    match pattern {
                        Pattern::Wildcard(_) | Pattern::Literal(_, _) => {}
                        Pattern::Ident(ident) => {
                            // Check if this is an enum case (zero-field constructor)
                            if self.type_env.find_enum_case(&ident.name).is_none() {
                                return Err(TypeError::Other(
                                    "Alternative patterns cannot bind variables".to_string()
                                ));
                            }
                        }
                        Pattern::Enum { fields, .. } => {
                            // Recursively check nested patterns
                            self.add_pattern_bindings(pattern, ty)?;
                        }
                        Pattern::Alternative(_, _) => {
                            // Nested alternatives
                            self.add_pattern_bindings(pattern, ty)?;
                        }
                        Pattern::Tuple(_, _) | Pattern::Expr(_) => {
                            return Err(TypeError::Other(
                                "Alternative patterns cannot contain bindings".to_string()
                            ));
                        }
                    }
                }
                Ok(())
            }

            Pattern::Expr(_) => Ok(()),
        }
    }

    // ========================================================================
    // Helper Methods
    // ========================================================================

    /// Check if a type contains any reference types (recursively)
    fn contains_reference_type(&self, ty: &Type) -> bool {
        match ty {
            Type::Reference(_) => true,
            Type::Tuple(tuple_ty) => {
                tuple_ty.fields.iter().any(|f| self.contains_reference_type(&f.ty))
                    || tuple_ty.variadic.as_ref().map_or(false, |(t, _)| self.contains_reference_type(t))
            }
            Type::Struct(struct_ty) => {
                struct_ty.fields.iter().any(|f| self.contains_reference_type(&f.ty))
            }
            Type::Function(func_ty) => {
                func_ty.params.iter().any(|p| self.contains_reference_type(p))
                    || func_ty.return_type.as_ref().map_or(false, |r| self.contains_reference_type(r))
            }
            Type::Generic { base, args } => {
                self.contains_reference_type(base)
                    || args.iter().any(|arg| {
                        if let ConstArg::Type(t) = arg {
                            self.contains_reference_type(t)
                        } else {
                            false
                        }
                    })
            }
            _ => false,
        }
    }

    /// Check if an expression is an lvalue (can be referenced with &)
    /// Lvalues are: variables, array/tuple elements, struct fields
    fn is_lvalue(&self, expr: &Expr) -> bool {
        match expr {
            // Variables are lvalues
            Expr::Ident(_) => true,
            
            // Array/tuple indexing is an lvalue: arr(i), tuple(0)
            Expr::Call { func, .. } => {
                // Check if func is a variable or field access (not a function call)
                // This handles cases like: arr(i), obj.field(i)
                match func.as_ref() {
                    Expr::Ident(_) | Expr::FieldAccess { .. } => true,
                    _ => false,
                }
            }
            
            // Field access is an lvalue: obj.field
            Expr::FieldAccess { .. } => true,
            
            // Method calls that return lvalues (for chaining)
            Expr::MethodCall { .. } => false,
            
            // All other expressions are not lvalues
            _ => false,
        }
    }

    // ========================================================================
    // Type Resolution
    // ========================================================================

    fn resolve_ast_type(&self, ast_type: &atom_ast::Type) -> TypeResult<Type> {
        match ast_type {
            atom_ast::Type::Named(ident) => {
                // Try to resolve as a type first
                match self.type_env.resolve_type(&ident.name) {
                    Ok(ty) => Ok(ty),
                    Err(_) => {
                        // If resolution fails, check if this looks like a variable name
                        // being incorrectly used as a type (parser bug in type inference)
                        // Treat it as a type parameter for now
                        // This handles cases like: `acc := init` where parser emits `init` as type
                        if ident.name.chars().next().map(|c| c.is_lowercase() || c == '$').unwrap_or(false) {
                            // Lowercase or $ prefix - likely a variable, treat as type param
                            Ok(Type::TypeParam(ident.name.clone()))
                        } else {
                            // Uppercase - should be a real type, propagate the error
                            self.type_env.resolve_type(&ident.name)
                        }
                    }
                }
            }

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
                // Special case: Generic with no params should just be the base type
                // This handles parser quirks where `Float` might be parsed as Generic { params: [] }
                if params.is_empty() {
                    return self.type_env.resolve_type(&name.name);
                }
                
                // Special handling for sized primitive types: Float(32), Int(64), etc.
                if params.len() == 1 {
                    // Try to extract an integer literal for the size
                    // The size can be in either the `ty` field or the `name` field
                    let size_str = if let Some(param_ty) = &params[0].ty {
                        // Size is a type: Float(Int(32))
                        if let atom_ast::Type::Named(size_ident) = param_ty.as_ref() {
                            Some(size_ident.name.as_str())
                        } else {
                            None
                        }
                    } else if let Some(name_ident) = &params[0].name {
                        // Size is a name: Float(32)
                        Some(name_ident.name.as_str())
                    } else {
                        None
                    };
                    
                    if let Some(size_str) = size_str {
                        if let Ok(size_val) = size_str.parse::<u32>() {
                            // Handle sized primitives
                            match name.name.as_str() {
                                "Float" => return Ok(Type::Float(Some(size_val))),
                                "Int" => return Ok(Type::Int(Some(size_val))),
                                "UInt" => return Ok(Type::UInt(Some(size_val))),
                                _ => {}
                            }
                        }
                    }
                }
                
                // General generic type handling
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

            atom_ast::Type::Reference { inner, .. } => {
                // Parse the reference type properly
                let inner_type = self.resolve_ast_type(inner)?;
                Ok(Type::Reference(Box::new(inner_type)))
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
                // If stdlib is not loaded, fall back to a tuple type representing a byte array
                self.type_env.resolve_type("String")
                    .unwrap_or_else(|_| Type::Tuple(TupleType {
                        fields: vec![],
                        variadic: Some((Box::new(Type::Int(Some(8))), false)),
                    }))
            }
            Literal::Rune(_) => Type::Rune,
            Literal::Bool(_) => {
                // Bool literals become the Bool enum type
                // If stdlib is not loaded, fall back to Int(1) (boolean)
                self.type_env.resolve_type("Bool")
                    .unwrap_or(Type::Int(Some(1)))
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
