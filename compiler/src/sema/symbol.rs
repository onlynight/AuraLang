//! 符号表与作用域管理（P3 任务 3.1）
//!
//! 作用域链结构：
//! GlobalScope (模块级：函数、类型、全局变量)
//! └── FunctionScope (函数参数、函数内局部)
//!     └── BlockScope (块级局部)

use crate::Span;
use crate::ast::{Type, Visibility};
use crate::sema::ty::Ty;
use std::collections::HashMap;

/// 符号种类
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolKind {
    /// 变量（val/var/参数/结构体字段）
    Variable { is_mutable: bool },
    /// 函数
    Function { params: Vec<ParamSym>, return_type: Ty },
    /// 类型（struct/enum/class/interface/别名）
    Type,
    /// 枚举变体
    EnumVariant,
    /// 模块
    Module,
}

/// 参数符号
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamSym {
    pub name: String,
    pub ty: Ty,
    pub has_default: bool,
    /// 是否为 vararg 可变参数
    pub is_vararg: bool,
}

/// 符号表中的单个条目
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub visibility: Visibility,
    pub span: Span,
    pub defined: bool,
}

impl Symbol {
    pub fn new(
        name: impl Into<String>,
        kind: SymbolKind,
        visibility: Visibility,
        span: Span,
    ) -> Self {
        Self {
            name: name.into(),
            kind,
            visibility,
            span,
            defined: true,
        }
    }
}

/// 单个作用域
#[derive(Debug, Clone)]
pub struct Scope {
    pub parent: Option<usize>,
    pub symbols: HashMap<String, Symbol>,
    pub is_function_scope: bool,
}

impl Scope {
    pub fn new(parent: Option<usize>, is_function_scope: bool) -> Self {
        Self {
            parent,
            symbols: HashMap::new(),
            is_function_scope,
        }
    }
}

/// 符号表：作用域链 + 类型/函数索引
#[derive(Debug, Clone)]
pub struct SymbolTable {
    scopes: Vec<Scope>,
    current_scope: usize,
    /// 全局类型索引：名字 -> Ty（struct/enum/class/interface/别名）
    pub types: HashMap<String, Ty>,
    /// 全局函数索引：名字 -> 函数符号（支持重载时用 Vec）
    pub functions: HashMap<String, Vec<Symbol>>,
}

impl SymbolTable {
    pub fn new() -> Self {
        let mut table = Self {
            scopes: Vec::new(),
            current_scope: 0,
            types: HashMap::new(),
            functions: HashMap::new(),
        };
        let global = Scope::new(None, false);
        table.scopes.push(global);
        table.current_scope = 0;
        table
    }

    // ── 作用域管理 ──

    pub fn enter_scope(&mut self, is_function_scope: bool) {
        let parent = self.current_scope;
        let idx = self.scopes.len();
        self.scopes.push(Scope::new(Some(parent), is_function_scope));
        self.current_scope = idx;
    }

    pub fn exit_scope(&mut self) {
        if let Some(parent) = self.scopes[self.current_scope].parent {
            self.current_scope = parent;
        }
    }

    pub fn current_scope_index(&self) -> usize {
        self.current_scope
    }

    // ── 符号插入与查找 ──

    /// 在当前作用域插入符号。若已存在则返回 Err(原名)
    pub fn insert(&mut self, sym: Symbol) -> Result<(), String> {
        match &sym.kind {
            // 函数允许同名（重载）；仅当签名（参数类型序列）完全相同时视为重复定义
            SymbolKind::Function { params, .. } => {
                if let Some(vec) = self.functions.get(&sym.name) {
                    let dup = vec.iter().any(|existing| match &existing.kind {
                        SymbolKind::Function {
                            params: ep, ..
                        } => {
                            ep.len() == params.len()
                                && ep.iter().zip(params).all(|(a, b)| a.ty == b.ty)
                        }
                        _ => false,
                    });
                    if dup {
                        return Err(sym.name.clone());
                    }
                }
                self.functions.entry(sym.name.clone()).or_default().push(sym.clone());
                Ok(())
            }
            _ => {
                if self.scopes[self.current_scope].symbols.contains_key(&sym.name) {
                    return Err(sym.name.clone());
                }
                // 同步到全局索引（typed）
                if let SymbolKind::Type = &sym.kind {
                    // 类型存入 types 索引（types 索引全局唯一）
                    if self.current_scope == 0 {
                        self.types.insert(sym.name.clone(), Ty::Named(sym.name.clone()));
                    }
                }
                self.scopes[self.current_scope].symbols.insert(sym.name.clone(), sym);
                Ok(())
            }
        }
    }

    /// 在当前作用域直接定义变量符号（不检查重名）
    pub fn define_var(&mut self, name: impl Into<String>, ty: Ty, mutable: bool, span: Span) {
        let s = Symbol::new(
            name,
            SymbolKind::Variable {
                is_mutable: mutable,
            },
            Visibility::Private,
            span,
        );
        let _ = self.insert(s);
        // 记录类型信息：变量符号本身携带类型信息通过 Ty::Named 不够精确，
        // 因此这里把变量类型记录到 special 字典中由 Checker 维护。
        let _ = ty;
    }

    /// 在当前作用域插入函数符号
    pub fn insert_function(
        &mut self,
        name: impl Into<String>,
        params: Vec<ParamSym>,
        return_type: Ty,
        visibility: Visibility,
        span: Span,
    ) -> Result<(), String> {
        let sym = Symbol::new(
            name,
            SymbolKind::Function {
                params,
                return_type,
            },
            visibility,
            span,
        );
        self.insert(sym)
    }

    /// 注册模块到符号表（用于 `import aura.math`）
    ///
    /// 调用时用 `aura.math.sin(...)` 形式。
    pub fn insert_module(&mut self, module_path: impl Into<String>) {
        let path = module_path.into();
        let sym = Symbol::new(
            path.clone(),
            SymbolKind::Module,
            Visibility::Public,
            Span::single(0, 0, 0),
        );
        let _ = self.insert(sym);
    }

    /// 注册模块别名（用于 `import aura.math as m`）
    ///
    /// 调用时用 `m.sin(...)` 形式。
    pub fn insert_module_alias(
        &mut self,
        alias: impl Into<String>,
        _module_path: impl Into<String>,
    ) {
        let alias_name = alias.into();
        let sym = Symbol::new(
            alias_name.clone(),
            SymbolKind::Module,
            Visibility::Public,
            Span::single(0, 0, 0),
        );
        let _ = self.insert(sym);
    }

    /// 查找符号（从当前作用域向上）
    pub fn lookup(&self, name: &str) -> Option<&Symbol> {
        let mut scope = self.current_scope;
        loop {
            if let Some(sym) = self.scopes[scope].symbols.get(name) {
                return Some(sym);
            }
            match self.scopes[scope].parent {
                Some(p) => scope = p,
                None => return None,
            }
        }
    }

    /// 全局类型查找
    pub fn lookup_type(&self, name: &str) -> Option<&Ty> {
        self.types.get(name)
    }

    /// 全局函数查找（返回所有同名函数，供重载解析）
    pub fn lookup_function(&self, name: &str) -> Option<&Vec<Symbol>> {
        self.functions.get(name)
    }

    /// 记录一个类型定义到全局索引
    pub fn register_type(&mut self, name: impl Into<String>, ty: Ty) {
        self.types.insert(name.into(), ty);
    }

    /// 检查当前作用域内是否已定义（避免重复定义）
    pub fn is_defined_in_current_scope(&self, name: &str) -> bool {
        self.scopes[self.current_scope].symbols.contains_key(name)
    }

    pub fn scope_depth(&self) -> usize {
        let mut depth = 1;
        let mut scope = self.current_scope;
        while let Some(p) = self.scopes[scope].parent {
            scope = p;
            depth += 1;
        }
        depth
    }

    pub fn dump(&self) -> String {
        let mut out = String::new();
        for (i, scope) in self.scopes.iter().enumerate() {
            out.push_str(&format!(
                "scope[{}] (parent={:?}, fn_scope={}):\n",
                i, scope.parent, scope.is_function_scope
            ));
            let mut names: Vec<&String> = scope.symbols.keys().collect();
            names.sort();
            for n in names {
                out.push_str(&format!("  {}\n", n));
            }
        }
        out
    }
}

impl Default for SymbolTable {
    fn default() -> Self {
        Self::new()
    }
}

/// 便捷：从 AST 类型得到 Ty（供 Checker 使用）
pub fn ast_type_to_ty(ty: &Type) -> Ty {
    Ty::from_ast(ty)
}
