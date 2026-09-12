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
    /// 当前正在发射的函数所属的类（方法名形如 `Lexer.peek`）；自由函数为 None。
    /// 用于把 `this` / `self` 的成员访问解析到**当前类**的字段表，
    /// 避免落入「按字段名全局扫描」的启发式而选到别的类的同名字段。
    pub current_class: Option<String>,
    /// 已知（会在模块中 `= type` 定义）的结构体 / 枚举名。
    /// 引用到未定义结构体时统一退化为 `i8*`，避免出现 unsized 类型
    ///（`%struct.ClosureManager` 等只在字段中被引用、从未定义）。
    pub known_structs: std::collections::HashSet<String>,
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
    /// 类/结构体字段默认值：类名 → [(字段索引, 字段 LLVM 类型, 默认值表达式)]。
    ///
    /// AOT 的 `New` 必须显式写入默认值（对齐 MIR 路径的「Alloc + 字段默认值 +
    /// __ctorN」）：否则未被构造参数或 init 块赋值的字段保持 `undef`，运行期读到
    /// 随机内存，表现为**非确定性**结果（同一二进制每次运行数值都不同）。
    pub class_defaults: HashMap<String, Vec<(usize, String, HirExpr)>>,
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
            current_class: None,
            known_structs: std::collections::HashSet::new(),
            type_aliases: HashMap::new(),
            enum_variants: HashMap::new(),
            enum_max_fields: HashMap::new(),
            lambda_counter: 0,
            lambda_funcs: Vec::new(),
            class_defaults: HashMap::new(),
        }
    }

    /// 类型映射（带「未定义结构体 → `i8*`」降级）。
    ///
    /// 与 `map_type` 的区别：若映射结果是 `%struct.X` 而 `X` 不在
    /// `known_structs` 中（未被定义），返回 `i8*`，避免生成 unsized 类型引用。
    pub fn llvm_type_checked(&self, ty: &HirType) -> String {
        let l = self.llvm_type(ty);
        match l.strip_prefix("%struct.") {
            Some(name) if !self.known_structs.contains(name) => "i8*".to_string(),
            _ => l,
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
        // 已知（会被定义）的结构体 / 枚举名。字段若引用到**未定义**的结构体
        // （例如某个类的字段类型来自未被 import 的模块），会生成
        // `%struct.VmRunner = type { …, %struct.FrameManager, … }` —— 而
        // `%struct.FrameManager` 从未 `= type`，属 unpaged/unsized 类型，
        // 导致后续 GEP 报 `base element of getelementptr must be sized`。
        // 这类字段统一退化为不透明指针 `i8*`。
        let mut known: std::collections::HashSet<String> = std::collections::HashSet::new();
        for st in &program.structs {
            known.insert(st.name.clone());
        }
        for e in &program.enums {
            known.insert(e.name.clone());
        }
        for st in &program.structs {
            let llvm_name = format!("%struct.{}", sanitizellvm(&st.name));
            if !self.declared_structs.insert(llvm_name.clone()) {
                continue;
            }
            let fields: Vec<String> = st
                .fields
                .iter()
                .map(|(_, ty)| {
                    let l = self.llvm_type(ty);
                    match l.strip_prefix("%struct.") {
                        Some(name) if !known.contains(name) => "i8*".to_string(),
                        _ => l,
                    }
                })
                .collect();
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
        self.sections.push(crate::codegen::aot::runtime::runtime_definitions());
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

    // 已知结构体/枚举名：字段/返回类型引用到未定义结构体时按 `i8*` 处理，
    // 与 emit_struct_defs 保持一致。
    //
    // 必须在「预注册函数返回类型」之前构建：`llvm_type_checked` 依据
    // `known_structs` 决定「未定义的 %struct.X → i8*」降级。若此时集合为空，
    // 类返回值的函数会被登记为 `i8*`，而 `emit_function`（在其后执行、集合
    // 已填充）却把定义生成为 `%struct.X`（按值返回，MSVC 走隐藏 sret 指针）
    // → 调用点与定义 ABI 不一致，实参整体错位。
    let mut known_structs: std::collections::HashSet<String> = std::collections::HashSet::new();
    for st in &program.structs {
        known_structs.insert(st.name.clone());
    }
    for e in &program.enums {
        known_structs.insert(e.name.clone());
    }
    ctx.known_structs = known_structs.clone();

    // 4.8 预注册函数返回类型映射（供 emit_call 推断返回类型）
    for func in &program.functions {
        if let Some(ref ret) = func.ret {
            ctx.func_ret_types.insert(func.name.clone(), ctx.llvm_type_checked(ret));
        } else {
            ctx.func_ret_types.insert(func.name.clone(), "void".to_string());
        }
        let param_tys: Vec<String> = func
            .params
            .iter()
            .map(|p| {
                p.ty.as_ref().map(|t| ctx.llvm_type_checked(t)).unwrap_or_else(|| "i32".to_string())
            })
            .collect();
        ctx.func_param_types.insert(func.name.clone(), param_tys);
    }
    // 预注册类字段类型映射（供 emit_member_access 推断字段类型和索引）
    for struct_def in &program.structs {
        // 与 `emit_struct_defs` 保持一致：同名结构体的 `%struct.X` 布局以**首个定义**为准，
        // 因此字段表也必须「首个定义优先」。若此处用 insert 覆盖成最后一个定义，
        // 字段类型/索引会与实际 `%struct.X` 布局不符 → 非法 IR
        //（如 `'%var.2' defined with type '{ ptr, i64 }' but expected 'i32'`）。
        if ctx.class_field_types.contains_key(&struct_def.name) {
            continue;
        }
        let mut field_map = HashMap::new();
        let mut defaults: Vec<(usize, String, HirExpr)> = Vec::new();
        for (fi, (field_name, field_ty)) in struct_def.fields.iter().enumerate() {
            let l = ctx.map_type(field_ty);
            let l = match l.strip_prefix("%struct.") {
                Some(name) if !known_structs.contains(name) => "i8*".to_string(),
                _ => l,
            };
            // 字段默认值：AOT 构造（New）时必须显式写入，见 class_defaults 注释。
            if let Some(Some(dv)) = struct_def.default_values.get(fi) {
                defaults.push((fi, l.clone(), (**dv).clone()));
            }
            field_map.insert(field_name.clone(), (l, fi));
        }
        ctx.class_field_types.insert(struct_def.name.clone(), field_map);
        if !defaults.is_empty() {
            ctx.class_defaults.insert(struct_def.name.clone(), defaults);
        }
    }
    // 预注册类型别名表（供 AOT 解析 typealias）
    for (alias_name, alias_ty) in &program.type_aliases {
        ctx.type_aliases.insert(alias_name.clone(), ctx.map_type(alias_ty));
    }
    for native in &program.natives {
        // 内置 runtime 函数（如 aura.lang.std.String.length → aura_string_length）使用 runtime 签名，
        // 保证调用点与 emit_runtime 声明类型一致
        let native_sym = sanitizellvm(&native.name);
        if let Some((ret, params)) = crate::codegen::aot::runtime::runtime_signature(&native_sym) {
            ctx.func_ret_types.insert(native.name.clone(), ret.to_string());
            ctx.func_param_types.insert(
                native.name.clone(),
                params.iter().map(|t| t.to_string()).collect(),
            );
            continue;
        }
        // C FFI 实现函数（aura_math_* 等）：HIR 的 Any 类型退化为 i8*，
        // 用真实 C ABI 类型覆盖，与 aura_std_cffi.c 中的定义保持一致
        if let Some((ret, params)) = crate::codegen::aot::runtime::cffi_signature(&native_sym) {
            ctx.func_ret_types.insert(native.name.clone(), ret.to_string());
            ctx.func_param_types.insert(
                native.name.clone(),
                params.iter().map(|t| t.to_string()).collect(),
            );
            continue;
        }
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
    let ret_ty =
        func.ret.as_ref().map(|t| ctx.llvm_type_checked(t)).unwrap_or_else(|| "void".to_string());
    let ret_str = if ret_ty.is_empty() { "void".to_string() } else { ret_ty };
    ctx.current_ret_ty = ret_str.clone();

    // 方法名形如 `Lexer.peek` → 当前类 `Lexer`（供 `this`/`self` 成员解析）
    ctx.current_class = func.name.rsplit_once('.').map(|(cls, _)| cls.to_string());

    // 参数类型
    let params: Vec<(String, String)> = func
        .params
        .iter()
        .map(|p| {
            let ty = ctx.llvm_type_checked(p.ty.as_ref().unwrap_or(&HirType::Named("Int".into())));
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
        sanitizellvm(&func.name),
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
                let want = ctx.current_ret_ty.clone();
                if want.is_empty() || want == "void" {
                    // void 函数中的 `return expr`（含末尾表达式转 Return）：丢弃值，统一 ret void，
                    // 避免 define void 与 ret i32 类型不匹配
                    let _ = emit_expr_val(ctx, blocks, v)?;
                    blocks.set_terminator("ret void");
                } else {
                    let (val_ir, val_ty) = emit_expr_val(ctx, blocks, v)?;
                    // 返回值类型必须与函数签名的返回类型一致（如 Float 函数里 `return 0.0`）
                    let converted = coerce_arg(ctx, blocks, val_ir, &val_ty, &want);
                    blocks.set_terminator(&format!("ret {} {}", converted.1, converted.0));
                }
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
        HirStmt::Try {
            body,
            catch_var: _,
            catch_body: _,
            finally,
        } => {
            // AOT 后端暂无异常运行时（见 README「已知差异」）：
            // 按正常路径发射 try 体，随后执行 finally；catch 子句不可达。
            emit_block(ctx, blocks, body)?;
            if let Some(fin) = finally {
                emit_block(ctx, blocks, fin)?;
            }
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
        eprintln!(
            "[DBG emit_variable_decl] name={}, inferred val_ty={}",
            name, val_ty
        );
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

/// 整数宽度适配：把 `val_ir`（类型 `val_ty`）转换为 `dst_ty` 所需的整数宽度。
///
/// 典型场景：`String.length` 产出 `i64`，而 `Int` 变量槽是 `i32`。不做转换会生成
/// `store i64 %x, i32* %p` —— 虽然 LLVM 23 接受该 IR，但运行期会向 4 字节槽位写入
/// 8 字节，破坏相邻栈槽（表现为难以定位的访问违例）。
fn coerce_int_width(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    val_ir: &str,
    val_ty: &str,
    dst_ty: &str,
) -> Option<String> {
    fn bits(t: &str) -> Option<u32> {
        t.strip_prefix('i').and_then(|r| r.parse::<u32>().ok())
    }
    // 指针 → 整数：`store i8* %x, i32* %p` 会向 4 字节槽写 8 字节（越界破坏相邻槽）。
    // 先 ptrtoint 到 i64，再按目标宽度截断。
    if is_ptr_ty(val_ty) {
        let dst_bits = bits(dst_ty)?;
        let pi = ctx.fresh_var();
        blocks.last_mut().body.push(format!("{} = ptrtoint {} {} to i64", pi, val_ty, val_ir));
        // Plan A：列表 / Any 中存放的整数是低位标记装箱值 `(v<<1)|1`（真实指针恒为
        // 偶数）。必须经 `aura_to_int_any` 拆箱，否则读出来的是 `2v+1`
        // （如 `list[i]` 取 7 会得到 15）。
        let unboxed = ctx.fresh_var();
        blocks.last_mut().body.push(format!(
            "{} = call i64 @aura_to_int_any(i64 {})",
            unboxed, pi
        ));
        if dst_bits == 64 {
            return Some(unboxed);
        }
        let out = ctx.fresh_var();
        blocks.last_mut().body.push(format!("{} = trunc i64 {} to {}", out, unboxed, dst_ty));
        return Some(out);
    }
    let (Some(src_bits), Some(dst_bits)) = (bits(val_ty), bits(dst_ty)) else {
        return None;
    };
    if src_bits == dst_bits {
        return None;
    }
    let out = ctx.fresh_var();
    let op = if src_bits > dst_bits {
        "trunc"
    } else if src_bits == 1 {
        // i1 → iN：布尔真值零扩展
        "zext"
    } else {
        // 一般整型：Aura 的整型是有符号的，按符号扩展
        "sext"
    };
    blocks.last_mut().body.push(format!(
        "{} = {} {} {} to {}",
        out, op, val_ty, val_ir, dst_ty
    ));
    Some(out)
}

/// 将值存储到目标类型中，处理可空结构体 { T, i1 } 的包装/提取与整数宽度适配
fn emit_store_converted(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    dst_ty: &str,
    val_ir: &str,
    val_ty: &str,
    var_name: &str,
) {
    if is_nullable_struct_type(dst_ty) && val_ty != dst_ty {
        let cur = blocks.last_mut();
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
        let cur = blocks.last_mut();
        cur.body.push(format!(
            "{} = extractvalue {} {}, 0",
            extract_var, val_ty, val_ir
        ));
        cur.body.push(format!(
            "store {} {} , {}* {}",
            inner_ty, extract_var, dst_ty, var_name
        ));
    } else if is_string_struct_ty(dst_ty) && val_ty == "i8*" {
        // i8* → 字符串结构体 `{ i8*, i64 }`：先经 `aura_to_str_any` 解析
        // （Plan A 下 i8* 可能是装箱整数），再用 strlen 补出长度字段。
        //
        // 若不处理，会退化成 `store i8* %p, { i8*, i64 }* %slot`：只写入 8 字节
        // 指针，长度字段保持栈上的垃圾值 —— 表现为 `list[i].length` 变成随机数、
        // 字符串拼接读到越界数据（`List<String>` 元素读出的核心故障）。
        let sptr = ctx.fresh_var();
        let slen = ctx.fresh_var();
        let w0 = ctx.fresh_var();
        let w1 = ctx.fresh_var();
        let cur = blocks.last_mut();
        cur.body.push(format!(
            "{} = call i8* @aura_to_str_any(i8* {})",
            sptr, val_ir
        ));
        cur.body.push(format!(
            "{} = call i64 @aura_string_length(i8* {})",
            slen, sptr
        ));
        cur.body.push(format!(
            "{} = insertvalue {{ i8*, i64 }} undef, i8* {}, 0",
            w0, sptr
        ));
        cur.body.push(format!(
            "{} = insertvalue {{ i8*, i64 }} {}, i64 {}, 1",
            w1, w0, slen
        ));
        cur.body.push(format!(
            "store {{ i8*, i64 }} {} , {{ i8*, i64 }}* {}",
            w1, var_name
        ));
    } else if val_ty == "{ i8*, i64 }" && dst_ty == "i8*" {
        // 字符串结构体 → i8*：取数据指针（反向转换，避免 `store { i8*, i64 }` 到 `i8**`）
        let dptr = ctx.fresh_var();
        let cur = blocks.last_mut();
        cur.body.push(format!(
            "{} = extractvalue {{ i8*, i64 }} {}, 0",
            dptr, val_ir
        ));
        cur.body.push(format!("store i8* {} , i8** {}", dptr, var_name));
    } else {
        // 指针 → 整数：coerce_int_width 处理 ptrtoint + trunc。
        // 整数宽度不同（如 i64 的 .length 存入 i32 的 Int 槽）必须先转换，
        // 否则会向 4 字节槽写 8 字节，破坏相邻栈槽。
        if is_ptr_ty(val_ty) {
            if let Some(v) = coerce_int_width(ctx, blocks, val_ir, val_ty, dst_ty) {
                blocks
                    .last_mut()
                    .body
                    .push(format!("store {} {} , {}* {}", dst_ty, v, dst_ty, var_name));
            } else {
                blocks.last_mut().body.push(format!(
                    "store {} {} , {}* {}",
                    val_ty, val_ir, dst_ty, var_name
                ));
            }
        } else if val_ty == "i64" && dst_ty == "i32" {
            let out = ctx.fresh_var();
            blocks.last_mut().body.push(format!("{} = trunc i64 {} to i32", out, val_ir));
            blocks.last_mut().body.push(format!("store i32 {} , i32* {}", out, var_name));
        } else if val_ty == "i32" && dst_ty == "i64" {
            let out = ctx.fresh_var();
            blocks.last_mut().body.push(format!("{} = sext i32 {} to i64", out, val_ir));
            blocks.last_mut().body.push(format!("store i64 {} , i64* {}", out, var_name));
        } else {
            blocks.last_mut().body.push(format!(
                "store {} {} , {}* {}",
                val_ty, val_ir, dst_ty, var_name
            ));
        }
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
    // 字段索引 / 类型表：优先按对象静态类型解析（this/self → 当前类；变量 → 其声明类），
    // 再退回按字段名全局扫描
    let found = resolve_member_field_owner(ctx, object, None, field);

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
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    container: &HirExpr,
    index: &HirExpr,
    val_ir: String,
    val_ty: String,
) -> Result<(), AotError> {
    let (c_ir, c_ty) = emit_expr_val(ctx, blocks, container)?;
    let (i_ir, i_ty) = emit_expr_val(ctx, blocks, index)?;

    // 容器不是指针时按不透明指针处理（类型信息缺失导致的退化情形）
    let base = if is_ptr_ty(&c_ty) {
        c_ir.clone()
    } else {
        let p = ctx.fresh_var();
        let cur = blocks.last_mut();
        cur.body.push(format!("{} = inttoptr {} {} to i8*", p, c_ty, c_ir));
        p
    };
    // 值按不透明指针存放（Map 值为 `Any` / 列表元素装箱）
    let (v, _) = coerce_arg(ctx, blocks, val_ir, &val_ty, "i8*");

    if is_string_type(&i_ty) {
        // `Map<String, Any>`：就地写入
        let (key, _) = coerce_arg(ctx, blocks, i_ir, &i_ty, "i8*");
        let cur = blocks.last_mut();
        cur.body.push(format!(
            "call void @aura_lang_std_Collections_mapSet(i8* {}, i8* {}, i8* {})",
            base, key, v
        ));
    } else {
        // 列表按下标写入
        let idx = if is_int_ty(&i_ty) && i_ty != "i64" {
            let cast = ctx.fresh_var();
            let cur = blocks.last_mut();
            cur.body.push(format!("{} = sext {} {} to i64", cast, i_ty, i_ir));
            cast
        } else {
            i_ir.clone()
        };
        let cur = blocks.last_mut();
        cur.body.push(format!(
            "call void @aura_lang_std_Collections_listSet(i8* {}, i64 {}, i8* {})",
            base, idx, v
        ));
    }
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
                current_class: None,
                known_structs: ctx.known_structs.clone(),
                type_aliases: ctx.type_aliases.clone(),
                enum_variants: ctx.enum_variants.clone(),
                enum_max_fields: ctx.enum_max_fields.clone(),
                lambda_counter: ctx.lambda_counter,
                lambda_funcs: Vec::new(),
                class_defaults: ctx.class_defaults.clone(),
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
        // 必须显式指定对齐（≥2）：Plan A 低位标记方案依赖「真实指针恒为偶数」
        // 来区分装箱整数 ((v<<1)|1) 与真实指针。字符数组默认对齐为 1，可能被
        // 链接器放在奇数地址上，从而被 aura_to_str_any 误判为带标记整数。
        ctx.globals.push(format!(
            "{} = private constant [{} x i8] {}, align 16",
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
        eprintln!(
            "[DBG emit_variable_load] name={}, llvm_ty={}",
            name, slot.llvm_ty
        );
        let tmp = ctx.fresh_var();
        let cur = blocks.last_mut();
        cur.body.push(format!(
            "{} = load {}, {}* {}",
            tmp, slot.llvm_ty, slot.llvm_ty, slot.llvm_name
        ));
        Ok((tmp, slot.llvm_ty))
    } else {
        eprintln!("[DBG emit_variable_load] name={} NOT FOUND", name);
        // 未声明变量：作为外部引用（可能是函数调用）
        Ok((name.to_string(), "i32".to_string()))
    }
}

fn is_string_type(ty: &str) -> bool {
    ty == "i8*" || ty == "{ i8*, i64 }"
}

/// 是否为字符串结构体 `{ i8*, i64 }`（区别于以 `i1` 结尾的可空结构体）。
fn is_string_struct_ty(ty: &str) -> bool {
    ty.starts_with('{') && ty.contains("i8*") && ty.contains("i64")
}

/// 是否为数值类型（整数 / 浮点）。
///
/// 注意：必须排除指针与结构体 —— `i8*` 也以 `i` 开头，旧实现
/// `starts_with("i")` 会把它误判为数值，进而生成 `add i8* …, 0`
/// 这类非法 IR（`integer/byte constant must have integer/byte type`）。
fn is_numeric_type(ty: &str) -> bool {
    is_int_ty(ty) || is_float_ty(ty)
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
        // Plan A：i8* 可能是装箱整数（低位标记），必须先用 aura_to_str_any 解析，
        // 否则直接对整数位 strlen 会解引用非法指针而崩溃。
        let sptr = ctx.fresh_var();
        let len_var = ctx.fresh_var();
        let cur = blocks.last_mut();
        cur.body.push(format!(
            "{} = call i8* @aura_to_str_any(i8* {})",
            sptr, val_ir
        ));
        cur.body.push(format!(
            "{} = call i64 @aura_string_length(i8* {})",
            len_var, sptr
        ));
        (sptr, len_var)
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
            // Plan A：与通用装箱一致，打低位标记 (v<<1)|1，
            // 后续 toString（= aura_to_str_any）才能正确解码回数值。
            let boxed = box_int_to_i8ptr(ctx, &mut cur.body, val_ir, val_ty);
            cur.body.push(format!("{} = bitcast i8* {} to i8*", as_ptr, boxed));
        } else {
            // 浮点：调用 toStringFloat(double)（bitcast 到指针会打印地址垃圾值）
            let fvar = ctx.fresh_var();
            cur.body.push(format!(
                "{} = call i8* @toStringFloat(double {})",
                fvar, val_ir
            ));
            cur.body.push(format!(
                "{} = call i64 @aura_string_length(i8* {})",
                len_var, fvar
            ));
            return (fvar, len_var);
        }
        cur.body.push(format!("{} = call i8* @toString(i8* {})", str_var, as_ptr));
        cur.body.push(format!(
            "{} = call i64 @aura_string_length(i8* {})",
            len_var, str_var
        ));
        (str_var, len_var)
    } else {
        // Fallback: 同样按 i8* 处理，但先经 aura_to_str_any（Plan A 装箱整数）
        let sptr = ctx.fresh_var();
        let len_var = ctx.fresh_var();
        let cur = blocks.last_mut();
        cur.body.push(format!(
            "{} = call i8* @aura_to_str_any(i8* {})",
            sptr, val_ir
        ));
        cur.body.push(format!(
            "{} = call i64 @aura_string_length(i8* {})",
            len_var, sptr
        ));
        (sptr, len_var)
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

    // 整数类型统一：Aura `Int`(i32) 与 runtime 返回的 i64 混用时，
    // 直接生成 `add i32, i64` 是非法 IR。统一到**较窄**的整型：
    // AOT 中 Aura 的 `Int` 即 i32，runtime 的 i64 返回值实际按 Int 使用，
    // 且结果可直接存回 i32 变量槽（避免后续 store 类型不符）。
    if !is_float && is_int_ty(&l_ty) && is_int_ty(&r_ty) && l_ty != r_ty {
        let target = if int_bits(&l_ty) <= int_bits(&r_ty) { l_ty.clone() } else { r_ty.clone() };
        l_ir = emit_numeric_convert(ctx, blocks, &l_ir, &l_ty, &target);
        r_ir = emit_numeric_convert(ctx, blocks, &r_ir, &r_ty, &target);
        l_ty = target.clone();
        r_ty = target;
    }

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
            // 字符串内容比较：任一侧是字符串结构体 `{ i8*, i64 }` 时，
            // 按位/指针比较语义都不对（且与 `i8*` 混用时 `icmp` 直接非法），
            // 统一降级为 runtime 内容比较 `aura_lang_std_String_equals`。
            {
                let is_struct_str =
                    |t: &str| t.starts_with('{') && t.contains("i8*") && t.contains("i64");
                let is_str_like = |t: &str| is_struct_str(t) || t == "i8*";
                if matches!(op, HirBinOp::Eq | HirBinOp::Ne)
                    && is_str_like(&l_ty)
                    && is_str_like(&r_ty)
                    && (is_struct_str(&l_ty) || is_struct_str(&r_ty))
                {
                    let (lv, _) = coerce_arg(ctx, blocks, l_ir.clone(), &l_ty, "i8*");
                    let (rv, _) = coerce_arg(ctx, blocks, r_ir.clone(), &r_ty, "i8*");
                    let eq = ctx.fresh_var();
                    let cur = blocks.last_mut();
                    cur.body.push(format!(
                        "{} = call i1 @aura_lang_std_String_equals(i8* {}, i8* {})",
                        eq, lv, rv
                    ));
                    if *op == HirBinOp::Eq {
                        return Ok((eq, "i1".to_string()));
                    }
                    cur.body.push(format!("{} = xor i1 {}, 1", tmp, eq));
                    return Ok((tmp, "i1".to_string()));
                }
                // 混合类型：一侧是字符串结构体，另一侧不是字符串类型。
                // 字符串与整数/浮点等永远不会相等，直接返回编译期常量。
                if matches!(op, HirBinOp::Eq | HirBinOp::Ne)
                    && (is_struct_str(&l_ty) || is_struct_str(&r_ty))
                    && !(is_str_like(&l_ty) && is_str_like(&r_ty))
                {
                    if *op == HirBinOp::Eq {
                        return Ok(("false".to_string(), "i1".to_string()));
                    }
                    return Ok(("true".to_string(), "i1".to_string()));
                }
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

/// 方法调用接收者的「就地取地址」。
///
/// 当接收者是**结构体类型的变量**或**结构体字段**（`this.ast` / `obj.field`），
/// 且形参是指针（方法 self 即 `i8*`）时，返回（地址 IR, 指针类型）。
///
/// 若不做此处理，接收者会按值加载成一份临时副本再传地址：方法内的
/// `this.count = …` 只写进副本，调用方读到的一直是旧值（表现为 AST 节点 id
/// 永远为 0、Lexer.pos 不前进等「状态不推进」类故障）。
fn receiver_address(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    recv: &HirExpr,
    want: &str,
) -> Result<Option<(String, String)>, AotError> {
    if !is_ptr_ty(want) {
        return Ok(None);
    }
    // 把「对象表达式」转为「该结构体类型的指针」
    fn base_of_struct(
        ctx: &mut EmitCtx,
        blocks: &mut FuncBlocks,
        object: &HirExpr,
        struct_ty: &str,
    ) -> Result<Option<String>, AotError> {
        if let HirExpr::Var(vn) = object {
            if let Some(slot) = ctx.lookup_var(vn).cloned() {
                if slot.llvm_ty == struct_ty {
                    let cast = ctx.fresh_var();
                    blocks.last_mut().body.push(format!(
                        "{} = bitcast {}* {} to {}*",
                        cast, slot.llvm_ty, slot.llvm_name, struct_ty
                    ));
                    return Ok(Some(cast));
                }
                if is_ptr_ty(&slot.llvm_ty) {
                    // 槽里存的是「指向对象的指针」：必须先 load 出值再 bitcast，
                    // 直接 bitcast 槽地址（X**）会得到非法的指针类型。
                    let loaded = ctx.fresh_var();
                    let cast = ctx.fresh_var();
                    blocks.last_mut().body.push(format!(
                        "{} = load {}, {}* {}",
                        loaded, slot.llvm_ty, slot.llvm_ty, slot.llvm_name
                    ));
                    blocks.last_mut().body.push(format!(
                        "{} = bitcast {} {} to {}*",
                        cast, slot.llvm_ty, loaded, struct_ty
                    ));
                    return Ok(Some(cast));
                }
            }
        }
        let (oir, oty) = emit_expr_val(ctx, blocks, object)?;
        if !is_ptr_ty(&oty) {
            return Ok(None);
        }
        let cast = ctx.fresh_var();
        blocks.last_mut().body.push(format!(
            "{} = bitcast {} {} to {}*",
            cast, oty, oir, struct_ty
        ));
        Ok(Some(cast))
    }

    match recv {
        HirExpr::Var(name) => {
            if let Some(slot) = ctx.lookup_var(name).cloned() {
                if slot.llvm_ty.starts_with("%struct.") {
                    let ptr_ty = format!("{}*", slot.llvm_ty);
                    let cast = ctx.fresh_var();
                    blocks.last_mut().body.push(format!(
                        "{} = bitcast {} {} to {}",
                        cast, ptr_ty, slot.llvm_name, want
                    ));
                    return Ok(Some((cast, want.to_string())));
                }
            }
            Ok(None)
        }
        HirExpr::Member {
            object,
            name: field,
        } => {
            // 注意：这里要传**内层对象**（`this` / 局部变量），否则 `this`/`self`
            // 分支不命中，会退化成「按字段名全局扫描」而选中同名字段的其它类
            // （如 `ast` 命中 HirLowerer 而不是 Parser），GEP 出错误偏移。
            let Some((class, field_ty, fidx)) =
                resolve_member_field_owner(ctx, object, None, field)
            else {
                return Ok(None);
            };
            let struct_ty = format!("%struct.{}", sanitizellvm(&class));
            let Some(base) = base_of_struct(ctx, blocks, object, &struct_ty)? else {
                return Ok(None);
            };
            let gep = ctx.fresh_var();
            let out = ctx.fresh_var();
            let cur = blocks.last_mut();
            cur.body.push(format!(
                "{} = getelementptr {}, {}* {}, i32 0, i32 {}",
                gep, struct_ty, struct_ty, base, fidx
            ));
            cur.body.push(format!(
                "{} = bitcast {}* {} to {}",
                out, field_ty, gep, want
            ));
            Ok(Some((out, want.to_string())))
        }
        _ => Ok(None),
    }
}

fn emit_call(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    callee: &str,
    args: &[HirExpr],
) -> Result<(String, String), AotError> {
    // P9: 检查是否为结构体构造函数。
    // declared_structs 存的是 `%struct.X` 格式，而 callee 是裸名 `X`，
    // 因此需要同时检查裸名和 `%struct.{callee}` 两种形式。
    {
        let struct_name = format!("%struct.{}", sanitizellvm(callee));
        if ctx.declared_structs.contains(callee) || ctx.declared_structs.contains(&struct_name) {
            return emit_struct_constructor(ctx, blocks, callee, args);
        }
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

    // 命名空间调用（如 `Collections.emptyList()`）在 HIR 中会把命名空间名当作
    // 首个实参传入（`args = [Var("Collections")]`），而对应 FFI 声明是 0 参。
    // 不剔除会生成 `call i8* @..._emptyList(i32 Collections)` 这类非法 IR
    //（llc: `expected value token`）。
    let mut effective_args: Vec<HirExpr> = args.to_vec();
    if let Some(cls_seg) = callee.split('.').rev().nth(1) {
        let is_phantom = match effective_args.first() {
            Some(HirExpr::Var(v)) => {
                v == cls_seg && !ctx.var_scope.iter().any(|scope| scope.contains_key(v.as_str()))
            }
            _ => false,
        };
        if is_phantom {
            effective_args.remove(0);
        }
    }
    let param_tys = ctx.func_param_types.get(callee).cloned();

    // 实参求值：若实参是「结构体局部变量」且对应形参是**指针**（方法接收者 self 即
    // `i8*`），直接传该局部变量的「槽地址」，而不是按值加载后拷贝。
    // 否则方法内的 `this.field = …` 会写进一份临时副本、调用方不可见，
    // 导致状态永不推进（Lexer.pos 不前进 → scanAll 死循环 + alloca 累积 → 栈溢出）。
    //
    // 收紧范围：**仅**对方法接收者（首个实参 + `Class.method` 形式的被调方）生效，
    // 避免影响其它「把结构体局部变量传给指针形参」的场景（如 Any/存进容器等，
    // 传栈槽地址会在作用域结束后悬空）。
    let is_method_call = callee.contains('.');
    let mut args_ir: Vec<(String, String)> = Vec::with_capacity(effective_args.len());
    for (i, a) in effective_args.iter().enumerate() {
        let want = param_tys.as_ref().and_then(|p| p.get(i)).cloned();
        if i == 0 && is_method_call {
            if let Some(w) = want.as_ref() {
                if let Some(recv) = receiver_address(ctx, blocks, a, w)? {
                    args_ir.push(recv);
                    continue;
                }
            }
        }
        args_ir.push(emit_expr_val(ctx, blocks, a)?);
    }

    // toStr(Boolean) / toString(Boolean)：AOT 下布尔与整数共用 Plan A 的 `i8*` 装箱
    // 通道（true 编码为 -1），直接装箱会打印成 "-1"/"0"，与 VM 的 "true"/"false" 不一致。
    // 在参数类型强制转换之前改派到 `aura_to_str_bool`。
    if matches!(callee, "toStr" | "toString") && args_ir.len() == 1 && args_ir[0].1 == "i1" {
        let (v, _) = &args_ir[0];
        let ext = ctx.fresh_var();
        let tmp = ctx.fresh_var();
        let cur = blocks.last_mut();
        cur.body.push(format!("{} = sext i1 {} to i64", ext, v));
        cur.body.push(format!("{} = call i8* @aura_to_str_bool(i64 {})", tmp, ext));
        return Ok((tmp, "i8*".to_string()));
    }

    let ret_ty = if callee == "Runtime" {
        // 内置异常构造器：返回 String 结构体的数据指针（i8*），直接交给 `__throw`。
        "i8*".to_string()
    } else {
        ctx.func_ret_types.get(callee).cloned().unwrap_or_else(|| "i32".to_string())
    };
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

    // 内置异常构造器 `Runtime(msg)`：Aura 侧 `throw Runtime("...")` 经 HIR 降级为
    // `__throw(Runtime("..."))`。`msg` 是 String 结构体 `{ i8*, i64 }`，此处取出其
    // 数据指针（i8*）传给 `@Runtime(i8*)`（见 runtime.rs 的 runtime_definitions），
    // 再由 `__throw(i8*)` 打印异常。直接在调用点解构，避免匿名结构体作为函数参数。
    if callee == "Runtime" && args_ir.len() == 1 {
        let (arg_ir, arg_ty) = &args_ir[0];
        let ptr = if arg_ty.starts_with('{') && arg_ty.contains("i8*") && arg_ty.contains("i64") {
            let p = ctx.fresh_var();
            let cur = blocks.last_mut();
            cur.body.push(format!("{} = extractvalue {} {}, 0", p, arg_ty, arg_ir));
            p
        } else {
            arg_ir.clone()
        };
        let tmp = ctx.fresh_var();
        let cur = blocks.last_mut();
        cur.body.push(format!("{} = call i8* @Runtime(i8* {})", tmp, ptr));
        return Ok((tmp, "i8*".to_string()));
    }

    // ── 集合/列表内建：映射到已有的 aura_lang_std_Collections_* 运行时 C 实现 ──
    // AOT 下 List/Array 表示为不透明指针 `i8*`（底层 AuraList，元素以 i64 句柄存储）。
    // 自举编译器大量使用 list.size / list.push / for x in list / arrayListOf / 1..10 等，
    // 这些内建在 VM 中以原生函数提供，但 AOT 后端此前未声明，导致链接到未定义符号。
    // 0 参列表构造（`listOf()` / `mutableListOf()` / `arrayListOf()` / `emptyList()`）：
    // 这类调用未被 HIR 降级为 `__list_new`，直接按名字发射会链接到不存在的
    // `@listOf`（C 运行时只提供 aura_lang_std_Collections_emptyList）。
    {
        let bare = callee.rsplit('.').next().unwrap_or(callee);
        if matches!(
            bare,
            "listOf" | "mutableListOf" | "arrayListOf" | "emptyList" | "emptyArray"
        ) && args_ir.is_empty()
        {
            let tmp = ctx.fresh_var();
            blocks.last_mut().body.push(format!(
                "{} = call i8* @aura_lang_std_Collections_emptyList()",
                tmp
            ));
            return Ok((tmp, "i8*".to_string()));
        }
    }

    if callee == "__list_len" && args_ir.len() == 1 {
        let (l, _) = &args_ir[0];
        let c = ctx.fresh_var();
        let cur = blocks.last_mut();
        cur.body.push(format!(
            "{} = call i64 @aura_lang_std_Collections_count(i8* {})",
            c, l
        ));
        let r = ctx.fresh_var();
        let cur = blocks.last_mut();
        cur.body.push(format!("{} = trunc i64 {} to i32", r, c));
        return Ok((r, "i32".to_string()));
    }
    if callee == "__size" && args_ir.len() == 1 {
        let (it, _) = &args_ir[0];
        let c = ctx.fresh_var();
        let cur = blocks.last_mut();
        cur.body.push(format!(
            "{} = call i64 @aura_lang_std_Collections_count(i8* {})",
            c, it
        ));
        let r = ctx.fresh_var();
        let cur = blocks.last_mut();
        cur.body.push(format!("{} = trunc i64 {} to i32", r, c));
        return Ok((r, "i32".to_string()));
    }
    if callee == "__get" && args_ir.len() == 2 {
        let (it, _) = &args_ir[0];
        let (idx, idx_ty) = &args_ir[1];
        let idx_val = if idx_ty == "i32" {
            let ext = ctx.fresh_var();
            let cur = blocks.last_mut();
            cur.body.push(format!("{} = zext i32 {} to i64", ext, idx));
            ext
        } else {
            idx.clone()
        };
        let tmp = ctx.fresh_var();
        let cur = blocks.last_mut();
        cur.body.push(format!(
            "{} = call i8* @aura_lang_std_Collections_getAt(i8* {}, i64 {})",
            tmp, it, idx_val
        ));
        return Ok((tmp, "i8*".to_string()));
    }
    if callee == "__list_push" && args_ir.len() == 2 {
        let (l, _) = &args_ir[0];
        let (e, e_ty) = &args_ir[1];
        let e_ptr = coerce_val_to_i8ptr(ctx, blocks, e, e_ty);
        let tmp = ctx.fresh_var();
        let cur = blocks.last_mut();
        cur.body.push(format!(
            "{} = call i8* @aura_lang_std_Collections_listAppend(i8* {}, i8* {})",
            tmp, l, e_ptr
        ));
        return Ok((tmp, "i8*".to_string()));
    }
    if callee == "__list_new" {
        // arrayListOf(a,b,c) → 空列表 + 逐个 append，避免可变参函数声明问题。
        // 每个元素按值规整为 i8* 再存入（与 collection 运行时约定一致）。
        let tmp0 = ctx.fresh_var();
        let cur = blocks.last_mut();
        cur.body.push(format!(
            "{} = call i8* @aura_lang_std_Collections_emptyList()",
            tmp0
        ));
        let mut cur_list = tmp0;
        for (e, e_ty) in &args_ir {
            let e_ptr = coerce_val_to_i8ptr(ctx, blocks, e, e_ty);
            let nxt = ctx.fresh_var();
            let cur = blocks.last_mut();
            cur.body.push(format!(
                "{} = call i8* @aura_lang_std_Collections_listAppend(i8* {}, i8* {})",
                nxt, cur_list, e_ptr
            ));
            cur_list = nxt;
        }
        return Ok((cur_list, "i8*".to_string()));
    }
    // list.pop()：方法名 `pop` 未被 HIR 降级（不像 push → __list_push），
    // 原样作为普通函数调用发出即产生 `@pop`。此处在 AOT 侧重写到 listPop，
    // 保持 HIR/字节码路径不变（字节码下 pop 由 VM 原生提供）。
    if callee == "pop" && args_ir.len() == 1 {
        let (l, _) = &args_ir[0];
        let tmp = ctx.fresh_var();
        let cur = blocks.last_mut();
        cur.body.push(format!(
            "{} = call i8* @aura_lang_std_Collections_listPop(i8* {})",
            tmp, l
        ));
        return Ok((tmp, "i8*".to_string()));
    }
    // Map 下标赋值 `m["k"] = v` 会被降级为 `Collections.set(collection, key, value)`，
    // 但 Collections.set 是**列表**语义（下标为 i64）；把字符串键传进去时实参是
    // `{ i8*, i64 }` 而形参是 `i64`，ABI 不匹配 → 运行期崩溃。
    // 当「下标」是字符串时改走 mapSet（Map 语义）。仅在 AOT 侧改派，不动 HIR/字节码。
    if (callee == "aura_lang_std_Collections_set" || callee == "aura.lang.std.Collections.set")
        && args_ir.len() == 3
    {
        let (coll, _) = &args_ir[0];
        let (idx, idx_ty) = &args_ir[1];
        if idx_ty.starts_with('{') || is_string_type(idx_ty) {
            let key_ptr = if idx_ty.starts_with('{') {
                let p = ctx.fresh_var();
                let cur = blocks.last_mut();
                cur.body.push(format!("{} = extractvalue {} {}, 0", p, idx_ty, idx));
                p
            } else {
                idx.clone()
            };
            let val_ptr = coerce_val_to_i8ptr(ctx, blocks, &args_ir[2].0, &args_ir[2].1);
            let cur = blocks.last_mut();
            cur.body.push(format!(
                "call void @aura_lang_std_Collections_mapSet(i8* {}, i8* {}, i8* {})",
                coll, key_ptr, val_ptr
            ));
            return Ok((coll.clone(), "i8*".to_string()));
        }
    }
    if callee == "__range" {
        // 参数：start(i32), end(i32), inclusive(i32)，由 HIR 在 desugar_expr 中传入。
        let s = args_ir.get(0).map(|(v, _)| v.clone()).unwrap_or_else(|| "0".to_string());
        let e = args_ir.get(1).map(|(v, _)| v.clone()).unwrap_or_else(|| "0".to_string());
        let inc = args_ir.get(2).map(|(v, _)| v.clone()).unwrap_or_else(|| "0".to_string());
        let tmp = ctx.fresh_var();
        let cur = blocks.last_mut();
        cur.body.push(format!(
            "{} = call i8* @aura_lang_std_Collections_range(i32 {}, i32 {}, i32 {})",
            tmp, s, e, inc
        ));
        return Ok((tmp, "i8*".to_string()));
    }

    let cur = blocks.last_mut();
    let args_str: Vec<String> = args_ir.iter().map(|(v, t)| format!("{} {}", t, v)).collect();

    // toString / toStr 对浮点实参：调用 toStringFloat(double)（C 的 toString 只收 int64）
    if (callee == "toString" || callee == "toStr") && args_ir.len() == 1 {
        let (v, t) = &args_ir[0];
        if t == "double" || t == "float" {
            let dv = if t == "float" {
                let ext = ctx.fresh_var();
                cur.body.push(format!("{} = fpext float {} to double", ext, v));
                ext
            } else {
                v.clone()
            };
            let tmp = ctx.fresh_var();
            cur.body.push(format!("{} = call i8* @toStringFloat(double {})", tmp, dv));
            return Ok((tmp, "i8*".to_string()));
        }
    }

    let callee_sym = sanitizellvm(callee);
    // 处理 void / 空返回类型：不能赋值给寄存器（LLVM IR 语法限制）
    if ret_ty.is_empty() || ret_ty == "void" {
        cur.body.push(format!(
            "call void @{}({})",
            callee_sym,
            args_str.join(", ")
        ));
        // 返回一个虚拟 i32 0 值，保持调用者接口兼容
        Ok(("0".to_string(), "i32".to_string()))
    } else {
        let tmp = ctx.fresh_var();
        cur.body.push(format!(
            "{} = call {} @{}({})",
            tmp,
            ret_ty,
            callee_sym,
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
    // 指针 → 结构体值：从指针 load 出结构体（如 `return this`，this 为对象指针，
    // 而函数签名按值返回 `%struct.X`；不转换会生成非法 IR：
    // `value doesn't match function result type '%struct.X'`）
    if is_ptr_ty(from) && to.starts_with("%struct.") {
        let cast = ctx.fresh_var();
        let loaded = ctx.fresh_var();
        let body = &mut blocks.last_mut().body;
        body.push(format!("{} = bitcast {} {} to {}*", cast, from, val, to));
        body.push(format!("{} = load {}, {}* {}", loaded, to, to, cast));
        return (loaded, to.to_string());
    }
    // i8* → 字符串结构体：先经 aura_to_str_any 解析（Plan A：可能是装箱整数），
    // 再用 strlen 补长度。直接用原值 strlen 会对非指针位解引用而崩溃。
    if from == "i8*" && to.starts_with('{') && to.contains("i8*") && to.contains("i64") {
        let sptr = ctx.fresh_var();
        let len = ctx.fresh_var();
        let with_len = ctx.fresh_var();
        let with_ptr = ctx.fresh_var();
        let body = &mut blocks.last_mut().body;
        body.push(format!("{} = call i8* @aura_to_str_any(i8* {})", sptr, val));
        body.push(format!(
            "{} = call i64 @aura_string_length(i8* {})",
            len, sptr
        ));
        body.push(format!(
            "{} = insertvalue {} undef, i8* {}, 0",
            with_ptr, to, sptr
        ));
        body.push(format!(
            "{} = insertvalue {} {}, i64 {}, 1",
            with_len, to, with_ptr, len
        ));
        return (with_len, to.to_string());
    }
    // 指针 ↔ 整数
    // Plan A（低位标记装箱）：整型 → i8* 时编码为 (v<<1)|1，读回时经 aura_to_int_any
    // 还原。真实指针恒为偶数，因此该 helper 对既有的「真实指针」路径零回归，
    // 只有我们主动标记的装箱整数（列表元素等走偶数✓）会被正确拆箱。
    if is_ptr_ty(to) && is_int_ty(from) {
        let t = box_int_to_i8ptr(ctx, &mut blocks.last_mut().body, &val, from);
        return (t, to.to_string());
    }
    if is_ptr_ty(from) && is_int_ty(to) {
        let body = &mut blocks.last_mut().body;
        let raw = ctx.fresh_var();
        body.push(format!("{} = ptrtoint {} {} to i64", raw, from, val));
        let d = ctx.fresh_var();
        body.push(format!("{} = call i64 @aura_to_int_any(i64 {})", d, raw));
        if to == "i64" {
            return (d, to.to_string());
        }
        let t = ctx.fresh_var();
        body.push(format!("{} = trunc i64 {} to {}", t, d, to));
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
    // 指针 → 整数：AOT 中 `Any`（`i8*`）与标量类型互转（装箱/拆箱）。
    // 缺少这一条时会把 `i8*` 直接塞进 `i32` 字段 → insertvalue 报
    // `operand and field disagree in type: 'ptr' instead of 'i32'`。
    if is_ptr_ty(from) && is_int_ty(to) {
        let t = ctx.fresh_var();
        emit(
            &mut blocks.last_mut().body,
            format!("{} = ptrtoint {} {} to {}", t, from, val, to),
        );
        return (t, to.to_string());
    }
    // 整数 → 指针
    if is_int_ty(from) && is_ptr_ty(to) {
        let t = ctx.fresh_var();
        emit(
            &mut blocks.last_mut().body,
            format!("{} = inttoptr {} {} to {}", t, from, val, to),
        );
        return (t, to.to_string());
    }
    (val, from.to_string())
}

/// 将任意 AOT 值规整为 `i8*`，用于按值存入 AuraDynList（AOT 下列表元素统一为 i8* 句柄）。
/// 与 `emit_coerce_to` 的约定保持一致：
/// - 字符串结构体 `{ ptr, i64 }` → 取出数据指针（字段 0），读取时再按 strlen 重建；
/// - 整数 → sext 后 inttoptr 存入；指针 → 原样。
/// 这样 `arrayListOf`/`list.push` 中的元素（含 String）才能与 `aura_lang_std_Collections_*`
/// 的 `i8*` 形参匹配，并能在 `getAt` 读回时正确还原。
fn coerce_val_to_i8ptr(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    val: &str,
    from: &str,
) -> String {
    if from == "i8*" || from == "ptr" {
        return val.to_string();
    }
    if from.starts_with('{') {
        let t = ctx.fresh_var();
        blocks.last_mut().body.push(format!("{} = extractvalue {} {}, 0", t, from, val));
        return t;
    }
    // 整数入列表：与通用装箱一致，采用 Plan A 低位标记 (v<<1)|1，
    // 读回时 aura_to_str_any / aura_to_int_any 才能正确还原。
    if from == "i64" {
        let sh = ctx.fresh_var();
        blocks.last_mut().body.push(format!("{} = shl i64 {}, 1", sh, val));
        let tg = ctx.fresh_var();
        blocks.last_mut().body.push(format!("{} = or i64 {}, 1", tg, sh));
        let t = ctx.fresh_var();
        blocks.last_mut().body.push(format!("{} = inttoptr i64 {} to i8*", t, tg));
        return t;
    }
    if from == "i32" {
        let ext = ctx.fresh_var();
        blocks.last_mut().body.push(format!("{} = sext i32 {} to i64", ext, val));
        let sh = ctx.fresh_var();
        blocks.last_mut().body.push(format!("{} = shl i64 {}, 1", sh, ext));
        let tg = ctx.fresh_var();
        blocks.last_mut().body.push(format!("{} = or i64 {}, 1", tg, sh));
        let t = ctx.fresh_var();
        blocks.last_mut().body.push(format!("{} = inttoptr i64 {} to i8*", t, tg));
        return t;
    }
    // 其它（如 %struct.*）→ 直接作为 i8*（尽力而为）
    val.to_string()
}

/// Plan A 装箱：把整型值编码为低位标记指针 `(v<<1)|1` → `i8*`。
///
/// 读回时由 `aura_to_int_any` / `aura_to_str_any` 解码。真实指针恒为偶数
/// （分配器对齐 ≥2），故二者不会混淆，且对既有「真实指针」路径零回归。
/// 注意：值本身已是 `i64` 时**不能**再发 `sext i64 → i64`（非法 cast）。
fn box_int_to_i8ptr(ctx: &mut EmitCtx, body: &mut Vec<String>, val: &str, from: &str) -> String {
    let ext = if from == "i64" {
        val.to_string()
    } else {
        let e = ctx.fresh_var();
        body.push(format!("{} = sext {} {} to i64", e, from, val));
        e
    };
    let sh = ctx.fresh_var();
    body.push(format!("{} = shl i64 {}, 1", sh, ext));
    let tg = ctx.fresh_var();
    body.push(format!("{} = or i64 {}, 1", tg, sh));
    let t = ctx.fresh_var();
    body.push(format!("{} = inttoptr i64 {} to i8*", t, tg));
    t
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

/// 推断 if 表达式两分支的公共 LLVM 类型，使 PHI 节点类型合法。
///
/// Aura 是动态类型语言，sema 常将分支类型退化为 `Any`/`i32`，导致两分支类型不一致
/// （如 `i32` 与 `{ i8*, i64 }` 字符串）。这里按「能安全表示两端」的原则选公共类型：
/// - 相同类型 → 原类型
/// - 同为整型 → 较宽者；同为浮点 → double
/// - 一端/两端为字符串（结构体 `{i8*,i64}` 或裸指针 `i8*`）→ 字符串表示
/// - 其余（数值 vs 字符串结构体等）→ `i8*`（Any）兜底
fn common_if_type(t1: &str, t2: &str) -> String {
    if t1 == t2 {
        return t1.to_string();
    }
    if is_int_ty(t1) && is_int_ty(t2) {
        return if int_bits(t1) >= int_bits(t2) { t1.to_string() } else { t2.to_string() };
    }
    if is_float_ty(t1) && is_float_ty(t2) {
        return if t1 == "double" { "double".to_string() } else { t2.to_string() };
    }
    let s1 = t1.starts_with('{') && t1.contains("i8*");
    let s2 = t2.starts_with('{') && t2.contains("i8*");
    if (s1 || t1 == "i8*") && (s2 || t2 == "i8*") {
        if s1 {
            return t1.to_string();
        }
        if s2 {
            return t2.to_string();
        }
        return "i8*".to_string();
    }
    "i8*".to_string()
}

/// 将值 `val`（类型 `from`）转换为目标类型 `to`，在 `block` 终止符前插入转换指令，
/// 返回转换后的值名。无法安全转换时尽力返回原值（交由后续 llc 暴露问题）。
fn emit_coerce_to(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    block: &str,
    val: &str,
    from: &str,
    to: &str,
) -> String {
    if from == to {
        return val.to_string();
    }
    // 目标为字符串结构体 { i8*, i64 }
    if to.starts_with('{') && to.contains("i8*") && to.contains("i64") {
        if from == "i8*" {
            // Plan A：先用 aura_to_str_any 解析（可能是低位标记的整数），再 strlen
            let sptr = ctx.fresh_var();
            let p = ctx.fresh_var();
            let len = ctx.fresh_var();
            let s = ctx.fresh_var();
            insert_before_terminator(
                blocks,
                block,
                format!("{} = call i8* @aura_to_str_any(i8* {})", sptr, val),
            );
            insert_before_terminator(
                blocks,
                block,
                format!("{} = call i64 @aura_string_length(i8* {})", len, sptr),
            );
            insert_before_terminator(
                blocks,
                block,
                format!("{} = insertvalue {} undef, i8* {}, 0", p, to, sptr),
            );
            insert_before_terminator(
                blocks,
                block,
                format!("{} = insertvalue {} {}, i64 {}, 1", s, to, p, len),
            );
            return s;
        }
        // 数值/其它 → 空字符串兜底（首字段 null，长度 0）
        let p = ctx.fresh_var();
        let s = ctx.fresh_var();
        insert_before_terminator(
            blocks,
            block,
            format!("{} = insertvalue {} undef, i8* null, 0", p, to),
        );
        insert_before_terminator(
            blocks,
            block,
            format!("{} = insertvalue {} {}, i64 0, 1", s, to, p),
        );
        return s;
    }
    // 目标为 i8*（Any）：整型 inttoptr，结构体取首字段，指针 bitcast
    if to == "i8*" {
        if is_int_ty(from) {
            // Plan A：装箱整数 → 低位标记 (v<<1)|1（已是 i64 时不做 sext）
            let ext = if from == "i64" {
                val.to_string()
            } else {
                let e = ctx.fresh_var();
                insert_before_terminator(
                    blocks,
                    block,
                    format!("{} = sext {} {} to i64", e, from, val),
                );
                e
            };
            let sh = ctx.fresh_var();
            insert_before_terminator(blocks, block, format!("{} = shl i64 {}, 1", sh, ext));
            let tg = ctx.fresh_var();
            insert_before_terminator(blocks, block, format!("{} = or i64 {}, 1", tg, sh));
            let v = ctx.fresh_var();
            insert_before_terminator(blocks, block, format!("{} = inttoptr i64 {} to i8*", v, tg));
            return v;
        }
        if from.starts_with('{') {
            let v = ctx.fresh_var();
            insert_before_terminator(
                blocks,
                block,
                format!("{} = extractvalue {} {}, 0", v, from, val),
            );
            return v;
        }
        if from.ends_with('*') {
            let v = ctx.fresh_var();
            insert_before_terminator(
                blocks,
                block,
                format!("{} = bitcast {} {} to i8*", v, from, val),
            );
            return v;
        }
        return val.to_string();
    }
    // i8* → 整型：Plan A 低位标记拆箱（aura_to_int_any），再截断到目标宽度
    if from == "i8*" && is_int_ty(to) {
        let raw = ctx.fresh_var();
        insert_before_terminator(
            blocks,
            block,
            format!("{} = ptrtoint i8* {} to i64", raw, val),
        );
        let d = ctx.fresh_var();
        insert_before_terminator(
            blocks,
            block,
            format!("{} = call i64 @aura_to_int_any(i64 {})", d, raw),
        );
        if to == "i64" {
            return d;
        }
        let v = ctx.fresh_var();
        insert_before_terminator(blocks, block, format!("{} = trunc i64 {} to {}", v, d, to));
        return v;
    }
    // 整型互转兜底
    if is_int_ty(from) && is_int_ty(to) {
        if int_bits(from) == int_bits(to) {
            return val.to_string();
        }
        let v = ctx.fresh_var();
        let op = if int_bits(from) < int_bits(to) { "zext" } else { "trunc" };
        insert_before_terminator(
            blocks,
            block,
            format!("{} = {} {} {} to {}", v, op, from, val, to),
        );
        return v;
    }
    val.to_string()
}

/// 若 `type_name` 存在匹配参数个数的合成构造函数 `Class.__ctorN`，则分配实例并调用之，
/// 返回 `Some((value, type))`；否则 `None`。统一处理「存在默认值字段 / 构造函数按名赋值」的类
/// （如 Dwarf.DebugInfo 有 subprograms 默认值字段，3 个构造参数对应 4 个字段）。位置式
/// insertvalue 会按「实参 i → 字段 i」机械映射，在这种类上错位，触发 llc 的
/// 「insertvalue operand and field disagree」。
/// 把类/结构体的字段默认值写入已分配对象 `alloc`（类型 `struct_ty`）。
///
/// 对应 MIR 路径的「Alloc + 字段默认值 + Call __ctorN」中的中间一步。
/// AOT 若省略这一步，未被构造参数 / init 块覆盖的字段会保持 `undef`。
fn emit_field_defaults(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    class: &str,
    struct_ty: &str,
    alloc: &str,
) -> Result<(), AotError> {
    let Some(defaults) = ctx.class_defaults.get(class).cloned() else {
        return Ok(());
    };
    for (idx, fty, expr) in defaults.iter() {
        let (v, vty) = emit_expr_val(ctx, blocks, expr)?;
        let (v, _) = coerce_arg(ctx, blocks, v, &vty, fty);
        let gep = ctx.fresh_var();
        let cur = blocks.last_mut();
        cur.body.push(format!(
            "{} = getelementptr {}, {}* {}, i32 0, i32 {}",
            gep, struct_ty, struct_ty, alloc, idx
        ));
        cur.body.push(format!("store {} {}, {}* {}", fty, v, fty, gep));
    }
    Ok(())
}

fn try_emit_ctor(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    type_name: &str,
    args: &[HirExpr],
    struct_type: &str,
) -> Result<Option<(String, String)>, AotError> {
    let ctor_suffix = format!(".__ctor{}", args.len());
    let ctor_name = ctx
        .func_param_types
        .keys()
        .find(|k| {
            let s: &str = k;
            s.ends_with(ctor_suffix.as_str()) && {
                let base = &s[..s.len() - ctor_suffix.len()];
                base == type_name || base.ends_with(&format!(".{}", type_name))
            }
        })
        .cloned();
    let param_tys = ctor_name.as_ref().and_then(|n| ctx.func_param_types.get(n).cloned());
    if let (Some(ctor_name), Some(param_tys)) = (ctor_name, param_tys) {
        if param_tys.len() == args.len() + 1 {
            let alloc = ctx.fresh_var();
            blocks.last_mut().body.push(format!("{} = alloca {}", alloc, struct_type));
            // 先写字段默认值（init 块/构造参数之外的字段仍须有确定初值）
            emit_field_defaults(ctx, blocks, type_name, struct_type, &alloc)?;
            // self 形参类型在 desugar 中是 Any → i8*；按实际类型传参（必要时 bitcast）。
            let self_ty = &param_tys[0];
            let self_arg = if self_ty == &format!("{}*", struct_type) {
                format!("{}* {}", struct_type, alloc)
            } else {
                let bc = ctx.fresh_var();
                blocks.last_mut().body.push(format!(
                    "{} = bitcast {}* {} to {}",
                    bc, struct_type, alloc, self_ty
                ));
                format!("{} {}", self_ty, bc)
            };
            let mut call_args: Vec<String> = vec![self_arg];
            for (i, a) in args.iter().enumerate() {
                let (val, ty) = emit_expr_val(ctx, blocks, a)?;
                let want = &param_tys[i + 1];
                let (v, _vty) = coerce_arg(ctx, blocks, val, &ty, want);
                call_args.push(format!("{} {}", want, v));
            }
            let cur = blocks.last_mut();
            cur.body.push(format!(
                "call void @{}({})",
                sanitizellvm(&ctor_name),
                call_args.join(", ")
            ));
            let loaded = ctx.fresh_var();
            let cur = blocks.last_mut();
            cur.body.push(format!(
                "{} = load {}, {}* {}",
                loaded, struct_type, struct_type, alloc
            ));
            return Ok(Some((loaded, struct_type.to_string())));
        }
    }
    Ok(None)
}

/// P9: 生成结构体构造函数代码
fn emit_struct_constructor(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    struct_name: &str,
    args: &[HirExpr],
) -> Result<(String, String), AotError> {
    let struct_type = ctx.map_type(&HirType::Named(struct_name.to_string()));

    // 优先走合成构造函数 Class.__ctorN（按名赋值各字段，与 struct 定义布局一致），
    // 可正确处理「存在默认值字段 / 构造函数按名赋值」的类。
    if let Some((v, t)) = try_emit_ctor(ctx, blocks, struct_name, args, &struct_type)? {
        return Ok((v, t));
    }

    // 兜底：位置式 insertvalue（适用于无合成构造函数的纯值结构体 / 枚举变体）
    let mut field_tys: Vec<(usize, String)> = ctx
        .class_field_types
        .get(struct_name)
        .map(|m| m.values().map(|(ty, idx)| (*idx, ty.clone())).collect())
        .unwrap_or_default();
    field_tys.sort_by_key(|(idx, _)| *idx);

    let args_ir: Vec<(String, String)> =
        args.iter().map(|a| emit_expr_val(ctx, blocks, a)).collect::<Result<_, _>>()?;
    // 无合成构造函数（如无 init 块的类）：也必须先写字段默认值，否则未赋值字段
    // 保持 undef → 运行期读到随机内存（非确定性）。
    let mut struct_val = if ctx.class_defaults.contains_key(struct_name) {
        let alloc = ctx.fresh_var();
        blocks.last_mut().body.push(format!("{} = alloca {}", alloc, struct_type));
        emit_field_defaults(ctx, blocks, struct_name, &struct_type, &alloc)?;
        let loaded = ctx.fresh_var();
        blocks.last_mut().body.push(format!(
            "{} = load {}, {}* {}",
            loaded, struct_type, struct_type, alloc
        ));
        loaded
    } else {
        "undef".to_string()
    };
    for (i, (val, ty)) in args_ir.iter().enumerate() {
        // 按字段声明类型转换实参（如 `Any`/`i8*` → `i32`）
        let (v, vty) = match field_tys.iter().find(|(idx, _)| *idx == i) {
            Some((_, want)) => coerce_arg(ctx, blocks, val.clone(), ty, want),
            None => (val.clone(), ty.clone()),
        };
        let new_val = ctx.fresh_var();
        let cur = blocks.last_mut();
        cur.body.push(format!(
            "{} = insertvalue {} {}, {} {}, {}",
            new_val, struct_type, struct_val, vty, v, i
        ));
        struct_val = new_val;
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
    eprintln!(
        "[DBG emit_member_access] name={}, obj_ir={}, obj_ty={}",
        name, obj_ir, obj_ty
    );

    // 内建属性：字符串 `{ i8*, i64 }` 的 length / size → 第 1 个字段（len）。
    // 若不特判，会落入下方「按字段名全局查找」的兜底分支取到字段 0（数据指针）。
    if obj_ty.starts_with('{') && obj_ty.contains("i8*") && obj_ty.contains("i64") {
        if name == "length" || name == "size" {
            let cur = blocks.last_mut();
            cur.body.push(format!("{} = extractvalue {} {}, 1", tmp, obj_ty, obj_ir));
            return Ok((tmp, "i64".to_string()));
        }
        if name == "isEmpty" {
            let len = ctx.fresh_var();
            let cur = blocks.last_mut();
            cur.body.push(format!("{} = extractvalue {} {}, 1", len, obj_ty, obj_ir));
            cur.body.push(format!("{} = icmp eq i64 {}, 0", tmp, len));
            return Ok((tmp, "i1".to_string()));
        }
    }

    // 内建属性：动态列表（不透明指针）的 size / length / isEmpty → runtime 计数
    if obj_ty == "i8*" || obj_ty == "ptr" {
        if name == "size" || name == "length" || name == "isEmpty" {
            let cnt = ctx.fresh_var();
            let cur = blocks.last_mut();
            cur.body.push(format!(
                "{} = call i64 @aura_lang_std_Collections_count(i8* {})",
                cnt, obj_ir
            ));
            if name == "isEmpty" {
                cur.body.push(format!("{} = icmp eq i64 {}, 0", tmp, cnt));
                return Ok((tmp, "i1".to_string()));
            }
            return Ok((cnt, "i64".to_string()));
        }
    }

    let cur = blocks.last_mut();

    // 查找字段所在类与字段（类型 + 索引）：优先按对象的**静态类型**解析
    // （`this`/`self` → 当前类；局部变量 → 其声明类型的类），最后才退回
    // 「按字段名全局扫描」的启发式（多类含同名字段时会选错）。
    // 注意：`extractvalue` 的结构体类型必须与字段索引来自**同一个类**，
    // 否则会生成 `extractvalue %struct.Token …, 1` 这类错类型 IR。
    let owner = resolve_member_field_owner(ctx, object, Some(&obj_ty), name);
    eprintln!("[DBG emit_member_access] owner={:?}", owner);
    let (field_llvm_ty, field_idx) = owner
        .as_ref()
        .map(|(_, ty, idx)| (ty.clone(), *idx))
        .unwrap_or_else(|| ("i32".to_string(), 0));
    // 未定义的结构体按不透明指针处理（避免 unsized 类型）
    let owner_struct = owner.as_ref().map(|(cls, _, _)| {
        if ctx.known_structs.contains(cls) {
            format!("%struct.{}", sanitizellvm(cls))
        } else {
            "i8*".to_string()
        }
    });
    eprintln!("[DBG emit_member_access] owner_struct={:?}", owner_struct);

    // 判断对象是指针还是值
    let is_pointer = obj_ty.ends_with('*') || obj_ty == "i8*" || obj_ty == "ptr";

    if is_pointer {
        // 对象是指针：先 load 结构体值，再 extractvalue
        let gep = ctx.fresh_var();
        let loaded = ctx.fresh_var();
        let struct_type = owner_struct.clone().unwrap_or_else(|| "i8*".to_string());

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
        // 对象是结构体值：直接使用 extractvalue（结构体类型与字段索引同源）
        let struct_type = owner_struct.clone().unwrap_or_else(|| obj_ty.clone());

        cur.body.push(format!(
            "{} = extractvalue {} {}, {}",
            tmp, struct_type, obj_ir, field_idx
        ));
    }
    Ok((tmp, field_llvm_ty))
}

/// 解析成员访问的 `(LLVM 字段类型, 字段索引)`。
///
/// 解析优先级：
/// 1. `this` / `self` → 当前类（`ctx.current_class`）的字段表
/// 2. 其它变量 → 由其声明类型反推的类（`%struct.X` / `%struct.X*`）
/// 3. 兜底：按字段名全局扫描（多类存在同名字段时可能选错，故优先级最低）
/// 解析成员所属的类与字段：`(类名, LLVM 类型, 字段索引)`。
///
/// 解析优先级：
/// 1. **对象的静态 LLVM 类型**（`%struct.X` / `%struct.X*`）——最可靠，如调用/属性返回值
/// 2. `this` / `self` → 当前类
/// 3. 变量 → 其声明类型反推的类
/// 4. 兜底：按字段名全局扫描（`HashMap` 迭代顺序不确定，可能选错类，故优先级最低）
///
/// 返回类名是必需的：`extractvalue` / 成员赋值都要用它拼 `%struct.X`，
/// 必须与字段索引来自同一个类，否则会生成
/// `'%var.N' defined with type '%struct.Token …' but expected '%struct.Span …'` 这类非法 IR。
fn resolve_member_field_owner(
    ctx: &EmitCtx,
    object: &HirExpr,
    obj_ty: Option<&str>,
    name: &str,
) -> Option<(String, String, usize)> {
    let mut candidates: Vec<String> = Vec::new();
    let mut obj_type_resolved = false;
    eprintln!(
        "[DBG resolve_member_field_owner] name={}, obj_ty={:?}, object={:?}",
        name, obj_ty, object
    );
    if let Some(t) = obj_ty {
        if let Some(cls) = struct_name_of_ty(t) {
            candidates.push(cls);
            obj_type_resolved = true;
        }
    }
    if let HirExpr::Var(var) = object {
        if var == "this" || var == "self" {
            eprintln!(
                "[DBG resolve_member_field_owner] var={}, current_class={:?}",
                var, ctx.current_class
            );
            if let Some(cls) = &ctx.current_class {
                candidates.push(cls.clone());
                obj_type_resolved = true;
            }
        } else if let Some(cls) = ctx
            .var_scope
            .iter()
            .rev()
            .find_map(|scope| scope.get(var))
            .and_then(|slot| struct_name_of_ty(&slot.llvm_ty))
        {
            candidates.push(cls);
            obj_type_resolved = true;
        }
    }
    for cls in &candidates {
        if let Some((ty, idx)) = ctx.class_field_types.get(cls).and_then(|m| m.get(name)) {
            return Some((cls.clone(), ty.clone(), *idx));
        }
    }
    // 如果对象的静态类型已解析为已知结构体，但字段未找到，
    // 退回全局扫描会生成错误的 extractvalue（用 Token 类型提取 Span 值）。
    // 此时宁可返回 None，让调用方按 i32/0 兜底，也不要用错结构体类型。
    if obj_type_resolved {
        return None;
    }
    ctx.class_field_types.iter().find_map(|(class, fields)| {
        fields.get(name).map(|(ty, idx)| (class.clone(), ty.clone(), *idx))
    })
}

/// 从 LLVM 类型串反推结构体类名：`%struct.Lexer*` / `%struct.Lexer` → `Lexer`
fn struct_name_of_ty(ty: &str) -> Option<String> {
    let t = ty.trim_end_matches('*');
    t.strip_prefix("%struct.").map(|s| s.to_string())
}

fn emit_index_access(
    ctx: &mut EmitCtx,
    blocks: &mut FuncBlocks,
    container: &HirExpr,
    index: &HirExpr,
) -> Result<(String, String), AotError> {
    let (c_ir, c_ty) = emit_expr_val(ctx, blocks, container)?;
    let (i_ir, i_ty) = emit_expr_val(ctx, blocks, index)?;
    let gep = ctx.fresh_var();
    let tmp = ctx.fresh_var();

    // 索引统一提升到 i64（GEP / runtime 取值索引槽位都是 i64）
    let idx_ir = if is_int_ty(&i_ty) && i_ty != "i64" {
        let cast = ctx.fresh_var();
        let cur = blocks.last_mut();
        cur.body.push(format!("{} = sext {} {} to i64", cast, i_ty, i_ir));
        cast
    } else {
        i_ir.clone()
    };

    // 情形 M：字符串键 → Map 取值（`frame["ip"]`，`Map<String, Any>`）
    if is_string_type(&i_ty) {
        let base = if is_ptr_ty(&c_ty) {
            c_ir.clone()
        } else {
            let p = ctx.fresh_var();
            let cur = blocks.last_mut();
            cur.body.push(format!("{} = inttoptr {} {} to i8*", p, c_ty, c_ir));
            p
        };
        let (key, _) = coerce_arg(ctx, blocks, i_ir, &i_ty, "i8*");
        let cur = blocks.last_mut();
        cur.body.push(format!(
            "{} = call i8* @aura_lang_std_Collections_mapGet(i8* {}, i8* {})",
            tmp, base, key
        ));
        return Ok((tmp, "i8*".to_string()));
    }

    // 情形 A：字符串 `{ i8*, i64 }` → charAt(data, idx)，返回「单字符字符串」结构体
    if c_ty.starts_with('{') && c_ty.contains("i8*") && c_ty.contains("i64") {
        let data = ctx.fresh_var();
        let ch = ctx.fresh_var();
        let r0 = ctx.fresh_var();
        let r1 = ctx.fresh_var();
        let cur = blocks.last_mut();
        cur.body.push(format!("{} = extractvalue {} {}, 0", data, c_ty, c_ir));
        cur.body.push(format!(
            "{} = call i8* @aura_lang_std_String_charAt(i8* {}, i64 {})",
            ch, data, idx_ir
        ));
        cur.body.push(format!(
            "{} = insertvalue {{ i8*, i64 }} undef, i8* {}, 0",
            r0, ch
        ));
        cur.body.push(format!(
            "{} = insertvalue {{ i8*, i64 }} {}, i64 1, 1",
            r1, r0
        ));
        return Ok((r1, "{ i8*, i64 }".to_string()));
    }

    // 情形 B：动态列表（不透明指针）→ runtime 取值，元素为不透明指针
    if c_ty == "i8*" || c_ty == "ptr" {
        let cur = blocks.last_mut();
        cur.body.push(format!(
            "{} = call i8* @aura_lang_std_Collections_getAt(i8* {}, i64 {})",
            tmp, c_ir, idx_ir
        ));
        return Ok((tmp, "i8*".to_string()));
    }

    // 情形 B2：容器静态类型既不是指针也不是字符串/结构体（典型是类型信息缺失
    // 退化成的 `i32`）→ 按「动态列表」处理：先 inttoptr 再走 runtime 取值。
    // 否则会对一个整数值做 GEP（`getelementptr i32, i32* %int_value`）产出非法 IR
    //（`'%var.N' defined with type 'i32' but expected 'ptr'`）。
    if !is_ptr_ty(&c_ty) && !c_ty.starts_with('{') {
        let ptr = ctx.fresh_var();
        let cur = blocks.last_mut();
        cur.body.push(format!("{} = inttoptr {} {} to i8*", ptr, c_ty, c_ir));
        cur.body.push(format!(
            "{} = call i8* @aura_lang_std_Collections_getAt(i8* {}, i64 {})",
            tmp, ptr, idx_ir
        ));
        return Ok((tmp, "i8*".to_string()));
    }

    // 情形 C：整型数组（`i32*` 等）→ GEP + load。
    // 基址若不是 `i32*`（如 `i8*`）先 bitcast，保证 GEP 合法。
    let base_ir = if c_ty == "i32*" || !is_ptr_ty(&c_ty) {
        c_ir.clone()
    } else {
        let cast = ctx.fresh_var();
        let cur = blocks.last_mut();
        cur.body.push(format!("{} = bitcast {} {} to i32*", cast, c_ty, c_ir));
        cast
    };

    let cur = blocks.last_mut();
    cur.body.push(format!(
        "{} = getelementptr i32, i32* {}, i64 {}",
        gep, base_ir, idx_ir
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

    // 优先走合成构造函数 Class.__ctorN（与 emit_struct_constructor 一致：按名赋值字段，
    // 正确处理默认值字段 / 构造函数按名赋值）。
    if let Some((v, t)) = try_emit_ctor(ctx, blocks, type_name, args, &llvm_struct_type)? {
        return Ok((v, t));
    }

    // 字段索引 → 声明的 LLVM 类型（实参类型不符时先转换，与 emit_struct_constructor 一致）
    let mut field_tys: Vec<(usize, String)> = ctx
        .class_field_types
        .get(type_name)
        .map(|m| m.values().map(|(ty, idx)| (*idx, ty.clone())).collect())
        .unwrap_or_default();
    field_tys.sort_by_key(|(idx, _)| *idx);

    // 使用 insertvalue 构建结构体值。
    // 无合成构造函数时同样要先写字段默认值（见 class_defaults 注释）。
    let mut struct_val = if ctx.class_defaults.contains_key(type_name) {
        let alloc = ctx.fresh_var();
        blocks.last_mut().body.push(format!("{} = alloca {}", alloc, llvm_struct_type));
        emit_field_defaults(ctx, blocks, type_name, &llvm_struct_type, &alloc)?;
        let loaded = ctx.fresh_var();
        blocks.last_mut().body.push(format!(
            "{} = load {}, {}* {}",
            loaded, llvm_struct_type, llvm_struct_type, alloc
        ));
        loaded
    } else {
        "undef".to_string()
    };
    for (i, (val, ty)) in arg_values.iter().enumerate() {
        let (v, vty) = match field_tys.iter().find(|(idx, _)| *idx == i) {
            Some((_, want)) => coerce_arg(ctx, blocks, val.clone(), ty, want),
            None => (val.clone(), ty.clone()),
        };
        let new_val = ctx.fresh_var();
        let cur = blocks.last_mut();
        cur.body.push(format!(
            "{} = insertvalue {} {}, {} {}, {}",
            new_val, llvm_struct_type, struct_val, vty, v, i
        ));
        struct_val = new_val;
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
    // 分支若以 break/continue/return 结束，当前块已带终止符：绝不能改写它
    // （改写会吞掉跳转，例如 `else { break }` 退化为死循环）。此时补一个
    // 不可达块作为 merge 的前驱，保证 merge 的边数与 PHI 条目一致。
    let then_terminated = blocks.has_terminator();
    let then_actual_block = if then_terminated {
        let name = ctx.fresh_bb("unreach");
        let _ = blocks.add_block_named(&name);
        name
    } else {
        blocks.blocks.last().map(|bb| bb.name.clone()).unwrap_or(then_name.clone())
    };
    blocks.set_terminator(&format!("br label %{}", merge_name));

    // Else 块
    let _ = blocks.add_block_named(&else_name);
    let (else_ir, else_ty) = emit_expr_val(ctx, blocks, else_e)?;
    let else_terminated = blocks.has_terminator();
    let else_actual_block = if else_terminated {
        let name = ctx.fresh_bb("unreach");
        let _ = blocks.add_block_named(&name);
        name
    } else {
        blocks.blocks.last().map(|bb| bb.name.clone()).unwrap_or(else_name.clone())
    };
    blocks.set_terminator(&format!("br label %{}", merge_name));

    // 类型协调：
    // (a) 任一侧已终止（break/return/continue）→ 该侧永不落到 merge，PHI 操作数用
    //     `undef`；另一侧的值 coerce 到公共类型（只能在该侧块内插入指令，且该侧块
    //     未终止，插入合法）。
    // (b) 否则按可空结构体 / 字符串 / 数值等情形协调。
    let (phi_type, then_phi_val, else_phi_val) = if then_terminated || else_terminated {
        let cty = common_if_type(&then_ty, &else_ty);
        let tv = if then_terminated {
            "undef".to_string()
        } else {
            emit_coerce_to(
                ctx,
                blocks,
                &then_actual_block,
                then_ir.as_str(),
                &then_ty,
                &cty,
            )
        };
        let ev = if else_terminated {
            "undef".to_string()
        } else {
            emit_coerce_to(
                ctx,
                blocks,
                &else_actual_block,
                else_ir.as_str(),
                &else_ty,
                &cty,
            )
        };
        (cty, tv, ev)
    } else if is_nullable_struct_type(&then_ty) && !is_nullable_struct_type(&else_ty) {
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
        // Plan A：i8* 可能是装箱整数，先经 aura_to_str_any 解析再 strlen
        let sptr = ctx.fresh_var();
        let len_var = ctx.fresh_var();
        let wrap_var = ctx.fresh_var();
        let wrap_var2 = ctx.fresh_var();
        insert_before_terminator(
            blocks,
            &else_actual_block,
            format!("{} = call i8* @aura_to_str_any(i8* {})", sptr, else_ir),
        );
        insert_before_terminator(
            blocks,
            &else_actual_block,
            format!("{} = call i64 @aura_string_length(i8* {})", len_var, sptr),
        );
        insert_before_terminator(
            blocks,
            &else_actual_block,
            format!(
                "{} = insertvalue {} undef, i8* {}, 0",
                wrap_var, then_ty, sptr
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
        // Plan A：同上，先解析可能的装箱整数
        let sptr = ctx.fresh_var();
        let len_var = ctx.fresh_var();
        let wrap_var = ctx.fresh_var();
        let wrap_var2 = ctx.fresh_var();
        insert_before_terminator(
            blocks,
            &then_actual_block,
            format!("{} = call i8* @aura_to_str_any(i8* {})", sptr, then_ir),
        );
        insert_before_terminator(
            blocks,
            &then_actual_block,
            format!("{} = call i64 @aura_string_length(i8* {})", len_var, sptr),
        );
        insert_before_terminator(
            blocks,
            &then_actual_block,
            format!(
                "{} = insertvalue {} undef, i8* {}, 0",
                wrap_var, else_ty, sptr
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
        if then_ty == else_ty {
            // 类型相同：数值类型用 add X,0 归一（恒等式，保证 PHI 操作数为寄存器），其余原样
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
        } else {
            // 类型不同（如 i32 与字符串 { i8*, i64 }）：协调为统一公共类型，
            // 两分支各 coerce 到该类型，确保 PHI 节点类型合法（值/指针表示统一）。
            let cty = common_if_type(&then_ty, &else_ty);
            let then_coerced = emit_coerce_to(
                ctx,
                blocks,
                &then_actual_block,
                then_ir.as_str(),
                &then_ty,
                &cty,
            );
            let else_coerced = emit_coerce_to(
                ctx,
                blocks,
                &else_actual_block,
                else_ir.as_str(),
                &else_ty,
                &cty,
            );
            (cty, then_coerced, else_coerced)
        }
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
        s.push_str(&format!(
            "  call void @{}({})\n",
            sanitizellvm(&func.name),
            args_str
        ));
    } else {
        s.push_str(&format!(
            "  {} = call {} @{}({})\n",
            call_var,
            ret_llvm_ty,
            sanitizellvm(&func.name),
            args_str
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
        s.push_str(&format!(
            "  call void @{}({})\n",
            sanitizellvm(&func.name),
            args_str
        ));
    } else {
        s.push_str(&format!(
            "  {} = call {} @{}({})\n",
            call_var,
            ret_llvm_ty,
            sanitizellvm(&func.name),
            args_str
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
