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

use super::AotCodeGenerator;

/// LLVM IR 生成上下文
pub(crate) struct EmitCtx {
    pub type_mapper: TypeMapper,
    pub target_triple: String,
    pub _opt_level: OptimizationLevel,
    pub _string_as_struct: bool,
    pub link_runtime: bool,
    pub debug_info: Option<DebugInfo>,
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
    ) -> Self {
        Self {
            type_mapper,
            target_triple,
            _opt_level: opt_level,
            _string_as_struct: string_as_struct,
            link_runtime,
            debug_info,
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
            scope.insert(name.to_string(), VarSlot { llvm_name, llvm_ty });
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
        self.sections
            .push(generate_runtime_declarations(&self.type_mapper));
    }
}

/// 从 HIR 程序生成完整 LLVM IR 文本
pub fn emit_program(codegen: &AotCodeGenerator, program: &HirProgram) -> Result<String, AotError> {
    let debug_info = if codegen.options.debug_info {
        Some(DebugInfo::new("main.aura"))
    } else {
        None
    };

    let mut ctx = EmitCtx::new(
        codegen.type_mapper.clone(),
        codegen.options.target.to_string(),
        codegen.options.opt_level,
        codegen.options.string_as_struct,
        codegen.options.link_runtime,
        debug_info,
    );

    // 1. 模块头
    ctx.emit_module_header();

    // 2. 结构体类型定义
    ctx.emit_struct_defs(program);

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

    // 5. 生成所有用户函数（期间收集的全局常量在函数后统一输出）
    for func in &program.functions {
        if ctx.generated_funcs.contains(&func.name) {
            continue;
        }
        ctx.generated_funcs.insert(func.name.clone());
        let func_ir = emit_function(&mut ctx, func)?;
        ctx.sections.push(func_ir);
    }

    // 6. 若没有 main 函数，合成一个
    if !ctx.generated_funcs.contains("main") {
        let main_func = synthesize_main(&program.functions);
        ctx.generated_funcs.insert("main".to_string());
        let func_ir = emit_function(&mut ctx, &main_func)?;
        ctx.sections.push(func_ir);
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
    let ret_ty = func
        .ret
        .as_ref()
        .map(|t| ctx.llvm_type(t))
        .unwrap_or_else(|| "i32".to_string());
    let ret_str = if ret_ty.is_empty() {
        "void".to_string()
    } else {
        ret_ty
    };

    // 参数类型
    let params: Vec<(String, String)> = func
        .params
        .iter()
        .map(|p| {
            let ty = ctx.llvm_type(p.ty.as_ref().unwrap_or(&HirType::Named("Int".into())));
            (p.name.clone(), ty)
        })
        .collect();

    let params_ir: Vec<String> = params
        .iter()
        .map(|(name, ty)| format!("{} %arg.{}", ty, sanitizellvm(name)))
        .collect();
    let params_str = params_ir.join(", ");

    // 函数定义头
    let mut s = String::new();
    if func.is_native {
        // 原生函数已作为 extern declare 生成，跳过
        ctx.exit_scope();
        return Ok(String::new());
    }

    s.push_str(&format!(
        "define {} @{}({}) {{\n",
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
        Self { blocks: Vec::new() }
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
        self.blocks
            .last()
            .map(|bb| bb.terminator.is_some())
            .unwrap_or(false)
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
        HirStmt::Val { name, ty, init } => emit_variable_decl(ctx, blocks, name, ty, init)?,
        HirStmt::Var { name, ty, init } => emit_variable_decl(ctx, blocks, name, ty, init)?,
        HirStmt::Assign { target, value } => emit_assign(ctx, blocks, target, value)?,
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
    let llvm_ty = ty
        .as_ref()
        .map(|t| ctx.llvm_type(t))
        .unwrap_or_else(|| "i32".to_string());

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
        HirExpr::Member { object, name } => {
            emit_member_assign(ctx, blocks, object, name, val_ir, val_ty)?;
        }
        HirExpr::Index { container, index } => {
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
    cur.body
        .push(format!("; store index {} {}", val_ty, val_ir));
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
        HirExpr::Binary { op, lhs, rhs } => emit_binary(ctx, blocks, op, lhs, rhs),
        HirExpr::Unary { op, operand } => emit_unary(ctx, blocks, op, operand),
        HirExpr::Call { callee, args } => emit_call(ctx, blocks, callee, args),
        HirExpr::Member { object, name } => emit_member_access(ctx, blocks, object, name),
        HirExpr::Index { container, index } => emit_index_access(ctx, blocks, container, index),
        HirExpr::New { type_name, args } => emit_new(ctx, blocks, type_name, args),
        HirExpr::If {
            cond,
            then_e,
            else_e,
        } => emit_if_expr(ctx, blocks, cond, then_e, else_e),
        HirExpr::Block(block) => emit_block_expr(ctx, blocks, block),
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
            cur.body.push(format!("{} = fadd float 0.0, {}", name, v));
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
                let op = if l_ty == "double" {
                    "fadd double"
                } else {
                    "fadd float"
                };
                cur.body
                    .push(format!("{} = {} {} {}, {}", tmp, op, l_ir, l_ty, r_ir));
            } else {
                cur.body
                    .push(format!("{} = add {} {}, {}", tmp, l_ty, l_ir, r_ir));
            }
            Ok((tmp, l_ty))
        }
        HirBinOp::Sub => {
            if l_ty.starts_with("float") || l_ty == "double" {
                let op = if l_ty == "double" {
                    "fsub double"
                } else {
                    "fsub float"
                };
                cur.body
                    .push(format!("{} = {} {} {}, {}", tmp, op, l_ir, l_ty, r_ir));
            } else {
                cur.body
                    .push(format!("{} = sub {} {}, {}", tmp, l_ty, l_ir, r_ir));
            }
            Ok((tmp, l_ty))
        }
        HirBinOp::Mul => {
            if l_ty.starts_with("float") || l_ty == "double" {
                let op = if l_ty == "double" {
                    "fmul double"
                } else {
                    "fmul float"
                };
                cur.body
                    .push(format!("{} = {} {} {}, {}", tmp, op, l_ir, l_ty, r_ir));
            } else {
                cur.body
                    .push(format!("{} = mul {} {}, {}", tmp, l_ty, l_ir, r_ir));
            }
            Ok((tmp, l_ty))
        }
        HirBinOp::Div => {
            if l_ty.starts_with("float") || l_ty == "double" {
                let op = if l_ty == "double" {
                    "fdiv double"
                } else {
                    "fdiv float"
                };
                cur.body
                    .push(format!("{} = {} {} {}, {}", tmp, op, l_ir, l_ty, r_ir));
            } else {
                cur.body
                    .push(format!("{} = sdiv {} {}, {}", tmp, l_ty, l_ir, r_ir));
            }
            Ok((tmp, l_ty))
        }
        HirBinOp::Rem => {
            cur.body
                .push(format!("{} = srem {} {}, {}", tmp, l_ty, l_ir, r_ir));
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
            cur.body
                .push(format!("{} = {} i1 {}, {}", tmp, op, l_ir, r_ir));
            Ok((tmp, "i1".to_string()))
        }
        HirBinOp::BitAnd | HirBinOp::BitOr | HirBinOp::BitXor => {
            let op = match op {
                HirBinOp::BitAnd => "and",
                HirBinOp::BitOr => "or",
                HirBinOp::BitXor => "xor",
                _ => unreachable!(),
            };
            cur.body
                .push(format!("{} = {} {} {}, {}", tmp, op, l_ty, l_ir, r_ir));
            Ok((tmp, l_ty))
        }
        HirBinOp::Shl | HirBinOp::Shr => {
            let op = if *op == HirBinOp::Shl { "shl" } else { "ashr" };
            cur.body
                .push(format!("{} = {} {} {}, {}", tmp, op, l_ty, l_ir, r_ir));
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
            cur.body
                .push(format!("{} = sub {} {}, {}", tmp, v_ty, 0, v_ir));
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
    let args_ir: Vec<(String, String)> = args
        .iter()
        .map(|a| emit_expr_val(ctx, blocks, a))
        .collect::<Result<_, _>>()?;

    let ret_ty = "i32".to_string(); // 默认 i32
    let tmp = ctx.fresh_var();
    let cur = blocks.last_mut();
    let args_str: Vec<String> = args_ir
        .iter()
        .map(|(v, t)| format!("{} {}", t, v))
        .collect();

    cur.body.push(format!(
        "{} = call {} @{}({})",
        tmp,
        ret_ty,
        callee,
        args_str.join(", ")
    ));
    Ok((tmp, ret_ty))
}

fn emit_member_access(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    object: &HirExpr,
    _name: &str,
) -> Result<(String, String), AotError> {
    let (obj_ir, _) = emit_expr_val(ctx, blocks, object)?;
    // 简化：字段偏移为 0
    let gep = ctx.fresh_var();
    let tmp = ctx.fresh_var();
    let cur = blocks.last_mut();
    cur.body
        .push(format!("{} = getelementptr i8, i8* {}, i64 0", gep, obj_ir));
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
    let _args_str: Vec<(String, String)> = args
        .iter()
        .map(|a| emit_expr_val(ctx, _blocks, a))
        .collect::<Result<_, _>>()?;
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
        cur.body
            .push(format!("{} = add {} {}, 0", then_phi, then_ty, then_ir));
        cur.terminator = Some(format!("br label %{}", merge_name));
    }

    // Else 块
    let _ = blocks.add_block_named(&else_name);
    let (else_ir, else_ty) = emit_expr_val(ctx, blocks, else_e)?;
    let else_phi = ctx.fresh_var();
    {
        let cur = blocks.last_mut();
        cur.body
            .push(format!("{} = add {} {}, 0", else_phi, else_ty, else_ir));
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
    if ty.is_empty() || ty == "void" {
        "void"
    } else {
        ty
    }
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
            f.ret
                .as_ref()
                .map(|t| matches!(t, HirType::Named(n) if n == "Int"))
                .unwrap_or(false)
        })
        .map(|f| f.name.clone())
        .unwrap_or_else(|| "println".to_string());

    HirFunction {
        name: "main".into(),
        params: vec![],
        ret: Some(HirType::Named("Int".into())),
        body: HirBlock {
            stmts: vec![HirStmt::Return(Some(HirExpr::Call {
                callee: call_target,
                args: vec![],
            }))],
        },
        is_native: false,
        type_params: vec![],
    }
}
