# Atom Compiler Backend

A complete compiler backend for the Atom programming language, featuring type checking, intermediate representation (IR), and native code generation via Cranelift.

## Architecture

The compiler backend consists of five main components:

1. **Type System** (`types.rs`) - Structural type system with subtyping
2. **Type Checker** (`typechecker.rs`) - Type checking and inference
3. **Intermediate Representation** (`ir.rs`) - SSA-based IR for optimization
4. **Lowering** (`lower.rs`) - AST to IR translation
5. **Code Generation** (`codegen.rs`) - Cranelift-based native code generation

### Pipeline

```
S-Expression AST → Type Checker → Typed AST → Lowering → IR → Cranelift → Object File
```

## Features

### Type System
- **Structural typing** - types are compared by structure, not names
- **Implicit conversions** following Atom language spec:
  - Struct → Struct (field subset)
  - Tuple → Tuple (prefix matching)
  - Tuple ↔ Struct (bidirectional)
  - Fields → Variadic fields
- **Primitives**: Int, UInt, Float (with bit sizes), Bool, Rune, String, Void
- **Composite types**: Tuples, Structs, Enums, Functions
- **Variadic tuples**: `T*` (zero or more), `T+` (one or more)
- **Generic/const parameters** with defaults

### Type Checking
- Three-pass type checking (collect types → collect signatures → check bodies)
- Function overloading support
- Pattern matching with type checking
- Closure type checking
- Comprehensive error messages
- Fail-fast on first error

### Intermediate Representation
- SSA (Single Static Assignment) form
- Basic block structure
- Type-annotated values
- Supports all Atom language features:
  - Functions, structs, enums
  - Match expressions
  - Closures (with capture)
  - All operators (arithmetic, logical, bitwise, comparison)
  - Tuples and struct operations

### Code Generation
- Cranelift-based native code generation
- Target-agnostic IR (can compile for different architectures)
- Efficient basic block translation
- Type size and alignment calculation
- Object file output

## Usage

### Command Line

```bash
# Compile S-Expression AST files to object file
atom-compile input1.sexpr input2.sexpr -o output.o

# Link with C compiler (manual step for now)
cc output.o -o executable
```

### As a Library

```rust
use atom_backend::{TypeChecker, Lower, CodeGenerator};
use atom_ast::from_sexpr::{FromSExpr, SExpr};

// Parse S-Expression AST
let sexpr = SExpr::parse(input)?;
let ast = Vec::<atom_ast::ast::TopLevel>::from_sexpr(&sexpr)?;

// Type check
let mut type_checker = TypeChecker::new();
let typed_program = type_checker.check_program(ast)?;

// Lower to IR
let mut lower = Lower::new(typed_program.type_env.clone());
let ir_program = lower.lower_program(typed_program.ast)?;

// Generate code
let mut codegen = CodeGenerator::new();
codegen.compile(ir_program, "output.o")?;
```

## Testing

The backend includes 28+ unit tests covering:
- IR creation and manipulation
- Type system (equality, conversions, sizes)
- Type checking (functions, structs, enums, patterns)
- AST to IR lowering
- Code generation

Plus 3 integration tests for end-to-end compilation.

Run tests:
```bash
cd atom-compiler
cargo test --package atom-backend
```

Run with verbose output:
```bash
cargo test --package atom-backend -- --nocapture
```

## Code Quality

- **No clippy warnings** with `-D warnings`
- **Rust 2024 edition**
- **Comprehensive documentation** - all public APIs documented
- **Well-tested** - 28+ unit tests, 3 integration tests

## Implementation Status

### ✅ Implemented
- Complete type system with structural typing
- Type checker for all major language features
- SSA-based IR with basic blocks
- AST to IR lowering for expressions and statements
- Cranelift backend for basic arithmetic and control flow
- Function calls (direct)
- Basic tuple and struct support
- Object file generation

### 🚧 Partial Implementation
- Closures (IR support exists, codegen needs work)
- Enums (type checking done, codegen needs work)
- String constants (need global data section)
- Method calls (lowering not yet implemented)

### 📋 Future Work
- Full closure code generation with capture
- Enum construction and pattern matching codegen
- Loop constructs (currently use manual branches)
- Mutable variables (Store instructions)
- Optimization passes
- Linker integration
- Executable output (currently generates object files)

## Dependencies

- `atom-ast` - AST data structures and S-Expression parsing
- `cranelift` - Code generation framework
- `cranelift-module` - Module management
- `cranelift-object` - Object file generation
- `target-lexicon` - Target architecture specification

## Project Structure

```
atom-backend/
├── src/
│   ├── lib.rs          # Public API
│   ├── main.rs         # CLI tool
│   ├── types.rs        # Type system (1170 lines)
│   ├── typechecker.rs  # Type checker (950 lines)
│   ├── ir.rs           # Intermediate representation (1017 lines)
│   ├── lower.rs        # AST to IR lowering (1050 lines)
│   └── codegen.rs      # Cranelift code generation (1000 lines)
├── tests/
│   └── integration_test.rs
├── Cargo.toml
└── README.md (this file)
```

## Contributing

The codebase follows Rust 2024 edition standards and Atom project guidelines (see `/AGENTS.md`). Key conventions:

- **Naming**: `snake_case` for functions/variables, `PascalCase` for types
- **Imports**: Group by `crate::`, external crates, then workspace crates
- **Formatting**: Standard rustfmt (4-space indentation)
- **Error handling**: Return `Result` with custom error types, use `?` operator
- **Comments**: `///` for public items, `//` for implementation details

## License

Part of the Atom programming language project.
