//! Bootstrap AOT 编译器核心（最小实现）。
//!
//! 职责（对应方案 Phase 1）：
//! - **LLVM IR 直发**：把字节码函数翻译为 LLVM IR 文本（与编译器主
//!   AOT 后端同方案：文本 IR + 外部 llc/clang，不依赖 llvm-sys/inkwell）；
//! - **AOT 直连**：FFI 调用生成 `declare @c_func` + `call @c_func`
//!   **直接调用指令**（非函数指针间接调用），LLVM 可进一步内联优化；
//! - **内联优化**：小函数（直线型、代码量 ≤ `inline_threshold`）在
//!   IR 生成前内联进调用方；
//! - **死代码消除**：仅发射从入口可达的函数。
//!
//! 类型：bootstrap 层无语义分析器，采用**局部类型推断**（常量/运算
//! 溯源，参数默认 i64），可通过 `FuncDef::sig` 提供显式签名。

use std::collections::HashSet;
use std::rc::Rc;

use super::vm_core::{BytecodeModule, CType, FfiCache, FuncDef, Insn, Value};

/// LLVM 值类型（bootstrap 子集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValType {
    I64,
    F64,
    I1,
    Ptr,
}

impl ValType {
    pub fn llvm(self) -> &'static str {
        match self {
            ValType::I64 => "i64",
            ValType::F64 => "double",
            ValType::I1 => "i1",
            ValType::Ptr => "ptr",
        }
    }

    /// 该类型的零常量文本。
    fn zero(self) -> &'static str {
        match self {
            ValType::I64 => "0",
            ValType::F64 => "0.0",
            ValType::I1 => "false",
            ValType::Ptr => "null",
        }
    }
}

fn val_type_of(v: &Value) -> ValType {
    match v {
        Value::Null | Value::Str(_) | Value::Ptr(_) => ValType::Ptr,
        Value::Bool(_) => ValType::I1,
        Value::Int(_) => ValType::I64,
        Value::Float(_) => ValType::F64,
    }
}

fn c_to_val(t: CType) -> ValType {
    match t {
        CType::I32 | CType::I64 | CType::U64 => ValType::I64,
        CType::F64 => ValType::F64,
        CType::Ptr => ValType::Ptr,
    }
}

/// AOT 配置。
#[derive(Clone, Copy, Debug)]
pub struct AotConfig {
    /// 优化级别（0-3），记录于 IR 元数据注释
    pub opt_level: u8,
    /// 内联阈值：直线型函数指令数 ≤ 该值时内联
    pub inline_threshold: usize,
}

impl Default for AotConfig {
    fn default() -> Self {
        Self {
            opt_level: 3,
            inline_threshold: 8,
        }
    }
}

/// AOT 生成器。
pub struct AotGenerator<'m> {
    pub module: &'m BytecodeModule,
    pub ffi: &'m FfiCache,
    pub config: AotConfig,
}

impl<'m> AotGenerator<'m> {
    pub fn new(module: &'m BytecodeModule, ffi: &'m FfiCache, config: AotConfig) -> Self {
        Self {
            module,
            ffi,
            config,
        }
    }

    /// 从 `entry` 出发生成完整 LLVM IR 文本。
    pub fn emit_llvm_ir(&self, entry: &str) -> Result<String, String> {
        let entry_idx = self
            .module
            .function_index(entry)
            .ok_or_else(|| format!("AOT: 入口函数不存在: {entry}"))?;

        // 1. 内联小函数（内联优化）
        let funcs = self.inline_small_functions();

        // 2. 死代码消除：仅保留从入口可达的函数
        let reachable = Self::reachable_functions(&funcs, entry_idx);

        // 3. 类型推断（函数返回类型递归推断 + 环检测）
        let mut ret_types: Vec<Option<ValType>> = vec![None; funcs.len()];
        let mut local_types: Vec<Vec<ValType>> = vec![Vec::new(); funcs.len()];
        for i in 0..funcs.len() {
            if reachable.contains(&i) {
                Self::infer_function(
                    &funcs,
                    self.ffi,
                    i,
                    &mut ret_types,
                    &mut local_types,
                    &mut HashSet::new(),
                )?;
            }
        }

        // 4. 字符串常量收集（去重）
        let mut strings: Vec<Rc<str>> = Vec::new();
        for i in reachable.iter() {
            collect_strings(&funcs[*i].code, &mut strings);
        }

        // 5. FFI 直连符号收集
        let mut ffi_slots: Vec<usize> = Vec::new();
        for i in reachable.iter() {
            for insn in &funcs[*i].code {
                if let Insn::CallFfi(slot) = insn {
                    let slot = *slot as usize;
                    if self.ffi.entry(slot).is_none() {
                        return Err(format!("AOT: FFI 槽位未预加载: {slot}"));
                    }
                    if !ffi_slots.contains(&slot) {
                        ffi_slots.push(slot);
                    }
                }
            }
        }

        // 6. 发射
        let mut ir = String::new();
        ir.push_str(&format!(
            "; Bootstrap AOT Module: {} (opt level {})\n",
            self.module.name, self.config.opt_level
        ));
        ir.push_str(&format!(
            "; FFI direct calls: {} symbol(s)\n",
            ffi_slots.len()
        ));

        for (n, s) in strings.iter().enumerate() {
            ir.push_str(&emit_string_global(n, s));
        }
        for slot in &ffi_slots {
            ir.push_str(&self.emit_ffi_decl(*slot));
        }

        for i in 0..funcs.len() {
            if reachable.contains(&i) {
                let body = self.emit_function(&funcs, i, &ret_types, &local_types, &strings)?;
                ir.push_str(&body);
            }
        }
        Ok(ir)
    }

    // ------------------------------------------------------------------
    // 内联优化
    // ------------------------------------------------------------------

    /// 内联所有直线型小函数（无跳转/嵌套调用/让出，仅末尾一条 Ret）。
    fn inline_small_functions(&self) -> Vec<FuncDef> {
        let mut funcs = self.module.funcs.clone();
        let inlinable: Vec<usize> = (0..funcs.len())
            .filter(|&i| Self::is_inlinable(&funcs[i], self.config.inline_threshold))
            .collect();

        for fi in 0..funcs.len() {
            if inlinable.contains(&fi) {
                continue;
            }
            let mut new_code: Vec<Insn> = Vec::with_capacity(funcs[fi].code.len());
            let mut base = funcs[fi].locals;

            for insn in &funcs[fi].code {
                match insn {
                    Insn::Call(callee) if inlinable.contains(&(*callee as usize)) => {
                        let c = &funcs[*callee as usize];
                        // 参数自栈顶逆序落入被内联函数的局部槽
                        for i in (0..c.params).rev() {
                            new_code.push(Insn::StoreLocal((base + i) as u16));
                        }
                        for ci in &c.code {
                            match ci {
                                Insn::Ret => {} // 返回值保留在操作数栈上
                                Insn::LoadLocal(i) => {
                                    new_code.push(Insn::LoadLocal(i + base as u16))
                                }
                                Insn::StoreLocal(i) => {
                                    new_code.push(Insn::StoreLocal(i + base as u16))
                                }
                                other => new_code.push(other.clone()),
                            }
                        }
                        base += c.locals;
                    }
                    other => new_code.push(other.clone()),
                }
            }
            funcs[fi].code = new_code;
            funcs[fi].locals = base;
        }
        funcs
    }

    fn is_inlinable(def: &FuncDef, threshold: usize) -> bool {
        if def.code.is_empty() || def.code.len() > threshold {
            return false;
        }
        let (body, last) = def.code.split_at(def.code.len() - 1);
        if !matches!(last[0], Insn::Ret) {
            return false;
        }
        body.iter().all(|i| {
            matches!(
                i,
                Insn::Const(_)
                    | Insn::LoadLocal(_)
                    | Insn::StoreLocal(_)
                    | Insn::Add
                    | Insn::Sub
                    | Insn::Mul
                    | Insn::Div
                    | Insn::Eq
                    | Insn::Lt
                    | Insn::CallFfi(_)
            )
        })
    }

    // ------------------------------------------------------------------
    // 死代码消除
    // ------------------------------------------------------------------

    fn reachable_functions(funcs: &[FuncDef], entry: usize) -> HashSet<usize> {
        let mut seen = HashSet::new();
        let mut work = vec![entry];
        while let Some(i) = work.pop() {
            if !seen.insert(i) {
                continue;
            }
            for insn in &funcs[i].code {
                if let Insn::Call(c) = insn {
                    work.push(*c as usize);
                }
            }
        }
        seen
    }

    // ------------------------------------------------------------------
    // 类型推断
    // ------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    fn infer_function(
        funcs: &[FuncDef],
        ffi: &FfiCache,
        fi: usize,
        ret_types: &mut Vec<Option<ValType>>,
        local_types: &mut Vec<Vec<ValType>>,
        visiting: &mut HashSet<usize>,
    ) -> Result<ValType, String> {
        if let Some(t) = ret_types[fi] {
            return Ok(t);
        }
        if !visiting.insert(fi) {
            return Err(format!("AOT: 递归函数不支持类型推断: {}", funcs[fi].name));
        }
        let def = &funcs[fi];

        let mut locals: Vec<Option<ValType>> = vec![None; def.locals];
        // 参数类型：显式签名优先，否则默认 i64
        for (i, p) in locals.iter_mut().enumerate().take(def.params) {
            *p = def
                .sig
                .as_ref()
                .and_then(|s| s.params.get(i).copied().flatten())
                .or(Some(ValType::I64));
        }

        let mut stack: Vec<ValType> = Vec::new();
        let mut ret: Option<ValType> = def.sig.as_ref().map(|s| s.ret);

        let local_ty = |ls: &[Option<ValType>], i: usize| -> Result<ValType, String> {
            ls.get(i)
                .copied()
                .flatten()
                .ok_or_else(|| format!("AOT: 局部变量 {i} 在初始化前使用（函数 {}）", def.name))
        };

        for insn in &def.code {
            match insn {
                Insn::Const(v) => stack.push(val_type_of(v)),
                Insn::LoadLocal(i) => stack.push(local_ty(&locals, *i as usize)?),
                Insn::StoreLocal(i) => {
                    let t = stack.pop().ok_or("AOT: StoreLocal 栈下溢")?;
                    let slot = &mut locals[*i as usize];
                    match *slot {
                        None => *slot = Some(t),
                        Some(prev) if prev != t => {
                            return Err(format!(
                                "AOT: 局部变量 {} 类型冲突 ({prev:?} vs {t:?})，函数 {}",
                                i, def.name
                            ));
                        }
                        _ => {}
                    }
                }
                Insn::Add | Insn::Sub | Insn::Mul | Insn::Div => {
                    let b = stack.pop().ok_or("AOT: 二元运算栈下溢")?;
                    let a = stack.pop().ok_or("AOT: 二元运算栈下溢")?;
                    stack.push(bin_type(&a, &b, &def.name)?);
                }
                Insn::Eq | Insn::Lt => {
                    stack.pop().ok_or("AOT: 比较栈下溢")?;
                    stack.pop().ok_or("AOT: 比较栈下溢")?;
                    stack.push(ValType::I1);
                }
                Insn::Jmp(_) => {}
                Insn::JmpIfFalse(_) => {
                    stack.pop().ok_or("AOT: 条件跳转栈下溢")?;
                }
                Insn::Call(c) => {
                    let ct = Self::infer_function(
                        funcs,
                        ffi,
                        *c as usize,
                        ret_types,
                        local_types,
                        visiting,
                    )?;
                    for _ in 0..funcs[*c as usize].params {
                        stack.pop().ok_or("AOT: 调用参数栈下溢")?;
                    }
                    stack.push(ct);
                }
                Insn::CallFfi(slot) => {
                    let entry = ffi
                        .entry(*slot as usize)
                        .ok_or_else(|| format!("AOT: FFI 槽位未预加载: {slot}"))?;
                    for _ in 0..entry.arity {
                        stack.pop().ok_or("AOT: FFI 参数栈下溢")?;
                    }
                    stack.push(c_to_val(entry.c_ret));
                }
                Insn::Ret => {
                    let t = stack.pop().or(def.sig.as_ref().map(|s| s.ret)).unwrap_or(ValType::I64);
                    match ret {
                        None => ret = Some(t),
                        Some(prev) if prev != t => {
                            return Err(format!(
                                "AOT: 返回类型冲突 ({prev:?} vs {t:?})，函数 {}",
                                def.name
                            ));
                        }
                        _ => {}
                    }
                }
                Insn::Yield => return Err("AOT: 协程 Yield 不支持 AOT 编译".to_string()),
            }
        }

        visiting.remove(&fi);
        let t = ret.ok_or_else(|| format!("AOT: 函数 {} 无返回类型", def.name))?;
        ret_types[fi] = Some(t);
        local_types[fi] = locals.into_iter().map(|o| o.unwrap_or(ValType::I64)).collect();
        Ok(t)
    }

    // ------------------------------------------------------------------
    // 发射
    // ------------------------------------------------------------------

    fn emit_ffi_decl(&self, slot: usize) -> String {
        let e = self.ffi.entry(slot).unwrap();
        let params = e.c_params.iter().map(|t| t.llvm()).collect::<Vec<_>>().join(", ");
        format!("declare {} @{}({})\n", e.c_ret.llvm(), e.name, params)
    }

    fn emit_function(
        &self,
        funcs: &[FuncDef],
        fi: usize,
        ret_types: &[Option<ValType>],
        local_types: &[Vec<ValType>],
        strings: &[Rc<str>],
    ) -> Result<String, String> {
        let def = &funcs[fi];
        let ret = ret_types[fi].unwrap();
        let ltys = &local_types[fi];
        let fqn = format!("\"{}{}\"", AOT_PREFIX, def.name);

        let params: Vec<&str> = (0..def.params).map(|i| ltys[i].llvm()).collect();
        let mut out = format!("define {} @{}({}) {{\n", ret.llvm(), fqn, params.join(", "));

        // 局部槽 alloca
        for (i, t) in ltys.iter().enumerate() {
            out.push_str(&format!("  %l{i} = alloca {}\n", t.llvm()));
        }
        // 形参存入槽（LLVM 隐式命名 %p0..%pn）
        for i in 0..def.params {
            out.push_str(&format!("  store {} %p{i}, ptr %l{i}\n", ltys[i].llvm()));
        }

        let mut ssa = 0usize;
        let mut stack: Vec<(ValType, String)> = Vec::new();
        let n = def.code.len();

        for (ip, insn) in def.code.iter().enumerate() {
            out.push_str(&format!("bb{ip}:\n"));

            let fall = format!("%bb{}", ip + 1);
            match insn {
                Insn::Const(v) => {
                    let t = val_type_of(v);
                    let text = match v {
                        Value::Null => "null".to_string(),
                        Value::Bool(b) => b.to_string(),
                        Value::Int(i) => i.to_string(),
                        Value::Float(f) => format!("0x{:016X}", f.to_bits()),
                        Value::Str(s) => {
                            let idx = strings.iter().position(|x| x == s).unwrap_or_default();
                            format!("@.str{idx}")
                        }
                        Value::Ptr(p) => format!("inttoptr (i64 {p} to ptr)"),
                    };
                    stack.push((t, text));
                }
                Insn::LoadLocal(i) => {
                    let t = ltys[*i as usize];
                    ssa += 1;
                    out.push_str(&format!("  %t{ssa} = load {}, ptr %l{}\n", t.llvm(), i));
                    stack.push((t, format!("%t{ssa}")));
                }
                Insn::StoreLocal(i) => {
                    let (t, v) = stack.pop().ok_or("AOT: StoreLocal 栈下溢")?;
                    out.push_str(&format!("  store {} {}, ptr %l{}\n", t.llvm(), v, i));
                }
                Insn::Add | Insn::Sub | Insn::Mul | Insn::Div => {
                    let (bt, bv) = stack.pop().ok_or("AOT: 栈下溢")?;
                    let (at, av) = stack.pop().ok_or("AOT: 栈下溢")?;
                    let t = bin_type(&at, &bt, &def.name)?;
                    ssa += 1;
                    let mn = match (insn, t) {
                        (Insn::Add, ValType::F64) => "fadd",
                        (Insn::Sub, ValType::F64) => "fsub",
                        (Insn::Mul, ValType::F64) => "fmul",
                        (Insn::Div, ValType::F64) => "fdiv",
                        (Insn::Add, _) => "add",
                        (Insn::Sub, _) => "sub",
                        (Insn::Mul, _) => "mul",
                        (Insn::Div, _) => "sdiv",
                        _ => unreachable!(),
                    };
                    out.push_str(&format!("  %t{ssa} = {mn} {} {av}, {bv}\n", t.llvm()));
                    stack.push((t, format!("%t{ssa}")));
                }
                Insn::Eq | Insn::Lt => {
                    let (bt, bv) = stack.pop().ok_or("AOT: 栈下溢")?;
                    let (at, av) = stack.pop().ok_or("AOT: 栈下溢")?;
                    if at != bt {
                        return Err(format!(
                            "AOT: 比较操作数类型不一致 ({at:?} vs {bt:?})，函数 {}",
                            def.name
                        ));
                    }
                    ssa += 1;
                    let cmp = match (insn, at) {
                        (Insn::Eq, ValType::F64) => "fcmp oeq",
                        (Insn::Lt, ValType::F64) => "fcmp olt",
                        (Insn::Eq, _) => "icmp eq",
                        (Insn::Lt, _) => "icmp slt",
                        _ => unreachable!(),
                    };
                    out.push_str(&format!("  %t{ssa} = {cmp} {} {av}, {bv}\n", at.llvm()));
                    stack.push((ValType::I1, format!("%t{ssa}")));
                }
                Insn::Jmp(off) => {
                    let t = (ip as i64 + 1 + *off as i64) as usize;
                    out.push_str(&format!("  br label %bb{t}\n"));
                }
                Insn::JmpIfFalse(off) => {
                    let (_, c) = stack.pop().ok_or("AOT: 条件跳转栈下溢")?;
                    let t = (ip as i64 + 1 + *off as i64) as usize;
                    out.push_str(&format!("  br i1 {c}, label %bb{t}, label {fall}\n"));
                }
                Insn::Call(c) => {
                    let ci = *c as usize;
                    let cdef = &funcs[ci];
                    let cret = ret_types[ci].unwrap();
                    let mut args = Vec::new();
                    for i in (0..cdef.params).rev() {
                        let (t, v) = stack.pop().ok_or("AOT: 调用参数栈下溢")?;
                        let want = local_types[ci][i];
                        let v = convert(&mut out, &mut ssa, t, want, v);
                        args.push(format!("{} {}", want.llvm(), v));
                    }
                    args.reverse();
                    ssa += 1;
                    out.push_str(&format!(
                        "  %t{ssa} = call {} @\"{}{}\"({})\n",
                        cret.llvm(),
                        AOT_PREFIX,
                        cdef.name,
                        args.join(", ")
                    ));
                    stack.push((cret, format!("%t{ssa}")));
                }
                Insn::CallFfi(slot) => {
                    let e = self.ffi.entry(*slot as usize).unwrap();
                    let mut args = Vec::new();
                    for cp in e.c_params.iter().rev() {
                        let (t, v) = stack.pop().ok_or("AOT: FFI 参数栈下溢")?;
                        // 转换为 C 形参的真实 LLVM 类型（如 i64 → i32 trunc）
                        let (ty, v) = convert_to_c(&mut out, &mut ssa, t, *cp, v);
                        args.push(format!("{ty} {v}"));
                    }
                    args.reverse();
                    ssa += 1;
                    let raw = format!("%t{ssa}");
                    out.push_str(&format!(
                        "  %t{ssa} = call {} @{}({}) ; AOT 直连：直接调用 C 符号，无函数指针\n",
                        e.c_ret.llvm(),
                        e.name,
                        args.join(", ")
                    ));
                    // 返回值类型规整：C int (i32) → i64
                    let want = c_to_val(e.c_ret);
                    let v = if e.c_ret == CType::I32 {
                        ssa += 1;
                        out.push_str(&format!("  %t{ssa} = sext i32 {raw} to i64\n"));
                        format!("%t{ssa}")
                    } else {
                        raw
                    };
                    stack.push((want, v));
                }
                Insn::Ret => {
                    let v = match stack.pop() {
                        Some((t, v)) => {
                            if t != ret {
                                return Err(format!(
                                    "AOT: 返回类型不匹配 ({t:?} vs {ret:?})，函数 {}",
                                    def.name
                                ));
                            }
                            v
                        }
                        None => ret.zero().to_string(),
                    };
                    out.push_str(&format!("  ret {} {v}\n", ret.llvm()));
                }
                Insn::Yield => return Err("AOT: Yield 不支持 AOT 编译".to_string()),
            }

            // 非终止指令需要显式跳转到下一块
            let is_terminator = matches!(insn, Insn::Jmp(_) | Insn::JmpIfFalse(_) | Insn::Ret);
            if !is_terminator && ip + 1 < n {
                out.push_str(&format!("  br label %bb{}\n", ip + 1));
            }
        }
        out.push_str("}\n\n");
        Ok(out)
    }
}

/// 操作数类型规整（需要时向 `out` 追加转换指令）。
fn convert(out: &mut String, ssa: &mut usize, have: ValType, want: ValType, v: String) -> String {
    if have == want {
        return v;
    }
    *ssa += 1;
    let name = format!("%t{ssa}");
    match (have, want) {
        (ValType::I64, ValType::I1) => {
            out.push_str(&format!("  {name} = icmp ne i64 {v}, 0\n"));
        }
        (ValType::I1, ValType::I64) => {
            out.push_str(&format!("  {name} = zext i1 {v} to i64\n"));
        }
        (ValType::I64, ValType::F64) => {
            out.push_str(&format!("  {name} = sitofp i64 {v} to double\n"));
        }
        (ValType::F64, ValType::I64) => {
            out.push_str(&format!("  {name} = fptosi double {v} to i64\n"));
        }
        (ValType::I64, ValType::Ptr) => {
            out.push_str(&format!("  {name} = inttoptr i64 {v} to ptr\n"));
        }
        (ValType::Ptr, ValType::I64) => {
            out.push_str(&format!("  {name} = ptrtoint ptr {v} to i64\n"));
        }
        _ => {}
    }
    name
}

/// 把 bootstrap 操作数转换为 **C 形参真实 LLVM 类型**（FFI 直连的
/// ABI 转换：如 i64 → C int 的 trunc），返回 `(llvm 类型, 值)`。
fn convert_to_c(
    out: &mut String,
    ssa: &mut usize,
    have: ValType,
    want: CType,
    v: String,
) -> (&'static str, String) {
    match want {
        CType::I32 => match have {
            ValType::I1 => {
                *ssa += 1;
                out.push_str(&format!("  %t{ssa} = zext i1 {v} to i32\n"));
                ("i32", format!("%t{ssa}"))
            }
            ValType::I64 => {
                *ssa += 1;
                out.push_str(&format!("  %t{ssa} = trunc i64 {v} to i32\n"));
                ("i32", format!("%t{ssa}"))
            }
            ValType::F64 => {
                *ssa += 1;
                out.push_str(&format!("  %t{ssa} = fptosi double {v} to i32\n"));
                ("i32", format!("%t{ssa}"))
            }
            ValType::Ptr => ("i32", "0".to_string()),
        },
        CType::I64 | CType::U64 => {
            let v = convert(out, ssa, have, ValType::I64, v);
            ("i64", v)
        }
        CType::F64 => {
            let v = convert(out, ssa, have, ValType::F64, v);
            ("double", v)
        }
        CType::Ptr => {
            let v = convert(out, ssa, have, ValType::Ptr, v);
            ("ptr", v)
        }
    }
}

fn bin_type(a: &ValType, b: &ValType, fname: &str) -> Result<ValType, String> {
    match (a, b) {
        (ValType::F64, _) | (_, ValType::F64) => Ok(ValType::F64),
        (ValType::I64, ValType::I64) => Ok(ValType::I64),
        _ => Err(format!(
            "AOT: 函数 {fname} 二元运算不支持类型组合 ({a:?}, {b:?})"
        )),
    }
}

fn collect_strings(code: &[Insn], out: &mut Vec<Rc<str>>) {
    for insn in code {
        if let Insn::Const(Value::Str(s)) = insn {
            if !out.contains(s) {
                out.push(s.clone());
            }
        }
    }
}

fn emit_string_global(n: usize, s: &str) -> String {
    let bytes = s.as_bytes();
    let mut esc = String::with_capacity(bytes.len() * 4);
    for &b in bytes {
        match b {
            b'"' => esc.push_str("\\22"),
            b'\\' => esc.push_str("\\5C"),
            0x20..=0x7E => esc.push(b as char),
            _ => esc.push_str(&format!("\\{b:02X}")),
        }
    }
    format!(
        "@.str{n} = private unnamed_addr constant [{} x i8] c\"{}\\00\"\n",
        bytes.len() + 1,
        esc
    )
}

/// Bootstrap 层 LLVM 符号前缀（命名空间隔离）。
pub const AOT_PREFIX: &str = "aura.bs.";
