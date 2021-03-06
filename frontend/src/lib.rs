pub mod ast;
pub mod token;
use crate::ast::*;
use crate::token::Token;

mod lexer {
    include!(concat!(env!("OUT_DIR"), "/lexer.rs"));
}

pub struct Parser<'a> {
    lexer: lexer::Lexer<'a>,
    ahead: Vec<Token>,
    current_id: u64,
}

impl<'a> Parser<'a> {
    pub fn new(input: &'a str) -> Self {
        let lexer = lexer::Lexer::new(&input, 1u64);
        Parser {
            lexer,
            ahead: Vec::new(),
            current_id: 0,
        }
    }

    fn peek(&mut self) -> Option<&Token> {
        if self.ahead.is_empty() {
            match self.lexer.yylex() {
                Ok(t) => {
                    self.ahead.push(t);
                    self.ahead.get(0)
                }
                _ => return None,
            }
        } else {
            self.ahead.get(0)
        }
    }

    // pos: 0-origin
    fn peek_n(&mut self, pos: usize) -> Option<&Token> {
        while self.ahead.len() < pos + 1 {
            match self.lexer.yylex() {
                Ok(t) => self.ahead.push(t),
                _ => return None,
            }
        }
        return self.ahead.get(pos);
    }

    fn consume(&mut self, count: usize) -> usize {
        return self.ahead.drain(0..count).count();
    }

    fn next(&mut self) {
        self.ahead.remove(0);
    }

    pub fn expect(&mut self, accept: &Token) -> bool {
        let tk = self.peek();
        if *tk.unwrap() == *accept {
            self.next();
            true
        } else {
            false
        }
    }

    fn new_binary(op: Operator, lhs: Expr, rhs: Expr) -> Expr {
        Expr::Binary(Box::new(BinaryExpr { op, lhs, rhs }))
    }

    pub fn expect_err(&mut self, accept: &Token) -> Result<(), String> {
        if !self.expect(accept) {
            return Err(format!("{:?} expected but {:?}", accept, self.ahead.get(0)));
        }
        Ok(())
    }

    // expr := assign NewLine
    // assign := val_def | identifier "=" logical_expr | logical_expr
    // val_def := "val" identifier (":" def_ty)? ("=" logical_expr)
    // def_ty := Int64 | UInt64 | identifier
    // logical_expr := equality ("&&" relational | "||" relational)*
    // equality := relational ("==" relational | "!=" relational)*
    // relational := add ("<" add | "<=" add | ">" add | ">=" add")*
    // add := mul ("+" mul | "-" mul)*
    // mul := primary ("*" mul | "/" mul)*
    // primary := "(" expr ")" | identifier "(" expr_list ")" |
    //            identifier |
    //            UInt64 | Int64 | Integer | Null
    // expr_list = "" | expr | expr "," expr_list
    pub fn parse_expr_line(&mut self) -> Result<Expr, String> {
        let lhs = self.parse_expr();
        if lhs.is_err() {
            return lhs;
        }
        match self.peek() {
            Some(Token::NewLine) => self.next(),
            None => (),
            x => {
                return Err(format!(
                    "parse_expr: expected NewLine or EOF(None) but {:?}",
                    x
                ))
            }
        }
        return lhs;
    }

    pub fn parse_expr(&mut self) -> Result<Expr, String> {
        return self.parse_assign();
    }

    pub fn parse_assign(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Some(Token::Val) => {
                self.next();
                return self.parse_val_def();
            }
            _ => {
                let lhs = self.parse_logical_expr()?;
                match self.peek() {
                    Some(Token::Equal) => {
                        self.next();
                        return Ok(Self::new_binary(
                            Operator::Assign,
                            lhs,
                            self.parse_logical_expr()?,
                        ));
                    }
                    _ => return Ok(lhs),
                }
            }
        }
    }

    pub fn parse_val_def(&mut self) -> Result<Expr, String> {
        let mut ident: String = match self.peek() {
            Some(Token::Identifier(s)) => {
                let s = s.to_string();
                self.next();
                s
            }
            x => return Err(format!("parse_val_def: expected identifier but {:?}", x)),
        };
        let mut def_ty: TVar = match self.peek() {
            Some(Token::Colon) => {
                self.next();
                self.parse_def_ty()?
            }
            _ => TVar {
                s: String::new(),
                ty: Type::Unknown,
            },
        };

        let rhs = match self.peek() {
            Some(Token::Equal) => {
                self.next();
                Some(Box::new(self.parse_logical_expr()?))
            }
            _ => None,
        };
        return Ok(Expr::Val(ident, def_ty, rhs));
    }

    pub fn parse_def_ty(&mut self) -> Result<TVar, String> {
        let mut ident = String::new();
        let ty = match self.peek() {
            Some(Token::U64) => Type::UInt64,
            Some(Token::I64) => Type::Int64,
            Some(Token::Identifier(s)) => {
                ident = s.to_string();
                Type::Variable(Box::new(self.fresh_ty()))
            }
            x => return Err(format!("parse_def_ty: expected type but {:?}", x)),
        };
        self.next();
        return Ok(TVar { s: ident, ty });
    }

    fn parse_logical_expr(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_equality()?;

        loop {
            match self.peek() {
                Some(Token::DoubleAnd) => {
                    self.next();
                    let rhs = self.parse_relational()?;
                    lhs = Self::new_binary(Operator::LogicalAnd, lhs, rhs);
                }
                Some(Token::DoubleOr) => {
                    self.next();
                    let rhs = self.parse_relational()?;
                    lhs = Self::new_binary(Operator::LogicalOr, lhs, rhs);
                }
                _ => return Ok(lhs),
            }
        }
    }

    fn parse_equality(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_relational()?;

        loop {
            match self.peek() {
                Some(Token::DoubleEqual) => {
                    self.next();
                    let rhs = self.parse_relational()?;
                    lhs = Self::new_binary(Operator::EQ, lhs, rhs);
                }
                Some(Token::NotEqual) => {
                    self.next();
                    let rhs = self.parse_relational()?;
                    lhs = Self::new_binary(Operator::NE, lhs, rhs);
                }
                _ => return Ok(lhs),
            }
        }
    }

    fn parse_relational(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_add()?;

        loop {
            match self.peek() {
                Some(Token::LT) => {
                    self.next();
                    lhs = Self::new_binary(Operator::LT, lhs, self.parse_add()?)
                }
                Some(Token::LE) => {
                    self.next();
                    lhs = Self::new_binary(Operator::LE, lhs, self.parse_add()?)
                }
                Some(Token::GT) => {
                    self.next();
                    lhs = Self::new_binary(Operator::GT, lhs, self.parse_add()?)
                }
                Some(Token::GE) => {
                    self.next();
                    lhs = Self::new_binary(Operator::GE, lhs, self.parse_add()?)
                }
                _ => return Ok(lhs),
            }
        }
    }

    fn parse_add(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_mul()?;

        loop {
            match self.peek() {
                Some(Token::IAdd) => {
                    self.next();
                    let rhs = self.parse_mul()?;
                    lhs = Self::new_binary(Operator::IAdd, lhs, rhs);
                }
                Some(Token::ISub) => {
                    self.next();
                    let rhs = self.parse_mul()?;
                    lhs = Self::new_binary(Operator::ISub, lhs, rhs);
                }
                _ => return Ok(lhs),
            }
        }
    }

    fn parse_mul(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_primary()?;

        loop {
            match self.peek() {
                Some(Token::IMul) => {
                    self.next();
                    let rhs = self.parse_mul()?;
                    lhs = Self::new_binary(Operator::IMul, lhs, rhs);
                }
                Some(Token::IDiv) => {
                    self.next();
                    let rhs = self.parse_mul()?;
                    lhs = Self::new_binary(Operator::IDiv, lhs, rhs);
                }
                _ => return Ok(lhs),
            }
        }
    }

    fn fresh_ty(&mut self) -> VarType {
        self.current_id += 1;
        return VarType {
            id: self.current_id,
            ty: Type::Unknown,
        };
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Some(Token::ParenOpen) => {
                self.next();
                let node = self.parse_expr()?;
                self.expect_err(&Token::ParenClose)?;
                return Ok(node);
            }
            Some(Token::Identifier(s)) => {
                let s = s.to_string();
                self.next();
                return match self.peek() {
                    Some(Token::ParenOpen) => {
                        // function call
                        self.next();
                        let ty = Type::Variable(Box::new(self.fresh_ty()));
                        let args = self.parse_expr_list(vec![])?;
                        self.expect_err(&Token::ParenClose)?;
                        Ok(Expr::Call(TVar { s, ty }, args))
                    }
                    _ => {
                        // identifier
                        let ty = Type::Variable(Box::new(self.fresh_ty()));
                        Ok(Expr::Identifier(TVar { s, ty }))
                    }
                };
            }
            _ => {
                let e = match self.peek() {
                    Some(&Token::UInt64(num)) => Ok(Expr::UInt64(num)),
                    Some(&Token::Int64(num)) => Ok(Expr::Int64(num)),
                    Some(Token::Integer(num)) => {
                        Ok(Expr::Int64(0)) // FIXME
                    }
                    Some(&Token::Null) => Ok(Expr::Null),
                    x => return Err(format!("parse_primary: unexpected token {:?}", x)),
                };
                self.next();
                return e;
            }
        }
    }

    fn parse_expr_list(&mut self, mut args: Vec<Expr>) -> Result<Vec<Expr>, String> {
        match self.peek() {
            Some(Token::ParenClose) => return Ok(args),
            _ => (),
        }

        let expr = self.parse_expr();
        if expr.is_err() {
            return Err(format!(
                "parse_expr_list: expected expr: {}",
                expr.unwrap_err()
            ));
        }
        args.push(expr.unwrap());

        return match self.peek() {
            Some(Token::Comma) => {
                self.next();
                self.parse_expr_list(args)
            }
            Some(Token::ParenClose) => Ok(args),
            x => Err(format!("parse_expr_list: unexpected token {:?}", x)),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::Token;

    #[test]
    fn lexer_simple_keyword() {
        let s = " if else while for class fn val var";
        let mut l = lexer::Lexer::new(&s, 1u64);
        assert_eq!(l.yylex().unwrap(), Token::If);
        assert_eq!(l.yylex().unwrap(), Token::Else);
        assert_eq!(l.yylex().unwrap(), Token::While);
        assert_eq!(l.yylex().unwrap(), Token::For);
        assert_eq!(l.yylex().unwrap(), Token::Class);
        assert_eq!(l.yylex().unwrap(), Token::Function);
        assert_eq!(l.yylex().unwrap(), Token::Val);
        assert_eq!(l.yylex().unwrap(), Token::Var);
    }

    #[test]
    fn lexer_simple_integer() {
        let s = " -1i64 1i64 2u64 123 -456";
        let mut l = lexer::Lexer::new(&s, 1u64);
        assert_eq!(l.yylex().unwrap(), Token::Int64(-1));
        assert_eq!(l.yylex().unwrap(), Token::Int64(1));
        assert_eq!(l.yylex().unwrap(), Token::UInt64(2u64));
        assert_eq!(l.yylex().unwrap(), Token::Integer("123".to_string()));
        assert_eq!(l.yylex().unwrap(), Token::Integer("-456".to_string()));
    }

    #[test]
    fn lexer_simple_symbol1() {
        let s = " ( ) { } [ ] , . :: : =";
        let mut l = lexer::Lexer::new(&s, 1u64);
        assert_eq!(l.yylex().unwrap(), Token::ParenOpen);
        assert_eq!(l.yylex().unwrap(), Token::ParenClose);
        assert_eq!(l.yylex().unwrap(), Token::BraceOpen);
        assert_eq!(l.yylex().unwrap(), Token::BraceClose);
        assert_eq!(l.yylex().unwrap(), Token::BracketOpen);
        assert_eq!(l.yylex().unwrap(), Token::BracketClose);
        assert_eq!(l.yylex().unwrap(), Token::Comma);
        assert_eq!(l.yylex().unwrap(), Token::Dot);
        assert_eq!(l.yylex().unwrap(), Token::DoubleColon);
        assert_eq!(l.yylex().unwrap(), Token::Colon);
        assert_eq!(l.yylex().unwrap(), Token::Equal);
    }

    #[test]
    fn lexer_simple_symbol2() {
        let s = "== != <= < >= >";
        let mut l = lexer::Lexer::new(&s, 1u64);
        assert_eq!(l.yylex().unwrap(), Token::DoubleEqual);
        assert_eq!(l.yylex().unwrap(), Token::NotEqual);
        assert_eq!(l.yylex().unwrap(), Token::LE);
        assert_eq!(l.yylex().unwrap(), Token::LT);
        assert_eq!(l.yylex().unwrap(), Token::GE);
        assert_eq!(l.yylex().unwrap(), Token::GT);
    }

    #[test]
    fn lexer_arithmetic_operator_symbol() {
        let s = " + - * / +. -. *. /.";
        let mut l = lexer::Lexer::new(&s, 1u64);
        assert_eq!(l.yylex().unwrap(), Token::IAdd);
        assert_eq!(l.yylex().unwrap(), Token::ISub);
        assert_eq!(l.yylex().unwrap(), Token::IMul);
        assert_eq!(l.yylex().unwrap(), Token::IDiv);
        assert_eq!(l.yylex().unwrap(), Token::FAdd);
        assert_eq!(l.yylex().unwrap(), Token::FSub);
        assert_eq!(l.yylex().unwrap(), Token::FMul);
        assert_eq!(l.yylex().unwrap(), Token::FDiv);
    }

    #[test]
    fn lexer_simple_identifier() {
        let s = " A _name Identifier ";
        let mut l = lexer::Lexer::new(&s, 1u64);
        assert_eq!(l.yylex().unwrap(), Token::Identifier("A".to_string()));
        assert_eq!(l.yylex().unwrap(), Token::Identifier("_name".to_string()));
        assert_eq!(
            l.yylex().unwrap(),
            Token::Identifier("Identifier".to_string())
        );
    }

    #[test]
    fn lexer_multiple_lines() {
        let s = " A \n B ";
        let mut l = lexer::Lexer::new(&s, 1u64);
        assert_eq!(l.yylex().unwrap(), Token::Identifier("A".to_string()));
        assert_eq!(l.yylex().unwrap(), Token::NewLine);
        assert_eq!(l.yylex().unwrap(), Token::Identifier("B".to_string()));
        assert_eq!(*l.get_line_count(), 2);
    }

    #[test]
    fn parser_util_lookahead() {
        let mut p = Parser::new("1u64 + 2u64");
        let t1 = p.peek_n(1).unwrap();
        assert_eq!(Token::IAdd, *t1);
        assert_eq!(2, p.consume(2));

        let t2 = p.peek().unwrap();
        assert_eq!(Token::UInt64(2), *t2);
    }

    #[test]
    fn parser_simple_expr() {
        let mut p = Parser::new("1u64 + 2u64 ");
        let res = p.parse_expr_line().unwrap();
        assert_eq!(
            Expr::Binary(Box::new(BinaryExpr {
                op: Operator::IAdd,
                lhs: Expr::UInt64(1),
                rhs: Expr::UInt64(2),
            })),
            res
        );
    }

    #[test]
    fn parser_simple_expr_mul() {
        let mut p = Parser::new("(1u64) + 2u64 * 3u64");
        let res = p.parse_expr_line().unwrap();
        assert_eq!(
            Expr::Binary(Box::new(BinaryExpr {
                op: Operator::IAdd,
                lhs: Expr::UInt64(1),
                rhs: Expr::Binary(Box::new(BinaryExpr {
                    op: Operator::IMul,
                    lhs: Expr::UInt64(2),
                    rhs: Expr::UInt64(3),
                })),
            })),
            res
        );
    }

    #[test]
    fn parser_simple_relational_expr() {
        let mut p = Parser::new("0u64 < 2u64 + 4u64");
        let res = p.parse_expr_line().unwrap();
        assert_eq!(
            Expr::Binary(Box::new(BinaryExpr {
                op: Operator::LT,
                lhs: Expr::UInt64(0),
                rhs: Expr::Binary(Box::new(BinaryExpr {
                    op: Operator::IAdd,
                    lhs: Expr::UInt64(2),
                    rhs: Expr::UInt64(4),
                })),
            })),
            res
        );
    }

    #[test]
    fn parser_simple_logical_expr() {
        let mut p = Parser::new("1u64 && 2u64 < 3u64");
        let res = p.parse_expr_line().unwrap();
        assert_eq!(
            Expr::Binary(Box::new(BinaryExpr {
                op: Operator::LogicalAnd,
                lhs: Expr::UInt64(1),
                rhs: Expr::Binary(Box::new(BinaryExpr {
                    op: Operator::LT,
                    lhs: Expr::UInt64(2),
                    rhs: Expr::UInt64(3),
                })),
            })),
            res
        );
    }

    #[test]
    fn parser_expr_accept() {
        assert!(Parser::new("1u64").parse_expr_line().is_ok());
        assert!(Parser::new("(1u64 + 2u64)").parse_expr_line().is_ok());
        assert!(Parser::new("1u64 && 2u64 < 3u64").parse_expr_line().is_ok());
        assert!(Parser::new("1u64 || 2u64 < 3u64").parse_expr_line().is_ok());
        assert!(Parser::new("1u64 || (2u64) < 3u64 + 4u64")
            .parse_expr_line()
            .is_ok());

        assert!(Parser::new("variable").parse_expr_line().is_ok());
        assert!(Parser::new("a + b").parse_expr_line().is_ok());
        assert!(Parser::new("a + 1u64").parse_expr_line().is_ok());

        assert!(Parser::new("a() + 1u64").parse_expr_line().is_ok());
        assert!(Parser::new("a(b,c) + 1u64").parse_expr_line().is_ok());
    }

    #[test]
    fn parser_simple_ident_expr() {
        let res = Parser::new("abc + 1u64").parse_expr_line().unwrap();
        assert_eq!(
            Expr::Binary(Box::new(BinaryExpr {
                op: Operator::IAdd,
                lhs: Expr::Identifier(TVar {
                    s: "abc".to_string(),
                    ty: Type::Variable(Box::new(VarType {
                        id: 1,
                        ty: Type::Unknown
                    })),
                }),
                rhs: Expr::UInt64(1),
            }),),
            res
        );
    }

    #[test]
    fn parser_simple_apply_empty() {
        let res = Parser::new("abc()").parse_expr_line().unwrap();
        assert_eq!(
            Expr::Call {
                0: TVar {
                    s: "abc".to_string(),
                    ty: Type::Variable(Box::new(VarType {
                        id: 1,
                        ty: Type::Unknown
                    }))
                },
                1: vec![],
            },
            res
        );
    }

    #[test]
    fn parser_simple_apply_expr() {
        let res = Parser::new("abc(1u64,2u64)").parse_expr_line().unwrap();
        assert_eq!(
            Expr::Call {
                0: TVar {
                    s: "abc".to_string(),
                    ty: Type::Variable(Box::new(VarType {
                        id: 1,
                        ty: Type::Unknown
                    }))
                },
                1: vec![Expr::UInt64(1), Expr::UInt64(2),],
            },
            res
        );
    }

    #[test]
    fn parser_simple_expr_null_value() {
        let res = Parser::new("null").parse_expr_line().unwrap();
        assert_eq!(Expr::Null, res);
    }

    #[test]
    fn parser_simple_assign() {
        let res = Parser::new("a = 1u64").parse_expr_line().unwrap();
        assert_eq!(
            Expr::Binary(Box::new(BinaryExpr {
                op: Operator::Assign,
                lhs: Expr::Identifier(TVar {
                    s: "a".to_string(),
                    ty: Type::Variable(Box::new(VarType {
                        id: 1,
                        ty: Type::Unknown
                    }))
                }),
                rhs: Expr::UInt64(1)
            })),
            res
        );
    }

    #[test]
    fn parser_err_primary() {
        let res = Parser::new(".").parse_expr_line();
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("parse_primary"));
    }

    #[test]
    fn parser_err_call_expr_list() {
        let res = Parser::new("hoge(a,,)").parse_expr_line();
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("parse_expr_list"));
    }

    #[test]
    fn parser_val_simple_expr() {
        let res = Parser::new("val hoge = 10u64").parse_expr_line().unwrap();
        assert_eq!(
            Expr::Val(
                "hoge".to_string(),
                TVar {
                    s: "".to_string(),
                    ty: Type::Unknown
                },
                Some(Box::new(Expr::UInt64(10)))
            ),
            res
        );
    }

    #[test]
    fn parser_val_simple_expr_with_type() {
        let res = Parser::new("val hoge: u64 = 30u64")
            .parse_expr_line()
            .unwrap();
        assert_eq!(
            Expr::Val(
                "hoge".to_string(),
                TVar {
                    s: "".to_string(),
                    ty: Type::UInt64
                },
                Some(Box::new(Expr::UInt64(30)))
            ),
            res
        );
    }
    #[test]
    fn parser_val_simple_expr_without_type1() {
        let res = Parser::new("val fuga = 20u64").parse_expr_line().unwrap();
        assert_eq!(
            Expr::Val(
                "fuga".to_string(),
                TVar {
                    s: "".to_string(),
                    ty: Type::Unknown
                },
                Some(Box::new(Expr::UInt64(20)))
            ),
            res
        );
    }

    #[test]
    fn parser_val_simple_expr_without_type2() {
        let res = Parser::new("val fuga: ty = 20u64")
            .parse_expr_line()
            .unwrap();
        assert_eq!(
            Expr::Val(
                "fuga".to_string(),
                TVar {
                    s: "ty".to_string(),
                    ty: Type::Variable(Box::new(VarType {
                        id: 1,
                        ty: Type::Unknown
                    }))
                },
                Some(Box::new(Expr::UInt64(20)))
            ),
            res
        );
    }
}
