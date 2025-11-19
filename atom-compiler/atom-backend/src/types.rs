//! Type system for the Atom language compiler backend.
//!
//! This module provides the type representation and type checking infrastructure for Atom,
//! including:
//! - Type representations for all Atom types (primitives, structs, enums, tuples, functions)
//! - Type equality and structural subtyping
//! - Type environment for tracking user-defined types
//! - Symbol tables for variable type tracking
//! - Implicit conversion rules as defined in the Atom specification
//!
//! # Atom Type System
//!
//! Atom uses structural typing, meaning types are compatible based on their structure
//! rather than their names. Key features:
//!
//! - **Structural typing**: Types with the same structure are compatible
//! - **Implicit conversions**: Structs/tuples can be converted based on field compatibility
//! - **Const/generic parameters**: Types can be parameterized with compile-time values
//! - **Variadic tuples**: Tuples can have variable-length tails (T* or T+)

use atom_ast::{self, Visibility};
use std::collections::HashMap;
use std::fmt;

/// The main type representation for Atom.
///
/// Types in Atom can be:
/// - Primitives: Int, UInt, Float, Rune, Bool, String, Void
/// - User-defined: Structs, Enums
/// - Composite: Tuples, Functions
/// - Generic: Types with const/type parameters
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    /// Void type - empty struct, supports all operators trivially
    Void,

    /// Signed integer with optional bit size (default 64)
    /// Int(None) = Int(64), Int(Some(32)) = 32-bit int
    Int(Option<u32>),

    /// Unsigned integer with optional bit size (default 64)
    UInt(Option<u32>),

    /// Floating point with optional bit size (default 64)
    /// Float(None) = Float(64), Float(Some(32)) = 32-bit float
    Float(Option<u32>),

    /// Unicode codepoint (single character)
    Rune,

    /// Meta-type representing a type value
    /// Used for type parameters and reflection
    TypeMeta,

    /// Tuple type with ordered, possibly heterogeneous fields
    /// Fields can be named or unnamed
    Tuple(TupleType),

    /// Struct type - named collection of fields
    /// Structurally typed: compatible with any struct/tuple with matching fields
    Struct(StructType),

    /// Enum type - sum type with named variants
    Enum(EnumType),

    /// Function type: parameters -> return type
    Function(FunctionType),

    /// Type parameter reference (for generics)
    /// Example: in `Container(t)`, `t` is a TypeParam
    TypeParam(String),

    /// Generic type instantiation
    /// Example: `Option(Int)`, `Result(String, MyError)`
    Generic {
        base: Box<Type>,
        args: Vec<ConstArg>,
    },

    /// Unknown/inferred type (used during type checking)
    Infer(InferType),

    /// Error type (for error recovery during type checking)
    Error,
}

/// Tuple type representation
#[derive(Debug, Clone, PartialEq)]
pub struct TupleType {
    /// Fixed fields at the beginning of the tuple
    pub fields: Vec<TupleField>,
    /// Optional variadic tail: None, Some((type, false)) for T*, Some((type, true)) for T+
    pub variadic: Option<(Box<Type>, bool)>,
}

/// A single field in a tuple
#[derive(Debug, Clone, PartialEq)]
pub struct TupleField {
    /// Optional field name (for named tuples/anonymous structs)
    pub name: Option<String>,
    /// Type of the field
    pub ty: Box<Type>,
}

/// Struct type representation
#[derive(Debug, Clone, PartialEq)]
pub struct StructType {
    /// Name of the struct (for diagnostics, not used in structural typing)
    pub name: String,
    /// Const/type parameters
    pub params: Vec<TypeParameter>,
    /// Fields of the struct (order matters for conversion to tuples)
    pub fields: Vec<StructField>,
    /// Visibility of the struct
    pub visibility: Visibility,
}

/// A field in a struct
#[derive(Debug, Clone, PartialEq)]
pub struct StructField {
    /// Field name
    pub name: String,
    /// Field type
    pub ty: Box<Type>,
}

/// Enum type representation
#[derive(Debug, Clone, PartialEq)]
pub struct EnumType {
    /// Name of the enum
    pub name: String,
    /// Const/type parameters
    pub params: Vec<TypeParameter>,
    /// Enum variants/cases
    pub cases: Vec<EnumCase>,
    /// Visibility of the enum
    pub visibility: Visibility,
}

/// A single case/variant in an enum
#[derive(Debug, Clone, PartialEq)]
pub struct EnumCase {
    /// Case name (must start with uppercase)
    pub name: String,
    /// Associated values (tuple of types)
    pub fields: Vec<Box<Type>>,
}

/// Function type representation
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionType {
    /// Const parameters (before semicolon in syntax)
    pub const_params: Vec<TypeParameter>,
    /// Regular parameters
    pub params: Vec<Box<Type>>,
    /// Return type (None for Void)
    pub return_type: Option<Box<Type>>,
}

/// Type or const parameter
#[derive(Debug, Clone, PartialEq)]
pub struct TypeParameter {
    /// Parameter name
    pub name: String,
    /// Optional constraint/type (Type for type params, Int for const int params, etc.)
    pub constraint: Option<Box<Type>>,
    /// Optional default value
    pub default: Option<ConstArg>,
}

/// Compile-time argument (for generic instantiation)
#[derive(Debug, Clone, PartialEq)]
pub enum ConstArg {
    /// Type argument
    Type(Box<Type>),
    /// Integer constant
    Int(i64),
    /// Tuple of const args
    Tuple(Vec<ConstArg>),
    /// Named parameter reference
    Param(String),
}

/// Inference type variable
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InferType {
    /// Unique ID for this inference variable
    pub id: usize,
}

/// Result of type checking operations
pub type TypeResult<T> = Result<T, TypeError>;

/// Type error during type checking or conversion
#[derive(Debug, Clone, PartialEq)]
pub enum TypeError {
    /// Types are incompatible and cannot be converted
    Incompatible {
        expected: Box<Type>,
        found: Box<Type>,
        reason: String,
    },
    /// Type not found in environment
    Undefined {
        name: String,
    },
    /// Variable not found in symbol table
    UndefinedVariable {
        name: String,
    },
    /// Incorrect number of generic arguments
    WrongArity {
        expected: usize,
        found: usize,
    },
    /// Field missing in struct conversion
    MissingField {
        field: String,
        target: String,
    },
    /// Cannot convert variadic to concrete fields
    VariadicConversion {
        from: Box<Type>,
        to: Box<Type>,
    },
    /// Cyclic type reference
    Cyclic {
        name: String,
    },
    /// Other error with message
    Other(String),
}

/// Type environment - tracks user-defined types (structs, enums)
#[derive(Debug, Clone)]
pub struct TypeEnvironment {
    /// Struct definitions by name
    structs: HashMap<String, StructType>,
    /// Enum definitions by name
    enums: HashMap<String, EnumType>,
    /// Type aliases (if we support them)
    aliases: HashMap<String, Type>,
    /// Counter for generating inference variable IDs
    next_infer_id: usize,
}

impl TypeEnvironment {
    /// Create a new empty type environment
    pub fn new() -> Self {
        Self {
            structs: HashMap::new(),
            enums: HashMap::new(),
            aliases: HashMap::new(),
            next_infer_id: 0,
        }
    }

    /// Create a new type environment with standard library types
    pub fn with_stdlib() -> Self {
        let mut env = Self::new();

        // Add Bool enum: Bool(True, False)
        env.add_enum(EnumType {
            name: "Bool".to_string(),
            params: vec![],
            cases: vec![
                EnumCase {
                    name: "True".to_string(),
                    fields: vec![],
                },
                EnumCase {
                    name: "False".to_string(),
                    fields: vec![],
                },
            ],
            visibility: Visibility::Public,
        });

        // Option and Result are now defined in the stdlib (result.atom)
        // so we don't need to add them here

        env
    }

    /// Add a struct definition to the environment
    pub fn add_struct(&mut self, struct_type: StructType) {
        self.structs.insert(struct_type.name.clone(), struct_type);
    }

    /// Add an enum definition to the environment
    pub fn add_enum(&mut self, enum_type: EnumType) {
        self.enums.insert(enum_type.name.clone(), enum_type);
    }

    /// Add a type alias
    pub fn add_alias(&mut self, name: String, ty: Type) {
        self.aliases.insert(name, ty);
    }

    /// Look up a struct by name
    pub fn get_struct(&self, name: &str) -> Option<&StructType> {
        self.structs.get(name)
    }

    /// Look up an enum by name
    pub fn get_enum(&self, name: &str) -> Option<&EnumType> {
        self.enums.get(name)
    }

    /// Look up a type alias
    pub fn get_alias(&self, name: &str) -> Option<&Type> {
        self.aliases.get(name)
    }

    /// Find an enum case by name across all enums
    /// Returns (enum_name, case, case_index) if found
    pub fn find_enum_case(&self, case_name: &str) -> Option<(&str, &EnumCase, usize)> {
        for (enum_name, enum_type) in &self.enums {
            for (idx, case) in enum_type.cases.iter().enumerate() {
                if case.name == case_name {
                    return Some((enum_name, case, idx));
                }
            }
        }
        None
    }

    /// Resolve a type name to a Type
    pub fn resolve_type(&self, name: &str) -> TypeResult<Type> {
        // Check primitives first
        match name {
            "Void" => return Ok(Type::Void),
            "Int" => return Ok(Type::Int(None)),
            "UInt" => return Ok(Type::UInt(None)),
            "Float" => return Ok(Type::Float(None)),
            "Rune" => return Ok(Type::Rune),
            // String is defined in stdlib as a struct, not a primitive
            "Type" => return Ok(Type::TypeMeta),
            _ => {}
        }

        // Check aliases
        if let Some(ty) = self.get_alias(name) {
            return Ok(ty.clone());
        }

        // Check structs (String is defined here)
        if let Some(struct_ty) = self.get_struct(name) {
            return Ok(Type::Struct(struct_ty.clone()));
        }

        // Check enums
        if let Some(enum_ty) = self.get_enum(name) {
            return Ok(Type::Enum(enum_ty.clone()));
        }

        // Check if this is an enum case (e.g., True, False, Some, None)
        // When used as a standalone value, resolve to the enum type
        for enum_ty in self.enums.values() {
            for case in &enum_ty.cases {
                if case.name == name {
                    // Found a matching enum case, return the enum type
                    return Ok(Type::Enum(enum_ty.clone()));
                }
            }
        }

        Err(TypeError::Undefined {
            name: name.to_string(),
        })
    }

    /// Generate a fresh inference variable
    pub fn fresh_infer(&mut self) -> Type {
        let id = self.next_infer_id;
        self.next_infer_id += 1;
        Type::Infer(InferType { id })
    }
}

impl Default for TypeEnvironment {
    fn default() -> Self {
        Self::new()
    }
}

/// Symbol table for tracking variable types in scopes
#[derive(Debug, Clone)]
pub struct SymbolTable {
    /// Stack of scopes, each mapping variable names to types
    scopes: Vec<HashMap<String, Type>>,
}

impl SymbolTable {
    /// Create a new empty symbol table
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
        }
    }

    /// Enter a new scope
    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    /// Exit the current scope
    pub fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    /// Add a variable to the current scope
    pub fn add_variable(&mut self, name: String, ty: Type) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, ty);
        }
    }

    /// Look up a variable in the symbol table (searches from innermost to outermost scope)
    pub fn lookup(&self, name: &str) -> Option<&Type> {
        for scope in self.scopes.iter().rev() {
            if let Some(ty) = scope.get(name) {
                return Some(ty);
            }
        }
        None
    }

    /// Check if a variable is defined in the current scope (not parent scopes)
    pub fn is_defined_locally(&self, name: &str) -> bool {
        self.scopes
            .last()
            .map(|scope| scope.contains_key(name))
            .unwrap_or(false)
    }
}

impl Default for SymbolTable {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Type Operations
// ============================================================================

impl Type {
    /// Check if this type is structurally equal to another type
    ///
    /// Structural equality means the types have the same structure,
    /// not just the same name.
    pub fn structurally_equal(&self, other: &Type) -> bool {
        match (self, other) {
            (Type::Void, Type::Void) => true,
            (Type::Int(a), Type::Int(b)) => a == b,
            (Type::UInt(a), Type::UInt(b)) => a == b,
            (Type::Float(a), Type::Float(b)) => a == b,
            (Type::Rune, Type::Rune) => true,
            (Type::TypeMeta, Type::TypeMeta) => true,

            (Type::Tuple(a), Type::Tuple(b)) => {
                a.fields.len() == b.fields.len()
                    && a.variadic == b.variadic
                    && a.fields
                        .iter()
                        .zip(&b.fields)
                        .all(|(fa, fb)| fa.ty.structurally_equal(&fb.ty) && fa.name == fb.name)
            }

            (Type::Struct(a), Type::Struct(b)) => {
                // Structs are structurally equal if they have the same fields
                // (names don't matter for structural typing, only field names)
                a.fields.len() == b.fields.len()
                    && a.fields.iter().zip(&b.fields).all(|(fa, fb)| {
                        fa.name == fb.name && fa.ty.structurally_equal(&fb.ty)
                    })
            }

            (Type::Enum(a), Type::Enum(b)) => {
                // Enums must have the same name and cases
                a.name == b.name
                    && a.cases.len() == b.cases.len()
                    && a.cases.iter().zip(&b.cases).all(|(ca, cb)| {
                        ca.name == cb.name
                            && ca.fields.len() == cb.fields.len()
                            && ca
                                .fields
                                .iter()
                                .zip(&cb.fields)
                                .all(|(fa, fb)| fa.structurally_equal(fb))
                    })
            }

            (Type::Function(a), Type::Function(b)) => {
                a.params.len() == b.params.len()
                    && a.params
                        .iter()
                        .zip(&b.params)
                        .all(|(pa, pb)| pa.structurally_equal(pb))
                    && match (&a.return_type, &b.return_type) {
                        (Some(ra), Some(rb)) => ra.structurally_equal(rb),
                        (None, None) => true,
                        _ => false,
                    }
            }

            (Type::TypeParam(a), Type::TypeParam(b)) => a == b,

            (Type::Generic { base: a, args: a_args }, Type::Generic { base: b, args: b_args }) => {
                a.structurally_equal(b) && a_args == b_args
            }

            (Type::Infer(a), Type::Infer(b)) => a.id == b.id,

            // Different type constructors are never equal
            _ => false,
        }
    }

    /// Check if this type can be implicitly converted to another type
    ///
    /// Implements the implicit conversion rules from the Atom specification:
    /// - Struct A -> Struct B if all fields of B are present in A
    /// - Tuple A -> Tuple B if all fields of B are the first fields of A
    /// - Tuple A -> Struct B if all fields of B are the first fields of A
    /// - Struct A -> Tuple B if all fields of B are the first fields of A
    /// - Concrete fields -> Variadic field of same type
    pub fn can_convert_to(&self, target: &Type) -> bool {
        // Exact structural equality is always convertible
        if self.structurally_equal(target) {
            return true;
        }

        match (self, target) {
            // Void <-> empty tuple conversions
            (Type::Tuple(t), Type::Void) => {
                // Empty tuple (non-variadic) can convert to Void
                t.fields.is_empty() && t.variadic.is_none()
            }
            (Type::Void, Type::Tuple(t)) => {
                // Void can convert to any empty tuple (variadic or not)
                // This allows: `current: t*` then `current = ()`
                t.fields.is_empty()
            }
            
            // Struct -> Struct: all fields of target must be in source
            (Type::Struct(source), Type::Struct(target)) => {
                target.fields.iter().all(|target_field| {
                    source.fields.iter().any(|source_field| {
                        source_field.name == target_field.name
                            && source_field.ty.can_convert_to(&target_field.ty)
                    })
                })
            }

            // Tuple -> Tuple: target fields must be prefix of source fields
            (Type::Tuple(source), Type::Tuple(target)) => {
                // Special case: empty tuple can convert to empty variadic tuple
                // This allows: `result: t* = ()`
                if source.fields.is_empty() && source.variadic.is_none() 
                   && target.fields.is_empty() && target.variadic.is_some() {
                    return true;
                }
                
                // Check if target fields match source prefix
                if target.fields.len() > source.fields.len() && source.variadic.is_none() {
                    return false;
                }

                // Check fixed fields match
                for (i, target_field) in target.fields.iter().enumerate() {
                    if i < source.fields.len() {
                        if !source.fields[i].ty.can_convert_to(&target_field.ty) {
                            return false;
                        }
                    } else {
                        // Target has more fields than source, check variadic
                        if let Some((var_ty, _)) = &source.variadic {
                            if !var_ty.can_convert_to(&target_field.ty) {
                                return false;
                            }
                        } else {
                            return false;
                        }
                    }
                }

                // Check variadic conversion
                match (&source.variadic, &target.variadic) {
                    (Some((s_ty, _)), Some((t_ty, _))) => s_ty.can_convert_to(t_ty),
                    (Some(_), None) => true, // Can drop variadic
                    (None, Some(_)) => false, // Cannot add variadic
                    (None, None) => true,
                }
            }

            // Tuple -> Struct: struct fields must match tuple prefix (by position)
            (Type::Tuple(source), Type::Struct(target)) => {
                if target.fields.len() > source.fields.len() && source.variadic.is_none() {
                    return false;
                }

                for (i, target_field) in target.fields.iter().enumerate() {
                    if i < source.fields.len() {
                        if !source.fields[i].ty.can_convert_to(&target_field.ty) {
                            return false;
                        }
                    } else if let Some((var_ty, _)) = &source.variadic {
                        if !var_ty.can_convert_to(&target_field.ty) {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }

                true
            }

            // Struct -> Tuple: tuple fields must match struct prefix (by position)
            (Type::Struct(source), Type::Tuple(target)) => {
                if target.fields.len() > source.fields.len() {
                    return false;
                }

                for (i, target_field) in target.fields.iter().enumerate() {
                    if i < source.fields.len() {
                        if !source.fields[i].ty.can_convert_to(&target_field.ty) {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }

                // Cannot convert to variadic from struct
                target.variadic.is_none()
            }

            // Generic type conversions
            (Type::Generic { base: s_base, args: s_args }, Type::Generic { base: t_base, args: t_args }) => {
                // Both are generic: check base types match and args are compatible
                if !s_base.structurally_equal(t_base) {
                    return false;
                }
                
                // For now, require exact match on args
                // TODO: handle variance properly
                s_args == t_args
            }
            
            // Instantiated generic -> generic with type params
            // e.g., Option(Int) -> Option(t) is allowed when checking function returns
            (Type::Generic { base: s_base, .. }, _) => {
                // If target has type parameters, we can instantiate to match
                // Just check if base types are compatible
                match target {
                    Type::TypeParam(_) => true, // Can assign to any type parameter
                    _ => s_base.can_convert_to(target),
                }
            }
            
            // Allow conversion from base type to its generic instantiation  
            (_, Type::Generic { base: t_base, args: _ }) => {
                // This handles cases where we need to compare against a generic signature
                // The actual instantiation should have been done earlier
                self.can_convert_to(t_base)
            }
            
            // Function -> Function: check parameter and return type compatibility
            (Type::Function(source), Type::Function(target)) => {
                // Functions must have the same number of parameters
                if source.params.len() != target.params.len() {
                    return false;
                }
                
                // For now, require exact parameter match (in the future could handle variance)
                for (source_param, target_param) in source.params.iter().zip(target.params.iter()) {
                    if !source_param.structurally_equal(target_param) {
                        return false;
                    }
                }
                
                // Return type must match
                match (&source.return_type, &target.return_type) {
                    (Some(s_ret), Some(t_ret)) => s_ret.structurally_equal(t_ret),
                    (None, None) => true,
                    _ => false,
                }
            }
            
            // TypeParam -> TypeParam: any type param can convert to any other type param
            // This enables polymorphism: len(arr t*) can accept u*, v*, etc.
            (Type::TypeParam(_), Type::TypeParam(_)) => true,
            
            // Concrete type -> TypeParam: any concrete type can be used where a type param is expected
            (_, Type::TypeParam(_)) => true,

            // Numeric conversions: Int can convert to Float (for literals like 0, 1, etc.)
            // This handles cases like `reduce(float_array, 0, fn)` where 0 is parsed as Int
            (Type::Int(_), Type::Float(_)) => true,

            // No other implicit conversions
            _ => false,
        }
    }

    /// Get the zero value type for this type
    ///
    /// Zero values:
    /// - Int/UInt/Float: 0
    /// - Rune: '\0'
    /// - Bool: False (first enum case)
    /// - String: ""
    /// - Tuple/Struct: all fields set to zero
    /// - Enum: first case with all associated values set to zero
    /// - Void: ()
    pub fn has_zero_value(&self) -> bool {
        match self {
            Type::Void
            | Type::Int(_)
            | Type::UInt(_)
            | Type::Float(_)
            | Type::Rune => true,
            Type::Tuple(t) => t.fields.iter().all(|f| f.ty.has_zero_value()),
            Type::Struct(s) => s.fields.iter().all(|f| f.ty.has_zero_value()),
            Type::Enum(e) => !e.cases.is_empty(), // Bool enum has zero value (first case)
            Type::Function(_) => false,
            Type::TypeMeta => false,
            Type::TypeParam(_) => false, // Depends on the actual type
            Type::Generic { base, .. } => base.has_zero_value(),
            Type::Infer(_) => false,
            Type::Error => false,
        }
    }

    /// Calculate the size of this type in bytes (for codegen)
    ///
    /// Returns None if the size cannot be determined (e.g., variadic tuples)
    pub fn size_bytes(&self) -> Option<usize> {
        match self {
            Type::Void => Some(0),
            Type::Int(Some(bits)) | Type::UInt(Some(bits)) | Type::Float(Some(bits)) => {
                Some(*bits as usize / 8)
            }
            Type::Int(None) | Type::UInt(None) | Type::Float(None) => Some(8),
            Type::Rune => Some(4), // UTF-32
            Type::TypeMeta => Some(8), // Size of a type ID

            Type::Tuple(t) => {
                if t.variadic.is_some() {
                    // Variadic tuples need dynamic allocation
                    Some(16) // Pointer + length
                } else {
                    let mut total = 0;
                    for field in &t.fields {
                        total += field.ty.size_bytes()?;
                    }
                    Some(total)
                }
            }

            Type::Struct(s) => {
                let mut total = 0;
                for field in &s.fields {
                    total += field.ty.size_bytes()?;
                }
                Some(total)
            }

            Type::Enum(e) => {
                // Enum is a tag plus the largest case
                let tag_size = 4; // u32 discriminant
                let max_case_size = e
                    .cases
                    .iter()
                    .map(|c| {
                        let mut size = 0;
                        for field in &c.fields {
                            size += field.size_bytes()?;
                        }
                        Some(size)
                    })
                    .max()
                    .flatten()
                    .unwrap_or(0);
                Some(tag_size + max_case_size)
            }

            Type::Function(_) => Some(8), // Function pointer
            Type::TypeParam(_) => None,   // Unknown until instantiated
            Type::Generic { base, .. } => base.size_bytes(), // Simplified
            Type::Infer(_) => None,
            Type::Error => None,
        }
    }

    /// Calculate the alignment of this type in bytes (for codegen)
    pub fn alignment(&self) -> Option<usize> {
        match self {
            Type::Void => Some(1),
            Type::Int(Some(bits)) | Type::UInt(Some(bits)) | Type::Float(Some(bits)) => {
                Some((*bits as usize / 8).min(8))
            }
            Type::Int(None) | Type::UInt(None) | Type::Float(None) => Some(8),
            Type::Rune => Some(4),
            Type::TypeMeta => Some(8),

            Type::Tuple(t) => {
                if t.variadic.is_some() {
                    Some(8)
                } else {
                    t.fields
                        .iter()
                        .map(|f| f.ty.alignment())
                        .max()
                        .flatten()
                        .or(Some(1))
                }
            }

            Type::Struct(s) => s
                .fields
                .iter()
                .map(|f| f.ty.alignment())
                .max()
                .flatten()
                .or(Some(1)),

            Type::Enum(_) => Some(8), // Align to largest field + tag

            Type::Function(_) => Some(8),
            Type::TypeParam(_) => None,
            Type::Generic { base, .. } => base.alignment(),
            Type::Infer(_) => None,
            Type::Error => None,
        }
    }

    /// Check if this is a primitive type
    pub fn is_primitive(&self) -> bool {
        matches!(
            self,
            Type::Void
                | Type::Int(_)
                | Type::UInt(_)
                | Type::Float(_)
                | Type::Rune
        )
    }

    /// Check if this is a numeric type
    pub fn is_numeric(&self) -> bool {
        matches!(self, Type::Int(_) | Type::UInt(_) | Type::Float(_))
    }

    /// Check if this is an integer type (signed or unsigned)
    pub fn is_integer(&self) -> bool {
        matches!(self, Type::Int(_) | Type::UInt(_))
    }

    /// Check if this is a floating-point type
    pub fn is_float(&self) -> bool {
        matches!(self, Type::Float(_))
    }

    /// Check if this is the Bool enum type
    pub fn is_bool(&self) -> bool {
        matches!(self, Type::Enum(e) if e.name == "Bool")
    }

    /// Check if this type supports a given operator
    ///
    /// Operators work on primitives and are automatically extended to structs/tuples
    /// if all fields support the operator.
    pub fn supports_operator(&self, op: &BinaryOp) -> bool {
        match op {
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => {
                self.supports_arithmetic()
            }
            BinaryOp::Eq | BinaryOp::Ne => self.supports_equality(),
            BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                self.supports_comparison()
            }
            BinaryOp::And | BinaryOp::Or => self.is_bool(),
            BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::LShift | BinaryOp::RShift => {
                self.is_integer() || self.is_bool()
            }
            BinaryOp::Concat => self.supports_concat(),
        }
    }

    /// Check if this type supports arithmetic operators
    fn supports_arithmetic(&self) -> bool {
        match self {
            Type::Int(_) | Type::UInt(_) | Type::Float(_) | Type::Void => true,
            Type::Struct(s) => s.fields.iter().all(|f| f.ty.supports_arithmetic()),
            Type::Tuple(t) => t.fields.iter().all(|f| f.ty.supports_arithmetic()),
            Type::TypeParam(_) => true, // Type params are assumed to support ops (constraint checking TODO)
            Type::Generic { base, .. } => base.supports_arithmetic(),
            _ => false,
        }
    }

    /// Check if this type supports equality comparison
    fn supports_equality(&self) -> bool {
        match self {
            Type::Void | Type::Int(_) | Type::UInt(_) | Type::Float(_)
            | Type::Rune => true,
            Type::Struct(s) => s.fields.iter().all(|f| f.ty.supports_equality()),
            Type::Tuple(t) => t.fields.iter().all(|f| f.ty.supports_equality()),
            Type::Enum(_) => true, // Enums support equality (including Bool)
            Type::TypeParam(_) => true, // Type params are assumed to support ops (constraint checking TODO)
            Type::Generic { base, .. } => base.supports_equality(),
            _ => false,
        }
    }

    /// Check if this type supports comparison operators
    fn supports_comparison(&self) -> bool {
        match self {
            Type::Void | Type::Int(_) | Type::UInt(_) | Type::Float(_) | Type::Rune => true,
            Type::Enum(_) => true, // Enum comparison by case order
            Type::Struct(s) => s.fields.iter().all(|f| f.ty.supports_comparison()),
            Type::Tuple(t) => t.fields.iter().all(|f| f.ty.supports_comparison()),
            Type::TypeParam(_) => true, // Type params are assumed to support ops (constraint checking TODO)
            Type::Generic { base, .. } => base.supports_comparison(),
            _ => false,
        }
    }

    /// Check if this type supports concatenation (++)
    fn supports_concat(&self) -> bool {
        match self {
            Type::Struct(s) if s.name == "String" => true, // String struct from stdlib
            Type::Tuple(t) => t.variadic.is_some(), // Only variadic tuples
            Type::Void => true,
            Type::TypeParam(_) => true, // Type params assumed to support ops
            Type::Generic { base, .. } => base.supports_concat(),
            _ => false,
        }
    }
}

/// Binary operator (for operator support checking)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    BitAnd,
    BitOr,
    LShift,
    RShift,
    Concat,
}

// ============================================================================
// Display implementations
// ============================================================================

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Void => write!(f, "Void"),
            Type::Int(None) => write!(f, "Int"),
            Type::Int(Some(bits)) => write!(f, "Int({})", bits),
            Type::UInt(None) => write!(f, "UInt"),
            Type::UInt(Some(bits)) => write!(f, "UInt({})", bits),
            Type::Float(None) => write!(f, "Float"),
            Type::Float(Some(bits)) => write!(f, "Float({})", bits),
            Type::Rune => write!(f, "Rune"),
            Type::TypeMeta => write!(f, "Type"),

            Type::Tuple(t) => {
                write!(f, "(")?;
                for (i, field) in t.fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    if let Some(name) = &field.name {
                        write!(f, "{}: {}", name, field.ty)?;
                    } else {
                        write!(f, "{}", field.ty)?;
                    }
                }
                if let Some((var_ty, non_empty)) = &t.variadic {
                    if !t.fields.is_empty() {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}{}", var_ty, if *non_empty { "+" } else { "*" })?;
                }
                write!(f, ")")
            }

            Type::Struct(s) => {
                // Just show the struct name, not its parameters
                // Parameters are shown in Type::Generic when instantiated
                write!(f, "{}", s.name)
            }

            Type::Enum(e) => {
                // Just show the enum name, not its parameters
                // Parameters are shown in Type::Generic when instantiated
                write!(f, "{}", e.name)
            }

            Type::Function(func) => {
                write!(f, "(")?;
                for (i, param) in func.params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", param)?;
                }
                write!(f, ")")?;
                if let Some(ret) = &func.return_type {
                    write!(f, " -> {}", ret)?;
                }
                Ok(())
            }

            Type::TypeParam(name) => write!(f, "{}", name),

            Type::Generic { base, args } => {
                write!(f, "{}(", base)?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", arg)?;
                }
                write!(f, ")")
            }

            Type::Infer(inf) => write!(f, "?{}", inf.id),
            Type::Error => write!(f, "<error>"),
        }
    }
}

impl fmt::Display for ConstArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConstArg::Type(ty) => write!(f, "{}", ty),
            ConstArg::Int(i) => write!(f, "{}", i),
            ConstArg::Tuple(args) => {
                write!(f, "(")?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", arg)?;
                }
                write!(f, ")")
            }
            ConstArg::Param(name) => write!(f, "{}", name),
        }
    }
}

impl fmt::Display for TypeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeError::Incompatible {
                expected,
                found,
                reason,
            } => {
                write!(
                    f,
                    "Type mismatch: expected {}, found {} ({})",
                    expected, found, reason
                )
            }
            TypeError::Undefined { name } => write!(f, "Undefined type: {}", name),
            TypeError::UndefinedVariable { name } => write!(f, "Undefined variable: {}", name),
            TypeError::WrongArity { expected, found } => {
                write!(
                    f,
                    "Wrong number of type arguments: expected {}, found {}",
                    expected, found
                )
            }
            TypeError::MissingField { field, target } => {
                write!(f, "Missing field '{}' in conversion to {}", field, target)
            }
            TypeError::VariadicConversion { from, to } => {
                write!(
                    f,
                    "Cannot convert variadic fields from {} to {}",
                    from, to
                )
            }
            TypeError::Cyclic { name } => write!(f, "Cyclic type reference: {}", name),
            TypeError::Other(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for TypeError {}

// ============================================================================
// Helper functions
// ============================================================================

/// Create an Int type with default size (64 bits)
pub fn int() -> Type {
    Type::Int(None)
}

/// Create an Int type with specific bit size
pub fn int_sized(bits: u32) -> Type {
    Type::Int(Some(bits))
}

/// Create a UInt type with default size (64 bits)
pub fn uint() -> Type {
    Type::UInt(None)
}

/// Create a UInt type with specific bit size
pub fn uint_sized(bits: u32) -> Type {
    Type::UInt(Some(bits))
}

/// Create a Float type with default size (64 bits)
pub fn float() -> Type {
    Type::Float(None)
}

/// Create a Float type with specific bit size
pub fn float_sized(bits: u32) -> Type {
    Type::Float(Some(bits))
}

/// Create a simple tuple type from a list of types
pub fn tuple(fields: Vec<Type>) -> Type {
    Type::Tuple(TupleType {
        fields: fields
            .into_iter()
            .map(|ty| TupleField {
                name: None,
                ty: Box::new(ty),
            })
            .collect(),
        variadic: None,
    })
}

/// Create a named tuple type (anonymous struct)
pub fn named_tuple(fields: Vec<(String, Type)>) -> Type {
    Type::Tuple(TupleType {
        fields: fields
            .into_iter()
            .map(|(name, ty)| TupleField {
                name: Some(name),
                ty: Box::new(ty),
            })
            .collect(),
        variadic: None,
    })
}

/// Create a variadic tuple type
pub fn variadic_tuple(element: Type, non_empty: bool) -> Type {
    Type::Tuple(TupleType {
        fields: vec![],
        variadic: Some((Box::new(element), non_empty)),
    })
}

/// Create a function type
pub fn function(params: Vec<Type>, return_type: Option<Type>) -> Type {
    Type::Function(FunctionType {
        const_params: vec![],
        params: params.into_iter().map(Box::new).collect(),
        return_type: return_type.map(Box::new),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_primitive_equality() {
        assert!(Type::Int(None).structurally_equal(&Type::Int(None)));
        assert!(Type::Int(Some(32)).structurally_equal(&Type::Int(Some(32))));
        assert!(!Type::Int(Some(32)).structurally_equal(&Type::Int(Some(64))));
        assert!(!Type::Int(None).structurally_equal(&Type::UInt(None)));
    }

    #[test]
    fn test_tuple_equality() {
        let t1 = tuple(vec![int(), float()]);
        let t2 = tuple(vec![int(), float()]);
        let t3 = tuple(vec![int(), int()]);

        assert!(t1.structurally_equal(&t2));
        assert!(!t1.structurally_equal(&t3));
    }

    #[test]
    fn test_struct_to_struct_conversion() {
        // Vec3 has more fields than Vec2, so Vec3 can convert to Vec2
        let vec3 = Type::Struct(StructType {
            name: "Vec3".to_string(),
            params: vec![],
            fields: vec![
                StructField {
                    name: "x".to_string(),
                    ty: Box::new(float()),
                },
                StructField {
                    name: "y".to_string(),
                    ty: Box::new(float()),
                },
                StructField {
                    name: "z".to_string(),
                    ty: Box::new(float()),
                },
            ],
            visibility: Visibility::Internal,
        });

        let vec2 = Type::Struct(StructType {
            name: "Vec2".to_string(),
            params: vec![],
            fields: vec![
                StructField {
                    name: "x".to_string(),
                    ty: Box::new(float()),
                },
                StructField {
                    name: "y".to_string(),
                    ty: Box::new(float()),
                },
            ],
            visibility: Visibility::Internal,
        });

        assert!(vec3.can_convert_to(&vec2));
        assert!(!vec2.can_convert_to(&vec3));
    }

    #[test]
    fn test_tuple_prefix_conversion() {
        let t1 = tuple(vec![int(), float(), int()]);
        let t2 = tuple(vec![int(), float()]);

        assert!(t1.can_convert_to(&t2));
        assert!(!t2.can_convert_to(&t1));
    }

    #[test]
    fn test_type_sizes() {
        assert_eq!(Type::Void.size_bytes(), Some(0));
        assert_eq!(Type::Int(None).size_bytes(), Some(8));
        assert_eq!(Type::Int(Some(32)).size_bytes(), Some(4));
        
        // Bool is an enum now, size is tag (4 bytes) + max case (0 bytes) = 4 bytes aligned to 8
        let env = TypeEnvironment::with_stdlib();
        let bool_ty = env.resolve_type("Bool").unwrap();
        assert!(bool_ty.size_bytes().is_some());
    }

    #[test]
    fn test_operator_support() {
        assert!(int().supports_operator(&BinaryOp::Add));
        assert!(float().supports_operator(&BinaryOp::Mul));
        
        // Bool is an enum now
        let env = TypeEnvironment::with_stdlib();
        let bool_ty = env.resolve_type("Bool").unwrap();
        assert!(bool_ty.supports_operator(&BinaryOp::And));
        
        // String struct from stdlib should support concat
        let string_ty = env.resolve_type("String").unwrap();
        assert!(string_ty.supports_operator(&BinaryOp::Concat));
        assert!(!string_ty.supports_operator(&BinaryOp::Add));
    }

    #[test]
    fn test_type_environment() {
        let env = TypeEnvironment::with_stdlib();

        // Check that Bool is defined
        assert!(env.get_enum("Bool").is_some());
        assert!(env.get_enum("Option").is_some());
        assert!(env.get_enum("Result").is_some());

        // Resolve types
        assert!(env.resolve_type("Int").is_ok());
        assert!(env.resolve_type("Bool").is_ok());
        assert!(env.resolve_type("Undefined").is_err());
    }

    #[test]
    fn test_symbol_table() {
        let mut syms = SymbolTable::new();

        syms.add_variable("x".to_string(), int());
        assert!(syms.lookup("x").is_some());
        assert!(syms.lookup("y").is_none());

        syms.push_scope();
        syms.add_variable("y".to_string(), float());
        assert!(syms.lookup("x").is_some()); // Still visible
        assert!(syms.lookup("y").is_some());

        syms.pop_scope();
        assert!(syms.lookup("x").is_some());
        assert!(syms.lookup("y").is_none()); // No longer visible
    }
}
