use atom_parser::lexer::Lexer;

fn main() {
    let code = std::fs::read_to_string("../examples/fibonacci.atom").unwrap();
    let mut lexer = Lexer::new(&code);
    
    match lexer.tokenize() {
        Ok(tokens) => {
            println!("Successfully tokenized {} tokens:", tokens.len());
            for (i, token) in tokens.iter().take(20).enumerate() {
                println!("{}: {:?} '{}'", i, token.kind, token.text);
            }
        }
        Err(e) => {
            eprintln!("Lexer error: {}", e);
        }
    }
}
