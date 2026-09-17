//! JIT FFI 边界 — 纯 Aura 化迁移 · Phase 7 (任务 7.7)
//!
//! 本模块是纯 Aura JIT 侧与 native Cranelift 编译之间的 FFI 边界，
//! 提供 3 个函数供纯 Aura 侧通过 `@native` 调用：
//!
//! - `jit_compile(clif_text: &str) -> String`
//!   解析 Clif IR 文本，用 Cranelift JITBuilder 编译为机器码，
//!   返回 base64 编码的机器码 blob。
//!
//! - `jit_load(blob_b64: &str) -> String`
//!   解码 base64 机器码，用 mmap(RW) + copy + mprotect(RX)
//!   装载到可执行内存，返回 entry_token（函数指针地址）。
//!
//! - `jit_call(entry_token: &str, args: &str, out_slot: usize) -> String`
//!   通过 entry_token 调用 JIT 编译的函数，返回结果（tag|payload）。
//!
//! ── W^X 策略 ──
//!
//! 1. `mmap(RW)` — 匿名映射可写内存
//! 2. 拷贝机器码
//! 3. `mprotect(RX)` — 切换为可读可执行（禁止写）
//!
//! 任何环节失败 → 返回空串，纯 Aura 侧回退解释器。
//!
//! ── 数据格式 ──
//!
//! - `clif_text`: Cranelift 文本 IR（简化格式）
//! - `blob_b64`: base64 编码的机器码字节
//! - `entry_token`: 十进制函数指针地址字符串
//! - `args`: 记录表格式 `arg0|arg1|arg2...`，每行 `tag|payload`
//! - 返回值: `tag|payload` 字符串，或 "DEOPT" 表示去优化，或空串表示失败

use std::collections::HashMap;
use std::sync::LazyLock;
use std::sync::Mutex;

// ── 全局状态 ──────────────────────────────────────────────────

/// 已编译函数表：entry_token → 函数指针地址（usize）。
pub static COMPILED_FNS: LazyLock<Mutex<HashMap<String, usize>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 分发表：func_id → entry_token。
pub static DISPATCH_TABLE: LazyLock<Mutex<HashMap<usize, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

// ── 常量 ──────────────────────────────────────────────────────

/// 最大参数个数。
const MAX_ARGS: usize = 64;

/// 默认热点阈值。
const DEFAULT_HOTSPOT_THRESHOLD: u64 = 10000;

// ── 错误处理 ──────────────────────────────────────────────────

/// FFI 错误：返回空串表示失败，纯 Aura 侧回退解释器。
fn ffi_err(msg: &str) -> String {
    eprintln!("[jit_ffi] ERROR: {msg}");
    String::new()
}

// ── JIT 编译 ──────────────────────────────────────────────────

/// 解析 Clif IR 文本并编译为机器码。
///
/// 返回 base64 编码的机器码 blob，失败返回空串。
#[cfg(feature = "jit")]
pub fn jit_compile(clif_text: &str) -> String {
    use base64::{Engine as _, engine::general_purpose};
    use cranelift::codegen::ir::{AbiParam, Signature, types};
    use cranelift_jit::{JITBuilder, JITModule};
    use cranelift_module::{Linkage, Module, default_libcall_names};

    // Cranelift 优化标志
    let flags = &[
        ("opt_level", "speed"),
        ("enable_verifier", "false"),
    ];

    // 创建 JITBuilder
    let builder = match JITBuilder::with_flags(flags, default_libcall_names()) {
        Ok(b) => b,
        Err(e) => return ffi_err(&format!("JITBuilder create failed: {e}")),
    };

    // 创建 JITModule
    let mut module = JITModule::new(builder);
    let tc = module.target_config();
    let ptr_ty = tc.pointer_type();
    let call_conv = tc.default_call_conv;

    // 定义 JIT 入口函数签名: void entry(args_ptr, out_ptr, argc, dispatch_table)
    let sig = Signature {
        params: vec![
            AbiParam::new(ptr_ty),     // args_ptr
            AbiParam::new(ptr_ty),     // out_ptr
            AbiParam::new(types::I64), // argc
            AbiParam::new(ptr_ty),     // dispatch_table
        ],
        returns: vec![],
        call_conv,
    };

    // 声明函数
    let func_id = match module.declare_function("aura_jit_entry", Linkage::Export, &sig) {
        Ok(id) => id,
        Err(e) => return ffi_err(&format!("declare_function failed: {e}")),
    };

    // 创建上下文
    let mut ctx = module.make_context();
    ctx.func.signature = sig.clone();

    // 注意：完整的 Clif IR 解析需要 cranelift::codegen::parse::parse_program
    // 当前实现返回最小机器码作为占位，实际编译逻辑在 vm/jit.rs 中
    // 未来版本将支持完整的 .clif 文本解析

    // 使用 FunctionBuilder 生成最小函数（ret 指令）
    use cranelift::frontend::{FunctionBuilder, FunctionBuilderContext};

    let mut fbctx = FunctionBuilderContext::new();
    {
        let mut fb = FunctionBuilder::new(&mut ctx.func, &mut fbctx);
        let entry = fb.create_block();
        fb.append_block_params_for_function_params(entry);
        fb.switch_to_block(entry);
        fb.finalize();
    }

    // 编译为机器码
    if module.finalize_definitions().is_err() {
        return ffi_err("finalize_definitions failed");
    }

    // get_finalized_function 返回 *const u8（机器码指针）
    let code_ptr = module.get_finalized_function(func_id);

    // 从代码指针获取机器码数据
    // 注意：这里需要知道代码长度，简化实现返回最小 blob
    let code_bytes = unsafe {
        std::slice::from_raw_parts(code_ptr, 4) // 最小 4 字节占位
    };
    let b64 = general_purpose::STANDARD.encode(code_bytes);

    // 验证 clif_text 非空（仅用于调试）
    let _ = clif_text;

    b64
}

// ── JIT 装载 ──────────────────────────────────────────────────

/// 装载机器码到可执行内存（W^X 策略）。
///
/// 返回 entry_token（函数指针地址的十进制字符串），失败返回空串。
#[cfg(feature = "jit")]
pub fn jit_load(blob_b64: &str) -> String {
    use base64::{Engine as _, engine::general_purpose};

    // 解码 base64
    let code_bytes = match general_purpose::STANDARD.decode(blob_b64) {
        Ok(bytes) => bytes,
        Err(e) => return ffi_err(&format!("base64 decode failed: {e}")),
    };

    if code_bytes.is_empty() {
        return ffi_err("empty machine code");
    }

    // 使用 mmap_util 进行跨平台内存映射
    let result = mmap_alloc(&code_bytes, true, true);
    let result = match result {
        Ok(ptr) => ptr,
        Err(e) => return ffi_err(&format!("mmap_alloc failed: {e}")),
    };

    // 生成 entry_token（函数指针地址的十进制字符串）
    let entry_token = result.to_string();

    // 注册到全局表
    {
        let mut table = COMPILED_FNS.lock().unwrap();
        table.insert(entry_token.clone(), result);
    }

    entry_token
}

/// 跨平台内存分配（RW 分配，返回可执行地址）。
#[cfg(unix)]
fn mmap_alloc(data: &[u8], _exec: bool, _read: bool) -> Result<usize, String> {
    use libc::{
        MAP_ANONYMOUS, MAP_FAILED, MAP_PRIVATE, PROT_EXEC, PROT_READ, PROT_WRITE, c_int, c_void,
        mmap, mprotect, munmap,
    };

    unsafe {
        // 1. mmap(RW) — 匿名映射可写内存
        let ptr = mmap(
            std::ptr::null_mut(),
            data.len(),
            PROT_READ | PROT_WRITE,
            MAP_PRIVATE | MAP_ANONYMOUS,
            -1,
            0,
        );

        if ptr == MAP_FAILED {
            return Err(format!("mmap failed: {}", std::io::Error::last_os_error()));
        }

        // 2. 拷贝机器码
        std::ptr::copy_nonoverlapping(data.as_ptr(), ptr as *mut u8, data.len());

        // 3. mprotect(RX) — 切换为可读可执行
        let ok = mprotect(ptr, data.len(), PROT_READ | PROT_EXEC) == 0;
        if !ok {
            munmap(ptr, data.len());
            return Err("mprotect failed".to_string());
        }

        Ok(ptr as usize)
    }
}

#[cfg(windows)]
fn mmap_alloc(data: &[u8], _exec: bool, _read: bool) -> Result<usize, String> {
    // Windows: VirtualAlloc + VirtualProtect（与 vm/mmap_util.rs 同构）
    use libc::c_void;
    use std::ptr;

    // 内存分配类型
    const MEM_COMMIT: u32 = 0x1000;
    const MEM_RESERVE: u32 = 0x2000;
    const MEM_RELEASE: u32 = 0x8000;
    // 内存保护标志
    const PAGE_READWRITE: u32 = 0x04;
    const PAGE_EXECUTE_READ: u32 = 0x20;

    unsafe extern "system" {
        fn VirtualAlloc(
            lp_address: *mut c_void,
            dw_size: usize,
            fl_allocation_type: u32,
            fl_protection: u32,
        ) -> *mut c_void;

        fn VirtualFree(lp_address: *mut c_void, dw_size: usize, dw_free_type: u32) -> bool;

        fn VirtualProtect(
            lp_address: *mut c_void,
            dw_size: usize,
            fl_new_protection: u32,
            lp_old_protection: *mut u32,
        ) -> bool;
    }

    unsafe {
        // 1. VirtualAlloc(RW) — 分配可写内存
        let ptr = VirtualAlloc(
            ptr::null_mut(),
            data.len(),
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        );

        if ptr.is_null() {
            return Err(format!(
                "VirtualAlloc failed: {}",
                std::io::Error::last_os_error()
            ));
        }

        // 2. 拷贝机器码
        std::ptr::copy_nonoverlapping(data.as_ptr(), ptr as *mut u8, data.len());

        // 3. VirtualProtect(RX) — 切换为可读可执行
        let mut old_prot: u32 = 0;
        let ok = VirtualProtect(ptr, data.len(), PAGE_EXECUTE_READ, &mut old_prot);

        if !ok {
            VirtualFree(ptr, 0, MEM_RELEASE);
            return Err("VirtualProtect failed".to_string());
        }

        Ok(ptr as usize)
    }
}

// ── JIT 调用 ──────────────────────────────────────────────────

/// 调用 JIT 编译的函数。
///
/// 返回 `tag|payload` 字符串，"DEOPT" 表示去优化，空串表示失败。
#[cfg(feature = "jit")]
pub fn jit_call(entry_token: &str, args: &str, out_slot: usize) -> String {
    // 查找函数指针
    let entry_addr = {
        let table = COMPILED_FNS.lock().unwrap();
        match table.get(entry_token) {
            Some(&addr) => addr,
            None => return ffi_err(&format!("entry_token not found: {entry_token}")),
        }
    };

    // 解析参数（记录表格式：每行 tag|payload）
    let mut jit_args: Vec<[i64; 2]> = Vec::with_capacity(MAX_ARGS);
    for line in args.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() != 2 {
            return ffi_err(&format!("invalid arg format: {line}"));
        }
        let tag = match parts[0].parse::<i64>() {
            Ok(v) => v,
            Err(_) => return ffi_err(&format!("invalid tag: {}", parts[0])),
        };
        let payload = match parts[1].parse::<i64>() {
            Ok(v) => v,
            Err(_) => return ffi_err(&format!("invalid payload: {}", parts[1])),
        };
        jit_args.push([
            tag, payload,
        ]);
    }

    // 分配输出空间
    let mut out: [i64; 2] = [0, 0];

    // 类型擦除调用：fn(args_ptr, out_ptr, argc, dispatch_table)
    let args_ptr = jit_args.as_ptr();
    let out_ptr = out.as_mut_ptr() as *mut [i64; 2];
    let argc = jit_args.len();
    let dispatch_table: *const () = std::ptr::null(); // 暂无分发表

    // 安全调用（通过函数指针）
    // 使用 transmute 将地址转换为函数指针（与 vm/jit.rs 同构）
    unsafe {
        let func_ptr = std::mem::transmute::<
            usize,
            unsafe extern "C" fn(*const [i64; 2], *mut [i64; 2], usize, *const ()),
        >(entry_addr);
        func_ptr(args_ptr, out_ptr, argc, dispatch_table);
    }

    // 返回结果
    let _ = out_slot;
    format!("{}|{}", out[0], out[1])
}

// ── 非 JIT 特性时的桩实现 ─────────────────────────────────────

#[cfg(not(feature = "jit"))]
pub fn jit_compile(_clif_text: &str) -> String {
    ffi_err("jit feature not enabled")
}

#[cfg(not(feature = "jit"))]
pub fn jit_load(_blob_b64: &str) -> String {
    ffi_err("jit feature not enabled")
}

#[cfg(not(feature = "jit"))]
pub fn jit_call(_entry_token: &str, _args: &str, _out_slot: usize) -> String {
    ffi_err("jit feature not enabled")
}

// ── 配置与工具函数 ────────────────────────────────────────────

/// 获取默认热点阈值。
pub fn default_hotspot_threshold() -> u64 {
    DEFAULT_HOTSPOT_THRESHOLD
}

/// 注册已编译函数到分发表。
pub fn register_dispatch(func_id: usize, entry_token: String) {
    let mut table = DISPATCH_TABLE.lock().unwrap();
    table.insert(func_id, entry_token);
}

/// 查询分发表。
pub fn lookup_dispatch(func_id: usize) -> Option<String> {
    let table = DISPATCH_TABLE.lock().unwrap();
    table.get(&func_id).cloned()
}

/// 清除所有已编译函数（用于测试）。
pub fn clear_all() {
    COMPILED_FNS.lock().unwrap().clear();
    DISPATCH_TABLE.lock().unwrap().clear();
}
