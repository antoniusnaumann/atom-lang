#![allow(unused)]
#![allow(clippy::all)]

//! Intermediate Representation (IR) for the Atom compiler.
//!
//! This module defines a simple, SSA-like IR that sits between the AST and
//! Cranelift code generation. The IR is designed to be:
//! - Easy to construct from the typed AST
//! - Simple to translate to Cranelift IR
//! - Clear and debuggable for educational purposes
//!
//! # Design
//!
//! The IR uses a basic block structure where:
//! - Each function consists of basic blocks
//! - Each basic block contains a sequence of instructions
//! - Instructions produce values that can be referenced by later instructions
//! - Control flow is explicit via jump/branch instructions
//! - Types are tracked on all values for type-safe code generation

use std::fmt;

/// A complete Atom program in IR form.
#[derive(Debug, Clone)]
pub struct IrProgram {
    /// All struct type definitions
    pub structs: Vec<IrStructDef>,
    /// All enum type definitions
    pub enums: Vec<IrEnumDef>,
    /// All global variables
    pub globals: Vec<IrGlobal>,
    /// All function definitions
    pub functions: Vec<IrFunction>,
}

/// A struct type definition.
#[derive(Debug, Clone, PartialEq)]
pub struct IrStructDef {
    /// Struct name
    pub name: String,
    /// Field names and types
    pub fields: Vec<(String, IrType)>,
}

/// An enum type definition.
#[derive(Debug, Clone, PartialEq)]
pub struct IrEnumDef {
    /// Enum name
    pub name: String,
    /// Variant names and associated data types
    pub variants: Vec<(String, Vec<IrType>)>,
}

/// A global variable.
#[derive(Debug, Clone)]
pub struct IrGlobal {
    /// Global variable name
    pub name: String,
    /// Type of the global
    pub ty: IrType,
    /// Initial value (None for zero-initialized)
    pub init: Option<IrConstant>,
    /// Whether this is exported
    pub is_public: bool,
}

/// A function in IR form.
#[derive(Debug, Clone)]
pub struct IrFunction {
    /// Function name (mangled if needed for overloading)
    pub name: String,
    /// Parameter names and types
    pub params: Vec<(String, IrType)>,
    /// Return type (None for void/unit)
    pub return_type: Option<IrType>,
    /// Basic blocks that make up the function body
    pub blocks: Vec<IrBlock>,
    /// Local variables (stack allocations)
    pub locals: Vec<IrLocal>,
    /// Whether this function is exported
    pub is_public: bool,
}

/// A local variable (stack allocation).
#[derive(Debug, Clone)]
pub struct IrLocal {
    /// Local variable ID
    pub id: LocalId,
    /// Name (for debugging)
    pub name: String,
    /// Type of the local
    pub ty: IrType,
}

/// A basic block - a sequence of instructions with a single entry and exit.
#[derive(Debug, Clone)]
pub struct IrBlock {
    /// Block label/identifier
    pub label: BlockId,
    /// Instructions in this block
    pub instructions: Vec<IrInstruction>,
    /// Terminator instruction (jump, branch, return)
    pub terminator: IrTerminator,
}

/// An instruction that performs computation and produces a value.
#[derive(Debug, Clone)]
pub struct IrInstruction {
    /// The value ID this instruction produces
    pub result: ValueId,
    /// The type of the value produced
    pub ty: IrType,
    /// The operation to perform
    pub kind: IrInstructionKind,
}

/// The kind of instruction operation.
#[derive(Debug, Clone)]
pub enum IrInstructionKind {
    /// Binary arithmetic or logical operation
    BinOp {
        op: IrBinOp,
        left: ValueId,
        right: ValueId,
    },
    /// Unary operation
    UnOp {
        op: IrUnOp,
        operand: ValueId,
    },
    /// Load a value from memory (local variable, struct field, etc.)
    Load {
        source: IrMemoryLocation,
    },
    /// Store a value to memory (local variable, struct field, etc.)
    Store {
        destination: IrMemoryLocation,
        value: ValueId,
    },
    /// Function call (direct)
    Call {
        function: String,
        args: Vec<ValueId>,
        is_tail: bool,  // True if this is a tail call
    },
    /// Indirect function call (via closure/function pointer)
    CallIndirect {
        func_value: ValueId,
        args: Vec<ValueId>,
    },
    /// Construct a tuple
    MakeTuple {
        elements: Vec<ValueId>,
    },
    /// Extract element from tuple by index
    TupleExtract {
        tuple: ValueId,
        index: u32,
    },
    /// Create an array/slice from elements (allocates on heap)
    MakeArray {
        element_type: IrType,
        elements: Vec<ValueId>,
    },
    /// Get array length
    ArrayLen {
        array: ValueId,
    },
    /// Index into array (returns element value)
    ArrayIndex {
        array: ValueId,
        index: ValueId,
    },
    /// Append element to array (creates new array)
    ArrayAppend {
        array: ValueId,
        element: ValueId,
    },
    /// Concatenate two arrays
    ArrayConcat {
        left: ValueId,
        right: ValueId,
    },
    /// Construct a struct
    MakeStruct {
        struct_name: String,
        fields: Vec<ValueId>,
    },
    /// Extract field from struct by index
    StructExtract {
        struct_value: ValueId,
        field_index: u32,
    },
    /// Construct an enum variant
    MakeEnum {
        enum_name: String,
        variant_index: u32,
        values: Vec<ValueId>,
    },
    /// Construct a closure (captures environment)
    MakeClosure {
        function: String,
        captures: Vec<ValueId>,
    },
    /// Load from a captured variable in closure
    LoadCapture {
        index: u32,
    },
    /// Constant value
    Const {
        value: IrConstant,
    },
    /// Phi node for SSA (merge values from different control flow paths)
    Phi {
        incoming: Vec<(BlockId, ValueId)>,
    },
    /// Take the address of a memory location (lvalue)
    AddressOf {
        location: IrMemoryLocation,
    },
    /// Dereference a pointer
    Deref {
        pointer: ValueId,
    },
}

/// Memory location for load/store operations.
#[derive(Debug, Clone)]
pub enum IrMemoryLocation {
    /// Local variable
    Local(LocalId),
    /// Struct field
    StructField {
        base: ValueId,
        field_index: u32,
    },
    /// Tuple element
    TupleElement {
        base: ValueId,
        index: u32,
    },
    /// Global variable
    Global(String),
}

/// Terminator instruction - ends a basic block.
#[derive(Debug, Clone)]
pub enum IrTerminator {
    /// Unconditional jump to another block
    Jump {
        target: BlockId,
    },
    /// Conditional branch
    Branch {
        condition: ValueId,
        true_block: BlockId,
        false_block: BlockId,
    },
    /// Return from function
    Return {
        value: Option<ValueId>,
    },
    /// Match/switch on enum tag
    Switch {
        value: ValueId,
        /// Map from variant index to block
        cases: Vec<(u32, BlockId)>,
        /// Default case if no match
        default: BlockId,
    },
    /// Unreachable code (for exhaustive matches, panics, etc.)
    Unreachable,
}

/// IR types - simplified from AST types.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum IrType {
    /// Void/unit type
    Void,
    /// Boolean
    Bool,
    /// Signed integer with bit width
    Int(u16),
    /// Unsigned integer with bit width
    UInt(u16),
    /// Floating point with bit width (32 or 64)
    Float(u16),
    /// Unicode codepoint (32-bit)
    Rune,
    /// Tuple of types
    Tuple(Vec<IrType>),
    /// Named struct type
    Struct(String),
    /// Named enum type
    Enum(String),
    /// Generic enum instantiation (e.g., Option(Int64))
    GenericEnum {
        name: String,
        type_args: Vec<IrType>,
    },
    /// Generic struct instantiation (e.g., Container(Int64))
    GenericStruct {
        name: String,
        type_args: Vec<IrType>,
    },
    /// Function pointer type
    Function {
        params: Vec<IrType>,
        return_type: Box<Option<IrType>>,
    },
    /// Closure type (function + environment)
    Closure {
        params: Vec<IrType>,
        return_type: Box<Option<IrType>>,
    },
    /// Pointer to a type (for references, heap allocations)
    Pointer(Box<IrType>),
    /// Array/slice type (fat pointer: pointer + length)
    /// Represents variadic tuples `T*` or `T+`
    Array {
        element: Box<IrType>,
    },
}

/// Binary operators in IR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrBinOp {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    // Concatenation (for ++ operator)
    Concat,
    // Comparison
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    // Logical (these are not short-circuiting in IR)
    And,
    Or,
    // Bitwise
    BitAnd,
    BitOr,
    LShift,
    RShift,
}

/// Unary operators in IR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrUnOp {
    /// Arithmetic negation
    Neg,
    /// Logical not
    Not,
    /// Bitwise not
    BitNot,
}

/// Constant values in IR.
#[derive(Debug, Clone, PartialEq)]
pub enum IrConstant {
    /// Integer constant
    Int(i64),
    /// Unsigned integer constant
    UInt(u64),
    /// Float constant
    Float(f64),
    /// Rune (Unicode codepoint)
    Rune(char),
    /// Boolean constant
    Bool(bool),
    /// String constant (stored as bytes)
    String(Vec<u8>),
    /// Void/unit constant
    Void,
}

/// Value identifier - references a computed value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ValueId(pub u32);

/// Block identifier - references a basic block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId(pub u32);

/// Local variable identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LocalId(pub u32);

// ============================================================================
// Display implementations for debugging
// ============================================================================

impl fmt::Display for IrProgram {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "IR Program")?;
        writeln!(f, "==========")?;
        writeln!(f)?;

        if !self.structs.is_empty() {
            writeln!(f, "Structs:")?;
            for s in &self.structs {
                writeln!(f, "  {}", s)?;
            }
            writeln!(f)?;
        }

        if !self.enums.is_empty() {
            writeln!(f, "Enums:")?;
            for e in &self.enums {
                writeln!(f, "  {}", e)?;
            }
            writeln!(f)?;
        }

        if !self.globals.is_empty() {
            writeln!(f, "Globals:")?;
            for g in &self.globals {
                writeln!(f, "  {}", g)?;
            }
            writeln!(f)?;
        }

        writeln!(f, "Functions:")?;
        for func in &self.functions {
            writeln!(f, "{}", func)?;
        }

        Ok(())
    }
}

impl fmt::Display for IrStructDef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "struct {} (", self.name)?;
        for (i, (name, ty)) in self.fields.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}: {}", name, ty)?;
        }
        write!(f, ")")
    }
}

impl fmt::Display for IrEnumDef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "enum {} (", self.name)?;
        for (i, (name, types)) in self.variants.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}", name)?;
            if !types.is_empty() {
                write!(f, "(")?;
                for (j, ty) in types.iter().enumerate() {
                    if j > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", ty)?;
                }
                write!(f, ")")?;
            }
        }
        write!(f, ")")
    }
}

impl fmt::Display for IrGlobal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}: {}",
            if self.is_public { "+" } else { "" },
            self.name,
            self.ty
        )?;
        if let Some(init) = &self.init {
            write!(f, " = {}", init)?;
        }
        Ok(())
    }
}

impl fmt::Display for IrFunction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}(",
            if self.is_public { "+" } else { "" },
            self.name
        )?;
        for (i, (name, ty)) in self.params.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{}: {}", name, ty)?;
        }
        write!(f, ")")?;
        if let Some(ret) = &self.return_type {
            write!(f, " -> {}", ret)?;
        }
        writeln!(f, " {{")?;

        if !self.locals.is_empty() {
            writeln!(f, "  locals:")?;
            for local in &self.locals {
                writeln!(f, "    {}", local)?;
            }
            writeln!(f)?;
        }

        for block in &self.blocks {
            writeln!(f, "{}", block)?;
        }

        writeln!(f, "}}")
    }
}

impl fmt::Display for IrLocal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({}): {}", self.name, self.id, self.ty)
    }
}

impl fmt::Display for IrBlock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "  {}:", self.label)?;
        for inst in &self.instructions {
            writeln!(f, "    {}", inst)?;
        }
        writeln!(f, "    {}", self.terminator)
    }
}

impl fmt::Display for IrInstruction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} = ", self.result)?;
        match &self.kind {
            IrInstructionKind::BinOp { op, left, right } => {
                write!(f, "{} {} {}", left, op, right)
            }
            IrInstructionKind::UnOp { op, operand } => {
                write!(f, "{} {}", op, operand)
            }
            IrInstructionKind::Load { source } => {
                write!(f, "load {}", source)
            }
            IrInstructionKind::Store { destination, value } => {
                write!(f, "store {}, {}", destination, value)
            }
            IrInstructionKind::Call { function, args, is_tail } => {
                if *is_tail {
                    write!(f, "tail_call {}(", function)?;
                } else {
                    write!(f, "call {}(", function)?;
                }
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", arg)?;
                }
                write!(f, ")")
            }
            IrInstructionKind::CallIndirect { func_value, args } => {
                write!(f, "call_indirect {}(", func_value)?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", arg)?;
                }
                write!(f, ")")
            }
            IrInstructionKind::MakeTuple { elements } => {
                write!(f, "tuple(")?;
                for (i, elem) in elements.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", elem)?;
                }
                write!(f, ")")
            }
            IrInstructionKind::TupleExtract { tuple, index } => {
                write!(f, "tuple_extract {}, {}", tuple, index)
            }
            IrInstructionKind::MakeArray { element_type, elements } => {
                write!(f, "array<{}>(", element_type)?;
                for (i, elem) in elements.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", elem)?;
                }
                write!(f, ")")
            }
            IrInstructionKind::ArrayLen { array } => {
                write!(f, "array_len {}", array)
            }
            IrInstructionKind::ArrayIndex { array, index } => {
                write!(f, "array_index {}, {}", array, index)
            }
            IrInstructionKind::ArrayAppend { array, element } => {
                write!(f, "array_append {}, {}", array, element)
            }
            IrInstructionKind::ArrayConcat { left, right } => {
                write!(f, "array_concat {}, {}", left, right)
            }
            IrInstructionKind::MakeStruct { struct_name, fields } => {
                write!(f, "struct {}(", struct_name)?;
                for (i, field) in fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", field)?;
                }
                write!(f, ")")
            }
            IrInstructionKind::StructExtract {
                struct_value,
                field_index,
            } => {
                write!(f, "struct_extract {}, {}", struct_value, field_index)
            }
            IrInstructionKind::MakeEnum {
                enum_name,
                variant_index,
                values,
            } => {
                write!(f, "enum {}.{}(", enum_name, variant_index)?;
                for (i, val) in values.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", val)?;
                }
                write!(f, ")")
            }
            IrInstructionKind::MakeClosure { function, captures } => {
                write!(f, "closure {}(", function)?;
                for (i, cap) in captures.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", cap)?;
                }
                write!(f, ")")
            }
            IrInstructionKind::LoadCapture { index } => {
                write!(f, "load_capture {}", index)
            }
            IrInstructionKind::Const { value } => {
                write!(f, "const {}", value)
            }
            IrInstructionKind::Phi { incoming } => {
                write!(f, "phi(")?;
                for (i, (block, value)) in incoming.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "[{}: {}]", block, value)?;
                }
                write!(f, ")")
            }
            IrInstructionKind::AddressOf { location } => {
                write!(f, "address_of {}", location)
            }
            IrInstructionKind::Deref { pointer } => {
                write!(f, "deref {}", pointer)
            }
        }?;
        write!(f, " : {}", self.ty)
    }
}

impl fmt::Display for IrMemoryLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IrMemoryLocation::Local(id) => write!(f, "local {}", id),
            IrMemoryLocation::StructField { base, field_index } => {
                write!(f, "field({}, {})", base, field_index)
            }
            IrMemoryLocation::TupleElement { base, index } => {
                write!(f, "element({}, {})", base, index)
            }
            IrMemoryLocation::Global(name) => write!(f, "global {}", name),
        }
    }
}

impl fmt::Display for IrTerminator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IrTerminator::Jump { target } => write!(f, "jump {}", target),
            IrTerminator::Branch {
                condition,
                true_block,
                false_block,
            } => write!(
                f,
                "branch {} ? {} : {}",
                condition, true_block, false_block
            ),
            IrTerminator::Return { value } => {
                if let Some(v) = value {
                    write!(f, "return {}", v)
                } else {
                    write!(f, "return")
                }
            }
            IrTerminator::Switch {
                value,
                cases,
                default,
            } => {
                write!(f, "switch {} [", value)?;
                for (i, (tag, block)) in cases.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", tag, block)?;
                }
                write!(f, ", default: {}]", default)
            }
            IrTerminator::Unreachable => write!(f, "unreachable"),
        }
    }
}

impl fmt::Display for IrType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IrType::Void => write!(f, "void"),
            IrType::Bool => write!(f, "bool"),
            IrType::Int(bits) => write!(f, "i{}", bits),
            IrType::UInt(bits) => write!(f, "u{}", bits),
            IrType::Float(bits) => write!(f, "f{}", bits),
            IrType::Rune => write!(f, "rune"),
            IrType::Tuple(types) => {
                write!(f, "(")?;
                for (i, ty) in types.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", ty)?;
                }
                write!(f, ")")
            }
            IrType::Struct(name) => write!(f, "{}", name),
            IrType::Enum(name) => write!(f, "{}", name),
            IrType::GenericEnum { name, type_args } => {
                write!(f, "{}", name)?;
                if !type_args.is_empty() {
                    write!(f, "(")?;
                    for (i, arg) in type_args.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{}", arg)?;
                    }
                    write!(f, ")")?;
                }
                Ok(())
            }
            IrType::GenericStruct { name, type_args } => {
                write!(f, "{}", name)?;
                if !type_args.is_empty() {
                    write!(f, "(")?;
                    for (i, arg) in type_args.iter().enumerate() {
                        if i > 0 {
                            write!(f, ", ")?;
                        }
                        write!(f, "{}", arg)?;
                    }
                    write!(f, ")")?;
                }
                Ok(())
            }
            IrType::Function {
                params,
                return_type,
            } => {
                write!(f, "fn(")?;
                for (i, param) in params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", param)?;
                }
                write!(f, ")")?;
                if let Some(ret) = return_type.as_ref() {
                    write!(f, " -> {}", ret)?;
                }
                Ok(())
            }
            IrType::Closure {
                params,
                return_type,
            } => {
                write!(f, "closure(")?;
                for (i, param) in params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", param)?;
                }
                write!(f, ")")?;
                if let Some(ret) = return_type.as_ref() {
                    write!(f, " -> {}", ret)?;
                }
                Ok(())
            }
            IrType::Pointer(inner) => write!(f, "*{}", inner),
            IrType::Array { element } => write!(f, "{}*", element),
        }
    }
}

impl fmt::Display for IrBinOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            IrBinOp::Add => "+",
            IrBinOp::Sub => "-",
            IrBinOp::Mul => "*",
            IrBinOp::Div => "/",
            IrBinOp::Mod => "%",
            IrBinOp::Concat => "++",
            IrBinOp::Eq => "==",
            IrBinOp::Ne => "!=",
            IrBinOp::Lt => "<",
            IrBinOp::Le => "<=",
            IrBinOp::Gt => ">",
            IrBinOp::Ge => ">=",
            IrBinOp::And => "&&",
            IrBinOp::Or => "||",
            IrBinOp::BitAnd => "&",
            IrBinOp::BitOr => "|",
            IrBinOp::LShift => "<<",
            IrBinOp::RShift => ">>",
        };
        write!(f, "{}", s)
    }
}

impl fmt::Display for IrUnOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            IrUnOp::Neg => "-",
            IrUnOp::Not => "!",
            IrUnOp::BitNot => "~",
        };
        write!(f, "{}", s)
    }
}

impl fmt::Display for IrConstant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IrConstant::Int(n) => write!(f, "{}", n),
            IrConstant::UInt(n) => write!(f, "{}u", n),
            IrConstant::Float(n) => write!(f, "{}", n),
            IrConstant::Rune(c) => write!(f, "'{}'", c),
            IrConstant::Bool(b) => write!(f, "{}", b),
            IrConstant::String(bytes) => {
                write!(f, "\"")?;
                for &byte in bytes {
                    write!(f, "{}", byte as char)?;
                }
                write!(f, "\"")
            }
            IrConstant::Void => write!(f, "()"),
        }
    }
}

impl fmt::Display for ValueId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "%{}", self.0)
    }
}

impl fmt::Display for BlockId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bb{}", self.0)
    }
}

impl fmt::Display for LocalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "${}", self.0)
    }
}

// ============================================================================
// Helper methods and utilities
// ============================================================================

impl IrProgram {
    /// Create a new empty IR program.
    pub fn new() -> Self {
        Self {
            structs: Vec::new(),
            enums: Vec::new(),
            globals: Vec::new(),
            functions: Vec::new(),
        }
    }

    /// Add a struct definition.
    pub fn add_struct(&mut self, def: IrStructDef) {
        self.structs.push(def);
    }

    /// Add an enum definition.
    pub fn add_enum(&mut self, def: IrEnumDef) {
        self.enums.push(def);
    }

    /// Add a global variable.
    pub fn add_global(&mut self, global: IrGlobal) {
        self.globals.push(global);
    }

    /// Add a function.
    pub fn add_function(&mut self, func: IrFunction) {
        self.functions.push(func);
    }

    /// Find a struct definition by name.
    pub fn find_struct(&self, name: &str) -> Option<&IrStructDef> {
        self.structs.iter().find(|s| s.name == name)
    }

    /// Find an enum definition by name.
    pub fn find_enum(&self, name: &str) -> Option<&IrEnumDef> {
        self.enums.iter().find(|e| e.name == name)
    }
}

impl IrFunction {
    /// Create a new function.
    pub fn new(
        name: String,
        params: Vec<(String, IrType)>,
        return_type: Option<IrType>,
        is_public: bool,
    ) -> Self {
        Self {
            name,
            params,
            return_type,
            blocks: Vec::new(),
            locals: Vec::new(),
            is_public,
        }
    }

    /// Add a basic block.
    pub fn add_block(&mut self, block: IrBlock) {
        self.blocks.push(block);
    }

    /// Add a local variable.
    pub fn add_local(&mut self, name: String, ty: IrType) -> LocalId {
        let id = LocalId(self.locals.len() as u32);
        self.locals.push(IrLocal { id, name, ty });
        id
    }

    /// Get entry block (first block).
    pub fn entry_block(&self) -> Option<&IrBlock> {
        self.blocks.first()
    }
}

impl IrBlock {
    /// Create a new basic block.
    pub fn new(label: BlockId) -> Self {
        Self {
            label,
            instructions: Vec::new(),
            terminator: IrTerminator::Unreachable,
        }
    }

    /// Add an instruction to this block.
    pub fn add_instruction(&mut self, inst: IrInstruction) {
        self.instructions.push(inst);
    }

    /// Set the terminator for this block.
    pub fn set_terminator(&mut self, term: IrTerminator) {
        self.terminator = term;
    }
}

impl IrType {
    /// Check if this is a void type.
    pub fn is_void(&self) -> bool {
        matches!(self, IrType::Void)
    }

    /// Check if this is a numeric type.
    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            IrType::Int(_) | IrType::UInt(_) | IrType::Float(_)
        )
    }

    /// Check if this is a signed integer type.
    pub fn is_signed_int(&self) -> bool {
        matches!(self, IrType::Int(_))
    }

    /// Check if this is an unsigned integer type.
    pub fn is_unsigned_int(&self) -> bool {
        matches!(self, IrType::UInt(_))
    }

    /// Check if this is a floating point type.
    pub fn is_float(&self) -> bool {
        matches!(self, IrType::Float(_))
    }

    /// Get the size in bits if this is a primitive numeric type.
    pub fn bit_width(&self) -> Option<u16> {
        match self {
            IrType::Int(bits) | IrType::UInt(bits) | IrType::Float(bits) => Some(*bits),
            IrType::Bool => Some(1),
            IrType::Rune => Some(32),
            _ => None,
        }
    }
}

impl Default for IrProgram {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ir_program_creation() {
        let mut program = IrProgram::new();

        // Add a simple struct
        program.add_struct(IrStructDef {
            name: "Vec2".to_string(),
            fields: vec![
                ("x".to_string(), IrType::Float(64)),
                ("y".to_string(), IrType::Float(64)),
            ],
        });

        assert_eq!(program.structs.len(), 1);
        assert_eq!(program.find_struct("Vec2").unwrap().name, "Vec2");
    }

    #[test]
    fn test_ir_function_creation() {
        let mut func = IrFunction::new(
            "add".to_string(),
            vec![
                ("a".to_string(), IrType::Int(64)),
                ("b".to_string(), IrType::Int(64)),
            ],
            Some(IrType::Int(64)),
            true,
        );

        let local = func.add_local("result".to_string(), IrType::Int(64));
        assert_eq!(local, LocalId(0));

        let mut block = IrBlock::new(BlockId(0));
        block.add_instruction(IrInstruction {
            result: ValueId(0),
            ty: IrType::Int(64),
            kind: IrInstructionKind::BinOp {
                op: IrBinOp::Add,
                left: ValueId(1),
                right: ValueId(2),
            },
        });
        block.set_terminator(IrTerminator::Return {
            value: Some(ValueId(0)),
        });

        func.add_block(block);

        assert_eq!(func.blocks.len(), 1);
        assert_eq!(func.locals.len(), 1);
    }

    #[test]
    fn test_type_queries() {
        assert!(IrType::Void.is_void());
        assert!(IrType::Int(64).is_numeric());
        assert!(IrType::Int(64).is_signed_int());
        assert!(IrType::UInt(32).is_unsigned_int());
        assert!(IrType::Float(64).is_float());
        assert_eq!(IrType::Int(64).bit_width(), Some(64));
        assert_eq!(IrType::Bool.bit_width(), Some(1));
    }

    #[test]
    fn test_display_formatting() {
        let block = IrBlock {
            label: BlockId(0),
            instructions: vec![IrInstruction {
                result: ValueId(0),
                ty: IrType::Int(64),
                kind: IrInstructionKind::Const {
                    value: IrConstant::Int(42),
                },
            }],
            terminator: IrTerminator::Return {
                value: Some(ValueId(0)),
            },
        };

        let output = format!("{}", block);
        assert!(output.contains("bb0:"));
        assert!(output.contains("%0"));
        assert!(output.contains("const 42"));
        assert!(output.contains("return %0"));
    }
}
