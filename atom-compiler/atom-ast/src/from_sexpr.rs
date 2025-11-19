//! S-Expression parser for Atom AST
//!
//! This module provides functionality to parse S-Expression format back into
//! Atom AST nodes.

use crate::ast::*;
use crate::span::Span;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum SExprToken {
    LParen,
    RParen,
    Symbol(String),
    String(String),
    Integer(i64),
    Float(f64),
}

#[derive(Debug)]
pub struct ParseError {
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ParseError {}

type Result<T> = std::result::Result<T, ParseError>;

/// Helper trait for parsing AST nodes from S-Expressions
pub trait FromSExpr: Sized {
    fn from_sexpr(sexpr: &SExpr) -> Result<Self>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum SExpr {
    List(Vec<SExpr>),
    Symbol(String),
    String(String),
    Integer(i64),
    Float(f64),
}

impl SExpr {
    pub fn parse(input: &str) -> Result<SExpr> {
        let tokens = tokenize(input)?;
        let (expr, rest) = parse_sexpr(&tokens)?;
        if !rest.is_empty() {
            return Err(ParseError {
                message: "Unexpected tokens after expression".to_string(),
            });
        }
        Ok(expr)
    }

    pub fn as_symbol(&self) -> Result<&str> {
        match self {
            SExpr::Symbol(s) => Ok(s),
            _ => Err(ParseError {
                message: format!("Expected symbol, got {:?}", self),
            }),
        }
    }

    pub fn as_string(&self) -> Result<&str> {
        match self {
            SExpr::String(s) => Ok(s),
            _ => Err(ParseError {
                message: format!("Expected string, got {:?}", self),
            }),
        }
    }

    pub fn as_list(&self) -> Result<&[SExpr]> {
        match self {
            SExpr::List(l) => Ok(l),
            _ => Err(ParseError {
                message: format!("Expected list, got {:?}", self),
            }),
        }
    }

    pub fn as_integer(&self) -> Result<i64> {
        match self {
            SExpr::Integer(n) => Ok(*n),
            _ => Err(ParseError {
                message: format!("Expected integer, got {:?}", self),
            }),
        }
    }

    pub fn as_float(&self) -> Result<f64> {
        match self {
            SExpr::Float(f) => Ok(*f),
            _ => Err(ParseError {
                message: format!("Expected float, got {:?}", self),
            }),
        }
    }
}

fn tokenize(input: &str) -> Result<Vec<SExprToken>> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();

    while let Some(&ch) = chars.peek() {
        match ch {
            '(' => {
                tokens.push(SExprToken::LParen);
                chars.next();
            }
            ')' => {
                tokens.push(SExprToken::RParen);
                chars.next();
            }
            '"' => {
                chars.next();
                let mut s = String::new();
                while let Some(&ch) = chars.peek() {
                    if ch == '"' {
                        chars.next();
                        break;
                    } else if ch == '\\' {
                        chars.next();
                        if let Some(&next) = chars.peek() {
                            chars.next();
                            match next {
                                'n' => s.push('\n'),
                                't' => s.push('\t'),
                                'r' => s.push('\r'),
                                '\\' => s.push('\\'),
                                '"' => s.push('"'),
                                _ => {
                                    s.push('\\');
                                    s.push(next);
                                }
                            }
                        }
                    } else {
                        s.push(ch);
                        chars.next();
                    }
                }
                tokens.push(SExprToken::String(s));
            }
            '\'' => {
                chars.next();
                let mut s = String::new();
                while let Some(&ch) = chars.peek() {
                    if ch == '\'' {
                        chars.next();
                        break;
                    } else if ch == '\\' {
                        chars.next();
                        if let Some(&next) = chars.peek() {
                            chars.next();
                            match next {
                                'n' => s.push('\n'),
                                't' => s.push('\t'),
                                'r' => s.push('\r'),
                                '\\' => s.push('\\'),
                                '\'' => s.push('\''),
                                _ => {
                                    s.push('\\');
                                    s.push(next);
                                }
                            }
                        }
                    } else {
                        s.push(ch);
                        chars.next();
                    }
                }
                // For runes, we store them as strings for now
                tokens.push(SExprToken::String(s));
            }
            c if c.is_whitespace() => {
                chars.next();
            }
            _ => {
                let mut symbol = String::new();
                while let Some(&ch) = chars.peek() {
                    if ch.is_whitespace() || ch == '(' || ch == ')' {
                        break;
                    }
                    symbol.push(ch);
                    chars.next();
                }

                // Try to parse as number
                if let Ok(n) = symbol.parse::<i64>() {
                    tokens.push(SExprToken::Integer(n));
                } else if let Ok(f) = symbol.parse::<f64>() {
                    tokens.push(SExprToken::Float(f));
                } else {
                    tokens.push(SExprToken::Symbol(symbol));
                }
            }
        }
    }

    Ok(tokens)
}

fn parse_sexpr(tokens: &[SExprToken]) -> Result<(SExpr, &[SExprToken])> {
    if tokens.is_empty() {
        return Err(ParseError {
            message: "Unexpected end of input".to_string(),
        });
    }

    match &tokens[0] {
        SExprToken::LParen => {
            let mut items = Vec::new();
            let mut rest = &tokens[1..];

            loop {
                if rest.is_empty() {
                    return Err(ParseError {
                        message: "Unexpected end of list".to_string(),
                    });
                }

                if matches!(rest[0], SExprToken::RParen) {
                    return Ok((SExpr::List(items), &rest[1..]));
                }

                let (expr, new_rest) = parse_sexpr(rest)?;
                items.push(expr);
                rest = new_rest;
            }
        }
        SExprToken::RParen => Err(ParseError {
            message: "Unexpected )".to_string(),
        }),
        SExprToken::Symbol(s) => Ok((SExpr::Symbol(s.clone()), &tokens[1..])),
        SExprToken::String(s) => Ok((SExpr::String(s.clone()), &tokens[1..])),
        SExprToken::Integer(n) => Ok((SExpr::Integer(*n), &tokens[1..])),
        SExprToken::Float(f) => Ok((SExpr::Float(*f), &tokens[1..])),
    }
}

// Helper to create a dummy span
fn dummy_span() -> Span {
    Span::new(0, 0)
}

// Helper to parse span from list if present
// Looks for `:span (start end)` pattern in the list
fn parse_span(list: &[SExpr]) -> Span {
    // Look for :span keyword followed by (start end)
    for i in 0..list.len().saturating_sub(1) {
        if let Ok(symbol) = list[i].as_symbol()
            && (symbol == ":span" || symbol == ":spans-enabled") {
                // Skip metadata markers
                if symbol == ":spans-enabled" {
                    continue;
                }
                
                // Try to parse the next element as a span
                if let Ok(span_list) = list[i + 1].as_list()
                    && span_list.len() == 2
                        && let (Ok(start), Ok(end)) = (
                            span_list[0].as_integer(),
                            span_list[1].as_integer(),
                        ) {
                            return Span::new(start as usize, end as usize);
                        }
            }
    }
    dummy_span()
}

// Helper to filter out span metadata from lists
fn filter_metadata(list: &[SExpr]) -> Vec<&SExpr> {
    let mut result = Vec::new();
    let mut i = 0;
    while i < list.len() {
        if let Ok(symbol) = list[i].as_symbol()
            && (symbol == ":span" || symbol == ":spans-enabled") {
                // Skip :span and its argument
                if symbol == ":span" && i + 1 < list.len() {
                    i += 2; // Skip :span and (start end)
                } else {
                    i += 1; // Skip :spans-enabled
                }
                continue;
            }
        result.push(&list[i]);
        i += 1;
    }
    result
}

impl FromSExpr for Vec<TopLevel> {
    fn from_sexpr(sexpr: &SExpr) -> Result<Self> {
        let list = sexpr.as_list()?;
        if list.is_empty() {
            return Err(ParseError {
                message: "Empty program".to_string(),
            });
        }

        let head = list[0].as_symbol()?;
        if head != "program" {
            return Err(ParseError {
                message: format!("Expected 'program', got '{}'", head),
            });
        }

        // Filter out metadata like :spans-enabled
        let filtered = filter_metadata(&list[1..]);
        
        let mut items = Vec::new();
        for item in filtered {
            items.push(TopLevel::from_sexpr(item)?);
        }
        Ok(items)
    }
}

impl FromSExpr for TopLevel {
    fn from_sexpr(sexpr: &SExpr) -> Result<Self> {
        let list = sexpr.as_list()?;
        if list.is_empty() {
            return Err(ParseError {
                message: "Empty top-level item".to_string(),
            });
        }

        let head = list[0].as_symbol()?;
        match head {
            "import" => Ok(TopLevel::Import(ImportDecl::from_sexpr(sexpr)?)),
            "struct" => Ok(TopLevel::Struct(StructDef::from_sexpr(sexpr)?)),
            "enum" => Ok(TopLevel::Enum(EnumDef::from_sexpr(sexpr)?)),
            "function" => Ok(TopLevel::Function(FunctionDef::from_sexpr(sexpr)?)),
            "const" | "var" => Ok(TopLevel::Variable(VarDecl::from_sexpr(sexpr)?)),
            "test" => Ok(TopLevel::TestBlock(TestBlock::from_sexpr(sexpr)?)),
            _ => Ok(TopLevel::Statement(Stmt::from_sexpr(sexpr)?)),
        }
    }
}

impl FromSExpr for Visibility {
    fn from_sexpr(sexpr: &SExpr) -> Result<Self> {
        let s = sexpr.as_symbol()?;
        match s {
            "public" => Ok(Visibility::Public),
            "file-private" => Ok(Visibility::FilePrivate),
            "internal" => Ok(Visibility::Internal),
            _ => Err(ParseError {
                message: format!("Unknown visibility: {}", s),
            }),
        }
    }
}

impl FromSExpr for ImportDecl {
    fn from_sexpr(sexpr: &SExpr) -> Result<Self> {
        let list = sexpr.as_list()?;
        if list.len() < 3 {
            return Err(ParseError {
                message: "Import declaration requires at least 3 elements".to_string(),
            });
        }

        let span = parse_span(list);
        
        let namespace = Ident {
            name: list[1].as_symbol()?.to_string(),
            span: dummy_span(),
        };

        let items = ImportItems::from_sexpr(&list[2])?;

        Ok(ImportDecl {
            namespace,
            items,
            span,
        })
    }
}

impl FromSExpr for ImportItems {
    fn from_sexpr(sexpr: &SExpr) -> Result<Self> {
        if let Ok(s) = sexpr.as_symbol()
            && s == "*" {
                return Ok(ImportItems::All);
            }

        let list = sexpr.as_list()?;
        let mut items = Vec::new();
        for item in list {
            items.push(Ident {
                name: item.as_symbol()?.to_string(),
                span: dummy_span(),
            });
        }
        Ok(ImportItems::Named(items))
    }
}

impl FromSExpr for StructDef {
    fn from_sexpr(sexpr: &SExpr) -> Result<Self> {
        let list = sexpr.as_list()?;
        if list.len() < 3 {
            return Err(ParseError {
                message: "Struct definition requires at least 3 elements".to_string(),
            });
        }

        let span = parse_span(list);
        let filtered = filter_metadata(list);
        
        let visibility = Visibility::from_sexpr(filtered[1])?;
        let name = Ident {
            name: filtered[2].as_symbol()?.to_string(),
            span: dummy_span(),
        };

        let mut idx = 3;
        let mut type_params = Vec::new();

        // Check for type-params
        if idx < filtered.len()
            && let Ok(inner_list) = filtered[idx].as_list()
                && !inner_list.is_empty() && inner_list[0].as_symbol().ok() == Some("type-params") {
                    for param_sexpr in &inner_list[1..] {
                        type_params.push(TypeParam::from_sexpr(param_sexpr)?);
                    }
                    idx += 1;
                }

        let mut fields = Vec::new();
        while idx < filtered.len() {
            fields.push(Field::from_sexpr(filtered[idx])?);
            idx += 1;
        }

        Ok(StructDef {
            visibility,
            name,
            type_params,
            fields,
            span,
        })
    }
}

impl FromSExpr for Field {
    fn from_sexpr(sexpr: &SExpr) -> Result<Self> {
        let list = sexpr.as_list()?;
        if list.len() < 2 {
            return Err(ParseError {
                message: "Field requires at least 2 elements".to_string(),
            });
        }

        let span = parse_span(list);
        let filtered = filter_metadata(list);

        if filtered[0].as_symbol()? != "field" {
            return Err(ParseError {
                message: "Expected 'field'".to_string(),
            });
        }

        // Check if we have a named field or tuple field
        let (name, ty_idx) = if filtered.len() == 2 {
            // Tuple field: (field Type)
            (None, 1)
        } else {
            // Named field: (field name Type)
            (
                Some(Ident {
                    name: filtered[1].as_symbol()?.to_string(),
                    span: dummy_span(),
                }),
                2,
            )
        };

        let ty = Box::new(Type::from_sexpr(filtered[ty_idx])?);

        Ok(Field {
            name,
            ty,
            span,
        })
    }
}

impl FromSExpr for EnumDef {
    fn from_sexpr(sexpr: &SExpr) -> Result<Self> {
        let list = sexpr.as_list()?;
        if list.len() < 3 {
            return Err(ParseError {
                message: "Enum definition requires at least 3 elements".to_string(),
            });
        }

        let span = parse_span(list);
        let filtered = filter_metadata(list);
        
        let visibility = Visibility::from_sexpr(filtered[1])?;
        let name = Ident {
            name: filtered[2].as_symbol()?.to_string(),
            span: dummy_span(),
        };

        let mut idx = 3;
        let mut type_params = Vec::new();

        // Check for type-params
        if idx < filtered.len()
            && let Ok(inner_list) = filtered[idx].as_list()
                && !inner_list.is_empty() && inner_list[0].as_symbol().ok() == Some("type-params") {
                    for param_sexpr in &inner_list[1..] {
                        type_params.push(TypeParam::from_sexpr(param_sexpr)?);
                    }
                    idx += 1;
                }

        let mut cases = Vec::new();
        while idx < filtered.len() {
            cases.push(EnumCase::from_sexpr(filtered[idx])?);
            idx += 1;
        }

        Ok(EnumDef {
            visibility,
            name,
            type_params,
            cases,
            span,
        })
    }
}

impl FromSExpr for EnumCase {
    fn from_sexpr(sexpr: &SExpr) -> Result<Self> {
        let list = sexpr.as_list()?;
        if list.len() < 2 {
            return Err(ParseError {
                message: "Enum case requires at least 2 elements".to_string(),
            });
        }

        let span = parse_span(list);
        let filtered = filter_metadata(list);

        if filtered[0].as_symbol()? != "case" {
            return Err(ParseError {
                message: "Expected 'case'".to_string(),
            });
        }

        let name = Ident {
            name: filtered[1].as_symbol()?.to_string(),
            span: dummy_span(),
        };

        let mut fields = Vec::new();
        for field_sexpr in &filtered[2..] {
            fields.push(Box::new(Type::from_sexpr(field_sexpr)?));
        }

        Ok(EnumCase {
            name,
            fields,
            span,
        })
    }
}

impl FromSExpr for TypeParam {
    fn from_sexpr(sexpr: &SExpr) -> Result<Self> {
        let list = sexpr.as_list()?;
        if list.is_empty() {
            return Err(ParseError {
                message: "Type parameter requires at least 1 element".to_string(),
            });
        }

        let span = parse_span(list);
        let filtered = filter_metadata(list);

        // Determine if the first element is a name or a type
        // Format can be:
        // (name Type) - name is Some, ty is Some
        // (name) - name is Some, ty is None (for const params like (32))
        // (Type) - name is None, ty is Some (anonymous type param like in Option<Int>)
        // () - name is None, ty is None (empty param)
        let (name, mut idx) = if filtered.is_empty() {
            // Empty: () -> no name, no type
            (None, 0)
        } else if let SExpr::Integer(n) = filtered[0] {
            // Integer: (32) -> const parameter, name is the integer as string
            (Some(Ident {
                name: n.to_string(),
                span: dummy_span(),
            }), 1)
        } else if let Ok(name_str) = filtered[0].as_symbol() {
            // Symbol: could be a name or a type
            if filtered.len() > 1 && filtered[1].as_symbol().ok() != Some(":default") {
                // More elements after the symbol (and it's not :default), so symbol is a name
                // Format: (name Type) or (name Type :default ...)
                (Some(Ident {
                    name: name_str.to_string(),
                    span: dummy_span(),
                }), 1)
            } else if filtered.len() == 1 {
                // Single symbol - need to disambiguate between name and type
                // Heuristic: lowercase identifiers (especially single letters) are typically
                // type parameter names, while capitalized identifiers are type names
                let first_char = name_str.chars().next().unwrap_or('A');
                if first_char.is_lowercase() {
                    // Treat as a parameter name: (t) -> name="t", ty=None
                    (Some(Ident {
                        name: name_str.to_string(),
                        span: dummy_span(),
                    }), 1)
                } else {
                    // Treat as a type: (Int) -> name=None, ty=Type::Named("Int")
                    (None, 0)
                }
            } else {
                // Symbol with :default: (t :default ...)
                (Some(Ident {
                    name: name_str.to_string(),
                    span: dummy_span(),
                }), 1)
            }
        } else {
            // List: first element is a complex type, no name
            // Format: ((type-param t)) or ((tuple ...))
            (None, 0)
        };

        let mut ty = None;
        let mut default = None;

        while idx < filtered.len() {
            if filtered[idx].as_symbol().ok() == Some(":default") {
                idx += 1;
                if idx < filtered.len() {
                    default = Some(Box::new(Type::from_sexpr(filtered[idx])?));
                    idx += 1;
                }
            } else {
                ty = Some(Box::new(Type::from_sexpr(filtered[idx])?));
                idx += 1;
            }
        }

        Ok(TypeParam {
            name,
            ty,
            default,
            span,
        })
    }
}

impl FromSExpr for FunctionDef {
    fn from_sexpr(sexpr: &SExpr) -> Result<Self> {
        let list = sexpr.as_list()?;
        if list.len() < 5 {
            return Err(ParseError {
                message: "Function definition requires at least 5 elements".to_string(),
            });
        }

        let span = parse_span(list);
        let filtered = filter_metadata(list);
        
        let visibility = Visibility::from_sexpr(filtered[1])?;
        let name = Ident {
            name: filtered[2].as_symbol()?.to_string(),
            span: dummy_span(),
        };

        let mut idx = 3;
        let mut const_params = Vec::new();

        // Check for const-params
        if idx < filtered.len()
            && let Ok(inner_list) = filtered[idx].as_list()
                && !inner_list.is_empty() && inner_list[0].as_symbol().ok() == Some("const-params") {
                    for param_sexpr in &inner_list[1..] {
                        const_params.push(Param::from_sexpr(param_sexpr)?);
                    }
                    idx += 1;
                }

        // Parse params
        let params_list = filtered[idx].as_list()?;
        if params_list.is_empty() || params_list[0].as_symbol()? != "params" {
            return Err(ParseError {
                message: "Expected 'params'".to_string(),
            });
        }

        let mut params = Vec::new();
        for param_sexpr in &params_list[1..] {
            params.push(Param::from_sexpr(param_sexpr)?);
        }
        idx += 1;

        // Check for returns
        let mut return_type = None;
        if idx < filtered.len()
            && let Ok(returns_list) = filtered[idx].as_list()
                && !returns_list.is_empty() && returns_list[0].as_symbol().ok() == Some("returns") {
                    return_type = Some(Box::new(Type::from_sexpr(&returns_list[1])?));
                    idx += 1;
                }

        // Parse body (block)
        let body = Block::from_sexpr(filtered[idx])?;

        Ok(FunctionDef {
            visibility,
            name,
            const_params,
            params,
            return_type,
            body,
            span,
        })
    }
}

impl FromSExpr for Param {
    fn from_sexpr(sexpr: &SExpr) -> Result<Self> {
        let list = sexpr.as_list()?;
        if list.is_empty() {
            return Err(ParseError {
                message: "Parameter requires at least 1 element".to_string(),
            });
        }

        let span = parse_span(list);
        let filtered = filter_metadata(list);

        let name = Ident {
            name: filtered[0].as_symbol()?.to_string(),
            span: dummy_span(),
        };

        let mut ty = None;
        let mut default = None;
        let mut idx = 1;

        while idx < filtered.len() {
            if filtered[idx].as_symbol().ok() == Some(":default") {
                idx += 1;
                if idx < filtered.len() {
                    default = Some(Box::new(Expr::from_sexpr(filtered[idx])?));
                    idx += 1;
                }
            } else {
                ty = Some(Box::new(Type::from_sexpr(filtered[idx])?));
                idx += 1;
            }
        }

        Ok(Param {
            name,
            ty,
            default,
            span,
        })
    }
}

impl FromSExpr for VarDecl {
    fn from_sexpr(sexpr: &SExpr) -> Result<Self> {
        let list = sexpr.as_list()?;
        if list.len() < 3 {
            return Err(ParseError {
                message: "Variable declaration requires at least 3 elements".to_string(),
            });
        }

        let span = parse_span(list);
        let filtered = filter_metadata(list);

        let is_const = filtered[0].as_symbol()? == "const";
        let visibility = Visibility::from_sexpr(filtered[1])?;

        // Parse names (can be a single name or a tuple)
        let names = if let Ok(names_list) = filtered[2].as_list() {
            // Tuple destructuring
            names_list
                .iter()
                .map(|s| {
                    Ok(Ident {
                        name: s.as_symbol()?.to_string(),
                        span: dummy_span(),
                    })
                })
                .collect::<Result<Vec<_>>>()?
        } else {
            // Single name
            vec![Ident {
                name: filtered[2].as_symbol()?.to_string(),
                span: dummy_span(),
            }]
        };

        let mut ty = None;
        let mut init = None;

        // Parse remaining elements (type and/or init)
        // Format: (var visibility name [type] [init] :span ...)
        // If we have 1 element: it could be type OR init (ambiguous for tuples)
        // If we have 2 elements: first is type, second is init
        match filtered.len() - 3 {
            0 => {
                // No type or init
            }
            1 => {
                // One element: could be type or init
                // Try parsing as expression first since var decls usually have init
                if let Ok(expr) = Expr::from_sexpr(filtered[3]) {
                    // Check if it can also be parsed as a type
                    if let Ok(_t) = Type::from_sexpr(filtered[3]) {
                        // Ambiguous: could be either type or init
                        // Heuristic: For variable declarations, init is more common than bare type
                        // Prefer init unless it looks clearly like a type annotation
                        match filtered[3] {
                            // Check if it's a complex type expression (starts with list)
                            SExpr::List(list) if !list.is_empty() => {
                                match list[0].as_symbol().ok() {
                                    Some("tuple") | Some("function-type") | Some("variadic*") | Some("variadic+") => {
                                        // Complex type syntax - treat as type
                                        ty = Some(Box::new(Type::from_sexpr(filtered[3])?));
                                    }
                                    _ => {
                                        // Other list forms are more likely expressions
                                        init = Some(Box::new(expr));
                                    }
                                }
                            }
                            // Simple symbol: use naming convention to disambiguate
                            // PascalCase (starts with uppercase) = type name
                            // snake_case/camelCase = variable/expression
                            SExpr::Symbol(s) => {
                                if s.chars().next().map_or(false, |c| c.is_uppercase()) {
                                    // Starts with uppercase - likely a type (e.g., String, Int, Bool)
                                    ty = Some(Box::new(Type::from_sexpr(filtered[3])?));
                                } else {
                                    // Starts with lowercase - likely a variable reference
                                    init = Some(Box::new(expr));
                                }
                            }
                            _ => {
                                // Default to init for ambiguous cases
                                init = Some(Box::new(expr));
                            }
                        }
                    } else {
                        // Can only be parsed as expression
                        init = Some(Box::new(expr));
                    }
                } else {
                    // Can only be parsed as type
                    ty = Some(Box::new(Type::from_sexpr(filtered[3])?));
                }
            }
            _ => {
                // Two or more elements: first is type, rest is init
                ty = Some(Box::new(Type::from_sexpr(filtered[3])?));
                init = Some(Box::new(Expr::from_sexpr(filtered[4])?));
            }
        }

        Ok(VarDecl {
            visibility,
            is_const,
            names,
            ty,
            init,
            span,
        })
    }
}

impl FromSExpr for TestBlock {
    fn from_sexpr(sexpr: &SExpr) -> Result<Self> {
        let list = sexpr.as_list()?;
        if list.len() < 2 {
            return Err(ParseError {
                message: "Test block requires at least 2 elements".to_string(),
            });
        }

        let span = parse_span(list);
        let filtered = filter_metadata(list);

        if filtered[0].as_symbol()? != "test" {
            return Err(ParseError {
                message: "Expected 'test'".to_string(),
            });
        }

        let mut idx = 1;
        let name = if let Ok(s) = filtered[idx].as_string() {
            idx += 1;
            Some(s.to_string())
        } else {
            None
        };

        let body = Block::from_sexpr(filtered[idx])?;

        Ok(TestBlock {
            name,
            body,
            span,
        })
    }
}

impl FromSExpr for Block {
    fn from_sexpr(sexpr: &SExpr) -> Result<Self> {
        let list = sexpr.as_list()?;
        if list.is_empty() {
            return Err(ParseError {
                message: "Block requires at least 1 element".to_string(),
            });
        }

        let span = parse_span(list);
        let filtered = filter_metadata(list);

        if filtered[0].as_symbol()? != "block" {
            return Err(ParseError {
                message: "Expected 'block'".to_string(),
            });
        }

        let mut stmts = Vec::new();
        for stmt_sexpr in &filtered[1..] {
            stmts.push(Stmt::from_sexpr(stmt_sexpr)?);
        }

        Ok(Block {
            stmts,
            span,
        })
    }
}

impl FromSExpr for Stmt {
    fn from_sexpr(sexpr: &SExpr) -> Result<Self> {
        // If it's not a list, it must be an expression
        if sexpr.as_list().is_err() {
            return Ok(Stmt::Expression(Expr::from_sexpr(sexpr)?));
        }
        
        let list = sexpr.as_list()?;
        if list.is_empty() {
            return Err(ParseError {
                message: "Statement requires at least 1 element".to_string(),
            });
        }

        let head = list[0].as_symbol()?;
        match head {
            "var" | "const" => Ok(Stmt::VarDecl(VarDecl::from_sexpr(sexpr)?)),
            _ => Ok(Stmt::Expression(Expr::from_sexpr(sexpr)?)),
        }
    }
}

impl FromSExpr for Type {
    fn from_sexpr(sexpr: &SExpr) -> Result<Self> {
        // If it's a simple symbol, it's a named type
        if let Ok(name) = sexpr.as_symbol() {
            return Ok(Type::Named(Ident {
                name: name.to_string(),
                span: dummy_span(),
            }));
        }

        let list = sexpr.as_list()?;
        if list.is_empty() {
            return Err(ParseError {
                message: "Type requires at least 1 element".to_string(),
            });
        }

        let span = parse_span(list);
        let filtered = filter_metadata(list);

        let head = filtered[0].as_symbol()?;
        match head {
            "type-param" => Ok(Type::Param(Ident {
                name: filtered[1].as_symbol()?.to_string(),
                span: dummy_span(),
            })),
            "tuple" => {
                let mut types = Vec::new();
                for ty_sexpr in &filtered[1..] {
                    types.push(Box::new(Type::from_sexpr(ty_sexpr)?));
                }
                Ok(Type::Tuple(types, span))
            }
            "generic" => {
                let name = Ident {
                    name: filtered[1].as_symbol()?.to_string(),
                    span: dummy_span(),
                };
                let mut params = Vec::new();
                for param_sexpr in &filtered[2..] {
                    params.push(Box::new(TypeParam::from_sexpr(param_sexpr)?));
                }
                Ok(Type::Generic {
                    name,
                    params,
                    span,
                })
            }
            "variadic+" => Ok(Type::Variadic {
                element: Box::new(Type::from_sexpr(filtered[1])?),
                non_empty: true,
                span,
            }),
            "variadic*" => Ok(Type::Variadic {
                element: Box::new(Type::from_sexpr(filtered[1])?),
                non_empty: false,
                span,
            }),
            "static-array" => Ok(Type::StaticArray {
                element: Box::new(Type::from_sexpr(filtered[1])?),
                size: Box::new(Expr::from_sexpr(filtered[2])?),
                span,
            }),
            "function-type" => {
                let params_list = filtered[1].as_list()?;
                if params_list.is_empty() || params_list[0].as_symbol()? != "params" {
                    return Err(ParseError {
                        message: "Expected 'params' in function type".to_string(),
                    });
                }

                let mut params = Vec::new();
                for param_sexpr in &params_list[1..] {
                    params.push(Box::new(Type::from_sexpr(param_sexpr)?));
                }

                let mut return_type = None;
                if filtered.len() > 2 {
                    let returns_list = filtered[2].as_list()?;
                    if !returns_list.is_empty() && returns_list[0].as_symbol().ok() == Some("returns") {
                        return_type = Some(Box::new(Type::from_sexpr(&returns_list[1])?));
                    }
                }

                Ok(Type::Function {
                    params,
                    return_type,
                    span,
                })
            }
            _ => Err(ParseError {
                message: format!("Unknown type: {}", head),
            }),
        }
    }
}

impl FromSExpr for Expr {
    fn from_sexpr(sexpr: &SExpr) -> Result<Self> {
        // Check for literals first
        match sexpr {
            SExpr::Integer(n) => {
                return Ok(Expr::Literal(Literal::Integer(*n), dummy_span()));
            }
            SExpr::Float(f) => {
                return Ok(Expr::Literal(Literal::Float(*f), dummy_span()));
            }
            SExpr::String(s) => {
                return Ok(Expr::Literal(Literal::String(s.clone()), dummy_span()));
            }
            SExpr::Symbol(s) => {
                // Check if it's a boolean literal
                if s == "True" {
                    return Ok(Expr::Literal(Literal::Bool(true), dummy_span()));
                } else if s == "False" {
                    return Ok(Expr::Literal(Literal::Bool(false), dummy_span()));
                } else {
                    // It's an identifier
                    return Ok(Expr::Ident(Ident {
                        name: s.clone(),
                        span: dummy_span(),
                    }));
                }
            }
            SExpr::List(_) => {}
        }

        let list = sexpr.as_list()?;
        if list.is_empty() {
            return Err(ParseError {
                message: "Expression requires at least 1 element".to_string(),
            });
        }

        let span = parse_span(list);
        let filtered = filter_metadata(list);

        let head = filtered[0].as_symbol()?;
        match head {
            "literal" => {
                // Wrapped literal with span: (literal value :span (start end))
                Ok(Expr::Literal(Literal::from_sexpr(filtered[1])?, span))
            }
            "ident" => {
                // Wrapped identifier with span: (ident name :span (start end))
                Ok(Expr::Ident(Ident {
                    name: filtered[1].as_symbol()?.to_string(),
                    span,
                }))
            }
            "+" | "*" | "/" | "%" | "==" | "!=" | "<" | "<=" | ">" | ">=" | "&&" | "||"
            | "&" | "|" | "<<" | ">>" | "++" | "=" | "+=" | "-=" | "*=" | "/=" | "%=" | "++=" => {
                let op = BinOp::from_str(head)?;
                Ok(Expr::Binary {
                    op,
                    left: Box::new(Expr::from_sexpr(filtered[1])?),
                    right: Box::new(Expr::from_sexpr(filtered[2])?),
                    span,
                })
            }
            "-" => {
                // "-" can be either binary or unary
                if filtered.len() == 2 {
                    Ok(Expr::Unary {
                        op: UnOp::Neg,
                        expr: Box::new(Expr::from_sexpr(filtered[1])?),
                        span,
                    })
                } else {
                    Ok(Expr::Binary {
                        op: BinOp::Sub,
                        left: Box::new(Expr::from_sexpr(filtered[1])?),
                        right: Box::new(Expr::from_sexpr(filtered[2])?),
                        span,
                    })
                }
            }
            "!" | "~" if filtered.len() == 2 => {
                let op = UnOp::from_str(head)?;
                Ok(Expr::Unary {
                    op,
                    expr: Box::new(Expr::from_sexpr(filtered[1])?),
                    span,
                })
            }
            "call" => {
                let func = Box::new(Expr::from_sexpr(filtered[1])?);
                let mut args = Vec::new();
                for arg_sexpr in &filtered[2..] {
                    args.push(Expr::from_sexpr(arg_sexpr)?);
                }
                Ok(Expr::Call {
                    func,
                    args,
                    span,
                })
            }
            "method-call" => {
                let receiver = Box::new(Expr::from_sexpr(filtered[1])?);
                let method = Ident {
                    name: filtered[2].as_symbol()?.to_string(),
                    span: dummy_span(),
                };
                let mut args = Vec::new();
                for arg_sexpr in &filtered[3..] {
                    args.push(Expr::from_sexpr(arg_sexpr)?);
                }
                Ok(Expr::MethodCall {
                    receiver,
                    method,
                    args,
                    span,
                })
            }
            "field-access" => Ok(Expr::FieldAccess {
                object: Box::new(Expr::from_sexpr(filtered[1])?),
                field: Ident {
                    name: filtered[2].as_symbol()?.to_string(),
                    span: dummy_span(),
                },
                span,
            }),
            "tuple" => {
                let mut exprs = Vec::new();
                for expr_sexpr in &filtered[1..] {
                    exprs.push(Expr::from_sexpr(expr_sexpr)?);
                }
                Ok(Expr::Tuple(exprs, span))
            }
            "struct-init" => {
                let ty = if filtered.len() > 1 {
                    if let Ok(name) = filtered[1].as_symbol() {
                        Some(Ident {
                            name: name.to_string(),
                            span: dummy_span(),
                        })
                    } else {
                        None
                    }
                } else {
                    None
                };

                let start_idx = if ty.is_some() { 2 } else { 1 };
                let mut fields = Vec::new();
                for field_sexpr in &filtered[start_idx..] {
                    fields.push(FieldInit::from_sexpr(field_sexpr)?);
                }

                Ok(Expr::StructInit {
                    ty,
                    fields,
                    span,
                })
            }
            "closure" => {
                let params_list = filtered[1].as_list()?;
                if params_list.is_empty() || params_list[0].as_symbol()? != "params" {
                    return Err(ParseError {
                        message: "Expected 'params' in closure".to_string(),
                    });
                }

                let mut params = Vec::new();
                for param_sexpr in &params_list[1..] {
                    params.push(Param::from_sexpr(param_sexpr)?);
                }

                let mut idx = 2;
                let mut return_type = None;

                if idx < filtered.len()
                    && let Ok(returns_list) = filtered[idx].as_list()
                        && !returns_list.is_empty()
                            && returns_list[0].as_symbol().ok() == Some("returns")
                        {
                            return_type = Some(Box::new(Type::from_sexpr(&returns_list[1])?));
                            idx += 1;
                        }

                let body = Box::new(Block::from_sexpr(filtered[idx])?);

                Ok(Expr::Closure {
                    params,
                    return_type,
                    body,
                    span,
                })
            }
            "block" => Ok(Expr::Block(Block::from_sexpr(sexpr)?)),
            "match" => {
                let expr = Box::new(Expr::from_sexpr(filtered[1])?);
                let mut arms = Vec::new();
                for arm_sexpr in &filtered[2..] {
                    arms.push(MatchArm::from_sexpr(arm_sexpr)?);
                }
                Ok(Expr::Match {
                    expr,
                    arms,
                    span,
                })
            }
            "comptime" => Ok(Expr::Comptime {
                expr: Box::new(Expr::from_sexpr(filtered[1])?),
                span,
            }),
            "rune" => {
                // Rune literal: (rune 'x')
                Ok(Expr::Literal(Literal::from_sexpr(sexpr)?, span))
            }
            _ => Err(ParseError {
                message: format!("Unknown expression: {}", head),
            }),
        }
    }
}

impl FromSExpr for FieldInit {
    fn from_sexpr(sexpr: &SExpr) -> Result<Self> {
        let list = sexpr.as_list()?;
        if list.len() < 2 {
            return Err(ParseError {
                message: "Field init requires at least 2 elements".to_string(),
            });
        }

        let span = parse_span(list);
        let filtered = filter_metadata(list);

        if filtered[0].as_symbol()? != "field-init" {
            return Err(ParseError {
                message: "Expected 'field-init'".to_string(),
            });
        }

        let (name, value_idx) = if filtered.len() == 2 {
            (None, 1)
        } else {
            (
                Some(Ident {
                    name: filtered[1].as_symbol()?.to_string(),
                    span: dummy_span(),
                }),
                2,
            )
        };

        let value = Box::new(Expr::from_sexpr(filtered[value_idx])?);

        Ok(FieldInit {
            name,
            value,
            span,
        })
    }
}

impl FromSExpr for MatchArm {
    fn from_sexpr(sexpr: &SExpr) -> Result<Self> {
        let list = sexpr.as_list()?;
        if list.len() < 3 {
            return Err(ParseError {
                message: "Match arm requires at least 3 elements".to_string(),
            });
        }

        let span = parse_span(list);
        let filtered = filter_metadata(list);

        if filtered[0].as_symbol()? != "arm" {
            return Err(ParseError {
                message: "Expected 'arm'".to_string(),
            });
        }

        let pattern = Pattern::from_sexpr(filtered[1])?;
        let body = Box::new(Expr::from_sexpr(filtered[2])?);

        Ok(MatchArm {
            pattern,
            body,
            span,
        })
    }
}

impl FromSExpr for Pattern {
    fn from_sexpr(sexpr: &SExpr) -> Result<Self> {
        let list = sexpr.as_list()?;
        if list.is_empty() {
            return Err(ParseError {
                message: "Pattern requires at least 1 element".to_string(),
            });
        }

        let span = parse_span(list);
        let filtered = filter_metadata(list);

        let head = filtered[0].as_symbol()?;
        match head {
            "pattern-wildcard" => Ok(Pattern::Wildcard(span)),
            "pattern-literal" => Ok(Pattern::Literal(
                Literal::from_sexpr(filtered[1])?,
                span,
            )),
            "pattern-ident" => Ok(Pattern::Ident(Ident {
                name: filtered[1].as_symbol()?.to_string(),
                span,
            })),
            "pattern-tuple" => {
                let mut patterns = Vec::new();
                for pattern_sexpr in &filtered[1..] {
                    patterns.push(Pattern::from_sexpr(pattern_sexpr)?);
                }
                Ok(Pattern::Tuple(patterns, span))
            }
            "pattern-enum" => {
                let name = Ident {
                    name: filtered[1].as_symbol()?.to_string(),
                    span: dummy_span(),
                };
                let mut fields = Vec::new();
                for field_sexpr in &filtered[2..] {
                    fields.push(Pattern::from_sexpr(field_sexpr)?);
                }
                Ok(Pattern::Enum {
                    name,
                    fields,
                    span,
                })
            }
            "pattern-expr" => Ok(Pattern::Expr(Box::new(Expr::from_sexpr(filtered[1])?))),
            _ => Err(ParseError {
                message: format!("Unknown pattern: {}", head),
            }),
        }
    }
}

impl FromSExpr for Literal {
    fn from_sexpr(sexpr: &SExpr) -> Result<Self> {
        match sexpr {
            SExpr::Integer(n) => Ok(Literal::Integer(*n)),
            SExpr::Float(f) => Ok(Literal::Float(*f)),
            SExpr::String(s) => Ok(Literal::String(s.clone())),
            SExpr::Symbol(s) => {
                if s == "True" {
                    Ok(Literal::Bool(true))
                } else if s == "False" {
                    Ok(Literal::Bool(false))
                } else {
                    Err(ParseError {
                        message: format!("Unknown literal: {}", s),
                    })
                }
            }
            SExpr::List(list) => {
                // Check for (rune 'x') form
                if list.len() == 2
                    && let Ok(symbol) = list[0].as_symbol()
                        && symbol == "rune" {
                            let rune_str = list[1].as_string()?;
                            if rune_str.len() == 1 {
                                return Ok(Literal::Rune(rune_str.chars().next().unwrap()));
                            }
                        }
                Err(ParseError {
                    message: format!("Expected literal, got {:?}", sexpr),
                })
            }
        }
    }
}

impl BinOp {
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "+" => Ok(BinOp::Add),
            "-" => Ok(BinOp::Sub),
            "*" => Ok(BinOp::Mul),
            "/" => Ok(BinOp::Div),
            "%" => Ok(BinOp::Mod),
            "==" => Ok(BinOp::Eq),
            "!=" => Ok(BinOp::Ne),
            "<" => Ok(BinOp::Lt),
            "<=" => Ok(BinOp::Le),
            ">" => Ok(BinOp::Gt),
            ">=" => Ok(BinOp::Ge),
            "&&" => Ok(BinOp::And),
            "||" => Ok(BinOp::Or),
            "&" => Ok(BinOp::BitAnd),
            "|" => Ok(BinOp::BitOr),
            "<<" => Ok(BinOp::LShift),
            ">>" => Ok(BinOp::RShift),
            "++" => Ok(BinOp::Concat),
            "=" => Ok(BinOp::Assign),
            "+=" => Ok(BinOp::AddAssign),
            "-=" => Ok(BinOp::SubAssign),
            "*=" => Ok(BinOp::MulAssign),
            "/=" => Ok(BinOp::DivAssign),
            "%=" => Ok(BinOp::ModAssign),
            "++=" => Ok(BinOp::ConcatAssign),
            _ => Err(ParseError {
                message: format!("Unknown binary operator: {}", s),
            }),
        }
    }
}

impl UnOp {
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "-" => Ok(UnOp::Neg),
            "!" => Ok(UnOp::Not),
            "~" => Ok(UnOp::BitNot),
            _ => Err(ParseError {
                message: format!("Unknown unary operator: {}", s),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sexpr::ToSExpr;

    #[test]
    fn test_parse_integer() {
        let sexpr = SExpr::parse("42").unwrap();
        assert_eq!(sexpr, SExpr::Integer(42));
    }

    #[test]
    fn test_parse_simple_list() {
        let sexpr = SExpr::parse("(+ 1 2)").unwrap();
        match sexpr {
            SExpr::List(items) => {
                assert_eq!(items.len(), 3);
                assert_eq!(items[0], SExpr::Symbol("+".to_string()));
                assert_eq!(items[1], SExpr::Integer(1));
                assert_eq!(items[2], SExpr::Integer(2));
            }
            _ => panic!("Expected list"),
        }
    }

    #[test]
    fn test_round_trip_literal() {
        let lit = Literal::Integer(42);
        let sexpr_str = lit.to_sexpr();
        let sexpr = SExpr::parse(&sexpr_str).unwrap();
        let lit2 = Literal::from_sexpr(&sexpr).unwrap();
        assert_eq!(lit, lit2);
    }

    #[test]
    fn test_span_preservation() {
        use crate::sexpr::print_ast_with_spans;
        
        // Create a simple AST with specific spans
        let func = FunctionDef {
            visibility: Visibility::Internal,
            name: Ident {
                name: "test".to_string(),
                span: Span::new(0, 4),
            },
            const_params: vec![],
            params: vec![],
            return_type: None,
            body: Block {
                stmts: vec![],
                span: Span::new(7, 9),
            },
            span: Span::new(0, 9),
        };
        
        let ast = vec![TopLevel::Function(func.clone())];
        
        // Convert to S-expression with spans
        let sexpr_str = print_ast_with_spans(&ast);
        
        // Verify spans are in output
        assert!(sexpr_str.contains(":span (0 9)"));
        assert!(sexpr_str.contains(":span (7 9)"));
        
        // Parse back
        let sexpr = SExpr::parse(&sexpr_str).unwrap();
        let ast2 = Vec::<TopLevel>::from_sexpr(&sexpr).unwrap();
        
        // Verify spans were preserved
        if let TopLevel::Function(func2) = &ast2[0] {
            assert_eq!(func2.span, Span::new(0, 9));
            assert_eq!(func2.body.span, Span::new(7, 9));
        } else {
            panic!("Expected function");
        }
    }
    
    #[test]
    fn test_round_trip_without_spans() {
        use crate::sexpr::print_ast;
        
        // Create a simple AST
        let func = FunctionDef {
            visibility: Visibility::Internal,
            name: Ident {
                name: "test".to_string(),
                span: Span::new(0, 4),
            },
            const_params: vec![],
            params: vec![],
            return_type: None,
            body: Block {
                stmts: vec![],
                span: Span::new(7, 9),
            },
            span: Span::new(0, 9),
        };
        
        let ast = vec![TopLevel::Function(func.clone())];
        
        // Convert to S-expression without spans
        let sexpr_str = print_ast(&ast);
        
        // Verify spans are NOT in output
        assert!(!sexpr_str.contains(":span"));
        
        // Parse back
        let sexpr = SExpr::parse(&sexpr_str).unwrap();
        let ast2 = Vec::<TopLevel>::from_sexpr(&sexpr).unwrap();
        
        // Verify it parsed successfully (with dummy spans)
        if let TopLevel::Function(func2) = &ast2[0] {
            assert_eq!(func2.span, Span::new(0, 0)); // dummy span
        } else {
            panic!("Expected function");
        }
    }
}
