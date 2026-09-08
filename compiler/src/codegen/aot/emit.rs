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
    /// SharedLibrary 模式：包装函数以 external dllexport 导出
    pub wrapper_exported: bool,
    /// C ABI 包装函数模式：为每个非 native 函数生成裸 C ABI 导出函数
    pub c_abi: bool,
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
    /// 循环块栈：(条件块, 结束块)，用于 break/continue
    pub loop_stack: Vec<(String, String)>,
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
    /// 函数名 → LLVM 参数类型列表（用于 emit_call 在调用点做实参类型转换）
    pub func_param_types: HashMap<String, Vec<String>>,
    /// 当前正在发射的函数的 LLVM 返回类型（用于 return 语句的类型转换）
    pub current_ret_ty: String,
    /// 类名 → (字段名 → (LLVM 类型字符串, 字段索引))（用于 emit_member_access 推断字段类型和索引）
    pub class_field_types: HashMap<String, HashMap<String, (String, usize)>>,
    /// 类型别名表：别名 → LLVM 类型字符串（用于 AOT 解析 typealias）
    pub type_aliases: HashMap<String, String>,
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
        wrapper_exported: bool,
        c_abi: bool,
    ) -> Self {
        Self {
            type_mapper,
            target_triple,
            _opt_level: opt_level,
            _string_as_struct: string_as_struct,
            link_runtime,
            debug_info,
            blob_mode,
            wrapper_exported,
            c_abi,
            declared_structs: std::collections::HashSet::new(),
            generated_funcs: std::collections::HashSet::new(),
            sections: Vec::new(),
            bb_counter: 0,
            var_counter: 0,
            const_counter: 0,
            loop_stack: Vec::new(),
            var_scope: vec![HashMap::new()],
            globals: Vec::new(),
            global_const_map: HashMap::new(),
            subprogram_meta: Vec::new(),
            subprogram_index: 0,
            func_dbg_ids: HashMap::new(),
            func_ret_types: HashMap::new(),
            func_param_types: HashMap::new(),
            current_ret_ty: String::new(),
            class_field_types: HashMap::new(),
            type_aliases: HashMap::new(),
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

    /// 映射 HIR 类型到 LLVM IR 类型（解析 typealias）
    pub fn map_type(&self, ty: &HirType) -> String {
        if let HirType::Named(name) = ty {
            if let Some(alias_ty) = self.type_aliases.get(name) {
                return alias_ty.clone();
            }
        }
        self.type_mapper.map(ty)
    }

    pub fn fresh_bb(&mut self, prefix: &str) -> String {
        let name = format!("bb_{}_{}", prefix, self.bb_counter);
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
        self.map_type(ty)
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
                // toString / toStr 原生函数实际返回 C 字符串 (const char*),
                // 而非结构体 { i8*, i64 }，因此覆盖为 i8*
                let fixed = if d.contains("@toString(") || d.contains("@toStr(") {
                    d.replace("{ i8*, i64 } @toString", "i8* @toString")
                        .replace("{ i8*, i64 } @toStr", "i8* @toStr")
                } else if d.contains("@aura_isOfType(") {
                    d.replace("i8* @aura_isOfType", "i1 @aura_isOfType")
                } else {
                    d.clone()
                };
                s.push_str(&fixed);
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
    wrapper_exported: bool,
    c_abi: bool,
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
        wrapper_exported,
        c_abi,
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
            ctx.func_ret_types.insert(func.name.clone(), ctx.map_type(ret));
        } else {
            ctx.func_ret_types.insert(func.name.clone(), "void".to_string());
        }
        let param_tys: Vec<String> = func
            .params
            .iter()
            .map(|p| p.ty.as_ref().map(|t| ctx.map_type(t)).unwrap_or_else(|| "i32".to_string()))
            .collect();
        ctx.func_param_types.insert(func.name.clone(), param_tys);
    }
    // 预注册类字段类型映射（供 emit_member_access 推断字段类型和索引）
    for struct_def in &program.structs {
        let mut field_map = HashMap::new();
        for (fi, (field_name, field_ty)) in struct_def.fields.iter().enumerate() {
            field_map.insert(field_name.clone(), (ctx.map_type(field_ty), fi));
        }
        ctx.class_field_types.insert(struct_def.name.clone(), field_map);
    }
    // 预注册类型别名表（供 AOT 解析 typealias）
    for (alias_name, alias_ty) in &program.type_aliases {
        ctx.type_aliases.insert(alias_name.clone(), ctx.map_type(alias_ty));
    }
    for native in &program.natives {
        if let Some(ref ret) = native.ret {
            // toString / toStr 原生函数实际返回 C 字符串 (const char*),
            // 而非结构体 { i8*, i64 }，因此覆盖为 i8*
            let llvm_ty = if native.name == "toString" || native.name == "toStr" {
                "i8*".to_string()
            } else if native.name == "aura_isOfType" {
                "i1".to_string()
            } else {
                ctx.map_type(ret)
            };
            ctx.func_ret_types.insert(native.name.clone(), llvm_ty);
        } else {
            ctx.func_ret_types.insert(native.name.clone(), "void".to_string());
        }
        // 参数类型：toString / toStr 收 C 字符串指针，println 收不透明指针
        let param_tys: Vec<String> = native
            .params
            .iter()
            .map(|p| {
                if (native.name == "toString" || native.name == "toStr") && p.name == "x" {
                    "i8*".to_string()
                } else {
                    p.ty.as_ref().map(|t| ctx.map_type(t)).unwrap_or_else(|| "i32".to_string())
                }
            })
            .collect();
        ctx.func_param_types.insert(native.name.clone(), param_tys);
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
            } else {
                eprintln!(
                    "warning: AOT wrapper generation failed for function '{}'",
                    func.name
                );
            }
        }
    }

    // 5.5 C ABI 包装函数（--cabi 标志）：为每个非 native 函数生成裸 C ABI 导出函数
    if c_abi {
        for func in &program.functions {
            if func.is_native {
                continue;
            }
            // C ABI 包装函数名与原始函数名不同，不检查 generated_funcs
            let wrapper_name = format!("aura_c_{}", sanitizellvm(&func.name));
            if ctx.generated_funcs.contains(&wrapper_name) {
                continue;
            }
            ctx.generated_funcs.insert(wrapper_name.clone());
            if let Ok(wrapper_ir) = emit_c_abi_wrapper(&mut ctx, func) {
                ctx.sections.push(wrapper_ir);
            } else {
                eprintln!(
                    "warning: C ABI wrapper generation failed for function '{}'",
                    func.name
                );
            }
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

    // 返回类型：无显式返回类型时默认为 void（Unit 函数）
    let ret_ty = func.ret.as_ref().map(|t| ctx.llvm_type(t)).unwrap_or_else(|| "void".to_string());
    let ret_str = if ret_ty.is_empty() { "void".to_string() } else { ret_ty };
    ctx.current_ret_ty = ret_str.clone();

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
        ret_str,
        func.name,
        params_str
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
                let want = ctx.current_ret_ty.clone();
                // 返回值类型必须与函数签名的返回类型一致（如 Float 函数里 `return 0.0`）
                let (val_ir, val_ty) = if want.is_empty() || want == "void" {
                    (val_ir, sanitize_ty_for_ret(&val_ty).to_string())
                } else {
                    let converted = coerce_arg(ctx, blocks, val_ir, &val_ty, &want);
                    (converted.0, converted.1)
                };
                blocks.set_terminator(&format!("ret {} {}", val_ty, val_ir));
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
            // 分支到当前循环的结束块
            if let Some((_, end_name)) = ctx.loop_stack.last() {
                blocks.set_terminator(&format!("br label %{}", end_name));
            } else {
                return Err(AotError::CodeGenerationFailed(
                    "break outside loop".to_string(),
                ));
            }
        }
        HirStmt::Continue => {
            // 分支到当前循环的条件块
            if let Some((cond_name, _)) = ctx.loop_stack.last() {
                blocks.set_terminator(&format!("br label %{}", cond_name));
            } else {
                return Err(AotError::CodeGenerationFailed(
                    "continue outside loop".to_string(),
                ));
            }
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
    // 当类型未显式标注时，从初始化器推断 LLVM 类型
    let llvm_ty = if let Some(t) = ty {
        ctx.llvm_type(t)
    } else if let Some(init_expr) = init {
        // 先发射初始化器获取其类型
        let (val_ir, val_ty) = emit_expr_val(ctx, blocks, init_expr)?;
        let var_name = ctx.fresh_var();
        {
            let cur = blocks.last_mut();
            cur.body.push(format!("{} = alloca {}", var_name, val_ty));
        }
        let cur = blocks.last_mut();
        // 如果值类型和目标类型不匹配（如 null 存入可空结构体），需要转换
        if val_ir == "null" && val_ty.starts_with("{ ") && val_ty.ends_with(" i1 }") {
            let null_val = zero_value(&val_ty);
            cur.body.push(format!(
                "store {} {} , {}* {}",
                val_ty, null_val, val_ty, var_name
            ));
        } else {
            cur.body.push(format!(
                "store {} {} , {}* {}",
                val_ty, val_ir, val_ty, var_name
            ));
        }
        ctx.declare_var(name, var_name, val_ty);
        return Ok(());
    } else {
        "i32".to_string()
    };

    let var_name = ctx.fresh_var();
    {
        let cur = blocks.last_mut();
        cur.body.push(format!("{} = alloca {}", var_name, llvm_ty));
    }

    if let Some(init) = init {
        let (val_ir, val_ty) = emit_expr_val(ctx, blocks, init)?;
        emit_store_converted(ctx, blocks, &llvm_ty, &val_ir, &val_ty, &var_name);
    }

    ctx.declare_var(name, var_name, llvm_ty);
    Ok(())
}

/// 将值存储到目标类型中，处理可空结构体 { T, i1 } 的包装/提取
fn emit_store_converted(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    dst_ty: &str,
    val_ir: &str,
    val_ty: &str,
    var_name: &str,
) {
    let cur = blocks.last_mut();

    if is_nullable_struct_type(dst_ty) && val_ty != dst_ty {
        // 目标类型是可空结构体 { T, i1 }
        if val_ir == "null" {
            // null → { T 0, i1 true }
            let null_val = zero_value(dst_ty);
            cur.body.push(format!(
                "store {} {} , {}* {}",
                dst_ty, null_val, dst_ty, var_name
            ));
        } else {
            // 非 null 值 → 包装为 { T <val>, i1 false }
            let inner_ty = extract_inner_type(dst_ty);
            let wrap0 = ctx.fresh_var();
            let wrap1 = ctx.fresh_var();
            cur.body.push(format!(
                "{} = insertvalue {} undef, {} {}, 0",
                wrap0, dst_ty, inner_ty, val_ir
            ));
            cur.body.push(format!(
                "{} = insertvalue {} {}, i1 false, 1",
                wrap1, dst_ty, wrap0
            ));
            cur.body.push(format!(
                "store {} {} , {}* {}",
                dst_ty, wrap1, dst_ty, var_name
            ));
        }
    } else if is_nullable_struct_type(val_ty) && !is_nullable_struct_type(dst_ty) {
        // 值类型是可空结构体，目标类型不是 → 提取内部值
        let inner_ty = extract_inner_type(val_ty);
        let extract_var = ctx.fresh_var();
        cur.body.push(format!(
            "{} = extractvalue {} {}, 0",
            extract_var, val_ty, val_ir
        ));
        cur.body.push(format!(
            "store {} {} , {}* {}",
            inner_ty, extract_var, dst_ty, var_name
        ));
    } else {
        cur.body.push(format!(
            "store {} {} , {}* {}",
            val_ty, val_ir, dst_ty, var_name
        ));
    }
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
                let slot_ty = slot.llvm_ty.clone();
                let slot_name = slot.llvm_name.clone();
                emit_store_converted(ctx, blocks, &slot_ty, &val_ir, &val_ty, &slot_name);
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
    field: &str,
    val_ir: String,
    val_ty: String,
) -> Result<(), AotError> {
    // 字段索引 / 类型表：按字段名在所有已注册类中查找
    let found = ctx.class_field_types.iter().find_map(|(class, fields)| {
        fields.get(field).map(|(ty, idx)| (class.clone(), ty.clone(), *idx))
    });

    // 情形 1：对象是「结构体值的局部变量」→ load / insertvalue / store 回写
    if let (HirExpr::Var(var_name), Some((class, field_ty, field_idx))) = (object, &found) {
        let slot = ctx.lookup_var(var_name).cloned();
        if let Some(slot) = slot {
            let struct_ty = format!("%struct.{}", sanitizellvm(class));
            if slot.llvm_ty == struct_ty {
                let (v, vty) = coerce_arg(ctx, _blocks, val_ir, &val_ty, field_ty);
                let loaded = ctx.fresh_var();
                let updated = ctx.fresh_var();
                let cur = _blocks.last_mut();
                cur.body.push(format!(
                    "{} = load {}, {}* {}",
                    loaded, struct_ty, struct_ty, slot.llvm_name
                ));
                cur.body.push(format!(
                    "{} = insertvalue {} {}, {} {}, {}",
                    updated, struct_ty, loaded, vty, v, field_idx
                ));
                cur.body.push(format!(
                    "store {} {}, {}* {}",
                    struct_ty, updated, struct_ty, slot.llvm_name
                ));
                return Ok(());
            }
        }
    }

    // 情形 2：对象是「指向结构体的指针」→ GEP 取字段地址后 store
    let (obj_ir, obj_ty) = emit_expr_val(ctx, _blocks, object)?;
    if let Some((class, field_ty, field_idx)) = found {
        let struct_ty = format!("%struct.{}", sanitizellvm(&class));
        if obj_ty.ends_with('*') || obj_ty == "i8*" || obj_ty == "ptr" {
            let cast = ctx.fresh_var();
            let gep = ctx.fresh_var();
            let (v, vty) = coerce_arg(ctx, _blocks, val_ir, &val_ty, &field_ty);
            let cur = _blocks.last_mut();
            cur.body.push(format!(
                "{} = bitcast {} {} to {}*",
                cast, obj_ty, obj_ir, struct_ty
            ));
            cur.body.push(format!(
                "{} = getelementptr {}, {}* {}, i32 0, i32 {}",
                gep, struct_ty, struct_ty, cast, field_idx
            ));
            cur.body.push(format!("store {} {}, {}* {}", vty, v, vty, gep));
            return Ok(());
        }
    }

    // 其它形态：无法定位字段地址，生成注释避免产生非法 IR
    let cur = _blocks.last_mut();
    cur.body.push(format!(
        "; unsupported member assign .{} = {} {}",
        field, val_ty, val_ir
    ));
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

    // 推入循环块栈（用于 break/continue）
    ctx.loop_stack.push((cond_name.clone(), end_name.clone()));

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

    // 弹出循环块栈
    ctx.loop_stack.pop();
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
                wrapper_exported: ctx.wrapper_exported,
                c_abi: ctx.c_abi,
                declared_structs: std::collections::HashSet::new(),
                generated_funcs: std::collections::HashSet::new(),
                sections: Vec::new(),
                bb_counter: 0,
                var_counter: 0,
                const_counter: 0,
                loop_stack: Vec::new(),
                var_scope: vec![HashMap::new()],
                globals: Vec::new(),
                global_const_map: HashMap::new(),
                subprogram_meta: Vec::new(),
                subprogram_index: 0,
                func_dbg_ids: HashMap::new(),
                func_ret_types: ctx.func_ret_types.clone(),
                func_param_types: ctx.func_param_types.clone(),
                current_ret_ty: String::new(),
                class_field_types: ctx.class_field_types.clone(),
                type_aliases: ctx.type_aliases.clone(),
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
        // CallVirtual — 与 Call 相同路径（AOT 生成静态调用）
        HirExpr::CallVirtual {
            recv,
            name,
            args,
        } => {
            // AOT 生成静态调用：把虚方法名解析为具体的 `Class.method`
            let mut resolved = name.clone();
            if !ctx.func_ret_types.contains_key(&resolved) {
                let recv_class = match recv.as_ref() {
                    HirExpr::Var(vn) => ctx
                        .lookup_var(vn)
                        .and_then(|s| s.llvm_ty.strip_prefix("%struct.").map(|c| c.to_string())),
                    _ => None,
                };
                if let Some(cls) = recv_class {
                    let cand = format!("{}.{}", cls, name);
                    if ctx.func_ret_types.contains_key(&cand) {
                        resolved = cand;
                    }
                }
                if resolved == *name {
                    let suffix = format!(".{}", name);
                    if let Some(k) =
                        ctx.func_ret_types.keys().find(|k| k.ends_with(&suffix)).cloned()
                    {
                        resolved = k;
                    }
                }
            }
            emit_call(ctx, blocks, &resolved, args)
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
            let name = ctx.fresh_const("double");
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
            // Double 在 LLVM 中是 double（64位），Float 是 float（32位）
            // Aura 的 Float 字面量默认映射为 double（与 Kotlin 一致）
            cur.body.push(format!("{} = fadd double 0.0, {}", name, lit));
            Ok((name, "double".to_string()))
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
    // LLVM IR 的 `c"..."` 字符串常量使用 C 风格转义序列。
    // 为避免 \n \t 等非 ASCII 字符被错误解析，使用十六进制转义（\XX）。
    let mut escaped = String::new();
    for &b in s.as_bytes() {
        match b {
            b'\\' => escaped.push_str("\\\\"),
            b'"' => escaped.push_str("\\22"),
            0x0A => escaped.push_str("\\0A"),
            0x0D => escaped.push_str("\\0D"),
            0x09 => escaped.push_str("\\09"),
            0x00 => escaped.push_str("\\00"),
            0x20..=0x7E => {
                // ASCII 可打印字符（除 \ 和 "）直接保留
                escaped.push(b as char);
            }
            _ => {
                // 非 ASCII 字节 → 十六进制转义
                escaped.push_str(&format!("\\{:02X}", b));
            }
        }
    }
    let llvm_str = format!("c\"{}\\00\"", escaped);
    // 计算实际字节长度：转义序列在 IR 中是单字节
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
    let struct_tmp = ctx.fresh_const("str_tmp");
    let struct_name = ctx.fresh_const("str_struct");
    let cur = blocks.last_mut();
    cur.body.push(format!(
        "{} = getelementptr [{} x i8], [{} x i8]* {}, i64 0, i64 0",
        gep_name, byte_len, byte_len, data_name
    ));
    // 构建字符串结构体 { i8*, i64 }：指针 + 长度（不含终止符）
    cur.body.push(format!(
        "{} = insertvalue {{ i8*, i64 }} undef, i8* {}, 0",
        struct_tmp, gep_name
    ));
    cur.body.push(format!(
        "{} = insertvalue {{ i8*, i64 }} {}, i64 {}, 1",
        struct_name,
        struct_tmp,
        byte_len - 1
    ));
    Ok((struct_name, "{ i8*, i64 }".to_string()))
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

fn is_string_type(ty: &str) -> bool {
    ty == "i8*" || ty == "{ i8*, i64 }"
}

fn is_numeric_type(ty: &str) -> bool {
    ty.starts_with("i") || ty.starts_with("f") || ty == "double"
}

/// 检查是否为可空结构体类型 { T, i1 }
fn is_nullable_struct_type(ty: &str) -> bool {
    ty.starts_with("{ ") && ty.ends_with(" i1 }")
}

/// 从可空结构体类型 { T, i1 } 中提取内部类型 T
fn extract_inner_type(ty: &str) -> String {
    // ty 格式: "{ T, i1 }"
    // 去掉 "{ " (2 chars) 和 ", i1 }" (6 chars)
    ty[2..ty.len() - 6].trim().to_string()
}

/// 在指定基本块的终止符之前插入指令
fn insert_before_terminator(blocks: &mut FuncBlocks, bb_name: &str, instr: String) {
    if let Some(pos) = blocks.blocks.iter().position(|b| b.name == bb_name) {
        let bb = &mut blocks.blocks[pos];
        if bb.terminator.is_some() {
            bb.body.push(instr);
        } else {
            bb.body.push(instr);
        }
    }
}

/// 从 LLVM IR 值中提取字符串数据指针和长度
/// - `i8*` 类型：直接用值作为指针，调用 `aura_string_length` 获取长度
/// - `{ i8*, i64 }` 类型：提取字段 0（指针）和字段 1（长度）
fn extract_string_parts(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    val_ir: &str,
    val_ty: &str,
) -> (String, String) {
    if val_ty == "i8*" {
        let len_var = ctx.fresh_var();
        let cur = blocks.last_mut();
        cur.body.push(format!(
            "{} = call i64 @aura_string_length(i8* {})",
            len_var, val_ir
        ));
        (val_ir.to_string(), len_var)
    } else if val_ty == "{ i8*, i64 }" {
        let ptr_var = ctx.fresh_var();
        let len_var = ctx.fresh_var();
        let cur = blocks.last_mut();
        cur.body.push(format!(
            "{} = extractvalue {{ i8*, i64 }} {}, 0",
            ptr_var, val_ir
        ));
        cur.body.push(format!(
            "{} = extractvalue {{ i8*, i64 }} {}, 1",
            len_var, val_ir
        ));
        (ptr_var, len_var)
    } else if is_int_ty(val_ty) || is_float_ty(val_ty) {
        // 非字符串操作数（如 Int 字段参与字符串拼接）：先转成指针交给运行时 toString
        let as_ptr = ctx.fresh_var();
        let str_var = ctx.fresh_var();
        let len_var = ctx.fresh_var();
        let cur = blocks.last_mut();
        if is_int_ty(val_ty) {
            cur.body.push(format!(
                "{} = inttoptr {} {} to i8*",
                as_ptr, val_ty, val_ir
            ));
        } else {
            let bits = ctx.fresh_var();
            cur.body.push(format!("{} = bitcast {} {} to i64", bits, val_ty, val_ir));
            cur.body.push(format!("{} = inttoptr i64 {} to i8*", as_ptr, bits));
        }
        cur.body.push(format!("{} = call i8* @toString(i8* {})", str_var, as_ptr));
        cur.body.push(format!(
            "{} = call i64 @aura_string_length(i8* {})",
            len_var, str_var
        ));
        (str_var, len_var)
    } else {
        // Fallback: treat as i8* with strlen
        let len_var = ctx.fresh_var();
        let cur = blocks.last_mut();
        cur.body.push(format!(
            "{} = call i64 @aura_string_length(i8* {})",
            len_var, val_ir
        ));
        (val_ir.to_string(), len_var)
    }
}

fn emit_binary(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    op: &HirBinOp,
    lhs: &HirExpr,
    rhs: &HirExpr,
) -> Result<(String, String), AotError> {
    let (mut l_ir, mut l_ty) = emit_expr_val(ctx, blocks, lhs)?;
    let (mut r_ir, mut r_ty) = emit_expr_val(ctx, blocks, rhs)?;
    let tmp = ctx.fresh_var();

    // 可空结构体提取：如果一侧是 { T, i1 } 而另一侧不是，提取内部值
    // 但 null 比较由后续专用逻辑处理，不在此处提取
    if r_ir != "null" && l_ir != "null" {
        if is_nullable_struct_type(&l_ty) && !is_nullable_struct_type(&r_ty) {
            let inner_ty = extract_inner_type(&l_ty);
            let extract_var = ctx.fresh_var();
            let cur = blocks.last_mut();
            cur.body.push(format!(
                "{} = extractvalue {} {}, 0",
                extract_var, l_ty, l_ir
            ));
            l_ir = extract_var;
            l_ty = inner_ty;
        } else if is_nullable_struct_type(&r_ty) && !is_nullable_struct_type(&l_ty) {
            let inner_ty = extract_inner_type(&r_ty);
            let extract_var = ctx.fresh_var();
            let cur = blocks.last_mut();
            cur.body.push(format!(
                "{} = extractvalue {} {}, 0",
                extract_var, r_ty, r_ir
            ));
            r_ir = extract_var;
            r_ty = inner_ty;
        } else if is_nullable_struct_type(&l_ty) && is_nullable_struct_type(&r_ty) {
            let l_inner = extract_inner_type(&l_ty);
            let r_inner = extract_inner_type(&r_ty);
            let l_extract = ctx.fresh_var();
            let r_extract = ctx.fresh_var();
            let cur = blocks.last_mut();
            cur.body.push(format!("{} = extractvalue {} {}, 0", l_extract, l_ty, l_ir));
            cur.body.push(format!("{} = extractvalue {} {}, 0", r_extract, r_ty, r_ir));
            l_ir = l_extract;
            l_ty = l_inner;
            r_ir = r_extract;
            r_ty = r_inner;
        }
    }

    // 混合类型检测：任一侧为浮点类型时使用浮点运算
    let l_is_float = l_ty.starts_with("float") || l_ty == "double";
    let r_is_float = r_ty.starts_with("float") || r_ty == "double";
    let is_float = l_is_float || r_is_float;

    match op {
        HirBinOp::Add => {
            // 字符串拼接：左右操作数为字符串类型时调用 aura_string_concat
            if is_string_type(&l_ty) || is_string_type(&r_ty) {
                let (l_ptr, l_len) = extract_string_parts(ctx, blocks, &l_ir, &l_ty);
                let (r_ptr, r_len) = extract_string_parts(ctx, blocks, &r_ir, &r_ty);
                let concat = ctx.fresh_var();
                let str_ty = ctx.map_type(&HirType::Named("String".to_string()));
                // String 在 LLVM IR 中表示为 { 指针, 长度 } 结构体时，需要把
                // concat 返回的裸指针补上长度字段，否则返回类型与函数签名不符。
                let (total, s1, s2) = if str_ty.starts_with('{') {
                    (
                        Some(ctx.fresh_var()),
                        Some(ctx.fresh_var()),
                        Some(ctx.fresh_var()),
                    )
                } else {
                    (None, None, None)
                };
                let cur = blocks.last_mut();
                cur.body.push(format!(
                    "{} = call i8* @aura_string_concat(i8* {}, i64 {}, i8* {}, i64 {})",
                    concat, l_ptr, l_len, r_ptr, r_len
                ));
                if let (Some(total), Some(s1), Some(s2)) = (total, s1, s2) {
                    cur.body.push(format!("{} = add i64 {}, {}", total, l_len, r_len));
                    cur.body.push(format!(
                        "{} = insertvalue {} undef, i8* {}, 0",
                        s1, str_ty, concat
                    ));
                    cur.body.push(format!(
                        "{} = insertvalue {} {}, i64 {}, 1",
                        s2, str_ty, s1, total
                    ));
                    Ok((s2, str_ty))
                } else {
                    Ok((concat, str_ty))
                }
            } else if is_float {
                // 混合类型时统一为 double
                let result_ty =
                    if l_ty == "double" || r_ty == "double" { "double" } else { "float" };
                let l_conv = emit_numeric_convert(ctx, blocks, &l_ir, &l_ty, result_ty);
                let r_conv = emit_numeric_convert(ctx, blocks, &r_ir, &r_ty, result_ty);
                let op = if result_ty == "double" { "fadd double" } else { "fadd float" };
                let cur = blocks.last_mut();
                cur.body.push(format!("{} = {} {}, {}", tmp, op, l_conv, r_conv));
                Ok((tmp, result_ty.to_string()))
            } else {
                let cur = blocks.last_mut();
                cur.body.push(format!("{} = add {} {}, {}", tmp, l_ty, l_ir, r_ir));
                Ok((tmp, l_ty))
            }
        }
        HirBinOp::Sub => {
            let cur = blocks.last_mut();
            if is_float {
                let result_ty =
                    if l_ty == "double" || r_ty == "double" { "double" } else { "float" };
                drop(cur);
                let l_conv = emit_numeric_convert(ctx, blocks, &l_ir, &l_ty, result_ty);
                let r_conv = emit_numeric_convert(ctx, blocks, &r_ir, &r_ty, result_ty);
                let op = if result_ty == "double" { "fsub double" } else { "fsub float" };
                let cur = blocks.last_mut();
                cur.body.push(format!("{} = {} {}, {}", tmp, op, l_conv, r_conv));
                Ok((tmp, result_ty.to_string()))
            } else {
                cur.body.push(format!("{} = sub {} {}, {}", tmp, l_ty, l_ir, r_ir));
                Ok((tmp, l_ty))
            }
        }
        HirBinOp::Mul => {
            let cur = blocks.last_mut();
            if is_float {
                let result_ty =
                    if l_ty == "double" || r_ty == "double" { "double" } else { "float" };
                drop(cur);
                let l_conv = emit_numeric_convert(ctx, blocks, &l_ir, &l_ty, result_ty);
                let r_conv = emit_numeric_convert(ctx, blocks, &r_ir, &r_ty, result_ty);
                let op = if result_ty == "double" { "fmul double" } else { "fmul float" };
                let cur = blocks.last_mut();
                cur.body.push(format!("{} = {} {}, {}", tmp, op, l_conv, r_conv));
                Ok((tmp, result_ty.to_string()))
            } else {
                cur.body.push(format!("{} = mul {} {}, {}", tmp, l_ty, l_ir, r_ir));
                Ok((tmp, l_ty))
            }
        }
        HirBinOp::Div => {
            let cur = blocks.last_mut();
            if is_float {
                let result_ty =
                    if l_ty == "double" || r_ty == "double" { "double" } else { "float" };
                drop(cur);
                let l_conv = emit_numeric_convert(ctx, blocks, &l_ir, &l_ty, result_ty);
                let r_conv = emit_numeric_convert(ctx, blocks, &r_ir, &r_ty, result_ty);
                let op = if result_ty == "double" { "fdiv double" } else { "fdiv float" };
                let cur = blocks.last_mut();
                cur.body.push(format!("{} = {} {}, {}", tmp, op, l_conv, r_conv));
                Ok((tmp, result_ty.to_string()))
            } else {
                cur.body.push(format!("{} = sdiv {} {}, {}", tmp, l_ty, l_ir, r_ir));
                Ok((tmp, l_ty))
            }
        }
        HirBinOp::Rem => {
            let cur = blocks.last_mut();
            cur.body.push(format!("{} = srem {} {}, {}", tmp, l_ty, l_ir, r_ir));
            Ok((tmp, l_ty))
        }
        HirBinOp::Eq | HirBinOp::Ne | HirBinOp::Lt | HirBinOp::Gt | HirBinOp::Le | HirBinOp::Ge => {
            // 可空类型与 null 比较：检查 is_null 标志（结构体字段 1）
            if r_ir == "null" && is_nullable_struct_type(&l_ty) {
                let null_flag = ctx.fresh_var();
                let cur = blocks.last_mut();
                // 提取 is_null 标志（字段 1）
                cur.body.push(format!("{} = extractvalue {} {}, 1", null_flag, l_ty, l_ir));
                match op {
                    HirBinOp::Eq => {
                        cur.body.push(format!("{} = icmp eq i1 {}, true", tmp, null_flag));
                    }
                    HirBinOp::Ne => {
                        cur.body.push(format!("{} = icmp ne i1 {}, true", tmp, null_flag));
                    }
                    _ => unreachable!(),
                }
                return Ok((tmp, "i1".to_string()));
            }
            // 结构体类型（如字符串 { i8*, i64 }）与 null 比较：提取指针比较
            if r_ir == "null" && l_ty.starts_with("{ ") {
                let ptr = ctx.fresh_var();
                let cur = blocks.last_mut();
                // 提取结构体第一个字段（指针）
                cur.body.push(format!("{} = extractvalue {} {}, 0", ptr, l_ty, l_ir));
                match op {
                    HirBinOp::Eq => {
                        cur.body.push(format!("{} = icmp eq i8* {}, null", tmp, ptr));
                    }
                    HirBinOp::Ne => {
                        cur.body.push(format!("{} = icmp ne i8* {}, null", tmp, ptr));
                    }
                    _ => unreachable!(),
                }
                return Ok((tmp, "i1".to_string()));
            }
            // 可空类型作为左操作数，右操作数不为 null：也用 is_null 标志判断
            if is_nullable_struct_type(&l_ty) && r_ir != "null" {
                let null_flag = ctx.fresh_var();
                let cur = blocks.last_mut();
                cur.body.push(format!("{} = extractvalue {} {}, 1", null_flag, l_ty, l_ir));
                // 提取右操作数的 is_null 标志（如果是可空结构体）
                let r_null_flag = if is_nullable_struct_type(&r_ty) {
                    let rf = ctx.fresh_var();
                    cur.body.push(format!("{} = extractvalue {} {}, 1", rf, r_ty, r_ir));
                    rf
                } else {
                    // 右操作数非可空，视为 false（非 null）
                    "false".to_string()
                };
                let icmp_pred = if *op == HirBinOp::Eq { "eq" } else { "ne" };
                cur.body.push(format!(
                    "{} = icmp {} i1 {}, {}",
                    tmp, icmp_pred, null_flag, r_null_flag
                ));
                return Ok((tmp, "i1".to_string()));
            }
            let (icmp_pred, is_signed) = match op {
                HirBinOp::Eq => ("eq", true),
                HirBinOp::Ne => ("ne", true),
                HirBinOp::Lt => ("slt", true),
                HirBinOp::Gt => ("sgt", true),
                HirBinOp::Le => ("sle", true),
                HirBinOp::Ge => ("sge", true),
                _ => unreachable!(),
            };
            // 数值比较：两侧类型不一致时统一到共同类型（否则生成非法 IR）
            if is_numeric_type(&l_ty) && is_numeric_type(&r_ty) && l_ty != r_ty {
                let target = if is_float_ty(&l_ty) || is_float_ty(&r_ty) {
                    if l_ty == "double" || r_ty == "double" {
                        "double".to_string()
                    } else {
                        "float".to_string()
                    }
                } else {
                    l_ty.clone()
                };
                l_ir = emit_numeric_convert(ctx, blocks, &l_ir, &l_ty, &target);
                r_ir = emit_numeric_convert(ctx, blocks, &r_ir, &r_ty, &target);
                l_ty = target.clone();
                r_ty = target;
            }
            let pred = if is_signed && l_ty.starts_with("i") {
                format!("icmp {} {} {}, {}", icmp_pred, l_ty, l_ir, r_ir)
            } else if l_ty.starts_with("{ ") {
                // 结构体类型（如字符串）：提取指针比较
                let l_ptr = ctx.fresh_var();
                let r_ptr = ctx.fresh_var();
                let cur = blocks.last_mut();
                cur.body.push(format!("{} = extractvalue {} {}, 0", l_ptr, l_ty, l_ir));
                if r_ty.starts_with("{ ") && r_ty != l_ty {
                    cur.body.push(format!("{} = extractvalue {} {}, 0", r_ptr, r_ty, r_ir));
                } else {
                    cur.body.push(format!("{} = extractvalue {} {}, 0", r_ptr, l_ty, r_ir));
                }
                format!("icmp {} i8* {}, {}", icmp_pred, l_ptr, r_ptr)
            } else {
                let fcmp_pred = match op {
                    HirBinOp::Eq => "oeq",
                    HirBinOp::Ne => "one",
                    HirBinOp::Lt => "olt",
                    HirBinOp::Gt => "ogt",
                    HirBinOp::Le => "ole",
                    HirBinOp::Ge => "oge",
                    _ => "one",
                };
                format!("fcmp {} {} {}, {}", fcmp_pred, l_ty, l_ir, r_ir)
            };
            let cur = blocks.last_mut();
            cur.body.push(format!("{} = {}", tmp, pred));
            Ok((tmp, "i1".to_string()))
        }
        HirBinOp::And | HirBinOp::Or => {
            let op = if *op == HirBinOp::And { "and" } else { "or" };
            let cur = blocks.last_mut();
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
            let cur = blocks.last_mut();
            cur.body.push(format!("{} = {} {} {}, {}", tmp, op, l_ty, l_ir, r_ir));
            Ok((tmp, l_ty))
        }
        HirBinOp::Shl | HirBinOp::Shr => {
            let op = if *op == HirBinOp::Shl { "shl" } else { "ashr" };
            let cur = blocks.last_mut();
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

    // aura_isOfType: AOT 中值没有运行时类型标签，编译期解析
    if callee == "aura_isOfType" && args.len() == 2 {
        let (val_ir, val_ty) = emit_expr_val(ctx, blocks, &args[0])?;
        let target_type = match &args[1] {
            crate::codegen::hir::HirExpr::Lit(crate::ast::Literal::String(s)) => s.clone(),
            _ => {
                return Err(AotError::CodeGenerationFailed(
                    "aura_isOfType: second arg must be a string literal".to_string(),
                ));
            }
        };

        // 将 LLVM IR 类型映射回 Aura 类型名
        let is_match = match val_ty.as_str() {
            "i32" => target_type == "Int",
            "i64" => target_type == "Long",
            "i16" => target_type == "Short" || target_type == "Char",
            "i8" => target_type == "Byte" || target_type == "U8",
            "float" => target_type == "Float",
            "double" => target_type == "Double",
            "i1" => target_type == "Boolean" || target_type == "Bool",
            // AuraString 结构体 { i8*, i64 }
            t if t.starts_with('{') && t.contains("i8*") && t.contains("i64") => {
                target_type == "String"
            }
            "i8*" => target_type == "String",
            _ => false,
        };

        return Ok((
            if is_match { "true".to_string() } else { "false".to_string() },
            "i1".to_string(),
        ));
    }

    // aura_cast / aura_cast_safety：AOT 中按静态类型在编译期求值
    // - 目标为数值基本类型 → 插入数值转换（as 为硬转换，as? 仅在类型匹配时转换）
    // - 目标为类 / Any / String 等指针语义类型 → 直接透传值
    if (callee == "aura_cast" || callee == "aura_cast_safety") && args.len() == 2 {
        let target_type = match &args[1] {
            HirExpr::Lit(crate::ast::Literal::String(s)) => s.clone(),
            _ => {
                return Err(AotError::CodeGenerationFailed(format!(
                    "{}: second arg must be a string literal",
                    callee
                )));
            }
        };
        let (val_ir, val_ty) = emit_expr_val(ctx, blocks, &args[0])?;
        let target_llvm = ctx.map_type(&HirType::Named(target_type.clone()));
        let numeric_target = is_int_ty(&target_llvm) || is_float_ty(&target_llvm);
        if numeric_target {
            if callee == "aura_cast_safety" && !same_numeric_kind(&val_ty, &target_llvm) {
                // 类型不匹配：as? 返回「空」值（数值 0）
                return Ok(("0".to_string(), target_llvm));
            }
            let (v, t) = coerce_arg(ctx, blocks, val_ir, &val_ty, &target_llvm);
            return Ok((v, t));
        }
        // 对象 / 字符串 / Any：直接透传（静态类型已知，无需运行时检查）
        return Ok((val_ir, val_ty));
    }

    // typeOf：AOT 中由静态类型在编译期折叠为字符串常量
    if callee == "typeOf" && args.len() == 1 {
        let (_val_ir, val_ty) = emit_expr_val(ctx, blocks, &args[0])?;
        let type_name = llvm_ty_to_aura_name(&val_ty).unwrap_or("Any");
        return emit_literal(
            ctx,
            blocks,
            &crate::ast::Literal::String(type_name.to_string()),
        );
    }

    let args_ir: Vec<(String, String)> =
        args.iter().map(|a| emit_expr_val(ctx, blocks, a)).collect::<Result<_, _>>()?;

    let ret_ty = ctx.func_ret_types.get(callee).cloned().unwrap_or_else(|| "i32".to_string());
    let param_tys = ctx.func_param_types.get(callee).cloned();
    // 按被调方声明的参数类型转换实参（LLVM IR 对调用/声明类型一致性要求严格）
    let args_ir: Vec<(String, String)> = match &param_tys {
        Some(pts) => {
            let mut out = Vec::with_capacity(args_ir.len());
            for (i, (v, t)) in args_ir.into_iter().enumerate() {
                match pts.get(i) {
                    Some(want) => out.push(coerce_arg(ctx, blocks, v, &t, want)),
                    None => out.push((v, t)),
                }
            }
            out
        }
        None => args_ir,
    };
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

/// 判断两个 LLVM 类型是否属于同一「数值类别」（as? 的基本类型匹配语义）
fn same_numeric_kind(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    (is_int_ty(a) && is_int_ty(b)) || (is_float_ty(a) && is_float_ty(b))
}

/// 由 LLVM 类型反推 Aura 类型名（供 typeOf 在 AOT 中编译期折叠）
fn llvm_ty_to_aura_name(t: &str) -> Option<&'static str> {
    match t {
        "i32" => Some("Int"),
        "i64" => Some("Long"),
        "i16" => Some("Short"),
        "i8" => Some("Byte"),
        "float" => Some("Float"),
        "double" => Some("Double"),
        "i1" => Some("Boolean"),
        "i8*" => Some("String"),
        t if t.starts_with('{') && t.contains("i8*") && t.contains("i64") => Some("String"),
        _ => None,
    }
}

/// 判断 LLVM 类型字符串是否是指针
fn is_ptr_ty(t: &str) -> bool {
    t.ends_with('*') || t == "ptr"
}

/// 判断 LLVM 类型字符串是否为整数（i1/i8/i16/i32/i64…）
fn is_int_ty(t: &str) -> bool {
    t.len() >= 2 && t.starts_with('i') && t[1..].chars().all(|c| c.is_ascii_digit())
}

fn int_bits(t: &str) -> u32 {
    t[1..].parse().unwrap_or(32)
}

fn is_float_ty(t: &str) -> bool {
    t == "float" || t == "double"
}

/// 将实参值转换为被调方声明的参数类型。
///
/// LLVM IR 要求 `call` 的实参类型与 `declare`/`define` 的参数类型完全一致；
/// Aura 中 `Any` 映射为 `i8*`、`String` 映射为 `{ i8*, i64 }` 或 `i8*`，
/// 因此调用点必须显式插入 `inttoptr` / `ptrtoint` / `zext` / `trunc` 等转换。
/// 无法安全转换时原样返回（让后续 llc 报错暴露问题，而不是静默生成错值）。
fn coerce_arg(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    val: String,
    from: &str,
    to: &str,
) -> (String, String) {
    if from == to || to.is_empty() || from.is_empty() {
        return (val, from.to_string());
    }
    let mut emit = |body: &mut Vec<String>, line: String| body.push(line);
    // 字符串结构体 → i8*：取出数据指针
    if from.starts_with('{') && from.contains("i8*") && to == "i8*" {
        let t = ctx.fresh_var();
        emit(
            &mut blocks.last_mut().body,
            format!("{} = extractvalue {} {}, 0", t, from, val),
        );
        return (t, to.to_string());
    }
    // 结构体值 → 指针：栈上分配后取地址（方法调用的 self 参数等）
    if from.starts_with("%struct.") && is_ptr_ty(to) {
        let alloca = ctx.fresh_var();
        let cast = ctx.fresh_var();
        let body = &mut blocks.last_mut().body;
        body.push(format!("{} = alloca {}", alloca, from));
        body.push(format!("store {} {}, {}* {}", from, val, from, alloca));
        body.push(format!("{} = bitcast {}* {} to {}", cast, from, alloca, to));
        return (cast, to.to_string());
    }
    // i8* → 字符串结构体：用 strlen 补长度
    if from == "i8*" && to.starts_with('{') && to.contains("i8*") && to.contains("i64") {
        let len = ctx.fresh_var();
        let with_len = ctx.fresh_var();
        let with_ptr = ctx.fresh_var();
        let body = &mut blocks.last_mut().body;
        body.push(format!(
            "{} = call i64 @aura_string_length(i8* {})",
            len, val
        ));
        body.push(format!(
            "{} = insertvalue {} undef, i8* {}, 0",
            with_ptr, to, val
        ));
        body.push(format!(
            "{} = insertvalue {} {}, i64 {}, 1",
            with_len, to, with_ptr, len
        ));
        return (with_len, to.to_string());
    }
    // 指针 ↔ 整数
    if is_ptr_ty(to) && is_int_ty(from) {
        let t = ctx.fresh_var();
        emit(
            &mut blocks.last_mut().body,
            format!("{} = inttoptr {} {} to {}", t, from, val, to),
        );
        return (t, to.to_string());
    }
    if is_ptr_ty(from) && is_int_ty(to) {
        let t = ctx.fresh_var();
        emit(
            &mut blocks.last_mut().body,
            format!("{} = ptrtoint {} {} to {}", t, from, val, to),
        );
        return (t, to.to_string());
    }
    // 整数宽度
    if is_int_ty(from) && is_int_ty(to) {
        let (fb, tb) = (int_bits(from), int_bits(to));
        if fb == tb {
            return (val, from.to_string());
        }
        let t = ctx.fresh_var();
        let op = if fb < tb { "zext" } else { "trunc" };
        emit(
            &mut blocks.last_mut().body,
            format!("{} = {} {} {} to {}", t, op, from, val, to),
        );
        return (t, to.to_string());
    }
    // 整数 ↔ 浮点
    if is_int_ty(from) && is_float_ty(to) {
        let t = ctx.fresh_var();
        emit(
            &mut blocks.last_mut().body,
            format!("{} = sitofp {} {} to {}", t, from, val, to),
        );
        return (t, to.to_string());
    }
    if is_float_ty(from) && is_int_ty(to) {
        let t = ctx.fresh_var();
        emit(
            &mut blocks.last_mut().body,
            format!("{} = fptosi {} {} to {}", t, from, val, to),
        );
        return (t, to.to_string());
    }
    // 浮点宽度
    if is_float_ty(from) && is_float_ty(to) && from != to {
        let t = ctx.fresh_var();
        let op = if from == "float" { "fpext" } else { "fptrunc" };
        emit(
            &mut blocks.last_mut().body,
            format!("{} = {} {} {} to {}", t, op, from, val, to),
        );
        return (t, to.to_string());
    }
    (val, from.to_string())
}

/// 数值类型转换（整数 ↔ 浮点、float ↔ double、整数宽度调整）。
/// 无法转换时原样返回，交由后续 llc 报错暴露。
fn emit_numeric_convert(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    val: &str,
    from: &str,
    to: &str,
) -> String {
    if from == to || from.is_empty() || to.is_empty() {
        return val.to_string();
    }
    let t = ctx.fresh_var();
    let cur = blocks.last_mut();
    if is_int_ty(from) && is_float_ty(to) {
        cur.body.push(format!("{} = sitofp {} {} to {}", t, from, val, to));
    } else if is_float_ty(from) && is_int_ty(to) {
        cur.body.push(format!("{} = fptosi {} {} to {}", t, from, val, to));
    } else if from == "float" && to == "double" {
        cur.body.push(format!("{} = fpext float {} to double", t, val));
    } else if from == "double" && to == "float" {
        cur.body.push(format!("{} = fptrunc double {} to float", t, val));
    } else if is_int_ty(from) && is_int_ty(to) {
        let (fb, tb) = (int_bits(from), int_bits(to));
        if fb == tb {
            return val.to_string();
        }
        let op = if fb < tb { "zext" } else { "trunc" };
        cur.body.push(format!("{} = {} {} {} to {}", t, op, from, val, to));
    } else {
        return val.to_string();
    }
    t
}

/// P9: 生成结构体构造函数代码
fn emit_struct_constructor(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    struct_name: &str,
    args: &[HirExpr],
) -> Result<(String, String), AotError> {
    let struct_type = ctx.map_type(&HirType::Named(struct_name.to_string()));
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

    // 普通成员访问：使用 extractvalue 从结构体值中提取字段
    let (obj_ir, obj_ty) = emit_expr_val(ctx, blocks, object)?;
    let tmp = ctx.fresh_var();
    let cur = blocks.last_mut();

    // 查找字段类型和索引：遍历所有类字段，找到匹配的字段名
    let (field_llvm_ty, field_idx) = ctx
        .class_field_types
        .values()
        .find_map(|fields| fields.get(name).map(|(ty, idx)| (ty.clone(), *idx)))
        .unwrap_or_else(|| ("i32".to_string(), 0));

    // 判断对象是指针还是值
    let is_pointer = obj_ty.ends_with('*') || obj_ty == "i8*" || obj_ty == "ptr";

    if is_pointer {
        // 对象是指针：先 load 结构体值，再 extractvalue
        let gep = ctx.fresh_var();
        let loaded = ctx.fresh_var();
        let struct_type = ctx
            .class_field_types
            .keys()
            .find(|k| ctx.class_field_types[*k].contains_key(&name.to_string()))
            .map(|k| format!("%struct.{}", sanitizellvm(k)))
            .unwrap_or_else(|| "i8*".to_string());

        if struct_type != "i8*" {
            // 对象指针可能是 i8*（self 参数等），先 bitcast 到具体结构体指针再 load
            let cast = ctx.fresh_var();
            cur.body.push(format!(
                "{} = bitcast {} {} to {}*",
                cast, obj_ty, obj_ir, struct_type
            ));
            cur.body.push(format!(
                "{} = load {}, {}* {}",
                loaded, struct_type, struct_type, cast
            ));
            cur.body.push(format!(
                "{} = extractvalue {} {}, {}",
                tmp, struct_type, loaded, field_idx
            ));
        } else {
            // 回退到 i8* 方式
            cur.body.push(format!("{} = getelementptr i8, i8* {}, i64 0", gep, obj_ir));
            cur.body.push(format!(
                "{} = load {}, {}* {}",
                tmp, field_llvm_ty, field_llvm_ty, gep
            ));
        }
    } else {
        // 对象是结构体值：直接使用 extractvalue
        let struct_type = ctx
            .class_field_types
            .keys()
            .find(|k| ctx.class_field_types[*k].contains_key(&name.to_string()))
            .map(|k| format!("%struct.{}", sanitizellvm(k)))
            .unwrap_or_else(|| obj_ty.clone());

        cur.body.push(format!(
            "{} = extractvalue {} {}, {}",
            tmp, struct_type, obj_ir, field_idx
        ));
    }
    Ok((tmp, field_llvm_ty))
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
    blocks: &mut FuncBlocks,
    type_name: &str,
    args: &[HirExpr],
) -> Result<(String, String), AotError> {
    // 评估所有构造器参数
    let arg_values: Vec<(String, String)> =
        args.iter().map(|a| emit_expr_val(ctx, blocks, a)).collect::<Result<_, _>>()?;

    // 获取结构体类型名
    let llvm_struct_type = format!("%struct.{}", sanitizellvm(type_name));

    // 使用 insertvalue 构建结构体值（与 emit_struct_constructor 一致）
    let cur = blocks.last_mut();
    let mut struct_val = "undef".to_string();
    for (i, (val, ty)) in arg_values.iter().enumerate() {
        let new_val = ctx.fresh_var();
        cur.body.push(format!(
            "{} = insertvalue {} {}, {} {}, {}",
            new_val, llvm_struct_type, struct_val, ty, val, i
        ));
        struct_val = new_val.clone();
    }

    Ok((struct_val, llvm_struct_type))
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
    // 获取 then 表达式的实际最后块名（可能是嵌套 if 的 merge 块）
    let then_actual_block =
        blocks.blocks.last().map(|bb| bb.name.clone()).unwrap_or(then_name.clone());
    // 设置 then 块终止符
    {
        let cur = blocks.last_mut();
        cur.terminator = Some(format!("br label %{}", merge_name));
    }

    // Else 块
    let _ = blocks.add_block_named(&else_name);
    let (else_ir, else_ty) = emit_expr_val(ctx, blocks, else_e)?;
    // 获取 else 表达式的实际最后块名（可能是嵌套 if 的 merge 块）
    let else_actual_block =
        blocks.blocks.last().map(|bb| bb.name.clone()).unwrap_or(else_name.clone());
    // 设置 else 块终止符（分支到 merge）
    {
        let cur = blocks.last_mut();
        cur.terminator = Some(format!("br label %{}", merge_name));
    }

    // 类型协调：如果一边是可空结构体而另一边是普通类型，提取内部值
    let (phi_type, then_phi_val, else_phi_val) =
        if is_nullable_struct_type(&then_ty) && !is_nullable_struct_type(&else_ty) {
            // then 分支是可空结构体，提取内部值
            let inner_ty = extract_inner_type(&then_ty);
            let extract_var = ctx.fresh_var();
            // 插入 extractvalue 到 then 块（在终止符之前）
            insert_before_terminator(
                blocks,
                &then_actual_block,
                format!("{} = extractvalue {} {}, 0", extract_var, then_ty, then_ir),
            );
            (inner_ty, extract_var, else_ir)
        } else if is_nullable_struct_type(&else_ty) && !is_nullable_struct_type(&then_ty) {
            // else 分支是可空结构体，提取内部值
            let inner_ty = extract_inner_type(&else_ty);
            let extract_var = ctx.fresh_var();
            insert_before_terminator(
                blocks,
                &else_actual_block,
                format!("{} = extractvalue {} {}, 0", extract_var, else_ty, else_ir),
            );
            (inner_ty, then_ir, extract_var)
        } else if then_ty.starts_with("{ i8*") && else_ty == "i8*" {
            // then 分支是字符串结构体，else 分支是字符串指针 → 将指针包装为结构体
            let len_var = ctx.fresh_var();
            let wrap_var = ctx.fresh_var();
            let wrap_var2 = ctx.fresh_var();
            insert_before_terminator(
                blocks,
                &else_actual_block,
                format!(
                    "{} = call i64 @aura_string_length(i8* {})",
                    len_var, else_ir
                ),
            );
            insert_before_terminator(
                blocks,
                &else_actual_block,
                format!(
                    "{} = insertvalue {} undef, i8* {}, 0",
                    wrap_var, then_ty, else_ir
                ),
            );
            insert_before_terminator(
                blocks,
                &else_actual_block,
                format!(
                    "{} = insertvalue {} {}, i64 {}, 1",
                    wrap_var2, then_ty, wrap_var, len_var
                ),
            );
            (then_ty.clone(), then_ir, wrap_var2)
        } else if else_ty.starts_with("{ i8*") && then_ty == "i8*" {
            // then 分支是字符串指针，else 分支是字符串结构体 → 将指针包装为结构体
            let len_var = ctx.fresh_var();
            let wrap_var = ctx.fresh_var();
            let wrap_var2 = ctx.fresh_var();
            insert_before_terminator(
                blocks,
                &then_actual_block,
                format!(
                    "{} = call i64 @aura_string_length(i8* {})",
                    len_var, then_ir
                ),
            );
            insert_before_terminator(
                blocks,
                &then_actual_block,
                format!(
                    "{} = insertvalue {} undef, i8* {}, 0",
                    wrap_var, else_ty, then_ir
                ),
            );
            insert_before_terminator(
                blocks,
                &then_actual_block,
                format!(
                    "{} = insertvalue {} {}, i64 {}, 1",
                    wrap_var2, else_ty, wrap_var, len_var
                ),
            );
            (else_ty.clone(), wrap_var2, else_ir)
        } else {
            // 类型相同，直接使用
            let ty = then_ty.clone();
            let then_val = if is_numeric_type(&ty) {
                let phi_val = ctx.fresh_var();
                let then_operand = if then_ir == "null" { "0".to_string() } else { then_ir };
                insert_before_terminator(
                    blocks,
                    &then_actual_block,
                    format!("{} = add {} {}, 0", phi_val, ty, then_operand),
                );
                phi_val
            } else {
                then_ir
            };
            let else_val = if is_numeric_type(&ty) {
                let phi_val = ctx.fresh_var();
                let else_operand = if else_ir == "null" { "0".to_string() } else { else_ir };
                insert_before_terminator(
                    blocks,
                    &else_actual_block,
                    format!("{} = add {} {}, 0", phi_val, ty, else_operand),
                );
                phi_val
            } else {
                else_ir
            };
            (ty, then_val, else_val)
        };

    // Merge 块 + PHI 节点
    let _ = blocks.add_block_named(&merge_name);
    let phi_name = ctx.fresh_var();
    {
        let cur = blocks.last_mut();
        cur.body.push(format!(
            "{} = phi {} [{} ,%{}], [{} ,%{}]",
            phi_name, phi_type, then_phi_val, then_actual_block, else_phi_val, else_actual_block
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
        _ if ty.starts_with("{ ") && ty.ends_with(" i1 }") => {
            // 可空结构体 { T, i1 }：值部分为零，is_null 标志为 true
            // 例如 { i32 0, i1 true }
            // 提取内部类型
            let inner = &ty[2..ty.len() - 6]; // 去掉 "{ " 和 ", i1 }"
            match inner.trim() {
                "i1" => "{ i1 false, i1 true }",
                "i8" | "i16" | "i32" | "i64" | "i128" => {
                    // 需要动态生成，但不能返回引用...
                    // 用静态字符串
                    if inner == "i32" {
                        "{ i32 0, i1 true }"
                    } else if inner == "i64" {
                        "{ i64 0, i1 true }"
                    } else if inner == "i16" {
                        "{ i16 0, i1 true }"
                    } else if inner == "i8" {
                        "{ i8 0, i1 true }"
                    } else {
                        "{ i32 0, i1 true }"
                    }
                }
                "float" => "{ float 0.0, i1 true }",
                "double" => "{ double 0.0, i1 true }",
                _ => "null",
            }
        }
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
const TAG_STR: u8 = 4;
const TAG_PTR: u8 = 5;
const TAG_OBJ: u8 = 6;
const TAG_FUNC: u8 = 7;
const TAG_ARRAY: u8 = 8;
const TAG_LIST: u8 = 9;
const TAG_MAP: u8 = 10;
const TAG_CLOSURE: u8 = 11;
const TAG_CSTRING: u8 = 12;

/// Phase 2 参数个数上限（设计文档 §5.3）
const MAX_AOT_ARGS: usize = 8;

/// 将 Aura HIR 类型映射为 JitValue 标签（Phase 2: 完整类型支持）
///
/// 设计文档 §4.4 / §6.4:
/// - 标量: Int/Float/Bool/Unit → TAG_INT/TAG_FLOAT/TAG_BOOL/TAG_NULL
/// - 引用: String → TAG_STR, Pointer<T> → TAG_PTR, Array/List/Map → TAG_ARRAY/TAG_LIST/TAG_MAP
/// - 闭包: Closure → TAG_CLOSURE, Function → TAG_FUNC
/// - C ABI: CString → TAG_CSTRING
fn map_type_to_tag(ty: &HirType) -> Result<u8, AotError> {
    match ty {
        HirType::Named(name) => match name.as_str() {
            "Int" | "Long" | "Short" | "Byte" | "U8" | "Char" => Ok(TAG_INT),
            "Float" | "Double" => Ok(TAG_FLOAT),
            "Boolean" | "Bool" => Ok(TAG_BOOL),
            "Unit" | "Void" | "Nothing" => Ok(TAG_NULL),
            "String" | "Str" => Ok(TAG_STR),
            "CString" | "CStr" => Ok(TAG_CSTRING),
            "List" => Ok(TAG_LIST),
            "Map" => Ok(TAG_MAP),
            "Array" => Ok(TAG_ARRAY),
            "Closure" | "Lambda" => Ok(TAG_CLOSURE),
            "Any" => Ok(TAG_OBJ), // Any 作为泛化对象指针
            other => Err(AotError::UnsupportedExpr(format!(
                "AOT 不支持类型 '{}'（已支持: Int/Float/Bool/Unit/String/Pointer/List/Map/Array/Closure）",
                other
            ))),
        },
        HirType::Pointer(_) => Ok(TAG_PTR),
        HirType::Function { .. } => Ok(TAG_FUNC),
        HirType::Nullable(inner) => map_type_to_tag(inner),
        HirType::Unknown => Err(AotError::UnsupportedExpr(
            "AOT 不支持 Unknown 类型".to_string(),
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
    let ret_llvm_ty =
        func.ret.as_ref().map(|t| ctx.llvm_type(t)).unwrap_or_else(|| "void".to_string());

    let param_llvm_types: Vec<String> = func
        .params
        .iter()
        .map(|p| ctx.llvm_type(p.ty.as_ref().unwrap_or(&HirType::Named("Int".into()))))
        .collect();

    // 6. 生成包装函数体
    let mut s = String::new();
    // SharedLibrary 模式：以 external dllexport 导出，供 dlsym/GetProcAddress 查找
    // Blob 模式：以 internal linkage 生成（仅供 blob 内部分派发）
    let linkage = if ctx.wrapper_exported { "external dllexport" } else { "internal" };
    s.push_str(&format!(
        "define {} i64 @\"{}\"(i64* %args, i64* %ret, i64 %argc, i64* %ctx) {{\n",
        linkage, wrapper_name
    ));
    s.push_str("entry:\n");

    // 6a. 解包参数
    let mut call_args: Vec<String> = Vec::new();
    for (i, (llvm_ty, tag)) in param_llvm_types.iter().zip(param_tags.iter()).enumerate() {
        let payload_idx = (i * 2 + 1).to_string();
        let gep_var = format!("%arg{}_gep", i);
        let load_var = format!("%arg{}_payload", i);
        let mut val_var = format!("%arg{}_val", i);

        // 加载 payload: args[2*i+1]
        s.push_str(&format!(
            "  {} = getelementptr i64, i64* %args, i64 {}\n",
            gep_var, payload_idx
        ));
        s.push_str(&format!("  {} = load i64, i64* {}\n", load_var, gep_var));

        // 根据类型转换 payload（设计文档 §6.4）
        match *tag {
            TAG_INT => {
                // i64 → target type (trunc/zext)
                let target_bits = llvm_ty.trim_start_matches('i').parse::<usize>().unwrap_or(32);
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
                s.push_str(&format!("  {} = trunc i64 {} to i1\n", val_var, load_var));
            }
            TAG_NULL => {
                // void: no parameter
                continue;
            }
            // ── Phase 2: 引用/指针类型 — payload = 指针值 (i64)，inttoptr 转为 LLVM 指针 ──
            TAG_STR | TAG_PTR | TAG_OBJ | TAG_ARRAY | TAG_LIST | TAG_MAP | TAG_CLOSURE
            | TAG_CSTRING => {
                // payload 是 i64 指针值，转为 LLVM 指针类型
                let inttoptr_var = format!("%arg{}_ptr", i);
                s.push_str(&format!(
                    "  {} = inttoptr i64 {} to {}\n",
                    inttoptr_var, load_var, llvm_ty
                ));
                // val_var = inttoptr 后的指针
                // 注意：这里 val_var 直接赋值为 inttoptr_var
                val_var = inttoptr_var;
            }
            TAG_FUNC => {
                // 函数索引: payload 是 i64 函数索引
                // LLVM 中函数类型通过指针表示，这里用 i64 地址
                let inttoptr_var = format!("%arg{}_ptr", i);
                s.push_str(&format!(
                    "  {} = inttoptr i64 {} to {}\n",
                    inttoptr_var, load_var, llvm_ty
                ));
                val_var = inttoptr_var;
            }
            _ => unreachable!(),
        }
        call_args.push(format!("{} {}", llvm_ty, val_var));
    }

    // 6b. 调用真实函数
    let call_var = ctx.fresh_var();
    let args_str = call_args.join(", ");
    if ret_llvm_ty == "void" || ret_llvm_ty.is_empty() {
        s.push_str(&format!("  call void @{}({})\n", func.name, args_str));
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
    s.push_str(&format!("  store i64 {}, i64* %ret_tag_addr\n", ret_tag));

    // 存储 payload
    match ret_tag {
        TAG_INT => {
            // i32/i16/i8 → i64 (zext); i64 → i64 (no conversion)
            let target_bits = ret_llvm_ty.trim_start_matches('i').parse::<usize>().unwrap_or(32);
            if target_bits < 64 {
                s.push_str(&format!(
                    "  %ret_payload = zext {} {} to i64\n",
                    ret_llvm_ty, call_var
                ));
            } else {
                s.push_str(&format!("  %ret_payload = add i64 {}, 0\n", call_var));
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
            s.push_str(&format!("  %ret_payload = zext i1 {} to i64\n", call_var));
        }
        TAG_NULL => {
            // void: payload = 0
            s.push_str("  %ret_payload = add i64 0, 0\n");
        }
        // ── Phase 2: 引用/指针类型 — ptrtoint 转为 i64 ──
        TAG_STR | TAG_PTR | TAG_OBJ | TAG_ARRAY | TAG_LIST | TAG_MAP | TAG_CLOSURE
        | TAG_CSTRING | TAG_FUNC => {
            if ret_llvm_ty.starts_with('{') {
                // 字符串等以「结构体」表示的值：取第 0 个字段（数据指针）转 i64
                s.push_str(&format!(
                    "  %ret_ptr = extractvalue {} {}, 0\n",
                    ret_llvm_ty, call_var
                ));
                s.push_str("  %ret_payload = ptrtoint i8* %ret_ptr to i64\n");
            } else {
                // 返回值是指针类型，用 ptrtoint 转为 i64
                s.push_str(&format!(
                    "  %ret_payload = ptrtoint {} {} to i64\n",
                    ret_llvm_ty, call_var
                ));
            }
        }
        _ => unreachable!(),
    }

    s.push_str("  store i64 %ret_payload, i64* %ret_payload_addr\n");

    // 6d. 异常字段初始化（Phase 2.6: 设计文档 §2.6）
    // 简化实现: VM 端 ctx.exception 已在 AotCallContext::new() 初始化为 0
    // 包装函数无需显式写入；异常传播机制待 Phase 3 完善

    // 6e. 返回 0（正常完成）
    s.push_str("  ret i64 0\n");
    s.push_str("}\n\n");

    Ok(s)
}

/// 生成 C ABI 包装函数（裸 C 调用约定，供外部 C/Python 消费者调用）
///
/// 当 `--cabi` 标志启用时，为每个非 native 函数生成一个 C ABI 包装函数。
/// 包装函数名格式：`aura_c_<funcname>`，直接调用原始 Aura 函数。
///
/// 示例：
/// ```llvm
/// define dso_local dllexport i32 @"aura_c_add"(i32 %a, i32 %b) {
///   %ret = call i32 @add(i32 %a, i32 %b)
///   ret i32 %ret
/// }
/// ```
fn emit_c_abi_wrapper(ctx: &mut EmitCtx, func: &HirFunction) -> Result<String, AotError> {
    // 1. 获取返回类型
    let ret_llvm_ty =
        func.ret.as_ref().map(|t| ctx.llvm_type(t)).unwrap_or_else(|| "void".to_string());
    let ret_str = if ret_llvm_ty.is_empty() { "void".to_string() } else { ret_llvm_ty.clone() };

    // 2. 获取参数类型和名称
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

    // 3. 构建 C ABI 包装函数名
    let c_wrapper_name = format!("aura_c_{}", sanitizellvm(&func.name));

    // 4. 生成包装函数体
    let mut s = String::new();

    // Windows: dso_local dllexport; Unix: dso_local visibility("default")
    let linkage =
        if ctx.target_triple.contains("windows") { "dso_local dllexport" } else { "dso_local" };
    let visibility =
        if ctx.target_triple.contains("windows") { "" } else { " visibility(\"default\")" };

    s.push_str(&format!(
        "define {}{} {} @\"{}\"({}) {{\n",
        linkage, visibility, ret_str, c_wrapper_name, params_str
    ));
    s.push_str("entry:\n");

    // 5. 调用原始函数
    let args_str = params
        .iter()
        .map(|(name, ty)| format!("{} %arg.{}", ty, sanitizellvm(name)))
        .collect::<Vec<_>>()
        .join(", ");

    let call_var = ctx.fresh_var();
    if ret_llvm_ty == "void" || ret_llvm_ty.is_empty() {
        s.push_str(&format!("  call void @{}({})\n", func.name, args_str));
    } else {
        s.push_str(&format!(
            "  {} = call {} @{}({})\n",
            call_var, ret_llvm_ty, func.name, args_str
        ));
    }

    // 6. 返回结果
    if ret_llvm_ty == "void" || ret_llvm_ty.is_empty() {
        s.push_str("  ret void\n");
    } else {
        s.push_str(&format!("  ret {} {}\n", ret_llvm_ty, call_var));
    }

    s.push_str("}\n\n");

    Ok(s)
}
