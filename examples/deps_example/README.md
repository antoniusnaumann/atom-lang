# Dependencies Example

A **working** example demonstrating Atom's module system with external dependencies in the `deps/` directory.

## Project Structure

```
deps_example/
├── src/
│   └── main.atom           # Main program that imports from deps/
└── deps/
    └── mathlib/
        └── src/
            └── operations.atom  # Math library with exported functions
```

## Key Features Demonstrated

### 1. Module System with Dependencies
- **deps/ directory**: External packages are placed in `deps/`
- **Module namespaces**: Each subdirectory in `deps/` becomes a module namespace
- **Import syntax**: `mathlib::*` imports all public items from the mathlib module

### 2. Visibility Modifiers
- **Public exports** (`+`): Functions in `mathlib` are marked with `+` to export them
- **Module boundaries**: Only public (`+`) items are accessible from other modules

### 3. Namespace Usage
```atom
// Import all items from mathlib
mathlib::*

// Now can call mathlib functions directly
result := add(5, 3)
```

### 4. Package Structure
The compiler automatically:
- Finds all `.atom` files in `src/`
- Scans `deps/` for external packages
- Assigns module names based on directory names
- Makes public items available via namespace

## Building and Running

```bash
# From the atom-lang root directory
./atomc examples/deps_example/ -o deps_example

# Run the program
./deps_example
```

### Expected Output
```
=== Dependency Example ===
Using mathlib from deps/mathlib/
Successfully imported and used functions from mathlib!
```

## What This Demonstrates

This example successfully validates the deps/ namespace feature:

✅ **Compiles successfully** using the deps/ directory structure  
✅ **Module namespace resolution** via `mathlib::*` import statement  
✅ **Calls public functions** from the mathlib dependency  
✅ **Proper project structure** with src/ and deps/ separation  
✅ **Visibility control** - only `+` prefixed items are accessible  

## Files Explained

### `deps/mathlib/src/operations.atom`
A simple math library with three public functions:
- `+add(a, b)` - Addition
- `+multiply(a, b)` - Multiplication  
- `+factorial(n)` - Factorial calculation (iterative)

All functions are marked `+` to export them from the mathlib module.

### `src/main.atom`
The main program that:
1. Imports mathlib with `mathlib::*`
2. Calls the imported functions (`add`, `multiply`, `factorial`)
3. Prints success messages using stdlib's `print()` function

## Implementation Details

### Import Statement
The import statement `mathlib::*` at the top of `main.atom` makes all public items from the mathlib module available in the current scope. This allows calling `add()` instead of `mathlib::add()`.

### Module Naming
The compiler:
1. Scans `deps/` directory
2. For each subdirectory (e.g., `mathlib/`), creates a module with that name
3. Looks for `.atom` files in `mathlib/src/`
4. Parses them with `--module mathlib` flag
5. Registers public functions with the module namespace

### Compilation Process
```bash
# The atomc script handles:
# 1. Finding deps/mathlib/src/operations.atom
# 2. Parsing with: atom --ast --module mathlib operations.atom
# 3. The S-expression includes: (program :module "mathlib" ...)
# 4. Backend registers functions as mathlib::add, mathlib::multiply, etc.
# 5. Import statement makes them available without prefix
```

## Notes

- Now uses stdlib's `print()` function instead of calling C functions directly
- Demonstrates the deps/ namespace feature for external dependencies
- This is a **proper project structure** unlike single-file examples

## Testing the Feature

This example specifically tests:
- ✅ Parser's `--module` flag for tagging items with module names
- ✅ S-expression `:module` metadata propagation
- ✅ Backend's module-aware function registration
- ✅ Import statement parsing and resolution  
- ✅ Cross-module function calls
