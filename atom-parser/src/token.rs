use crate::span::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Identifiers (no keywords in Atom!)
    /// Identifier starting with uppercase (Type names, enum variants)
    TypeIdent,
    /// Identifier starting with lowercase or underscore (functions, variables, fields)
    ValueIdent,
    /// Dollar identifier ($0, $1, etc. - loop iteration variables)
    DollarIdent,

    // Literals
    Integer,
    Float,
    String,
    Rune,

    // Operators (single char)
    Plus,       // +
    Minus,      // -
    Star,       // *
    Slash,      // /
    Percent,    // %
    Eq,         // =
    Lt,         // <
    Gt,         // >
    And,        // &
    Or,         // |
    Not,        // !
    Question,   // ?
    Tilde,      // ~
    Dot,        // .
    Comma,      // ,
    Colon,      // :
    Semicolon,  // ;
    Hash,       // #

    // Multi-char operators
    PlusPlus,   // ++
    PlusEq,     // +=
    MinusEq,    // -=
    StarEq,     // *=
    SlashEq,    // /=
    PercentEq,  // %=
    EqEq,       // ==
    NotEq,      // !=
    LtEq,       // <=
    GtEq,       // >=
    AndAnd,     // &&
    OrOr,       // ||
    LShift,     // <<
    RShift,     // >>
    ColonEq,    // :=
    ColonColon, // ::
    Arrow,      // ->
    DotDot,     // ..
    PlusPlusEq, // ++=

    // Delimiters
    LParen,     // (
    RParen,     // )
    LBrace,     // {
    RBrace,     // }
    LBracket,   // [
    RBracket,   // ]

    // Special
    /// Inserted by ASI or explicit semicolon
    NewlineOrSemi,
    /// Comment (line or doc)
    Comment,
    /// End of file
    Eof,
}

impl TokenKind {
    /// Check if this token is a continuation character for ASI
    /// Lines starting with these do NOT get a semicolon inserted
    pub fn is_continuation(&self) -> bool {
        matches!(
            self,
            TokenKind::Dot
                | TokenKind::Plus
                | TokenKind::Minus
                | TokenKind::Star
                | TokenKind::Slash
                | TokenKind::Percent
                | TokenKind::Eq
                | TokenKind::Lt
                | TokenKind::Gt
                | TokenKind::And
                | TokenKind::Or
                | TokenKind::Comma
                | TokenKind::RParen
                | TokenKind::RBracket
                | TokenKind::RBrace
        )
    }
}
