//! Renames module-level names inside a parsed module.
//!
//! Names are looked up scope-aware: a parameter, `let`, loop variable or closure parameter with
//! the same name as an item shadows it, exactly as in the type checker.

use std::collections::{HashMap, HashSet};

use telvax_ast::*;

pub struct Rewriter<'a> {
    /// Module-level name -> program-wide name.
    names: &'a HashMap<String, String>,
    scopes: Vec<HashSet<String>>,
}

impl<'a> Rewriter<'a> {
    pub fn new(names: &'a HashMap<String, String>) -> Self {
        Self {
            names,
            scopes: Vec::new(),
        }
    }

    fn bound(&self, name: &str) -> bool {
        self.scopes.iter().any(|s| s.contains(name))
    }

    fn bind(&mut self, name: &str) {
        if let Some(s) = self.scopes.last_mut() {
            s.insert(name.to_string());
        }
    }

    fn rename(&self, id: &mut Ident) {
        if let Some(new) = self.names.get(&id.node) {
            id.node = new.clone();
        }
    }

    pub fn item(&mut self, item: &mut Item) {
        match item {
            Item::Import(_) | Item::Serve(_) => {}
            Item::Let(l) => {
                self.scopes.push(HashSet::new());
                self.let_decl(l, false);
                self.scopes.pop();
            }
            Item::Struct(s) => {
                self.rename(&mut s.name);
                for f in &mut s.fields {
                    self.ty(&mut f.ty);
                    if let Some(d) = &mut f.default {
                        self.expr(d);
                    }
                }
            }
            Item::Enum(e) => {
                self.rename(&mut e.name);
                for v in &mut e.variants {
                    for t in &mut v.fields {
                        self.ty(t);
                    }
                }
            }
            Item::Alias(a) => {
                self.rename(&mut a.name);
                self.ty(&mut a.ty);
            }
            Item::Trait(t) => {
                self.rename(&mut t.name);
                for f in &mut t.fns {
                    self.function_named(f, false);
                }
            }
            Item::Impl(imp) => {
                self.rename(&mut imp.ty);
                if let Some(tr) = &mut imp.trait_name {
                    self.rename(tr);
                }
                // The methods keep their names: they are reached through the type.
                for f in &mut imp.fns {
                    self.function_named(f, false);
                }
            }
            Item::Fn(f) => self.function(f),
        }
    }

    /// Renames the enum in a path such as `Shape::Circle` (the variant is not a module item).
    fn enum_path(&self, p: &mut [Ident]) {
        if p.len() == 2 && !self.bound(&p[0].node) {
            self.rename(&mut p[0]);
        }
    }

    fn pattern(&mut self, pat: &mut Pattern) {
        match &mut pat.node {
            PatKind::Bind(name) => self.bind(name),
            PatKind::Variant { path, args } => {
                self.enum_path(path);
                args.iter_mut().for_each(|a| self.pattern(a));
            }
            PatKind::Tuple(items) | PatKind::Or(items) => {
                items.iter_mut().for_each(|a| self.pattern(a))
            }
            PatKind::List {
                before,
                rest,
                after,
            } => {
                before
                    .iter_mut()
                    .chain(rest.as_deref_mut())
                    .chain(after.iter_mut())
                    .for_each(|a| self.pattern(a));
            }
            PatKind::Struct { path, fields } => {
                if path.len() == 1 {
                    self.rename(&mut path[0]);
                }
                fields.iter_mut().for_each(|(_, a)| self.pattern(a));
            }
            PatKind::Wild | PatKind::Lit(_) | PatKind::NegLit(_) => {}
        }
    }

    fn function(&mut self, f: &mut Fn) {
        self.function_named(f, true);
    }

    /// `rename_name`: whether the function's own name is a module-level name (not for methods).
    fn function_named(&mut self, f: &mut Fn, rename_name: bool) {
        if rename_name {
            self.rename(&mut f.name);
        }
        self.scopes.push(HashSet::new());
        for attr in &mut f.attrs {
            for (_, value) in &mut attr.args {
                self.expr(value);
            }
        }
        for p in &mut f.params {
            self.ty(&mut p.ty);
            if let Some(d) = &mut p.default {
                self.expr(d);
            }
            self.bind(&p.name.node);
        }
        if let Some(r) = &mut f.ret {
            self.ty(r);
        }
        self.block(&mut f.body);
        self.scopes.pop();
    }

    /// `declare`: whether the `let` binds its name in the current scope (false for globals,
    /// whose name is an item).
    fn let_decl(&mut self, l: &mut Let, declare: bool) {
        if let Some(t) = &mut l.ty {
            self.ty(t);
        }
        self.expr(&mut l.init);
        if declare {
            self.bind(&l.name.node);
        } else {
            self.rename(&mut l.name);
        }
    }

    fn ty(&mut self, t: &mut Type) {
        if t.path.len() == 1 {
            self.rename(&mut t.path[0]);
        }
        for a in &mut t.args {
            match a {
                TypeArg::Type(inner) => self.ty(inner),
                TypeArg::Row(fields) => fields.iter_mut().for_each(|f| self.ty(&mut f.ty)),
                TypeArg::Shape(_) => {}
            }
        }
    }

    fn block(&mut self, b: &mut Block) {
        self.scopes.push(HashSet::new());
        for s in &mut b.stmts {
            match s {
                Stmt::Let(l) => self.let_decl(l, true),
                Stmt::Expr(e) => self.expr(e),
            }
        }
        if let Some(t) = &mut b.tail {
            self.expr(t);
        }
        self.scopes.pop();
    }

    fn expr(&mut self, e: &mut Expr) {
        match &mut e.node {
            ExprKind::Lit(_) | ExprKind::Error => {}
            ExprKind::Path(p) => {
                if p.len() == 1 && !self.bound(&p[0].node) {
                    self.rename(&mut p[0]);
                } else {
                    self.enum_path(p);
                }
            }
            ExprKind::Match { scrutinee, arms } => {
                self.expr(scrutinee);
                for arm in arms {
                    self.scopes.push(HashSet::new());
                    self.pattern(&mut arm.pat);
                    if let Some(g) = &mut arm.guard {
                        self.expr(g);
                    }
                    self.expr(&mut arm.body);
                    self.scopes.pop();
                }
            }
            ExprKind::Call { callee, args } => {
                self.expr(callee);
                args.iter_mut().for_each(|a| self.expr(a));
            }
            ExprKind::Field { base, .. } => self.expr(base),
            ExprKind::Named { value, .. } => self.expr(value),
            ExprKind::Unary { expr, .. } | ExprKind::Try(expr) | ExprKind::Await(expr) => {
                self.expr(expr)
            }
            ExprKind::Tuple(items) => items.iter_mut().for_each(|a| self.expr(a)),
            ExprKind::Binary { lhs, rhs, .. } | ExprKind::Pipe { lhs, rhs } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Closure {
                params,
                types,
                body,
            } => {
                types.iter_mut().flatten().for_each(|t| self.ty(t));
                self.scopes
                    .push(params.iter().map(|p| p.node.clone()).collect());
                self.expr(body);
                self.scopes.pop();
            }
            ExprKind::StructLit { path, fields } => {
                if path.len() == 1 {
                    self.rename(&mut path[0]);
                }
                fields.iter_mut().for_each(|(_, v)| self.expr(v));
            }
            ExprKind::If {
                cond,
                then,
                otherwise,
            } => {
                self.expr(cond);
                self.block(then);
                if let Some(o) = otherwise {
                    self.expr(o);
                }
            }
            ExprKind::Block(b) => self.block(b),
            ExprKind::Return(v) => {
                if let Some(v) = v {
                    self.expr(v);
                }
            }
            ExprKind::Break | ExprKind::Continue => {}
            ExprKind::FString(parts) => {
                for p in parts {
                    for x in p.exprs_mut() {
                        self.expr(x);
                    }
                }
            }
            ExprKind::Assign { target, value } => {
                self.expr(target);
                self.expr(value);
            }
            ExprKind::While { cond, body } => {
                self.expr(cond);
                self.block(body);
            }
            ExprKind::For { var, iter, body } => {
                match iter {
                    ForIter::Range { start, end, .. } => {
                        self.expr(start);
                        self.expr(end);
                    }
                    ForIter::Expr(v) => self.expr(v),
                }
                self.scopes.push(HashSet::from([var.node.clone()]));
                self.block(body);
                self.scopes.pop();
            }
            ExprKind::Index { base, index } => {
                self.expr(base);
                self.expr(index);
            }
            ExprKind::Array(items) => items.iter_mut().for_each(|i| self.expr(i)),
        }
    }
}
