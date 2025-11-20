//! Atom compiler backend CLI tool
//!
//! Usage: atom-compile [OPTIONS] <input-files>...
//!
//! Compiles Atom S-Expression AST files to native executables.
//!
//! NOTE: This backend intentionally does NOT invoke the parser directly.
//! It only accepts pre-parsed S-Expression AST files as input.
//! This allows the parser to be swapped out with alternative implementations.
//! The `atomc` script handles parsing .atom source files to S-expressions.

use std::env;
use std::fs;
use std::process::{self, Command};

use atom_ast::from_sexpr::{FromSExpr, SExpr};

mod codegen;
mod ir;
mod lower;
mod types;
mod typechecker;

use codegen::CodeGenerator;
use lower::Lower;
use typechecker::TypeChecker;

fn main() {
    let args: Vec<String> = env::args().collect();
    
    if args.len() < 2 {
        eprintln!("Usage: atom-compile <input.sexpr>... [-o output] [--debug]");
        eprintln!();
        eprintln!("Compiles Atom S-Expression AST files to native code.");
        eprintln!();
        eprintln!("Arguments:");
        eprintln!("  <input.sexpr>...    One or more S-Expression AST files");
        eprintln!("  -o <output>         Output executable path (default: a.out)");
        eprintln!("  --debug             Enable debug output during compilation");
        eprintln!();
        eprintln!("Note: Use the `atomc` script to compile .atom source files directly.");
        process::exit(1);
    }
    
    // Parse arguments
    let mut input_files = Vec::new();
    let mut output_file = "a.out".to_string();
    let mut debug = false;
    let mut i = 1;
    
    while i < args.len() {
        if args[i] == "-o" {
            if i + 1 >= args.len() {
                eprintln!("Error: -o requires an argument");
                process::exit(1);
            }
            output_file = args[i + 1].clone();
            i += 2;
        } else if args[i] == "--debug" {
            debug = true;
            i += 1;
        } else {
            input_files.push(args[i].clone());
            i += 1;
        }
    }
    
    if input_files.is_empty() {
        eprintln!("Error: No input files specified");
        process::exit(1);
    }
    
    // Set debug environment variable for the backend
    if debug {
        unsafe {
            env::set_var("ATOM_DEBUG", "1");
        }
    }
    
    // Compile
    if let Err(e) = compile(&input_files, &output_file) {
        eprintln!("Compilation failed: {}", e);
        process::exit(1);
    }
    
    println!("Successfully compiled to {}", output_file);
}

fn compile(input_files: &[String], output_file: &str) -> Result<(), String> {
    // Step 1: Parse all S-Expression input files
    println!("Parsing {} S-Expression file(s)...", input_files.len());
    let mut all_items = Vec::new();
    
    for file_path in input_files {
        let content = fs::read_to_string(file_path)
            .map_err(|e| format!("Failed to read {}: {}", file_path, e))?;
        
        let sexpr = SExpr::parse(&content)
            .map_err(|e| format!("Failed to parse S-expression in {}: {}", file_path, e))?;
        
        let items = Vec::<atom_ast::ast::TopLevel>::from_sexpr(&sexpr)
            .map_err(|e| format!("Failed to deserialize AST from {}: {}", file_path, e))?;
        
        all_items.extend(items);
    }
    
    println!("Parsed {} top-level items", all_items.len());
    
    // Step 2: Type check
    println!("Type checking...");
    let mut type_checker = TypeChecker::new();
    let typed_program = type_checker.check_program(all_items)
        .map_err(|e| format!("Type error: {}", e))?;
    
    println!("Type checking complete");
    
    // Step 3: Lower to IR
    println!("Lowering to IR...");
    let mut lower = Lower::new_with_sigs(typed_program.type_env.clone(), typed_program.functions.clone());
    let ir_program = lower.lower_program(typed_program.ast)
        .map_err(|e| format!("Lowering error: {}", e))?;
    
    println!("Lowered to {} functions", ir_program.functions.len());
    
    // Step 4: Generate code
    println!("Generating code...");
    let mut codegen = CodeGenerator::new();
    
    // Generate an object file
    let obj_file = format!("{}.o", output_file);
    codegen.compile(ir_program, &obj_file)
        .map_err(|e| format!("Code generation error: {}", e))?;
    
    println!("Generated object file: {}", obj_file);
    
    // Step 5: Link with C standard library
    println!("Linking...");
    link_object_file(&obj_file, output_file)?;
    
    // Clean up object file
    let _ = fs::remove_file(&obj_file);
    
    Ok(())
}

fn link_object_file(obj_file: &str, output_file: &str) -> Result<(), String> {
    // Invoke the system linker (cc) to create the final executable
    let linker = env::var("CC").unwrap_or_else(|_| "cc".to_string());
    
    let output = Command::new(&linker)
        .arg(obj_file)
        .arg("-o")
        .arg(output_file)
        .output()
        .map_err(|e| format!("Failed to invoke linker '{}': {}", linker, e))?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Linker failed:\n{}", stderr));
    }
    
    // Print linker warnings if any
    if !output.stderr.is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("{}", stderr);
    }
    
    Ok(())
}
