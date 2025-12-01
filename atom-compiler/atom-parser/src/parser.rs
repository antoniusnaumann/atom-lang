use crate::{
    error::{ParseError, ParseResult},
    token::{Token, TokenKind},
};
use atom_ast::{self, Span};
use atom_ast::*;

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    filename: Option<String>,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { 
            tokens, 
            pos: 0,
            filename: None,
        }
    }

    pub fn new_with_filename(tokens: Vec<Token>, filename: String) -> Self {
        Self { 
            tokens, 
            pos: 0,
            filename: Some(filename),
        }
    }

    fn is_test_file(&self) -> bool {
        self.filename
            .as_ref()
            .map(|f| f.ends_with(".test.atom"))
            .unwrap_or(false)
    }

    pub fn parse(&mut self) -> ParseResult<Vec<TopLevel>> {
        let mut items = Vec::new();

        while !self.is_at_end() {
            // Skip semicolons at top level
            if self.check(&TokenKind::NewlineOrSemi) {
                self.advance();
                continue;
            }

            items.push(self.parse_top_level()?);
        }

        // Validate: check for top-level statements in non-test files
        self.validate_top_level_items(&items)?;

        Ok(items)
    }

    fn validate_top_level_items(&self, items: &[TopLevel]) -> ParseResult<()> {
        if !self.is_test_file() {
            for item in items {
                if let TopLevel::Statement(stmt) = item {
                    // Get span from the statement
                    let span = match stmt {
                        Stmt::Expression(expr) => expr.span(),
                        Stmt::VarDecl(decl) => decl.span,
                    };
                    return Err(ParseError::new(
                        "Top-level statements are only allowed in .test.atom files",
                        span,
                    ));
                }
            }
        }
        Ok(())
    }

    fn parse_top_level(&mut self) -> ParseResult<TopLevel> {
        let start = self.current_span();

        // Check for import first (no visibility prefix)
        // Import syntax: valueIdent::* or valueIdent::(item1, item2)
        // Package names are lowercase (matrix, physics, etc.)
        if self.check(&TokenKind::ValueIdent) {
            // Peek ahead to see if this is an import
            let mut parser_copy = self.clone();
            parser_copy.advance(); // Skip ValueIdent
            if parser_copy.check(&TokenKind::ColonColon) {
                // This is an import
                let name = self.expect_value_ident()?;
                self.expect(&TokenKind::ColonColon)?;
                return self.parse_import(name, start);
            }
        }

        // Check for visibility prefix
        let visibility = if self.match_token(&TokenKind::Plus) {
            Visibility::Public
        } else if self.match_token(&TokenKind::Minus) {
            Visibility::FilePrivate
        } else {
            Visibility::Internal
        };

        // Next token determines what we're parsing
        if self.check(&TokenKind::TypeIdent) {
            // Could be struct or enum definition
            let name = self.expect_type_ident()?;
            
            if !self.check(&TokenKind::LParen) {
                return Err(self.error("Expected '(' after type name"));
            }

            // Peek ahead to distinguish struct from enum
            // Enums have cases (TypeIdent), structs have fields (valueIdent or ..)
            if self.is_enum_definition() {
                self.parse_enum_def(visibility, name, start)
            } else {
                self.parse_struct_def(visibility, name, start)
            }
        } else if self.check(&TokenKind::ValueIdent) {
            // Could be function definition, variable declaration, or statement (function call)
            // Save position in case we need to backtrack
            let saved_pos = self.pos;
            let name = self.expect_value_ident()?;
            
            if self.check(&TokenKind::LParen) {
                // Could be function definition or function call
                // Try to parse as function definition first
                match self.parse_function_def(visibility, name, start) {
                    Ok(func) => Ok(func),
                    Err(_) => {
                        // Not a function definition, restore and parse as statement
                        self.pos = saved_pos;
                        let stmt = self.parse_statement()?;
                        Ok(TopLevel::Statement(stmt))
                    }
                }
            } else {
                // Variable/constant declaration or statement
                // Try variable declaration first
                match self.parse_top_level_var_decl(visibility, name, start) {
                    Ok(var) => Ok(var),
                    Err(_) => {
                        // Not a variable declaration, restore and parse as statement  
                        self.pos = saved_pos;
                        let stmt = self.parse_statement()?;
                        Ok(TopLevel::Statement(stmt))
                    }
                }
            }
        } else if self.check(&TokenKind::String) {
            // Test block with name
            self.parse_test_block(start)
        } else if self.check(&TokenKind::LBrace) {
            // Anonymous test block or top-level code in test files
            self.parse_test_block(start)
        } else {
            // Try to parse as a statement (for test files)
            // This allows bare expressions like assert(...) at top level
            match self.parse_statement() {
                Ok(stmt) => Ok(TopLevel::Statement(stmt)),
                Err(_) => Err(self.error("Expected top-level item (struct, enum, function, variable, or statement)"))
            }
        }
    }

    fn is_enum_definition(&self) -> bool {
        // Look ahead to see if this is an enum vs struct
        // Enums have cases (uppercase TypeIdent after parens/semicolon)
        // Structs have fields (lowercase valueIdent or ..)
        
        let mut parser_copy = self.clone();
        
        parser_copy.expect(&TokenKind::LParen).ok();
        
        // Skip newlines
        while parser_copy.match_token(&TokenKind::NewlineOrSemi) {}
        
        // Skip past type parameters if any (before semicolon)
        let mut depth = 0;
        while !parser_copy.is_at_end() {
            if parser_copy.check(&TokenKind::NewlineOrSemi) && depth == 0 {
                // Could be a semicolon separator or just a newline
                // Check if this looks like it's separating type params from cases
                parser_copy.advance();
                while parser_copy.match_token(&TokenKind::NewlineOrSemi) {}
                break;
            }
            if parser_copy.check(&TokenKind::RParen) && depth == 0 {
                break;
            }
            if parser_copy.check(&TokenKind::LParen) {
                depth += 1;
            } else if parser_copy.check(&TokenKind::RParen) {
                depth -= 1;
            }
            parser_copy.advance();
        }
        
        // Now check what's next - should be either TypeIdent (enum case) or valueIdent/.. (struct field)
        parser_copy.check(&TokenKind::TypeIdent)
    }

    fn parse_import(&mut self, namespace: Ident, start: Span) -> ParseResult<TopLevel> {
        // Already consumed: TypeIdent::
        // Now expecting either * or (item1, item2, ...)
        
        let items = if self.match_token(&TokenKind::Star) {
            ImportItems::All
        } else if self.match_token(&TokenKind::LParen) {
            // Parse comma-separated list of identifiers
            let mut names = Vec::new();
            
            while self.match_token(&TokenKind::NewlineOrSemi) {}
            
            if !self.check(&TokenKind::RParen) {
                loop {
                    let name = self.expect_value_ident()?;
                    names.push(name);
                    
                    while self.match_token(&TokenKind::NewlineOrSemi) {}
                    
                    if !self.match_token(&TokenKind::Comma) {
                        break;
                    }
                    
                    while self.match_token(&TokenKind::NewlineOrSemi) {}
                }
            }
            
            self.expect(&TokenKind::RParen)?;
            ImportItems::Named(names)
        } else {
            return Err(self.error("Expected '*' or '(' after '::' in import declaration"));
        };
        
        let span = start.merge(self.previous_span());
        
        Ok(TopLevel::Import(ImportDecl {
            namespace,
            items,
            span,
        }))
    }

    fn parse_struct_def(&mut self, visibility: Visibility, name: Ident, start: Span) -> ParseResult<TopLevel> {
        self.expect(&TokenKind::LParen)?;
        
        // Skip leading newlines/semicolons
        while self.match_token(&TokenKind::NewlineOrSemi) {}
        
        // Parse type parameters if present
        let (type_params, fields) = self.parse_struct_params_and_fields()?;
        
        self.expect(&TokenKind::RParen)?;
        
        let span = start.merge(self.previous_span());
        
        Ok(TopLevel::Struct(StructDef {
            visibility,
            name,
            type_params,
            fields,
            span,
        }))
    }

    fn parse_struct_params_and_fields(&mut self) -> ParseResult<(Vec<TypeParam>, Vec<Field>)> {
        let type_params = Vec::new();
        let mut fields = Vec::new();
        
        // Check for const parameters (separated by semicolon)
        while !self.check(&TokenKind::RParen) && !self.check(&TokenKind::Semicolon) {
            // Could be type param or field - for now, treat as field
            fields.push(self.parse_field()?);
            
            if !self.match_token(&TokenKind::Comma) && !self.match_token(&TokenKind::NewlineOrSemi) {
                break;
            }
            
            // Skip trailing newlines/semicolons
            while self.match_token(&TokenKind::NewlineOrSemi) {}
        }
        
        // TODO: Handle type parameters properly
        
        Ok((type_params, fields))
    }

    fn parse_field(&mut self) -> ParseResult<Field> {
        let start = self.current_span();
        
        // Check for spread operator (..)
        if self.match_token(&TokenKind::DotDot) {
            let ty = self.parse_type()?;
            return Ok(Field {
                name: None,
                ty: Box::new(ty),
                span: start.merge(self.previous_span()),
            });
        }
        
        // Field: name Type or just Type (for tuple-like)
        if self.check(&TokenKind::ValueIdent) && self.peek_ahead(1).is_some_and(|t| 
            matches!(t.kind, TokenKind::TypeIdent | TokenKind::ValueIdent)) {
            // Named field
            let name = self.expect_value_ident()?;
            let ty = self.parse_type()?;
            
            Ok(Field {
                name: Some(name),
                ty: Box::new(ty),
                span: start.merge(self.previous_span()),
            })
        } else {
            // Unnamed field (tuple-like)
            let ty = self.parse_type()?;
            Ok(Field {
                name: None,
                ty: Box::new(ty),
                span: start.merge(self.previous_span()),
            })
        }
    }

    fn parse_enum_def(&mut self, visibility: Visibility, name: Ident, start: Span) -> ParseResult<TopLevel> {
        self.expect(&TokenKind::LParen)?;
        
        // Skip leading newlines/semicolons
        while self.match_token(&TokenKind::NewlineOrSemi) {}
        
        let mut type_params = Vec::new();
        let mut cases = Vec::new();
        
        // Check if we have type parameters by looking at the pattern
        // Type params: lowercase ident optionally followed by type or = default
        // Cases: uppercase ident (TypeIdent)
        // Look ahead to distinguish
        let has_type_params = if self.check(&TokenKind::ValueIdent) {
            // Could be type param like `t` or `e String` or `e = String`
            // Check what comes after
            let next = self.peek_ahead(1);
            match next.map(|t| &t.kind) {
                Some(TokenKind::Comma) | Some(TokenKind::NewlineOrSemi) | Some(TokenKind::Semicolon) => {
                    // Pattern: valueIdent , or valueIdent ; - could be type param like `t;`
                    // Need to look further to see if there's an explicit semicolon separator
                    // For now, assume if we see any explicit looking separator pattern, it's type params
                    true
                },
                Some(TokenKind::TypeIdent) | Some(TokenKind::Eq) => {
                    // Pattern: valueIdent TypeIdent or valueIdent = - definitely type param
                    true
                },
                _ => false,
            }
        } else if self.check(&TokenKind::TypeIdent) {
            // Could be:  
            // 1. Type param that's a type: `String;` 
            // 2. Enum case: `Some(t)`
            // Distinguish by looking for explicit semicolon separator
            // If we see TypeIdent followed eventually by an explicit `;` before any actual cases, it's params
            // For now, be conservative: TypeIdent at start = enum case (no type params)
            false
        } else {
            false
        };
        
        if has_type_params {
            // Parse type parameters until we hit a semicolon separator
            loop {
                type_params.push(self.parse_type_param()?);
                
                // Check if we're at the separator
                if self.check(&TokenKind::Semicolon) || 
                   (self.check(&TokenKind::NewlineOrSemi) && self.is_semicolon_separator()) {
                    self.advance(); // Consume the separator
                    while self.match_token(&TokenKind::NewlineOrSemi) {}
                    break;
                }
                
                // Otherwise expect comma
                if !self.match_token(&TokenKind::Comma) {
                    // No comma, we might be at RParen (no cases)
                    break;
                }
                
                // Skip newlines after comma
                while self.match_token(&TokenKind::NewlineOrSemi) {}
            }
        }
        
        // Parse enum cases
        while !self.check(&TokenKind::RParen) && !self.is_at_end() {
            cases.push(self.parse_enum_case()?);
            
            // After each case, there may be a comma or newline
            if self.match_token(&TokenKind::Comma) || self.match_token(&TokenKind::NewlineOrSemi) {
                // Skip additional newlines
                while self.match_token(&TokenKind::NewlineOrSemi) {}
            } else {
                // No separator, stop
                break;
            }
        }
        
        self.expect(&TokenKind::RParen)?;
        
        let span = start.merge(self.previous_span());
        
        Ok(TopLevel::Enum(EnumDef {
            visibility,
            name,
            type_params,
            cases,
            span,
        }))
    }
    
    // Helper to check if a NewlineOrSemi is actually a semicolon separator (not just a newline)
    // Heuristic: if next token after NewlineOrSemi is TypeIdent (enum case), previous was likely just newline
    // if next is valueIdent or more type stuff, it's likely a separator
    fn is_semicolon_separator(&self) -> bool {
        // Look ahead: if next is TypeIdent at start of line, this is probably just a newline between cases
        // if next is anything else or EOF or RParen, this might be a separator
        // This is a heuristic and not perfect, but works for common cases
        if let Some(next) = self.peek_ahead(1) {
            !matches!(next.kind, TokenKind::TypeIdent)
        } else {
            true
        }
    }

    fn parse_type_param(&mut self) -> ParseResult<TypeParam> {
        let start = self.current_span();
        
        // Type parameter can be:
        // - Just a type: `t` or `String`
        // - Named with type: `e String`
        // - With default: `e = String`
        
        if self.check(&TokenKind::ValueIdent) || self.check(&TokenKind::TypeIdent) {
            let name_or_type = if self.check(&TokenKind::ValueIdent) {
                self.expect_value_ident()?
            } else {
                self.expect_type_ident()?
            };
            
            // Check for = (default value)
            if self.match_token(&TokenKind::Eq) {
                let default_type = Box::new(self.parse_type()?);
                return Ok(TypeParam {
                    name: Some(name_or_type),
                    ty: None,
                    default: Some(default_type),
                    span: start.merge(self.previous_span()),
                });
            }
            
            // Check if next is a type (meaning this is a named parameter)
            if !self.check(&TokenKind::Comma) && !self.check(&TokenKind::Semicolon) && 
               !self.check(&TokenKind::RParen) && !self.check(&TokenKind::NewlineOrSemi) {
                let ty = Box::new(self.parse_type()?);
                return Ok(TypeParam {
                    name: Some(name_or_type),
                    ty: Some(ty),
                    default: None,
                    span: start.merge(self.previous_span()),
                });
            }
            
            // Just a type parameter name
            return Ok(TypeParam {
                name: Some(name_or_type),
                ty: None,
                default: None,
                span: start.merge(self.previous_span()),
            });
        }
        
        // Otherwise parse as type
        let ty = Box::new(self.parse_type()?);
        Ok(TypeParam {
            name: None,
            ty: Some(ty),
            default: None,
            span: start.merge(self.previous_span()),
        })
    }

    fn parse_enum_case(&mut self) -> ParseResult<EnumCase> {
        let start = self.current_span();
        let name = self.expect_type_ident()?;
        
        let fields = if self.match_token(&TokenKind::LParen) {
            let mut fields = Vec::new();
            while !self.check(&TokenKind::RParen) {
                fields.push(Box::new(self.parse_type()?));
                if !self.match_token(&TokenKind::Comma) {
                    break;
                }
            }
            self.expect(&TokenKind::RParen)?;
            fields
        } else {
            Vec::new()
        };
        
        Ok(EnumCase {
            name,
            fields,
            span: start.merge(self.previous_span()),
        })
    }

    fn parse_function_def(&mut self, visibility: Visibility, name: Ident, start: Span) -> ParseResult<TopLevel> {
        self.expect(&TokenKind::LParen)?;
        
        // Parse parameters
        let (const_params, params) = self.parse_function_params()?;
        
        self.expect(&TokenKind::RParen)?;
        
        // Parse return type if present (before the body)
        let return_type = if !self.check(&TokenKind::LBrace) {
            Some(Box::new(self.parse_return_type()?))
        } else {
            None
        };
        
        // Parse body
        let body = self.parse_block()?;

        
        let span = start.merge(self.previous_span());
        
        Ok(TopLevel::Function(FunctionDef {
            visibility,
            name,
            const_params,
            params,
            return_type,
            body,
            span,
        }))
    }

    fn parse_function_params(&mut self) -> ParseResult<(Vec<Param>, Vec<Param>)> {
        let mut const_params = Vec::new();
        let mut params = Vec::new();
        
        // Skip leading newlines/semicolons
        while self.match_token(&TokenKind::NewlineOrSemi) {}
        
        let mut seen_semicolon = false;
        
        while !self.check(&TokenKind::RParen) {
            if self.match_token(&TokenKind::Semicolon) {
                // Everything before semicolon goes to const_params, everything after to params
                seen_semicolon = true;
                const_params = params.clone();
                params.clear();
                
                // Skip newlines after semicolon
                while self.match_token(&TokenKind::NewlineOrSemi) {}
                continue;
            }
            
            let param = self.parse_param()?;
            params.push(param);
            
            if !self.match_token(&TokenKind::Comma) && !self.match_token(&TokenKind::NewlineOrSemi) {
                break;
            }
            
            // Skip trailing newlines/semicolons
            while self.match_token(&TokenKind::NewlineOrSemi) {}
        }
        
        if seen_semicolon {
            Ok((const_params, params))
        } else {
            // No semicolon = all params are regular params
            Ok((Vec::new(), params))
        }
    }

    fn parse_param(&mut self) -> ParseResult<Param> {
        let start = self.current_span();
        
        // Check for reference prefix: &name Type
        let is_ref = self.match_token(&TokenKind::And);
        
        let name = self.expect_value_ident()?;
        
        // Parse type if present
        let ty = if !self.check(&TokenKind::Comma) && !self.check(&TokenKind::RParen) && 
                    !self.check(&TokenKind::Eq) && !self.check(&TokenKind::Semicolon) {
            let inner_ty = self.parse_type()?;
            if is_ref {
                // Wrap the type in a Reference type
                Some(Box::new(Type::Reference {
                    inner: Box::new(inner_ty),
                    span: start.merge(self.previous_span()),
                }))
            } else {
                Some(Box::new(inner_ty))
            }
        } else if is_ref {
            // Reference without explicit type - error
            return Err(self.error("Reference parameter requires an explicit type"));
        } else {
            None
        };
        
        // Parse default value if present
        let default = if self.match_token(&TokenKind::Eq) {
            Some(Box::new(self.parse_expression()?))
        } else {
            None
        };
        
        Ok(Param {
            name,
            ty,
            default,
            span: start.merge(self.previous_span()),
        })
    }

    fn parse_top_level_var_decl(&mut self, visibility: Visibility, name: Ident, start: Span) -> ParseResult<TopLevel> {
        // Variable declaration: name := value or name: Type = value or name: Type
        let (ty, init) = if self.match_token(&TokenKind::ColonEq) {
            // Inferred type
            (None, Some(Box::new(self.parse_expression()?)))
        } else if self.match_token(&TokenKind::Colon) {
            // Explicit type
            let ty = Some(Box::new(self.parse_type()?));
            let init = if self.match_token(&TokenKind::Eq) {
                Some(Box::new(self.parse_expression()?))
            } else {
                None
            };
            (ty, init)
        } else {
            return Err(self.error("Expected ':=' or ':' in variable declaration"));
        };
        
        Ok(TopLevel::Variable(VarDecl {
            visibility,
            is_const: true, // Top-level declarations are constants
            names: vec![name],  // Top-level declarations are always single names
            ty,
            init,
            span: start.merge(self.previous_span()),
        }))
    }

    fn parse_test_block(&mut self, start: Span) -> ParseResult<TopLevel> {
        let name = if self.check(&TokenKind::String) {
            let tok = self.advance();
            Some(tok.text.clone())
        } else {
            None
        };
        
        let body = self.parse_block()?;
        
        Ok(TopLevel::TestBlock(TestBlock {
            name,
            body,
            span: start.merge(self.previous_span()),
        }))
    }

    fn parse_block(&mut self) -> ParseResult<Block> {
        let start = self.current_span();
        self.expect(&TokenKind::LBrace)?;
        
        // Skip leading newlines/semicolons
        while self.match_token(&TokenKind::NewlineOrSemi) {}
        
        // Check if this looks like an inline match block: TypeIdent { ... }, TypeIdent { ... }
        // Pattern: TypeIdent followed by LBrace
        let is_inline_match = self.check(&TokenKind::TypeIdent) && 
                             self.peek_ahead(1).map(|t| t.kind == TokenKind::LBrace).unwrap_or(false);
        
        if is_inline_match {
            // Parse as match arms instead of statements
            let mut stmts = Vec::new();
            while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
                // Skip newlines
                while self.match_token(&TokenKind::NewlineOrSemi) {}
                
                if self.check(&TokenKind::RBrace) {
                    break;
                }
                
                // Parse pattern { body }
                let pattern = self.expect_type_ident()?;
                self.expect(&TokenKind::LBrace)?;
                
                // Parse the body as a block of statements
                let mut arm_stmts = Vec::new();
                while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
                    // Skip newlines
                    if self.match_token(&TokenKind::NewlineOrSemi) {
                        continue;
                    }
                    
                    if self.check(&TokenKind::RBrace) {
                        break;
                    }
                    
                    arm_stmts.push(self.parse_statement()?);
                    
                    // Optional semicolon/newline after statement
                    self.match_token(&TokenKind::NewlineOrSemi);
                }
                
                self.expect(&TokenKind::RBrace)?;
                
                // Create a block expression for the match arm body
                let arm_body = Expr::Block(Block {
                    stmts: arm_stmts,
                    span: start.merge(self.previous_span()),
                });
                
                // Create an expression statement that represents the match arm
                // For now, we'll represent it as: pattern(arm_body)
                // This is a bit of a hack - ideally we'd have a MatchArm statement type
                let match_arm_expr = Expr::Call {
                    func: Box::new(Expr::Ident(pattern)),
                    args: vec![arm_body],
                    span: start.merge(self.previous_span()),
                };
                stmts.push(Stmt::Expression(match_arm_expr));
                
                // Check for comma separator
                if !self.match_token(&TokenKind::Comma) {
                    // No comma, check for newline or end of block
                    if !self.check(&TokenKind::RBrace) {
                        self.match_token(&TokenKind::NewlineOrSemi);
                    }
                }
            }
            
            self.expect(&TokenKind::RBrace)?;
            
            return Ok(Block {
                stmts,
                span: start.merge(self.previous_span()),
            });
        }
        
        // Regular block with statements
        let mut stmts = Vec::new();
        
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            // Skip empty statements
            if self.match_token(&TokenKind::NewlineOrSemi) {
                continue;
            }
            
            stmts.push(self.parse_statement()?);
            
            // Optional semicolon/newline after statement
            self.match_token(&TokenKind::NewlineOrSemi);
        }
        
        self.expect(&TokenKind::RBrace)?;
        
        Ok(Block {
            stmts,
            span: start.merge(self.previous_span()),
        })
    }

    fn parse_match_arms(&mut self) -> ParseResult<Vec<MatchArm>> {
        self.expect(&TokenKind::LBrace)?;
        
        let mut arms = Vec::new();
        
        // Skip leading newlines/semicolons
        while self.match_token(&TokenKind::NewlineOrSemi) {}
        
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            let arm_start = self.current_span();
            
            // Try to parse as pattern first, but if we don't see { after,
            // it's actually an expression (guard-style match)
            let pattern = self.parse_match_pattern_or_expr()?;
            
            // Expect { for the arm body
            self.expect(&TokenKind::LBrace)?;
            
            // Parse the body expression
            // The body is a single expression (block content)
            let body = if self.check(&TokenKind::RBrace) {
                // Empty body - use unit literal
                Box::new(Expr::Tuple(vec![], self.current_span()))
            } else {
                // Parse as expression (could be a block or single expression)
                // For simplicity, collect statements until we hit RBrace
                let body_start = self.current_span();
                let mut stmts = Vec::new();
                
                while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
                    if self.match_token(&TokenKind::NewlineOrSemi) {
                        continue;
                    }
                    if self.check(&TokenKind::RBrace) {
                        break;
                    }
                    stmts.push(self.parse_statement()?);
                    self.match_token(&TokenKind::NewlineOrSemi);
                }
                
                let body_span = body_start.merge(self.current_span());
                Box::new(Expr::Block(Block { stmts, span: body_span }))
            };
            
            self.expect(&TokenKind::RBrace)?;
            
            let span = arm_start.merge(self.previous_span());
            arms.push(MatchArm { pattern, body, span });
            
            // Skip newlines/commas between arms
            while self.match_token(&TokenKind::NewlineOrSemi) || self.match_token(&TokenKind::Comma) {}
        }
        
        self.expect(&TokenKind::RBrace)?;
        
        Ok(arms)
    }

    fn parse_match_pattern_or_expr(&mut self) -> ParseResult<Pattern> {
        // Save position in case we need to backtrack
        let saved_pos = self.pos;
        
        // Try parsing as a pattern first
        if let Ok(pattern) = self.parse_pattern() {
            // Check if next token is {
            if self.check(&TokenKind::LBrace) {
                return Ok(pattern);
            }
        }
        
        // Restore position and parse as expression instead
        self.pos = saved_pos;
        let expr = self.parse_expression()?;
        Ok(Pattern::Expr(Box::new(expr)))
    }

    fn parse_pattern(&mut self) -> ParseResult<Pattern> {
        let start = self.current_span();
        
        // Parse the first (base) pattern
        let base_pattern = self.parse_base_pattern()?;
        
        // Check for alternative patterns (|)
        if self.check(&TokenKind::Or) {
            let mut alternatives = vec![base_pattern];
            
            while self.match_token(&TokenKind::Or) {
                alternatives.push(self.parse_base_pattern()?);
            }
            
            Ok(Pattern::Alternative(alternatives, start.merge(self.previous_span())))
        } else {
            Ok(base_pattern)
        }
    }

    fn parse_base_pattern(&mut self) -> ParseResult<Pattern> {
        let start = self.current_span();
        
        // Wildcard: _
        if self.check(&TokenKind::ValueIdent) {
            let is_wildcard = self.peek().text == "_";
            if is_wildcard {
                let span = self.advance().span;
                return Ok(Pattern::Wildcard(span));
            }
        }
        
        // Integer literal
        if self.check(&TokenKind::Integer) {
            let tok = self.advance();
            let value = self.parse_integer_literal(&tok.text)
                .map_err(|_| ParseError::new("Invalid integer literal in pattern", tok.span))?;
            return Ok(Pattern::Literal(Literal::Integer(value), tok.span));
        }
        
        // String literal
        if self.check(&TokenKind::String) {
            let tok = self.advance();
            return Ok(Pattern::Literal(Literal::String(tok.text.clone()), tok.span));
        }
        
        // Rune literal
        if self.check(&TokenKind::Rune) {
            let tok = self.advance();
            let ch = tok.text.chars().next()
                .ok_or_else(|| ParseError::new("Empty rune literal in pattern", tok.span))?;
            return Ok(Pattern::Literal(Literal::Rune(ch), tok.span));
        }
        
        // Tuple pattern: (a, b, c)
        if self.match_token(&TokenKind::LParen) {
            let mut patterns = Vec::new();
            while !self.check(&TokenKind::RParen) {
                patterns.push(self.parse_pattern()?);
                if !self.match_token(&TokenKind::Comma) {
                    break;
                }
                while self.match_token(&TokenKind::NewlineOrSemi) {}
            }
            self.expect(&TokenKind::RParen)?;
            return Ok(Pattern::Tuple(patterns, start.merge(self.previous_span())));
        }
        
        // Enum pattern or identifier binding: Some(x), None, x
        if self.check(&TokenKind::TypeIdent) {
            let name = self.expect_type_ident()?;
            
            // Check for enum pattern with fields
            if self.match_token(&TokenKind::LParen) {
                let mut fields = Vec::new();
                while !self.check(&TokenKind::RParen) {
                    fields.push(self.parse_pattern()?);
                    if !self.match_token(&TokenKind::Comma) {
                        break;
                    }
                    while self.match_token(&TokenKind::NewlineOrSemi) {}
                }
                self.expect(&TokenKind::RParen)?;
                return Ok(Pattern::Enum {
                    name,
                    fields,
                    span: start.merge(self.previous_span()),
                });
            }
            
            // Enum pattern without fields
            return Ok(Pattern::Enum {
                name,
                fields: vec![],
                span: start.merge(self.previous_span()),
            });
        }
        
        // Identifier binding: x
        if self.check(&TokenKind::ValueIdent) {
            let name = self.expect_value_ident()?;
            return Ok(Pattern::Ident(name));
        }
        
        Err(self.error("Expected pattern"))
    }

    fn parse_statement(&mut self) -> ParseResult<Stmt> {
        // Check if this is a variable declaration
        if self.check(&TokenKind::ValueIdent) {
            let saved_pos = self.pos;
            self.advance(); // Skip first identifier
            
            // Look ahead for tuple destructuring (comma-separated identifiers)
            // or single variable declaration (:= or :)
            while self.check(&TokenKind::Comma) {
                self.advance(); // Skip comma
                if !self.check(&TokenKind::ValueIdent) {
                    // Not a valid tuple destructuring, restore and parse as expression
                    self.pos = saved_pos;
                    return Ok(Stmt::Expression(self.parse_expression()?));
                }
                self.advance(); // Skip identifier
            }
            
            // Now check for := or :
            if self.check(&TokenKind::ColonEq) || self.check(&TokenKind::Colon) {
                // It's a variable declaration (single or tuple destructuring)
                self.pos = saved_pos;
                return Ok(Stmt::VarDecl(self.parse_var_decl()?));
            }
            
            // Not a declaration, restore position and parse as expression
            self.pos = saved_pos;
        }
        
        // Otherwise it's an expression statement
        Ok(Stmt::Expression(self.parse_expression()?))
    }

    fn parse_var_decl(&mut self) -> ParseResult<VarDecl> {
        let start = self.current_span();
        
        // Parse first identifier
        let first_name = self.expect_value_ident()?;
        
        // Check if this is tuple destructuring (comma after first name)
        let names = if self.check(&TokenKind::Comma) {
            // Tuple destructuring: a, b, c := ...
            let mut names = vec![first_name];
            
            while self.match_token(&TokenKind::Comma) {
                names.push(self.expect_value_ident()?);
            }
            
            names
        } else {
            // Single variable
            vec![first_name]
        };
        
        let (ty, init) = if self.match_token(&TokenKind::ColonEq) {
            // For tuple destructuring with multiple names, try to parse tuple literal
            let init_expr = if names.len() > 1 {
                // Multiple names: parse as comma-separated expressions (tuple literal)
                let mut exprs = vec![self.parse_expression()?];
                while self.match_token(&TokenKind::Comma) {
                    exprs.push(self.parse_expression()?);
                }
                // If we got multiple expressions, wrap in tuple; otherwise keep as-is
                if exprs.len() > 1 {
                    let span = exprs.first().unwrap().span().merge(exprs.last().unwrap().span());
                    Expr::Tuple(exprs, span)
                } else {
                    exprs.into_iter().next().unwrap()
                }
            } else {
                // Single name: parse as normal expression
                self.parse_expression()?
            };
            (None, Some(Box::new(init_expr)))
        } else if self.match_token(&TokenKind::Colon) {
            let ty = Some(Box::new(self.parse_type()?));
            let init = if self.match_token(&TokenKind::Eq) {
                Some(Box::new(self.parse_expression()?))
            } else {
                None
            };
            (ty, init)
        } else {
            return Err(self.error("Expected ':=' or ':' in variable declaration"));
        };
        
        Ok(VarDecl {
            visibility: Visibility::Internal,
            is_const: false,
            names,
            ty,
            init,
            span: start.merge(self.previous_span()),
        })
    }

    fn parse_type(&mut self) -> ParseResult<Type> {
        let start = self.current_span();
        
        // Note: Reference types (&T) are NOT parsed here in parse_type().
        // References are only created through parameter syntax: &name Type
        // This is handled in parse_param() which wraps the type in Type::Reference
        
        // Base type
        let mut ty = if self.match_token(&TokenKind::LParen) {
            // Tuple type or function type
            let mut types = Vec::new();
            
            while !self.check(&TokenKind::RParen) {
                types.push(Box::new(self.parse_type()?));
                if !self.match_token(&TokenKind::Comma) {
                    break;
                }
            }
            
            self.expect(&TokenKind::RParen)?;
            
            // Check if it's a function type: (T, U) { R }
            if self.match_token(&TokenKind::LBrace) {
                let return_type = if !self.check(&TokenKind::RBrace) {
                    Some(Box::new(self.parse_type()?))
                } else {
                    None
                };
                self.expect(&TokenKind::RBrace)?;
                
                Type::Function {
                    params: types,
                    return_type,
                    span: start.merge(self.previous_span()),
                }
            } else {
                // Tuple type
                Type::Tuple(types, start.merge(self.previous_span()))
            }
        } else if self.check(&TokenKind::TypeIdent) {
            let name = self.expect_type_ident()?;
            
            // Check for generic type: Type(params)
            if self.match_token(&TokenKind::LParen) {
                let mut params = Vec::new();
                while !self.check(&TokenKind::RParen) {
                    // Type parameters can be types OR expressions (for const generics like Float(32))
                    // Try to parse as type first, if that fails, try as expression
                    let param_start = self.current_span();
                    
                    // Check if this looks like a type (TypeIdent, ValueIdent for type params, or LParen for tuple)
                    if self.check(&TokenKind::TypeIdent) || self.check(&TokenKind::ValueIdent) || self.check(&TokenKind::LParen) {
                        // Try parsing as type
                        let checkpoint = self.pos;
                        match self.parse_type() {
                            Ok(ty) => {
                                params.push(Box::new(TypeParam {
                                    name: None,
                                    ty: Some(Box::new(ty)),
                                    default: None,
                                    span: param_start.merge(self.previous_span()),
                                }));
                            }
                            Err(_) => {
                                // Failed as type, try as expression (for const generics)
                                self.pos = checkpoint;
                                // For now, just parse integers as a simple case
                                // TODO: Full expression parsing for const generics
                                if self.check(&TokenKind::Integer) {
                                    let tok = self.advance();
                                    // Store as a type param with a "fake" type that's just the number
                                    // This is a hack - ideally TypeParam should support expressions
                                    params.push(Box::new(TypeParam {
                                        name: Some(Ident {
                                            name: tok.text.clone(),
                                            span: tok.span,
                                        }),
                                        ty: None,
                                        default: None,
                                        span: tok.span,
                                    }));
                                } else {
                                    return Err(self.error("Expected type or const expression in generic parameter"));
                                }
                            }
                        }
                    } else if self.check(&TokenKind::Integer) {
                        // Const parameter (like 32 in Float(32))
                        let tok = self.advance();
                        params.push(Box::new(TypeParam {
                            name: Some(Ident {
                                name: tok.text.clone(),
                                span: tok.span,
                            }),
                            ty: None,
                            default: None,
                            span: tok.span,
                        }));
                    } else {
                        return Err(self.error("Expected type or const parameter"));
                    }
                    
                    if !self.match_token(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(&TokenKind::RParen)?;
                
                Type::Generic {
                    name,
                    params,
                    span: start.merge(self.previous_span()),
                }
            } else {
                Type::Named(name)
            }
        } else if self.check(&TokenKind::ValueIdent) {
            // Type parameter reference (lowercase identifier)
            let name = self.expect_value_ident()?;
            Type::Param(name)
        } else {
            return Err(self.error("Expected type"));
        };
        
        // Handle postfix type operators: *, +, ?
        loop {
            if self.match_token(&TokenKind::Star) {
                // Could be variadic (*) or static array (*n)
                // Check if next token is an identifier/expression WITHOUT space
                // For now, simplify: if followed by identifier/number directly, it's static array
                if self.check(&TokenKind::ValueIdent) || self.check(&TokenKind::Integer) {
                    // Static array: T*n
                    let size = Box::new(self.parse_primary_expression()?);
                    ty = Type::StaticArray {
                        element: Box::new(ty),
                        size,
                        span: start.merge(self.previous_span()),
                    };
                } else {
                    // Variadic: T*
                    ty = Type::Variadic {
                        element: Box::new(ty),
                        non_empty: false,
                        span: start.merge(self.previous_span()),
                    };
                }
            } else if self.match_token(&TokenKind::Plus) {
                // Non-empty variadic: T+
                ty = Type::Variadic {
                    element: Box::new(ty),
                    non_empty: true,
                    span: start.merge(self.previous_span()),
                };
            } else if self.match_token(&TokenKind::Question) {
                // Optional type - this is sugar for Option(T)
                // For now, represent as generic
                ty = Type::Generic {
                    name: Ident {
                        name: "Option".to_string(),
                        span: self.previous_span(),
                    },
                    params: vec![Box::new(TypeParam {
                        name: None,
                        ty: Some(Box::new(ty)),
                        default: None,
                        span: self.previous_span(),
                    })],
                    span: start.merge(self.previous_span()),
                };
            } else {
                break;
            }
        }
        
        Ok(ty)
    }

    fn parse_return_type(&mut self) -> ParseResult<Type> {
        let start = self.current_span();
        
        // For return types, we need special handling because:
        // 1. Tuple types like (T, U) might be followed by { which could be confused with function type
        // 2. Relaxed tuple notation T, U is allowed
        // 3. Function types (T) { U } are NOT allowed as bare return types (use closure syntax instead)
        
        let mut ty = if self.check(&TokenKind::LParen) {
            // Parse tuple type but DON'T allow it to become a function type
            self.advance(); // consume (
            let mut types = Vec::new();
            
            while !self.check(&TokenKind::RParen) {
                types.push(Box::new(self.parse_type()?));
                if !self.match_token(&TokenKind::Comma) {
                    break;
                }
            }
            
            self.expect(&TokenKind::RParen)?;
            
            // DO NOT check for { to make it a function type - that's the function body!
            Type::Tuple(types, start.merge(self.previous_span()))
        } else {
            // Parse first type
            self.parse_type()?
        };
        
        // After parsing the base type (whether tuple or simple), check for postfix operators
        // like * (variadic) that can apply to return types
        loop {
            if self.match_token(&TokenKind::Star) {                // Variadic return type: (T, U)* or T*
                ty = Type::Variadic {
                    element: Box::new(ty),
                    non_empty: false,
                    span: start.merge(self.previous_span()),
                };
            } else if self.match_token(&TokenKind::Plus) {
                // Non-empty variadic: (T, U)+ or T+
                ty = Type::Variadic {
                    element: Box::new(ty),
                    non_empty: true,
                    span: start.merge(self.previous_span()),
                };
            } else {
                break;
            }
        }
        
        // Check for relaxed tuple notation: T, U, V (without parentheses)
        // This is only valid for return types, not for parameter types
        if self.check(&TokenKind::Comma) {
            let mut types = vec![Box::new(ty)];
            while self.match_token(&TokenKind::Comma) {
                types.push(Box::new(self.parse_type()?));
            }
            ty = Type::Tuple(types, start.merge(self.previous_span()));
        }
        
        Ok(ty)
    }

    fn parse_expression(&mut self) -> ParseResult<Expr> {
        self.parse_binary_expression(0)
    }

    fn parse_binary_expression(&mut self, min_precedence: u8) -> ParseResult<Expr> {
        let mut left = self.parse_unary_expression()?;
        
        // Check if we're starting a comparison chain
        if let Some((first_op, precedence)) = self.get_binary_op() {
            if precedence >= min_precedence && self.is_comparison_op(first_op) {
                // Potential multi-way comparison - look ahead
                let start_span = left.span();
                
                // Collect all consecutive comparison operators
                let mut operands = vec![left.clone()];
                let mut operators = Vec::new();
                
                let mut current_op = first_op;
                let mut current_prec = precedence;
                
                while current_prec >= min_precedence && self.is_comparison_op(current_op) {
                    self.advance(); // Consume operator
                    operators.push(current_op);
                    
                    // Parse the next operand (without allowing more comparisons at this level)
                    let next_operand = self.parse_binary_expression(current_prec + 1)?;
                    operands.push(next_operand);
                    
                    // Check if there's another comparison operator
                    if let Some((next_op, next_prec)) = self.get_binary_op() {
                        if next_prec == current_prec && self.is_comparison_op(next_op) {
                            // Continue the chain
                            current_op = next_op;
                            current_prec = next_prec;
                        } else {
                            // Different operator or precedence, stop the chain
                            break;
                        }
                    } else {
                        // No more operators
                        break;
                    }
                }
                
                // If we collected multiple comparisons, create a MultiComparison
                if operators.len() > 1 {
                    let end_span = operands.last().unwrap().span();
                    let span = start_span.merge(end_span);
                    left = Expr::MultiComparison {
                        operands,
                        operators,
                        span,
                    };
                } else {
                    // Only one comparison, create a regular Binary expression
                    let span = operands[0].span().merge(operands[1].span());
                    left = Expr::Binary {
                        op: operators[0],
                        left: Box::new(operands[0].clone()),
                        right: Box::new(operands[1].clone()),
                        span,
                    };
                }
            }
        }
        
        // Continue parsing non-comparison binary operators
        while let Some((op, precedence)) = self.get_binary_op() {
            if precedence < min_precedence {
                break;
            }
            
            // Skip if it's a comparison operator (already handled above)
            if self.is_comparison_op(op) {
                break;
            }
            
            self.advance(); // Consume operator
            let right = self.parse_binary_expression(precedence + 1)?;
            
            let span = left.span().merge(right.span());
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }
        
        Ok(left)
    }
    
    /// Check if an operator is a comparison operator
    fn is_comparison_op(&self, op: BinOp) -> bool {
        matches!(op, BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge)
    }

    fn get_binary_op(&self) -> Option<(BinOp, u8)> {
        if self.is_at_end() {
            return None;
        }
        
        let (op, prec) = match &self.peek().kind {
            TokenKind::PlusPlus => (BinOp::Concat, 1),
            TokenKind::OrOr => (BinOp::Or, 2),
            TokenKind::AndAnd => (BinOp::And, 3),
            TokenKind::EqEq => (BinOp::Eq, 4),
            TokenKind::NotEq => (BinOp::Ne, 4),
            TokenKind::Lt => (BinOp::Lt, 5),
            TokenKind::Gt => (BinOp::Gt, 5),
            TokenKind::LtEq => (BinOp::Le, 5),
            TokenKind::GtEq => (BinOp::Ge, 5),
            TokenKind::Or => (BinOp::BitOr, 6),
            TokenKind::And => (BinOp::BitAnd, 7),
            TokenKind::LShift => (BinOp::LShift, 8),
            TokenKind::RShift => (BinOp::RShift, 8),
            TokenKind::Plus => (BinOp::Add, 9),
            TokenKind::Minus => (BinOp::Sub, 9),
            TokenKind::Star => (BinOp::Mul, 10),
            TokenKind::Slash => (BinOp::Div, 10),
            TokenKind::Percent => (BinOp::Mod, 10),
            
            // Assignment operators (lowest precedence, but parse differently)
            TokenKind::Eq => (BinOp::Assign, 0),
            TokenKind::PlusEq => (BinOp::AddAssign, 0),
            TokenKind::MinusEq => (BinOp::SubAssign, 0),
            TokenKind::StarEq => (BinOp::MulAssign, 0),
            TokenKind::SlashEq => (BinOp::DivAssign, 0),
            TokenKind::PercentEq => (BinOp::ModAssign, 0),
            TokenKind::PlusPlusEq => (BinOp::ConcatAssign, 0),
            
            _ => return None,
        };
        
        Some((op, prec))
    }

    fn parse_unary_expression(&mut self) -> ParseResult<Expr> {
        let start = self.current_span();
        
        // Check for reference operator: &expr
        if self.match_token(&TokenKind::And) {
            let expr = Box::new(self.parse_unary_expression()?);
            let span = start.merge(expr.span());
            return Ok(Expr::Reference { expr, span });
        }
        
        // Check for unary operators
        let op = if self.match_token(&TokenKind::Minus) {
            Some(UnOp::Neg)
        } else if self.match_token(&TokenKind::Not) {
            Some(UnOp::Not)
        } else if self.match_token(&TokenKind::Tilde) {
            Some(UnOp::BitNot)
        } else {
            None
        };
        
        if let Some(op) = op {
            let expr = Box::new(self.parse_unary_expression()?);
            let span = start.merge(expr.span());
            Ok(Expr::Unary { op, expr, span })
        } else {
            self.parse_postfix_expression()
        }
    }

    fn parse_postfix_expression(&mut self) -> ParseResult<Expr> {
        let mut expr = self.parse_primary_expression()?;
        
        loop {
            if self.match_token(&TokenKind::LParen) {
                // Function call
                let mut args = Vec::new();
                while !self.check(&TokenKind::RParen) {
                    args.push(self.parse_expression()?);
                    if !self.match_token(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(&TokenKind::RParen)?;
                
                // Check for trailing closure
                // Special case: match(expr) { Pattern { body } } should be parsed as Expr::Match
                if self.check(&TokenKind::LBrace) {
                    // Check if this is a match expression
                    if let Expr::Ident(ref ident) = expr
                        && ident.name == "match" && args.len() == 1 {
                        // This is match(expr) { arms }
                        // Parse the match arms
                        let match_expr = args.into_iter().next().unwrap();
                        let arms = self.parse_match_arms()?;
                        let span = expr.span().merge(self.previous_span());
                        expr = Expr::Match {
                            expr: Box::new(match_expr),
                            arms,
                            span,
                        };
                        continue; // Skip creating a Call expression
                    }
                    
                    // Regular function call with trailing closure
                    let block = self.parse_block()?;
                    args.push(Expr::Block(block));
                }
                
                let span = expr.span().merge(self.previous_span());
                expr = Expr::Call {
                    func: Box::new(expr),
                    args,
                    span,
                };
            } else if self.match_token(&TokenKind::ColonColon) {
                // Namespace access: module::item
                let item = self.expect_value_ident()?;
                
                // Create a namespaced identifier by concatenating
                let span = expr.span().merge(item.span);
                let new_name = if let Expr::Ident(ident) = expr {
                    format!("{}::{}", ident.name, item.name)
                } else {
                    return Err(self.error("Namespace operator can only follow identifiers"));
                };
                
                expr = Expr::Ident(Ident {
                    name: new_name,
                    span,
                });
            } else if self.match_token(&TokenKind::Dot) {
                // Method call or field access
                // Can be: .method() or .namespace::method() or .field
                let mut method = self.expect_value_ident()?;
                
                // Check for namespace operator after the method name
                while self.match_token(&TokenKind::ColonColon) {
                    let item = self.expect_value_ident()?;
                    method = Ident {
                        name: format!("{}::{}", method.name, item.name),
                        span: method.span.merge(item.span),
                    };
                }
                
                if self.match_token(&TokenKind::LParen) {
                    // Method call
                    let mut args = Vec::new();
                    while !self.check(&TokenKind::RParen) {
                        args.push(self.parse_expression()?);
                        if !self.match_token(&TokenKind::Comma) {
                            break;
                        }
                    }
                    self.expect(&TokenKind::RParen)?;
                    
                    // Check for trailing closure
                    if self.check(&TokenKind::LBrace) {
                        let block = self.parse_block()?;
                        args.push(Expr::Block(block));
                    }
                    
                    let span = expr.span().merge(self.previous_span());
                    expr = Expr::MethodCall {
                        receiver: Box::new(expr),
                        method,
                        args,
                        span,
                    };
                } else {
                    // Field access
                    let span = expr.span().merge(self.previous_span());
                    expr = Expr::FieldAccess {
                        object: Box::new(expr),
                        field: method,
                        span,
                    };
                }
            } else {
                break;
            }
        }
        
        Ok(expr)
    }

    fn parse_primary_expression(&mut self) -> ParseResult<Expr> {
        let start = self.current_span();
        
        // Literals
        if self.check(&TokenKind::Integer) {
            let tok = self.advance();
            let value = self.parse_integer_literal(&tok.text)
                .map_err(|_| ParseError::new("Invalid integer literal", tok.span))?;
            return Ok(Expr::Literal(Literal::Integer(value), tok.span));
        }
        
        if self.check(&TokenKind::Float) {
            let tok = self.advance();
            let value = tok.text.parse::<f64>()
                .map_err(|_| ParseError::new("Invalid float literal", tok.span))?;
            return Ok(Expr::Literal(Literal::Float(value), tok.span));
        }
        
        if self.check(&TokenKind::String) {
            let tok = self.advance();
            return Ok(Expr::Literal(Literal::String(tok.text.clone()), tok.span));
        }
        
        if self.check(&TokenKind::Rune) {
            let tok = self.advance();
            let ch = tok.text.chars().next()
                .ok_or_else(|| ParseError::new("Empty rune literal", tok.span))?;
            return Ok(Expr::Literal(Literal::Rune(ch), tok.span));
        }
        
        // Block expression
        if self.check(&TokenKind::LBrace) {
            let block = self.parse_block()?;
            return Ok(Expr::Block(block));
        }
        
        // Compile-time evaluation: #expr or #(expr)
        if self.match_token(&TokenKind::Hash) {
            let expr = Box::new(self.parse_primary_expression()?);
            return Ok(Expr::Comptime {
                expr,
                span: start.merge(self.previous_span()),
            });
        }
        
        // Parenthesized expression, tuple, or closure
        if self.match_token(&TokenKind::LParen) {
            return self.parse_paren_or_closure_or_tuple(start);
        }
        
        // Identifiers
        if self.check(&TokenKind::ValueIdent) {
            let ident = self.expect_value_ident()?;
            
            // Check for special identifiers that are actually boolean literals
            if ident.name == "True" || ident.name == "true" {
                return Ok(Expr::Literal(Literal::Bool(true), ident.span));
            } else if ident.name == "False" || ident.name == "false" {
                return Ok(Expr::Literal(Literal::Bool(false), ident.span));
            }
            
            return Ok(Expr::Ident(ident));
        }
        
        // Dollar identifiers ($0, $1, etc.)
        if self.check(&TokenKind::DollarIdent) {
            let tok = self.advance();
            return Ok(Expr::Ident(Ident {
                name: tok.text.clone(),
                span: tok.span,
            }));
        }
        
        // Type constructors (struct/enum initialization)
        if self.check(&TokenKind::TypeIdent) {
            let type_name = self.expect_type_ident()?;
            
            // Check for special identifiers that are actually boolean literals
            // (True/False are TypeIdents because they start with uppercase)
            if type_name.name == "True" {
                return Ok(Expr::Literal(Literal::Bool(true), type_name.span));
            } else if type_name.name == "False" {
                return Ok(Expr::Literal(Literal::Bool(false), type_name.span));
            }
            
            if self.match_token(&TokenKind::LParen) {
                // Struct/enum constructor
                let fields = self.parse_struct_init_fields()?;
                self.expect(&TokenKind::RParen)?;
                
                return Ok(Expr::StructInit {
                    ty: Some(type_name),
                    fields,
                    span: start.merge(self.previous_span()),
                });
            } else {
                // Just a type identifier in expression context (e.g., enum variant)
                return Ok(Expr::Ident(Ident {
                    name: type_name.name,
                    span: type_name.span,
                }));
            }
        }
        
        Err(self.error("Expected expression"))
    }

    fn parse_paren_or_closure_or_tuple(&mut self, start: Span) -> ParseResult<Expr> {
        // Could be:
        // 1. (expr) - parenthesized expression
        // 2. (a, b, c) - tuple
        // 3. (x, y) { body } - closure
        // 4. (x Int) { body } - closure with types
        
        // Empty parens - could be empty tuple or parameterless closure
        if self.check(&TokenKind::RParen) {
            self.advance();
            if self.check(&TokenKind::LBrace) || self.check(&TokenKind::Arrow) {
                // Parameterless closure: () { body } or () -> Type { body }
                return self.parse_closure_body(Vec::new(), start);
            } else if self.check(&TokenKind::TypeIdent) {
                // Check if it's () Type { body } pattern
                let checkpoint = self.pos;
                self.advance(); // Skip the type identifier
                if self.check(&TokenKind::LBrace) {
                    // It's a closure with return type!
                    self.pos = checkpoint; // Rewind to parse the type properly
                    return self.parse_closure_body(Vec::new(), start);
                }
                // Not a closure, rewind
                self.pos = checkpoint;
            }
            // Empty tuple
            return Ok(Expr::Tuple(Vec::new(), start.merge(self.previous_span())));
        }
        
        // Try to parse as closure parameters first
        let checkpoint = self.pos;
        if let Ok(params) = self.try_parse_closure_params() {
            self.expect(&TokenKind::RParen).ok();
            
            if self.check(&TokenKind::LBrace) || self.check(&TokenKind::Arrow) {
                // It's a closure!
                return self.parse_closure_body(params, start);
            } else if self.check(&TokenKind::TypeIdent) {
                // Check if it's (params) Type { body } pattern
                let checkpoint2 = self.pos;
                self.advance(); // Skip the type identifier
                if self.check(&TokenKind::LBrace) {
                    // It's a closure with return type!
                    self.pos = checkpoint2; // Rewind to parse the type properly
                    return self.parse_closure_body(params, start);
                }
                // Not a closure, rewind
                self.pos = checkpoint2;
            }
        }
        
        // Not a closure, rewind and parse as expression/tuple
        self.pos = checkpoint;
        
        let mut exprs = Vec::new();
        exprs.push(self.parse_expression()?);
        
        // Check for tuple (more expressions separated by commas)
        if self.match_token(&TokenKind::Comma) {
            while !self.check(&TokenKind::RParen) {
                exprs.push(self.parse_expression()?);
                if !self.match_token(&TokenKind::Comma) {
                    break;
                }
            }
        }
        
        self.expect(&TokenKind::RParen)?;
        
        if exprs.len() == 1 {
            // Parenthesized expression
            Ok(exprs.into_iter().next().unwrap())
        } else {
            // Tuple
            Ok(Expr::Tuple(exprs, start.merge(self.previous_span())))
        }
    }

    fn try_parse_closure_params(&mut self) -> ParseResult<Vec<Param>> {
        let mut params = Vec::new();
        
        while !self.check(&TokenKind::RParen) {
            params.push(self.parse_param()?);
            if !self.match_token(&TokenKind::Comma) {
                break;
            }
        }
        
        Ok(params)
    }

    fn parse_closure_body(&mut self, params: Vec<Param>, start: Span) -> ParseResult<Expr> {
        // Parse optional return type (with or without arrow)
        let return_type = if self.match_token(&TokenKind::Arrow) {
            Some(Box::new(self.parse_type()?))
        } else if self.check(&TokenKind::TypeIdent) {
            // Return type without arrow: (params) Type { body }
            Some(Box::new(self.parse_type()?))
        } else {
            None
        };
        
        let body = Box::new(self.parse_block()?);
        
        Ok(Expr::Closure {
            params,
            return_type,
            body,
            span: start.merge(self.previous_span()),
        })
    }

    fn parse_struct_init_fields(&mut self) -> ParseResult<Vec<FieldInit>> {
        let mut fields = Vec::new();
        
        while !self.check(&TokenKind::RParen) {
            let start = self.current_span();
            
            // Check if it's a named field (name: value) or positional (just value)
            if self.check(&TokenKind::ValueIdent) {
                let checkpoint = self.pos;
                let ident = self.expect_value_ident()?;
                
                if self.match_token(&TokenKind::Colon) {
                    // Named field
                    let value = Box::new(self.parse_expression()?);
                    fields.push(FieldInit {
                        name: Some(ident),
                        value,
                        span: start.merge(self.previous_span()),
                    });
                } else {
                    // Just an expression (positional), rewind
                    self.pos = checkpoint;
                    let value = Box::new(self.parse_expression()?);
                    fields.push(FieldInit {
                        name: None,
                        value,
                        span: start.merge(self.previous_span()),
                    });
                }
            } else {
                // Positional field
                let value = Box::new(self.parse_expression()?);
                fields.push(FieldInit {
                    name: None,
                    value,
                    span: start.merge(self.previous_span()),
                });
            }
            
            if !self.match_token(&TokenKind::Comma) {
                break;
            }
        }
        
        Ok(fields)
    }

    // Helper methods
    fn current_span(&self) -> Span {
        if self.is_at_end() {
            self.tokens.last().map(|t| t.span).unwrap_or(Span::new(0, 0))
        } else {
            self.tokens[self.pos].span
        }
    }

    fn previous_span(&self) -> Span {
        if self.pos > 0 {
            self.tokens[self.pos - 1].span
        } else {
            Span::new(0, 0)
        }
    }

    fn is_at_end(&self) -> bool {
        self.pos >= self.tokens.len() || matches!(self.peek().kind, TokenKind::Eof)
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn peek_ahead(&self, n: usize) -> Option<&Token> {
        if self.pos + n < self.tokens.len() {
            Some(&self.tokens[self.pos + n])
        } else {
            None
        }
    }

    fn advance(&mut self) -> Token {
        if !self.is_at_end() {
            self.pos += 1;
        }
        self.tokens[self.pos - 1].clone()
    }

    fn check(&self, kind: &TokenKind) -> bool {
        !self.is_at_end() && &self.peek().kind == kind
    }

    fn match_token(&mut self, kind: &TokenKind) -> bool {
        if self.check(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: &TokenKind) -> ParseResult<Token> {
        if self.check(kind) {
            Ok(self.advance())
        } else {
            Err(self.error(&format!("Expected {:?}, found {:?}", kind, self.peek().kind)))
        }
    }

    fn expect_type_ident(&mut self) -> ParseResult<Ident> {
        if self.check(&TokenKind::TypeIdent) {
            let tok = self.advance();
            Ok(Ident {
                name: tok.text,
                span: tok.span,
            })
        } else {
            Err(self.error("Expected type identifier"))
        }
    }

    fn expect_value_ident(&mut self) -> ParseResult<Ident> {
        if self.check(&TokenKind::ValueIdent) {
            let tok = self.advance();
            Ok(Ident {
                name: tok.text,
                span: tok.span,
            })
        } else {
            Err(self.error("Expected value identifier"))
        }
    }

    fn parse_integer_literal(&self, text: &str) -> Result<i64, std::num::ParseIntError> {
        if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
            i64::from_str_radix(hex, 16)
        } else if let Some(bin) = text.strip_prefix("0b").or_else(|| text.strip_prefix("0B")) {
            i64::from_str_radix(bin, 2)
        } else if let Some(oct) = text.strip_prefix("0o").or_else(|| text.strip_prefix("0O")) {
            i64::from_str_radix(oct, 8)
        } else {
            text.parse::<i64>()
        }
    }

    fn error(&self, message: &str) -> ParseError {
        ParseError::new(message, self.current_span())
    }
}

impl Clone for Parser {
    fn clone(&self) -> Self {
        Self {
            tokens: self.tokens.clone(),
            pos: self.pos,
            filename: self.filename.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn parse_expr(input: &str) -> ParseResult<Expr> {
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens);
        parser.parse_expression()
    }

    fn parse(input: &str) -> ParseResult<Vec<TopLevel>> {
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens);
        parser.parse()
    }

    fn parse_with_filename(input: &str, filename: &str) -> ParseResult<Vec<TopLevel>> {
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new_with_filename(tokens, filename.to_string());
        parser.parse()
    }

    // ===== TOP-LEVEL STATEMENTS =====

    #[test]
    fn test_top_level_statements_allowed_in_test_files() {
        let input = "assert(1 == 1)\nprint(\"test\")\nx := 42";
        let result = parse_with_filename(input, "test.test.atom");
        assert!(result.is_ok(), "Expected success, got error: {:?}", result);
        
        let items = result.unwrap();
        assert_eq!(items.len(), 3, "Expected 3 top-level items");
        
        // Check that we have statement nodes
        match &items[0] {
            TopLevel::Statement(_) => {},
            other => panic!("Expected TopLevel::Statement for assert, got {:?}", other),
        }
        match &items[1] {
            TopLevel::Statement(_) => {},
            other => panic!("Expected TopLevel::Statement for print, got {:?}", other),
        }
        match &items[2] {
            TopLevel::Variable(_) => {},
            other => panic!("Expected TopLevel::Variable for x, got {:?}", other),
        }
    }

    #[test]
    fn test_top_level_statements_error_in_regular_files() {
        let input = "assert(1 == 1)";
        let result = parse_with_filename(input, "main.atom");
        assert!(result.is_err(), "Expected error for top-level statement in non-test file");
        
        let error = result.unwrap_err();
        let error_msg = format!("{:?}", error);
        assert!(
            error_msg.contains("Top-level statements are only allowed in .test.atom files"),
            "Expected error message about .test.atom files, got: {:?}", error
        );
    }

    #[test]
    fn test_top_level_statements_multiple_errors() {
        let input = "assert(1 == 1)\nprint(\"hello\")\nfoo()";
        let result = parse_with_filename(input, "main.atom");
        assert!(result.is_err(), "Expected error for top-level statements in non-test file");
    }

    // ===== BASIC LITERALS =====
    
    #[test]
    fn test_integer_literal() {
        let expr = parse_expr("42").unwrap();
        match expr {
            Expr::Literal(Literal::Integer(42), _) => {},
            _ => panic!("Expected integer literal 42, got {:?}", expr),
        }
    }

    #[test]
    fn test_float_literal() {
        let expr = parse_expr("3.14").unwrap();
        match expr {
            Expr::Literal(Literal::Float(f), _) if (f - 3.14).abs() < 0.001 => {},
            _ => panic!("Expected float literal 3.14, got {:?}", expr),
        }
    }

    #[test]
    fn test_string_literal() {
        let expr = parse_expr(r#""hello world""#).unwrap();
        match expr {
            Expr::Literal(Literal::String(s), _) if s == "hello world" => {},
            _ => panic!("Expected string literal, got {:?}", expr),
        }
    }

    #[test]
    fn test_rune_literal() {
        let expr = parse_expr("'a'").unwrap();
        match expr {
            Expr::Literal(Literal::Rune('a'), _) => {},
            _ => panic!("Expected rune literal 'a', got {:?}", expr),
        }
    }

    #[test]
    fn test_bool_literals() {
        // True and False are special TypeIdents that are converted to Bool literals by the parser
        let expr = parse_expr("True").unwrap();
        match expr {
            Expr::Literal(Literal::Bool(true), _) => {},
            _ => panic!("Expected Bool(true) literal, got {:?}", expr),
        }

        let expr = parse_expr("False").unwrap();
        match expr {
            Expr::Literal(Literal::Bool(false), _) => {},
            _ => panic!("Expected Bool(false) literal, got {:?}", expr),
        }
    }

    // ===== IDENTIFIERS =====

    #[test]
    fn test_value_identifier() {
        let expr = parse_expr("my_var").unwrap();
        match expr {
            Expr::Ident(ident) if ident.name == "my_var" => {},
            _ => panic!("Expected identifier 'my_var', got {:?}", expr),
        }
    }

    #[test]
    fn test_dollar_identifier() {
        let expr = parse_expr("$0").unwrap();
        match expr {
            Expr::Ident(ident) if ident.name == "$0" => {},
            _ => panic!("Expected identifier '$0', got {:?}", expr),
        }
    }

    // ===== BINARY EXPRESSIONS =====

    #[test]
    fn test_addition() {
        let expr = parse_expr("1 + 2").unwrap();
        match expr {
            Expr::Binary { op: BinOp::Add, .. } => {},
            _ => panic!("Expected addition, got {:?}", expr),
        }
    }

    #[test]
    fn test_multiplication_precedence() {
        let expr = parse_expr("1 + 2 * 3").unwrap();
        // Should parse as 1 + (2 * 3)
        match expr {
            Expr::Binary { 
                op: BinOp::Add, 
                left,
                right,
                .. 
            } => {
                assert!(matches!(*left, Expr::Literal(Literal::Integer(1), _)));
                assert!(matches!(*right, Expr::Binary { op: BinOp::Mul, .. }));
            },
            _ => panic!("Expected 1 + (2 * 3), got {:?}", expr),
        }
    }

    #[test]
    fn test_concatenation() {
        let expr = parse_expr(r#""Hello " ++ "World""#).unwrap();
        match expr {
            Expr::Binary { op: BinOp::Concat, .. } => {},
            _ => panic!("Expected concatenation, got {:?}", expr),
        }
    }

    #[test]
    fn test_comparison() {
        let expr = parse_expr("a == b").unwrap();
        match expr {
            Expr::Binary { op: BinOp::Eq, .. } => {},
            _ => panic!("Expected equality comparison, got {:?}", expr),
        }
    }

    #[test]
    fn test_logical_and() {
        let expr = parse_expr("a && b").unwrap();
        match expr {
            Expr::Binary { op: BinOp::And, .. } => {},
            _ => panic!("Expected logical AND, got {:?}", expr),
        }
    }

    // ===== UNARY EXPRESSIONS =====

    #[test]
    fn test_negation() {
        let expr = parse_expr("-5").unwrap();
        match expr {
            Expr::Unary { op: UnOp::Neg, .. } => {},
            _ => panic!("Expected negation, got {:?}", expr),
        }
    }

    #[test]
    fn test_logical_not() {
        let expr = parse_expr("!condition").unwrap();
        match expr {
            Expr::Unary { op: UnOp::Not, .. } => {},
            _ => panic!("Expected logical NOT, got {:?}", expr),
        }
    }

    // ===== FUNCTION CALLS =====

    #[test]
    fn test_function_call_no_args() {
        let expr = parse_expr("foo()").unwrap();
        match expr {
            Expr::Call { func, args, .. } => {
                assert!(matches!(*func, Expr::Ident(_)));
                assert_eq!(args.len(), 0);
            },
            _ => panic!("Expected function call, got {:?}", expr),
        }
    }

    #[test]
    fn test_function_call_with_args() {
        let expr = parse_expr("sum(1, 2, 3)").unwrap();
        match expr {
            Expr::Call { func, args, .. } => {
                assert!(matches!(*func, Expr::Ident(_)));
                assert_eq!(args.len(), 3);
            },
            _ => panic!("Expected function call with 3 args, got {:?}", expr),
        }
    }

    #[test]
    fn test_method_call() {
        let expr = parse_expr("arr.len()").unwrap();
        match expr {
            Expr::MethodCall { receiver, method, args, .. } => {
                assert!(matches!(*receiver, Expr::Ident(_)));
                assert_eq!(method.name, "len");
                assert_eq!(args.len(), 0);
            },
            _ => panic!("Expected method call, got {:?}", expr),
        }
    }

    #[test]
    fn test_chained_method_calls() {
        let expr = parse_expr("arr.reverse().first()").unwrap();
        match expr {
            Expr::MethodCall { method, .. } if method.name == "first" => {},
            _ => panic!("Expected chained method calls, got {:?}", expr),
        }
    }

    // ===== FIELD ACCESS =====

    #[test]
    fn test_field_access() {
        let expr = parse_expr("point.x").unwrap();
        match expr {
            Expr::FieldAccess { object, field, .. } => {
                assert!(matches!(*object, Expr::Ident(_)));
                assert_eq!(field.name, "x");
            },
            _ => panic!("Expected field access, got {:?}", expr),
        }
    }

    // ===== STRUCT INITIALIZATION =====

    #[test]
    fn test_struct_init_named_fields() {
        let expr = parse_expr("Point(x: 1, y: 2)").unwrap();
        match expr {
            Expr::StructInit { ty, fields, .. } => {
                assert_eq!(ty.as_ref().unwrap().name, "Point");
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].name.as_ref().unwrap().name, "x");
                assert_eq!(fields[1].name.as_ref().unwrap().name, "y");
            },
            _ => panic!("Expected struct initialization, got {:?}", expr),
        }
    }

    #[test]
    fn test_struct_init_positional() {
        let expr = parse_expr("Pair(5, 7)").unwrap();
        match expr {
            Expr::StructInit { ty, fields, .. } => {
                assert_eq!(ty.as_ref().unwrap().name, "Pair");
                assert_eq!(fields.len(), 2);
                assert!(fields[0].name.is_none());
                assert!(fields[1].name.is_none());
            },
            _ => panic!("Expected struct initialization, got {:?}", expr),
        }
    }

    // ===== TUPLES =====

    #[test]
    fn test_tuple_expression() {
        let expr = parse_expr("(1, 2, 3)").unwrap();
        match expr {
            Expr::Tuple(exprs, _) => {
                assert_eq!(exprs.len(), 3);
            },
            _ => panic!("Expected tuple, got {:?}", expr),
        }
    }

    #[test]
    fn test_parenthesized_expression() {
        let expr = parse_expr("(42)").unwrap();
        // Single element in parens is not a tuple
        match expr {
            Expr::Literal(Literal::Integer(42), _) => {},
            _ => panic!("Expected parenthesized expression (unwrapped), got {:?}", expr),
        }
    }

    // ===== CLOSURES =====

    #[test]
    fn test_closure_no_params() {
        let expr = parse_expr("() { 42 }").unwrap();
        match expr {
            Expr::Closure { params, body, .. } => {
                assert_eq!(params.len(), 0);
                assert!(!body.stmts.is_empty());
            },
            _ => panic!("Expected closure, got {:?}", expr),
        }
    }

    #[test]
    fn test_closure_with_params() {
        let expr = parse_expr("(x Int, y Int) { x + y }").unwrap();
        match expr {
            Expr::Closure { params, .. } => {
                assert_eq!(params.len(), 2);
                assert_eq!(params[0].name.name, "x");
                assert_eq!(params[1].name.name, "y");
            },
            _ => panic!("Expected closure with params, got {:?}", expr),
        }
    }

    // ===== BLOCKS =====

    #[test]
    fn test_block_expression() {
        let expr = parse_expr("{ a := 5; a * 2 }").unwrap();
        match expr {
            Expr::Block(block) => {
                assert_eq!(block.stmts.len(), 2);
            },
            _ => panic!("Expected block expression, got {:?}", expr),
        }
    }

    // ===== COMPTIME =====

    #[test]
    fn test_comptime_expression() {
        // Comptime is parsed as #identifier, then it becomes a call
        // So #add(3, 5) is Call(Comptime(add), [3, 5])
        let expr = parse_expr("#add(3, 5)").unwrap();
        match expr {
            Expr::Call { func, args, .. } => {
                assert!(matches!(*func, Expr::Comptime { .. }));
                assert_eq!(args.len(), 2);
            },
            _ => panic!("Expected call with comptime function, got {:?}", expr),
        }
    }

    // ===== STRUCT DEFINITIONS =====

    #[test]
    fn test_struct_definition_simple() {
        let items = parse("Point(x Float, y Float)").unwrap();
        assert_eq!(items.len(), 1);
        
        match &items[0] {
            TopLevel::Struct(def) => {
                assert_eq!(def.name.name, "Point");
                assert_eq!(def.fields.len(), 2);
                assert_eq!(def.fields[0].name.as_ref().unwrap().name, "x");
                assert_eq!(def.fields[1].name.as_ref().unwrap().name, "y");
            },
            _ => panic!("Expected struct definition"),
        }
    }

    #[test]
    fn test_struct_definition_with_visibility() {
        let items = parse("+ExportedStruct(field Int)").unwrap();
        assert_eq!(items.len(), 1);
        
        match &items[0] {
            TopLevel::Struct(def) => {
                assert_eq!(def.visibility, Visibility::Public);
                assert_eq!(def.name.name, "ExportedStruct");
            },
            _ => panic!("Expected public struct definition"),
        }
    }

    #[test]
    fn test_struct_definition_with_spread() {
        let items = parse("Vec3(..Vec2, z Float)").unwrap();
        assert_eq!(items.len(), 1);
        
        match &items[0] {
            TopLevel::Struct(def) => {
                assert_eq!(def.name.name, "Vec3");
                assert_eq!(def.fields.len(), 2);
                // First field should be spread
                assert!(def.fields[0].name.is_none());
            },
            _ => panic!("Expected struct definition with spread"),
        }
    }

    // ===== ENUM DEFINITIONS =====

    #[test]
    fn test_enum_definition_simple() {
        // Newlines help distinguish enum cases from struct fields
        let items = parse("Bool(\n  True\n  False\n)").unwrap();
        assert_eq!(items.len(), 1);
        
        match &items[0] {
            TopLevel::Enum(def) => {
                assert_eq!(def.name.name, "Bool");
                assert_eq!(def.cases.len(), 2, "Expected 2 enum cases");
                assert_eq!(def.cases[0].name.name, "True");
                assert_eq!(def.cases[1].name.name, "False");
            },
            _ => panic!("Expected enum definition, got {:?}", &items[0]),
        }
    }

    #[test]
    fn test_enum_definition_with_fields() {
        let items = parse("Option(\n  Some(t)\n  None\n)").unwrap();
        assert_eq!(items.len(), 1);
        
        match &items[0] {
            TopLevel::Enum(def) => {
                assert_eq!(def.name.name, "Option");
                assert_eq!(def.cases.len(), 2);
                assert_eq!(def.cases[0].name.name, "Some");
                assert_eq!(def.cases[0].fields.len(), 1);
                assert_eq!(def.cases[1].name.name, "None");
                assert_eq!(def.cases[1].fields.len(), 0);
            },
            _ => panic!("Expected enum definition with fields, got {:?}", &items[0]),
        }
    }

    // ===== FUNCTION DEFINITIONS =====

    #[test]
    fn test_function_definition_no_params() {
        let items = parse("main() { print(\"Hello\") }").unwrap();
        assert_eq!(items.len(), 1);
        
        match &items[0] {
            TopLevel::Function(def) => {
                assert_eq!(def.name.name, "main");
                assert_eq!(def.params.len(), 0);
                assert!(def.return_type.is_none());
            },
            _ => panic!("Expected function definition"),
        }
    }

    #[test]
    fn test_function_definition_with_params_and_return() {
        let items = parse("add(a Int, b Int) Int { a + b }").unwrap();
        assert_eq!(items.len(), 1);
        
        match &items[0] {
            TopLevel::Function(def) => {
                assert_eq!(def.name.name, "add");
                assert_eq!(def.params.len(), 2);
                assert!(def.return_type.is_some());
            },
            _ => panic!("Expected function definition"),
        }
    }

    #[test]
    fn test_function_definition_variadic() {
        let items = parse("sum(values Int*) Int { 0 }").unwrap();
        assert_eq!(items.len(), 1);
        
        match &items[0] {
            TopLevel::Function(def) => {
                assert_eq!(def.name.name, "sum");
                assert_eq!(def.params.len(), 1);
                // Check that the type is variadic
                match def.params[0].ty.as_ref().unwrap().as_ref() {
                    Type::Variadic { non_empty: false, .. } => {},
                    _ => panic!("Expected variadic type"),
                }
            },
            _ => panic!("Expected function definition"),
        }
    }

    #[test]
    fn test_function_with_default_param() {
        let items = parse("print(msg String, level LogLevel = Info) {}").unwrap();
        assert_eq!(items.len(), 1);
        
        match &items[0] {
            TopLevel::Function(def) => {
                assert_eq!(def.params.len(), 2);
                assert!(def.params[0].default.is_none());
                assert!(def.params[1].default.is_some());
            },
            _ => panic!("Expected function with default param"),
        }
    }

    // ===== VARIABLE DECLARATIONS =====

    #[test]
    fn test_var_decl_inferred() {
        let items = parse("x := 5").unwrap();
        assert_eq!(items.len(), 1);
        
        match &items[0] {
            TopLevel::Variable(decl) => {
                assert_eq!(decl.names.len(), 1);
                assert_eq!(decl.names[0].name, "x");
                assert!(decl.ty.is_none());
                assert!(decl.init.is_some());
            },
            _ => panic!("Expected variable declaration"),
        }
    }

    #[test]
    fn test_var_decl_typed() {
        let items = parse("count: Int = 0").unwrap();
        assert_eq!(items.len(), 1);
        
        match &items[0] {
            TopLevel::Variable(decl) => {
                assert_eq!(decl.names.len(), 1);
                assert_eq!(decl.names[0].name, "count");
                assert!(decl.ty.is_some());
                assert!(decl.init.is_some());
            },
            _ => panic!("Expected variable declaration"),
        }
    }

    #[test]
    fn test_var_decl_zero_init() {
        let items = parse("value: Int").unwrap();
        assert_eq!(items.len(), 1);
        
        match &items[0] {
            TopLevel::Variable(decl) => {
                assert_eq!(decl.names.len(), 1);
                assert_eq!(decl.names[0].name, "value");
                assert!(decl.ty.is_some());
                assert!(decl.init.is_none());
            },
            _ => panic!("Expected zero-initialized variable"),
        }
    }

    // ===== TUPLE DESTRUCTURING =====

    fn parse_stmt(input: &str) -> ParseResult<Stmt> {
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize().unwrap();
        let mut parser = Parser::new(tokens);
        parser.parse_statement()
    }

    #[test]
    fn test_tuple_destructuring_two_vars() {
        let stmt = parse_stmt("a, b := 1, 2").unwrap();
        match stmt {
            Stmt::VarDecl(decl) => {
                assert_eq!(decl.names.len(), 2);
                assert_eq!(decl.names[0].name, "a");
                assert_eq!(decl.names[1].name, "b");
                assert!(decl.init.is_some());
            },
            _ => panic!("Expected variable declaration"),
        }
    }

    #[test]
    fn test_tuple_destructuring_three_vars() {
        let stmt = parse_stmt("x, y, z := 1, 2, 3").unwrap();
        match stmt {
            Stmt::VarDecl(decl) => {
                assert_eq!(decl.names.len(), 3);
                assert_eq!(decl.names[0].name, "x");
                assert_eq!(decl.names[1].name, "y");
                assert_eq!(decl.names[2].name, "z");
            },
            _ => panic!("Expected variable declaration"),
        }
    }

    #[test]
    fn test_tuple_destructuring_function_call() {
        let stmt = parse_stmt("rune, consumed := decode_utf8_at(s, i, seq_len)").unwrap();
        match stmt {
            Stmt::VarDecl(decl) => {
                assert_eq!(decl.names.len(), 2);
                assert_eq!(decl.names[0].name, "rune");
                assert_eq!(decl.names[1].name, "consumed");
                // Check that init is a function call
                match decl.init.as_ref() {
                    Some(expr) => {
                        assert!(matches!(**expr, Expr::Call { .. }));
                    },
                    None => panic!("Expected init expression"),
                }
            },
            _ => panic!("Expected variable declaration"),
        }
    }

    // ===== TYPES =====

    #[test]
    fn test_type_named() {
        let items = parse("foo(x String) {}").unwrap();
        match &items[0] {
            TopLevel::Function(def) => {
                match def.params[0].ty.as_ref().unwrap().as_ref() {
                    Type::Named(ident) if ident.name == "String" => {},
                    _ => panic!("Expected named type String"),
                }
            },
            _ => panic!("Expected function"),
        }
    }

    #[test]
    fn test_type_tuple() {
        let items = parse("foo(x (Int, Float)) {}").unwrap();
        match &items[0] {
            TopLevel::Function(def) => {
                match def.params[0].ty.as_ref().unwrap().as_ref() {
                    Type::Tuple(types, _) => {
                        assert_eq!(types.len(), 2);
                    },
                    _ => panic!("Expected tuple type"),
                }
            },
            _ => panic!("Expected function"),
        }
    }

    #[test]
    fn test_type_generic() {
        let items = parse("foo(x Option(Int)) {}").unwrap();
        match &items[0] {
            TopLevel::Function(def) => {
                match def.params[0].ty.as_ref().unwrap().as_ref() {
                    Type::Generic { name, params, .. } => {
                        assert_eq!(name.name, "Option");
                        assert_eq!(params.len(), 1);
                    },
                    _ => panic!("Expected generic type"),
                }
            },
            _ => panic!("Expected function"),
        }
    }

    #[test]
    fn test_type_variadic() {
        let items = parse("foo(x Int*) {}").unwrap();
        match &items[0] {
            TopLevel::Function(def) => {
                match def.params[0].ty.as_ref().unwrap().as_ref() {
                    Type::Variadic { non_empty: false, .. } => {},
                    _ => panic!("Expected variadic type"),
                }
            },
            _ => panic!("Expected function"),
        }
    }

    #[test]
    fn test_type_non_empty_variadic() {
        let items = parse("foo(x Int+) {}").unwrap();
        match &items[0] {
            TopLevel::Function(def) => {
                match def.params[0].ty.as_ref().unwrap().as_ref() {
                    Type::Variadic { non_empty: true, .. } => {},
                    _ => panic!("Expected non-empty variadic type"),
                }
            },
            _ => panic!("Expected function"),
        }
    }

    #[test]
    fn test_type_static_array() {
        let items = parse("foo(x Int*4) {}").unwrap();
        match &items[0] {
            TopLevel::Function(def) => {
                match def.params[0].ty.as_ref().unwrap().as_ref() {
                    Type::StaticArray { .. } => {},
                    _ => panic!("Expected static array type"),
                }
            },
            _ => panic!("Expected function"),
        }
    }

    #[test]
    fn test_type_function() {
        let items = parse("foo(f (Int, Int) { Int }) {}").unwrap();
        match &items[0] {
            TopLevel::Function(def) => {
                match def.params[0].ty.as_ref().unwrap().as_ref() {
                    Type::Function { params, return_type, .. } => {
                        assert_eq!(params.len(), 2);
                        assert!(return_type.is_some());
                    },
                    _ => panic!("Expected function type"),
                }
            },
            _ => panic!("Expected function"),
        }
    }

    // ===== ASSIGNMENT =====

    #[test]
    fn test_assignment() {
        let expr = parse_expr("x = 5").unwrap();
        match expr {
            Expr::Binary { op: BinOp::Assign, .. } => {},
            _ => panic!("Expected assignment, got {:?}", expr),
        }
    }

    #[test]
    fn test_compound_assignment() {
        let expr = parse_expr("x += 5").unwrap();
        match expr {
            Expr::Binary { op: BinOp::AddAssign, .. } => {},
            _ => panic!("Expected compound assignment, got {:?}", expr),
        }
    }

    #[test]
    fn test_concat_assignment() {
        let expr = parse_expr("arr ++= 5").unwrap();
        match expr {
            Expr::Binary { op: BinOp::ConcatAssign, .. } => {},
            _ => panic!("Expected concat assignment, got {:?}", expr),
        }
    }

    // ===== NAMESPACE ACCESS =====

    #[test]
    fn test_namespace_access() {
        let expr = parse_expr("std::print").unwrap();
        match expr {
            Expr::Ident(ident) if ident.name == "std::print" => {},
            _ => panic!("Expected namespace access, got {:?}", expr),
        }
    }

    // ===== TRAILING CLOSURE SYNTAX =====

    #[test]
    fn test_function_call_with_trailing_closure() {
        // loop(arr) { $0 * 2 }
        let expr = parse_expr("loop(arr) { $0 * 2 }").unwrap();
        match expr {
            Expr::Call { func, args, .. } => {
                // func should be 'loop'
                match *func {
                    Expr::Ident(ref ident) if ident.name == "loop" => {},
                    _ => panic!("Expected loop function, got {:?}", func),
                }
                // args should be [arr, closure_block]
                assert_eq!(args.len(), 2, "Expected arr and closure block");
                // Second arg should be a block
                assert!(matches!(args[1], Expr::Block(_)));
            },
            _ => panic!("Expected call with trailing closure, got {:?}", expr),
        }
    }

    #[test]
    fn test_match_as_function() {
        // match(value) { True { 1 } } - should now be parsed as Expr::Match
        let expr = parse_expr("match(x) { True { 1 } }").unwrap();
        match expr {
            Expr::Match { expr: match_expr, arms, .. } => {
                assert!(matches!(*match_expr, Expr::Ident(_)));
                assert_eq!(arms.len(), 1);
            },
            _ => panic!("Expected match expression, got {:?}", expr),
        }
    }

    #[test]
    fn test_inline_match_with_commas() {
        // match(x) { True { 1 }, False { 2 } } - now parsed as Expr::Match
        let expr = parse_expr("match(x) { True { 1 }, False { 2 } }").unwrap();
        match expr {
            Expr::Match { expr: match_expr, arms, .. } => {
                assert!(matches!(*match_expr, Expr::Ident(_)));
                assert_eq!(arms.len(), 2);
            },
            _ => panic!("Expected match expression, got {:?}", expr),
        }
    }

    #[test]
    fn test_loop_with_condition_trailing_closure() {
        // loop(i < 10) { i += 1 }
        let expr = parse_expr("loop(i < 10) { i += 1 }").unwrap();
        match expr {
            Expr::Call { args, .. } => {
                assert_eq!(args.len(), 2);
                // First arg is condition
                assert!(matches!(args[0], Expr::Binary { .. }));
                // Second is block
                assert!(matches!(args[1], Expr::Block(_)));
            },
            _ => panic!("Expected loop with condition and closure, got {:?}", expr),
        }
    }

    #[test]
    fn test_chained_method_with_trailing_closure() {
        // arr.loop() { print($0) }
        let expr = parse_expr("arr.loop() { print($0) }").unwrap();
        match expr {
            Expr::MethodCall { receiver, method, args, .. } => {
                assert!(matches!(*receiver, Expr::Ident(_)));
                assert_eq!(method.name, "loop");
                assert_eq!(args.len(), 1); // Just the block
                assert!(matches!(args[0], Expr::Block(_)));
            },
            _ => panic!("Expected method call with trailing closure, got {:?}", expr),
        }
    }

    // ===== STRING INTERPOLATION (NOT YET IMPLEMENTED) =====

    #[test]
    #[should_panic(expected = "interpolation")]
    #[ignore] // TODO: Implement string interpolation
    fn test_string_interpolation() {
        let _expr = parse_expr(r#""Hello \(name)!""#).unwrap();
        // Should parse as interpolated string
        panic!("String interpolation not yet implemented");
    }

    // ===== REAL-WORLD EXAMPLES FROM STDLIB =====

    #[test]
    fn test_fibonacci_function() {
        let code = r#"
fib(nth Int) Int {
    match(nth) {
        0 { 0 }
        1 { 1 }
        _ { fib(nth - 1) + fib(nth - 2) }
    }
}
"#;
        let items = parse(code).unwrap();
        assert_eq!(items.len(), 1);
        
        match &items[0] {
            TopLevel::Function(def) => {
                assert_eq!(def.name.name, "fib");
                assert_eq!(def.params.len(), 1);
                assert!(def.return_type.is_some());
                // Body should have a match call
                assert!(!def.body.stmts.is_empty());
            },
            _ => panic!("Expected function definition"),
        }
    }

    #[test]
    fn test_array_map_from_stdlib() {
        let code = r#"
+map(arr t*, fn (t) {u}) u* {
    result: u*
    loop(arr) {
        result ++= fn($0)
    }
    result
}
"#;
        let items = parse(code).unwrap();
        assert_eq!(items.len(), 1);
        
        match &items[0] {
            TopLevel::Function(def) => {
                assert_eq!(def.visibility, Visibility::Public);
                assert_eq!(def.name.name, "map");
                assert_eq!(def.params.len(), 2);
                
                // Second param should be function type
                match def.params[1].ty.as_ref().unwrap().as_ref() {
                    Type::Function { .. } => {},
                    _ => panic!("Expected function type for fn parameter"),
                }
            },
            _ => panic!("Expected function definition"),
        }
    }

    #[test]
    fn test_option_enum_from_stdlib() {
        let code = r#"
+Option(
    Some(t)
    None
)
"#;
        let items = parse(code).unwrap();
        assert_eq!(items.len(), 1);
        
        match &items[0] {
            TopLevel::Enum(def) => {
                assert_eq!(def.visibility, Visibility::Public);
                assert_eq!(def.name.name, "Option");
                assert_eq!(def.cases.len(), 2);
                assert_eq!(def.cases[0].name.name, "Some");
                assert_eq!(def.cases[0].fields.len(), 1);
                assert_eq!(def.cases[1].name.name, "None");
                assert_eq!(def.cases[1].fields.len(), 0);
            },
            _ => panic!("Expected enum definition"),
        }
    }

    #[test]
    fn test_result_enum_with_type_params() {
        let code = r#"
+Result(t, e = String;
    Ok(t)
    Err(e)
)
"#;
        let items = parse(code).unwrap();
        assert_eq!(items.len(), 1);
        
        match &items[0] {
            TopLevel::Enum(def) => {
                assert_eq!(def.name.name, "Result");
                assert_eq!(def.type_params.len(), 2);
                // Second type param should have default
                assert!(def.type_params[1].default.is_some());
                assert_eq!(def.cases.len(), 2);
            },
            _ => panic!("Expected enum definition"),
        }
    }

    #[test]
    fn test_complex_expression_from_stdlib() {
        // Match expression with trailing closure - now parsed as Expr::Match
        let code = "match(x) { True { 1 } False { 2 } }";
        let expr = parse_expr(code).unwrap();
        match expr {
            Expr::Match { .. } => {},
            _ => panic!("Expected match expression, got {:?}", expr),
        }
    }

    #[test]
    fn test_zero_initialized_variable() {
        // From stdlib: result: u*
        let code = "result: u*";
        let items = parse(code).unwrap();
        assert_eq!(items.len(), 1);
        
        match &items[0] {
            TopLevel::Variable(decl) => {
                assert_eq!(decl.names.len(), 1);
                assert_eq!(decl.names[0].name, "result");
                assert!(decl.ty.is_some());
                assert!(decl.init.is_none()); // Zero-initialized
            },
            _ => panic!("Expected variable declaration"),
        }
    }

    #[test]
    fn test_generic_type_in_function() {
        let code = "+len(of String) Int { 0 }";
        let items = parse(code).unwrap();
        
        match &items[0] {
            TopLevel::Function(def) => {
                assert_eq!(def.visibility, Visibility::Public);
                assert_eq!(def.name.name, "len");
            },
            _ => panic!("Expected function"),
        }
    }

    #[test]
    fn test_multiline_match() {
        let expr = parse_expr(r#"match(x) {
    True { }
    False { 1 }
}"#).unwrap();
        match expr {
            Expr::Match { .. } => {},
            _ => panic!("Expected match expression, got {:?}", expr),
        }
    }

    #[test]
    fn test_hex_literal() {
        let expr = parse_expr("0xFFFD").unwrap();
        match expr {
            Expr::Literal(Literal::Integer(val), _) => {
                assert_eq!(val, 0xFFFD);
            },
            _ => panic!("Expected hex literal"),
        }
    }

    #[test]
    fn test_binary_literal() {
        let expr = parse_expr("0b10000000").unwrap();
        match expr {
            Expr::Literal(Literal::Integer(val), _) => {
                assert_eq!(val, 0b10000000);
            },
            _ => panic!("Expected binary literal"),
        }
    }

    #[test]
    fn test_octal_literal() {
        let expr = parse_expr("0o755").unwrap();
        match expr {
            Expr::Literal(Literal::Integer(val), _) => {
                assert_eq!(val, 0o755);
            },
            _ => panic!("Expected octal literal"),
        }
    }

    #[test]
    fn test_import_all() {
        let code = "matrix::*";
        let items = parse(code).unwrap();
        assert_eq!(items.len(), 1);
        
        match &items[0] {
            TopLevel::Import(decl) => {
                assert_eq!(decl.namespace.name, "matrix");
                assert!(matches!(decl.items, ImportItems::All));
            },
            _ => panic!("Expected import declaration"),
        }
    }

    #[test]
    fn test_import_named() {
        let code = "physics::(force, kinematics)";
        let items = parse(code).unwrap();
        assert_eq!(items.len(), 1);
        
        match &items[0] {
            TopLevel::Import(decl) => {
                assert_eq!(decl.namespace.name, "physics");
                match &decl.items {
                    ImportItems::Named(names) => {
                        assert_eq!(names.len(), 2);
                        assert_eq!(names[0].name, "force");
                        assert_eq!(names[1].name, "kinematics");
                    },
                    _ => panic!("Expected named imports"),
                }
            },
            _ => panic!("Expected import declaration"),
        }
    }

    #[test]
    fn test_import_single() {
        let code = "math::(sqrt)";
        let items = parse(code).unwrap();
        assert_eq!(items.len(), 1);
        
        match &items[0] {
            TopLevel::Import(decl) => {
                assert_eq!(decl.namespace.name, "math");
                match &decl.items {
                    ImportItems::Named(names) => {
                        assert_eq!(names.len(), 1);
                        assert_eq!(names[0].name, "sqrt");
                    },
                    _ => panic!("Expected named imports"),
                }
            },
            _ => panic!("Expected import declaration"),
        }
    }

    #[test]
    fn test_match_simple() {
        let code = "match(x) { True { 1 } False { 2 } }";
        let expr = parse_expr(code).unwrap();
        
        match expr {
            Expr::Match { expr: match_expr, arms, .. } => {
                // Check that we're matching on 'x'
                assert!(matches!(*match_expr, Expr::Ident(_)));
                
                // Should have 2 arms
                assert_eq!(arms.len(), 2);
                
                // First arm: True { 1 }
                assert!(matches!(arms[0].pattern, Pattern::Enum { .. }));
                
                // Second arm: False { 2 }
                assert!(matches!(arms[1].pattern, Pattern::Enum { .. }));
            },
            _ => panic!("Expected match expression, got {:?}", expr),
        }
    }

    #[test]
    fn test_match_with_wildcard() {
        let code = "match(x) { Some(val) { val } _ { 0 } }";
        let expr = parse_expr(code).unwrap();
        
        match expr {
            Expr::Match { arms, .. } => {
                assert_eq!(arms.len(), 2);
                
                // First arm: Some(val)
                if let Pattern::Enum { name, fields, .. } = &arms[0].pattern {
                    assert_eq!(name.name, "Some");
                    assert_eq!(fields.len(), 1);
                    assert!(matches!(fields[0], Pattern::Ident(_)));
                } else {
                    panic!("Expected enum pattern");
                }
                
                // Second arm: _
                assert!(matches!(arms[1].pattern, Pattern::Wildcard(_)));
            },
            _ => panic!("Expected match expression"),
        }
    }

    #[test]
    fn test_match_nested_pattern() {
        let code = "match(response) { Success(Some(payload)) { payload } _ { False } }";
        let expr = parse_expr(code).unwrap();
        
        match expr {
            Expr::Match { arms, .. } => {
                assert_eq!(arms.len(), 2);
                
                // First arm: Success(Some(payload))
                if let Pattern::Enum { name, fields, .. } = &arms[0].pattern {
                    assert_eq!(name.name, "Success");
                    assert_eq!(fields.len(), 1);
                    
                    // Nested pattern: Some(payload)
                    if let Pattern::Enum { name: inner_name, fields: inner_fields, .. } = &fields[0] {
                        assert_eq!(inner_name.name, "Some");
                        assert_eq!(inner_fields.len(), 1);
                        assert!(matches!(inner_fields[0], Pattern::Ident(_)));
                    } else {
                        panic!("Expected nested enum pattern");
                    }
                } else {
                    panic!("Expected enum pattern");
                }
            },
            _ => panic!("Expected match expression"),
        }
    }

    #[test]
    fn test_match_from_stdlib() {
        // Real example from stdlib result.atom
        let code = "unwrap(op Option(t), msg String) t { match(op) { Some(inner) { inner } None { error(msg) } } }";
        
        let items = parse(code).unwrap();
        assert_eq!(items.len(), 1);
        
        match &items[0] {
            TopLevel::Function(func) => {
                assert_eq!(func.name.name, "unwrap");
                // The body should contain a match expression
                assert!(!func.body.stmts.is_empty());
                // The first (and only) statement should be an expression containing a match
                if let Stmt::Expression(Expr::Match { arms, .. }) = &func.body.stmts[0] {
                    assert_eq!(arms.len(), 2);
                } else {
                    panic!("Expected match expression in function body, got {:?}", func.body.stmts[0]);
                }
            },
            _ => panic!("Expected function definition"),
        }
    }

    #[test]
    fn test_pattern_alternatives() {
        // Test pattern alternatives with | operator
        let code = "match(token) { LBrace | RBrace | LParen | RParen { True } _ { False } }";
        let expr = parse_expr(code).unwrap();
        
        match expr {
            Expr::Match { arms, .. } => {
                assert_eq!(arms.len(), 2);
                
                // First arm should have alternative pattern with 4 patterns
                match &arms[0].pattern {
                    Pattern::Alternative(patterns, _) => {
                        assert_eq!(patterns.len(), 4);
                        // Check that all patterns are enum patterns
                        for pat in patterns {
                            assert!(matches!(pat, Pattern::Enum { .. }));
                        }
                    }
                    _ => panic!("Expected alternative pattern, got {:?}", arms[0].pattern),
                }
                
                // Second arm should be wildcard
                assert!(matches!(arms[1].pattern, Pattern::Wildcard(_)));
            }
            _ => panic!("Expected match expression, got {:?}", expr),
        }
    }

    #[test]
    fn test_pattern_alternatives_literals() {
        // Test pattern alternatives with literal values
        let code = "match(x) { 1 | 2 | 3 { \"small\" } 10 | 20 | 30 { \"big\" } _ { \"other\" } }";
        let expr = parse_expr(code).unwrap();
        
        match expr {
            Expr::Match { arms, .. } => {
                assert_eq!(arms.len(), 3);
                
                // First arm: 1 | 2 | 3
                match &arms[0].pattern {
                    Pattern::Alternative(patterns, _) => {
                        assert_eq!(patterns.len(), 3);
                        for pat in patterns {
                            assert!(matches!(pat, Pattern::Literal(Literal::Integer(_), _)));
                        }
                    }
                    _ => panic!("Expected alternative pattern"),
                }
                
                // Second arm: 10 | 20 | 30
                match &arms[1].pattern {
                    Pattern::Alternative(patterns, _) => {
                        assert_eq!(patterns.len(), 3);
                    }
                    _ => panic!("Expected alternative pattern"),
                }
            }
            _ => panic!("Expected match expression"),
        }
    }
}
