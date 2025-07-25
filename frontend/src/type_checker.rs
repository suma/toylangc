use std::collections::HashMap;
use std::rc::Rc;
use string_interner::{DefaultStringInterner, DefaultSymbol};
use crate::ast::*;
use crate::type_decl::*;
use crate::visitor::AstVisitor;

// Import new modular structure
pub mod core;
pub mod context;
pub mod error;
pub mod function;
pub mod inference;
pub mod optimization;

pub use core::CoreReferences;
pub use context::{TypeCheckContext, VarState};
pub use error::{SourceLocation, TypeCheckError, TypeCheckErrorKind};
pub use function::FunctionCheckingState;
pub use inference::TypeInferenceState;
pub use optimization::PerformanceOptimization;

// Struct definitions moved to separate modules

pub struct TypeCheckerVisitor <'a, 'b, 'c, 'd> {
    pub core: CoreReferences<'a, 'b, 'c, 'd>,
    pub context: TypeCheckContext,
    pub type_inference: TypeInferenceState,
    pub function_checking: FunctionCheckingState,
    pub optimization: PerformanceOptimization,
}




impl<'a, 'b, 'c, 'd> TypeCheckerVisitor<'a, 'b, 'c, 'd> {
    pub fn new(stmt_pool: &'a StmtPool, expr_pool: &'b mut ExprPool, string_interner: &'c DefaultStringInterner, location_pool: &'d LocationPool) -> Self {
        Self {
            core: CoreReferences {
                stmt_pool,
                expr_pool,
                string_interner,
                location_pool,
            },
            context: TypeCheckContext::new(),
            type_inference: TypeInferenceState::new(),
            function_checking: FunctionCheckingState::new(),
            optimization: PerformanceOptimization::new(),
        }
    }
    
    fn get_expr_location(&self, expr_ref: &ExprRef) -> Option<SourceLocation> {
        self.core.location_pool.get_expr_location(expr_ref).cloned()
    }
    
    fn get_stmt_location(&self, stmt_ref: &StmtRef) -> Option<SourceLocation> {
        self.core.location_pool.get_stmt_location(stmt_ref).cloned()
    }
    
    // Helper methods for location tracking (can be used for future error reporting enhancements)

    pub fn push_context(&mut self) {
        self.context.vars.push(HashMap::new());
    }

    pub fn pop_context(&mut self) {
        self.context.vars.pop();
    }

    pub fn add_function(&mut self, f: Rc<Function>) {
        self.context.set_fn(f.name, f.clone());
    }

    fn process_val_type(&mut self, name: DefaultSymbol, type_decl: &Option<TypeDecl>, expr: &Option<ExprRef>) -> Result<TypeDecl, TypeCheckError> {
        let expr_ty = match expr {
            Some(e) => {
                let ty = self.visit_expr(e)?;
                if ty == TypeDecl::Unit {
                    return Err(TypeCheckError::type_mismatch(TypeDecl::Unknown, ty));
                }
                Some(ty)
            }
            None => None,
        };

        match (type_decl, expr_ty.as_ref()) {
            (Some(TypeDecl::Unknown), Some(ty)) => {
                self.context.set_var(name, ty.clone());
            }
            (Some(decl), Some(ty)) => {
                if decl != ty {
                    return Err(TypeCheckError::type_mismatch(decl.clone(), ty.clone()));
                }
                self.context.set_var(name, ty.clone());
            }
            (None, Some(ty)) => {
                // No explicit type declaration - store the inferred type
                self.context.set_var(name, ty.clone());
            }
            _ => (),
        }

        Ok(TypeDecl::Unit)
    }

    pub fn type_check(&mut self, func: Rc<Function>) -> Result<TypeDecl, TypeCheckError> {
        let mut last = TypeDecl::Unit;
        let s = func.code.clone();

        // Is already checked
        match self.function_checking.is_checked_fn.get(&func.name) {
            Some(Some(result_ty)) => return Ok(result_ty.clone()),  // already checked
            Some(None) => return Ok(TypeDecl::Unknown), // now checking
            None => (),
        }

        // Now checking...
        self.function_checking.is_checked_fn.insert(func.name, None);

        // Clear type cache at the start of each function to limit cache scope
        self.optimization.type_cache.clear();

        self.function_checking.call_depth += 1;

        let statements = match self.core.stmt_pool.get(s.to_index()).ok_or_else(|| TypeCheckError::generic_error("Invalid statement reference"))? {
            Stmt::Expression(e) => {
                match self.core.expr_pool.0.get(e.to_index()).ok_or_else(|| TypeCheckError::generic_error("Invalid expression reference"))? {
                    Expr::Block(statements) => {
                        statements.clone()  // Clone required: statements is used in multiple loops and we need mutable access to self
                    }
                    _ => {
                        return Err(TypeCheckError::generic_error("type_check: expected block expression"));
                    }
                }
            }
            _ => return Err(TypeCheckError::generic_error("type_check: expected block statement")),
        };

        self.push_context();
        // Define variable of argument for this `func`
        func.parameter.iter().for_each(|(name, type_decl)| {
            self.context.set_var(*name, type_decl.clone());
        });

        // Pre-scan for explicit type declarations and establish global type context
        let mut global_numeric_type: Option<TypeDecl> = None;
        for s in statements.iter() {
            if let Some(stmt) = self.core.stmt_pool.get(s.to_index()) {
                match stmt {
                    Stmt::Val(_, Some(type_decl), _) | Stmt::Var(_, Some(type_decl), _) => {
                        if matches!(type_decl, TypeDecl::Int64 | TypeDecl::UInt64) {
                            global_numeric_type = Some(type_decl.clone());
                            break; // Use the first explicit numeric type found
                        }
                    }
                    _ => {}
                }
            }
        }
        
        // Set global type hint if found
        let original_hint = self.type_inference.type_hint.clone();
        if let Some(ref global_type) = global_numeric_type {
            self.type_inference.type_hint = Some(global_type.clone());
        }

        for stmt in statements.iter() {
            let stmt_obj = self.core.stmt_pool.get(stmt.to_index()).ok_or_else(|| TypeCheckError::generic_error("Invalid statement reference"))?;
            let res = stmt_obj.clone().accept(self);
            if res.is_err() {
                return res;
            } else {
                last = res?;
            }
        }
        self.pop_context();
        self.function_checking.call_depth -= 1;

        // Restore original type hint
        self.type_inference.type_hint = original_hint;

        // Final pass: convert any remaining Number literals to default type (UInt64)
        self.finalize_number_types()?;
        
        // Check if the function body type matches the declared return type
        if let Some(ref expected_return_type) = func.return_type {
            if &last != expected_return_type {
                return Err(TypeCheckError::type_mismatch(
                    expected_return_type.clone(),
                    last.clone()
                ));
            }
        }
        
        self.function_checking.is_checked_fn.insert(func.name, Some(last.clone()));
        Ok(last)
    }
}
pub trait Acceptable {
    fn accept(&mut self, visitor: &mut dyn AstVisitor) -> Result<TypeDecl, TypeCheckError>;
}

impl Acceptable for Expr {
    fn accept(&mut self, visitor: &mut dyn AstVisitor) -> Result<TypeDecl, TypeCheckError> {
        match self {
            Expr::Binary(op, lhs, rhs) => visitor.visit_binary(op, lhs, rhs),
            Expr::Block(statements) => visitor.visit_block(statements),
            Expr::IfElifElse(cond, then_block, elif_pairs, else_block) => visitor.visit_if_elif_else(cond, then_block, elif_pairs, else_block),
            Expr::Assign(lhs, rhs) => visitor.visit_assign(lhs, rhs),
            Expr::Identifier(name) => visitor.visit_identifier(*name),
            Expr::Call(fn_name, args) => visitor.visit_call(*fn_name, args),
            Expr::Int64(val) => visitor.visit_int64_literal(val),
            Expr::UInt64(val) => visitor.visit_uint64_literal(val),
            Expr::Number(val) => visitor.visit_number_literal(*val),
            Expr::String(val) => visitor.visit_string_literal(*val),
            Expr::True | Expr::False => visitor.visit_boolean_literal(self),
            Expr::Null => visitor.visit_null_literal(),
            Expr::ExprList(items) => visitor.visit_expr_list(items),
            Expr::ArrayLiteral(elements) => visitor.visit_array_literal(elements),
            Expr::ArrayAccess(array, index) => visitor.visit_array_access(array, index),
            Expr::FieldAccess(obj, field) => visitor.visit_field_access(obj, field),
            Expr::MethodCall(obj, method, args) => visitor.visit_method_call(obj, method, args),
            Expr::StructLiteral(struct_name, fields) => visitor.visit_struct_literal(struct_name, fields),
        }
    }
}

impl Acceptable for Stmt {
    fn accept(&mut self, visitor: &mut dyn AstVisitor) -> Result<TypeDecl, TypeCheckError> {
        match self {
            Stmt::Expression(expr) => visitor.visit_expression_stmt(expr),
            Stmt::Var(name, type_decl, expr) => visitor.visit_var(*name, type_decl, expr),
            Stmt::Val(name, type_decl, expr) => visitor.visit_val(*name, type_decl, expr),
            Stmt::Return(expr) => visitor.visit_return(expr),
            Stmt::For(init, cond, step, body) => visitor.visit_for(*init, cond, step, body),
            Stmt::While(cond, body) => visitor.visit_while(cond, body),
            Stmt::Break => visitor.visit_break(),
            Stmt::Continue => visitor.visit_continue(),
            Stmt::StructDecl { name, fields } => visitor.visit_struct_decl(name, fields),
            Stmt::ImplBlock { target_type, methods } => visitor.visit_impl_block(target_type, methods),
        }
    }
}

impl<'a, 'b, 'c, 'd> AstVisitor for TypeCheckerVisitor<'a, 'b, 'c, 'd> {
    fn visit_expr(&mut self, expr: &ExprRef) -> Result<TypeDecl, TypeCheckError> {
        // Check cache first
        if let Some(cached_type) = self.get_cached_type(expr) {
            return Ok(cached_type.clone());
        }
        
        // Set up context hint for nested expressions
        let original_hint = self.type_inference.type_hint.clone();
        let expr_obj = self.core.expr_pool.get(expr.to_index()).ok_or_else(|| TypeCheckError::generic_error("Invalid expression reference"))?;
        let result = expr_obj.clone().accept(self);
        
        // If an error occurred, try to add location information if not already present
        let result = match result {
            Err(mut error) if error.location.is_none() => {
                error.location = self.get_expr_location(expr);
                Err(error)
            }
            other => other,
        };
        
        // Cache the result if successful
        if let Ok(ref result_type) = result {
            self.cache_type(expr.clone(), result_type.clone());
            
            // Context propagation: if this expression resolved to a concrete numeric type,
            // and we don't have a current hint, set it for sibling expressions
            if original_hint.is_none() && (result_type == &TypeDecl::Int64 || result_type == &TypeDecl::UInt64) {
                if self.type_inference.type_hint.is_none() {
                    self.type_inference.type_hint = Some(result_type.clone());
                }
            }
        }
        
        result
    }

    fn visit_stmt(&mut self, stmt: &StmtRef) -> Result<TypeDecl, TypeCheckError> {
        let result = self.core.stmt_pool.get(stmt.to_index()).unwrap_or(&Stmt::Break).clone().accept(self);
        
        // If an error occurred, try to add location information if not already present
        match result {
            Err(mut error) if error.location.is_none() => {
                error.location = self.get_stmt_location(stmt);
                Err(error)
            }
            other => other,
        }
    }

    fn visit_binary(&mut self, op: &Operator, lhs: &ExprRef, rhs: &ExprRef) -> Result<TypeDecl, TypeCheckError> {
        let op = op.clone();
        let lhs = lhs.clone();
        let rhs = rhs.clone();
        let lhs_ty = {
            let lhs_obj = self.core.expr_pool.get(lhs.to_index()).ok_or_else(|| TypeCheckError::generic_error("Invalid left-hand expression reference"))?;
            lhs_obj.clone().accept(self)?
        };
        let rhs_ty = {
            let rhs_obj = self.core.expr_pool.get(rhs.to_index()).ok_or_else(|| TypeCheckError::generic_error("Invalid right-hand expression reference"))?;
            rhs_obj.clone().accept(self)?
        };
        
        // Resolve types with automatic conversion for Number type
        let (resolved_lhs_ty, resolved_rhs_ty) = self.resolve_numeric_types(&lhs_ty, &rhs_ty)?;
        
        // Context propagation: if we have a type hint, propagate it to Number expressions
        if let Some(hint) = self.type_inference.type_hint.clone() {
            if lhs_ty == TypeDecl::Number && (hint == TypeDecl::Int64 || hint == TypeDecl::UInt64) {
                self.propagate_type_to_number_expr(&lhs, &hint)?;
            }
            if rhs_ty == TypeDecl::Number && (hint == TypeDecl::Int64 || hint == TypeDecl::UInt64) {
                self.propagate_type_to_number_expr(&rhs, &hint)?;
            }
        }
        
        // Record Number usage context for later finalization
        self.record_number_usage_context(&lhs, &lhs_ty, &resolved_lhs_ty)?;
        self.record_number_usage_context(&rhs, &rhs_ty, &resolved_rhs_ty)?;
        
        // Immediate propagation: if one side has concrete type, propagate to Number variables
        if resolved_lhs_ty != TypeDecl::Number && rhs_ty == TypeDecl::Number {
            self.propagate_to_number_variable(&rhs, &resolved_lhs_ty)?;
        }
        if resolved_rhs_ty != TypeDecl::Number && lhs_ty == TypeDecl::Number {
            self.propagate_to_number_variable(&lhs, &resolved_rhs_ty)?;
        }
        
        // Transform AST nodes if type conversion occurred
        if lhs_ty == TypeDecl::Number && resolved_lhs_ty != TypeDecl::Number {
            self.transform_numeric_expr(&lhs, &resolved_lhs_ty)?;
        }
        if rhs_ty == TypeDecl::Number && resolved_rhs_ty != TypeDecl::Number {
            self.transform_numeric_expr(&rhs, &resolved_rhs_ty)?;
        }
        
        // Update variable types if identifiers were involved in type conversion
        self.update_identifier_types(&lhs, &lhs_ty, &resolved_lhs_ty)?;
        self.update_identifier_types(&rhs, &rhs_ty, &resolved_rhs_ty)?;
        
        let result_type = match op {
            Operator::IAdd if resolved_lhs_ty == TypeDecl::String && resolved_rhs_ty == TypeDecl::String => {
                TypeDecl::String
            }
            Operator::IAdd | Operator::ISub | Operator::IDiv | Operator::IMul => {
                if resolved_lhs_ty == TypeDecl::UInt64 && resolved_rhs_ty == TypeDecl::UInt64 {
                    TypeDecl::UInt64
                } else if resolved_lhs_ty == TypeDecl::Int64 && resolved_rhs_ty == TypeDecl::Int64 {
                    TypeDecl::Int64
                } else {
                    return Err(TypeCheckError::type_mismatch_operation("arithmetic", resolved_lhs_ty.clone(), resolved_rhs_ty.clone()));
                }
            }
            Operator::LE | Operator::LT | Operator::GE | Operator::GT | Operator::EQ | Operator::NE => {
                if (resolved_lhs_ty == TypeDecl::UInt64 || resolved_lhs_ty == TypeDecl::Int64) && 
                   (resolved_rhs_ty == TypeDecl::UInt64 || resolved_rhs_ty == TypeDecl::Int64) {
                    TypeDecl::Bool
                } else if resolved_lhs_ty == TypeDecl::Bool && resolved_rhs_ty == TypeDecl::Bool {
                    TypeDecl::Bool
                } else {
                    return Err(TypeCheckError::type_mismatch_operation("comparison", resolved_lhs_ty.clone(), resolved_rhs_ty.clone()));
                }
            }
            Operator::LogicalAnd | Operator::LogicalOr => {
                if resolved_lhs_ty == TypeDecl::Bool && resolved_rhs_ty == TypeDecl::Bool {
                    TypeDecl::Bool
                } else {
                    return Err(TypeCheckError::type_mismatch_operation("logical", resolved_lhs_ty.clone(), resolved_rhs_ty.clone()));
                }
            }
        };
        
        Ok(result_type)
    }

    fn visit_block(&mut self, statements: &Vec<StmtRef>) -> Result<TypeDecl, TypeCheckError> {
        let mut last_empty = true;
        let mut last: Option<TypeDecl> = None;
        
        // Clear type cache at the start of each block to limit cache scope to current block
        self.optimization.type_cache.clear();
        
        // Pre-scan for explicit type declarations and establish global type context
        let mut global_numeric_type: Option<TypeDecl> = None;
        for s in statements.iter() {
            if let Some(stmt) = self.core.stmt_pool.get(s.to_index()) {
                match stmt {
                    Stmt::Val(_, Some(type_decl), _) | Stmt::Var(_, Some(type_decl), _) => {
                        if matches!(type_decl, TypeDecl::Int64 | TypeDecl::UInt64) {
                            global_numeric_type = Some(type_decl.clone());
                            break; // Use the first explicit numeric type found
                        }
                    }
                    _ => {}
                }
            }
        }
        
        // Set global type hint if found
        let original_hint = self.type_inference.type_hint.clone();
        if let Some(ref global_type) = global_numeric_type {
            self.type_inference.type_hint = Some(global_type.clone());
        }
        
        // This code assumes Block(expression) don't make nested function
        // so `return` expression always return for this context.
        for s in statements.iter() {
            let stmt = self.core.stmt_pool.get(s.to_index()).ok_or_else(|| TypeCheckError::generic_error("Invalid statement reference in block"))?;
            let stmt_type = match stmt {
                Stmt::Return(None) => Ok(TypeDecl::Unit),
                Stmt::Return(ret_ty) => {
                    if let Some(e) = ret_ty {
                        let e = e.clone();
                        let expr_obj = self.core.expr_pool.get(e.to_index()).ok_or_else(|| TypeCheckError::generic_error("Invalid expression reference in return"))?;
                        let ty = expr_obj.clone().accept(self)?;
                        if last_empty {
                            last_empty = false;
                            Ok(ty)
                        } else if let Some(last_ty) = last.clone() {
                            if last_ty == ty {
                                Ok(ty)
                            } else {
                                return Err(TypeCheckError::type_mismatch(last_ty, ty).with_context("return statement"));
                            }
                        } else {
                            Ok(ty)
                        }
                    } else {
                        Ok(TypeDecl::Unit)
                    }
                }
                _ => {
                    let stmt_obj = self.core.stmt_pool.get(s.to_index()).ok_or_else(|| TypeCheckError::generic_error("Invalid statement reference"))?;
                    stmt_obj.clone().accept(self)
                }
            };

            match stmt_type {
                Ok(def_ty) => last = Some(def_ty),
                Err(e) => return Err(e),
            }
        }
        
        // Restore original type hint
        self.type_inference.type_hint = original_hint;

        if let Some(last_type) = last {
            Ok(last_type)
        } else {
            Err(TypeCheckError::generic_error("Empty block - no return value"))
        }
    }


    fn visit_if_elif_else(&mut self, _cond: &ExprRef, then_block: &ExprRef, elif_pairs: &Vec<(ExprRef, ExprRef)>, else_block: &ExprRef) -> Result<TypeDecl, TypeCheckError> {
        // Collect all block types
        let mut block_types = Vec::new();

        // Check if-block
        let if_block = then_block.clone();
        let is_if_empty = match self.core.expr_pool.get(if_block.to_index()).ok_or_else(|| TypeCheckError::generic_error("Invalid if block expression reference"))? {
            Expr::Block(expressions) => expressions.is_empty(),
            _ => false,
        };
        if !is_if_empty {
            let if_expr = self.core.expr_pool.get(if_block.to_index()).ok_or_else(|| TypeCheckError::generic_error("Invalid if block expression reference"))?;
            let if_ty = if_expr.clone().accept(self)?;
            block_types.push(if_ty);
        }

        // Check elif-blocks
        for (_, elif_block) in elif_pairs {
            let elif_block = elif_block.clone();
            let is_elif_empty = match self.core.expr_pool.get(elif_block.to_index()).ok_or_else(|| TypeCheckError::generic_error("Invalid elif block expression reference"))? {
                Expr::Block(expressions) => expressions.is_empty(),
                _ => false,
            };
            if !is_elif_empty {
                let elif_expr = self.core.expr_pool.get(elif_block.to_index()).ok_or_else(|| TypeCheckError::generic_error("Invalid elif block expression reference"))?;
                let elif_ty = elif_expr.clone().accept(self)?;
                block_types.push(elif_ty);
            }
        }

        // Check else-block
        let else_block = else_block.clone();
        let is_else_empty = match self.core.expr_pool.get(else_block.to_index()).ok_or_else(|| TypeCheckError::generic_error("Invalid else block expression reference"))? {
            Expr::Block(expressions) => expressions.is_empty(),
            _ => false,
        };
        if !is_else_empty {
            let else_expr = self.core.expr_pool.get(else_block.to_index()).ok_or_else(|| TypeCheckError::generic_error("Invalid else block expression reference"))?;
            let else_ty = else_expr.clone().accept(self)?;
            block_types.push(else_ty);
        }

        // If no blocks have values or all blocks are empty, return Unit
        if block_types.is_empty() {
            return Ok(TypeDecl::Unit);
        }

        // Check if all blocks have the same type
        let first_type = &block_types[0];
        for block_type in &block_types[1..] {
            if block_type != first_type {
                return Ok(TypeDecl::Unit); // Different types, return Unit
            }
        }

        Ok(first_type.clone())
    }

    fn visit_assign(&mut self, lhs: &ExprRef, rhs: &ExprRef) -> Result<TypeDecl, TypeCheckError> {
        let lhs = lhs.clone();
        let rhs = rhs.clone();
        let lhs_ty = {
            let lhs_obj = self.core.expr_pool.get(lhs.to_index()).ok_or_else(|| TypeCheckError::generic_error("Invalid left-hand expression reference"))?;
            lhs_obj.clone().accept(self)?
        };
        let rhs_ty = {
            let rhs_obj = self.core.expr_pool.get(rhs.to_index()).ok_or_else(|| TypeCheckError::generic_error("Invalid right-hand expression reference"))?;
            rhs_obj.clone().accept(self)?
        };
        if lhs_ty != rhs_ty {
            return Err(TypeCheckError::type_mismatch(lhs_ty, rhs_ty).with_context("assignment"));
        }
        Ok(lhs_ty)
    }

    fn visit_identifier(&mut self, name: DefaultSymbol) -> Result<TypeDecl, TypeCheckError> {
        if let Some(val_type) = self.context.get_var(name) {
            // Return the stored type, which may be Number for type inference
            Ok(val_type.clone())
        } else if let Some(fun) = self.context.get_fn(name) {
            Ok(fun.return_type.clone().unwrap_or(TypeDecl::Unknown))
        } else {
            let name_str = self.core.string_interner.resolve(name).unwrap_or("<NOT_FOUND>");
            return Err(TypeCheckError::not_found("Identifier", name_str));
        }
    }

    fn visit_call(&mut self, fn_name: DefaultSymbol, _args: &ExprRef) -> Result<TypeDecl, TypeCheckError> {
        self.push_context();
        if let Some(fun) = self.context.get_fn(fn_name) {
            let status = self.function_checking.is_checked_fn.get(&fn_name);
            if status.is_none() || status.as_ref().and_then(|s| s.as_ref()).is_none() {
                // not checked yet
                let fun = self.context.get_fn(fn_name).ok_or_else(|| TypeCheckError::not_found("Function", "<INTERNAL_ERROR>"))?;
                self.type_check(fun.clone())?;
            }

            self.pop_context();
            Ok(fun.return_type.clone().unwrap_or(TypeDecl::Unknown))
        } else {
            self.pop_context();
            let fn_name_str = self.core.string_interner.resolve(fn_name).unwrap_or("<NOT_FOUND>");
            Err(TypeCheckError::not_found("Function", fn_name_str))
        }
    }

    fn visit_int64_literal(&mut self, _value: &i64) -> Result<TypeDecl, TypeCheckError> {
        Ok(TypeDecl::Int64)
    }

    fn visit_uint64_literal(&mut self, _value: &u64) -> Result<TypeDecl, TypeCheckError> {
        Ok(TypeDecl::UInt64)
    }

    fn visit_number_literal(&mut self, value: DefaultSymbol) -> Result<TypeDecl, TypeCheckError> {
        let num_str = self.core.string_interner.resolve(value)
            .ok_or_else(|| TypeCheckError::generic_error("Failed to resolve number literal"))?;
        
        // If we have a type hint from val/var declaration, validate and return the hint type
        if let Some(hint) = self.type_inference.type_hint.clone() {
            match hint {
                TypeDecl::Int64 => {
                    if let Ok(_val) = num_str.parse::<i64>() {
                        // Return the hinted type - transformation will happen in visit_val or array processing
                        return Ok(hint);
                    } else {
                        return Err(TypeCheckError::conversion_error(num_str, "Int64"));
                    }
                },
                TypeDecl::UInt64 => {
                    if let Ok(_val) = num_str.parse::<u64>() {
                        // Return the hinted type - transformation will happen in visit_val or array processing
                        return Ok(hint);
                    } else {
                        return Err(TypeCheckError::conversion_error(num_str, "UInt64"));
                    }
                },
                _ => {
                    // Other types, fall through to default logic
                }
            }
        }
        
        // Parse the number and determine appropriate type
        if let Ok(val) = num_str.parse::<i64>() {
            if val >= 0 && val <= (i64::MAX) {
                // Positive number that fits in both i64 and u64 - use Number for inference
                Ok(TypeDecl::Number)
            } else {
                // Negative number or very large positive - must be i64
                Ok(TypeDecl::Int64)
            }
        } else if let Ok(_val) = num_str.parse::<u64>() {
            // Very large positive number that doesn't fit in i64 - must be u64
            Ok(TypeDecl::UInt64)
        } else {
            Err(TypeCheckError::invalid_literal(num_str, "number"))
        }
    }

    fn visit_string_literal(&mut self, _value: DefaultSymbol) -> Result<TypeDecl, TypeCheckError> {
        Ok(TypeDecl::String)
    }

    fn visit_boolean_literal(&mut self, _value: &Expr) -> Result<TypeDecl, TypeCheckError> {
        Ok(TypeDecl::Bool)

    }

    fn visit_null_literal(&mut self) -> Result<TypeDecl, TypeCheckError> {
        Ok(TypeDecl::Any)
    }

    fn visit_expr_list(&mut self, _items: &Vec<ExprRef>) -> Result<TypeDecl, TypeCheckError> {
        Ok(TypeDecl::Unit)
    }

    fn visit_array_literal(&mut self, elements: &Vec<ExprRef>) -> Result<TypeDecl, TypeCheckError> {
        if elements.is_empty() {
            return Err(TypeCheckError::array_error("Empty array literals are not supported"));
        }

        // Save the original type hint to restore later
        let original_hint = self.type_inference.type_hint.clone();
        
        // If we have a type hint for the array element type, use it for element type inference
        let element_type_hint = if let Some(TypeDecl::Array(element_types, _)) = &self.type_inference.type_hint {
            if !element_types.is_empty() {
                Some(element_types[0].clone())
            } else {
                None
            }
        } else {
            None
        };

        // Type check all elements with proper type hint for each element
        let mut element_types = Vec::new();
        for element in elements {
            // Set the element type hint for each element individually
            if let Some(ref hint) = element_type_hint {
                self.type_inference.type_hint = Some(hint.clone());
            }
            
            let element_type = self.visit_expr(element)?;
            element_types.push(element_type);
            
            // Restore original hint after processing each element
            self.type_inference.type_hint = original_hint.clone();
        }

        // If we have array type hint, handle type inference for all elements
        if let Some(TypeDecl::Array(ref expected_element_types, _)) = original_hint {
            if !expected_element_types.is_empty() {
                let expected_element_type = &expected_element_types[0];
                
                // Handle type inference for each element
                for (i, element) in elements.iter().enumerate() {
                    match &element_types[i] {
                        TypeDecl::Number => {
                            // Transform Number literals to the expected type
                            self.transform_numeric_expr(element, expected_element_type)?;
                            element_types[i] = expected_element_type.clone();
                        },
                        actual_type if actual_type == expected_element_type => {
                            // Element already has the expected type, but may need AST transformation
                            // Check if this is a number literal that needs transformation
                            if let Some(expr) = self.core.expr_pool.get(element.to_index()) {
                                if matches!(expr, Expr::Number(_)) {
                                    self.transform_numeric_expr(element, expected_element_type)?;
                                }
                            }
                        },
                        TypeDecl::Unknown => {
                            // For variables with unknown type, try to infer from context
                            element_types[i] = expected_element_type.clone();
                        },
                        actual_type if actual_type != expected_element_type => {
                            // Check if type conversion is possible
                            match (actual_type, expected_element_type) {
                                (TypeDecl::Int64, TypeDecl::UInt64) | 
                                (TypeDecl::UInt64, TypeDecl::Int64) => {
                                    return Err(TypeCheckError::array_error(&format!(
                                        "Cannot mix signed and unsigned integers in array. Element {} has type {:?} but expected {:?}",
                                        i, actual_type, expected_element_type
                                    )));
                                },
                                _ => {
                                    // Accept the actual type if it matches expectations
                                    if actual_type == expected_element_type {
                                        // Already matches, no change needed
                                    } else {
                                        return Err(TypeCheckError::array_error(&format!(
                                            "Array element {} has type {:?} but expected {:?}",
                                            i, actual_type, expected_element_type
                                        )));
                                    }
                                }
                            }
                        },
                        _ => {
                            // Type already matches expected type
                        }
                    }
                }
            }
        }

        // Restore the original type hint
        self.type_inference.type_hint = original_hint;

        let first_type = &element_types[0];
        for (i, element_type) in element_types.iter().enumerate() {
            if element_type != first_type {
                return Err(TypeCheckError::array_error(&format!(
                    "Array elements must have the same type, but element {} has type {:?} while first element has type {:?}",
                    i, element_type, first_type
                )));
            }
        }

        Ok(TypeDecl::Array(element_types, elements.len()))
    }

    fn visit_array_access(&mut self, array: &ExprRef, index: &ExprRef) -> Result<TypeDecl, TypeCheckError> {
        let array_type = self.visit_expr(array)?;
        
        // Set type hint for index to UInt64 (default for array indexing)
        let original_hint = self.type_inference.type_hint.clone();
        self.type_inference.type_hint = Some(TypeDecl::UInt64);
        
        let index_type = self.visit_expr(index)?;
        
        // Restore original type hint
        self.type_inference.type_hint = original_hint;

        // Handle index type inference and conversion
        let _final_index_type = match index_type {
            TypeDecl::Number => {
                // Transform Number index to UInt64 (default for array indexing)
                self.transform_numeric_expr(index, &TypeDecl::UInt64)?;
                TypeDecl::UInt64
            },
            TypeDecl::Unknown => {
                // Infer index as UInt64 for unknown types (likely variables)
                TypeDecl::UInt64
            },
            TypeDecl::UInt64 | TypeDecl::Int64 => {
                // Already a valid integer type
                index_type
            },
            _ => {
                return Err(TypeCheckError::array_error(&format!(
                    "Array index must be an integer type, but got {:?}", index_type
                )));
            }
        };

        // Array must be an array type
        match array_type {
            TypeDecl::Array(ref element_types, _size) => {
                if element_types.is_empty() {
                    return Err(TypeCheckError::array_error("Cannot access elements of empty array"));
                }
                Ok(element_types[0].clone())
            }
            _ => Err(TypeCheckError::array_error(&format!(
                "Cannot index into non-array type {:?}", array_type
            )))
        }
    }

    fn visit_expression_stmt(&mut self, expr: &ExprRef) -> Result<TypeDecl, TypeCheckError> {
        let expr_obj = self.core.expr_pool.get(expr.to_index()).ok_or_else(|| TypeCheckError::generic_error("Invalid expression reference in statement"))?;
        expr_obj.clone().accept(self)
    }

    fn visit_var(&mut self, name: DefaultSymbol, type_decl: &Option<TypeDecl>, expr: &Option<ExprRef>) -> Result<TypeDecl, TypeCheckError> {
        let type_decl = type_decl.clone();
        let expr = expr.clone();
        self.process_val_type(name, &type_decl, &expr)?;
        Ok(TypeDecl::Unit)
    }

    fn visit_val(&mut self, name: DefaultSymbol, type_decl: &Option<TypeDecl>, expr: &ExprRef) -> Result<TypeDecl, TypeCheckError> {
        let expr_ref = expr.clone();
        let type_decl = type_decl.clone();
        
        // Set type hint and evaluate expression
        let old_hint = self.setup_type_hint_for_val(&type_decl);
        let expr_ty = self.visit_expr(&expr_ref)?;
        
        // Manage variable-expression mapping
        self.update_variable_expr_mapping(name, &expr_ref, &expr_ty);
        
        // Apply type transformations
        self.apply_type_transformations(&type_decl, &expr_ty, &expr_ref)?;
        
        // Determine final type and store variable
        let final_type = self.determine_final_type(&type_decl, &expr_ty);
        self.context.set_var(name, final_type);
        
        // Restore previous type hint
        self.type_inference.type_hint = old_hint;
        
        Ok(TypeDecl::Unit)
    }


    fn visit_return(&mut self, expr: &Option<ExprRef>) -> Result<TypeDecl, TypeCheckError> {
        if expr.is_none() {
            Ok(TypeDecl::Unit)
        } else {
            let e = expr.as_ref().ok_or_else(|| TypeCheckError::generic_error("Expected expression in return"))?;
            let expr_obj = self.core.expr_pool.get(e.to_index()).ok_or_else(|| TypeCheckError::generic_error("Invalid expression reference in return"))?;
            expr_obj.clone().accept(self)?;
            Ok(TypeDecl::Unit)
        }
    }

    fn visit_for(&mut self, init: DefaultSymbol, _cond: &ExprRef, range: &ExprRef, body: &ExprRef) -> Result<TypeDecl, TypeCheckError> {
        self.push_context();
        let range_obj = self.core.expr_pool.get(range.to_index()).ok_or_else(|| TypeCheckError::generic_error("Invalid range expression reference"))?;
        let range_ty = range_obj.clone().accept(self)?;
        let ty = Some(range_ty);
        self.process_val_type(init, &ty, &Some(*range))?;
        let body_obj = self.core.expr_pool.get(body.to_index()).ok_or_else(|| TypeCheckError::generic_error("Invalid body expression reference"))?;
        let res = body_obj.clone().accept(self);
        self.pop_context();
        res
    }

    fn visit_while(&mut self, _cond: &ExprRef, body: &ExprRef) -> Result<TypeDecl, TypeCheckError> {
        let body_obj = self.core.expr_pool.get(body.to_index()).ok_or_else(|| TypeCheckError::generic_error("Invalid body expression reference in while"))?;
        body_obj.clone().accept(self)
    }

    fn visit_break(&mut self) -> Result<TypeDecl, TypeCheckError> {
        Ok(TypeDecl::Unit)
    }

    fn visit_continue(&mut self) -> Result<TypeDecl, TypeCheckError> {
        Ok(TypeDecl::Unit)
    }

    fn visit_struct_decl(&mut self, name: &String, fields: &Vec<StructField>) -> Result<TypeDecl, TypeCheckError> {
        // Struct declaration type checking - actual processing is not implemented yet
        // Check field types for validity
        for field in fields {
            // Check if each field type is valid
            match &field.type_decl {
                TypeDecl::Int64 | TypeDecl::UInt64 | TypeDecl::Bool | TypeDecl::String => {
                    // Valid types
                },
                _ => {
                    return Err(TypeCheckError::unsupported_operation(
                        &format!("field type in struct '{}'", name), field.type_decl.clone()
                    ));
                }
            }
        }
        
        // Struct declaration returns Unit
        Ok(TypeDecl::Unit)
    }

    fn visit_impl_block(&mut self, target_type: &String, methods: &Vec<Rc<MethodFunction>>) -> Result<TypeDecl, TypeCheckError> {
        // Impl block type checking - validate methods
        for method in methods {
            // Check method parameter types
            for (_, param_type) in &method.parameter {
                match param_type {
                    TypeDecl::Int64 | TypeDecl::UInt64 | TypeDecl::Bool | TypeDecl::String => {
                        // Valid parameter types
                    },
                    _ => {
                        let method_name = self.core.string_interner.resolve(method.name).unwrap_or("<unknown>");
                        return Err(TypeCheckError::unsupported_operation(
                            &format!("parameter type in method '{}' for impl block '{}'", method_name, target_type),
                            param_type.clone()
                        ));
                    }
                }
            }
            
            // Check return type if specified
            if let Some(ref ret_type) = method.return_type {
                match ret_type {
                    TypeDecl::Int64 | TypeDecl::UInt64 | TypeDecl::Bool | TypeDecl::String | TypeDecl::Unit => {
                        // Valid return types
                    },
                    _ => {
                        let method_name = self.core.string_interner.resolve(method.name).unwrap_or("<unknown>");
                        return Err(TypeCheckError::unsupported_operation(
                            &format!("return type in method '{}' for impl block '{}'", method_name, target_type),
                            ret_type.clone()
                        ));
                    }
                }
            }
        }
        
        // Impl block declaration returns Unit
        Ok(TypeDecl::Unit)
    }

    fn visit_field_access(&mut self, obj: &ExprRef, field: &DefaultSymbol) -> Result<TypeDecl, TypeCheckError> {
        let obj_type = self.visit_expr(obj)?;
        
        // For now, we assume all field accesses return the type of the field
        // This is a simplified implementation - in practice, we'd need to look up
        // the struct definition and check the field type
        match obj_type {
            TypeDecl::Identifier(_) | TypeDecl::Struct(_) => {
                // Assume field access on custom types is valid for now
                // Return a placeholder type - this should be improved to look up actual field types
                Ok(TypeDecl::Unknown)
            }
            _ => {
                let field_name = self.core.string_interner.resolve(*field).unwrap_or("<unknown>");
                Err(TypeCheckError::unsupported_operation(
                    &format!("field access '{}'", field_name), obj_type
                ))
            }
        }
    }

    fn visit_method_call(&mut self, obj: &ExprRef, method: &DefaultSymbol, args: &Vec<ExprRef>) -> Result<TypeDecl, TypeCheckError> {
        let obj_type = self.visit_expr(obj)?;
        
        // Type check all arguments
        for arg in args {
            self.visit_expr(arg)?;
        }
        
        let method_name = self.core.string_interner.resolve(*method).unwrap_or("<unknown>");
        
        // Handle built-in methods for basic types
        match obj_type {
            TypeDecl::String => {
                match method_name {
                    "len" => {
                        // String.len() method - no arguments required, returns u64
                        if !args.is_empty() {
                            return Err(TypeCheckError::method_error(
                                "len", TypeDecl::String, &format!("takes no arguments, but {} provided", args.len())
                            ));
                        }
                        Ok(TypeDecl::UInt64)
                    }
                    _ => {
                        Err(TypeCheckError::method_error(
                            method_name, TypeDecl::String, "method not found"
                        ))
                    }
                }
            }
            TypeDecl::Identifier(_) | TypeDecl::Struct(_) => {
                // Assume method calls on custom types are valid for now
                // Return a placeholder type - this should be improved to look up actual method return types
                Ok(TypeDecl::Unknown)
            }
            _ => {
                Err(TypeCheckError::method_error(
                    method_name, obj_type, "method call on non-struct type"
                ))
            }
        }
    }

    fn visit_struct_literal(&mut self, struct_name: &DefaultSymbol, fields: &Vec<(DefaultSymbol, ExprRef)>) -> Result<TypeDecl, TypeCheckError> {
        // Type check all field values
        for (_field_name, field_expr) in fields {
            self.visit_expr(field_expr)?;
        }
        
        // Return the struct type
        Ok(TypeDecl::Struct(*struct_name))
    }
}

impl<'a, 'b, 'c, 'd> TypeCheckerVisitor<'a, 'b, 'c, 'd> {
    /// Get cached type for an expression if available
    fn get_cached_type(&self, expr_ref: &ExprRef) -> Option<&TypeDecl> {
        self.optimization.type_cache.get(expr_ref)
    }
    
    /// Cache type result for an expression
    fn cache_type(&mut self, expr_ref: ExprRef, type_decl: TypeDecl) {
        self.optimization.type_cache.insert(expr_ref, type_decl);
    }

    /// Sets up type hint for variable declaration and returns the old hint
    fn setup_type_hint_for_val(&mut self, type_decl: &Option<TypeDecl>) -> Option<TypeDecl> {
        let old_hint = self.type_inference.type_hint.clone();
        
        if let Some(decl) = type_decl {
            match decl {
                TypeDecl::Array(element_types, _) => {
                    // For array types, set the array type as hint for array literal processing
                    if !element_types.is_empty() {
                        self.type_inference.type_hint = Some(decl.clone());
                    }
                },
                _ if decl != &TypeDecl::Unknown && decl != &TypeDecl::Number => {
                    self.type_inference.type_hint = Some(decl.clone());
                },
                _ => {}
            }
        }
        
        old_hint
    }

    /// Updates variable-expression mapping for type inference
    fn update_variable_expr_mapping(&mut self, name: DefaultSymbol, expr_ref: &ExprRef, expr_ty: &TypeDecl) {
        if *expr_ty == TypeDecl::Number || (*expr_ty != TypeDecl::Number && self.has_number_in_expr(expr_ref)) {
            self.type_inference.variable_expr_mapping.insert(name, expr_ref.clone());
        } else {
            // Remove old mapping for non-Number types to prevent stale references
            self.type_inference.variable_expr_mapping.remove(&name);
            // Also remove from number_usage_context to prevent stale type inference
            let indices_to_remove: Vec<usize> = self.type_inference.number_usage_context
                .iter()
                .enumerate()
                .filter_map(|(i, (old_expr, _))| {
                    if self.is_old_number_for_variable(name, old_expr) {
                        Some(i)
                    } else {
                        None
                    }
                })
                .collect();
            
            // Remove in reverse order to maintain valid indices
            for &index in indices_to_remove.iter().rev() {
                self.type_inference.number_usage_context.remove(index);
            }
        }
    }

    /// Applies type transformations for numeric expressions
    fn apply_type_transformations(&mut self, type_decl: &Option<TypeDecl>, expr_ty: &TypeDecl, expr_ref: &ExprRef) -> Result<(), TypeCheckError> {
        if type_decl.is_none() && *expr_ty == TypeDecl::Number {
            // No explicit type, but we have a Number - use type hint if available
            if let Some(hint) = self.type_inference.type_hint.clone() {
                if matches!(hint, TypeDecl::Int64 | TypeDecl::UInt64) {
                    // Transform Number to hinted type
                    self.transform_numeric_expr(expr_ref, &hint)?;
                }
            }
        } else if type_decl.as_ref().map_or(false, |decl| *decl == TypeDecl::Unknown) && *expr_ty == TypeDecl::Int64 {
            // Unknown type declaration with Int64 inference - also transform
            if let Some(hint) = self.type_inference.type_hint.clone() {
                if matches!(hint, TypeDecl::Int64 | TypeDecl::UInt64) {
                    self.transform_numeric_expr(expr_ref, &hint)?;
                }
            }
        } else if let Some(decl) = type_decl {
            if decl != &TypeDecl::Unknown && decl != &TypeDecl::Number && *expr_ty == *decl {
                // Expression returned the hinted type, transform Number literals to concrete type
                if let Some(expr) = self.core.expr_pool.get(expr_ref.to_index()) {
                    if let Expr::Number(_) = expr {
                        self.transform_numeric_expr(expr_ref, decl)?;
                    }
                }
            }
        }
        
        Ok(())
    }

    /// Determines the final type for a variable declaration
    fn determine_final_type(&self, type_decl: &Option<TypeDecl>, expr_ty: &TypeDecl) -> TypeDecl {
        match (type_decl, expr_ty) {
            (Some(TypeDecl::Unknown), _) => expr_ty.clone(),
            (Some(decl), _) if decl != &TypeDecl::Unknown && decl != &TypeDecl::Number => decl.clone(),
            (None, _) => expr_ty.clone(),
            _ => expr_ty.clone(),
        }
    }

    // Transform Expr::Number nodes to concrete types based on resolved types
    fn transform_numeric_expr(&mut self, expr_ref: &ExprRef, target_type: &TypeDecl) -> Result<(), TypeCheckError> {
        if let Some(expr) = self.core.expr_pool.get_mut(expr_ref.to_index()) {
            if let Expr::Number(value) = expr {
                let num_str = self.core.string_interner.resolve(*value)
                    .ok_or_else(|| TypeCheckError::generic_error("Failed to resolve number literal"))?;
                
                match target_type {
                    TypeDecl::UInt64 => {
                        if let Ok(val) = num_str.parse::<u64>() {
                            *expr = Expr::UInt64(val);
                        } else {
                            return Err(TypeCheckError::conversion_error(num_str, "UInt64"));
                        }
                    },
                    TypeDecl::Int64 => {
                        if let Ok(val) = num_str.parse::<i64>() {
                            *expr = Expr::Int64(val);
                        } else {
                            return Err(TypeCheckError::conversion_error(num_str, "Int64"));
                        }
                    },
                    _ => {
                        return Err(TypeCheckError::unsupported_operation("transform", target_type.clone()));
                    }
                }
            }
        }
        Ok(())
    }

    // Update variable type in context if identifier was type-converted
    fn update_identifier_types(&mut self, expr_ref: &ExprRef, original_ty: &TypeDecl, resolved_ty: &TypeDecl) -> Result<(), TypeCheckError> {
        if original_ty == &TypeDecl::Number && resolved_ty != &TypeDecl::Number {
            if let Some(expr) = self.core.expr_pool.get(expr_ref.to_index()) {
                if let Expr::Identifier(name) = expr {
                    // Update the variable's type
                    self.context.update_var_type(*name, resolved_ty.clone());
                }
            }
        }
        Ok(())
    }

    // Record Number usage context for both identifiers and direct Number literals
    fn record_number_usage_context(&mut self, expr_ref: &ExprRef, original_ty: &TypeDecl, resolved_ty: &TypeDecl) -> Result<(), TypeCheckError> {
        if original_ty == &TypeDecl::Number && resolved_ty != &TypeDecl::Number {
            if let Some(expr) = self.core.expr_pool.get(expr_ref.to_index()) {
                match expr {
                    Expr::Identifier(name) => {
                        // Find all Number expressions that might belong to this variable
                        // and record the context type
                        for i in 0..self.core.expr_pool.len() {
                            if let Some(candidate_expr) = self.core.expr_pool.get(i) {
                                if let Expr::Number(_) = candidate_expr {
                                    let candidate_ref = ExprRef(i as u32);
                                    // Check if this Number might be associated with this variable
                                    if self.is_number_for_variable(*name, &candidate_ref) {
                                        self.type_inference.number_usage_context.push((candidate_ref, resolved_ty.clone()));
                                    }
                                }
                            }
                        }
                    }
                    Expr::Number(_) => {
                        // Direct Number literal - record its resolved type
                        self.type_inference.number_usage_context.push((expr_ref.clone(), resolved_ty.clone()));
                    }
                    _ => {}
                }
            }
        }
        
        Ok(())
    }

    // Check if an expression contains Number literals
    fn has_number_in_expr(&self, expr_ref: &ExprRef) -> bool {
        if let Some(expr) = self.core.expr_pool.get(expr_ref.to_index()) {
            match expr {
                Expr::Number(_) => true,
                _ => false, // For now, only check direct Number literals
            }
        } else {
            false
        }
    }

    // Check if a Number expression is associated with a specific variable
    fn is_number_for_variable(&self, var_name: DefaultSymbol, number_expr_ref: &ExprRef) -> bool {
        // Use the recorded mapping to check if this Number expression belongs to this variable
        if let Some(mapped_expr_ref) = self.type_inference.variable_expr_mapping.get(&var_name) {
            return mapped_expr_ref == number_expr_ref;
        }
        false
    }
    
    // Check if an old Number expression might be associated with a variable for cleanup
    fn is_old_number_for_variable(&self, _var_name: DefaultSymbol, number_expr_ref: &ExprRef) -> bool {
        // Check if this Number expression was previously mapped to this variable
        // This is used for cleanup when variables are redefined
        if let Some(expr) = self.core.expr_pool.get(number_expr_ref.to_index()) {
            if let Expr::Number(_) = expr {
                // For now, we'll be conservative and remove all Number contexts when variables are redefined
                return true;
            }
        }
        false
    }

    // Propagate concrete type to Number variable immediately
    fn propagate_to_number_variable(&mut self, expr_ref: &ExprRef, target_type: &TypeDecl) -> Result<(), TypeCheckError> {
        if let Some(expr) = self.core.expr_pool.get(expr_ref.to_index()) {
            if let Expr::Identifier(name) = expr {
                if let Some(var_type) = self.context.get_var(*name) {
                    if var_type == TypeDecl::Number {
                        // Find and record the Number expression for this variable
                        for i in 0..self.core.expr_pool.len() {
                            if let Some(candidate_expr) = self.core.expr_pool.get(i) {
                                if let Expr::Number(_) = candidate_expr {
                                    let candidate_ref = ExprRef(i as u32);
                                    if self.is_number_for_variable(*name, &candidate_ref) {
                                        self.type_inference.number_usage_context.push((candidate_ref, target_type.clone()));
                                        // Update variable type in context
                                        self.context.update_var_type(*name, target_type.clone());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    // Finalize any remaining Number types with context-aware inference
    fn finalize_number_types(&mut self) -> Result<(), TypeCheckError> {
        // Use recorded context information to transform Number expressions
        let context_info = self.type_inference.number_usage_context.clone();
        for (expr_ref, target_type) in &context_info {
            if let Some(expr) = self.core.expr_pool.get(expr_ref.to_index()) {
                if let Expr::Number(_) = expr {
                    self.transform_numeric_expr(&expr_ref, &target_type)?;
                    
                    // Update variable types in context if this expression is mapped to a variable
                    for (var_name, mapped_expr_ref) in &self.type_inference.variable_expr_mapping.clone() {
                        if mapped_expr_ref == expr_ref {
                            self.context.update_var_type(*var_name, target_type.clone());
                        }
                    }
                }
            }
        }
        
        // Second pass: handle any remaining Number types by using variable context
        let expr_len = self.core.expr_pool.len();
        for i in 0..expr_len {
            if let Some(expr) = self.core.expr_pool.get(i) {
                if let Expr::Number(_) = expr {
                    let expr_ref = ExprRef(i as u32);
                    
                    // Skip if already processed in first pass
                    let already_processed = context_info.iter().any(|(processed_ref, _)| processed_ref == &expr_ref);
                    if already_processed {
                        continue;
                    }
                    
                    // Find if this Number is associated with a variable and use its final type
                    // Use type hint if available, otherwise default to UInt64
                    let mut target_type = self.type_inference.type_hint.clone().unwrap_or(TypeDecl::UInt64);
                    
                    for (var_name, mapped_expr_ref) in &self.type_inference.variable_expr_mapping {
                        if mapped_expr_ref == &expr_ref {
                            // Check the current type of this variable in context
                            if let Some(var_type) = self.context.get_var(*var_name) {
                                if var_type != TypeDecl::Number {
                                    target_type = var_type;
                                    break;
                                }
                            }
                        }
                    }
                    
                    self.transform_numeric_expr(&expr_ref, &target_type)?;
                    
                    // Update variable types in context if this expression is mapped to a variable
                    for (var_name, mapped_expr_ref) in &self.type_inference.variable_expr_mapping.clone() {
                        if mapped_expr_ref == &expr_ref {
                            self.context.update_var_type(*var_name, target_type.clone());
                        }
                    }
                }
            }
        }
        Ok(())
    }


    // Helper method to resolve numeric types with automatic conversion
    fn resolve_numeric_types(&self, lhs_ty: &TypeDecl, rhs_ty: &TypeDecl) -> Result<(TypeDecl, TypeDecl), TypeCheckError> {
        match (lhs_ty, rhs_ty) {
            // Both types are already concrete - no conversion needed
            (TypeDecl::UInt64, TypeDecl::UInt64) => Ok((TypeDecl::UInt64, TypeDecl::UInt64)),
            (TypeDecl::Int64, TypeDecl::Int64) => Ok((TypeDecl::Int64, TypeDecl::Int64)),
            (TypeDecl::Bool, TypeDecl::Bool) => Ok((TypeDecl::Bool, TypeDecl::Bool)),
            (TypeDecl::String, TypeDecl::String) => Ok((TypeDecl::String, TypeDecl::String)),
            
            // Number type automatic conversion
            (TypeDecl::Number, TypeDecl::UInt64) => Ok((TypeDecl::UInt64, TypeDecl::UInt64)),
            (TypeDecl::UInt64, TypeDecl::Number) => Ok((TypeDecl::UInt64, TypeDecl::UInt64)),
            (TypeDecl::Number, TypeDecl::Int64) => Ok((TypeDecl::Int64, TypeDecl::Int64)),
            (TypeDecl::Int64, TypeDecl::Number) => Ok((TypeDecl::Int64, TypeDecl::Int64)),
            
            // Two Number types - check if we have a context hint, otherwise default to UInt64
            (TypeDecl::Number, TypeDecl::Number) => {
                if let Some(hint) = &self.type_inference.type_hint {
                    match hint {
                        TypeDecl::Int64 => Ok((TypeDecl::Int64, TypeDecl::Int64)),
                        TypeDecl::UInt64 => Ok((TypeDecl::UInt64, TypeDecl::UInt64)),
                        _ => Ok((TypeDecl::UInt64, TypeDecl::UInt64)),
                    }
                } else {
                    Ok((TypeDecl::UInt64, TypeDecl::UInt64))
                }
            },
            
            // Cross-type operations (UInt64 vs Int64) - generally not allowed for safety
            (TypeDecl::UInt64, TypeDecl::Int64) | (TypeDecl::Int64, TypeDecl::UInt64) => {
                Err(TypeCheckError::type_mismatch_operation("mixed signed/unsigned", lhs_ty.clone(), rhs_ty.clone()))
            },
            
            // Other type mismatches
            _ => {
                if lhs_ty == rhs_ty {
                    Ok((lhs_ty.clone(), rhs_ty.clone()))
                } else {
                    Err(TypeCheckError::type_mismatch(lhs_ty.clone(), rhs_ty.clone()))
                }
            }
        }
    }
    
    // Propagate type to Number expression and associated variables
    fn propagate_type_to_number_expr(&mut self, expr_ref: &ExprRef, target_type: &TypeDecl) -> Result<(), TypeCheckError> {
        if let Some(expr) = self.core.expr_pool.get(expr_ref.to_index()) {
            match expr {
                Expr::Identifier(name) => {
                    // If this is an identifier with Number type, update it
                    if let Some(var_type) = self.context.get_var(*name) {
                        if var_type == TypeDecl::Number {
                            self.context.update_var_type(*name, target_type.clone());
                            // Also record for Number expression transformation
                            if let Some(mapped_expr) = self.type_inference.variable_expr_mapping.get(name) {
                                self.type_inference.number_usage_context.push((mapped_expr.clone(), target_type.clone()));
                            }
                        }
                    }
                },
                Expr::Number(_) => {
                    // Direct Number literal
                    self.type_inference.number_usage_context.push((expr_ref.clone(), target_type.clone()));
                },
                _ => {
                    // For other expression types, we might need to recurse
                }
            }
        }
        Ok(())
    }
}