//! LLVM IR 文本生成器
//!
//! 对应 技术方案 §9.2.1 / §9.2.3 / §9.2.4。
//!
//! 将 HIR 程序生成为 LLVM IR 文本字符串。生成流程：
//!
//! 1. 模块头（`target triple` / `data layout`）
//! 2. 全局结构体类型定义（`%struct.<Name> = type { ... }`）
//! 3. FFI 外部函数声明（`declare ...`）
//! 4. Runtime 库函数声明
//! 5. 用户函数定义（`define ...`）
//! 6. `main` 入口（若不存在则合成）
//! 7. Debug 元数据（可选）
//!
//! 生成策略：
//! - 使用 LLVM 13+ 的 **不透明指针**（`ptr`）语法
//! - 局部变量在 `entry` 基本块使用 `alloca` 分配（栈分配，符合 Aura 栈优先设计）
//! - 控制流使用条件分支与 PHI 节点（LLVM SSA 形式）
//! - 字符串作为 `{ i8*, i64 }` 结构（数据指针 + 长度），通过 runtime 创建

use std::collections::HashMap;

use crate::codegen::aot::dwarf::{DebugInfo, emit_debug_metadata, emit_subprogram};
use crate::codegen::aot::error::AotError;
use crate::codegen::aot::ffi::FfiGenerator;
use crate::codegen::aot::optimize::OptimizationLevel;
use crate::codegen::aot::runtime::generate_runtime_declarations;
use crate::codegen::aot::types::{TypeMapper, sanitizellvm};
use crate::codegen::hir::{
    HirBinOp, HirBlock, HirExpr, HirFunction, HirProgram, HirStmt, HirType, HirUnOp,
};
use crate::codegen::opcode::{Const, FfiAbi};

use super::AotCodeGenerator;

/// LLVM IR 生成上下文
pub(crate) struct EmitCtx {
    pub type_mapper: TypeMapper,
    pub target_triple: String,
    pub _opt_level: OptimizationLevel,
    pub _string_as_struct: bool,
    pub link_runtime: bool,
    pub debug_info: Option<DebugInfo>,
    /// Phase 1 AOT Blob 模式：生成 JitValue ABI 包装函数
    pub blob_mode: bool,
    /// 已声明的结构体类型名集合
    pub declared_structs: std::collections::HashSet<String>,
    /// 已生成的函数名集合
    pub generated_funcs: std::collections::HashSet<String>,
    /// 生成的 LLVM IR 片段
    pub sections: Vec<String>,
    /// 全局基本块计数器
    pub bb_counter: u64,
    /// 全局变量计数器
    pub var_counter: u64,
    /// 全局常量计数器
    pub const_counter: u64,
    /// 当前函数内变量作用域栈
    pub var_scope: Vec<HashMap<String, VarSlot>>,
    /// 全局常量/变量定义（模块顶层，§9.2.1 generate_globals）
    pub globals: Vec<String>,
    /// 全局常量去重（key → 对应 LLVM 全局名），避免同一字符串重复分配
    pub global_const_map: HashMap<String, String>,
    /// DWARF 子程序元数据实体（`!N = !DISubprogram(...)`，模块末尾统一输出）
    pub subprogram_meta: Vec<String>,
    /// 下一个 DISubprogram 元数据索引
    pub subprogram_index: u32,
    /// 函数名 → DISubprogram 元数据 ID（供 `!dbg !N` 引用）
    pub func_dbg_ids: HashMap<String, u32>,
    /// 函数名 → LLVM 返回类型字符串（用于 emit_call 推断返回类型）
    pub func_ret_types: HashMap<String, String>,
    /// P3.2: 枚举变体映射（枚举名 → [(变体名, 变体索引, 关联值数)]）
    pub enum_variants: HashMap<String, Vec<(String, usize, usize)>>,
    /// P3.2: 枚举最大关联值字段数（用于 tagged union 结构体）
    pub enum_max_fields: HashMap<String, usize>,
    /// P3.3: Lambda 静态函数名计数器
    pub lambda_counter: u64,
    /// P3.3: Lambda 生成的函数 IR 片段（模块末尾输出）
    pub lambda_funcs: Vec<String>,
}

/// 变量槽（LLVM 名称 + 类型）
#[derive(Debug, Clone)]
pub(crate) struct VarSlot {
    llvm_name: String,
    llvm_ty: String,
}

impl EmitCtx {
    pub fn new(
        type_mapper: TypeMapper,
        target_triple: String,
        opt_level: OptimizationLevel,
        string_as_struct: bool,
        link_runtime: bool,
        debug_info: Option<DebugInfo>,
        blob_mode: bool,
    ) -> Self {
        Self {
            type_mapper,
            target_triple,
            _opt_level: opt_level,
            _string_as_struct: string_as_struct,
            link_runtime,
            debug_info,
            blob_mode,
            declared_structs: std::collections::HashSet::new(),
            generated_funcs: std::collections::HashSet::new(),
            sections: Vec::new(),
            bb_counter: 0,
            var_counter: 0,
            const_counter: 0,
            var_scope: vec![HashMap::new()],
            globals: Vec::new(),
            global_const_map: HashMap::new(),
            subprogram_meta: Vec::new(),
            subprogram_index: 0,
            func_dbg_ids: HashMap::new(),
            func_ret_types: HashMap::new(),
            enum_variants: HashMap::new(),
            enum_max_fields: HashMap::new(),
            lambda_counter: 0,
            lambda_funcs: Vec::new(),
        }
    }

    pub fn fresh_var(&mut self) -> String {
        let name = format!("%var.{}", self.var_counter);
        self.var_counter += 1;
        name
    }

    pub fn fresh_bb(&mut self, prefix: &str) -> String {
        let name = format!("bb.{}.{}", prefix, self.bb_counter);
        self.bb_counter += 1;
        name
    }

    pub fn fresh_const(&mut self, prefix: &str) -> String {
        let name = format!("%{}.{}", prefix, self.const_counter);
        self.const_counter += 1;
        name
    }

    pub fn declare_var(&mut self, name: &str, llvm_name: String, llvm_ty: String) {
        if let Some(scope) = self.var_scope.last_mut() {
            scope.insert(
                name.to_string(),
                VarSlot {
                    llvm_name,
                    llvm_ty,
                },
            );
        }
    }

    pub fn lookup_var(&self, name: &str) -> Option<&VarSlot> {
        for scope in self.var_scope.iter().rev() {
            if let Some(v) = scope.get(name) {
                return Some(v);
            }
        }
        None
    }

    pub fn enter_scope(&mut self) {
        self.var_scope.push(HashMap::new());
    }

    pub fn exit_scope(&mut self) {
        self.var_scope.pop();
    }

    pub fn llvm_type(&self, ty: &HirType) -> String {
        self.type_mapper.map(ty)
    }

    pub fn emit_module_header(&mut self) {
        let data_layout = match &self.target_triple[..] {
            t if t.starts_with("x86_64") => {
                "e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-f80:128-n8:16:32:64-S128"
            }
            t if t.starts_with("aarch64") => {
                "e-m:e-i8:8:32-i16:16:32-i32:32:64-i64:64:-n16:32:64-S128"
            }
            t if t.starts_with("armv7") => {
                "e-m:e-i8:8:32-i16:16:32-i32:32:32-i64:64:-i128:128-n32:64-S128"
            }
            _ => "e-m:e-i64:64-f80:128-n8:16:32:64-S128",
        };

        let mut s = String::new();
        s.push_str("; ModuleID = 'aura-aot-module'\n");
        s.push_str("source_filename = \"aura-aot\"\n");
        s.push_str(&format!("target triple = \"{}\"\n", self.target_triple));
        s.push_str(&format!("target datalayout = \"{}\"\n", data_layout));
        s.push('\n');
        self.sections.push(s);
    }

    pub fn emit_struct_defs(&mut self, program: &HirProgram) {
        let mut s = String::new();
        s.push_str("; ---- Structure Type Definitions ----\n");
        for st in &program.structs {
            let llvm_name = format!("%struct.{}", sanitizellvm(&st.name));
            if !self.declared_structs.insert(llvm_name.clone()) {
                continue;
            }
            let fields: Vec<String> = st.fields.iter().map(|(_, ty)| self.llvm_type(ty)).collect();
            s.push_str(&format!("{} = type {{{}}}\n", llvm_name, fields.join(", ")));
        }
        // P3.2: Enum → tagged union 结构体（tag: i32, 后续字段为关联值）
        s.push_str("; ---- Enum Type Definitions (tagged union) ----\n");
        for e in &program.enums {
            let llvm_name = format!("%struct.{}", sanitizellvm(&e.name));
            if !self.declared_structs.insert(llvm_name.clone()) {
                continue;
            }
            // 取所有变体中最多的关联值字段数
            let max_fields = e.variants.iter().map(|(_, fields)| fields.len()).max().unwrap_or(0);
            let mut fields_str = vec!["i32".to_string()]; // tag
            for _ in 0..max_fields {
                fields_str.push("ptr".to_string()); // 关联值用 ptr（堆分配）
            }
            s.push_str(&format!(
                "{} = type {{{}}}\n",
                llvm_name,
                fields_str.join(", ")
            ));
            // P3.2: 记录枚举变体映射（用于 emit_member_access 构造 tagged union）
            let mut variants_info = Vec::new();
            for (vi, (vname, vfields)) in e.variants.iter().enumerate() {
                variants_info.push((vname.clone(), vi, vfields.len()));
            }
            self.enum_variants.insert(e.name.clone(), variants_info);
            self.enum_max_fields.insert(e.name.clone(), max_fields);
        }
        self.sections.push(s);
    }

    /// 输出 FFI 常量（extern 块中的 val 声明 → LLVM 全局常量）
    pub fn emit_ffi_constants(&mut self, program: &HirProgram) {
        if program.constants.is_empty() {
            return;
        }
        let mut s = String::new();
        s.push_str("; ---- FFI Constants ----\n");
        for (name, c) in &program.constants {
            let llvm_name = format!("@{}", sanitizellvm(name));
            let (llvm_ty, llvm_val) = match c {
                Const::Int(i) => {
                    let ty = "i64";
                    let v = format!("{}", i);
                    (ty.to_string(), v)
                }
                Const::Float(f) => {
                    let ty = "double";
                    let v = format!("{:.1e}", f);
                    (ty.to_string(), v)
                }
                Const::Str(str) => {
                    let c_str: Vec<u8> = format!("{}\0", str).into_bytes();
                    let hex_bytes: Vec<String> = c_str.iter().map(|b| format!("c{}", b)).collect();
                    (
                        format!("[{} x i8]", c_str.len()),
                        format!("{{ {} }}", hex_bytes.join(", ")),
                    )
                }
                Const::Bool(b) => {
                    let ty = "i1";
                    let v = format!("{}", if *b { 1 } else { 0 });
                    (ty.to_string(), v)
                }
                Const::Null => {
                    let ty = "ptr";
                    let v = "null".to_string();
                    (ty.to_string(), v)
                }
            };
            s.push_str(&format!(
                "{} = global {} {}\n",
                llvm_name, llvm_ty, llvm_val
            ));
        }
        s.push('\n');
        self.sections.push(s);
    }

    pub fn emit_ffi(&mut self, program: &HirProgram) {
        let mut s = String::new();
        s.push_str("; ---- FFI Declarations ----\n");
        let ffi_gen = FfiGenerator::new(&self.type_mapper);
        if let Ok(decls) = ffi_gen.generate_declarations(&program.natives) {
            for d in &decls {
                s.push_str(d);
                s.push('\n');
            }
        }
        self.sections.push(s);
    }

    pub fn emit_runtime(&mut self) {
        if !self.link_runtime {
            return;
        }
        self.sections.push(generate_runtime_declarations(&self.type_mapper));
    }
}

/// 从 HIR 程序生成完整 LLVM IR 文本
pub fn emit_program(
    codegen: &AotCodeGenerator,
    program: &HirProgram,
    blob_mode: bool,
) -> Result<String, AotError> {
    let debug_info =
        if codegen.options.debug_info { Some(DebugInfo::new("main.aura")) } else { None };

    let mut ctx = EmitCtx::new(
        codegen.type_mapper.clone(),
        codegen.options.target.to_string(),
        codegen.options.opt_level,
        codegen.options.string_as_struct,
        codegen.options.link_runtime,
        debug_info,
        blob_mode,
    );

    // 1. 模块头
    ctx.emit_module_header();

    // 2. 结构体类型定义
    ctx.emit_struct_defs(program);

    // 2.5 FFI 常量（P8.1）：extern 块中的常量声明 → LLVM 全局常量
    ctx.emit_ffi_constants(program);

    // 3. FFI 声明
    ctx.emit_ffi(program);

    // 4. Runtime 声明
    ctx.emit_runtime();

    // 4.5 Debug 元数据声明（模块级，先于函数体输出，保证 `!dbg !N` 引用合法）
    if ctx.debug_info.is_some() {
        let mut s = emit_debug_metadata(ctx.debug_info.as_ref().unwrap());
        s.push_str(&crate::codegen::aot::dwarf::emit_subroutine_type());
        // 预注册所有用户函数的 DISubprogram 实体
        for func in &program.functions {
            if func.is_native {
                continue;
            }
            let idx = ctx.subprogram_index;
            ctx.subprogram_index += 1;
            let (_mid, loc_id, text) =
                emit_subprogram(ctx.debug_info.as_ref().unwrap(), func, 1, idx);
            ctx.func_dbg_ids.insert(func.name.clone(), loc_id);
            ctx.subprogram_meta.push(text);
        }
        // 若没有 main，为合成 main 预留 ID（emit_function 内处理）
        if !program.functions.iter().any(|f| f.name == "main") {
            let idx = ctx.subprogram_index;
            ctx.subprogram_index += 1;
            let (_mid, loc_id, text) = emit_subprogram(
                ctx.debug_info.as_ref().unwrap(),
                &synthesize_main(&program.functions),
                1,
                idx,
            );
            ctx.func_dbg_ids.insert("main".to_string(), loc_id);
            ctx.subprogram_meta.push(text);
        }
        for m in &ctx.subprogram_meta {
            s.push_str(m);
        }
        s.push('\n');
        ctx.sections.push(s);
    }

    // 4.8 预注册函数返回类型映射（供 emit_call 推断返回类型）
    for func in &program.functions {
        if let Some(ref ret) = func.ret {
            ctx.func_ret_types.insert(func.name.clone(), ctx.type_mapper.map(ret));
        } else {
            ctx.func_ret_types.insert(func.name.clone(), "void".to_string());
        }
    }
    for native in &program.natives {
        if let Some(ref ret) = native.ret {
            ctx.func_ret_types.insert(native.name.clone(), ctx.type_mapper.map(ret));
        } else {
            ctx.func_ret_types.insert(native.name.clone(), "void".to_string());
        }
    }

    // 5. 生成所有用户函数（期间收集的全局常量在函数后统一输出）
    //    blob_mode 下为每个非原生函数生成 JitValue ABI 包装函数
    for func in &program.functions {
        if ctx.generated_funcs.contains(&func.name) {
            continue;
        }
        ctx.generated_funcs.insert(func.name.clone());
        let func_ir = emit_function(&mut ctx, func)?;
        ctx.sections.push(func_ir);

        // blob_mode: 生成包装函数
        if blob_mode && !func.is_native {
            if let Ok(wrapper_ir) = emit_wrapper(&mut ctx, func) {
                ctx.sections.push(wrapper_ir);
            }
            // 注意：包装函数生成失败不中断编译（该函数将被跳过）
        }
    }

    // 6. 若没有 main 函数，合成一个（blob_mode 下不需要 main 入口）
    if !blob_mode && !ctx.generated_funcs.contains("main") {
        let main_func = synthesize_main(&program.functions);
        ctx.generated_funcs.insert("main".to_string());
        let func_ir = emit_function(&mut ctx, &main_func)?;
        ctx.sections.push(func_ir);
    }

    // 6.5 Lambda 静态函数（P3.3）
    if !ctx.lambda_funcs.is_empty() {
        let mut s = String::new();
        s.push_str("; ---- Lambda Functions ----\n");
        for lf in &ctx.lambda_funcs {
            s.push_str(lf);
            s.push('\n');
        }
        s.push('\n');
        ctx.sections.push(s);
    }

    // 7. 模块级全局常量（字符串字面量等，§9.2.1 generate_globals）
    if !ctx.globals.is_empty() {
        let mut s = String::new();
        s.push_str("; ---- Globals ----\n");
        for g in &ctx.globals {
            s.push_str(g);
            s.push('\n');
        }
        s.push('\n');
        ctx.sections.push(s);
    }

    // 8. 全部完成（Debug 元数据已在步骤 4.5 输出）

    Ok(ctx.sections.join("\n"))
}

/// 生成单个函数的 LLVM IR
fn emit_function(ctx: &mut EmitCtx, func: &HirFunction) -> Result<String, AotError> {
    ctx.enter_scope();

    let mut blocks = FuncBlocks::new();
    let entry_name = ctx.fresh_bb("entry");
    let _ = blocks.add_block_named(&entry_name);

    // 返回类型
    let ret_ty = func.ret.as_ref().map(|t| ctx.llvm_type(t)).unwrap_or_else(|| "i32".to_string());
    let ret_str = if ret_ty.is_empty() { "void".to_string() } else { ret_ty };

    // 参数类型
    let params: Vec<(String, String)> = func
        .params
        .iter()
        .map(|p| {
            let ty = ctx.llvm_type(p.ty.as_ref().unwrap_or(&HirType::Named("Int".into())));
            (p.name.clone(), ty)
        })
        .collect();

    let params_ir: Vec<String> =
        params.iter().map(|(name, ty)| format!("{} %arg.{}", ty, sanitizellvm(name))).collect();
    let params_str = params_ir.join(", ");

    // 函数定义头
    let mut s = String::new();
    if func.is_native {
        // 原生函数已作为 extern declare 生成，跳过
        ctx.exit_scope();
        return Ok(String::new());
    }

    s.push_str(&format!(
        "define {}{} @{}({}) {{\n",
        if ctx.blob_mode { "internal " } else { "" },
        ret_str, func.name, params_str
    ));

    // Debug 元数据：使用 emit_program 预注册的 DISubprogram ID（`!dbg !N`）
    let mut dbg_id: Option<u32> = None;
    if let Some(mid) = ctx.func_dbg_ids.get(&func.name) {
        dbg_id = Some(*mid);
    }

    // 为参数生成 alloca 并 store
    for (name, ty) in &params {
        let var_name = ctx.fresh_var();
        {
            let cur = blocks.last_mut();
            cur.body.push(format!("{} = alloca {}", var_name, ty));
            cur.body.push(format!(
                "store {} %arg.{} , {}* {}",
                ty,
                sanitizellvm(name),
                ty,
                var_name
            ));
        }
        ctx.declare_var(name, var_name.clone(), ty.clone());
    }

    // 生成函数体
    emit_block(ctx, &mut blocks, &func.body)?;

    // 若函数体没有终止符，追加 return
    if !blocks.has_terminator() {
        if ret_str == "void" {
            blocks.set_terminator("ret void");
        } else {
            blocks.set_terminator(&format!("ret {} {}", ret_str, zero_value(&ret_str)));
        }
    }

    // DWARF：在函数体首条指令追加 `, !dbg !N`（LLVM 指令级元数据需逗号分隔）
    if let Some(mid) = dbg_id {
        if let Some(bb) = blocks.blocks.first_mut() {
            if let Some(first) = bb.body.first_mut() {
                if !first.contains("!dbg") {
                    *first = format!("{}, !dbg !{}", first, mid);
                }
            }
        }
    }

    s.push_str(&blocks.to_string());
    s.push_str("}\n");

    ctx.exit_scope();
    Ok(s)
}

/// 基本块信息
struct BasicBlock {
    name: String,
    terminator: Option<String>,
    body: Vec<String>,
}

impl BasicBlock {
    fn new(name: String) -> Self {
        Self {
            name,
            terminator: None,
            body: Vec::new(),
        }
    }
}

/// 函数内的所有基本块
struct FuncBlocks {
    blocks: Vec<BasicBlock>,
}

impl FuncBlocks {
    fn new() -> Self {
        Self {
            blocks: Vec::new(),
        }
    }

    /// 添加基本块（返回名称）。添加后此块成为当前块。
    fn add_block_named(&mut self, name: &str) -> String {
        let name = name.to_string();
        let bb = BasicBlock::new(name.clone());
        self.blocks.push(bb);
        name
    }

    /// 获取最后一个基本块（当前块）的可变引用
    fn last_mut(&mut self) -> &mut BasicBlock {
        self.blocks.last_mut().unwrap()
    }

    /// 检查最后一个基本块是否有终止符
    fn has_terminator(&self) -> bool {
        self.blocks.last().map(|bb| bb.terminator.is_some()).unwrap_or(false)
    }

    /// 为最后一个基本块设置终止符
    fn set_terminator(&mut self, term: &str) {
        if let Some(bb) = self.blocks.last_mut() {
            bb.terminator = Some(term.to_string());
        }
    }

    fn to_string(&self) -> String {
        let mut s = String::new();
        for bb in &self.blocks {
            s.push_str(&format!("  {}:\n", bb.name));
            for instr in &bb.body {
                s.push_str("    ");
                s.push_str(instr);
                s.push('\n');
            }
            if let Some(ref term) = bb.terminator {
                s.push_str("    ");
                s.push_str(term);
                s.push('\n');
            }
        }
        s
    }
}

/// 生成一个 HIR 块（stmt 序列）
fn emit_block(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    block: &HirBlock,
) -> Result<(), AotError> {
    ctx.enter_scope();

    for stmt in &block.stmts {
        emit_statement(ctx, blocks, stmt)?;
    }

    ctx.exit_scope();
    Ok(())
}

/// 生成一条 HIR 语句
fn emit_statement(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    stmt: &HirStmt,
) -> Result<(), AotError> {
    match stmt {
        HirStmt::Val {
            name,
            ty,
            init,
        } => emit_variable_decl(ctx, blocks, name, ty, init)?,
        HirStmt::Var {
            name,
            ty,
            init,
        } => emit_variable_decl(ctx, blocks, name, ty, init)?,
        HirStmt::Assign {
            target,
            value,
        } => emit_assign(ctx, blocks, target, value)?,
        HirStmt::Expr(e) => {
            let _ = emit_expr_val(ctx, blocks, e)?;
        }
        HirStmt::Return(val) => {
            if let Some(v) = val {
                let (val_ir, val_ty) = emit_expr_val(ctx, blocks, v)?;
                let ret_ty = sanitize_ty_for_ret(&val_ty);
                blocks.set_terminator(&format!("ret {} {}", ret_ty, val_ir));
            } else {
                blocks.set_terminator("ret void");
            }
        }
        HirStmt::If {
            cond,
            then_b,
            else_b,
        } => emit_if_stmt(ctx, blocks, cond, then_b, else_b)?,
        HirStmt::While { cond, body } => emit_while_stmt(ctx, blocks, cond, body)?,
        HirStmt::Break => {
            blocks.set_terminator("; break (simplified)");
        }
        HirStmt::Continue => {
            blocks.set_terminator("; continue (simplified)");
        }
        HirStmt::Block(b) => {
            emit_block(ctx, blocks, b)?;
        }
        HirStmt::Defer(b) => {
            // P7.4: defer 简化处理 — 立即执行块（与 Block 一致）
            emit_block(ctx, blocks, b)?;
        }
    }
    Ok(())
}

/// 生成变量声明
fn emit_variable_decl(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    name: &str,
    ty: &Option<HirType>,
    init: &Option<HirExpr>,
) -> Result<(), AotError> {
    let llvm_ty = ty.as_ref().map(|t| ctx.llvm_type(t)).unwrap_or_else(|| "i32".to_string());

    let var_name = ctx.fresh_var();
    {
        let cur = blocks.last_mut();
        cur.body.push(format!("{} = alloca {}", var_name, llvm_ty));
    }

    if let Some(init) = init {
        let (val_ir, val_ty) = emit_expr_val(ctx, blocks, init)?;
        let cur = blocks.last_mut();
        cur.body.push(format!(
            "store {} {} , {}* {}",
            val_ty, val_ir, llvm_ty, var_name
        ));
    }

    ctx.declare_var(name, var_name, llvm_ty);
    Ok(())
}

/// 生成赋值语句
fn emit_assign(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    target: &HirExpr,
    value: &HirExpr,
) -> Result<(), AotError> {
    let (val_ir, val_ty) = emit_expr_val(ctx, blocks, value)?;

    match target {
        HirExpr::Var(name) => {
            if let Some(slot) = ctx.lookup_var(name) {
                let cur = blocks.last_mut();
                cur.body.push(format!(
                    "store {} {} , {}* {}",
                    val_ty, val_ir, slot.llvm_ty, slot.llvm_name
                ));
            }
        }
        HirExpr::Member {
            object,
            name,
        } => {
            emit_member_assign(ctx, blocks, object, name, val_ir, val_ty)?;
        }
        HirExpr::Index {
            container,
            index,
        } => {
            emit_index_assign(ctx, blocks, container, index, val_ir, val_ty)?;
        }
        _ => {
            return Err(AotError::UnsupportedExpr(format!(
                "不可赋值的左值: {:?}",
                target
            )));
        }
    }
    Ok(())
}

fn emit_member_assign(
    ctx: &mut EmitCtx,
    _blocks: &mut FuncBlocks,
    object: &HirExpr,
    _field: &str,
    val_ir: String,
    val_ty: String,
) -> Result<(), AotError> {
    // 完整实现需要结构体字段偏移表；这里通过 runtime 简化
    // 对象被视为 i8*，字段偏移为 0
    let (obj_ir, _) = emit_expr_val(ctx, _blocks, object)?;
    {
        let cur = _blocks.last_mut();
        cur.body.push(format!(
            "{} = getelementptr i8, i8* {}, i64 0",
            ctx.fresh_var(),
            obj_ir
        ));
        // 直接 store（实际应该 store 到 gep 结果）
        let _ = (val_ir, val_ty);
    }
    Ok(())
}

fn emit_index_assign(
    _ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    _container: &HirExpr,
    _index: &HirExpr,
    val_ir: String,
    val_ty: String,
) -> Result<(), AotError> {
    // 简化：直接 store 到临时变量
    let _ = (_ctx, _container, _index);
    let cur = blocks.last_mut();
    cur.body.push(format!("; store index {} {}", val_ty, val_ir));
    Ok(())
}

/// 生成 if 语句（无返回值）
fn emit_if_stmt(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    cond: &HirExpr,
    then_b: &HirBlock,
    else_b: &Option<HirBlock>,
) -> Result<(), AotError> {
    let then_name = ctx.fresh_bb("then");
    let else_name = ctx.fresh_bb("else");
    let merge_name = ctx.fresh_bb("merge");

    // 生成条件表达式
    let (cond_ir, cond_ty) = emit_expr_val(ctx, blocks, cond)?;
    let icmp_var = ctx.fresh_var();
    let pred = if cond_ty == "i1" {
        format!("icmp eq i1 {}, true", cond_ir)
    } else {
        format!("icmp ne {} {}, 0", cond_ty, cond_ir)
    };
    {
        let cur = blocks.last_mut();
        cur.body.push(format!("{} = {}", icmp_var, pred));
        cur.terminator = Some(format!(
            "br i1 {}, label %{}, label %{}",
            icmp_var, then_name, else_name
        ));
    }

    // Then 块
    let _ = blocks.add_block_named(&then_name);
    emit_block(ctx, blocks, then_b)?;
    if !blocks.has_terminator() {
        blocks.set_terminator(&format!("br label %{}", merge_name));
    }

    // Else 块
    let _ = blocks.add_block_named(&else_name);
    if let Some(else_block) = else_b {
        emit_block(ctx, blocks, else_block)?;
    }
    if !blocks.has_terminator() {
        blocks.set_terminator(&format!("br label %{}", merge_name));
    }

    // Merge 块（对应 §9.2.4）：后续语句在此块继续生成。
    // 若 then/else 均已终止（ret/br），merge 仍创建为空块并作为当前块，
    // 由调用方（emit_block）决定是否补充终止符。
    let _ = blocks.add_block_named(&merge_name);
    Ok(())
}

/// 生成 while 语句
fn emit_while_stmt(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    cond: &HirExpr,
    body: &HirBlock,
) -> Result<(), AotError> {
    let cond_name = ctx.fresh_bb("loop.cond");
    let body_name = ctx.fresh_bb("loop.body");
    let end_name = ctx.fresh_bb("loop.end");

    // 跳转至条件块
    blocks.set_terminator(&format!("br label %{}", cond_name));

    // 条件块
    let _ = blocks.add_block_named(&cond_name);
    let (cond_ir, cond_ty) = emit_expr_val(ctx, blocks, cond)?;
    let icmp_var = ctx.fresh_var();
    let pred = if cond_ty == "i1" {
        format!("icmp eq i1 {}, true", cond_ir)
    } else {
        format!("icmp ne {} {}, 0", cond_ty, cond_ir)
    };
    {
        let cur = blocks.last_mut();
        cur.body.push(format!("{} = {}", icmp_var, pred));
        cur.terminator = Some(format!(
            "br i1 {}, label %{}, label %{}",
            icmp_var, body_name, end_name
        ));
    }

    // 循环体块
    let _ = blocks.add_block_named(&body_name);
    emit_block(ctx, blocks, body)?;
    if !blocks.has_terminator() {
        blocks.set_terminator(&format!("br label %{}", cond_name));
    }

    // 循环结束块
    let _ = blocks.add_block_named(&end_name);
    Ok(())
}

/// 生成表达式，返回 (IR 值, IR 类型)
fn emit_expr_val(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    expr: &HirExpr,
) -> Result<(String, String), AotError> {
    match expr {
        HirExpr::Lit(lit) => emit_literal(ctx, blocks, lit),
        HirExpr::Var(name) => emit_variable_load(ctx, blocks, name),
        HirExpr::Binary {
            op,
            lhs,
            rhs,
        } => emit_binary(ctx, blocks, op, lhs, rhs),
        HirExpr::Unary {
            op,
            operand,
        } => emit_unary(ctx, blocks, op, operand),
        HirExpr::Call {
            callee,
            args,
        } => emit_call(ctx, blocks, callee, args),
        HirExpr::Member {
            object,
            name,
        } => emit_member_access(ctx, blocks, object, name),
        HirExpr::Index {
            container,
            index,
        } => emit_index_access(ctx, blocks, container, index),
        HirExpr::New {
            type_name,
            args,
        } => emit_new(ctx, blocks, type_name, args),
        HirExpr::If {
            cond,
            then_e,
            else_e,
        } => emit_if_expr(ctx, blocks, cond, then_e, else_e),
        HirExpr::Block(block) => emit_block_expr(ctx, blocks, block),
        HirExpr::Box(inner) => {
            // P7.5: 堆分配 — AOT 直接透传值（堆管理由运行时处理）
            emit_expr_val(ctx, blocks, inner)
        }
        HirExpr::WeakRef(inner) => {
            // P7.3: 弱引用 — 求值内部表达式
            emit_expr_val(ctx, blocks, inner)
        }
        HirExpr::Await(inner) => {
            // P10.1: await — 直接返回内部值（非协程上下文 no-op）
            emit_expr_val(ctx, blocks, inner)
        }
        // P3.3: Lambda — 生成静态函数并返回函数指针
        // Phase 5: 支持捕获变量
        HirExpr::Lambda {
            params,
            body,
        } => {
            let func_name = format!("__lambda_{}", ctx.lambda_counter);
            ctx.lambda_counter += 1;

            // Phase 5: 收集捕获变量（自由变量）
            let mut captures: Vec<String> = Vec::new();
            for stmt in &body.stmts {
                collect_free_vars_in_stmt(stmt, params, &mut captures);
            }

            // 生成参数列表（用户参数 + 捕获参数）
            let mut param_strs: Vec<String> = Vec::new();
            for (i, p) in params.iter().enumerate() {
                let ty =
                    p.ty.as_ref().map(|t| ctx.llvm_type(t)).unwrap_or_else(|| "i32".to_string());
                param_strs.push(format!("{} %arg_{}", ty, i));
            }
            // 捕获参数作为额外参数
            for (i, cap) in captures.iter().enumerate() {
                param_strs.push(format!("ptr %cap_{}", i));
            }

            // 生成函数体
            let mut func_ir = String::new();
            func_ir.push_str(&format!(
                "define i32 @{}({}) {{\n",
                func_name,
                param_strs.join(", ")
            ));
            func_ir.push_str("entry:\n");

            // 用子上下文发射函数体
            let mut sub_ctx = EmitCtx {
                type_mapper: ctx.type_mapper.clone(),
                target_triple: ctx.target_triple.clone(),
                _opt_level: ctx._opt_level,
                _string_as_struct: ctx._string_as_struct,
                link_runtime: ctx.link_runtime,
                debug_info: None,
                blob_mode: ctx.blob_mode,
                declared_structs: std::collections::HashSet::new(),
                generated_funcs: std::collections::HashSet::new(),
                sections: Vec::new(),
                bb_counter: 0,
                var_counter: 0,
                const_counter: 0,
                var_scope: vec![HashMap::new()],
                globals: Vec::new(),
                global_const_map: HashMap::new(),
                subprogram_meta: Vec::new(),
                subprogram_index: 0,
                func_dbg_ids: HashMap::new(),
                func_ret_types: ctx.func_ret_types.clone(),
                enum_variants: ctx.enum_variants.clone(),
                enum_max_fields: ctx.enum_max_fields.clone(),
                lambda_counter: ctx.lambda_counter,
                lambda_funcs: Vec::new(),
            };

            // 注册参数
            for (i, p) in params.iter().enumerate() {
                let ty =
                    p.ty.as_ref()
                        .map(|t| sub_ctx.llvm_type(t))
                        .unwrap_or_else(|| "i32".to_string());
                sub_ctx.var_scope.last_mut().unwrap().insert(
                    p.name.clone(),
                    VarSlot {
                        llvm_name: format!("%arg_{}", i),
                        llvm_ty: ty,
                    },
                );
            }
            // 注册捕获变量
            for (i, cap) in captures.iter().enumerate() {
                sub_ctx.var_scope.last_mut().unwrap().insert(
                    cap.clone(),
                    VarSlot {
                        llvm_name: format!("%cap_{}", i),
                        llvm_ty: "ptr".to_string(),
                    },
                );
            }

            // 发射函数体
            let mut fb = FuncBlocks::new();
            fb.add_block_named("entry");
            let _ = emit_block(&mut sub_ctx, &mut fb, body);
            if !fb.has_terminator() {
                fb.set_terminator("ret i32 0");
            }

            // 收集 IR
            for blk in fb.blocks.iter() {
                if blk.name != "entry" {
                    func_ir.push_str(&format!("{}:\n", blk.name));
                }
                for inst in &blk.body {
                    func_ir.push_str(inst);
                    func_ir.push('\n');
                }
                if let Some(ref term) = blk.terminator {
                    func_ir.push_str(term);
                    func_ir.push('\n');
                }
            }
            func_ir.push_str("}\n\n");

            ctx.lambda_funcs.push(func_ir);

            // Phase 5: 返回闭包结构体（函数指针 + 捕获值）
            let closure_type = format!("{{ ptr, {} x ptr }}", captures.len());
            let closure_name = ctx.fresh_var();
            let cur = blocks.last_mut();

            // 分配闭包结构体
            cur.body.push(format!("{} = alloca {}", closure_name, closure_type));

            // 存储函数指针
            cur.body.push(format!("store ptr @{}, ptr {}[0]", func_name, closure_name));

            // 存储捕获值
            for (i, cap) in captures.iter().enumerate() {
                // 加载捕获变量的值
                let cap_addr = ctx.lookup_var(cap).map(|v| v.llvm_name.clone()).unwrap_or_default();
                if !cap_addr.is_empty() {
                    cur.body.push(format!(
                        "store ptr {}, ptr {}[1][{}]",
                        cap_addr, closure_name, i
                    ));
                } else {
                    // 未找到捕获变量，存储空指针
                    cur.body.push(format!("store ptr null, ptr {}[1][{}]", closure_name, i));
                }
            }

            Ok((closure_name, closure_type))
        }
    }
}

fn emit_literal(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    lit: &crate::ast::Literal,
) -> Result<(String, String), AotError> {
    match lit {
        crate::ast::Literal::Int(v) => {
            let name = ctx.fresh_const("int");
            let cur = blocks.last_mut();
            cur.body.push(format!("{} = add i32 0, {}", name, v));
            Ok((name, "i32".to_string()))
        }
        crate::ast::Literal::Float(v) => {
            let name = ctx.fresh_const("float");
            let cur = blocks.last_mut();
            // LLVM 浮点字面量必须含小数点/指数（Rust 的 {} 会把 2.0 打印成 2）
            let lit = if v.is_finite()
                && !format!("{}", v).contains('.')
                && !format!("{}", v).contains('e')
                && !format!("{}", v).contains('E')
            {
                format!("{}.0", v)
            } else {
                format!("{}", v)
            };
            cur.body.push(format!("{} = fadd float 0.0, {}", name, lit));
            Ok((name, "float".to_string()))
        }
        crate::ast::Literal::Bool(v) => {
            Ok((format!("{}", if *v { 1 } else { 0 }), "i1".to_string()))
        }
        crate::ast::Literal::Null => Ok(("null".to_string(), "i8*".to_string())),
        crate::ast::Literal::String(s) => emit_string_literal(ctx, blocks, s),
        crate::ast::Literal::Char(c) => {
            let v = *c as i64;
            Ok((format!("{}", v), "i16".to_string()))
        }
    }
}

fn emit_string_literal(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    s: &str,
) -> Result<(String, String), AotError> {
    // 字符串字面量 → 模块级全局常量
    //
    // LLVM IR 的 `c"..."` 字符串常量**不含**隐式 NUL（实测 LLVM 23）：
    // `c"hello"` 即 5 字节 `[h,e,l,l,o]`。需要 C 风格 NUL 结尾时须显式追加
    // `\00`（单个转义 = 1 字节）：`c"hello\00"` 为 6 字节。
    // 此处保留显式 `\00` 结尾（供 CStr/FFI 使用），数组长度 = s.len() + 1。
    let escaped = s
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    let llvm_str = format!("c\"{}\\00\"", escaped);
    let byte_len = s.len() + 1;
    let key = format!("[{} x i8] {}", byte_len, llvm_str);

    // 模块级全局字符串常量（去重后只定义一次，对应 §9.2.1 generate_globals）。
    // 全局量使用 `@` 前缀（LLVM 全局标识符），与函数局部 `%` 寄存器区分。
    let data_name = if let Some(existing) = ctx.global_const_map.get(&key) {
        existing.clone()
    } else {
        let name = format!("@str_data.{}", ctx.const_counter);
        ctx.const_counter += 1;
        ctx.globals.push(format!(
            "{} = private constant [{} x i8] {}",
            name, byte_len, llvm_str
        ));
        ctx.global_const_map.insert(key, name.clone());
        name
    };

    // 函数体内：取全局常量地址
    let gep_name = ctx.fresh_const("str_gep");
    let cur = blocks.last_mut();
    cur.body.push(format!(
        "{} = getelementptr [{} x i8], [{} x i8]* {}, i64 0, i64 0",
        gep_name, byte_len, byte_len, data_name
    ));
    Ok((gep_name, "i8*".to_string()))
}

fn emit_variable_load(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    name: &str,
) -> Result<(String, String), AotError> {
    if let Some(slot) = ctx.lookup_var(name).cloned() {
        let tmp = ctx.fresh_var();
        let cur = blocks.last_mut();
        cur.body.push(format!(
            "{} = load {}, {}* {}",
            tmp, slot.llvm_ty, slot.llvm_ty, slot.llvm_name
        ));
        Ok((tmp, slot.llvm_ty))
    } else {
        // 未声明变量：作为外部引用（可能是函数调用）
        Ok((name.to_string(), "i32".to_string()))
    }
}

fn emit_binary(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    op: &HirBinOp,
    lhs: &HirExpr,
    rhs: &HirExpr,
) -> Result<(String, String), AotError> {
    let (l_ir, l_ty) = emit_expr_val(ctx, blocks, lhs)?;
    let (r_ir, _r_ty) = emit_expr_val(ctx, blocks, rhs)?;
    let tmp = ctx.fresh_var();
    let cur = blocks.last_mut();

    match op {
        HirBinOp::Add => {
            if l_ty.starts_with("float") || l_ty == "double" {
                let op = if l_ty == "double" { "fadd double" } else { "fadd float" };
                cur.body.push(format!("{} = {} {}, {}", tmp, op, l_ir, r_ir));
            } else {
                cur.body.push(format!("{} = add {} {}, {}", tmp, l_ty, l_ir, r_ir));
            }
            Ok((tmp, l_ty))
        }
        HirBinOp::Sub => {
            if l_ty.starts_with("float") || l_ty == "double" {
                let op = if l_ty == "double" { "fsub double" } else { "fsub float" };
                cur.body.push(format!("{} = {} {}, {}", tmp, op, l_ir, r_ir));
            } else {
                cur.body.push(format!("{} = sub {} {}, {}", tmp, l_ty, l_ir, r_ir));
            }
            Ok((tmp, l_ty))
        }
        HirBinOp::Mul => {
            if l_ty.starts_with("float") || l_ty == "double" {
                let op = if l_ty == "double" { "fmul double" } else { "fmul float" };
                cur.body.push(format!("{} = {} {}, {}", tmp, op, l_ir, r_ir));
            } else {
                cur.body.push(format!("{} = mul {} {}, {}", tmp, l_ty, l_ir, r_ir));
            }
            Ok((tmp, l_ty))
        }
        HirBinOp::Div => {
            if l_ty.starts_with("float") || l_ty == "double" {
                let op = if l_ty == "double" { "fdiv double" } else { "fdiv float" };
                cur.body.push(format!("{} = {} {}, {}", tmp, op, l_ir, r_ir));
            } else {
                cur.body.push(format!("{} = sdiv {} {}, {}", tmp, l_ty, l_ir, r_ir));
            }
            Ok((tmp, l_ty))
        }
        HirBinOp::Rem => {
            cur.body.push(format!("{} = srem {} {}, {}", tmp, l_ty, l_ir, r_ir));
            Ok((tmp, l_ty))
        }
        HirBinOp::Eq | HirBinOp::Ne | HirBinOp::Lt | HirBinOp::Gt | HirBinOp::Le | HirBinOp::Ge => {
            let (icmp_pred, is_signed) = match op {
                HirBinOp::Eq => ("eq", true),
                HirBinOp::Ne => ("ne", true),
                HirBinOp::Lt => ("slt", true),
                HirBinOp::Gt => ("sgt", true),
                HirBinOp::Le => ("sle", true),
                HirBinOp::Ge => ("sge", true),
                _ => unreachable!(),
            };
            let pred = if is_signed && l_ty.starts_with("i") {
                format!("icmp {} {} {}, {}", icmp_pred, l_ty, l_ir, r_ir)
            } else {
                format!("fcmp one {} {}, {}", l_ty, l_ir, r_ir)
            };
            cur.body.push(format!("{} = {}", tmp, pred));
            Ok((tmp, "i1".to_string()))
        }
        HirBinOp::And | HirBinOp::Or => {
            let op = if *op == HirBinOp::And { "and" } else { "or" };
            cur.body.push(format!("{} = {} i1 {}, {}", tmp, op, l_ir, r_ir));
            Ok((tmp, "i1".to_string()))
        }
        HirBinOp::BitAnd | HirBinOp::BitOr | HirBinOp::BitXor => {
            let op = match op {
                HirBinOp::BitAnd => "and",
                HirBinOp::BitOr => "or",
                HirBinOp::BitXor => "xor",
                _ => unreachable!(),
            };
            cur.body.push(format!("{} = {} {} {}, {}", tmp, op, l_ty, l_ir, r_ir));
            Ok((tmp, l_ty))
        }
        HirBinOp::Shl | HirBinOp::Shr => {
            let op = if *op == HirBinOp::Shl { "shl" } else { "ashr" };
            cur.body.push(format!("{} = {} {} {}, {}", tmp, op, l_ty, l_ir, r_ir));
            Ok((tmp, l_ty))
        }
        _ => Ok((l_ir, l_ty)),
    }
}

fn emit_unary(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    op: &HirUnOp,
    operand: &HirExpr,
) -> Result<(String, String), AotError> {
    let (v_ir, v_ty) = emit_expr_val(ctx, blocks, operand)?;
    let tmp = ctx.fresh_var();
    let cur = blocks.last_mut();

    match op {
        HirUnOp::Minus => {
            cur.body.push(format!("{} = sub {} {}, {}", tmp, v_ty, 0, v_ir));
            Ok((tmp, v_ty))
        }
        HirUnOp::Not => {
            cur.body.push(format!("{} = xor i1 {}, 1", tmp, v_ir));
            Ok((tmp, "i1".to_string()))
        }
    }
}

fn emit_call(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    callee: &str,
    args: &[HirExpr],
) -> Result<(String, String), AotError> {
    // P9: 检查是否为结构体构造函数
    if ctx.declared_structs.contains(callee) {
        return emit_struct_constructor(ctx, blocks, callee, args);
    }
    
    let args_ir: Vec<(String, String)> =
        args.iter().map(|a| emit_expr_val(ctx, blocks, a)).collect::<Result<_, _>>()?;

    let ret_ty = ctx.func_ret_types.get(callee).cloned().unwrap_or_else(|| "i32".to_string());
    let cur = blocks.last_mut();
    let args_str: Vec<String> = args_ir.iter().map(|(v, t)| format!("{} {}", t, v)).collect();

    // 处理 void / 空返回类型：不能赋值给寄存器（LLVM IR 语法限制）
    if ret_ty.is_empty() || ret_ty == "void" {
        cur.body.push(format!("call void @{}({})", callee, args_str.join(", ")));
        // 返回一个虚拟 i32 0 值，保持调用者接口兼容
        Ok(("0".to_string(), "i32".to_string()))
    } else {
        let tmp = ctx.fresh_var();
        cur.body.push(format!(
            "{} = call {} @{}({})",
            tmp,
            ret_ty,
            callee,
            args_str.join(", ")
        ));
        Ok((tmp, ret_ty))
    }
}

/// P9: 生成结构体构造函数代码
fn emit_struct_constructor(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    struct_name: &str,
    args: &[HirExpr],
) -> Result<(String, String), AotError> {
    let struct_type = ctx.type_mapper.map(&HirType::Named(struct_name.to_string()));
    let args_ir: Vec<(String, String)> =
        args.iter().map(|a| emit_expr_val(ctx, blocks, a)).collect::<Result<_, _>>()?;
    let cur = blocks.last_mut();
    let mut struct_val = "undef".to_string();
    for (i, (val, ty)) in args_ir.iter().enumerate() {
        let new_val = ctx.fresh_var();
        cur.body.push(format!(
            "{} = insertvalue {} {}, {} {}, {}",
            new_val, struct_type, struct_val, ty, val, i
        ));
        struct_val = new_val.clone();
    }
    Ok((struct_val, struct_type))
}

fn emit_member_access(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    object: &HirExpr,
    name: &str,
) -> Result<(String, String), AotError> {
    // P3.2: 枚举变体构造 — Direction.North → tagged union 结构体
    if let HirExpr::Var(enum_name) = object {
        // 先提取枚举信息（避免 borrow checker 冲突）
        let (variant_idx, field_count, max_fields) = {
            let variants = ctx.enum_variants.get(enum_name);
            let mf = ctx.enum_max_fields.get(enum_name).copied().unwrap_or(0);
            if let Some(vs) = variants {
                if let Some((_, vi, fc)) = vs.iter().find(|(vn, _, _)| vn == name) {
                    (Some(*vi), *fc, mf)
                } else {
                    (None, 0, mf)
                }
            } else {
                (None, 0, mf)
            }
        };

        if let Some(variant_idx) = variant_idx {
            let struct_name = format!("%struct.{}", sanitizellvm(enum_name));

            // 分配 tagged union 结构体
            let alloc = ctx.fresh_var();
            let cur = blocks.last_mut();
            cur.body.push(format!("{} = alloca {}", alloc, struct_name));

            // 设置 tag 字段（变体索引）
            let tag_ptr = ctx.fresh_var();
            cur.body.push(format!(
                "{} = getelementptr {}, {}* {}, i64 0, i32 0",
                tag_ptr, struct_name, struct_name, alloc
            ));
            cur.body.push(format!("store i32 {}, i32* {}", variant_idx, tag_ptr));

            // 设置关联值字段：置零
            for fi in 0..max_fields {
                let f_ptr = ctx.fresh_var();
                cur.body.push(format!(
                    "{} = getelementptr {}, {}* {}, i64 0, i32 {}",
                    f_ptr,
                    struct_name,
                    struct_name,
                    alloc,
                    fi + 1
                ));
                cur.body.push(format!("store ptr null, ptr* {}", f_ptr));
            }

            return Ok((alloc.clone(), format!("{}*", struct_name)));
        }
    }

    // 普通成员访问：字段偏移为 0
    let (obj_ir, _) = emit_expr_val(ctx, blocks, object)?;
    let gep = ctx.fresh_var();
    let tmp = ctx.fresh_var();
    let cur = blocks.last_mut();
    cur.body.push(format!("{} = getelementptr i8, i8* {}, i64 0", gep, obj_ir));
    cur.body.push(format!("{} = load i32, i32* {}", tmp, gep));
    Ok((tmp, "i32".to_string()))
}

fn emit_index_access(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    container: &HirExpr,
    index: &HirExpr,
) -> Result<(String, String), AotError> {
    let (c_ir, _) = emit_expr_val(ctx, blocks, container)?;
    let (i_ir, _) = emit_expr_val(ctx, blocks, index)?;
    let gep = ctx.fresh_var();
    let tmp = ctx.fresh_var();
    let cur = blocks.last_mut();
    cur.body.push(format!(
        "{} = getelementptr i32, i32* {}, i64 {}",
        gep, c_ir, i_ir
    ));
    cur.body.push(format!("{} = load i32, i32* {}", tmp, gep));
    Ok((tmp, "i32".to_string()))
}

fn emit_new(
    ctx: &mut EmitCtx,
    _blocks: &mut FuncBlocks,
    _type_name: &str,
    args: &[HirExpr],
) -> Result<(String, String), AotError> {
    // 简化：返回 null 指针
    let _args_str: Vec<(String, String)> =
        args.iter().map(|a| emit_expr_val(ctx, _blocks, a)).collect::<Result<_, _>>()?;
    Ok(("null".to_string(), "i8*".to_string()))
}

fn emit_if_expr(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    cond: &HirExpr,
    then_e: &HirExpr,
    else_e: &HirExpr,
) -> Result<(String, String), AotError> {
    let then_name = ctx.fresh_bb("then");
    let else_name = ctx.fresh_bb("else");
    let merge_name = ctx.fresh_bb("merge");

    let (cond_ir, cond_ty) = emit_expr_val(ctx, blocks, cond)?;
    let icmp_var = ctx.fresh_var();
    let pred = if cond_ty == "i1" {
        format!("icmp eq i1 {}, true", cond_ir)
    } else {
        format!("icmp ne {} {}, 0", cond_ty, cond_ir)
    };
    {
        let cur = blocks.last_mut();
        cur.body.push(format!("{} = {}", icmp_var, pred));
        cur.terminator = Some(format!(
            "br i1 {}, label %{}, label %{}",
            icmp_var, then_name, else_name
        ));
    }

    // Then 块
    let _ = blocks.add_block_named(&then_name);
    let (then_ir, then_ty) = emit_expr_val(ctx, blocks, then_e)?;
    let then_phi = ctx.fresh_var();
    {
        let cur = blocks.last_mut();
        cur.body.push(format!("{} = add {} {}, 0", then_phi, then_ty, then_ir));
        cur.terminator = Some(format!("br label %{}", merge_name));
    }

    // Else 块
    let _ = blocks.add_block_named(&else_name);
    let (else_ir, else_ty) = emit_expr_val(ctx, blocks, else_e)?;
    let else_phi = ctx.fresh_var();
    {
        let cur = blocks.last_mut();
        cur.body.push(format!("{} = add {} {}, 0", else_phi, else_ty, else_ir));
        cur.terminator = Some(format!("br label %{}", merge_name));
    }

    // Merge 块 + PHI 节点
    let _ = blocks.add_block_named(&merge_name);
    let phi_name = ctx.fresh_var();
    let phi_type = then_ty;
    {
        let cur = blocks.last_mut();
        cur.body.push(format!(
            "{} = phi {}, [{} %{}], [{} %{}]",
            phi_name, phi_type, then_phi, then_name, else_phi, else_name
        ));
    }

    Ok((phi_name, phi_type))
}

fn emit_block_expr(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    block: &HirBlock,
) -> Result<(String, String), AotError> {
    let last_name = ctx.fresh_bb("block.last");
    blocks.set_terminator(&format!("br label %{}", last_name));

    let _ = blocks.add_block_named(&last_name);
    ctx.enter_scope();

    for (i, stmt) in block.stmts.iter().enumerate() {
        if i + 1 == block.stmts.len() {
            match stmt {
                HirStmt::Expr(e) => {
                    let (v, t) = emit_expr_val(ctx, blocks, e)?;
                    ctx.exit_scope();
                    return Ok((v, t));
                }
                HirStmt::Return(Some(e)) => {
                    let (v, t) = emit_expr_val(ctx, blocks, e)?;
                    blocks.set_terminator(&format!("ret {} {}", sanitize_ty_for_ret(&t), v));
                    ctx.exit_scope();
                    return Ok((v, t));
                }
                _ => {
                    emit_statement(ctx, blocks, stmt)?;
                }
            }
        } else {
            emit_statement(ctx, blocks, stmt)?;
        }
    }

    ctx.exit_scope();
    Ok(("0".to_string(), "i32".to_string()))
}

fn sanitize_ty_for_ret(ty: &str) -> &str {
    if ty.is_empty() || ty == "void" { "void" } else { ty }
}

fn zero_value(ty: &str) -> &str {
    match ty {
        "i1" => "false",
        "i8" | "i16" | "i32" | "i64" | "i128" => "0",
        "float" => "0.0",
        "double" => "0.0",
        _ => "null",
    }
}

/// 若程序没有 main 函数，合成一个
fn synthesize_main(funcs: &[HirFunction]) -> HirFunction {
    let call_target = funcs
        .iter()
        .find(|f| {
            f.ret.as_ref().map(|t| matches!(t, HirType::Named(n) if n == "Int")).unwrap_or(false)
        })
        .map(|f| f.name.clone())
        .unwrap_or_else(|| "println".to_string());

    HirFunction {
        name: "main".into(),
        params: vec![],
        ret: Some(HirType::Named("Int".into())),
        body: HirBlock {
            stmts: vec![
                HirStmt::Return(Some(HirExpr::Call {
                    callee: call_target,
                    args: vec![],
                })),
            ],
        },
        is_native: false,
        type_params: vec![],
        ffi_abi: FfiAbi::None,
        ffi_lib: None,
    }
}

/// Phase 5: 收集 Lambda body 中的自由变量（捕获变量）
fn collect_free_vars(
    expr: &HirExpr,
    params: &[crate::codegen::hir::HirParam],
    captures: &mut Vec<String>,
) {
    // 构建参数名集合
    let param_names: std::collections::HashSet<&str> =
        params.iter().map(|p| p.name.as_str()).collect();

    match expr {
        HirExpr::Var(name) => {
            if !param_names.contains(name.as_str()) && !captures.contains(name) && !is_builtin(name)
            {
                captures.push(name.clone());
            }
        }
        HirExpr::Binary {
            lhs, rhs, ..
        } => {
            collect_free_vars(lhs, params, captures);
            collect_free_vars(rhs, params, captures);
        }
        HirExpr::Unary {
            operand, ..
        } => {
            collect_free_vars(operand, params, captures);
        }
        HirExpr::Call {
            callee,
            args,
            ..
        } => {
            if !is_builtin(callee) {
                collect_free_vars(&HirExpr::Var(callee.clone()), params, captures);
            }
            for arg in args {
                collect_free_vars(arg, params, captures);
            }
        }
        HirExpr::Member { object, .. } => {
            collect_free_vars(object, params, captures);
        }
        HirExpr::Index {
            container,
            index,
            ..
        } => {
            collect_free_vars(container, params, captures);
            collect_free_vars(index, params, captures);
        }
        HirExpr::Block(block) => {
            for stmt in &block.stmts {
                collect_free_vars_in_stmt(stmt, params, captures);
            }
        }
        HirExpr::Lambda {
            params: inner_params,
            body,
            ..
        } => {
            // 嵌套 Lambda：用其参数名作为局部变量（简化处理，直接收集）
            let _ = inner_params;
            for stmt in &body.stmts {
                collect_free_vars_in_stmt(stmt, params, captures);
            }
        }
        HirExpr::If {
            cond,
            then_e,
            else_e,
        } => {
            collect_free_vars(cond, params, captures);
            collect_free_vars(then_e, params, captures);
            collect_free_vars(else_e, params, captures);
        }
        _ => {}
    }
}

/// Phase 5: 在语句中收集自由变量
fn collect_free_vars_in_stmt(
    stmt: &HirStmt,
    params: &[crate::codegen::hir::HirParam],
    captures: &mut Vec<String>,
) {
    match stmt {
        HirStmt::Val { init, .. } | HirStmt::Var { init, .. } => {
            if let Some(e) = init {
                collect_free_vars(e, params, captures);
            }
        }
        HirStmt::Assign {
            target,
            value,
        } => {
            collect_free_vars(target, params, captures);
            collect_free_vars(value, params, captures);
        }
        HirStmt::Expr(e) => {
            collect_free_vars(e, params, captures);
        }
        HirStmt::Return(Some(e)) => {
            collect_free_vars(e, params, captures);
        }
        HirStmt::If {
            cond,
            then_b,
            else_b,
            ..
        } => {
            collect_free_vars(cond, params, captures);
            for stmt in &then_b.stmts {
                collect_free_vars_in_stmt(stmt, params, captures);
            }
            if let Some(b) = else_b {
                for stmt in &b.stmts {
                    collect_free_vars_in_stmt(stmt, params, captures);
                }
            }
        }
        HirStmt::While {
            cond, body, ..
        } => {
            collect_free_vars(cond, params, captures);
            for stmt in &body.stmts {
                collect_free_vars_in_stmt(stmt, params, captures);
            }
        }
        HirStmt::Block(block) => {
            for stmt in &block.stmts {
                collect_free_vars_in_stmt(stmt, params, captures);
            }
        }
        _ => {}
    }
}

/// Phase 5: 检查是否为内置函数/变量
fn is_builtin(name: &str) -> bool {
    matches!(
        name,
        "println"
            | "print"
            | "puts"
            | "abs"
            | "sqrt"
            | "pow"
            | "toInt"
            | "toFloat"
            | "toStr"
            | "toString"
            | "clock"
            | "strlen"
            | "malloc"
            | "free"
            | "true"
            | "false"
            | "null"
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// Phase 1 AOT Blob: JitValue ABI 包装函数生成（设计文档 §6.3-§6.5）
// ─────────────────────────────────────────────────────────────────────────────

/// JitValue 标签常量（与 vm/abi.rs 保持一致）
const TAG_INT: u8 = 0;
const TAG_FLOAT: u8 = 1;
const TAG_BOOL: u8 = 2;
const TAG_NULL: u8 = 3;

/// Phase 1 参数个数上限
const MAX_AOT_ARGS: usize = 8;

/// 将 Aura HIR 类型映射为 JitValue 标签
///
/// Phase 1 仅支持 Int/Float/Bool/Unit（及其别名 Long/Double/Short/Byte 等）。
fn map_type_to_tag(ty: &HirType) -> Result<u8, AotError> {
    match ty {
        HirType::Named(name) => match name.as_str() {
            "Int" | "Long" | "Short" | "Byte" | "U8" | "Char" => Ok(TAG_INT),
            "Float" | "Double" => Ok(TAG_FLOAT),
            "Boolean" | "Bool" => Ok(TAG_BOOL),
            "Unit" | "Void" | "Nothing" => Ok(TAG_NULL),
            other => Err(AotError::UnsupportedExpr(format!(
                "Phase 1 AOT 不支持类型 '{}'（仅支持 Int/Float/Bool/Unit）",
                other
            ))),
        },
        HirType::Nullable(_) | HirType::Pointer(_) | HirType::Function { .. } => {
            Err(AotError::UnsupportedExpr(
                "Phase 1 AOT 不支持复杂类型（仅支持标量类型）".to_string(),
            ))
        }
        HirType::Unknown => Err(AotError::UnsupportedExpr(
            "Phase 1 AOT 不支持 Unknown 类型".to_string(),
        )),
    }
}

/// 获取 Aura 类型对应的 LLVM 类型字符串
fn aura_type_to_llvm(ty: &HirType) -> String {
    match ty {
        HirType::Named(name) => match name.as_str() {
            "Int" => "i32".to_string(),
            "Long" => "i64".to_string(),
            "Short" => "i16".to_string(),
            "Byte" | "U8" => "i8".to_string(),
            "Char" => "i16".to_string(),
            "Float" => "float".to_string(),
            "Double" => "double".to_string(),
            "Boolean" | "Bool" => "i1".to_string(),
            "Unit" | "Void" | "Nothing" => "void".to_string(),
            _ => "i32".to_string(), // fallback
        },
        _ => "i32".to_string(),
    }
}

/// 为 Aura 函数生成 JitValue ABI 包装函数
///
/// 包装函数签名：`define internal i64 @aura_aot_<name>!<meta>(i64* %args, i64* %ret, i64 %argc, i64* %ctx)`
///
/// 参数布局（JitValue 数组展开为 i64 数组）：
/// - `args[2*i]` = 参数 i 的 tag
/// - `args[2*i+1]` = 参数 i 的 payload
///
/// 返回值：
/// - `ret[0]` = 返回值的 tag
/// - `ret[1]` = 返回值的 payload
///
/// 符号名格式：`aura_aot_<sanitized_name>!<nargs>!<rettag>!<tag0>!<tag1>!...`
fn emit_wrapper(ctx: &mut EmitCtx, func: &HirFunction) -> Result<String, AotError> {
    // 1. 检查参数个数
    if func.params.len() > MAX_AOT_ARGS {
        return Err(AotError::UnsupportedExpr(format!(
            "函数 '{}' 参数个数 {} 超过上限 {}（Phase 1 限制）",
            func.name,
            func.params.len(),
            MAX_AOT_ARGS
        )));
    }

    // 2. 映射参数类型到标签
    let param_tags: Vec<u8> = func
        .params
        .iter()
        .map(|p| map_type_to_tag(p.ty.as_ref().unwrap_or(&HirType::Named("Int".into()))))
        .collect::<Result<_, _>>()?;

    // 3. 映射返回类型到标签
    let ret_tag = match &func.ret {
        Some(ty) => map_type_to_tag(ty)?,
        None => TAG_NULL,
    };

    // 4. 构建包装函数名（含元数据）
    let func_part = format!("aura_aot_{}", sanitizellvm(&func.name));
    let mut name_parts = vec![
        func_part.clone(),
        param_tags.len().to_string(),
        ret_tag.to_string(),
    ];
    for tag in &param_tags {
        name_parts.push(tag.to_string());
    }
    let wrapper_name = name_parts.join("!");

    // 5. 获取真实函数的 LLVM 返回类型和参数类型
    let ret_llvm_ty = func
        .ret
        .as_ref()
        .map(|t| ctx.llvm_type(t))
        .unwrap_or_else(|| "void".to_string());

    let param_llvm_types: Vec<String> = func
        .params
        .iter()
        .map(|p| {
            ctx.llvm_type(p.ty.as_ref().unwrap_or(&HirType::Named("Int".into())))
        })
        .collect();

    // 6. 生成包装函数体
    let mut s = String::new();
    s.push_str(&format!(
        "define internal i64 @\"{}\"(i64* %args, i64* %ret, i64 %argc, i64* %ctx) {{\n",
        wrapper_name
    ));
    s.push_str("entry:\n");

    // 6a. 解包参数
    let mut call_args: Vec<String> = Vec::new();
    for (i, (llvm_ty, tag)) in param_llvm_types.iter().zip(param_tags.iter()).enumerate() {
        let payload_idx = (i * 2 + 1).to_string();
        let gep_var = format!("%arg{}_gep", i);
        let load_var = format!("%arg{}_payload", i);
        let val_var = format!("%arg{}_val", i);

        // 加载 payload: args[2*i+1]
        s.push_str(&format!(
            "  {} = getelementptr i64, i64* %args, i64 {}\n",
            gep_var, payload_idx
        ));
        s.push_str(&format!(
            "  {} = load i64, i64* {}\n",
            load_var, gep_var
        ));

        // 根据类型转换 payload
        match *tag {
            TAG_INT => {
                // i64 → target type (trunc/zext)
                let target_bits = llvm_ty
                    .trim_start_matches('i')
                    .parse::<usize>()
                    .unwrap_or(32);
                if target_bits < 64 {
                    s.push_str(&format!(
                        "  {} = trunc i64 {} to {}\n",
                        val_var, load_var, llvm_ty
                    ));
                } else {
                    // i64 → i64 (no conversion needed)
                    s.push_str(&format!("  {} = add i64 {}, 0\n", val_var, load_var));
                }
            }
            TAG_FLOAT => {
                // i64 → double (bitcast) → float (fptrunc) if needed
                let bitcast_var = format!("%arg{}_f64", i);
                s.push_str(&format!(
                    "  {} = bitcast i64 {} to double\n",
                    bitcast_var, load_var
                ));
                if llvm_ty == "float" {
                    s.push_str(&format!(
                        "  {} = fptrunc double {} to float\n",
                        val_var, bitcast_var
                    ));
                } else {
                    // double → no conversion needed
                    s.push_str(&format!(
                        "  {} = fadd double {}, 0.0\n",
                        val_var, bitcast_var
                    ));
                }
            }
            TAG_BOOL => {
                // i64 → i1 (trunc)
                s.push_str(&format!(
                    "  {} = trunc i64 {} to i1\n",
                    val_var, load_var
                ));
            }
            TAG_NULL => {
                // void: no parameter
                continue;
            }
            _ => unreachable!(),
        }
        call_args.push(format!("{} {}", llvm_ty, val_var));
    }

    // 6b. 调用真实函数
    let call_var = ctx.fresh_var();
    let args_str = call_args.join(", ");
    if ret_llvm_ty == "void" || ret_llvm_ty.is_empty() {
        s.push_str(&format!(
            "  call void @{}({})\n",
            func.name, args_str
        ));
    } else {
        s.push_str(&format!(
            "  {} = call {} @{}({})\n",
            call_var, ret_llvm_ty, func.name, args_str
        ));
    }

    // 6c. 打包返回值
    let ret_tag_idx = "0";
    let ret_payload_idx = "1";

    // 获取 ret tag 地址
    s.push_str(&format!(
        "  %ret_tag_addr = getelementptr i64, i64* %ret, i64 {}\n",
        ret_tag_idx
    ));
    s.push_str(&format!(
        "  %ret_payload_addr = getelementptr i64, i64* %ret, i64 {}\n",
        ret_payload_idx
    ));

    // 存储 tag
    s.push_str(&format!(
        "  store i64 {}, i64* %ret_tag_addr\n",
        ret_tag
    ));

    // 存储 payload
    match ret_tag {
        TAG_INT => {
            // i32/i16/i8 → i64 (zext); i64 → i64 (no conversion)
            let target_bits = ret_llvm_ty
                .trim_start_matches('i')
                .parse::<usize>()
                .unwrap_or(32);
            if target_bits < 64 {
                s.push_str(&format!(
                    "  %ret_payload = zext {} {} to i64\n",
                    ret_llvm_ty, call_var
                ));
            } else {
                s.push_str(&format!(
                    "  %ret_payload = add i64 {}, 0\n",
                    call_var
                ));
            }
        }
        TAG_FLOAT => {
            // float → double (fpext) → i64 (bitcast); double → i64 (bitcast)
            if ret_llvm_ty == "float" {
                s.push_str(&format!(
                    "  %ret_f64 = fpext float {} to double\n",
                    call_var
                ));
                s.push_str(&format!(
                    "  %ret_payload = bitcast double %ret_f64 to i64\n"
                ));
            } else {
                s.push_str(&format!(
                    "  %ret_payload = bitcast double {} to i64\n",
                    call_var
                ));
            }
        }
        TAG_BOOL => {
            // i1 → i64 (zext)
            s.push_str(&format!(
                "  %ret_payload = zext i1 {} to i64\n",
                call_var
            ));
        }
        TAG_NULL => {
            // void: payload = 0
            s.push_str("  %ret_payload = add i64 0, 0\n");
        }
        _ => unreachable!(),
    }

    s.push_str("  store i64 %ret_payload, i64* %ret_payload_addr\n");

    // 6d. 返回 0
    s.push_str("  ret i64 0\n");
    s.push_str("}\n\n");

    Ok(s)
}
