use std::collections::HashMap;
use std::rc::Rc;
use crate::ast::*;
use crate::type_decl::*;

#[derive(Debug)]
pub struct TypeCheckContext {
    vars: HashMap<String, TypeDecl>,
    functions: HashMap<String, Rc<Function>>,
    super_context: Option<Box<TypeCheckContext>>,
}

#[derive(Debug)]
pub struct TypeCheckError {
    msg: String,
}

impl std::fmt::Display for TypeCheckError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.msg)
    }
}

impl TypeCheckError {
    fn new(msg: String) -> Self {
        Self { msg }
    }
}

impl TypeCheckContext {
    pub fn new() -> Self {
        Self {
            vars: HashMap::new(),
            functions: HashMap::new(),
            super_context: None,
        }
    }

    pub fn set_var(&mut self, name: &str, ty: TypeDecl) {
        self.vars.insert(name.to_string(), ty);
    }

    pub fn set_fn(&mut self, name: &str, f: Rc<Function>) {
        self.functions.insert(name.to_string(), f);
    }

    pub fn get_var(&self, name: &str) -> Option<TypeDecl> {
        let name = name.to_string();
        if let Some(val) = self.vars.get(&name) {
            Some(val.clone())
        } else if let Some(box super_context) = &self.super_context {
            super_context.get_var(&name)
        } else {
            None
        }
    }

    pub fn get_fn(&self, name: &str) -> Option<Rc<Function>> {
        let name = name.to_string();
        if let Some(val) = self.functions.get(&name) {
            Some(val.clone())
        } else if let Some(box super_context) = &self.super_context {
            super_context.get_fn(&name)
        } else {
            None
        }
    }
}

fn process_val_type(stmt_pool: &StmtPool, expr_pool: &ExprPool, ctx: &mut TypeCheckContext, name: &String, type_decl: &Option<TypeDecl>, expr: &Option<ExprRef>) -> Result<TypeDecl, TypeCheckError> {
    let expr_ty = match expr {
        Some(e) => {
            let ty = type_check_expr(stmt_pool, expr_pool, e, ctx)?;
            if ty == TypeDecl::Unit {
                return Err(TypeCheckError::new(format!("Type mismatch: expected <expression>, but got {:?}", ty)));
            }
            Some(ty)
        }
        None => None,
    };

    match (type_decl, expr_ty.as_ref()) {
        (Some(TypeDecl::Unknown), Some(ty)) => {
            ctx.set_var(name.as_str(), ty.clone());
        }
        (Some(decl), Some(ty)) => {
            if decl != ty {
                return Err(TypeCheckError::new(format!("Type mismatch: expected {:?}, but got {:?}", decl, ty)));
            }
            ctx.set_var(name.as_str(), ty.clone());
        }
        _ => (),
    }

    Ok(TypeDecl::Unit)
}

pub fn type_check_expr(stmt_pool: &StmtPool, expr_pool: &ExprPool, e: &ExprRef, ctx: &mut TypeCheckContext) -> Result<TypeDecl, TypeCheckError> {
    let is_block_empty = |blk: ExprRef| -> bool {
        match expr_pool.0.get(blk.to_index()).unwrap() {
            Expr::Block(expressions) => {
                expressions.is_empty()
            }
            _ => false,
        }
    };

    match expr_pool.0.get(e.to_index()).unwrap_or(&Expr::Null) {
        Expr::True | Expr::False => Ok(TypeDecl::Bool),
        Expr::IfElse(_cond, blk1, blk2) => {
            let blk1_empty = is_block_empty(*blk1);
            let blk2_empty = is_block_empty(*blk2);
            if blk1_empty || blk2_empty {
                return Ok(TypeDecl::Unit); // ignore to infer empty of blk
            }

            let blk1_ty = check_block(stmt_pool, expr_pool, blk1, ctx)?;
            let blk2_ty = check_block(stmt_pool, expr_pool, blk2, ctx)?;
            if blk1_ty != blk2_ty {
                Ok(TypeDecl::Unit)
            } else {
                Ok(blk1_ty)
            }
        }
        Expr::Binary(op, lhs, rhs) => {
            let lhs_ty = type_check_expr(stmt_pool, expr_pool, lhs, ctx)?;
            let rhs_ty = type_check_expr(stmt_pool, expr_pool, rhs, ctx)?;
            if lhs_ty != rhs_ty {
                return Err(TypeCheckError::new(format!("Type mismatch: lhs expected {:?}, but rhs got {:?}", lhs_ty, rhs_ty)));
            }
            match op {
                Operator::IAdd if lhs_ty == TypeDecl::String && rhs_ty == TypeDecl::String => {
                    Ok(TypeDecl::String)
                }
                Operator::IAdd | Operator::ISub | Operator::IDiv | Operator::IMul |
                    Operator::LE | Operator::LT | Operator::GE | Operator::GT => {
                    if lhs_ty == TypeDecl::UInt64 {
                        if rhs_ty != TypeDecl::UInt64 {
                            return Err(TypeCheckError::new(format!("Type mismatch: lhs expected UInt64, but rhs got {:?}", rhs_ty)));
                        }
                        Ok(TypeDecl::UInt64)
                    } else if lhs_ty == TypeDecl::Int64 {
                        if rhs_ty != TypeDecl::Int64 {
                            return Err(TypeCheckError::new(format!("Type mismatch: lhs expected Int64, but rhs got {:?}", rhs_ty)));
                        }
                        Ok(TypeDecl::Int64)
                    } else {
                        return Err(TypeCheckError::new(format!("Type mismatch: lhs expected Int64 or UInt64, but rhs got {:?}", rhs_ty)));
                    }
                }
                Operator::LogicalAnd | Operator::LogicalOr => {
                    if lhs_ty == TypeDecl::Bool && rhs_ty == TypeDecl::Bool {
                        Ok(TypeDecl::Bool)
                    } else {
                        Err(TypeCheckError::new(format!("Type mismatch(bool): lhs expected Bool, but rhs got {:?}", rhs_ty)))
                    }
                }
                _ => Err(TypeCheckError::new(format!("Type mismatch: expected {:?}, but got {:?}", op, lhs_ty))),
            }

        }

        Expr::Block(_expressions) => check_block(stmt_pool, expr_pool, e, ctx),
        Expr::Int64(_) => Ok(TypeDecl::Int64),
        Expr::UInt64(_) => Ok(TypeDecl::UInt64),
        Expr::String(_) => Ok(TypeDecl::String),

        Expr::Identifier(name) => {
            if let Some(val_type) = ctx.get_var(&name) {
                Ok(val_type.clone())
            } else if let Some(fun) = ctx.get_fn(name.as_str()) {
                Ok(fun.return_type.clone().unwrap_or(TypeDecl::Unknown))
            } else {
                return Err(TypeCheckError::new(format!("Identifier {:?} not found", name)));
            }
        }

        Expr::Null => Ok(TypeDecl::Any),
        Expr::ExprList(_) => Ok(TypeDecl::Unit),

        Expr::Call(fn_name, _) => {
            if let Some(fun) = ctx.get_fn(fn_name.as_str()) {
                Ok(fun.return_type.clone().unwrap_or(TypeDecl::Unknown))
            } else {
                return Err(TypeCheckError::new(format!("Function {:?} not found", fn_name)));
            }
        }

        Expr::Assign(lhs, rhs) => {
            let lhs_ty = type_check_expr(stmt_pool, expr_pool, lhs, ctx)?;
            let rhs_ty = type_check_expr(stmt_pool, expr_pool, rhs, ctx)?;
            if lhs_ty != rhs_ty {
                return Err(TypeCheckError::new(format!("Type mismatch: lhs expected {:?}, but rhs got {:?}", lhs_ty, rhs_ty)));
            }
            Ok(lhs_ty)
        }
    }
}

pub fn type_check_stmt(s: &StmtRef, stmt_pool: &StmtPool, expr_pool: &ExprPool, ctx: &mut TypeCheckContext) -> Result<TypeDecl, TypeCheckError> {
    let to_stmt = |e: &StmtRef| -> &Stmt { stmt_pool.get(e.to_index()).unwrap_or(&Stmt::Break) };

    Ok(match to_stmt(s) {
        Stmt::Expression(e) => {
            type_check_expr(stmt_pool, expr_pool, e, ctx)?
        }
        Stmt::Var(name, type_decl, expr) => {
            process_val_type(stmt_pool, expr_pool, ctx, name, type_decl, expr)?
        }
        Stmt::Val(name, type_decl, expr) => {
            let expr = Some(expr.clone());
            process_val_type(stmt_pool, expr_pool, ctx, name, type_decl, &expr)?
        }
        Stmt::Return(expr) => {
            if expr.is_none() {
                TypeDecl::Unit
            } else {
                let e = expr.unwrap();
                type_check_expr(stmt_pool, expr_pool, &e, ctx)?
            }
        }
        Stmt::For(_, _, _, _) => TypeDecl::Unit,
        Stmt::While(_, _) => TypeDecl::Unit,
        Stmt::Break => TypeDecl::Unit,
        Stmt::Continue => TypeDecl::Unit,
    })
}
pub fn check_block(stmt_pool: &StmtPool, expr_pool: &ExprPool, e: &ExprRef, ctx: &mut TypeCheckContext) -> Result<TypeDecl, TypeCheckError> {
    let to_expr = |e: &ExprRef| -> &Expr { expr_pool.get(e.to_index()).unwrap_or(&Expr::Null) };

    match to_expr(&e) {
        Expr::Block(statements) => {
            if statements.is_empty() {
                return Ok(TypeDecl::Unit);
            }
            let mut last_empty = true;
            let mut last: Option<TypeDecl> = None;
            // This code assumes Block(expression) don't make nested function
            // so `return` expression always return for this context.
            for s in statements {
                let stmt = stmt_pool.0.get(s.to_index()).unwrap();
                let def_ty: TypeDecl = match stmt {
                    Stmt::Return(None) => {
                        TypeDecl::Unit
                    }
                    Stmt::Return(ret_ty) => {
                        let e = ret_ty.unwrap();
                        let ty = type_check_expr(stmt_pool, expr_pool, &e, ctx)?;
                        if last_empty {
                            last_empty = false;
                            ty
                        } else {
                            match last {
                                Some(last_ty) if last_ty == ty => ty,
                                _ => Err(TypeCheckError::new(format!("Type mismatch(return): expected {:?}, but got {:?} : {:?}", last, to_expr(&ret_ty.unwrap()), stmt)))?,
                            }
                        }
                    }
                    _ => type_check_stmt(s, stmt_pool, expr_pool, ctx)?,
                };
                last = Some(def_ty);
            }
            if last.is_some() {
                Ok(last.unwrap().clone())
            } else {
                Err(TypeCheckError::new(format!("Type of block mismatch: expected {:?}", last)))
            }
        }
        _ => panic!("check_block: expected block but {:?}", expr_pool.0.get(e.to_index()).unwrap()),
    }
}

pub fn type_check(s: &StmtRef, stmt_pool: &StmtPool, expr_pool: &ExprPool, ctx: &mut TypeCheckContext) -> Result<TypeDecl, TypeCheckError> {
    let mut last = TypeDecl::Unit;

    match stmt_pool.get(s.to_index()).unwrap() {
        Stmt::Expression(e) => {
            match expr_pool.get(e.to_index()).unwrap() {
                Expr::Block(statements) => {
                    for stmt in statements {
                        let res = type_check_stmt(stmt, stmt_pool, expr_pool, ctx);
                        if res.is_err() {
                            return res;
                        } else {
                            last = res.unwrap();
                        }
                    }
                }
                _ => {
                    panic!("type_check: expected block but {:?}", expr_pool.0.get(s.to_index()).unwrap());
                }
            }
        }
        _ => panic!("type_check: expected block but {:?}", expr_pool.0.get(s.to_index()).unwrap()),
    }
    Ok(last)
}
