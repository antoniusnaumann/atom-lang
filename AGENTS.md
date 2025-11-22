# Agent Guidelines for Atom Language Development

## Build & Test Commands
- **Build**: `cd atom-compiler && cargo build --workspace`
- **Release build**: `cd atom-compiler && cargo build --workspace --release`
- **All tests**: `cd atom-compiler && cargo test --workspace --verbose`
- **Single test**: `cd atom-compiler && cargo test <TESTNAME> --verbose` (e.g., `cargo test parse_all_files`)
- **Single test in crate**: `cd atom-compiler/atom-parser && cargo test <TESTNAME> --verbose`
- **Lint**: `cd atom-compiler && cargo clippy --workspace -- -D warnings`
- **Compile Atom source**: `./atomc <file.atom> [--no-std] [-o output] [--debug]`
- **Tree-sitter build**: `cd tree-sitter-atom && tree-sitter generate`
- **Tree-sitter test**: `cd tree-sitter-atom && tree-sitter parse <file.atom> --quiet --stat`

## Code Style (Rust)
- **Edition**: 2024, workspace members in `atom-compiler/`
- **Imports**: Group by `crate::`, external crates, then `atom_ast::{self, ...}`. Use relative paths for internal modules.
- **Formatting**: Standard rustfmt (no custom config). Prefer 4-space indentation.
- **Types**: Explicit type annotations for struct fields and function params. Use `Box<T>` for recursive types.
- **Naming**: `snake_case` for functions/vars/modules, `PascalCase` for types/enums, `SCREAMING_SNAKE_CASE` for consts.
- **Error handling**: Return `ParseResult<T>` (alias for `Result<T, ParseError>`). Use `?` operator for propagation.
- **Patterns**: Prefer pattern matching over conditionals. Use `Option` and `Result` explicitly, avoid unwrap in lib code.
- **Comments**: Use doc comments `///` for public items. Inline comments for complex logic only.
- **Debug output**: Wrap verbose debug output with `if std::env::var("ATOM_DEBUG").ok().as_deref() == Some("1")` checks.

## Atom Language Rules
- **Casing**: `PascalCase` for types/enum cases, `snake_case` for functions/vars/fields/modules. No keywords—structural typing.
- **Visibility**: `+` (public), `-` (file-private), none (package-internal). Prefix types and functions.
- **Specification**: Always reference the README.md and the .atom files in std/src/ and examples/ when working on the compiler.

## Architecture
- **Modular design**: Parser and backend are intentionally separate. Parser outputs S-Expression AST, backend consumes it.
- **Backend independence**: atom-backend does NOT depend on atom-parser. This allows swapping parser implementations.
- **Compilation pipeline**: `.atom source` → `atom-parser` → `S-Expression AST` → `atom-compile` → `executable`
- **Use atomc script**: For end-to-end compilation, use `./atomc` which orchestrates the full pipeline.
- **IR lowering**: Match statements generate switch IR. In codegen, switches become chained if-else with first case in current block, rest in intermediate blocks.
