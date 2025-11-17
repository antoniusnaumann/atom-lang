use atom_parser::{lexer::Lexer, parser::Parser};

fn main() {
    let code = std::fs::read_to_string("../examples/fibonacci.atom")
        .expect("Failed to read file");
    
    println!("=== Lexing ===");
    let mut lexer = Lexer::new(&code);
    let tokens = match lexer.tokenize() {
        Ok(tokens) => {
            println!("Successfully tokenized {} tokens", tokens.len());
            tokens
        }
        Err(e) => {
            eprintln!("Lexer error: {}", e);
            return;
        }
    };
    
    println!("\n=== Parsing ===");
    let mut parser = Parser::new(tokens);
    match parser.parse() {
        Ok(ast) => {
            println!("Successfully parsed {} top-level items:", ast.len());
            for (i, item) in ast.iter().enumerate() {
                println!("{}: {:#?}", i, item);
            }
        }
        Err(e) => {
            eprintln!("Parser error: {}", e);
        }
    }
}
