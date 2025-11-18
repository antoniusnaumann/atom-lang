# Agent Guidelines for Atom Language Development

## Build & Test Commands
- **Build**: `cd atom-compiler && cargo build --workspace`
- **All tests**: `cd atom-compiler && cargo test --workspace --verbose`
- **Single test**: `cd atom-compiler && cargo test <TESTNAME> --verbose` (e.g., `cargo test parse_all_files`)
- **Lint**: `cd atom-compiler && cargo clippy --workspace -- -D warnings`
- **Tree-sitter build**: `cd tree-sitter-atom && tree-sitter generate`
- **Tree-sitter test**: `tree-sitter parse <file.atom> --quiet --stat` (from tree-sitter-atom dir)

## Code Style (Rust)
- **Edition**: 2024, workspace members in `atom-compiler/`
- **Imports**: Group by `crate::`, external crates, then `atom_ast::{self, ...}`. Use relative paths for internal modules.
- **Formatting**: Standard rustfmt (no custom config). Prefer 4-space indentation.
- **Types**: Explicit type annotations for struct fields and function params. Use `Box<T>` for recursive types.
- **Naming**: `snake_case` for functions/vars/modules, `PascalCase` for types/enums, `SCREAMING_SNAKE_CASE` for consts.
- **Error handling**: Return `ParseResult<T>` (alias for `Result<T, ParseError>`). Use `?` operator for propagation.
- **Patterns**: Prefer pattern matching over conditionals. Use `Option` and `Result` explicitly, avoid unwrap in lib code.
- **Comments**: Use doc comments `///` for public items. Inline comments for complex logic only.

## Atom Language Rules
- **Casing**: `PascalCase` for types/enum cases, `snake_case` for functions/vars/fields/modules. No keywords—structural typing.
- **Visibility**: `+` (public), `-` (file-private), none (package-internal). Prefix types and functions.
- **Specification**: Always reference the README.md and the .atom files in std/src/ and examples/ when working on the compiler
