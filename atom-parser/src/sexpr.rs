//! S-Expression printer for Atom AST
//!
//! This module provides functionality to convert Atom AST nodes into
//! S-Expression format for debugging and tooling purposes.

use crate::ast::*;
use std::fmt::{self, Write};

/// Helper trait for converting AST nodes to S-Expressions
pub trait ToSExpr {
    fn to_sexpr(&self) -> String;
    fn write_sexpr(&self, f: &mut impl Write, indent: usize) -> fmt::Result;
}

/// Helper function to write indentation
fn write_indent(f: &mut impl Write, indent: usize) -> fmt::Result {
    for _ in 0..indent {
        write!(f, "  ")?;
    }
    Ok(())
}

impl ToSExpr for Vec<TopLevel> {
    fn to_sexpr(&self) -> String {
        let mut s = String::new();
        self.write_sexpr(&mut s, 0).unwrap();
        s
    }

    fn write_sexpr(&self, f: &mut impl Write, indent: usize) -> fmt::Result {
        writeln!(f, "(program")?;
        for item in self {
            item.write_sexpr(f, indent + 1)?;
        }
        write!(f, ")")?;
        Ok(())
    }
}

impl ToSExpr for TopLevel {
    fn to_sexpr(&self) -> String {
        let mut s = String::new();
        self.write_sexpr(&mut s, 0).unwrap();
        s
    }

    fn write_sexpr(&self, f: &mut impl Write, indent: usize) -> fmt::Result {
        match self {
            TopLevel::Import(decl) => decl.write_sexpr(f, indent),
            TopLevel::Struct(def) => def.write_sexpr(f, indent),
            TopLevel::Enum(def) => def.write_sexpr(f, indent),
            TopLevel::Function(def) => def.write_sexpr(f, indent),
            TopLevel::Variable(decl) => decl.write_sexpr(f, indent),
            TopLevel::TestBlock(block) => block.write_sexpr(f, indent),
            TopLevel::Statement(stmt) => stmt.write_sexpr(f, indent),
        }
    }
}

impl ToSExpr for Visibility {
    fn to_sexpr(&self) -> String {
        match self {
            Visibility::Public => "public".to_string(),
            Visibility::FilePrivate => "file-private".to_string(),
            Visibility::Internal => "internal".to_string(),
        }
    }

    fn write_sexpr(&self, f: &mut impl Write, _indent: usize) -> fmt::Result {
        write!(f, "{}", self.to_sexpr())
    }
}

impl ToSExpr for ImportDecl {
    fn to_sexpr(&self) -> String {
        let mut s = String::new();
        self.write_sexpr(&mut s, 0).unwrap();
        s
    }

    fn write_sexpr(&self, f: &mut impl Write, indent: usize) -> fmt::Result {
        write_indent(f, indent)?;
        write!(f, "(import {} ", self.namespace.name)?;
        self.items.write_sexpr(f, 0)?;
        writeln!(f, ")")?;
        Ok(())
    }
}

impl ToSExpr for ImportItems {
    fn to_sexpr(&self) -> String {
        let mut s = String::new();
        self.write_sexpr(&mut s, 0).unwrap();
        s
    }

    fn write_sexpr(&self, f: &mut impl Write, _indent: usize) -> fmt::Result {
        match self {
            ImportItems::All => write!(f, "*"),
            ImportItems::Named(items) => {
                write!(f, "(")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{}", item.name)?;
                }
                write!(f, ")")
            }
        }
    }
}

impl ToSExpr for StructDef {
    fn to_sexpr(&self) -> String {
        let mut s = String::new();
        self.write_sexpr(&mut s, 0).unwrap();
        s
    }

    fn write_sexpr(&self, f: &mut impl Write, indent: usize) -> fmt::Result {
        write_indent(f, indent)?;
        write!(f, "(struct {} ", self.visibility.to_sexpr())?;
        write!(f, "{}", self.name.name)?;
        
        if !self.type_params.is_empty() {
            write!(f, " (type-params")?;
            for param in &self.type_params {
                write!(f, " ")?;
                param.write_sexpr(f, 0)?;
            }
            write!(f, ")")?;
        }
        
        writeln!(f)?;
        for field in &self.fields {
            field.write_sexpr(f, indent + 1)?;
        }
        write_indent(f, indent)?;
        writeln!(f, ")")?;
        Ok(())
    }
}

impl ToSExpr for Field {
    fn to_sexpr(&self) -> String {
        let mut s = String::new();
        self.write_sexpr(&mut s, 0).unwrap();
        s
    }

    fn write_sexpr(&self, f: &mut impl Write, indent: usize) -> fmt::Result {
        write_indent(f, indent)?;
        write!(f, "(field ")?;
        if let Some(name) = &self.name {
            write!(f, "{} ", name.name)?;
        }
        self.ty.write_sexpr(f, 0)?;
        writeln!(f, ")")?;
        Ok(())
    }
}

impl ToSExpr for EnumDef {
    fn to_sexpr(&self) -> String {
        let mut s = String::new();
        self.write_sexpr(&mut s, 0).unwrap();
        s
    }

    fn write_sexpr(&self, f: &mut impl Write, indent: usize) -> fmt::Result {
        write_indent(f, indent)?;
        write!(f, "(enum {} ", self.visibility.to_sexpr())?;
        write!(f, "{}", self.name.name)?;
        
        if !self.type_params.is_empty() {
            write!(f, " (type-params")?;
            for param in &self.type_params {
                write!(f, " ")?;
                param.write_sexpr(f, 0)?;
            }
            write!(f, ")")?;
        }
        
        writeln!(f)?;
        for case in &self.cases {
            case.write_sexpr(f, indent + 1)?;
        }
        write_indent(f, indent)?;
        writeln!(f, ")")?;
        Ok(())
    }
}

impl ToSExpr for EnumCase {
    fn to_sexpr(&self) -> String {
        let mut s = String::new();
        self.write_sexpr(&mut s, 0).unwrap();
        s
    }

    fn write_sexpr(&self, f: &mut impl Write, indent: usize) -> fmt::Result {
        write_indent(f, indent)?;
        write!(f, "(case {}", self.name.name)?;
        if !self.fields.is_empty() {
            for field in &self.fields {
                write!(f, " ")?;
                field.write_sexpr(f, 0)?;
            }
        }
        writeln!(f, ")")?;
        Ok(())
    }
}

impl ToSExpr for TypeParam {
    fn to_sexpr(&self) -> String {
        let mut s = String::new();
        self.write_sexpr(&mut s, 0).unwrap();
        s
    }

    fn write_sexpr(&self, f: &mut impl Write, _indent: usize) -> fmt::Result {
        write!(f, "(")?;
        if let Some(name) = &self.name {
            write!(f, "{}", name.name)?;
        }
        if let Some(ty) = &self.ty {
            write!(f, " ")?;
            ty.write_sexpr(f, 0)?;
        }
        if let Some(default) = &self.default {
            write!(f, " :default ")?;
            default.write_sexpr(f, 0)?;
        }
        write!(f, ")")
    }
}

impl ToSExpr for FunctionDef {
    fn to_sexpr(&self) -> String {
        let mut s = String::new();
        self.write_sexpr(&mut s, 0).unwrap();
        s
    }

    fn write_sexpr(&self, f: &mut impl Write, indent: usize) -> fmt::Result {
        write_indent(f, indent)?;
        write!(f, "(function {} {}", self.visibility.to_sexpr(), self.name.name)?;
        
        if !self.const_params.is_empty() {
            write!(f, " (const-params")?;
            for param in &self.const_params {
                write!(f, " ")?;
                param.write_sexpr(f, 0)?;
            }
            write!(f, ")")?;
        }
        
        write!(f, " (params")?;
        for param in &self.params {
            write!(f, " ")?;
            param.write_sexpr(f, 0)?;
        }
        write!(f, ")")?;
        
        if let Some(ret_type) = &self.return_type {
            write!(f, " (returns ")?;
            ret_type.write_sexpr(f, 0)?;
            write!(f, ")")?;
        }
        
        writeln!(f)?;
        self.body.write_sexpr(f, indent + 1)?;
        write_indent(f, indent)?;
        writeln!(f, ")")?;
        Ok(())
    }
}

impl ToSExpr for Param {
    fn to_sexpr(&self) -> String {
        let mut s = String::new();
        self.write_sexpr(&mut s, 0).unwrap();
        s
    }

    fn write_sexpr(&self, f: &mut impl Write, _indent: usize) -> fmt::Result {
        write!(f, "({}", self.name.name)?;
        if let Some(ty) = &self.ty {
            write!(f, " ")?;
            ty.write_sexpr(f, 0)?;
        }
        if let Some(default) = &self.default {
            write!(f, " :default ")?;
            default.write_sexpr(f, 0)?;
        }
        write!(f, ")")
    }
}

impl ToSExpr for VarDecl {
    fn to_sexpr(&self) -> String {
        let mut s = String::new();
        self.write_sexpr(&mut s, 0).unwrap();
        s
    }

    fn write_sexpr(&self, f: &mut impl Write, indent: usize) -> fmt::Result {
        write_indent(f, indent)?;
        write!(f, "({} ", if self.is_const { "const" } else { "var" })?;
        write!(f, "{} ", self.visibility.to_sexpr())?;
        
        if self.names.len() == 1 {
            write!(f, "{}", self.names[0].name)?;
        } else {
            write!(f, "(")?;
            for (i, name) in self.names.iter().enumerate() {
                if i > 0 {
                    write!(f, " ")?;
                }
                write!(f, "{}", name.name)?;
            }
            write!(f, ")")?;
        }
        
        if let Some(ty) = &self.ty {
            write!(f, " ")?;
            ty.write_sexpr(f, 0)?;
        }
        
        if let Some(init) = &self.init {
            write!(f, " ")?;
            init.write_sexpr(f, 0)?;
        }
        
        writeln!(f, ")")?;
        Ok(())
    }
}

impl ToSExpr for TestBlock {
    fn to_sexpr(&self) -> String {
        let mut s = String::new();
        self.write_sexpr(&mut s, 0).unwrap();
        s
    }

    fn write_sexpr(&self, f: &mut impl Write, indent: usize) -> fmt::Result {
        write_indent(f, indent)?;
        write!(f, "(test")?;
        if let Some(name) = &self.name {
            write!(f, " \"{}\"", name.escape_default())?;
        }
        writeln!(f)?;
        self.body.write_sexpr(f, indent + 1)?;
        write_indent(f, indent)?;
        writeln!(f, ")")?;
        Ok(())
    }
}

impl ToSExpr for Block {
    fn to_sexpr(&self) -> String {
        let mut s = String::new();
        self.write_sexpr(&mut s, 0).unwrap();
        s
    }

    fn write_sexpr(&self, f: &mut impl Write, indent: usize) -> fmt::Result {
        write_indent(f, indent)?;
        writeln!(f, "(block")?;
        for stmt in &self.stmts {
            stmt.write_sexpr(f, indent + 1)?;
        }
        write_indent(f, indent)?;
        writeln!(f, ")")?;
        Ok(())
    }
}

impl ToSExpr for Stmt {
    fn to_sexpr(&self) -> String {
        let mut s = String::new();
        self.write_sexpr(&mut s, 0).unwrap();
        s
    }

    fn write_sexpr(&self, f: &mut impl Write, indent: usize) -> fmt::Result {
        match self {
            Stmt::VarDecl(decl) => decl.write_sexpr(f, indent),
            Stmt::Expression(expr) => expr.write_sexpr(f, indent),
        }
    }
}

impl ToSExpr for Type {
    fn to_sexpr(&self) -> String {
        let mut s = String::new();
        self.write_sexpr(&mut s, 0).unwrap();
        s
    }

    fn write_sexpr(&self, f: &mut impl Write, _indent: usize) -> fmt::Result {
        match self {
            Type::Named(name) => write!(f, "{}", name.name),
            Type::Param(name) => write!(f, "(type-param {})", name.name),
            Type::Tuple(types, _) => {
                write!(f, "(tuple")?;
                for ty in types {
                    write!(f, " ")?;
                    ty.write_sexpr(f, 0)?;
                }
                write!(f, ")")
            }
            Type::Generic { name, params, .. } => {
                write!(f, "(generic {}", name.name)?;
                for param in params {
                    write!(f, " ")?;
                    param.write_sexpr(f, 0)?;
                }
                write!(f, ")")
            }
            Type::Variadic { element, non_empty, .. } => {
                if *non_empty {
                    write!(f, "(variadic+ ")?;
                } else {
                    write!(f, "(variadic* ")?;
                }
                element.write_sexpr(f, 0)?;
                write!(f, ")")
            }
            Type::StaticArray { element, size, .. } => {
                write!(f, "(static-array ")?;
                element.write_sexpr(f, 0)?;
                write!(f, " ")?;
                size.write_sexpr(f, 0)?;
                write!(f, ")")
            }
            Type::Function { params, return_type, .. } => {
                write!(f, "(function-type (params")?;
                for param in params {
                    write!(f, " ")?;
                    param.write_sexpr(f, 0)?;
                }
                write!(f, ")")?;
                if let Some(ret) = return_type {
                    write!(f, " (returns ")?;
                    ret.write_sexpr(f, 0)?;
                    write!(f, ")")?;
                }
                write!(f, ")")
            }
        }
    }
}

impl ToSExpr for Expr {
    fn to_sexpr(&self) -> String {
        let mut s = String::new();
        self.write_sexpr(&mut s, 0).unwrap();
        s
    }

    fn write_sexpr(&self, f: &mut impl Write, indent: usize) -> fmt::Result {
        match self {
            Expr::Literal(lit, _) => {
                write_indent(f, indent)?;
                lit.write_sexpr(f, 0)?;
                writeln!(f)?;
            }
            Expr::Ident(ident) => {
                write_indent(f, indent)?;
                writeln!(f, "{}", ident.name)?;
            }
            Expr::Binary { op, left, right, .. } => {
                write_indent(f, indent)?;
                write!(f, "({}", op.to_sexpr())?;
                writeln!(f)?;
                left.write_sexpr(f, indent + 1)?;
                right.write_sexpr(f, indent + 1)?;
                write_indent(f, indent)?;
                writeln!(f, ")")?;
            }
            Expr::Unary { op, expr, .. } => {
                write_indent(f, indent)?;
                write!(f, "({}", op.to_sexpr())?;
                writeln!(f)?;
                expr.write_sexpr(f, indent + 1)?;
                write_indent(f, indent)?;
                writeln!(f, ")")?;
            }
            Expr::Call { func, args, .. } => {
                write_indent(f, indent)?;
                writeln!(f, "(call")?;
                func.write_sexpr(f, indent + 1)?;
                for arg in args {
                    arg.write_sexpr(f, indent + 1)?;
                }
                write_indent(f, indent)?;
                writeln!(f, ")")?;
            }
            Expr::MethodCall { receiver, method, args, .. } => {
                write_indent(f, indent)?;
                writeln!(f, "(method-call")?;
                receiver.write_sexpr(f, indent + 1)?;
                write_indent(f, indent + 1)?;
                writeln!(f, "{}", method.name)?;
                for arg in args {
                    arg.write_sexpr(f, indent + 1)?;
                }
                write_indent(f, indent)?;
                writeln!(f, ")")?;
            }
            Expr::FieldAccess { object, field, .. } => {
                write_indent(f, indent)?;
                writeln!(f, "(field-access")?;
                object.write_sexpr(f, indent + 1)?;
                write_indent(f, indent + 1)?;
                writeln!(f, "{}", field.name)?;
                write_indent(f, indent)?;
                writeln!(f, ")")?;
            }
            Expr::Tuple(exprs, _) => {
                write_indent(f, indent)?;
                writeln!(f, "(tuple")?;
                for expr in exprs {
                    expr.write_sexpr(f, indent + 1)?;
                }
                write_indent(f, indent)?;
                writeln!(f, ")")?;
            }
            Expr::StructInit { ty, fields, .. } => {
                write_indent(f, indent)?;
                write!(f, "(struct-init")?;
                if let Some(ty_name) = ty {
                    write!(f, " {}", ty_name.name)?;
                }
                writeln!(f)?;
                for field in fields {
                    field.write_sexpr(f, indent + 1)?;
                }
                write_indent(f, indent)?;
                writeln!(f, ")")?;
            }
            Expr::Closure { params, return_type, body, .. } => {
                write_indent(f, indent)?;
                write!(f, "(closure (params")?;
                for param in params {
                    write!(f, " ")?;
                    param.write_sexpr(f, 0)?;
                }
                write!(f, ")")?;
                if let Some(ret) = return_type {
                    write!(f, " (returns ")?;
                    ret.write_sexpr(f, 0)?;
                    write!(f, ")")?;
                }
                writeln!(f)?;
                body.write_sexpr(f, indent + 1)?;
                write_indent(f, indent)?;
                writeln!(f, ")")?;
            }
            Expr::Block(block) => {
                block.write_sexpr(f, indent)?;
            }
            Expr::Match { expr, arms, .. } => {
                write_indent(f, indent)?;
                writeln!(f, "(match")?;
                expr.write_sexpr(f, indent + 1)?;
                for arm in arms {
                    arm.write_sexpr(f, indent + 1)?;
                }
                write_indent(f, indent)?;
                writeln!(f, ")")?;
            }
            Expr::Comptime { expr, .. } => {
                write_indent(f, indent)?;
                writeln!(f, "(comptime")?;
                expr.write_sexpr(f, indent + 1)?;
                write_indent(f, indent)?;
                writeln!(f, ")")?;
            }
        }
        Ok(())
    }
}

impl ToSExpr for FieldInit {
    fn to_sexpr(&self) -> String {
        let mut s = String::new();
        self.write_sexpr(&mut s, 0).unwrap();
        s
    }

    fn write_sexpr(&self, f: &mut impl Write, indent: usize) -> fmt::Result {
        write_indent(f, indent)?;
        write!(f, "(field-init")?;
        if let Some(name) = &self.name {
            write!(f, " {}", name.name)?;
        }
        writeln!(f)?;
        self.value.write_sexpr(f, indent + 1)?;
        write_indent(f, indent)?;
        writeln!(f, ")")?;
        Ok(())
    }
}

impl ToSExpr for MatchArm {
    fn to_sexpr(&self) -> String {
        let mut s = String::new();
        self.write_sexpr(&mut s, 0).unwrap();
        s
    }

    fn write_sexpr(&self, f: &mut impl Write, indent: usize) -> fmt::Result {
        write_indent(f, indent)?;
        writeln!(f, "(arm")?;
        self.pattern.write_sexpr(f, indent + 1)?;
        self.body.write_sexpr(f, indent + 1)?;
        write_indent(f, indent)?;
        writeln!(f, ")")?;
        Ok(())
    }
}

impl ToSExpr for Pattern {
    fn to_sexpr(&self) -> String {
        let mut s = String::new();
        self.write_sexpr(&mut s, 0).unwrap();
        s
    }

    fn write_sexpr(&self, f: &mut impl Write, indent: usize) -> fmt::Result {
        write_indent(f, indent)?;
        match self {
            Pattern::Wildcard(_) => writeln!(f, "(pattern-wildcard)")?,
            Pattern::Literal(lit, _) => {
                write!(f, "(pattern-literal ")?;
                lit.write_sexpr(f, 0)?;
                writeln!(f, ")")?;
            }
            Pattern::Ident(ident) => writeln!(f, "(pattern-ident {})", ident.name)?,
            Pattern::Tuple(patterns, _) => {
                writeln!(f, "(pattern-tuple")?;
                for pattern in patterns {
                    pattern.write_sexpr(f, indent + 1)?;
                }
                write_indent(f, indent)?;
                writeln!(f, ")")?;
            }
            Pattern::Enum { name, fields, .. } => {
                writeln!(f, "(pattern-enum {}", name.name)?;
                for field in fields {
                    field.write_sexpr(f, indent + 1)?;
                }
                write_indent(f, indent)?;
                writeln!(f, ")")?;
            }
            Pattern::Expr(expr) => {
                writeln!(f, "(pattern-expr")?;
                expr.write_sexpr(f, indent + 1)?;
                write_indent(f, indent)?;
                writeln!(f, ")")?;
            }
        }
        Ok(())
    }
}

impl ToSExpr for Literal {
    fn to_sexpr(&self) -> String {
        let mut s = String::new();
        self.write_sexpr(&mut s, 0).unwrap();
        s
    }

    fn write_sexpr(&self, f: &mut impl Write, _indent: usize) -> fmt::Result {
        match self {
            Literal::Integer(n) => write!(f, "{}", n),
            Literal::Float(n) => write!(f, "{}", n),
            Literal::String(s) => write!(f, "\"{}\"", s.escape_default()),
            Literal::Rune(c) => write!(f, "'{}'", c.escape_default()),
            Literal::Bool(b) => write!(f, "{}", if *b { "True" } else { "False" }),
        }
    }
}

impl BinOp {
    fn to_sexpr(&self) -> &str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Mod => "%",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::And => "&&",
            BinOp::Or => "||",
            BinOp::BitAnd => "&",
            BinOp::BitOr => "|",
            BinOp::LShift => "<<",
            BinOp::RShift => ">>",
            BinOp::Concat => "++",
            BinOp::Assign => "=",
            BinOp::AddAssign => "+=",
            BinOp::SubAssign => "-=",
            BinOp::MulAssign => "*=",
            BinOp::DivAssign => "/=",
            BinOp::ModAssign => "%=",
            BinOp::ConcatAssign => "++=",
        }
    }
}

impl UnOp {
    fn to_sexpr(&self) -> &str {
        match self {
            UnOp::Neg => "-",
            UnOp::Not => "!",
            UnOp::BitNot => "~",
        }
    }
}

/// Print an AST as an S-Expression
pub fn print_ast(ast: &[TopLevel]) -> String {
    let mut s = String::new();
    s.push_str("(program\n");
    for item in ast {
        item.write_sexpr(&mut s, 1).unwrap();
    }
    s.push_str(")");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_literal_sexpr() {
        let lit = Literal::Integer(42);
        assert_eq!(lit.to_sexpr(), "42");

        let lit = Literal::String("hello".to_string());
        assert_eq!(lit.to_sexpr(), "\"hello\"");

        let lit = Literal::Bool(true);
        assert_eq!(lit.to_sexpr(), "True");
    }

    #[test]
    fn test_visibility_sexpr() {
        assert_eq!(Visibility::Public.to_sexpr(), "public");
        assert_eq!(Visibility::FilePrivate.to_sexpr(), "file-private");
        assert_eq!(Visibility::Internal.to_sexpr(), "internal");
    }
}
