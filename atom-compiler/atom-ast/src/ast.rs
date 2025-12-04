use crate::span::Span;

/// Top-level item in an Atom source file
#[derive(Debug, Clone, PartialEq)]
pub enum TopLevel {
    Import(ImportDecl),
    Struct(StructDef),
    Enum(EnumDef),
    Function(FunctionDef),
    Variable(VarDecl),
    TestBlock(TestBlock),
    /// Statement (only allowed in .test.atom files)
    Statement(Stmt),
}

/// Visibility prefix: + (public), - (file-private), or none (package-internal)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Public,       // +
    FilePrivate,  // -
    Internal,     // (no prefix)
}

/// Identifier with span
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

/// Import declaration: `matrix::*` or `physics::(force, kinematics)`
/// With visibility: `+matrix::*` (re-export), `-matrix::*` (file-private), or no prefix (package-internal)
#[derive(Debug, Clone, PartialEq)]
pub struct ImportDecl {
    pub visibility: Visibility,
    pub namespace: Ident,
    pub items: ImportItems,
    pub span: Span,
}

/// Items to import from a namespace
#[derive(Debug, Clone, PartialEq)]
pub enum ImportItems {
    /// Import all items: `*`
    All,
    /// Import specific items: `(item1, item2)`
    Named(Vec<Ident>),
}

/// Struct definition: `StructName(field1 Type1, field2 Type2)`
#[derive(Debug, Clone, PartialEq)]
pub struct StructDef {
    pub visibility: Visibility,
    pub name: Ident,
    pub type_params: Vec<TypeParam>,
    pub fields: Vec<Field>,
    pub span: Span,
}

/// Struct field or enum case parameter
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub name: Option<Ident>,  // None for tuple-like fields
    pub ty: Box<Type>,
    pub span: Span,
}

/// Enum definition: `EnumName(Case1(T), Case2)`
#[derive(Debug, Clone, PartialEq)]
pub struct EnumDef {
    pub visibility: Visibility,
    pub name: Ident,
    pub type_params: Vec<TypeParam>,
    pub cases: Vec<EnumCase>,
    pub span: Span,
}

/// Enum case: `CaseName` or `CaseName(Type1, Type2)`
#[derive(Debug, Clone, PartialEq)]
pub struct EnumCase {
    pub name: Ident,
    pub fields: Vec<Box<Type>>,
    pub span: Span,
}

/// Type parameter: `t` or `n Int` or `e = String`
#[derive(Debug, Clone, PartialEq)]
pub struct TypeParam {
    pub name: Option<Ident>,  // None for positional type params
    pub ty: Option<Box<Type>>,     // The type or constraint
    pub default: Option<Box<Type>>, // Default value (for `e = String`)
    pub span: Span,
}

/// Function definition: `name(param Type) ReturnType { body }`
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDef {
    pub visibility: Visibility,
    pub name: Ident,
    pub const_params: Vec<Param>,  // Const parameters (before semicolon)
    pub params: Vec<Param>,
    pub return_type: Option<Box<Type>>,
    pub body: Block,
    pub span: Span,
}

/// Function parameter: `name Type` or `name Type = default`
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: Ident,
    pub ty: Option<Box<Type>>,
    pub default: Option<Box<Expr>>,
    pub span: Span,
}

/// Variable/constant declaration: `name := value` or `name: Type = value`
/// Also supports tuple destructuring: `a, b := expr` or `a, b := 1, 2`
#[derive(Debug, Clone, PartialEq)]
pub struct VarDecl {
    pub visibility: Visibility,
    pub is_const: bool,  // true for top-level declarations without :=
    pub names: Vec<Ident>,  // Single name for normal decl, multiple for tuple destructuring
    pub ty: Option<Box<Type>>,
    pub init: Option<Box<Expr>>,
    pub span: Span,
}

/// Test block: `"test name" { ... }` or anonymous `{ ... }`
#[derive(Debug, Clone, PartialEq)]
pub struct TestBlock {
    pub name: Option<String>,
    pub body: Block,
    pub span: Span,
}

/// Block: `{ stmt1; stmt2; expr }`
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

/// Statement
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    VarDecl(VarDecl),
    Expression(Expr),
}

/// Type expression
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    /// Type identifier: `Int`, `String`, `MyType`
    Named(Ident),
    /// Type parameter reference: `t`, `e`
    Param(Ident),
    /// Tuple type: `(T, U)` or `T, U` (relaxed notation)
    Tuple(Vec<Box<Type>>, Span),
    /// Generic type: `Option(t)`, `Result(t, e)`
    Generic {
        name: Ident,
        params: Vec<Box<TypeParam>>,
        span: Span,
    },
    /// Variadic type: `T*` or `T+` (non-empty)
    Variadic {
        element: Box<Type>,
        non_empty: bool,  // true for T+, false for T*
        span: Span,
    },
    /// Static array type: `T*n` or `T*5`
    StaticArray {
        element: Box<Type>,
        size: Box<Expr>,
        span: Span,
    },
    /// Function type: `(T, U) { R }`
    Function {
        params: Vec<Box<Type>>,
        return_type: Option<Box<Type>>,
        span: Span,
    },
    /// Reference type: `&T` (only valid for function parameters)
    Reference {
        inner: Box<Type>,
        span: Span,
    },
}

/// Expression
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// Literal value
    Literal(Literal, Span),
    /// Identifier reference
    Ident(Ident),
    /// Binary operation: `a + b`
    Binary {
        op: BinOp,
        left: Box<Expr>,
        right: Box<Expr>,
        span: Span,
    },
    /// Unary operation: `-a`, `!b`
    Unary {
        op: UnOp,
        expr: Box<Expr>,
        span: Span,
    },
    /// Function call: `func(arg1, arg2)`
    Call {
        func: Box<Expr>,
        args: Vec<Expr>,
        span: Span,
    },
    /// Method call: `obj.method(args)`
    MethodCall {
        receiver: Box<Expr>,
        method: Ident,
        args: Vec<Expr>,
        span: Span,
    },
    /// Field access: `obj.field`
    FieldAccess {
        object: Box<Expr>,
        field: Ident,
        span: Span,
    },
    /// Tuple: `(a, b, c)` or `a, b, c` (relaxed)
    Tuple(Vec<Expr>, Span),
    /// Struct initialization: `Type(field1: val1, field2: val2)`
    StructInit {
        ty: Option<Ident>,  // None for anonymous struct
        fields: Vec<FieldInit>,
        span: Span,
    },
    /// Closure: `(x, y) { x + y }`
    Closure {
        params: Vec<Param>,
        return_type: Option<Box<Type>>,
        body: Box<Block>,
        span: Span,
    },
    /// Block expression: `{ stmts }`
    Block(Block),
    /// Match expression: `match(expr) { Case1 { ... } }`
    Match {
        expr: Box<Expr>,
        arms: Vec<MatchArm>,
        span: Span,
    },
    /// Comptime expression: `#expr`
    Comptime {
        expr: Box<Expr>,
        span: Span,
    },
    /// Reference expression: `&expr`
    Reference {
        expr: Box<Expr>,
        span: Span,
    },
    /// Multi-way comparison: `a < b < c` or `x == y == True`
    /// Stores the operands and operators in order
    MultiComparison {
        operands: Vec<Expr>,
        operators: Vec<BinOp>,
        span: Span,
    },
}

/// Field initialization in struct literal: `name: value` or just `value`
#[derive(Debug, Clone, PartialEq)]
pub struct FieldInit {
    pub name: Option<Ident>,
    pub value: Box<Expr>,
    pub span: Span,
}

/// Match arm: `Pattern { expr }`
#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Box<Expr>,
    pub span: Span,
}

/// Pattern for match expressions
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    /// Wildcard: `_`
    Wildcard(Span),
    /// Literal: `5`, `"hello"`, `True`
    Literal(Literal, Span),
    /// Identifier binding: `x`
    Ident(Ident),
    /// Tuple pattern: `(a, b, c)`
    Tuple(Vec<Pattern>, Span),
    /// Enum pattern: `Some(x)`, `None`
    Enum {
        name: Ident,
        fields: Vec<Pattern>,
        span: Span,
    },
    /// Alternative patterns: `pattern1 | pattern2 | pattern3`
    Alternative(Vec<Pattern>, Span),
    /// Expression (guard): `x > 5`, `a && b`
    /// Used in guard-style matches like: match(True) { x > 5 { ... } }
    Expr(Box<Expr>),
}

/// Literal value
#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Integer(i64),
    Float(f64),
    String(String),
    Rune(char),
    Bool(bool),
}

/// Binary operator
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    // Arithmetic
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    // Comparison
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    // Logical
    And,
    Or,
    // Bitwise
    BitAnd,
    BitOr,
    LShift,
    RShift,
    // Collection
    Concat,      // ++
    // Assignment
    Assign,      // =
    AddAssign,   // +=
    SubAssign,   // -=
    MulAssign,   // *=
    DivAssign,   // /=
    ModAssign,   // %=
    ConcatAssign, // ++=
}

/// Unary operator
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,     // -
    Not,     // !
    BitNot,  // ~
}

impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Literal(_, span) => *span,
            Expr::Ident(ident) => ident.span,
            Expr::Binary { span, .. } => *span,
            Expr::Unary { span, .. } => *span,
            Expr::Call { span, .. } => *span,
            Expr::MethodCall { span, .. } => *span,
            Expr::FieldAccess { span, .. } => *span,
            Expr::Tuple(_, span) => *span,
            Expr::StructInit { span, .. } => *span,
            Expr::Closure { span, .. } => *span,
            Expr::Block(block) => block.span,
            Expr::Match { span, .. } => *span,
            Expr::Comptime { span, .. } => *span,
            Expr::Reference { span, .. } => *span,
            Expr::MultiComparison { span, .. } => *span,
        }
    }
}
