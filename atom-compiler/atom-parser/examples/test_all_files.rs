use atom_parser::{lexer::Lexer, parser::Parser};
use std::fs;
use std::path::{Path, PathBuf};

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

fn main() {
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
    
    if test_files.is_empty() {
        eprintln!("No .atom files found!");
        std::process::exit(1);
    }
    
    println!("Found {} .atom files to test\n", test_files.len());
    
    let mut passed = 0;
    let mut failed = 0;
    
    for file in &test_files {
        let display_path = file.to_string_lossy();
        print!("Testing {:<60} ... ", display_path);
        match test_file(file) {
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
