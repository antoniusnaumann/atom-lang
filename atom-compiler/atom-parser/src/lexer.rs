use crate::{
    error::{ParseError, ParseResult},
    token::{Token, TokenKind},
};
use atom_ast::Span;

pub struct Lexer {
    input: Vec<char>,
    pos: usize,
}

impl Lexer {
    pub fn new(input: &str) -> Self {
        Self {
            input: input.chars().collect(),
            pos: 0,
        }
    }

    pub fn tokenize(&mut self) -> ParseResult<Vec<Token>> {
        let mut tokens = Vec::new();
        let mut last_was_newline = false;

        while !self.is_at_end() {
            // Skip whitespace except newlines
            self.skip_whitespace_except_newlines();
            
            // Skip comments
            if self.current() == '/' && self.peek() == Some('/') {
                while !self.is_at_end() && self.current() != '\n' {
                    self.advance();
                }
                continue;
            }
            
            if self.is_at_end() {
                break;
            }
            
            // Handle newlines
            if self.current() == '\n' {
                self.advance();
                last_was_newline = true;
                continue;
            }
            
            // Insert semicolon if we had a newline and next token is not a continuation
            if last_was_newline && self.should_insert_asi_before_token() {
                let span = Span::new(self.pos, self.pos);
                tokens.push(Token {
                    kind: TokenKind::NewlineOrSemi,
                    span,
                    text: String::from(";"),
                });
            }
            last_was_newline = false;

            let token = self.next_token()?;
            tokens.push(token);
        }

        // Add EOF token
        tokens.push(Token {
            kind: TokenKind::Eof,
            span: Span::new(self.pos, self.pos),
            text: String::new(),
        });

        Ok(tokens)
    }

    fn should_insert_asi_before_token(&self) -> bool {
        if self.is_at_end() {
            return false;
        }
        
        let ch = self.current();
        
        // Don't insert semicolon if line starts with continuation character
        !matches!(ch, '.' | '+' | '-' | '*' | '/' | '%' | '=' | '<' | '>' | '&' | '|' | ',' | ')' | ']' | '}')
    }

    fn next_token(&mut self) -> ParseResult<Token> {
        let start = self.pos;
        let ch = self.current();

        match ch {
            // Single-char delimiters
            '(' => self.make_token(TokenKind::LParen, start),
            ')' => self.make_token(TokenKind::RParen, start),
            '{' => self.make_token(TokenKind::LBrace, start),
            '}' => self.make_token(TokenKind::RBrace, start),
            ',' => self.make_token(TokenKind::Comma, start),
            ';' => self.make_token(TokenKind::NewlineOrSemi, start),
            '?' => self.make_token(TokenKind::Question, start),
            '~' => self.make_token(TokenKind::Tilde, start),
            '#' => self.make_token(TokenKind::Hash, start),

            // Operators (potentially multi-char)
            '+' => {
                self.advance();
                if self.match_char('+') {
                    if self.match_char('=') {
                        Ok(self.make_simple_token(TokenKind::PlusPlusEq, start))
                    } else {
                        Ok(self.make_simple_token(TokenKind::PlusPlus, start))
                    }
                } else if self.match_char('=') {
                    Ok(self.make_simple_token(TokenKind::PlusEq, start))
                } else {
                    Ok(self.make_simple_token(TokenKind::Plus, start))
                }
            }
            '-' => {
                self.advance();
                if self.match_char('>') {
                    Ok(self.make_simple_token(TokenKind::Arrow, start))
                } else if self.match_char('=') {
                    Ok(self.make_simple_token(TokenKind::MinusEq, start))
                } else {
                    Ok(self.make_simple_token(TokenKind::Minus, start))
                }
            }
            '*' => {
                self.advance();
                if self.match_char('=') {
                    Ok(self.make_simple_token(TokenKind::StarEq, start))
                } else {
                    Ok(self.make_simple_token(TokenKind::Star, start))
                }
            }
            '/' => {
                self.advance();
                if self.match_char('=') {
                    Ok(self.make_simple_token(TokenKind::SlashEq, start))
                } else {
                    Ok(self.make_simple_token(TokenKind::Slash, start))
                }
            }
            '%' => {
                self.advance();
                if self.match_char('=') {
                    Ok(self.make_simple_token(TokenKind::PercentEq, start))
                } else {
                    Ok(self.make_simple_token(TokenKind::Percent, start))
                }
            }
            '=' => {
                self.advance();
                if self.match_char('=') {
                    Ok(self.make_simple_token(TokenKind::EqEq, start))
                } else {
                    Ok(self.make_simple_token(TokenKind::Eq, start))
                }
            }
            '!' => {
                self.advance();
                if self.match_char('=') {
                    Ok(self.make_simple_token(TokenKind::NotEq, start))
                } else {
                    Ok(self.make_simple_token(TokenKind::Not, start))
                }
            }
            '<' => {
                self.advance();
                if self.match_char('=') {
                    Ok(self.make_simple_token(TokenKind::LtEq, start))
                } else if self.match_char('<') {
                    Ok(self.make_simple_token(TokenKind::LShift, start))
                } else {
                    Ok(self.make_simple_token(TokenKind::Lt, start))
                }
            }
            '>' => {
                self.advance();
                if self.match_char('=') {
                    Ok(self.make_simple_token(TokenKind::GtEq, start))
                } else if self.match_char('>') {
                    Ok(self.make_simple_token(TokenKind::RShift, start))
                } else {
                    Ok(self.make_simple_token(TokenKind::Gt, start))
                }
            }
            '&' => {
                self.advance();
                if self.match_char('&') {
                    Ok(self.make_simple_token(TokenKind::AndAnd, start))
                } else {
                    Ok(self.make_simple_token(TokenKind::And, start))
                }
            }
            '|' => {
                self.advance();
                if self.match_char('|') {
                    Ok(self.make_simple_token(TokenKind::OrOr, start))
                } else {
                    Ok(self.make_simple_token(TokenKind::Or, start))
                }
            }
            ':' => {
                self.advance();
                if self.match_char(':') {
                    Ok(self.make_simple_token(TokenKind::ColonColon, start))
                } else if self.match_char('=') {
                    Ok(self.make_simple_token(TokenKind::ColonEq, start))
                } else {
                    Ok(self.make_simple_token(TokenKind::Colon, start))
                }
            }
            '.' => {
                self.advance();
                if self.match_char('.') {
                    Ok(self.make_simple_token(TokenKind::DotDot, start))
                } else {
                    Ok(self.make_simple_token(TokenKind::Dot, start))
                }
            }

            // String literals
            '"' => self.lex_string(start),

            // Rune literals
            '\'' => self.lex_rune(start),

            // Numbers
            '0'..='9' => self.lex_number(start),

            // Identifiers (and keywords in other languages, but Atom has none!)
            'A'..='Z' => self.lex_type_ident(start),
            'a'..='z' | '_' => self.lex_value_ident(start),
            
            // Dollar identifiers for loop iteration variables
            '$' => self.lex_dollar_ident(start),

            _ => Err(ParseError::new(
                format!("Unexpected character: '{}'", ch),
                Span::new(start, self.pos + 1),
            )),
        }
    }

    fn lex_string(&mut self, start: usize) -> ParseResult<Token> {
        self.advance(); // Skip opening "
        let mut value = String::new();

        while !self.is_at_end() && self.current() != '"' {
            if self.current() == '\\' {
                self.advance();
                if self.is_at_end() {
                    return Err(ParseError::new(
                        "Unterminated string literal",
                        Span::new(start, self.pos),
                    ));
                }
                // Handle escape sequences
                match self.current() {
                    'n' => value.push('\n'),
                    't' => value.push('\t'),
                    'r' => value.push('\r'),
                    '\\' => value.push('\\'),
                    '"' => value.push('"'),
                    '(' => {
                        // TODO: Handle string interpolation
                        value.push_str("\\(");
                    }
                    _ => {
                        value.push('\\');
                        value.push(self.current());
                    }
                }
                self.advance();
            } else {
                value.push(self.current());
                self.advance();
            }
        }

        if self.is_at_end() {
            return Err(ParseError::new(
                "Unterminated string literal",
                Span::new(start, self.pos),
            ));
        }

        self.advance(); // Skip closing "

        Ok(Token {
            kind: TokenKind::String,
            span: Span::new(start, self.pos),
            text: value,
        })
    }

    fn lex_rune(&mut self, start: usize) -> ParseResult<Token> {
        self.advance(); // Skip opening '

        if self.is_at_end() {
            return Err(ParseError::new(
                "Unterminated rune literal",
                Span::new(start, self.pos),
            ));
        }

        let ch = if self.current() == '\\' {
            self.advance();
            if self.is_at_end() {
                return Err(ParseError::new(
                    "Unterminated rune literal",
                    Span::new(start, self.pos),
                ));
            }
            match self.current() {
                'n' => '\n',
                't' => '\t',
                'r' => '\r',
                '\\' => '\\',
                '\'' => '\'',
                _ => self.current(),
            }
        } else {
            self.current()
        };

        self.advance();

        if self.is_at_end() || self.current() != '\'' {
            return Err(ParseError::new(
                "Unterminated rune literal",
                Span::new(start, self.pos),
            ));
        }

        self.advance(); // Skip closing '

        Ok(Token {
            kind: TokenKind::Rune,
            span: Span::new(start, self.pos),
            text: ch.to_string(),
        })
    }

    fn lex_number(&mut self, start: usize) -> ParseResult<Token> {
        // Check for hex, binary, or octal literals
        if self.current() == '0' && !self.is_at_end()
            && let Some(next) = self.peek() {
                match next {
                    'x' | 'X' => {
                        self.advance(); // Skip '0'
                        self.advance(); // Skip 'x' or 'X'
                        
                        // Parse hex digits
                        if self.is_at_end() || !self.current().is_ascii_hexdigit() {
                            return Err(ParseError::new(
                                "Expected hex digit after '0x'",
                                Span::new(start, self.pos),
                            ));
                        }
                        
                        while !self.is_at_end() && self.current().is_ascii_hexdigit() {
                            self.advance();
                        }
                        
                        return Ok(Token {
                            kind: TokenKind::Integer,
                            span: Span::new(start, self.pos),
                            text: self.slice(start, self.pos),
                        });
                    }
                    'b' | 'B' => {
                        self.advance(); // Skip '0'
                        self.advance(); // Skip 'b' or 'B'
                        
                        // Parse binary digits
                        if self.is_at_end() || !matches!(self.current(), '0' | '1') {
                            return Err(ParseError::new(
                                "Expected binary digit after '0b'",
                                Span::new(start, self.pos),
                            ));
                        }
                        
                        while !self.is_at_end() && matches!(self.current(), '0' | '1') {
                            self.advance();
                        }
                        
                        return Ok(Token {
                            kind: TokenKind::Integer,
                            span: Span::new(start, self.pos),
                            text: self.slice(start, self.pos),
                        });
                    }
                    'o' | 'O' => {
                        self.advance(); // Skip '0'
                        self.advance(); // Skip 'o' or 'O'
                        
                        // Parse octal digits
                        if self.is_at_end() || !matches!(self.current(), '0'..='7') {
                            return Err(ParseError::new(
                                "Expected octal digit after '0o'",
                                Span::new(start, self.pos),
                            ));
                        }
                        
                        while !self.is_at_end() && matches!(self.current(), '0'..='7') {
                            self.advance();
                        }
                        
                        return Ok(Token {
                            kind: TokenKind::Integer,
                            span: Span::new(start, self.pos),
                            text: self.slice(start, self.pos),
                        });
                    }
                    _ => {} // Fall through to decimal parsing
                }
            }
        
        // Parse decimal digits
        while !self.is_at_end() && self.current().is_ascii_digit() {
            self.advance();
        }

        // Check for float
        if !self.is_at_end() && self.current() == '.' && self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.advance(); // Skip '.'
            while !self.is_at_end() && self.current().is_ascii_digit() {
                self.advance();
            }
            Ok(Token {
                kind: TokenKind::Float,
                span: Span::new(start, self.pos),
                text: self.slice(start, self.pos),
            })
        } else {
            Ok(Token {
                kind: TokenKind::Integer,
                span: Span::new(start, self.pos),
                text: self.slice(start, self.pos),
            })
        }
    }

    fn lex_type_ident(&mut self, start: usize) -> ParseResult<Token> {
        while !self.is_at_end() && (self.current().is_alphanumeric() || self.current() == '_') {
            self.advance();
        }

        Ok(Token {
            kind: TokenKind::TypeIdent,
            span: Span::new(start, self.pos),
            text: self.slice(start, self.pos),
        })
    }

    fn lex_value_ident(&mut self, start: usize) -> ParseResult<Token> {
        while !self.is_at_end() && (self.current().is_alphanumeric() || self.current() == '_') {
            self.advance();
        }

        Ok(Token {
            kind: TokenKind::ValueIdent,
            span: Span::new(start, self.pos),
            text: self.slice(start, self.pos),
        })
    }

    fn lex_dollar_ident(&mut self, start: usize) -> ParseResult<Token> {
        self.advance(); // Skip '$'
        
        // Must be followed by digits
        if self.is_at_end() || !self.current().is_ascii_digit() {
            return Err(ParseError::new(
                "Expected digit after '$'",
                Span::new(start, self.pos),
            ));
        }
        
        while !self.is_at_end() && self.current().is_ascii_digit() {
            self.advance();
        }

        Ok(Token {
            kind: TokenKind::DollarIdent,
            span: Span::new(start, self.pos),
            text: self.slice(start, self.pos),
        })
    }

    fn skip_whitespace_except_newlines(&mut self) {
        loop {
            if self.is_at_end() {
                break;
            }

            match self.current() {
                ' ' | '\t' | '\r' => {
                    self.advance();
                }
                _ => break,
            }
        }
    }

    // Helper methods
    fn current(&self) -> char {
        self.input[self.pos]
    }

    fn peek(&self) -> Option<char> {
        if self.pos + 1 < self.input.len() {
            Some(self.input[self.pos + 1])
        } else {
            None
        }
    }

    fn is_at_end(&self) -> bool {
        self.pos >= self.input.len()
    }

    fn advance(&mut self) {
        if !self.is_at_end() {
            self.pos += 1;
        }
    }

    fn match_char(&mut self, expected: char) -> bool {
        if self.is_at_end() || self.current() != expected {
            false
        } else {
            self.advance();
            true
        }
    }

    fn make_token(&mut self, kind: TokenKind, start: usize) -> ParseResult<Token> {
        self.advance();
        Ok(self.make_simple_token(kind, start))
    }

    fn make_simple_token(&self, kind: TokenKind, start: usize) -> Token {
        Token {
            kind,
            span: Span::new(start, self.pos),
            text: self.slice(start, self.pos),
        }
    }

    fn slice(&self, start: usize, end: usize) -> String {
        self.input[start..end].iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_tokens() {
        let mut lexer = Lexer::new("+ - * / ( ) { }");
        let tokens = lexer.tokenize().unwrap();
        
        assert_eq!(tokens[0].kind, TokenKind::Plus);
        assert_eq!(tokens[1].kind, TokenKind::Minus);
        assert_eq!(tokens[2].kind, TokenKind::Star);
        assert_eq!(tokens[3].kind, TokenKind::Slash);
        assert_eq!(tokens[4].kind, TokenKind::LParen);
        assert_eq!(tokens[5].kind, TokenKind::RParen);
        assert_eq!(tokens[6].kind, TokenKind::LBrace);
        assert_eq!(tokens[7].kind, TokenKind::RBrace);
        assert_eq!(tokens[8].kind, TokenKind::Eof);
    }

    #[test]
    fn test_identifiers() {
        let mut lexer = Lexer::new("main MyType foo_bar");
        let tokens = lexer.tokenize().unwrap();
        
        assert_eq!(tokens[0].kind, TokenKind::ValueIdent);
        assert_eq!(tokens[0].text, "main");
        assert_eq!(tokens[1].kind, TokenKind::TypeIdent);
        assert_eq!(tokens[1].text, "MyType");
        assert_eq!(tokens[2].kind, TokenKind::ValueIdent);
        assert_eq!(tokens[2].text, "foo_bar");
    }

    #[test]
    fn test_numbers() {
        let mut lexer = Lexer::new("42 3.14");
        let tokens = lexer.tokenize().unwrap();
        
        assert_eq!(tokens[0].kind, TokenKind::Integer);
        assert_eq!(tokens[0].text, "42");
        assert_eq!(tokens[1].kind, TokenKind::Float);
        assert_eq!(tokens[1].text, "3.14");
    }

    #[test]
    fn test_strings() {
        let mut lexer = Lexer::new(r#""hello world""#);
        let tokens = lexer.tokenize().unwrap();
        
        assert_eq!(tokens[0].kind, TokenKind::String);
        assert_eq!(tokens[0].text, "hello world");
    }

    #[test]
    fn test_hex_literals() {
        let mut lexer = Lexer::new("0xFFFD 0x10 0xFF");
        let tokens = lexer.tokenize().unwrap();
        
        assert_eq!(tokens[0].kind, TokenKind::Integer);
        assert_eq!(tokens[0].text, "0xFFFD");
        assert_eq!(tokens[1].kind, TokenKind::Integer);
        assert_eq!(tokens[1].text, "0x10");
        assert_eq!(tokens[2].kind, TokenKind::Integer);
        assert_eq!(tokens[2].text, "0xFF");
    }

    #[test]
    fn test_binary_literals() {
        let mut lexer = Lexer::new("0b1010 0b11111111");
        let tokens = lexer.tokenize().unwrap();
        
        assert_eq!(tokens[0].kind, TokenKind::Integer);
        assert_eq!(tokens[0].text, "0b1010");
        assert_eq!(tokens[1].kind, TokenKind::Integer);
        assert_eq!(tokens[1].text, "0b11111111");
    }

    #[test]
    fn test_octal_literals() {
        let mut lexer = Lexer::new("0o755 0o644");
        let tokens = lexer.tokenize().unwrap();
        
        assert_eq!(tokens[0].kind, TokenKind::Integer);
        assert_eq!(tokens[0].text, "0o755");
        assert_eq!(tokens[1].kind, TokenKind::Integer);
        assert_eq!(tokens[1].text, "0o644");
    }
}
