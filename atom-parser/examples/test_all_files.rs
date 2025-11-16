use atom_parser::{lexer::Lexer, parser::Parser};
use std::fs;
use std::path::Path;

fn test_file(path: &Path) -> Result<(), String> {
    let code = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;
    
    let mut lexer = Lexer::new(&code);
    let tokens = lexer.tokenize()
        .map_err(|e| format!("Lexer error: {}", e))?;
    
    let mut parser = Parser::new(tokens);
    parser.parse()
        .map_err(|e| format!("Parser error: {}", e))?;
    
    Ok(())
}

fn main() {
    let test_files = vec![
        // Examples
        "../examples/fibonacci.atom",
        "../examples/array_demo.atom",
        "../examples/std_demo.atom",
        // Standard library
        "../std/src/array.atom",
        "../std/src/io.atom",
        "../std/src/math.atom",
        "../std/src/process.atom",
        "../std/src/result.atom",
        "../std/src/string.atom",
        // Tree-sitter examples
        "../tree-sitter-atom/example/test_cases.atom",
        "../tree-sitter-atom/example/test_closure_edge_cases.atom",
        "../tree-sitter-atom/example/test_closure_no_backslash.atom",
        "../tree-sitter-atom/example/test_highlight.atom",
        "../tree-sitter-atom/example/test_minimal.atom",
        "../tree-sitter-atom/example/test_tuples_complete.atom",
    ];
    
    let mut passed = 0;
    let mut failed = 0;
    
    for file in &test_files {
        print!("Testing {:<50} ... ", file);
        match test_file(Path::new(file)) {
            Ok(_) => {
                println!("✓ PASSED");
                passed += 1;
            }
            Err(e) => {
                println!("✗ FAILED");
                println!("  Error: {}", e);
                failed += 1;
            }
        }
    }
    
    println!("\n========================================");
    println!("Results: {} passed, {} failed out of {} total", passed, failed, test_files.len());
    println!("========================================");
    
    if failed > 0 {
        std::process::exit(1);
    }
}
