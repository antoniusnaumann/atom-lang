# Atom Compiler

This is a Rust workspace containing the compiler infrastructure for the Atom programming language.

## Structure

The workspace is organized into the following crates:

### atom-ast
Contains the Abstract Syntax Tree (AST) definitions and S-expression support:
- AST node definitions for all Atom language constructs
- S-expression serialization (AST → S-expression)
- S-expression deserialization (S-expression → AST)
- Span tracking for source locations

### atom-parser
Contains the lexer and parser for Atom source code:
- Lexer: Tokenizes Atom source code
- Parser: Builds AST from tokens
- Binary executable: `atom` command-line tool
- Examples for parsing and printing ASTs

## Building

From the workspace root:
```bash
cargo build
```

## Testing

Run all tests:
```bash
cargo test
```

Test parsing all .atom files in the repository:
```bash
cargo run --example test_all_files
```

## Usage

Parse and print an Atom file as an S-expression:
```bash
cargo run --example print_ast examples/fibonacci.atom
```

Use the `atom` binary:
```bash
cargo run --bin atom -- <file.atom>
```

## S-Expression Round-Trip

The AST supports full round-trip conversion to/from S-expressions:
1. Parse Atom source → AST
2. Convert AST → S-expression string
3. Parse S-expression string → AST
4. Both ASTs should be identical

This is useful for:
- Debugging and testing
- External tools that want to work with Atom ASTs
- AST transformations and macros
