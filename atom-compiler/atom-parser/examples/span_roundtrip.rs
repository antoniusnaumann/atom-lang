use atom_parser::{Lexer, Parser};
use atom_ast::{print_ast, print_ast_with_spans, FromSExpr};
use atom_ast::from_sexpr::SExpr;
use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() != 2 {
        eprintln!("Usage: {} <file.atom>", args[0]);
        process::exit(1);
    }

    let filename = &args[1];
    
    let source = match fs::read_to_string(filename) {
        Ok(content) => content,
        Err(err) => {
            eprintln!("Error reading file '{}': {}", filename, err);
            process::exit(1);
        }
    };

    // Parse the source code
    let mut lexer = Lexer::new(&source);
    let tokens = match lexer.tokenize() {
        Ok(tokens) => tokens,
        Err(err) => {
            eprintln!("Lexing error: {:?}", err);
            process::exit(1);
        }
    };

    let mut parser = Parser::new(tokens);
    let ast = match parser.parse() {
        Ok(ast) => ast,
        Err(err) => {
            eprintln!("Parse error: {:?}", err);
            process::exit(1);
        }
    };

    // Print without spans
    println!("=== AST without spans ===\n");
    println!("{}", print_ast(&ast));
    
    // Print with spans
    println!("\n=== AST with spans ===\n");
    let with_spans = print_ast_with_spans(&ast);
    println!("{}", with_spans);
    
    // Verify round-trip with spans
    println!("\n=== Verifying span round-trip ===\n");
    let sexpr = match SExpr::parse(&with_spans) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error parsing S-expression: {:?}", e);
            process::exit(1);
        }
    };
    
    let ast2 = match Vec::from_sexpr(&sexpr) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Error converting from S-expression: {:?}", e);
            process::exit(1);
        }
    };
    
    // Convert back to S-expression with spans and compare
    let with_spans2 = print_ast_with_spans(&ast2);
    
    if with_spans == with_spans2 {
        println!("✓ Round-trip successful! Spans preserved correctly.");
    } else {
        println!("✗ Round-trip failed! Spans not preserved.");
        println!("\nOriginal:\n{}", with_spans);
        println!("\nAfter round-trip:\n{}", with_spans2);
        process::exit(1);
    }
}
