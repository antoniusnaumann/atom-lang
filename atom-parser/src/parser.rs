use crate::{
    ast::*,
    error::{ParseError, ParseResult},
    span::Span,
    token::{Token, TokenKind},
};

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
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

        Ok(items)
    }

    fn parse_top_level(&mut self) -> ParseResult<TopLevel> {
        let start = self.current_span();

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
            // Could be function definition or variable declaration
            let name = self.expect_value_ident()?;
            
            if self.check(&TokenKind::LParen) {
                // Function definition
                self.parse_function_def(visibility, name, start)
            } else {
                // Variable/constant declaration
                self.parse_top_level_var_decl(visibility, name, start)
            }
        } else if self.check(&TokenKind::String) {
            // Test block with name
            self.parse_test_block(start)
        } else if self.check(&TokenKind::LBrace) {
            // Anonymous test block or top-level code in test files
            self.parse_test_block(start)
        } else {
            Err(self.error("Expected top-level item (struct, enum, function, or variable)"))
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
        if self.check(&TokenKind::ValueIdent) && self.peek_ahead(1).map_or(false, |t| 
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
        
        // Parse type parameters if there's a semicolon before enum cases
        // Check if we have type parameters by looking for semicolon
        let saved_pos = self.pos;
        let mut has_type_params = false;
        
        // Look ahead to see if there's a semicolon (indicating type params before cases)
        let mut depth = 0;
        while !self.is_at_end() {
            if self.check(&TokenKind::NewlineOrSemi) && depth == 0 {
                // This could be a semicolon separator - mark it for type param parsing
                has_type_params = true;
                break;
            }
            if self.check(&TokenKind::LParen) {
                depth += 1;
            } else if self.check(&TokenKind::RParen) {
                if depth == 0 {
                    break;
                }
                depth -= 1;
            }
            self.advance();
        }
        
        // Restore position
        self.pos = saved_pos;
        
        if has_type_params {
            // Parse type parameters
            while !self.check(&TokenKind::NewlineOrSemi) && !self.check(&TokenKind::RParen) {
                type_params.push(self.parse_type_param()?);
                
                // Check what comes next
                if self.match_token(&TokenKind::Comma) {
                    // More type params, skip newlines and continue
                    while self.match_token(&TokenKind::NewlineOrSemi) {}
                } else {
                    // No comma, must be at the semicolon separator or RParen
                    break;
                }
            }
            
            // Expect the semicolon separator (which is lexed as NewlineOrSemi)
            if !self.check(&TokenKind::RParen) {
                self.expect(&TokenKind::NewlineOrSemi)?;
                while self.match_token(&TokenKind::NewlineOrSemi) {}
            }
        }
        
        // Parse enum cases
        while !self.check(&TokenKind::RParen) {
            cases.push(self.parse_enum_case()?);
            
            if !self.match_token(&TokenKind::Comma) && !self.match_token(&TokenKind::NewlineOrSemi) {
                break;
            }
            
            // Skip trailing newlines/semicolons
            while self.match_token(&TokenKind::NewlineOrSemi) {}
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
            Some(Box::new(self.parse_type()?))
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
        let name = self.expect_value_ident()?;
        
        // Parse type if present
        let ty = if !self.check(&TokenKind::Comma) && !self.check(&TokenKind::RParen) && 
                    !self.check(&TokenKind::Eq) && !self.check(&TokenKind::Semicolon) {
            Some(Box::new(self.parse_type()?))
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
            name,
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

    fn parse_statement(&mut self) -> ParseResult<Stmt> {
        // Check if this is a variable declaration
        if self.check(&TokenKind::ValueIdent) {
            let saved_pos = self.pos;
            let _name = self.advance();
            
            // Look ahead for := or :
            if self.check(&TokenKind::ColonEq) || self.check(&TokenKind::Colon) {
                // It's a variable declaration
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
        let name = self.expect_value_ident()?;
        
        let (ty, init) = if self.match_token(&TokenKind::ColonEq) {
            (None, Some(Box::new(self.parse_expression()?)))
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
            name,
            ty,
            init,
            span: start.merge(self.previous_span()),
        })
    }

    fn parse_type(&mut self) -> ParseResult<Type> {
        let start = self.current_span();
        
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
                    // TODO: Parse type parameters properly
                    params.push(Box::new(TypeParam {
                        name: None,
                        ty: Some(Box::new(self.parse_type()?)),
                        default: None,
                        span: self.current_span(),
                    }));
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

    fn parse_expression(&mut self) -> ParseResult<Expr> {
        self.parse_binary_expression(0)
    }

    fn parse_binary_expression(&mut self, min_precedence: u8) -> ParseResult<Expr> {
        let mut left = self.parse_unary_expression()?;
        
        while let Some((op, precedence)) = self.get_binary_op() {
            if precedence < min_precedence {
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
                if self.check(&TokenKind::LBrace) {
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
                let method = self.expect_value_ident()?;
                
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
            let value = tok.text.parse::<i64>()
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
            } else {
                // Empty tuple
                return Ok(Expr::Tuple(Vec::new(), start.merge(self.previous_span())));
            }
        }
        
        // Try to parse as closure parameters first
        let checkpoint = self.pos;
        if let Ok(params) = self.try_parse_closure_params() {
            self.expect(&TokenKind::RParen).ok();
            
            if self.check(&TokenKind::LBrace) || self.check(&TokenKind::Arrow) {
                // It's a closure!
                return self.parse_closure_body(params, start);
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
        // Parse optional return type
        let return_type = if self.match_token(&TokenKind::Arrow) {
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

    fn error(&self, message: &str) -> ParseError {
        ParseError::new(message, self.current_span())
    }
}

impl Clone for Parser {
    fn clone(&self) -> Self {
        Self {
            tokens: self.tokens.clone(),
            pos: self.pos,
        }
    }
}

// Add span() method to Expr
impl Expr {
    pub fn span(&self) -> Span {
        match self {
            Expr::Literal(_, span) => *span,
            Expr::Ident(ident) => ident.span,
            Expr::Binary { span, .. } => *span,
            Expr::Unary { span, .. } => *span,
            Expr::Call { span, .. } => *span,
            Expr::MethodCall { span, .. } => *span,
            Expr::FieldAccess { span, .. } => *span,
            Expr::Tuple(_, span) => *span,
            Expr::StructInit { span, .. } => *span,
            Expr::Closure { span, .. } => *span,
            Expr::Block(block) => block.span,
            Expr::Match { span, .. } => *span,
            Expr::Loop { span, .. } => *span,
            Expr::Return { span, .. } => *span,
            Expr::Break(span) => *span,
            Expr::Continue(span) => *span,
            Expr::Comptime { span, .. } => *span,
        }
    }
}
