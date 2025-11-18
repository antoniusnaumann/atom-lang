use atom_parser::{Lexer, Parser};
use atom_ast::{print_ast_with_spans, FromSExpr};
use atom_ast::from_sexpr::SExpr;
use std::fs;
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: test_roundtrip <file.atom>");
        std::process::exit(1);
    }
    
    let path = &args[1];
    let source = fs::read_to_string(path).unwrap();
    let mut lexer = Lexer::new(&source);
    let tokens = lexer.tokenize().unwrap();
    let mut parser = Parser::new(tokens);
    let ast = parser.parse().unwrap();
    
    let with_spans = print_ast_with_spans(&ast);
    let sexpr = SExpr::parse(&with_spans).unwrap();
    let ast2 = Vec::from_sexpr(&sexpr).unwrap();
    let with_spans2 = print_ast_with_spans(&ast2);
    
    fs::write("/tmp/original.txt", &with_spans).unwrap();
    fs::write("/tmp/roundtrip.txt", &with_spans2).unwrap();
    
    if with_spans == with_spans2 {
        println!("SUCCESS: Roundtrip preserves spans!");
    } else {
        println!("FAILED: Spans not preserved");
        println!("Run: diff /tmp/original.txt /tmp/roundtrip.txt");
    }
}
