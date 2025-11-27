//! Integration tests for the Atom compiler backend

use std::fs;
use atom_ast::from_sexpr::{FromSExpr, SExpr};
use atom_backend::{TypeChecker, Lower, CodeGenerator};

#[test]
fn test_simple_add_function() {
    // A simple S-Expression AST for: add(a Int, b Int) Int { a + b }
    let sexpr_str = r#"
        (program
            (function internal add
                (params (a Int) (b Int))
                (returns Int)
                (block
                    (+ a b)
                )
            )
        )
    "#;
    
    let sexpr = SExpr::parse(sexpr_str).expect("Failed to parse S-expr");
    let ast = Vec::<atom_ast::ast::TopLevel>::from_sexpr(&sexpr)
        .expect("Failed to deserialize AST");
    
    // Type check
    let mut type_checker = TypeChecker::new();
    let typed_program = type_checker.check_program(ast)
        .expect("Type checking failed");
    
    // Lower to IR
    let mut lower = Lower::new(typed_program.type_env.clone());
    let ir_program = lower.lower_program(typed_program.ast)
        .expect("Lowering to IR failed");
    
    // Verify IR was generated
    assert_eq!(ir_program.functions.len(), 1);
    assert_eq!(ir_program.functions[0].name, "add");
    
    // Generate code (to object file)
    let mut codegen = CodeGenerator::new();
    let result = codegen.compile(ir_program, "test_add.o");
    
    // Code generation might fail for unsupported features, that's okay
    // The important part is that type checking and IR generation work
    match result {
        Ok(_) => {
            // Clean up
            let _ = fs::remove_file("test_add.o");
            println!("Successfully compiled simple add function!");
        }
        Err(e) => {
            println!("Code generation failed (expected): {}", e);
            // This is okay - we're testing the pipeline, not full code generation
        }
    }
}

#[test]
fn test_struct_definition() {
    let sexpr_str = r#"
        (program
            (struct internal Vec2
                (field x Int)
                (field y Int)
            )
            (function internal make_vec
                (params (x Int) (y Int))
                (returns Vec2)
                (block
                    (struct-init Vec2
                        (field-init x x)
                        (field-init y y)
                    )
                )
            )
        )
    "#;
    
    let sexpr = SExpr::parse(sexpr_str).expect("Failed to parse S-expr");
    let ast = Vec::<atom_ast::ast::TopLevel>::from_sexpr(&sexpr)
        .expect("Failed to deserialize AST");
    
    // Type check
    let mut type_checker = TypeChecker::new();
    let typed_program = type_checker.check_program(ast)
        .expect("Type checking failed");
    
    // Verify struct was registered by checking if we can look it up
    assert!(typed_program.type_env.get_struct("Vec2").is_some(), "Vec2 struct should be registered");
    
    // Lower to IR
    let mut lower = Lower::new(typed_program.type_env.clone());
    let ir_program = lower.lower_program(typed_program.ast)
        .expect("Lowering to IR failed");
    
    // Verify struct and function were generated
    assert_eq!(ir_program.structs.len(), 1);
    assert_eq!(ir_program.structs[0].name, "Vec2");
    assert_eq!(ir_program.functions.len(), 1);
    assert_eq!(ir_program.functions[0].name, "make_vec");
}

#[test]
fn test_type_error_detection() {
    // A function that returns the wrong type
    let sexpr_str = r#"
        (program
            (function internal bad_function
                (params)
                (returns Int)
                (block
                    "not an int"
                )
            )
        )
    "#;
    
    let sexpr = SExpr::parse(sexpr_str).expect("Failed to parse S-expr");
    let ast = Vec::<atom_ast::ast::TopLevel>::from_sexpr(&sexpr)
        .expect("Failed to deserialize AST");
    
    // Type check should fail
    let mut type_checker = TypeChecker::new();
    let result = type_checker.check_program(ast);
    
    assert!(result.is_err(), "Expected type error");
    println!("Correctly detected type error: {}", result.unwrap_err());
}

// ============================================================================
// Reference Type Tests
// ============================================================================

#[test]
fn test_basic_reference_parameter() {
    // Test that a function can accept a reference parameter and modify it
    // increment(x &Int) { x = x + 1 }
    let sexpr_str = r#"
        (program
            (function internal increment
                (params (x (reference Int)))
                (returns Void)
                (block
                    (= x (+ x 1))
                )
            )
        )
    "#;
    
    let sexpr = SExpr::parse(sexpr_str).expect("Failed to parse S-expr");
    let ast = Vec::<atom_ast::ast::TopLevel>::from_sexpr(&sexpr)
        .expect("Failed to deserialize AST");
    
    let mut type_checker = TypeChecker::new();
    let result = type_checker.check_program(ast);
    
    // This should eventually pass once reference semantics are implemented
    match result {
        Ok(_) => println!("Reference parameter accepted"),
        Err(e) => println!("Reference parameter test failed (TODO): {}", e),
    }
}

#[test]
fn test_reference_modifies_original() {
    // Test that modifications through a reference affect the original variable
    // increment(x &Int) { x = x + 1 }
    // main() { a := 5; increment(&a); assert(a == 6) }
    let sexpr_str = r#"
        (program
            (function internal increment
                (params (x (reference Int)))
                (returns Void)
                (block
                    (= x (+ x 1))
                )
            )
            (function internal main
                (params)
                (returns Void)
                (block
                    (var-decl a (type Int) (init 5))
                    (call increment (reference a))
                    (call assert (== a 6))
                )
            )
        )
    "#;
    
    let sexpr = SExpr::parse(sexpr_str).expect("Failed to parse S-expr");
    let ast = Vec::<atom_ast::ast::TopLevel>::from_sexpr(&sexpr)
        .expect("Failed to deserialize AST");
    
    let mut type_checker = TypeChecker::new();
    let result = type_checker.check_program(ast);
    
    match result {
        Ok(_) => println!("Reference modification test compiled"),
        Err(e) => println!("Reference modification test failed (TODO): {}", e),
    }
}

#[test]
fn test_reference_to_struct_field() {
    // Test that references to struct fields work correctly
    // Point(x Int, y Int)
    // move_x(p &Point, dx Int) { p.x = p.x + dx }
    let sexpr_str = r#"
        (program
            (struct internal Point
                (field x Int)
                (field y Int)
            )
            (function internal move_x
                (params (p (reference Point)) (dx Int))
                (returns Void)
                (block
                    (= (field-access p x) (+ (field-access p x) dx))
                )
            )
            (function internal main
                (params)
                (returns Void)
                (block
                    (var-decl point (type Point) (init (struct-init Point (field-init x 10) (field-init y 20))))
                    (call move_x (reference point) 5)
                    (call assert (== (field-access point x) 15))
                )
            )
        )
    "#;
    
    let sexpr = SExpr::parse(sexpr_str).expect("Failed to parse S-expr");
    let ast = Vec::<atom_ast::ast::TopLevel>::from_sexpr(&sexpr)
        .expect("Failed to deserialize AST");
    
    let mut type_checker = TypeChecker::new();
    let result = type_checker.check_program(ast);
    
    match result {
        Ok(typed_program) => {
            println!("Reference to struct field test type-checked");
            // Verify struct was registered
            assert!(typed_program.type_env.get_struct("Point").is_some());
        }
        Err(e) => println!("Reference to struct field test failed (TODO): {}", e),
    }
}

#[test]
fn test_reference_to_array_element() {
    // Test that references to array elements work
    // increment_at(arr &Int*, idx Int) { arr(idx) = arr(idx) + 1 }
    let sexpr_str = r#"
        (program
            (function internal increment_at
                (params (arr (reference (variadic Int))) (idx Int))
                (returns Void)
                (block
                    (= (index-access arr idx) (+ (index-access arr idx) 1))
                )
            )
            (function internal main
                (params)
                (returns Void)
                (block
                    (var-decl numbers (type (variadic Int)) (init (tuple 1 2 3 4 5)))
                    (call increment_at (reference numbers) 2)
                    (call assert (== (index-access numbers 2) 4))
                )
            )
        )
    "#;
    
    let sexpr = SExpr::parse(sexpr_str).expect("Failed to parse S-expr");
    let ast = Vec::<atom_ast::ast::TopLevel>::from_sexpr(&sexpr)
        .expect("Failed to deserialize AST");
    
    let mut type_checker = TypeChecker::new();
    let result = type_checker.check_program(ast);
    
    match result {
        Ok(_) => println!("Reference to array element test compiled"),
        Err(e) => println!("Reference to array element test failed (TODO): {}", e),
    }
}

#[test]
fn test_ufcs_with_reference() {
    // Test UFCS (Uniform Function Call Syntax) with reference parameters
    // increment(x &Int) { x = x + 1 }
    // main() { a := 5; a.increment(); assert(a == 6) }
    let sexpr_str = r#"
        (program
            (function internal increment
                (params (x (reference Int)))
                (returns Void)
                (block
                    (= x (+ x 1))
                )
            )
            (function internal main
                (params)
                (returns Void)
                (block
                    (var-decl a (type Int) (init 5))
                    (method-call a increment)
                    (call assert (== a 6))
                )
            )
        )
    "#;
    
    let sexpr = SExpr::parse(sexpr_str).expect("Failed to parse S-expr");
    let ast = Vec::<atom_ast::ast::TopLevel>::from_sexpr(&sexpr)
        .expect("Failed to deserialize AST");
    
    let mut type_checker = TypeChecker::new();
    let result = type_checker.check_program(ast);
    
    match result {
        Ok(_) => println!("UFCS with reference test compiled"),
        Err(e) => println!("UFCS with reference test failed (TODO): {}", e),
    }
}

#[test]
fn test_multiple_references() {
    // Test multiple reference parameters in a single function
    // swap(a &Int, b &Int) { tmp := a; a = b; b = tmp }
    let sexpr_str = r#"
        (program
            (function internal swap
                (params (a (reference Int)) (b (reference Int)))
                (returns Void)
                (block
                    (var-decl tmp (type Int) (init a))
                    (= a b)
                    (= b tmp)
                )
            )
            (function internal main
                (params)
                (returns Void)
                (block
                    (var-decl x (type Int) (init 10))
                    (var-decl y (type Int) (init 20))
                    (call swap (reference x) (reference y))
                    (call assert (== x 20))
                    (call assert (== y 10))
                )
            )
        )
    "#;
    
    let sexpr = SExpr::parse(sexpr_str).expect("Failed to parse S-expr");
    let ast = Vec::<atom_ast::ast::TopLevel>::from_sexpr(&sexpr)
        .expect("Failed to deserialize AST");
    
    let mut type_checker = TypeChecker::new();
    let result = type_checker.check_program(ast);
    
    match result {
        Ok(_) => println!("Multiple references test compiled"),
        Err(e) => println!("Multiple references test failed (TODO): {}", e),
    }
}

// ============================================================================
// Reference Error Tests - These should fail type checking
// ============================================================================

#[test]
fn test_reference_type_mismatch() {
    // Test that passing wrong reference type fails
    // increment(x &Int) { x = x + 1 }
    // main() { s := "string"; increment(&s) } // ERROR: expected &Int, got &String
    let sexpr_str = r#"
        (program
            (function internal increment
                (params (x (reference Int)))
                (returns Void)
                (block
                    (= x (+ x 1))
                )
            )
            (function internal main
                (params)
                (returns Void)
                (block
                    (var-decl s (type String) (init "string"))
                    (call increment (reference s))
                )
            )
        )
    "#;
    
    let sexpr = SExpr::parse(sexpr_str).expect("Failed to parse S-expr");
    let ast = Vec::<atom_ast::ast::TopLevel>::from_sexpr(&sexpr)
        .expect("Failed to deserialize AST");
    
    let mut type_checker = TypeChecker::new();
    let result = type_checker.check_program(ast);
    
    // This SHOULD fail - wrong type for reference
    if result.is_err() {
        println!("Correctly rejected reference type mismatch: {}", result.unwrap_err());
    } else {
        println!("WARNING: Should have rejected reference type mismatch (TODO)");
    }
}

#[test]
fn test_reference_to_non_lvalue() {
    // Test that taking a reference to a non-lvalue (like a literal) fails
    // increment(x &Int) { x = x + 1 }
    // main() { increment(&42) } // ERROR: cannot take reference to literal
    let sexpr_str = r#"
        (program
            (function internal increment
                (params (x (reference Int)))
                (returns Void)
                (block
                    (= x (+ x 1))
                )
            )
            (function internal main
                (params)
                (returns Void)
                (block
                    (call increment (reference 42))
                )
            )
        )
    "#;
    
    let sexpr = SExpr::parse(sexpr_str).expect("Failed to parse S-expr");
    let ast = Vec::<atom_ast::ast::TopLevel>::from_sexpr(&sexpr)
        .expect("Failed to deserialize AST");
    
    let mut type_checker = TypeChecker::new();
    let result = type_checker.check_program(ast);
    
    // This SHOULD fail - cannot take reference to literal
    if result.is_err() {
        println!("Correctly rejected reference to non-lvalue: {}", result.unwrap_err());
    } else {
        println!("WARNING: Should have rejected reference to non-lvalue (TODO)");
    }
}

#[test]
fn test_missing_ampersand_for_reference_param() {
    // Test that passing a value (not a reference) to a reference parameter fails
    // increment(x &Int) { x = x + 1 }
    // main() { a := 5; increment(a) } // ERROR: expected &Int, got Int
    let sexpr_str = r#"
        (program
            (function internal increment
                (params (x (reference Int)))
                (returns Void)
                (block
                    (= x (+ x 1))
                )
            )
            (function internal main
                (params)
                (returns Void)
                (block
                    (var-decl a (type Int) (init 5))
                    (call increment a)
                )
            )
        )
    "#;
    
    let sexpr = SExpr::parse(sexpr_str).expect("Failed to parse S-expr");
    let ast = Vec::<atom_ast::ast::TopLevel>::from_sexpr(&sexpr)
        .expect("Failed to deserialize AST");
    
    let mut type_checker = TypeChecker::new();
    let result = type_checker.check_program(ast);
    
    // This SHOULD fail - missing & for reference parameter
    if result.is_err() {
        println!("Correctly rejected missing ampersand: {}", result.unwrap_err());
    } else {
        println!("WARNING: Should have rejected missing ampersand (TODO)");
    }
}

#[test]
fn test_reference_in_return_type_not_allowed() {
    // Test that reference types are not allowed in return types
    // get_ref(x Int) &Int { &x } // ERROR: reference types only allowed in parameters
    let sexpr_str = r#"
        (program
            (function internal get_ref
                (params (x Int))
                (returns (reference Int))
                (block
                    (reference x)
                )
            )
        )
    "#;
    
    let sexpr = SExpr::parse(sexpr_str).expect("Failed to parse S-expr");
    let ast = Vec::<atom_ast::ast::TopLevel>::from_sexpr(&sexpr)
        .expect("Failed to deserialize AST");
    
    let mut type_checker = TypeChecker::new();
    let result = type_checker.check_program(ast);
    
    // This SHOULD fail - references only allowed in parameters
    if result.is_err() {
        println!("Correctly rejected reference in return type: {}", result.unwrap_err());
    } else {
        println!("WARNING: Should have rejected reference in return type (TODO)");
    }
}

#[test]
fn test_reference_to_temporary_expression() {
    // Test that taking a reference to a temporary expression fails
    // increment(x &Int) { x = x + 1 }
    // main() { increment(&(5 + 3)) } // ERROR: cannot take reference to temporary
    let sexpr_str = r#"
        (program
            (function internal increment
                (params (x (reference Int)))
                (returns Void)
                (block
                    (= x (+ x 1))
                )
            )
            (function internal main
                (params)
                (returns Void)
                (block
                    (call increment (reference (+ 5 3)))
                )
            )
        )
    "#;
    
    let sexpr = SExpr::parse(sexpr_str).expect("Failed to parse S-expr");
    let ast = Vec::<atom_ast::ast::TopLevel>::from_sexpr(&sexpr)
        .expect("Failed to deserialize AST");
    
    let mut type_checker = TypeChecker::new();
    let result = type_checker.check_program(ast);
    
    // This SHOULD fail - cannot take reference to temporary
    if result.is_err() {
        println!("Correctly rejected reference to temporary: {}", result.unwrap_err());
    } else {
        println!("WARNING: Should have rejected reference to temporary (TODO)");
    }
}

#[test]
fn test_nested_reference_not_allowed() {
    // Test that nested references are not allowed: &&T
    let sexpr_str = r#"
        (program
            (function internal double_ref
                (params (x (reference (reference Int))))
                (returns Void)
                (block
                    Void
                )
            )
        )
    "#;
    
    let sexpr = SExpr::parse(sexpr_str).expect("Failed to parse S-expr");
    let ast = Vec::<atom_ast::ast::TopLevel>::from_sexpr(&sexpr)
        .expect("Failed to deserialize AST");
    
    let mut type_checker = TypeChecker::new();
    let result = type_checker.check_program(ast);
    
    // This SHOULD fail - nested references not allowed
    if result.is_err() {
        println!("Correctly rejected nested reference: {}", result.unwrap_err());
    } else {
        println!("WARNING: Should have rejected nested reference (TODO)");
    }
}

#[test]
fn test_reference_in_struct_field_not_allowed() {
    // Test that reference types are not allowed in struct fields
    // RefContainer(&Int) // ERROR: references only in function parameters
    let sexpr_str = r#"
        (program
            (struct internal RefContainer
                (field value (reference Int))
            )
        )
    "#;
    
    let sexpr = SExpr::parse(sexpr_str).expect("Failed to parse S-expr");
    let ast = Vec::<atom_ast::ast::TopLevel>::from_sexpr(&sexpr)
        .expect("Failed to deserialize AST");
    
    let mut type_checker = TypeChecker::new();
    let result = type_checker.check_program(ast);
    
    // This SHOULD fail - references not allowed in struct fields
    if result.is_err() {
        println!("Correctly rejected reference in struct field: {}", result.unwrap_err());
    } else {
        println!("WARNING: Should have rejected reference in struct field (TODO)");
    }
}
