//! 语法分析器 — 将 Token 流转换为 AST
//!
//! 采用手写递归下降 + Pratt 优先级解析算法。

pub use crate::ast::*;
pub use crate::errors::{CompileError, ErrorSeverity};
pub use crate::span::Span;
pub use crate::token::{Token, TokenKind};

/// 语法分析器
pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    errors: Vec<CompileError>,
    /// 最近收集的文档注释，由随后的声明取走（KDoc：`///` / `/** */`）
    pending_doc: Option<String>,
}

impl Parser {
    /// 从 Token 列表创建 Parser
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            errors: Vec::new(),
            pending_doc: None,
        }
    }

    /// 返回所有错误
    pub fn errors(&self) -> &[CompileError] {
        &self.errors
    }

    /// 解析完整程序
    pub fn parse_program(&mut self) -> Program {
        let mut imports = Vec::new();
        let mut declarations = Vec::new();
        let mut top_level_statements = Vec::new();

        while !self.is_at_end() {
            match self.current().kind {
                TokenKind::Import => {
                    imports.push(self.parse_import());
                }
                TokenKind::EOF => break,
                _ => {
                    if let Ok(decl) = self.parse_declaration() {
                        declarations.push(decl);
                    } else if self.current().kind == TokenKind::Error {
                        // 消费错误 Token，继续解析
                        self.advance();
                    } else {
                        // 顶层语句（脚本模式）：表达式语句、val/var/lateinit 声明等
                        let stmt = self.parse_statement();
                        top_level_statements.push(stmt);
                    }
                }
            }
        }

        Program {
            imports,
            declarations,
            top_level_statements,
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Token 读取工具
    // ─────────────────────────────────────────────────────────────────────────

    fn current(&self) -> &Token {
        if self.pos < self.tokens.len() {
            &self.tokens[self.pos]
        } else {
            static EOF_TOKEN: Token = Token {
                kind: TokenKind::EOF,
                literal: String::new(),
                span: Span {
                    start: 0,
                    end: 0,
                    start_line: 1,
                    start_col: 1,
                    end_line: 1,
                    end_col: 1,
                },
            };
            &EOF_TOKEN
        }
    }

    #[allow(dead_code)]
    fn current_kind(&self) -> TokenKind {
        self.current().kind
    }

    fn peek(&self, offset: usize) -> TokenKind {
        let idx = self.pos + offset;
        if idx < self.tokens.len() {
            self.tokens[idx].kind
        } else {
            TokenKind::EOF
        }
    }

    /// 返回当前 token + offset 位置的完整 Token（用于 lookahead）
    fn peek_ahead(&self, offset: usize) -> Token {
        let idx = self.pos + offset;
        if idx < self.tokens.len() {
            self.tokens[idx].clone()
        } else {
            Token::new(TokenKind::EOF, "", Span::single(0, 1, 1))
        }
    }

    fn advance(&mut self) -> Token {
        if self.pos < self.tokens.len() {
            let tok = self.tokens[self.pos].clone();
            self.pos += 1;
            tok
        } else {
            Token::new(TokenKind::EOF, "", Span::single(0, 1, 1))
        }
    }

    fn is_at_end(&self) -> bool {
        self.pos >= self.tokens.len() || self.current().kind == TokenKind::EOF
    }

    fn check(&self, kind: TokenKind) -> bool {
        self.current().kind == kind
    }

    /// 收集紧邻的一条或多条文档注释（`///` 行或 `/** */` 块），合并为多行文本
    fn collect_doc(&mut self) {
        let mut lines: Vec<String> = Vec::new();
        while self.check(TokenKind::DocComment) {
            lines.push(self.advance().literal.clone());
        }
        if !lines.is_empty() {
            self.pending_doc = Some(lines.join("\n"));
        }
    }

    /// 取走当前挂起的文档注释
    fn take_doc(&mut self) -> Option<String> {
        self.pending_doc.take()
    }

    #[allow(dead_code)]
    fn consume(&mut self, expected: TokenKind) -> Result<Token, ()> {
        if self.current().kind == expected {
            Ok(self.advance())
        } else {
            Err(())
        }
    }

    fn expect(&mut self, expected: TokenKind) -> Token {
        if self.current().kind == expected {
            self.advance()
        } else {
            let span = self.current().span;
            self.errors.push(CompileError::spanned(
                format!("Expected {:?}, got {:?}", expected, self.current().kind),
                span,
            ));
            self.advance()
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    #[allow(dead_code)]
    pub fn parse_declaration(&mut self) -> Result<Decl, ()> {
        // 声明前可能带有文档注释（/// 或 /** */）
        self.collect_doc();

        if self.check(TokenKind::Import) {
            return Ok(Decl::Import(self.parse_import()));
        }
        if self.check(TokenKind::Extern) {
            return Ok(Decl::Extern(self.parse_extern()));
        }
        if self.check(TokenKind::At) {
            return Ok(Decl::Annotation(self.parse_annotation()));
        }
        if self.check(TokenKind::Typealias) {
            return Ok(Decl::TypeAlias(self.parse_type_alias()));
        }
        // struct / data struct / sealed struct
        if self.check(TokenKind::Struct) {
            return Ok(Decl::Struct(self.parse_struct()));
        }
        if self.check(TokenKind::Data) && self.peek_ahead(1).kind == TokenKind::Struct {
            return Ok(Decl::Struct(self.parse_data_struct()));
        }
        if self.check(TokenKind::Sealed) && self.peek_ahead(1).kind == TokenKind::Struct {
            return Ok(Decl::Struct(self.parse_sealed_struct()));
        }
        if self.check(TokenKind::Enum) {
            return Ok(Decl::Enum(self.parse_enum()));
        }
        // class / data class / sealed class
        if self.check(TokenKind::Class) {
            return Ok(Decl::Class(self.parse_class()));
        }
        if self.check(TokenKind::Data) && self.peek_ahead(1).kind == TokenKind::Class {
            return Ok(Decl::Class(self.parse_data_class()));
        }
        if self.check(TokenKind::Sealed) && self.peek_ahead(1).kind == TokenKind::Class {
            return Ok(Decl::Class(self.parse_sealed_class()));
        }
        // interface
        if self.check(TokenKind::Interface) {
            return Ok(Decl::Interface(self.parse_interface()));
        }
        if self.check(TokenKind::Actor) {
            return Ok(Decl::Actor(self.parse_actor()));
        }
        if self.check(TokenKind::Fun) || self.is_method_modifier_token() {
            return Ok(Decl::Function(self.parse_fn_decl()));
        }
        // 未知声明类型
        Err(())
    }

    fn parse_import(&mut self) -> ImportDecl {
        let start = self.current().span;
        self.advance(); // import

        // import "path/to/module" as alias
        if self.check(TokenKind::StringLiteral) {
            let path = self.advance().literal.clone();
            let alias = if self.check(TokenKind::As) {
                self.advance();
                Some(self.advance().literal.clone())
            } else {
                None
            };
            return ImportDecl {
                path,
                alias,
                wildcard: false,
                span: Span::merge(&start, &self.current().span),
            };
        }

        // import std.io.println or import std.io.*
        let mut path = String::new();
        path.push_str(&self.advance().literal);
        while self.check(TokenKind::Dot) {
            self.advance();
            path.push('.');
            let tok = self.advance();
            path.push_str(&tok.literal);
        }
        let wildcard = self.check(TokenKind::Star);
        if wildcard {
            self.advance();
        }
        let alias = if self.check(TokenKind::As) {
            self.advance();
            Some(self.advance().literal.clone())
        } else {
            None
        };

        ImportDecl {
            path,
            alias,
            wildcard,
            span: Span::merge(&start, &self.current().span),
        }
    }

    fn parse_annotation(&mut self) -> AnnotationDecl {
        let start = self.current().span;
        self.advance(); // @

        let name = self.advance().literal.clone();
        let mut args = Vec::new();

        if self.check(TokenKind::LParen) {
            self.advance(); // (
            while !self.check(TokenKind::RParen) && !self.is_at_end() {
                args.push(self.parse_expression(0));
                if !self.check(TokenKind::Comma) {
                    break;
                }
                self.advance();
            }
            self.expect(TokenKind::RParen);
        }

        AnnotationDecl {
            name,
            args,
            span: Span::merge(&start, &self.current().span),
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    #[allow(dead_code)]
    pub fn parse_fn_decl(&mut self) -> FnDecl {
        let start = self.current().span;

        let visibility = self.try_parse_visibility();
        let modifiers = self.try_parse_fn_modifiers();
        self.expect(TokenKind::Fun);

        // Kotlin 风格泛型函数：`fun <T : Bound> name(...)`（类型参数在函数名前）
        let mut type_params = if self.check(TokenKind::Lt) {
            self.try_parse_type_params()
        } else {
            Vec::new()
        };
        let name = self.advance().literal.clone();
        // 兼容 `fun name<T>(...)` 的写法（类型参数在名字后）
        if self.check(TokenKind::Lt) {
            type_params.extend(self.try_parse_type_params());
        }
        let params = self.parse_params();

        let return_type = if self.check(TokenKind::Colon) {
            self.advance();
            Some(Box::new(self.parse_type()))
        } else if self.check(TokenKind::Arrow) {
            self.advance();
            Some(Box::new(self.parse_type()))
        } else {
            None
        };

        let body = if self.check(TokenKind::LBrace) {
            Some(Box::new(self.parse_block()))
        } else if self.check(TokenKind::Assign) {
            self.advance();
            Some(Box::new(self.parse_expression(0)))
        } else {
            None
        };

        FnDecl {
            visibility,
            modifiers,
            name,
            type_params,
            params,
            return_type,
            body,
            doc: self.take_doc(),
            span: Span::merge(&start, &self.current().span),
        }
    }

    fn parse_params(&mut self) -> Vec<Param> {
        self.expect(TokenKind::LParen);
        let mut params = Vec::new();

        if self.check(TokenKind::RParen) {
            self.advance();
            return params;
        }

        loop {
            params.push(self.parse_param());
            if !self.check(TokenKind::Comma) {
                break;
            }
            self.advance();
        }

        self.expect(TokenKind::RParen);
        params
    }

    fn parse_param(&mut self) -> Param {
        let start = self.current().span;
        let is_vararg = self.check(TokenKind::TripleDotOp)
            || (self.current().kind == TokenKind::Ident && self.current().literal == "vararg");
        if is_vararg {
            self.advance();
        }

        let name = self.advance().literal.clone();
        let type_hint = if self.check(TokenKind::Colon) {
            self.advance();
            Some(Box::new(self.parse_type()))
        } else {
            None
        };

        let default_value = if self.check(TokenKind::Assign) {
            self.advance();
            Some(Box::new(self.parse_expression(0)))
        } else {
            None
        };

        Param {
            name,
            type_hint,
            default_value,
            is_vararg,
            span: Span::merge(&start, &self.current().span),
        }
    }

    /// 消费类型参数/实参的闭合 `>`。
    ///
    /// 由于 lexer 会把连续的 `>>` / `>>>` 识别为移位运算符 token（GtGt / GtGtGt），
    /// 而类型中合法的相邻闭合（如 `fun <T : Comparable<T>> max`）也必须被接受，
    /// 因此这里按「每次拆出一个 `>`，剩余留给下一次闭合」的方式分裂消费。
    fn expect_type_gt(&mut self) {
        match self.current().kind {
            TokenKind::Gt => {
                self.advance();
            }
            TokenKind::GtGt => {
                let t = self.advance();
                // 将 ">>" 拆成 ">"（本次闭合） + 合成 ">"（留给外层闭合）
                self.tokens
                    .insert(self.pos, Token::new(TokenKind::Gt, ">", t.span));
            }
            TokenKind::GtGtGt => {
                let t = self.advance();
                // 将 ">>>" 拆成 ">"（本次闭合） + 合成 ">>"（留给外层继续分裂）
                self.tokens
                    .insert(self.pos, Token::new(TokenKind::GtGt, ">>", t.span));
            }
            _ => {
                self.expect(TokenKind::Gt);
            }
        }
    }

    fn parse_type(&mut self) -> Type {
        let start = self.current().span;

        // 函数类型：(A, B) -> R（Kotlin 函数类型，P2.13）
        if self.check(TokenKind::LParen) {
            self.advance();
            let mut params = Vec::new();
            if !self.check(TokenKind::RParen) {
                loop {
                    params.push(Param {
                        name: String::new(),
                        type_hint: Some(Box::new(self.parse_type())),
                        default_value: None,
                        is_vararg: false,
                        span: self.current().span,
                    });
                    if self.check(TokenKind::Comma) {
                        self.advance();
                        continue;
                    }
                    break;
                }
            }
            self.expect(TokenKind::RParen);
            if self.check(TokenKind::Arrow) {
                self.advance();
                let ret = self.parse_type();
                return Type::Function {
                    params,
                    return_type: Some(Box::new(ret)),
                    span: Span::merge(&start, &self.current().span),
                };
            }
            // 纯括号分组类型：返回内部唯一类型
            return params
                .into_iter()
                .next()
                .and_then(|p| p.type_hint)
                .map(|b| *b)
                .unwrap_or(Type::Any);
        }

        let name = self.advance().literal.clone();

        // 类型参数
        let ty = if self.check(TokenKind::Lt) {
            self.advance(); // <
            let mut args = Vec::new();
            loop {
                // 星投影：List<*>
                if self.check(TokenKind::Star) {
                    let span = self.advance().span;
                    args.push(Type::StarProjection { span });
                } else {
                    args.push(self.parse_type());
                }
                if self.check(TokenKind::Comma) {
                    self.advance();
                    continue;
                }
                break;
            }
            self.expect_type_gt();
            // Fix 4: Pointer<T> 应解析为 Type::Pointer 而非 Type::Generic
            if name == "Pointer" && args.len() == 1 {
                let span = Span::merge(&start, &self.current().span);
                Type::Pointer(Box::new(args.into_iter().next().unwrap()))
            } else {
                Type::Generic {
                    name,
                    args,
                    span: Span::merge(&start, &self.current().span),
                }
            }
        } else {
            Type::Named {
                name,
                span: Span::merge(&start, &self.current().span),
            }
        };

        // 可空类型
        if self.check(TokenKind::QuestionMark) {
            self.advance();
            return Type::Nullable(Box::new(ty));
        }

        ty
    }

    fn try_parse_visibility(&mut self) -> Visibility {
        if self.check(TokenKind::Public) {
            self.advance();
            return Visibility::Public;
        }
        if self.check(TokenKind::Protected) {
            self.advance();
            return Visibility::Protected;
        }
        if self.check(TokenKind::Private) {
            self.advance();
            return Visibility::Private;
        }
        // Kotlin 语义：未显式标注时默认 public
        Visibility::Public
    }

    fn try_parse_fn_modifiers(&mut self) -> Vec<FnModifier> {
        let mut mods = Vec::new();
        if self.check(TokenKind::Suspend) {
            self.advance();
            mods.push(FnModifier::Suspend);
        }
        if self.check(TokenKind::Async) {
            self.advance();
            mods.push(FnModifier::Async);
        }
        if self.check(TokenKind::Inline) {
            self.advance();
            mods.push(FnModifier::Inline);
        }
        if self.check(TokenKind::Override) {
            self.advance();
            mods.push(FnModifier::Override);
        }
        if self.check(TokenKind::Comptime) {
            self.advance();
            mods.push(FnModifier::Comptime);
        }
        mods
    }

    /// 当前 token 是否为函数修饰符（可出现在 `fun` 之前，如 `override fun` / `suspend fun` / `async fun`）
    fn is_method_modifier_token(&self) -> bool {
        self.check(TokenKind::Override)
            || self.check(TokenKind::Suspend)
            || self.check(TokenKind::Async)
            || self.check(TokenKind::Inline)
            || self.check(TokenKind::Comptime)
    }

    /// 当前 token 是否为可见性关键字（`public` / `private` / `protected`）
    fn is_visibility_token(&self) -> bool {
        matches!(
            self.current().kind,
            TokenKind::Public | TokenKind::Private | TokenKind::Protected
        )
    }

    /// 解析类/结构体/接口体中的一个成员（字段或方法），正确处理前导可见性 / 修饰符
    fn parse_class_member(&mut self, fields: &mut Vec<StructField>, methods: &mut Vec<FnDecl>) {
        self.collect_doc();
        if self.check(TokenKind::Val) || self.check(TokenKind::Var) {
            fields.push(self.parse_struct_field());
            return;
        }
        if self.check(TokenKind::Fun) || self.is_method_modifier_token() {
            methods.push(self.parse_fn_decl());
            return;
        }
        if self.is_visibility_token() {
            // 可见性后跟随 val/var（字段）或 fun（方法）
            let nxt = self.peek_ahead(1).kind;
            if nxt == TokenKind::Val || nxt == TokenKind::Var {
                fields.push(self.parse_struct_field());
            } else {
                methods.push(self.parse_fn_decl());
            }
            return;
        }
        self.advance();
    }

    fn try_parse_type_params(&mut self) -> Vec<TypeParam> {
        if self.check(TokenKind::Lt) {
            self.advance(); // <
            let mut params = Vec::new();
            params.push(self.parse_type_param());
            while self.check(TokenKind::Comma) {
                self.advance();
                params.push(self.parse_type_param());
            }
            self.expect_type_gt();
            params
        } else {
            Vec::new()
        }
    }

    fn parse_type_param(&mut self) -> TypeParam {
        let start = self.current().span;

        // 型变修饰符：`out T` / `in T`（Kotlin 声明处型变）
        let variance = if self.check(TokenKind::Ident)
            && self.current().literal == "out"
            && self.peek_ahead(1).kind == TokenKind::Ident
        {
            self.advance();
            TypeVariance::Out
        } else if self.check(TokenKind::In) && self.peek_ahead(1).kind == TokenKind::Ident {
            self.advance();
            TypeVariance::In
        } else {
            TypeVariance::Invariant
        };

        let name = self.advance().literal.clone();

        // 泛型约束：T : B 或 T : B1 : B2（B 可能是泛型类型 Comparable<T>）
        let bounds = if self.check(TokenKind::Colon) {
            self.advance();
            let mut bounds: Vec<Type> = Vec::new();
            bounds.push(self.parse_type());
            while self.check(TokenKind::Colon) {
                self.advance();
                bounds.push(self.parse_type());
            }
            bounds
        } else {
            Vec::new()
        };

        TypeParam {
            name,
            variance,
            bounds,
            default: None,
            span: Span::merge(&start, &self.current().span),
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    #[allow(dead_code)]
    pub fn parse_struct(&mut self) -> StructDecl {
        let start = self.current().span;
        let visibility = self.try_parse_visibility();

        let sealed = self.check(TokenKind::Sealed);
        if sealed {
            self.advance();
        }

        self.expect(TokenKind::Struct);
        let name = self.advance().literal.clone();
        let type_params = self.try_parse_type_params();

        let mut fields = Vec::new();
        let mut methods = Vec::new();
        let mut implementations = Vec::new();

        if self.check(TokenKind::LBrace) {
            self.advance();
            while !self.check(TokenKind::RBrace) && !self.is_at_end() {
                // 成员前可能带有文档注释
                self.collect_doc();
                self.parse_class_member(&mut fields, &mut methods);
            }
            self.expect(TokenKind::RBrace);
        } else if self.check(TokenKind::LParen) {
            // 数据类语法：struct Point(val x: Float, val y: Float)
            self.advance();
            while !self.check(TokenKind::RParen) && !self.is_at_end() {
                if self.check(TokenKind::Val) || self.check(TokenKind::Var) {
                    fields.push(self.parse_struct_field());
                } else {
                    fields.push(StructField {
                        visibility: self.try_parse_visibility(),
                        is_mutable: false,
                        name: self.advance().literal.clone(),
                        type_hint: None,
                        default_value: None,
                        span: Span::single(0, 1, 1),
                    });
                }
                if !self.check(TokenKind::Comma) {
                    break;
                }
                self.advance();
            }
            self.expect(TokenKind::RParen);
        }

        // 接口实现：`:: TraitName` 或 Kotlin 风格 `: TraitName`
        if self.check(TokenKind::DoubleColon) || self.check(TokenKind::Colon) {
            self.advance();
            implementations.push(self.advance().literal.clone());
        }

        StructDecl {
            visibility,
            sealed,
            name,
            type_params,
            fields,
            methods,
            implementations,
            doc: self.take_doc(),
            span: Span::merge(&start, &self.current().span),
        }
    }

    fn parse_struct_field(&mut self) -> StructField {
        let start = self.current().span;
        let visibility = self.try_parse_visibility();

        let mut is_mutable = false;
        if self.check(TokenKind::Var) {
            self.advance();
            is_mutable = true;
        } else if self.check(TokenKind::Val) {
            self.advance();
        }

        let name = self.advance().literal.clone();
        let type_hint = if self.check(TokenKind::Colon) {
            self.advance();
            Some(Box::new(self.parse_type()))
        } else {
            None
        };

        let default_value = if self.check(TokenKind::Assign) {
            self.advance();
            Some(Box::new(self.parse_expression(0)))
        } else {
            None
        };

        StructField {
            visibility,
            is_mutable,
            name,
            type_hint,
            default_value,
            span: Span::merge(&start, &self.current().span),
        }
    }

    #[allow(dead_code)]
    pub fn parse_enum(&mut self) -> EnumDecl {
        let start = self.current().span;
        let visibility = self.try_parse_visibility();
        self.expect(TokenKind::Enum);
        let name = self.advance().literal.clone();

        let mut variants = Vec::new();
        self.expect(TokenKind::LBrace);
        while !self.check(TokenKind::RBrace) && !self.is_at_end() {
            let vname = self.advance().literal.clone();
            let fields = if self.check(TokenKind::LParen) {
                self.advance();
                let mut params = Vec::new();
                while !self.check(TokenKind::RParen) && !self.is_at_end() {
                    // CUSTOM(val r: Int, val g: Int, val b: Int)
                    if self.check(TokenKind::Val) || self.check(TokenKind::Var) {
                        self.advance();
                    }
                    params.push(self.parse_param());
                    if !self.check(TokenKind::Comma) {
                        break;
                    }
                    self.advance();
                }
                self.expect(TokenKind::RParen);
                params
            } else {
                Vec::new()
            };
            variants.push(EnumVariant {
                name: vname,
                fields,
                span: Span::merge(&start, &self.current().span),
            });
            if !self.check(TokenKind::Comma) {
                break;
            }
            self.advance();
        }
        self.expect(TokenKind::RBrace);

        EnumDecl {
            visibility,
            name,
            variants,
            doc: self.take_doc(),
            span: Span::merge(&start, &self.current().span),
        }
    }

    /// 解析父类 / 接口引用：`Name`、`Name<T, U>`、`Name(args)`
    ///
    /// 同时接受 Aura 既有的 `::` 与 Kotlin 风格的 `:`（技术方案 §3.4 使用单冒号）。
    /// 泛型实参与构造实参仅做跳过（当前阶段不参与语义检查）。
    fn parse_superclass_ref(&mut self) -> Option<String> {
        if !(self.check(TokenKind::DoubleColon) || self.check(TokenKind::Colon)) {
            return None;
        }
        self.advance();
        let name = self.advance().literal.clone();

        // 泛型实参：Result<T, Nothing>
        if self.check(TokenKind::Lt) {
            self.skip_balanced(
                TokenKind::Lt,
                &[TokenKind::Gt, TokenKind::GtGt, TokenKind::GtGtGt],
            );
        }
        // 父类构造调用实参：Base(1, 2)
        if self.check(TokenKind::LParen) {
            self.skip_balanced(TokenKind::LParen, &[TokenKind::RParen]);
        }

        Some(name)
    }

    /// 从当前 token（开括号）起跳过与之配对的括号区间
    fn skip_balanced(&mut self, open: TokenKind, closes: &[TokenKind]) {
        self.advance(); // 消费开括号
        let mut depth = 1usize;
        while depth > 0 && !self.is_at_end() {
            let kind = self.current().kind;
            if kind == open {
                depth += 1;
            } else if closes.contains(&kind) {
                // `>>` / `>>>` 会同时闭合多层
                let n = match kind {
                    TokenKind::GtGt => 2,
                    TokenKind::GtGtGt => 3,
                    _ => 1,
                };
                depth = depth.saturating_sub(n);
            }
            self.advance();
        }
    }

    #[allow(dead_code)]
    pub fn parse_class(&mut self) -> ClassDecl {
        let start = self.current().span;
        let visibility = self.try_parse_visibility();
        self.expect(TokenKind::Class);
        let name = self.advance().literal.clone();
        let type_params = self.try_parse_type_params();

        let superclass = self.parse_superclass_ref();

        let mut fields = Vec::new();
        let mut methods = Vec::new();
        let mut implementations = Vec::new();

        if self.check(TokenKind::LBrace) {
            self.advance();
            while !self.check(TokenKind::RBrace) && !self.is_at_end() {
                // 成员前可能带有文档注释
                self.collect_doc();
                self.parse_class_member(&mut fields, &mut methods);
            }
            self.expect(TokenKind::RBrace);
        }

        // Kotlin 风格：`: Base(), Trait1, Trait2`（父类之后的接口列表）
        if superclass.is_some() {
            while self.check(TokenKind::Comma) {
                self.advance();
                implementations.push(self.advance().literal.clone());
            }
        }

        // 接口实现：`:: TraitName` 或 Kotlin 风格 `: TraitName`（在 body 之后）
        if self.check(TokenKind::DoubleColon) || self.check(TokenKind::Colon) {
            self.advance();
            implementations.push(self.advance().literal.clone());
        }

        ClassDecl {
            visibility,
            sealed: false,
            name,
            type_params,
            superclass,
            fields,
            methods,
            implementations,
            doc: self.take_doc(),
            span: Span::merge(&start, &self.current().span),
        }
    }

    /// 解析 `data struct Name { ... }`
    #[allow(dead_code)]
    pub fn parse_data_struct(&mut self) -> StructDecl {
        let start = self.current().span;
        let visibility = self.try_parse_visibility();
        self.expect(TokenKind::Data);
        self.expect(TokenKind::Struct);
        let name = self.advance().literal.clone();
        let type_params = self.try_parse_type_params();

        let mut fields = Vec::new();
        let mut methods = Vec::new();

        if self.check(TokenKind::LBrace) {
            self.advance();
            while !self.check(TokenKind::RBrace) && !self.is_at_end() {
                // 成员前可能带有文档注释
                self.collect_doc();
                self.parse_class_member(&mut fields, &mut methods);
            }
            self.expect(TokenKind::RBrace);
        } else if self.check(TokenKind::LParen) {
            self.advance();
            while !self.check(TokenKind::RParen) && !self.is_at_end() {
                let f = self.parse_struct_field();
                fields.push(f);
                if !self.check(TokenKind::Comma) {
                    break;
                }
                self.advance();
            }
            self.expect(TokenKind::RParen);
        }

        StructDecl {
            visibility,
            sealed: false,
            name,
            type_params,
            fields,
            methods,
            implementations: Vec::new(),
            doc: self.take_doc(),
            span: Span::merge(&start, &self.current().span),
        }
    }

    /// 解析 `sealed struct Name { ... }`
    #[allow(dead_code)]
    pub fn parse_sealed_struct(&mut self) -> StructDecl {
        let start = self.current().span;
        let visibility = self.try_parse_visibility();
        self.expect(TokenKind::Sealed);
        self.expect(TokenKind::Struct);
        let name = self.advance().literal.clone();
        let type_params = self.try_parse_type_params();

        let mut fields = Vec::new();
        let mut methods = Vec::new();

        if self.check(TokenKind::LBrace) {
            self.advance();
            while !self.check(TokenKind::RBrace) && !self.is_at_end() {
                // 成员前可能带有文档注释
                self.collect_doc();
                self.parse_class_member(&mut fields, &mut methods);
            }
            self.expect(TokenKind::RBrace);
        }

        StructDecl {
            visibility,
            sealed: true,
            name,
            type_params,
            fields,
            methods,
            implementations: Vec::new(),
            doc: self.take_doc(),
            span: Span::merge(&start, &self.current().span),
        }
    }

    /// 解析 `data class Name { ... }` 或 `data class Name(ctor) { ... }`
    #[allow(dead_code)]
    pub fn parse_data_class(&mut self) -> ClassDecl {
        let start = self.current().span;
        let visibility = self.try_parse_visibility();
        self.expect(TokenKind::Data);
        self.expect(TokenKind::Class);
        let name = self.advance().literal.clone();
        let type_params = self.try_parse_type_params();

        let mut superclass = self.parse_superclass_ref();

        let mut fields = Vec::new();
        let mut methods = Vec::new();
        let mut implementations = Vec::new();

        // 主构造器 data class Foo(val x: Int, val y: Int) { ... }
        if self.check(TokenKind::LParen) {
            self.advance();
            while !self.check(TokenKind::RParen) && !self.is_at_end() {
                let f = self.parse_struct_field();
                fields.push(f);
                if !self.check(TokenKind::Comma) {
                    break;
                }
                self.advance();
            }
            self.expect(TokenKind::RParen);
        }

        // Kotlin 风格：主构造器之后再写 `: Base(...)`
        if superclass.is_none() {
            superclass = self.parse_superclass_ref();
        }

        // Kotlin 风格：`: Base(), Trait1, Trait2`
        if superclass.is_some() {
            while self.check(TokenKind::Comma) {
                self.advance();
                implementations.push(self.advance().literal.clone());
            }
        }

        // 接口实现：`:: TraitName` 或 Kotlin 风格 `: TraitName`
        if self.check(TokenKind::DoubleColon) || self.check(TokenKind::Colon) {
            self.advance();
            implementations.push(self.advance().literal.clone());
        }

        if self.check(TokenKind::LBrace) {
            self.advance();
            while !self.check(TokenKind::RBrace) && !self.is_at_end() {
                // 成员前可能带有文档注释
                self.collect_doc();
                self.parse_class_member(&mut fields, &mut methods);
            }
            self.expect(TokenKind::RBrace);
        }

        ClassDecl {
            visibility,
            sealed: false,
            name,
            type_params,
            superclass,
            fields,
            methods,
            implementations,
            doc: self.take_doc(),
            span: Span::merge(&start, &self.current().span),
        }
    }

    /// 解析 `sealed class Name { ... }`
    #[allow(dead_code)]
    pub fn parse_sealed_class(&mut self) -> ClassDecl {
        let start = self.current().span;
        let visibility = self.try_parse_visibility();
        self.expect(TokenKind::Sealed);
        self.expect(TokenKind::Class);
        let name = self.advance().literal.clone();
        let type_params = self.try_parse_type_params();

        let superclass = self.parse_superclass_ref();

        let mut fields = Vec::new();
        let mut methods = Vec::new();
        let mut implementations = Vec::new();

        // Kotlin 风格：`: Base(), Trait1, Trait2`（父类之后的接口列表）
        if superclass.is_some() {
            while self.check(TokenKind::Comma) {
                self.advance();
                implementations.push(self.advance().literal.clone());
            }
        }

        // 接口实现：`:: TraitName` 或 Kotlin 风格 `: TraitName`（在 body 之后）
        if self.check(TokenKind::DoubleColon) || self.check(TokenKind::Colon) {
            self.advance();
            implementations.push(self.advance().literal.clone());
        }

        if self.check(TokenKind::LBrace) {
            self.advance();
            while !self.check(TokenKind::RBrace) && !self.is_at_end() {
                // 成员前可能带有文档注释
                self.collect_doc();
                self.parse_class_member(&mut fields, &mut methods);
            }
            self.expect(TokenKind::RBrace);
        }

        ClassDecl {
            visibility,
            sealed: true,
            name,
            type_params,
            superclass,
            fields,
            methods,
            implementations,
            doc: self.take_doc(),
            span: Span::merge(&start, &self.current().span),
        }
    }

    #[allow(dead_code)]
    pub fn parse_interface(&mut self) -> InterfaceDecl {
        let start = self.current().span;
        let visibility = self.try_parse_visibility();
        self.expect(TokenKind::Interface);
        let name = self.advance().literal.clone();

        let mut methods = Vec::new();
        if self.check(TokenKind::LBrace) {
            self.advance();
            while !self.check(TokenKind::RBrace) && !self.is_at_end() {
                if self.check(TokenKind::Fun) || self.is_method_modifier_token() {
                    methods.push(self.parse_fn_decl());
                } else {
                    self.advance();
                }
            }
            self.expect(TokenKind::RBrace);
        }

        InterfaceDecl {
            visibility,
            name,
            type_params: Vec::new(),
            methods,
            doc: self.take_doc(),
            span: Span::merge(&start, &self.current().span),
        }
    }

    #[allow(dead_code)]
    pub fn parse_actor(&mut self) -> ActorDecl {
        let start = self.current().span;
        let visibility = self.try_parse_visibility();
        self.expect(TokenKind::Actor);
        let name = self.advance().literal.clone();

        let mut fields = Vec::new();
        let mut methods = Vec::new();

        if self.check(TokenKind::LBrace) {
            self.advance();
            while !self.check(TokenKind::RBrace) && !self.is_at_end() {
                // 成员前可能带有文档注释
                self.collect_doc();
                self.parse_class_member(&mut fields, &mut methods);
            }
            self.expect(TokenKind::RBrace);
        }

        ActorDecl {
            visibility,
            name,
            fields,
            methods,
            doc: self.take_doc(),
            span: Span::merge(&start, &self.current().span),
        }
    }

    #[allow(dead_code)]
    pub fn parse_type_alias(&mut self) -> TypeAliasDecl {
        let start = self.current().span;
        let visibility = self.try_parse_visibility();
        self.expect(TokenKind::Typealias);
        let name = self.advance().literal.clone();
        self.expect(TokenKind::Assign);
        let aliased_type = Box::new(self.parse_type());

        TypeAliasDecl {
            visibility,
            name,
            type_params: Vec::new(),
            aliased_type,
            doc: self.take_doc(),
            span: Span::merge(&start, &self.current().span),
        }
    }

    #[allow(dead_code)]
    pub fn parse_extern(&mut self) -> ExternDecl {
        let start = self.current().span;
        self.expect(TokenKind::Extern);

        let abi = if self.check(TokenKind::StringLiteral) {
            self.advance().literal.clone()
        } else {
            "c".to_string()
        };

        let library = if self.check(TokenKind::StringLiteral) {
            Some(self.advance().literal.clone())
        } else {
            None
        };

        let mut functions = Vec::new();
        let mut constants = Vec::new();
        if self.check(TokenKind::LBrace) {
            self.advance();
            while !self.check(TokenKind::RBrace) && !self.is_at_end() {
                if self.check(TokenKind::Fun) || self.is_method_modifier_token() {
                    functions.push(self.parse_fn_decl());
                } else if self.check(TokenKind::Val) || self.check(TokenKind::Var) {
                    // P8.1: 解析 extern 块内的常量声明
                    let stmt = if self.check(TokenKind::Val) {
                        self.parse_val_stmt()
                    } else {
                        self.parse_var_stmt()
                    };
                    constants.push(stmt);
                } else {
                    self.advance();
                }
            }
            self.expect(TokenKind::RBrace);
        }

        ExternDecl {
            abi,
            library,
            functions,
            constants,
            span: Span::merge(&start, &self.current().span),
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    #[allow(dead_code)]
    pub fn parse_block(&mut self) -> Expr {
        let start = self.current().span;
        self.expect(TokenKind::LBrace);
        let mut stmts = Vec::new();

        while !self.check(TokenKind::RBrace) && !self.is_at_end() {
            stmts.push(self.parse_statement());
        }

        self.expect(TokenKind::RBrace);
        Expr::Block(stmts, Span::merge(&start, &self.current().span))
    }

    fn parse_statement(&mut self) -> Stmt {
        if self.check(TokenKind::Val) {
            return self.parse_val_stmt();
        }
        if self.check(TokenKind::Var) {
            return self.parse_var_stmt();
        }
        if self.check(TokenKind::Lateinit) {
            return self.parse_lateinit_var();
        }
        if self.check(TokenKind::Return) {
            let start = self.advance().span;
            let value = if self.is_expression_terminator() {
                None
            } else {
                Some(Box::new(self.parse_expression(0)))
            };
            return Stmt::Expr(Expr::Return {
                value,
                span: Span::merge(&start, &self.current().span),
            });
        }
        if self.check(TokenKind::Break) {
            let t = self.advance();
            return Stmt::Expr(Expr::Break { span: t.span });
        }
        if self.check(TokenKind::Continue) {
            let t = self.advance();
            return Stmt::Expr(Expr::Continue { span: t.span });
        }
        if self.check(TokenKind::LBrace) {
            let blk = self.parse_block();
            return Stmt::Expr(blk);
        }
        // 表达式语句
        Stmt::Expr(self.parse_expression(0))
    }

    /// 判断当前 token 是否表示表达式结束（表达式语句无需继续解析）
    fn is_expression_terminator(&self) -> bool {
        matches!(
            self.current().kind,
            TokenKind::Semicolon
                | TokenKind::RBrace
                | TokenKind::RParen
                | TokenKind::EOF
                | TokenKind::Val
                | TokenKind::Var
                | TokenKind::Return
                | TokenKind::Fun
                | TokenKind::If
                | TokenKind::Else
                | TokenKind::For
                | TokenKind::While
                | TokenKind::Do
                | TokenKind::Try
        )
    }

    fn parse_val_stmt(&mut self) -> Stmt {
        let start = self.current().span;
        self.advance(); // val
        let name = self.advance().literal.clone();

        // val x by lazy { ... } 惰性初始化
        if self.check(TokenKind::Ident) && self.current().literal == "by" {
            self.advance(); // by
            self.expect(TokenKind::Lazy);
            let block = self.parse_expression(0);
            let type_hint = if self.check(TokenKind::Colon) {
                self.advance();
                Some(Box::new(self.parse_type()))
            } else {
                None
            };
            return Stmt::Val {
                name,
                type_hint,
                initializer: Some(Box::new(block)),
                span: Span::merge(&start, &self.current().span),
            };
        }

        // 解构：val (a, b) = expr
        if self.check(TokenKind::LParen) {
            self.advance();
            let mut patterns = Vec::new();
            while !self.check(TokenKind::RParen) && !self.is_at_end() {
                let p = self.parse_expression(0);
                patterns.push(p);
                if !self.check(TokenKind::Comma) {
                    break;
                }
                self.advance();
            }
            self.expect(TokenKind::RParen);
            self.expect(TokenKind::Assign);
            let initializer = self.parse_expression(0);
            let type_hint = if self.check(TokenKind::Colon) {
                self.advance();
                Some(Box::new(self.parse_type()))
            } else {
                None
            };
            return Stmt::Destructure {
                patterns,
                expr: Box::new(initializer),
                type_hint,
                span: Span::merge(&start, &self.current().span),
            };
        }

        let type_hint = if self.check(TokenKind::Colon) {
            self.advance();
            Some(Box::new(self.parse_type()))
        } else {
            None
        };

        let initializer = if self.check(TokenKind::Assign) {
            self.advance();
            Some(Box::new(self.parse_expression(0)))
        } else {
            None
        };

        Stmt::Val {
            name,
            type_hint,
            initializer,
            span: Span::merge(&start, &self.current().span),
        }
    }

    fn parse_var_stmt(&mut self) -> Stmt {
        let start = self.current().span;
        self.advance(); // var
        let name = self.advance().literal.clone();

        // lateinit var x: Type
        if name == "<lateinit>" {
            // 这里实际上 lateinit 已经作为独立 token 处理
        }

        let type_hint = if self.check(TokenKind::Colon) {
            self.advance();
            Some(Box::new(self.parse_type()))
        } else {
            None
        };

        let initializer = if self.check(TokenKind::Assign) {
            self.advance();
            Some(Box::new(self.parse_expression(0)))
        } else {
            None
        };

        Stmt::Var {
            name,
            type_hint,
            initializer,
            span: Span::merge(&start, &self.current().span),
        }
    }

    /// 解析 `lateinit var x: Type`
    fn parse_lateinit_var(&mut self) -> Stmt {
        let start = self.current().span;
        self.expect(TokenKind::Lateinit);
        self.expect(TokenKind::Var);
        let name = self.advance().literal.clone();
        let type_hint = if self.check(TokenKind::Colon) {
            self.advance();
            Some(Box::new(self.parse_type()))
        } else {
            None
        };
        Stmt::Var {
            name,
            type_hint,
            initializer: None,
            span: Span::merge(&start, &self.current().span),
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Pratt 优先级解析 — 表达式入口
    // ─────────────────────────────────────────────────────────────────────────

    #[allow(dead_code)]
    pub fn parse_expression(&mut self, min_bp: u8) -> Expr {
        let start = self.current().span;
        let mut lhs = self.parse_prefix_expression();

        // Pratt 循环：左结合运算符用 `bp > min_bp` 判断，RHS 以同优先级递归。
        // 非运算符 token（含 EOF、`)`、`}`、关键字）绑定优先级为 0，直接终止，
        // 杜绝 `advance` 在 EOF 处不推进导致的无限循环。
        loop {
            let next_bp = self.infix_binding_power();
            if next_bp == 0 || next_bp <= min_bp {
                break;
            }

            // 范围表达式：0..10 / 0..<10（inclusive / exclusive）
            if self.check(TokenKind::DoubleDotOp) {
                self.advance();
                let inclusive = !self.check(TokenKind::Lt);
                if !inclusive {
                    self.advance(); // 消费 <
                }
                let end = self.parse_expression(next_bp);
                let end = if inclusive {
                    end
                } else {
                    // 0..<10 → end = 10 减 1 的语义交给语义分析，这里保持原值
                    end
                };
                lhs = Expr::Range {
                    start: Some(Box::new(lhs)),
                    end: Some(Box::new(end)),
                    inclusive,
                    span: Span::merge(&start, &self.current().span),
                };
                continue;
            }

            // 复合赋值：x += y → x = x + y
            if let Some(cop) = self.compound_assign_op() {
                let rhs = self.parse_expression(next_bp - 1);
                let bin = Expr::Binary {
                    op: cop,
                    lhs: Box::new(lhs.clone()),
                    rhs: Box::new(rhs),
                    span: Span::merge(&start, &self.current().span),
                };
                lhs = Expr::Assign {
                    target: Box::new(lhs),
                    value: Box::new(bin),
                    span: Span::merge(&start, &self.current().span),
                };
                continue;
            }

            // `to` 操作符：key to value → Binary(To)
            if self.check(TokenKind::To) {
                self.advance();
                let rhs = self.parse_expression(next_bp);
                lhs = Expr::Binary {
                    op: BinOp::To,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                    span: Span::merge(&start, &self.current().span),
                };
                continue;
            }

            let op = self.parse_infix_operator();

            // 赋值是右结合：a = b = c 解析为 a = (b = c)
            if op == BinOp::Assign {
                let rhs = self.parse_expression(next_bp - 1);
                lhs = Expr::Assign {
                    target: Box::new(lhs),
                    value: Box::new(rhs),
                    span: Span::merge(&start, &self.current().span),
                };
                continue;
            }

            // 其余二元运算符左结合：rhs 用同优先级递归，保证结合性与优先级正确
            let rhs = self.parse_expression(next_bp);
            lhs = Expr::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
                span: Span::merge(&start, &self.current().span),
            };
        }

        lhs
    }

    fn parse_prefix_expression(&mut self) -> Expr {
        let start = self.current().span;

        // ── 一元运算符（必须先消费运算符，再解析操作数，否则无限递归）──
        if self.check(TokenKind::Bang) {
            self.advance();
            let operand = self.parse_prefix_expression();
            return Expr::Unary {
                op: UnOp::Not,
                operand: Box::new(operand),
                span: Span::merge(&start, &self.current().span),
            };
        }
        if self.check(TokenKind::Minus) {
            self.advance(); // 先消费 '-'
            let operand = self.parse_prefix_expression();
            return Expr::Unary {
                op: UnOp::Minus,
                operand: Box::new(operand),
                span: Span::merge(&start, &self.current().span),
            };
        }
        if self.check(TokenKind::DoublePlus) {
            self.advance();
            let operand = self.parse_prefix_expression();
            return Expr::Unary {
                op: UnOp::Increment,
                operand: Box::new(operand),
                span: Span::merge(&start, &self.current().span),
            };
        }
        if self.check(TokenKind::DoubleMinus) {
            self.advance();
            let operand = self.parse_prefix_expression();
            return Expr::Unary {
                op: UnOp::Decrement,
                operand: Box::new(operand),
                span: Span::merge(&start, &self.current().span),
            };
        }

        // ── 关键字表达式 ──
        if self.check(TokenKind::If) {
            return self.parse_if_expression(start);
        }
        if self.check(TokenKind::When) {
            return self.parse_when_expression(start);
        }
        if self.check(TokenKind::For) {
            return self.parse_for_expression(start);
        }
        if self.check(TokenKind::While) {
            return self.parse_while_expression(start);
        }
        if self.check(TokenKind::Do) {
            return self.parse_do_while_expression(start);
        }
        if self.check(TokenKind::Try) {
            return self.parse_try_expression(start);
        }
        if self.check(TokenKind::Throw) {
            self.advance();
            let value = self.parse_expression(0);
            return Expr::Throw {
                value: Box::new(value),
                span: Span::merge(&start, &self.current().span),
            };
        }
        if self.check(TokenKind::Await) {
            self.advance();
            let expr = self.parse_expression(0);
            return Expr::Await {
                expr: Box::new(expr),
                span: Span::merge(&start, &self.current().span),
            };
        }
        if self.check(TokenKind::LBrace) {
            return self.parse_block();
        }

        // 字面量
        if self.check(TokenKind::IntLiteral) {
            let tok = self.advance();
            if let Ok(n) = tok.literal.parse::<i64>() {
                return Expr::Literal(Literal::Int(n), tok.span);
            }
            return Expr::Literal(Literal::Int(0), tok.span);
        }
        if self.check(TokenKind::FloatLiteral) {
            let tok = self.advance();
            if let Ok(n) = tok
                .literal
                .trim_end_matches(['f', 'F', 'd', 'D'])
                .parse::<f64>()
            {
                return Expr::Literal(Literal::Float(n), tok.span);
            }
            return Expr::Literal(Literal::Float(0.0), tok.span);
        }
        if self.check(TokenKind::StringLiteral) {
            let tok = self.advance();
            return Expr::Literal(Literal::String(tok.literal.clone()), tok.span);
        }
        if self.check(TokenKind::CharLiteral) {
            let tok = self.advance();
            let ch = tok.literal.chars().next().unwrap_or('\0');
            return Expr::Literal(Literal::Char(ch), tok.span);
        }
        if self.check(TokenKind::BoolLiteral) {
            let tok = self.advance();
            let b = tok.literal == "true";
            return Expr::Literal(Literal::Bool(b), tok.span);
        }
        if self.check(TokenKind::Null) {
            self.advance();
            return Expr::Literal(Literal::Null, self.current().span);
        }

        // 括号表达式 / 调用 / Lambda
        if self.check(TokenKind::LParen) {
            self.advance();
            // 检查是否是 Lambda：param: Type -> Expr
            if self.is_lambda_start() {
                return self.parse_lambda();
            }
            let expr = self.parse_expression(0);
            self.expect(TokenKind::RParen);
            return expr;
        }

        // 标签 + 循环：outer@ for / outer@ while / outer@ do
        if self.current().kind == TokenKind::Ident && self.peek(1) == TokenKind::At {
            self.advance(); // 标签名
            self.advance(); // @
            return self.parse_expression(0);
        }

        // 标识符 / 关键字作为表达式
        let tok = self.advance();
        let mut expr = Expr::Ident(tok.literal.clone(), tok.span);

        // 后缀：调用、成员访问、索引
        loop {
            match self.current().kind {
                TokenKind::LParen => {
                    self.advance();
                    let mut args = Vec::new();
                    if !self.check(TokenKind::RParen) {
                        // 检查是否是命名参数：name = expr
                        if self.current().kind == TokenKind::Ident && self.peek(1) == TokenKind::Assign {
                            let name = self.advance().literal.clone();
                            self.advance(); // =
                            args.push(Expr::NamedArg {
                                name,
                                value: Box::new(self.parse_expression(0)),
                                span: self.current().span,
                            });
                        } else {
                            args.push(self.parse_expression(0));
                        }
                        while self.check(TokenKind::Comma) {
                            self.advance();
                            // 检查是否是命名参数：name = expr
                            if self.current().kind == TokenKind::Ident && self.peek(1) == TokenKind::Assign {
                                let name = self.advance().literal.clone();
                                self.advance(); // =
                                args.push(Expr::NamedArg {
                                    name,
                                    value: Box::new(self.parse_expression(0)),
                                    span: self.current().span,
                                });
                            } else {
                                args.push(self.parse_expression(0));
                            }
                        }
                    }
                    self.expect(TokenKind::RParen);
                    expr = Expr::Call {
                        callee: Box::new(expr),
                        args,
                        span: Span::merge(&start, &self.current().span),
                    };
                }
                TokenKind::Dot => {
                    self.advance();
                    let name = self.advance().literal.clone();
                    expr = Expr::MemberAccess {
                        object: Box::new(expr),
                        name,
                        span: Span::merge(&start, &self.current().span),
                    };
                }
                TokenKind::QuestionMark => {
                    self.advance();
                    if self.check(TokenKind::Colon) {
                        // Elvis：a ?: b
                        self.advance();
                        let rhs = self.parse_expression(0);
                        expr = Expr::Elvis {
                            lhs: Box::new(expr),
                            rhs: Box::new(rhs),
                            span: Span::merge(&start, &self.current().span),
                        };
                    } else {
                        // 安全调用 a?.b
                        if self.check(TokenKind::Dot) {
                            self.advance();
                        }
                        let name = self.advance().literal.clone();
                        expr = Expr::SafeAccess {
                            object: Box::new(expr),
                            name,
                            span: Span::merge(&start, &self.current().span),
                        };
                    }
                }
                TokenKind::LBracket => {
                    self.advance();
                    // 空索引 `Int[]` 作为数组构造器类型（后续可接调用）
                    if self.check(TokenKind::RBracket) {
                        self.advance();
                        // 不创建 Index 节点，保留原表达式（如 Int[]）
                    } else {
                        let index = self.parse_expression(0);
                        self.expect(TokenKind::RBracket);
                        expr = Expr::Index {
                            container: Box::new(expr),
                            index: Box::new(index),
                            span: Span::merge(&start, &self.current().span),
                        };
                    }
                }
                TokenKind::DoubleBang => {
                    // 非空断言：a!!
                    self.advance();
                    expr = Expr::Unary {
                        op: UnOp::NotNull,
                        operand: Box::new(expr),
                        span: Span::merge(&start, &self.current().span),
                    };
                }
                _ => break,
            }
        }

        expr
    }

    fn is_lambda_start(&self) -> bool {
        // 启发式：如果当前是标识符或 val/var，且后面跟冒号或箭头，则可能是 lambda
        if self.current().kind == TokenKind::Ident {
            let peek1 = self.peek(1);
            // 单参数 lambda: `x -> expr` 或 `x: Type -> expr`
            if peek1 == TokenKind::Colon || peek1 == TokenKind::Arrow {
                return true;
            }
            // 括号参数 lambda: `(x) -> expr` 或 `(x: Type) -> expr`
            if peek1 == TokenKind::RParen {
                let peek2 = self.peek(2);
                return peek2 == TokenKind::Arrow;
            }
        }
        false
    }

    fn parse_lambda(&mut self) -> Expr {
        let start = self.current().span;
        let mut params = Vec::new();
        params.push(self.parse_param());
        while self.check(TokenKind::Comma) {
            self.advance();
            params.push(self.parse_param());
        }
        // 处理括号参数 lambda: `(x) -> expr`
        if self.check(TokenKind::RParen) {
            self.advance();
        }
        self.expect(TokenKind::Arrow);
        let body = self.parse_expression(0);
        Expr::Lambda {
            params,
            body: Box::new(body),
            span: Span::merge(&start, &self.current().span),
        }
    }

    fn infix_binding_power(&self) -> u8 {
        match self.current().kind {
            TokenKind::Assign => 1,
            TokenKind::PlusEq
            | TokenKind::MinusEq
            | TokenKind::StarEq
            | TokenKind::SlashEq
            | TokenKind::PercentEq => 1,
            TokenKind::Plus | TokenKind::Minus => 6,
            TokenKind::Star | TokenKind::Slash | TokenKind::Percent => 7,
            TokenKind::EqEq | TokenKind::Neq => 3,
            TokenKind::Lt | TokenKind::Gt | TokenKind::LtEq | TokenKind::GtEq => 4,
            TokenKind::AndAnd => 2,
            TokenKind::OrOr => 1,
            TokenKind::LtLt | TokenKind::GtGt | TokenKind::GtGtGt => 5,
            TokenKind::QuestionMark => 8, // Elvis
            TokenKind::DoubleDotOp => 8,  // 范围 ..
            TokenKind::To => 2,           // map entry: "a" to 1
            _ => 0,
        }
    }

    /// 检测并消费复合赋值运算符（`+=` 等），返回对应的二元运算符
    fn compound_assign_op(&mut self) -> Option<BinOp> {
        let op = match self.current().kind {
            TokenKind::PlusEq => BinOp::Add,
            TokenKind::MinusEq => BinOp::Sub,
            TokenKind::StarEq => BinOp::Mul,
            TokenKind::SlashEq => BinOp::Div,
            TokenKind::PercentEq => BinOp::Mod,
            _ => return None,
        };
        self.advance();
        Some(op)
    }

    fn parse_infix_operator(&mut self) -> BinOp {
        match self.advance().kind {
            TokenKind::Assign => BinOp::Assign,
            TokenKind::Plus => BinOp::Add,
            TokenKind::Minus => BinOp::Sub,
            TokenKind::Star => BinOp::Mul,
            TokenKind::Slash => BinOp::Div,
            TokenKind::Percent => BinOp::Mod,
            TokenKind::EqEq => BinOp::Eq,
            TokenKind::Neq => BinOp::Ne,
            TokenKind::Lt => BinOp::Lt,
            TokenKind::Gt => BinOp::Gt,
            TokenKind::LtEq => BinOp::Le,
            TokenKind::GtEq => BinOp::Ge,
            TokenKind::AndAnd => BinOp::And,
            TokenKind::OrOr => BinOp::Or,
            TokenKind::LtLt => BinOp::Shl,
            TokenKind::GtGt => BinOp::Shr,
            TokenKind::GtGtGt => BinOp::UShr,
            _ => BinOp::Add,
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // 关键字表达式
    // ─────────────────────────────────────────────────────────────────────────

    /// if (cond) thenExpr [else elseExpr]
    fn parse_if_expression(&mut self, start: Span) -> Expr {
        self.advance(); // if
        self.expect(TokenKind::LParen);
        let condition = self.parse_expression(0);
        self.expect(TokenKind::RParen);

        let then_branch = self.parse_expression(0);
        let else_branch = if self.check(TokenKind::Else) {
            self.advance();
            Some(Box::new(self.parse_expression(0)))
        } else {
            None
        };

        Expr::If {
            condition: Box::new(condition),
            then_branch: Box::new(then_branch),
            else_branch,
            span: Span::merge(&start, &self.current().span),
        }
    }

    /// try { ... } catch (e: Type) { ... } finally { ... }
    fn parse_try_expression(&mut self, start: Span) -> Expr {
        self.advance(); // try
        let block = self.parse_block();

        let mut catches = Vec::new();
        while self.check(TokenKind::Catch) {
            self.advance(); // catch
            self.expect(TokenKind::LParen);
            let variable = self.advance().literal.clone();
            let type_name = if self.check(TokenKind::Colon) {
                self.advance();
                let ty = self.parse_type();
                match ty {
                    Type::Named { name, .. } => name,
                    Type::Generic { name, .. } => name,
                    _ => "Any".to_string(),
                }
            } else {
                "Any".to_string()
            };
            self.expect(TokenKind::RParen);
            let body = self.parse_block();
            catches.push(CatchClause {
                variable,
                type_name,
                body: Box::new(body),
                span: Span::merge(&start, &self.current().span),
            });
        }

        let finally = if self.check(TokenKind::Finally) {
            self.advance(); // finally
            Some(Box::new(self.parse_block()))
        } else {
            None
        };

        Expr::Try {
            block: Box::new(block),
            catches,
            finally,
            span: Span::merge(&start, &self.current().span),
        }
    }

    /// when (subject) { pattern -> body ... }
    fn parse_when_expression(&mut self, start: Span) -> Expr {
        self.advance(); // when

        let subject = if self.check(TokenKind::LParen) {
            self.advance();
            let s = self.parse_expression(0);
            self.expect(TokenKind::RParen);
            Some(Box::new(s))
        } else {
            None
        };

        self.expect(TokenKind::LBrace);
        let mut arms = Vec::new();

        while !self.check(TokenKind::RBrace) && !self.is_at_end() {
            let arm_start = self.current().span;

            // 模式解析（支持多值分支：`0, 1 -> ...`）
            let mut patterns = Vec::new();
            loop {
                let pattern = if self.check(TokenKind::In) {
                    // in 90..100 -> ...（范围匹配模式）
                    self.advance();
                    let range = self.parse_expression(0);
                    Expr::InRange {
                        range: Box::new(range),
                        span: arm_start,
                    }
                } else if self.check(TokenKind::Is) {
                    // is String -> ... 或 is List<*> -> ...
                    self.advance();
                    let ty = self.parse_type();
                    self.pattern_to_expr(ty, arm_start)
                } else {
                    // 普通表达式模式（含 else ->）
                    self.parse_expression(0)
                };
                patterns.push(pattern);

                if self.check(TokenKind::Comma) {
                    self.advance();
                    continue;
                }
                break;
            }

            // 守卫：is X && cond -> ...  或 pattern && cond -> ...
            let guard = if self.check(TokenKind::AndAnd) || self.check(TokenKind::OrOr) {
                self.advance(); // 消费 && 或 ||
                let guard_expr = self.parse_expression(0);
                Some(Box::new(guard_expr))
            } else {
                None
            };

            self.expect(TokenKind::Arrow);
            let body = self.parse_expression(0);

            arms.push(WhenArm {
                patterns,
                guard,
                body: Box::new(body),
                span: Span::merge(&arm_start, &self.current().span),
            });
        }
        self.expect(TokenKind::RBrace);

        Expr::When {
            subject,
            arms,
            span: Span::merge(&start, &self.current().span),
        }
    }

    /// for (pattern in iterable) body
    fn parse_for_expression(&mut self, start: Span) -> Expr {
        self.advance(); // for
        self.expect(TokenKind::LParen);
        let pattern = self.parse_expression(0);
        self.expect(TokenKind::In);
        let iterable = self.parse_expression(0);
        self.expect(TokenKind::RParen);
        let body = self.parse_expression(0);

        Expr::For {
            pattern: Box::new(pattern),
            iterable: Box::new(iterable),
            body: Box::new(body),
            span: Span::merge(&start, &self.current().span),
        }
    }

    /// while (condition) body
    fn parse_while_expression(&mut self, start: Span) -> Expr {
        self.advance(); // while
        self.expect(TokenKind::LParen);
        let condition = self.parse_expression(0);
        self.expect(TokenKind::RParen);
        let body = self.parse_expression(0);

        Expr::While {
            condition: Box::new(condition),
            body: Box::new(body),
            span: Span::merge(&start, &self.current().span),
        }
    }

    /// do body while (condition)
    fn parse_do_while_expression(&mut self, start: Span) -> Expr {
        self.advance(); // do
        let body = self.parse_expression(0);
        self.expect(TokenKind::While);
        self.expect(TokenKind::LParen);
        let condition = self.parse_expression(0);
        self.expect(TokenKind::RParen);

        Expr::DoWhile {
            condition: Box::new(condition),
            body: Box::new(body),
            span: Span::merge(&start, &self.current().span),
        }
    }

    /// 将 when 分支中的类型模式转为表达式节点（简单起见以类型名作为标识符节点）
    fn pattern_to_expr(&self, ty: Type, start: Span) -> Expr {
        match ty {
            Type::Named { name, .. } => Expr::Ident(name, start),
            Type::Generic { name, .. } => Expr::Ident(name, start),
            _ => Expr::Ident("<type>".to_string(), start),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;

    fn parse_program(src: &str) -> (Program, Vec<CompileError>) {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize();
        let mut parser = Parser::new(tokens);
        let program = parser.parse_program();
        let errors: Vec<CompileError> = parser.errors().to_vec();
        (program, errors)
    }

    #[test]
    fn test_parse_simple_fn() {
        let (program, errors) = parse_program(
            r#"
            fun add(a: Int, b: Int): Int {
                return a + b
            }
        "#,
        );
        assert!(errors.is_empty());
        assert_eq!(program.declarations.len(), 1);
        match &program.declarations[0] {
            Decl::Function(fn_decl) => {
                assert_eq!(fn_decl.name, "add");
                assert_eq!(fn_decl.params.len(), 2);
                assert_eq!(fn_decl.params[0].name, "a");
                assert_eq!(fn_decl.params[1].name, "b");
                assert!(fn_decl.return_type.is_some());
                assert!(fn_decl.body.is_some());
            }
            _ => panic!("Expected function declaration"),
        }
    }

    #[test]
    fn test_parse_val_statement() {
        let (_, errors) = parse_program("val x: Int = 100");
        assert!(errors.is_empty());
    }

    #[test]
    fn test_parse_binary_expression() {
        let (_, errors) = parse_program("val result = 1 + 2 * 3");
        assert!(errors.is_empty());
    }

    #[test]
    fn test_parse_struct() {
        let (program, errors) = parse_program(
            r#"
            struct Player(
                val id: Int,
                var name: String
            )
        "#,
        );
        assert!(errors.is_empty());
        assert_eq!(program.declarations.len(), 1);
        match &program.declarations[0] {
            Decl::Struct(s) => {
                assert_eq!(s.name, "Player");
                assert_eq!(s.fields.len(), 2);
            }
            _ => panic!("Expected struct declaration"),
        }
    }

    #[test]
    fn test_parse_enum() {
        let (program, errors) = parse_program(
            r#"
            enum Color {
                RED,
                GREEN,
                BLUE
            }
        "#,
        );
        assert!(errors.is_empty());
        match &program.declarations[0] {
            Decl::Enum(e) => {
                assert_eq!(e.name, "Color");
                assert_eq!(e.variants.len(), 3);
            }
            _ => panic!("Expected enum declaration"),
        }
    }

    #[test]
    fn test_parse_if_expression() {
        let (_, errors) = parse_program("val max = if (a > b) a else b");
        assert!(errors.is_empty());
    }

    #[test]
    fn test_parse_when_expression() {
        let (_, errors) = parse_program(
            r#"
            val result = when (x) {
                0 -> "zero"
                1 -> "one"
                else -> "other"
            }
        "#,
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn test_parse_for_loop() {
        let (_, errors) = parse_program("for (i in 0..10) { println(i) }");
        assert!(errors.is_empty());
    }

    #[test]
    fn test_parse_multiple_declarations() {
        let (program, errors) = parse_program(
            r#"
            fun foo() { }
            fun bar(x: Int): Int = x * 2
        "#,
        );
        assert!(errors.is_empty());
        assert_eq!(program.declarations.len(), 2);
    }

    // ── 回归测试：此前会导致无限循环/内存暴涨的场景 ──

    #[test]
    fn test_regression_unary_minus() {
        // 一元负号此前会无限递归（先递归再 advance）
        let (_, errors) = parse_program("val x = -5");
        assert!(errors.is_empty());
    }

    #[test]
    fn test_regression_unary_minus_complex() {
        let (_, errors) = parse_program("val x = -a + b * -c");
        assert!(errors.is_empty());
    }

    #[test]
    fn test_regression_binary_chain() {
        // 长链二元运算此前会在 EOF 处死循环
        let (_, errors) = parse_program("val result = 1 + 2 * 3 - 4 / 2 % 3");
        assert!(errors.is_empty());
    }

    #[test]
    fn test_regression_nested_block() {
        let (_, errors) = parse_program(
            r#"
            fun main() {
                if (true) {
                    for (i in 0..10) {
                        println(i)
                    }
                }
            }
        "#,
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn test_regression_eof_content() {
        // 文件只有函数声明无 body，不应死循环
        let (_, errors) = parse_program("fun foo(x: Int): Int");
        assert!(errors.is_empty());
    }

    #[test]
    fn test_regression_return_stmt() {
        let (_, errors) = parse_program(
            r#"
            fun fib(n: Int): Int {
                if (n <= 1) return n
                return fib(n - 1) + fib(n - 2)
            }
        "#,
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn test_regression_empty_input() {
        let (prog, errors) = parse_program("");
        assert!(errors.is_empty());
        assert!(prog.declarations.is_empty());
    }

    #[test]
    fn test_regression_class_interface() {
        let (prog, errors) = parse_program(
            r#"
            interface Renderable {
                fun render()
                fun zOrder(): Int = 0
            }

            class Sprite : Renderable {
                var x: Float = 0f
                var y: Float = 0f
                override fun render() { }
            }
        "#,
        );
        assert!(errors.is_empty());
        assert_eq!(prog.declarations.len(), 2);
    }

    #[test]
    fn test_regression_struct_data_class() {
        let (prog, errors) = parse_program(
            r#"
            struct Player(
                val id: Int,
                var name: String = "unknown",
                var health: Int = 100
            )
        "#,
        );
        assert!(errors.is_empty());
        match &prog.declarations[0] {
            Decl::Struct(s) => assert_eq!(s.fields.len(), 3),
            _ => panic!("Expected struct"),
        }
    }

    #[test]
    fn test_complex_expression_precedence() {
        let (_, errors) = parse_program("val result = a == b && c > d || e != f");
        assert!(errors.is_empty());
    }

    #[test]
    fn test_parse_string_with_interpolation() {
        let (_, errors) = parse_program(r#"val msg = "Hello, $name! Score: ${score * 2}""#);
        assert!(errors.is_empty());
    }

    // ── 新增特性回归测试 ──

    #[test]
    fn test_lateinit_var() {
        let (_, errors) = parse_program("lateinit var texture: Texture");
        assert!(errors.is_empty());
    }

    #[test]
    fn test_val_by_lazy() {
        let (_, errors) = parse_program("val config by lazy { loadConfig() }");
        assert!(errors.is_empty());
    }

    #[test]
    fn test_data_class() {
        let (prog, errors) = parse_program("data class Point(val x: Float, val y: Float)");
        assert!(errors.is_empty());
        match &prog.declarations[0] {
            Decl::Class(c) => assert_eq!(c.fields.len(), 2),
            _ => panic!("Expected class"),
        }
    }

    #[test]
    fn test_data_struct() {
        let (prog, errors) = parse_program("data struct Vec2(val x: Float, val y: Float)");
        assert!(errors.is_empty());
        match &prog.declarations[0] {
            Decl::Struct(s) => assert_eq!(s.fields.len(), 2),
            _ => panic!("Expected struct"),
        }
    }

    #[test]
    fn test_sealed_class() {
        let (_, errors) = parse_program("sealed class Result<T, E> { fun isOk(): Boolean }");
        assert!(errors.is_empty());
    }

    #[test]
    fn test_sealed_struct() {
        let (_, errors) = parse_program("sealed struct Shape { fun area(): Float }");
        assert!(errors.is_empty());
    }

    #[test]
    fn test_named_args_in_call() {
        let (_, errors) =
            parse_program("createWindow(title = \"Aura\", width = 800, height = 600)");
        assert!(errors.is_empty());
    }

    #[test]
    fn test_to_operator_map_entry() {
        let (_, errors) = parse_program("val m = mapOf(\"a\" to 1, \"b\" to 2)");
        assert!(errors.is_empty());
    }

    #[test]
    fn test_when_guard() {
        let (_, errors) = parse_program(
            r#"
            when (event) {
                is MouseClick && event.x > 0 -> handleClick(event)
                is KeyPress && event.key == Key.Escape -> shutdown()
                else -> {}
            }
            "#,
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn test_await_expression() {
        let (_, errors) = parse_program("val result = await fetchData(url)");
        assert!(errors.is_empty());
    }

    #[test]
    fn test_destructure() {
        let (_, errors) = parse_program("val (x, y) = getPoint()");
        assert!(errors.is_empty());
    }

    #[test]
    fn test_generic_bounds() {
        let (_, errors) = parse_program("fun <T : Comparable<T>> max(a: T, b: T): T = a");
        assert!(errors.is_empty());
    }

    #[test]
    fn test_doc_comment_on_function() {
        let (program, errors) =
            parse_program("/// Adds two numbers\nfun add(a: Int, b: Int): Int { return a + b }");
        assert!(errors.is_empty());
        match &program.declarations[0] {
            Decl::Function(f) => {
                assert_eq!(f.name, "add");
                assert_eq!(f.doc.as_deref(), Some("Adds two numbers"));
            }
            other => panic!("expected function declaration, got {:?}", other),
        }
    }

    #[test]
    fn test_doc_comment_block_on_struct() {
        let (program, errors) = parse_program(
            "/**\n * A point.\n * Second line.\n */\nstruct Point(val x: Int, val y: Int)",
        );
        assert!(errors.is_empty());
        match &program.declarations[0] {
            Decl::Struct(s) => {
                assert_eq!(s.name, "Point");
                assert_eq!(s.doc.as_deref(), Some("A point.\nSecond line."));
            }
            other => panic!("expected struct declaration, got {:?}", other),
        }
    }

    #[test]
    fn test_doc_comment_on_member_function() {
        let (program, errors) =
            parse_program("struct Box {\n    /// Renders the box\n    fun render() {}\n}");
        assert!(errors.is_empty());
        match &program.declarations[0] {
            Decl::Struct(s) => {
                assert_eq!(s.methods.len(), 1);
                assert_eq!(s.methods[0].doc.as_deref(), Some("Renders the box"));
            }
            other => panic!("expected struct declaration, got {:?}", other),
        }
    }

    #[test]
    fn test_decl_without_doc_comment() {
        let (program, errors) = parse_program("fun add(a: Int, b: Int): Int { return a + b }");
        assert!(errors.is_empty());
        match &program.declarations[0] {
            Decl::Function(f) => assert_eq!(f.doc, None),
            other => panic!("expected function declaration, got {:?}", other),
        }
    }

    #[test]
    fn test_inline_override_suspend() {
        let (_, errors) = parse_program(
            r#"
            inline fun <T> filter(list: List<T>, pred: (T) -> Boolean): List<T> = listOf()
            override fun render() { }
            suspend fun fetchData(url: String): Data { return Data() }
            "#,
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn test_actor_spawn() {
        let (_, errors) = parse_program(
            r#"
            actor WindowManager {
                fun onCreate(config: WindowConfig): Window { return Window(config) }
            }
            fun main() {
                val wm = WindowManager.spawn()
            }
            "#,
        );
        assert!(errors.is_empty());
    }

    #[test]
    fn test_when_multi_pattern_arm() {
        let (program, errors) = parse_program(
            "fun f(x: Int): String { return when (x) { 0, 1 -> \"low\" in 2..9 -> \"mid\" else -> \"high\" } }",
        );
        assert!(errors.is_empty());
        match &program.declarations[0] {
            Decl::Function(f) => {
                let body = f.body.as_ref().expect("body");
                if let Expr::Block(stmts, _) = &**body {
                    if let Stmt::Expr(Expr::Return { value: Some(v), .. }) = &stmts[0] {
                        if let Expr::When { arms, .. } = &**v {
                            assert_eq!(arms.len(), 3);
                            assert_eq!(arms[0].patterns.len(), 2);
                            return;
                        }
                    }
                }
                panic!("expected when expression, got {:?}", body);
            }
            other => panic!("expected function declaration, got {:?}", other),
        }
    }

    #[test]
    fn test_star_projection_type() {
        let (_, errors) =
            parse_program("fun f(v: Any): Int { return when (v) { is List<*> -> 1 else -> 0 } }");
        assert!(errors.is_empty());
    }

    #[test]
    fn test_class_inheritance_single_colon() {
        let (program, errors) = parse_program("class Dog : Animal() { fun bark() {} }");
        assert!(errors.is_empty());
        match &program.declarations[0] {
            Decl::Class(c) => {
                assert_eq!(c.name, "Dog");
                assert_eq!(c.superclass.as_deref(), Some("Animal"));
            }
            other => panic!("expected class, got {:?}", other),
        }
    }

    #[test]
    fn test_sealed_class_generic_inheritance() {
        let (program, errors) = parse_program(
            "sealed class Result<out T> { }\ndata class Success<T>(val data: T) : Result<T>()",
        );
        assert!(errors.is_empty());
        match &program.declarations[1] {
            Decl::Class(c) => {
                assert_eq!(c.name, "Success");
                assert_eq!(c.superclass.as_deref(), Some("Result"));
                assert_eq!(c.fields.len(), 1);
            }
            other => panic!("expected class, got {:?}", other),
        }
    }

    #[test]
    fn test_class_with_interface_list() {
        let (program, errors) =
            parse_program("class Dog : Animal(), Runnable, Pet { fun bark() {} }");
        assert!(errors.is_empty());
        match &program.declarations[0] {
            Decl::Class(c) => {
                assert_eq!(c.superclass.as_deref(), Some("Animal"));
                assert_eq!(c.implementations, vec!["Runnable", "Pet"]);
            }
            other => panic!("expected class, got {:?}", other),
        }
    }
}
