use atom_parser::{Lexer, Parser};
use atom_ast::{print_ast_with_spans, FromSExpr};
use atom_ast::from_sexpr::SExpr;
use std::fs;
use std::path::{Path, PathBuf};

fn test_span_roundtrip(path: &Path) -> Result<(), String> {
    let source = fs::read_to_string(path)
        .map_err(|e| format!("Failed to read file: {}", e))?;

    // Parse the source code
    let mut lexer = Lexer::new(&source);
    let tokens = lexer.tokenize()
        .map_err(|e| format!("Lexing error: {}", e))?;

    let mut parser = Parser::new(tokens);
    let ast = parser.parse()
        .map_err(|e| format!("Parser error: {}", e))?;

    // Print with spans
    let with_spans = print_ast_with_spans(&ast);
    
    // Verify round-trip with spans
    let sexpr = SExpr::parse(&with_spans)
        .map_err(|e| format!("Error parsing S-expression: {}", e))?;
    
    let ast2 = Vec::from_sexpr(&sexpr)
        .map_err(|e| format!("Error converting from S-expression: {}", e))?;
    
    // Convert back to S-expression with spans and compare
    let with_spans2 = print_ast_with_spans(&ast2);
    
    if with_spans != with_spans2 {
        return Err(format!(
            "Round-trip failed! Spans not preserved.\nOriginal:\n{}\n\nAfter round-trip:\n{}", 
            with_spans, 
            with_spans2
        ));
    }
    
    Ok(())
}

fn discover_atom_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("atom") {
                files.push(path);
            } else if path.is_dir() {
                // Recursively search subdirectories
                files.extend(discover_atom_files(&path));
            }
        }
    }
    
    files.sort();
    files
}

#[test]
fn test_span_roundtrip_all_files() {
    // Auto-discover .atom files from various directories
    let mut test_files = Vec::new();
    
    // Try both relative paths (when run from workspace root and from crate dir)
    let base_paths = vec![".", "../.."];
    
    for base in &base_paths {
        // Collect from examples/
        let examples_dir = Path::new(base).join("examples");
        if examples_dir.exists() {
            test_files.extend(discover_atom_files(&examples_dir));
        }
        
        // Collect from std/
        let std_dir = Path::new(base).join("std");
        if std_dir.exists() {
            test_files.extend(discover_atom_files(&std_dir));
        }
        
        // Collect from tree-sitter-atom/example/
        let tree_sitter_dir = Path::new(base).join("tree-sitter-atom/example");
        if tree_sitter_dir.exists() {
            test_files.extend(discover_atom_files(&tree_sitter_dir));
        }
        
        // If we found files, stop searching
        if !test_files.is_empty() {
            break;
        }
    }
    
    assert!(!test_files.is_empty(), "No .atom files found!");
    
    println!("Testing span round-trip for {} .atom files\n", test_files.len());
    
    let mut failed_files = Vec::new();
    
    for file in &test_files {
        let display_path = file.to_string_lossy();
        print!("Testing {:<60} ... ", display_path);
        match test_span_roundtrip(file) {
            Ok(_) => {
                println!("✓ PASSED");
            }
            Err(e) => {
                println!("✗ FAILED");
                println!("  Error: {}", e);
                failed_files.push((file.clone(), e));
            }
        }
    }
    
    println!("\n========================================");
    println!("Results: {} passed, {} failed out of {} total", 
             test_files.len() - failed_files.len(), 
             failed_files.len(), 
             test_files.len());
    println!("========================================");
    
    if !failed_files.is_empty() {
        let mut error_msg = format!("Span round-trip failed for {} files:\n", failed_files.len());
        for (file, err) in &failed_files {
            error_msg.push_str(&format!("  - {}: {}\n", file.display(), err));
        }
        panic!("{}", error_msg);
    }
}