# Atom
Atom is a small language with value semantics, providing modern features with a focus on orthogonality, i.e., many small features that generalize and compose well.

## Structure
An example atom project would look like this:

```
📂 src
 ├─ 📄 main.atom
 ├─ 📂 physics
 │   ├─ 📄 simulation.atom
 │   └─ 📄 simulation.test.atom
 ├─ 📂 graphics
 │   ├─ 📄 sprites.atom
 │   └─ 📄 animations.atom
📂 test
 ├─ 📄 integration.test.atom
📂 deps
 ├─ 📂 matrix
 │   └─ 📂 src
 │       ├─ 📄 types.atom
 │       ├─ 📄 operations.atom
 │       └─ 📄 simulation.test.atom
 └─ 📂 accelerate
     ├─ ⚙️ lib.dylib
     ├─ ⚙️ lib.so
     └─ ⚙️ lib.dll
```

By default, functions and types are internal to the package, i.e., visible to all other files in `src/` if not marked with the `private` keyword.
Libraries are placed in `deps/`. In order for other packages to be visible, functions and types have to be marked with the `export` keyword. Libraries written in Atom are compiled together with the source code, while libraries provided as dynamic libraries are linked during compilation.

## Basics

### Structs & Tuples
Structs are declared by listing all its fields. Struct fields always match the visibility of the struct itself:

```atom
InternalFields {
  field Int
}

export PublicFields {
  field Int
}

private FileVisibleFields {
  field Int
}
```
`export` and `private` are contextual keywords, meaning they can still be used as variable and function names.

Structs can derive fields from other structs

```atom
Vec2 {
  x Float
  y Float
}

Vec3 {
  ..Vec2
  z Float
}

Vec2D {
  ..Vec2
  x Float // error: Redeclaration of struct field
}
```

#### Constructors
Constructors use the type name and field names
```atom
Pair {
  first Int
  second Int
}

main() {
  a := Pair(first: 5, second: 7)
}
```

Since typing in Atom is structural, you can omit the type name
```atom
main() {
  a := (first: 5, second: 7) // The compiler will infer Pair as the type
}
```

alternatively field names can be ommitted and instead use positionals
```atom
main() {
  a := Pair(5, 7)
}
```

#### Variadic Tuples
As "arrays", Atom supports variadic tuples and structs:

```atom
Values {
  title String
  values Int*
}

main {
  a: Values = ("Prices", 14, 10, 5)
}
```

Tuples (without named fields) can be accessed by index (using function call syntax):

```atom
main() {
  a: (Int*) = (5, 1, 17, 3)
  b: (Int, Float) = (5, 27.0) 

  assert(a(2) == 17)
  assert(a(2) == 27.0)
}
```
For now, to allow typechecking, the index on heterogenous tuples must be comptime known.

Variadics can be specified with `+` instead of `*` to be non-empty:
```atom
main() {
  a: (Int+) = (1, 2, 3)
  b: (Int+) = () // error: Non-empty variadic cannot be initialized from empty tuple
}
```

For tuples with known size, there is a shorthand to specify repetition: `(Int, Int, Int, Int, Float, Float)` can be written as `(Int*4, Float*2)`.

#### Relaxed Tuple Notation
Where unambigous, the parenthesese on tuples can be left out, both for the type notation and the actual values. The following code snippets are equivalent:

```
// TYPE DECLARATION
Inventory {
  player Player
  items (Item*)
}
// can be written as:
Inventory {
  player Player
  items Item*
}

// CONSTRUCTORS
inv := Inventory(p1, (sword, helmet, potato))
// can be written as:
inv := Inventory(p1, sword, helmet, potato)

// FUNCTION RETURN VALUES
drop2() (Item, Item) {
  sword := // ...
  helmet := // ...

  (sword, helmet)
}
// can be written as:
drop2() Item, Item {
  sword := // ...
  helmet := // ...

  sword, helmet
}
// or with repetition syntax:
drop2() Item*2 {
  // ...
}

// TYPE ANNOTATIONS
(i1, i2) := drop2()
// can be written as:
i1, i2 := drop2()

(i1, i2): (Item, Item) = drop2()
// can be written as:
i1, i2: Item, Item = drop2()
// or with repetition syntax:
i1, i2: Item*2 = drop2()
```

#### Implicit Conversion
Structs can be converted into each other using the following rules:

a) Struct *A* -> Struct *B* **if** all fields of *B* are present in *A* (and types are convertible)
```atom
Vec2 {
  x Float
  y Float
}

Vec3 {
  x Float
  y Float
  z Float
}

main() {
  a: Vec2 = Vec3(1.0, 2.0) 
  b: Vec3 = Vec2(1.0, 2.0) // error: cannot convert (x: Float, y: Float) to (x: Float, y: Float, **z: Float**)
}
```

b) Tuple *A* -> Tuple *B* **if** all fields of *B* are listed as the first fields of *A*
```atom
main() {
  a: (Int, Float) = (5, 7.0, 3)
}
```

c) Tuple *A* -> Struct *B* **if** all fields of *B* are listed as the first fields of *A*
```atom
main() {
  a: Vec2 = (5.0, 7.0)
  b: Vec2 = (3.3, 7.7, "Additional Info")
}
```

c) Struct *A* -> Tuple *B* **if** all fields of *B* are listed as the first fields of *A*
```atom
main() {
  a: (Float, Float) = Vec2(5.0, 7.0)
  b: (Float, Float) = (x: 5.0, y: 7.0)
}
```

d) Fields can be converted to a variadic field of the same type -- but not vice versa
```atom
main() {
  a: (String, Int, Int) = ("Hello", 1, 2)
  b: (String, Int*) = a

  a = b // error: Cannot safely convert variadic fields to concrete number of fields
}
```

Nominal types can be emulated by adding a void field
```atom
Cat {
  name String
  age Int

  cat Void
}

main() {
  meow: Cat = (name: "Meow", age: 3) // error: Cannot convert (name: String, age: Int) to (name: String, age: Int, **cat: Void**)   
}
```


### Enums
Enums are declared by listing all cases and their associated values if they have some:

```atom
MaybeInt {
  Some(Int)
  None
}
```

Atom is rather strict with casing: Enum cases and type names always start with an upper case letter, variables and functions always start with a lower case letter.

### Control Flow
By default, there are only two built-in control flow constructs: `match` and `loop`.

```atom
main() {
  match(1 == 1) {
    True -> print("This is correct!")
    False -> error("This should have been true...")
  }
}
```

Match allows nested patterns as well:

```atom
retry_until_success() -> Payload {
  response = send_request()
  match(response) {
    Success(Some(payload)) -> payload
    Success(None) -> error("Expected payload on succesful response")
    Loading -> retry_until_success()
    Error(err) -> error("Request failed: \(err)")
  }
}
```

`loop` can either loop forever without argument or take a boolean expression as condition
```atom
main() {
  a := 3
  loop(a > 0) {
    print("\(a) iterations remaining")
    a -= 1
  }

  print("This will run forever...")
  loop {
    print("...and ever...")
  }
}  
```

Loop can also take an integer to loop n times
```atom
main() {
  print("Say hello three times:")
  loop(3) {
    print("Hello")
  }
}
```

Additionally, loop can iterate over tuples, providing the current element as `$0`

```atom
main() {
  values := (1, 2, 4, 13)
  sum := 0

  loop(values) {
    sum += $0
  }

  assert(sum == 20)
}
```

### Operators
There are the following operators:
- Arithmetic: `+`, `-`, `*`, `/`, `%` along with the corresponding assignment operators
- Comparison: `<`, `>`, `<=`, `>=`, `==`, `!=`
- Logical: `||`, `&&`, `!`
- Collection: `++` concatenates tuples (and strings), e.g., `"Hello " ++ "World!"` or `(1, 2, 3) ++ (4, 5) == (1, 2, 3, 4, 5)`, variadic tuples and strings also support extending via `++=`

### Builtin Types
#### Primitives
`Int`, `UInt` `Float`, `Rune`, `String`

`Int`, `UInt` and `Float` take a size in bits as optional [const parameter](#const-parameters), i.e., an `UInt(8)` would be a byte-long integer from 0..255. By default, all number types are 64 bits large. 

#### Standard Library Types
`Bool` (implemented as an enum, but builtin support in that it is returned by the comparison operators and supports logical operators),
`Result`, `Option`
`Void`

`Void` is simply an empty type that supports all operators with the following results:
- `<arithmetic operator>`: `Void`
- `==`, `<=`, `>=`: `True`
- `!=`, `<`, `>`: `False`
- `||`: `False` (while admittedly a bit awkward at first glance, this is so that [derived operators on structs](#canonical-operator-implementation) works as expected when void fields are present)
- `&&`: `True`
- `!`: `Void`
- `++`: `Void`

The following types are all the same type as `Void` per structural typing `Empty {}`, `SomeOtherName {}`,  `()` 
### Canonical Operator Implementation
While Atom has no operator overloading, operators are still automatically supported on structs if all fields individually support the operator. If so, the operator is implemented by applying it field-wise.

```
main() {
  v := Pair(2, 3) + Pair(4, 5)
  assert(v.x == 6)
  assert(v.y == 8)
}  
```

### Functions
Functions are declared using a name, a parameter list and the return type (if it has some).
Their last expression is the return value:

```atom
double(a Int) Int {
  a * 2
}

print2(msg String) {
  print(msg)
  print(msg)
}
```

Visibility modifiers are added in front like for types:
```atom
export len(of String) Int {
  // ...
}
```

Functions support overloading:
```atom
dup(a Int) Int {
  10 * a + a
}

dup(a String) String {
  "\(a)\(a)"
}
```

Functions allow for variadic arguments:
```atom
sum(values Int*) Int {
  res := 0
  #loop(values) {
    res += $0
  }
}

main() {
  assert(sum(1, 2, 3) == 6)
}
```

### Closures
Functions are first-class citizens and can be passed as arguments:

```atom
floatify(items Int*, func (Int) {Float}) Float* {
  results: Float*
  loop(items) {
    results ++= func($0)
  }

  results
}  
```

### Uniform Calling syntax
Methods can use the dot-notation on their first argument, this even works for `loop` and `match`:

```main
loop(items) {
  // ...
}
// can be written as:
items.loop() {
  // ...
}

match(maybe) {
  Some(_) -> //...
  None -> // ...
}
// can be written as:
maybe.match() {
  // ...
}
```

### String Interpolation
Atom support string interpolation with `\()`

```
main() {
  a := 5
  print("I think \(a * 2 + 1) is an odd number")
}
```

### Testing
As shown [earlier](#structure), atom locates tests in `.test.atom` files.
Test files allow for two additional syntactic constructs: top-level code and named blocks.

```atom
// filename.test.atom

// This is a toplevel test
foo := 5
assert(foo + 2 == 7)

// This is a named test
"Basic math" {
  assert(2 + 3 == 5)
}

"Weird test" {
  assert(1 == 0, "Welp...")
}

// This is an anonymous test
{
  assert("Hello " ++ "World!" == "Hello World!")
}
```

`atom test` will report this roughly as follows:

```
3 out of 4 tests passed!
📂 test
 └─ 📄 filename.test.atom
     ├─ ✔️ *toplevel*
     ├─ ✔️ *anonymous*
     ├─ ❌ "Weird test" -- Assertion failed: "Welp..." (expected: `False`, got: `True`)
     └─ ✔️ "Basic math"   
```

### Zero values
Variables are automatically initialized with their zero values, like in *Go*. Unlike in Go, however, constructors cannot implicitly leave out fields that have not explicit default value.

### Comptime Evaluation
Prefixing any call with `#` evaluates it at compile-time. This does not cause closures to evaluate, so that for `match` and `loop` this means unfolding the match arm / unrolling the loop but not evaluating the body.

```atom
add(a Int, b Int) {
  a + b
}

main() {
  number := #add(3, 5)
  other := random()

  #loop(number) {
    print(other)
  }

  #loop(number) {
    #print("This will be printed during compilation 8 times!")
  }

  #match(number) {
    8 -> add(8, other)
    _ -> #error("The number should have been 8 at compile time")
  }
}
```


### Const Parameters
Types and functions allow for const parameters.
Const parameters are specified in a function or constructor after the non-const arguments.
```atom
Container(t Type) {
  item t 
}

get(t Type; c Container(t)) t {
  c.item
}

"Init container" {
  c := Container(Int; 5)
  print("\(c.get()) is five")
}
```

By convention, lower case letters in type position are implicitly declared as constants. If possible, constants (like type parameters) are inferred by the compiler.
```atom
Container {
  item t
}

"Init container" {
  c := Container()  
}
```

