use atom_parser::{lexer::Lexer, parser::Parser, print_ast};
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    
    // Parse command-line arguments
    let mut filename = None;
    let mut print_as_sexpr = false;
    
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--ast" => print_as_sexpr = true,
            arg if !arg.starts_with("--") => filename = Some(arg.to_string()),
            _ => {
                eprintln!("Unknown option: {}", args[i]);
                eprintln!("Usage: {} [--ast] <file.atom>", args[0]);
                std::process::exit(1);
            }
        }
        i += 1;
    }
    
    let filename = filename.unwrap_or_else(|| "../examples/fibonacci.atom".to_string());
    
    let code = std::fs::read_to_string(&filename)
        .unwrap_or_else(|e| {
            eprintln!("Failed to read file '{}': {}", filename, e);
            std::process::exit(1);
        });
    
    if !print_as_sexpr {
        println!("=== Lexing ===");
    }
    
    let mut lexer = Lexer::new(&code);
    let tokens = match lexer.tokenize() {
        Ok(tokens) => {
            if !print_as_sexpr {
                println!("Successfully tokenized {} tokens", tokens.len());
            }
            tokens
        }
        Err(e) => {
            eprintln!("Lexer error: {}", e);
            return;
        }
    };
    
    if !print_as_sexpr {
        println!("\n=== Parsing ===");
    }
    
    let mut parser = Parser::new(tokens);
    match parser.parse() {
        Ok(ast) => {
            if print_as_sexpr {
                // Print AST as S-Expression
                println!("{}", print_ast(&ast));
            } else {
                // Print debug representation
                println!("Successfully parsed {} top-level items:", ast.len());
                for (i, item) in ast.iter().enumerate() {
                    println!("{}: {:#?}", i, item);
                }
            }
        }
        Err(e) => {
            eprintln!("Parser error: {}", e);
        }
    }
}
