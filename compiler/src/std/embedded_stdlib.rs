//! Phase 3: 嵌入式标准库（预编译 .auc 嵌入二进制）
//!
//! `aura/core/aura/lang/**/*.aura` 是唯一真相源，构建时预编译为 .auc 并嵌入
//! 二进制，VM 启动时自动加载，优先执行嵌入版本（而非 Rust native 或字节码解释）。
//!
//! 编译（**两步，顺序不能颠倒**）：
//! ```text
//! aura stdlib-compile aura/core/aura/lang/std        --output build
//! aura stdlib-compile aura/core/aura/lang/concurrent --output build
//! cargo build -p cli --features llvm
//! ```
//! 第二步的 `cargo build` 不能省：`.auc` 是通过 `include_bytes!` 编进二进制的，
//! 只跑 `stdlib-compile` 不会刷新运行期看到的副本。
//!
//! 调用优先级：
//!   1. 嵌入 .auc 中的 Aura 编译函数（`stdlib_func_map`，非 native 声明）
//!   2. Rust native（Layer 0-A 引导层 + FFI）
//!
//! ── 合并期下标重定位 ──
//! `load_embedded_stdlib` 把嵌入模块的函数追加到宿主模块函数表时，会同步：
//!   * 追加该模块的常量池与原生函数表；
//!   * 平移指令里的常量 / native / 函数下标（`remap_embedded_instrs`）；
//!   * 为 AOT 分发表构造**按宿主函数下标索引**的稀疏 desc 表。
//! 因此嵌入模块可以调用 native（如 `Memory.alloc`）并跨函数调用。
//!
//! ── 表中包含哪些模块 ──
//!   * `aura.lang.std` 的纯逻辑模块（Math / Time / ...）；
//!   * `aura.lang.concurrent` 的纯 Aura 实现：Atomic / Mutex / RwLock / Condvar /
//!     Barrier / Semaphore / Thread / Future。
//!     这些类只把**不可在 Aura 源码层表达**的部分留给 native：
//!     `Memory.*` / `Cpu.atomicAdd`（原语）与 `ThreadOps.*`（线程桥）。
//! `aura.lang.concurrent` 的 Coroutine / Actor / Channel 不在此表：
//! 它们分别是自举运行时类与 Rust 运行时设施，不作为用户可见标准库嵌入。
//!
//! 目录结构（v3.2）：
//!   aura/core/aura/lang/std/        — 用户标准库（Layer 1+）
//!   aura/core/aura/lang/concurrent/ — 并发设施
//!   aura/core/aura/lang/native/     — native 原语声明（Layer 0-A）
//!   aura/compiler/aura/lang/compiler/ — 编译器基础设施（Layer 0-B）
//!   build/                            — 预编译 .auc 文件（输出目录）

/// 嵌入的 Math 标准库 — 纯逻辑
pub static EMBEDDED_MATH_AUC: &[u8] = include_bytes!(concat!("../../../build/", "Math.auc"));

/// 嵌入的 Time 标准库 — 纯逻辑
pub static EMBEDDED_TIME_AUC: &[u8] = include_bytes!(concat!("../../../build/", "Time.auc"));

/// 嵌入的 Collections 标准库 — 纯逻辑
pub static EMBEDDED_COLLECTIONS_AUC: &[u8] =
    include_bytes!(concat!("../../../build/", "Collections.auc"));

/// 嵌入的 Test 标准库 — 纯逻辑
pub static EMBEDDED_TEST_AUC: &[u8] = include_bytes!(concat!("../../../build/", "Test.auc"));

/// 嵌入的 Ascii 标准库 — 纯逻辑
pub static EMBEDDED_ASCII_AUC: &[u8] = include_bytes!(concat!("../../../build/", "Ascii.auc"));

/// 嵌入的 Assert 标准库 — 纯逻辑
pub static EMBEDDED_ASSERT_AUC: &[u8] = include_bytes!(concat!("../../../build/", "Assert.auc"));

/// 嵌入的 Encoding 标准库 — 纯逻辑（Base64 / Hex）
pub static EMBEDDED_ENCODING_AUC: &[u8] =
    include_bytes!(concat!("../../../build/", "Encoding.auc"));

/// 嵌入的 Iter 标准库 — 纯逻辑（函数式工具）
pub static EMBEDDED_ITER_AUC: &[u8] = include_bytes!(concat!("../../../build/", "Iter.auc"));

/// 嵌入的 Json 标准库 — 纯逻辑（JSON parse/stringify）
pub static EMBEDDED_JSON_AUC: &[u8] = include_bytes!(concat!("../../../build/", "Json.auc"));

/// 嵌入的 StringBuilder 标准库 — 纯逻辑
pub static EMBEDDED_SB_AUC: &[u8] = include_bytes!(concat!("../../../build/", "StringBuilder.auc"));

/// 嵌入的 TestHelper 标准库 — 纯逻辑
pub static EMBEDDED_TEST_HELPER_AUC: &[u8] =
    include_bytes!(concat!("../../../build/", "TestHelper.auc"));

/// 嵌入的 Path 标准库 — 纯逻辑（路径操作）
pub static EMBEDDED_PATH_AUC: &[u8] = include_bytes!(concat!("../../../build/", "Path.auc"));

/// 嵌入的 String 标准库 — 纯逻辑
pub static EMBEDDED_STRING_AUC: &[u8] = include_bytes!(concat!("../../../build/", "String.auc"));

// ── aura.lang.native（native 原语辅助模块）──
//
// 这些模块是纯 Aura 实现，为 std/ 包中的模块提供底层数学/IO/网络等辅助功能。

/// 嵌入的 MathOps（纯 Aura 数学运算）
pub static EMBEDDED_MATH_OPS_AUC: &[u8] =
    include_bytes!(concat!("../../../build/", "math/MathOps.auc"));

/// 嵌入的 Stdio（纯 Aura IO 操作）
pub static EMBEDDED_STDIO_AUC: &[u8] = include_bytes!(concat!("../../../build/", "io/Stdio.auc"));

/// 嵌入的 EnvOps（纯 Aura 环境变量操作）
pub static EMBEDDED_ENV_OPS_AUC: &[u8] =
    include_bytes!(concat!("../../../build/", "env/EnvOps.auc"));

/// 嵌入的 FileOps（纯 Aura 文件操作）
pub static EMBEDDED_FILE_OPS_AUC: &[u8] =
    include_bytes!(concat!("../../../build/", "file/FileOps.auc"));

/// 嵌入的 Console（纯 Aura 控制台操作）
pub static EMBEDDED_CONSOLE_AUC: &[u8] =
    include_bytes!(concat!("../../../build/", "console/Console.auc"));

/// 嵌入的 Clock（纯 Aura 时钟操作）
pub static EMBEDDED_CLOCK_AUC: &[u8] = include_bytes!(concat!("../../../build/", "time/Clock.auc"));

/// 嵌入的 NetworkOps（纯 Aura 网络操作）
pub static EMBEDDED_NETWORK_OPS_AUC: &[u8] =
    include_bytes!(concat!("../../../build/", "network/NetworkOps.auc"));

// ── aura.lang.concurrent（纯 Aura 同步原语）──
//
// 这些模块是纯 Aura 实现，底层通过 native 包（`aura.lang.native.Memory` /
// `aura.lang.native.Cpu`）调用内存分配与内联汇编原子指令。加载器
// `load_embedded_stdlib` 会在合并时重定位常量池 / 原生表 / 函数表下标
// （`remap_embedded_instrs`），因此它们可以安全地被嵌入并优先派发。

/// 嵌入的 Atomic（AOT 机器码）— 纯 Aura
pub static EMBEDDED_ATOMIC_AUC: &[u8] = include_bytes!(concat!("../../../build/", "Atomic.auc"));

/// 嵌入的 Mutex（AOT 机器码）— 纯 Aura（自旋锁）
pub static EMBEDDED_MUTEX_AUC: &[u8] = include_bytes!(concat!("../../../build/", "Mutex.auc"));

/// 嵌入的 RwLock（AOT 机器码）— 纯 Aura（自旋锁）
pub static EMBEDDED_RWLOCK_AUC: &[u8] = include_bytes!(concat!("../../../build/", "RwLock.auc"));

/// 嵌入的 Condvar（AOT 机器码）— 纯 Aura
pub static EMBEDDED_CONDVAR_AUC: &[u8] = include_bytes!(concat!("../../../build/", "Condvar.auc"));

/// 嵌入的 Barrier（AOT 机器码）— 纯 Aura
pub static EMBEDDED_BARRIER_AUC: &[u8] = include_bytes!(concat!("../../../build/", "Barrier.auc"));

/// 嵌入的 Semaphore（AOT 机器码）— 纯 Aura
pub static EMBEDDED_SEMAPHORE_AUC: &[u8] =
    include_bytes!(concat!("../../../build/", "Semaphore.auc"));

/// 嵌入的 Thread（纯 Aura，仅线程桥走 native `ThreadOps`）
pub static EMBEDDED_THREAD_AUC: &[u8] = include_bytes!(concat!("../../../build/", "Thread.auc"));

/// 嵌入的 Future（纯 Aura，结果槽 + 状态机）
pub static EMBEDDED_FUTURE_AUC: &[u8] = include_bytes!(concat!("../../../build/", "Future.auc"));

/// 标准库包前缀：并发设施
pub const PKG_CONCURRENT: &str = "aura.lang.concurrent";

/// 标准库包前缀：native 辅助模块
pub const PKG_NATIVE_MATH: &str = "aura.lang.native.math";
pub const PKG_NATIVE_IO: &str = "aura.lang.native.io";
pub const PKG_NATIVE_ENV: &str = "aura.lang.native.env";
pub const PKG_NATIVE_FILE: &str = "aura.lang.native.file";
pub const PKG_NATIVE_CONSOLE: &str = "aura.lang.native.console";
pub const PKG_NATIVE_TIME: &str = "aura.lang.native.time";
pub const PKG_NATIVE_NETWORK: &str = "aura.lang.native.network";

/// 所有嵌入的标准库模块：(模块名, 包前缀, .auc 字节)
pub static EMBEDDED_STDLIB_MODULES: &[(&str, &str, &[u8])] = &[
    ("Math", "aura.lang.std", EMBEDDED_MATH_AUC),
    ("Time", "aura.lang.std", EMBEDDED_TIME_AUC),
    ("Collections", "aura.lang.std", EMBEDDED_COLLECTIONS_AUC),
    ("Test", "aura.lang.std", EMBEDDED_TEST_AUC),
    ("Ascii", "aura.lang.std", EMBEDDED_ASCII_AUC),
    ("Assert", "aura.lang.std", EMBEDDED_ASSERT_AUC),
    ("Encoding", "aura.lang.std", EMBEDDED_ENCODING_AUC),
    ("Iter", "aura.lang.std", EMBEDDED_ITER_AUC),
    ("Json", "aura.lang.std", EMBEDDED_JSON_AUC),
    ("StringBuilder", "aura.lang.std", EMBEDDED_SB_AUC),
    ("TestHelper", "aura.lang.std", EMBEDDED_TEST_HELPER_AUC),
    ("Path", "aura.lang.std", EMBEDDED_PATH_AUC),
    ("String", "aura.lang.std", EMBEDDED_STRING_AUC),
    // ── 并发同步原语（aura.lang.concurrent，纯 Aura）──
    ("Atomic", PKG_CONCURRENT, EMBEDDED_ATOMIC_AUC),
    ("Mutex", PKG_CONCURRENT, EMBEDDED_MUTEX_AUC),
    ("RwLock", PKG_CONCURRENT, EMBEDDED_RWLOCK_AUC),
    ("Condvar", PKG_CONCURRENT, EMBEDDED_CONDVAR_AUC),
    ("Barrier", PKG_CONCURRENT, EMBEDDED_BARRIER_AUC),
    ("Semaphore", PKG_CONCURRENT, EMBEDDED_SEMAPHORE_AUC),
    ("Thread", PKG_CONCURRENT, EMBEDDED_THREAD_AUC),
    ("Future", PKG_CONCURRENT, EMBEDDED_FUTURE_AUC),
    // ── native 辅助模块（纯 Aura，为 std/ 包提供底层支持）──
    ("MathOps", PKG_NATIVE_MATH, EMBEDDED_MATH_OPS_AUC),
    ("Stdio", PKG_NATIVE_IO, EMBEDDED_STDIO_AUC),
    ("EnvOps", PKG_NATIVE_ENV, EMBEDDED_ENV_OPS_AUC),
    ("FileOps", PKG_NATIVE_FILE, EMBEDDED_FILE_OPS_AUC),
    ("Console", PKG_NATIVE_CONSOLE, EMBEDDED_CONSOLE_AUC),
    ("Clock", PKG_NATIVE_TIME, EMBEDDED_CLOCK_AUC),
    ("NetworkOps", PKG_NATIVE_NETWORK, EMBEDDED_NETWORK_OPS_AUC),
];

/// 标准库模块总数
pub const EMBEDDED_STDLIB_COUNT: usize = EMBEDDED_STDLIB_MODULES.len();
