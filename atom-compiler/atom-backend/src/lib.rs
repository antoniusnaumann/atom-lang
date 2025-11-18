pub mod codegen;
pub mod ir;
pub mod lower;
pub mod typechecker;
pub mod types;

// Re-export commonly used types
pub use codegen::{CodeGenerator, CodegenError, CodegenResult};
pub use ir::{IrProgram, IrFunction, IrBlock, IrInstruction, IrType};
pub use lower::{Lower, LowerError, LowerResult};
pub use typechecker::{TypeChecker, TypedProgram, FunctionSignature};
pub use types::{Type, TypeEnvironment, SymbolTable, TypeError};
