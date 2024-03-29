use frontend;
use frontend::ast::*;
use std::collections::HashMap;

pub struct Environment {
    context: HashMap<String, Type>,
}

impl Environment {
    pub fn new() -> Self {
        Environment {
            context: HashMap::new(),
        }
    }
}

/*
fn norm(t: &mut Type) -> &mut Type {
    match t {
        Type::Variable(box VarType {
            id: _,
            ty: Type::Unknown,
        }) => t,
        Type::Variable(_) => norm(t),
        ty => ty,
    }
}

fn unify(t1: &mut Type, t2: &mut Type) -> Result<(), String> {
    let t1 = norm(t1);
    let t2 = norm(t2);
    match (t1, t2) {
        (
            Type::Variable(box VarType {
                id: i1,
                ty: Type::Unknown,
            }),
            Type::Variable(box VarType {
                id: i2,
                ty: Type::Unknown,
            }),
        ) => {
            *i1 = *i2;
        }
        (Type::Variable(box VarType { id: _, ty: ty }), ty2) if *ty == Type::Unknown => {
            *ty = ty2.clone();
        }
        (ty1, Type::Variable(box tv2)) if tv2.ty == Type::Unknown => {
            tv2.ty = ty1.clone();
        }
        (Type::Int64, Type::Int64) => (),
        (Type::UInt64, Type::UInt64) => (),
        (Type::Bool, Type::Bool) => (),
        (lhs, rhs) => return Err(format!("{:?} {:?} unify failed", lhs, rhs)),
    }
    Ok(())
}

pub fn typing(expr: &mut Expr, env: &mut Environment) -> Result<Type, String> {
    match expr {
        Expr::Binary(box x) => {
            let mut t1 = typing(&mut x.lhs, env)?;
            let mut t2 = typing(&mut x.rhs, env)?;
            let mut ty_op = typing_op(x.op.clone());
            if ty_op == Type::Bool {
                if t1 != Type::Bool || t2 != Type::Bool {
                    return Err(format!("bool op but {:?} {:?}", t1, t2));
                } else {
                    return Ok(Type::Bool);
                }
            } else if ty_op == Type::Int64 {
                unify(&mut t1, &mut t2)?;

                // int64
                let int_res = unify(&mut ty_op, &mut t1); // int64

                // uint64
                let mut ty_uint = Type::UInt64;
                let uint_res = unify(&mut ty_uint, &mut t1); // int64

                // check
                if int_res.is_ok() || uint_res.is_ok() {
                    // OK
                } else {
                    int_res?;
                    uint_res?;
                }
            } else {
                unify(&mut t1, &mut t2)?;
                unify(&mut ty_op, &mut t1)?;
            }
            Ok(t1)
        }
        Expr::Int64(_) => Ok(Type::Int64),
        Expr::UInt64(_) => Ok(Type::UInt64),
        /*
        Expr::Val(_, _, _) => {},
        Expr::Identifier(_) => {},
        Expr::Null => {},
        Expr::Call(_, _) => {},
         */
        _ => Err(format!("err")),
    }
}

pub fn typing_op(op: Operator) -> Type {
    match op {
        Operator::Assign => Type::Unit,
        Operator::IAdd => Type::Int64,
        Operator::ISub => Type::Int64,
        Operator::IMul => Type::Int64,
        Operator::IDiv => Type::Int64,
        Operator::EQ => Type::Bool,
        Operator::NE => Type::Bool,
        Operator::LT => Type::Bool,
        Operator::LE => Type::Bool,
        Operator::GT => Type::Bool,
        Operator::GE => Type::Bool,
        Operator::LogicalAnd => Type::Bool,
        Operator::LogicalOr => Type::Bool,
    }
}

 */
