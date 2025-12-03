use atom_parser::{lexer::Lexer, parser::Parser, print_ast};
use std::env;
use std::fs;
use std::process;

fn print_usage(program_name: &str) {
    eprintln!("Usage: {} [OPTIONS] <file.atom>", program_name);
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --ast            Print AST as S-Expression");
    eprintln!("  --module <name>  Specify module/namespace for this file");
    eprintln!("  --help           Show this help message");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  {}  example.atom                    # Parse and show debug output", program_name);
    eprintln!("  {}  --ast example.atom              # Parse and show S-Expression AST", program_name);
    eprintln!("  {}  --ast --module matrix lib.atom  # Parse with module name 'matrix'", program_name);
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let program_name = args.first().map(|s| s.as_str()).unwrap_or("atom");

    // Parse command-line arguments
    let mut filename: Option<String> = None;
    let mut print_as_sexpr = false;
    let mut module_name: Option<String> = None;
    
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--ast" => print_as_sexpr = true,
            "--module" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: --module requires an argument");
                    eprintln!();
                    print_usage(program_name);
                    process::exit(1);
                }
                module_name = Some(args[i + 1].clone());
                i += 1; // Skip the next argument
            }
            "--help" | "-h" => {
                print_usage(program_name);
                process::exit(0);
            }
            arg if arg.starts_with("--") => {
                eprintln!("Error: Unknown option: {}", arg);
                eprintln!();
                print_usage(program_name);
                process::exit(1);
            }
            arg => {
                if filename.is_some() {
                    eprintln!("Error: Multiple input files specified");
                    eprintln!();
                    print_usage(program_name);
                    process::exit(1);
                }
                filename = Some(arg.to_string());
            }
        }
        i += 1;
    }

    // Ensure we have a filename
    let filename = match filename {
        Some(f) => f,
        None => {
            eprintln!("Error: No input file specified");
            eprintln!();
            print_usage(program_name);
            process::exit(1);
        }
    };

    // Read source file
    let source = match fs::read_to_string(&filename) {
        Ok(content) => content,
        Err(err) => {
            eprintln!("Error reading file '{}': {}", filename, err);
            process::exit(1);
        }
    };

    // Lex the source code
    if !print_as_sexpr {
        println!("=== Lexing {} ===", filename);
    }

    let mut lexer = Lexer::new(&source);
    let tokens = match lexer.tokenize() {
        Ok(tokens) => {
            if !print_as_sexpr {
                println!("Successfully tokenized {} tokens", tokens.len());
            }
            tokens
        }
        Err(err) => {
            eprintln!("Lexer error in '{}': {}", filename, err);
            process::exit(1);
        }
    };

    // Parse the tokens
    if !print_as_sexpr {
        println!("\n=== Parsing ===");
    }

    let mut parser = Parser::new_with_filename(tokens, filename.clone());
    let ast = match parser.parse() {
        Ok(ast) => ast,
        Err(err) => {
            eprintln!("Parser error in '{}': {}", filename, err);
            process::exit(1);
        }
    };

    // Output the result
    if print_as_sexpr {
        // Print AST as S-Expression with optional module metadata
        use atom_ast::sexpr::print_ast_with_module;
        if let Some(module) = module_name {
            println!("{}", print_ast_with_module(&ast, &module));
        } else {
            println!("{}", print_ast(&ast));
        }
    } else {
        // Print debug representation
        println!("Successfully parsed {} top-level items", ast.len());
        if let Some(module) = &module_name {
            println!("Module: {}", module);
        }
        println!();
        for (i, item) in ast.iter().enumerate() {
            println!("=== Item {} ===", i);
            println!("{:#?}", item);
            println!();
        }
    }
}
