//! Phase 3: 嵌入式标准库（预编译 .auc 嵌入二进制）
//!
//! `aura/core/aura/lang/**/*.aura` 是唯一真相源，构建时预编译为 .auc 并嵌入
//! 二进制，VM 启动时自动加载，优先执行嵌入版本（而非 Rust native 或字节码解释）。
//!
//! 编译（**两步，顺序不能颠倒**）：
//! ```text
//! aura stdlib-compile aura/core --output rust/build/aura_core_auc
//! cargo build -p cli --features llvm
//! ```
//! 第二条 `cargo build` 不能省：`.auc` 是通过 `include_bytes!` 编进二进制的，
//! 只跑 `stdlib-compile` 不会刷新运行期看到的副本。
//!
//! 输出**只此一处**：`rust/build/aura_core_auc/`，且目录结构镜像 `aura/core/`
//! （见下方「嵌入路径约定」）。一次编译整棵 core 树即可覆盖全部嵌入模块。
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
//!   rust/build/aura_core_auc/         — 预编译 .auc（目录镜像 aura/core/）

// ── 嵌入路径约定 ──
//
// `.auc` 字节码**统一收拢**在 `rust/build/aura_core_auc/` 下，目录结构**镜像
// `aura/core/`** 的源码路径（去掉 `aura/core/` 前缀，`.aura` → `.auc`）：
//
//   aura/core/aura/lang/String.aura            → aura_core_auc/aura/lang/String.auc
//   aura/core/aura/lang/std/Math.aura          → aura_core_auc/aura/lang/std/Math.auc
//   aura/core/aura/lang/std/collection/Collections.aura
//                                             → aura_core_auc/aura/lang/std/collection/Collections.auc
//   aura/core/aura/lang/native/io/Stdio.aura   → aura_core_auc/aura/lang/native/io/Stdio.auc
//
// 因此生成只需**一条命令**（一次编译整棵 core 树，输出自带镜像目录层级）：
//
//   aura stdlib-compile aura/core --output rust/build/aura_core_auc
//
// 之后 `cargo build` 不能省：`.auc` 通过 `core_auc!`（= `include_bytes!`）
// 编进二进制，只跑 `stdlib-compile` 不会刷新运行期看到的副本。

/// 嵌入 `rust/build/aura_core_auc/<相对路径>` 下的 `.auc`。
macro_rules! core_auc {
    ($rel:literal) => {
        include_bytes!(concat!("../../../build/aura_core_auc/", $rel))
    };
}

/// 嵌入的 Math 标准库 — 纯逻辑
pub static EMBEDDED_MATH_AUC: &[u8] = core_auc!("aura/lang/std/Math.auc");

/// 嵌入的 Time 标准库 — 纯逻辑
pub static EMBEDDED_TIME_AUC: &[u8] = core_auc!("aura/lang/std/Time.auc");

/// 嵌入的 Collections 标准库 — 纯逻辑
pub static EMBEDDED_COLLECTIONS_AUC: &[u8] =
    core_auc!("aura/lang/std/collection/Collections.auc");

/// 嵌入的 Test 标准库 — 纯逻辑
pub static EMBEDDED_TEST_AUC: &[u8] = core_auc!("aura/lang/std/Test.auc");

/// 嵌入的 Ascii 标准库 — 纯逻辑
pub static EMBEDDED_ASCII_AUC: &[u8] = core_auc!("aura/lang/std/Ascii.auc");

/// 嵌入的 Assert 标准库 — 纯逻辑
pub static EMBEDDED_ASSERT_AUC: &[u8] = core_auc!("aura/lang/std/Assert.auc");

/// 嵌入的 Encoding 标准库 — 纯逻辑（Base64 / Hex）
pub static EMBEDDED_ENCODING_AUC: &[u8] =
    core_auc!("aura/lang/std/Encoding.auc");

/// 嵌入的 Iter 标准库 — 纯逻辑（函数式工具）
pub static EMBEDDED_ITER_AUC: &[u8] = core_auc!("aura/lang/std/Iter.auc");

/// 嵌入的 Json 标准库 — 纯逻辑（JSON parse/stringify）
pub static EMBEDDED_JSON_AUC: &[u8] = core_auc!("aura/lang/std/Json.auc");

/// 嵌入的 StringBuilder 标准库 — 纯逻辑
pub static EMBEDDED_SB_AUC: &[u8] = core_auc!("aura/lang/std/string/StringBuilder.auc");

/// 嵌入的 TestHelper 标准库 — 纯逻辑
pub static EMBEDDED_TEST_HELPER_AUC: &[u8] =
    core_auc!("aura/lang/std/TestHelper.auc");

/// 嵌入的 Path 标准库 — 纯逻辑（路径操作）
pub static EMBEDDED_PATH_AUC: &[u8] = core_auc!("aura/lang/std/Path.auc");

/// 嵌入的 String 标准库 — 纯逻辑
pub static EMBEDDED_STRING_AUC: &[u8] = core_auc!("aura/lang/String.auc");

// ── 由 Rust 内置实现迁移而来的纯 Aura 模块 ──
//
// 这三个模块原先只有 Rust 实现（`std/random.rs` / `std/fs.rs` / `std/process.rs`），
// 现按「内置类必须由 Aura 代码实现」的约定改写为纯 Aura
// （`aura/core/aura/lang/std/{Random,File,Process}.aura`），并在
// `stdlib-compile` 时与其它 std 模块一起预编译嵌入。

/// 嵌入的 Random 标准库 — 纯逻辑（XorShift64 有状态实例）
pub static EMBEDDED_RANDOM_AUC: &[u8] = core_auc!("aura/lang/std/Random.auc");

/// 嵌入的 File 标准库 — 纯逻辑（基于 FileOps 的文件读写实例）
pub static EMBEDDED_FILE_AUC: &[u8] = core_auc!("aura/lang/std/File.auc");

/// 嵌入的 Process 标准库 — 纯逻辑（ProcessNative 的薄封装）
pub static EMBEDDED_PROCESS_AUC: &[u8] =
    core_auc!("aura/lang/std/Process.auc");

/// 嵌入的 Exception 异常类层次结构 — 纯逻辑（新包结构 aura.lang.errors）
pub static EMBEDDED_ERRORS_THROWABLE_AUC: &[u8] =
    core_auc!("aura/lang/errors/Throwable.auc");
pub static EMBEDDED_ERRORS_ERROR_AUC: &[u8] =
    core_auc!("aura/lang/errors/Error.auc");
pub static EMBEDDED_ERRORS_EXCEPTION_AUC: &[u8] =
    core_auc!("aura/lang/errors/Exception.auc");
pub static EMBEDDED_ERRORS_IO_AUC: &[u8] =
    core_auc!("aura/lang/errors/IOException.auc");

// ── aura.lang.native（native 原语辅助模块）──
//
// 这些模块是纯 Aura 实现，为 std/ 包中的模块提供底层数学/IO/网络等辅助功能。

/// 嵌入的 MathOps（纯 Aura 数学运算）
pub static EMBEDDED_MATH_OPS_AUC: &[u8] =
    core_auc!("aura/lang/native/math/MathOps.auc");

/// 嵌入的 Stdio（纯 Aura IO 操作）
pub static EMBEDDED_STDIO_AUC: &[u8] = core_auc!("aura/lang/native/io/Stdio.auc");

/// 嵌入的 EnvOps（纯 Aura 环境变量操作）
pub static EMBEDDED_ENV_OPS_AUC: &[u8] =
    core_auc!("aura/lang/native/env/EnvOps.auc");

/// 嵌入的 FileOps（纯 Aura 文件操作）
pub static EMBEDDED_FILE_OPS_AUC: &[u8] =
    core_auc!("aura/lang/native/file/FileOps.auc");

/// 嵌入的 Console（纯 Aura 控制台操作）
pub static EMBEDDED_CONSOLE_AUC: &[u8] =
    core_auc!("aura/lang/native/console/Console.auc");

/// 嵌入的 Clock（纯 Aura 时钟操作）
pub static EMBEDDED_CLOCK_AUC: &[u8] = core_auc!("aura/lang/native/time/Clock.auc");

/// 嵌入的 NetworkOps（纯 Aura 网络操作）
pub static EMBEDDED_NETWORK_OPS_AUC: &[u8] =
    core_auc!("aura/lang/native/network/NetworkOps.auc");

// ── aura.lang.concurrent（纯 Aura 同步原语）──
//
// 这些模块是纯 Aura 实现，底层通过 native 包（`aura.lang.native.Memory` /
// `aura.lang.native.Cpu`）调用内存分配与内联汇编原子指令。加载器
// `load_embedded_stdlib` 会在合并时重定位常量池 / 原生表 / 函数表下标
// （`remap_embedded_instrs`），因此它们可以安全地被嵌入并优先派发。

/// 嵌入的 Atomic（AOT 机器码）— 纯 Aura
pub static EMBEDDED_ATOMIC_AUC: &[u8] = core_auc!("aura/lang/concurrent/Atomic.auc");

/// 嵌入的 Mutex（AOT 机器码）— 纯 Aura（自旋锁）
pub static EMBEDDED_MUTEX_AUC: &[u8] = core_auc!("aura/lang/concurrent/Mutex.auc");

/// 嵌入的 RwLock（AOT 机器码）— 纯 Aura（自旋锁）
pub static EMBEDDED_RWLOCK_AUC: &[u8] = core_auc!("aura/lang/concurrent/RwLock.auc");

/// 嵌入的 Condvar（AOT 机器码）— 纯 Aura
pub static EMBEDDED_CONDVAR_AUC: &[u8] = core_auc!("aura/lang/concurrent/Condvar.auc");

/// 嵌入的 Barrier（AOT 机器码）— 纯 Aura
pub static EMBEDDED_BARRIER_AUC: &[u8] = core_auc!("aura/lang/concurrent/Barrier.auc");

/// 嵌入的 Semaphore（AOT 机器码）— 纯 Aura
pub static EMBEDDED_SEMAPHORE_AUC: &[u8] =
    core_auc!("aura/lang/concurrent/Semaphore.auc");

/// 嵌入的 Thread（纯 Aura，仅线程桥走 native `ThreadOps`）
pub static EMBEDDED_THREAD_AUC: &[u8] = core_auc!("aura/lang/concurrent/Thread.auc");

/// 嵌入的 Future（纯 Aura，结果槽 + 状态机）
pub static EMBEDDED_FUTURE_AUC: &[u8] = core_auc!("aura/lang/concurrent/Future.auc");

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
    // ── `Collections` **不嵌入**（重要）──
    //
    // `aura/lang/std/collection/Collections.aura` 的实现全部绑定 **Plan A 原生内存
    // 布局**（`Collections.listSize` → `Memory.read64(list)`，`listSet` →
    // `Memory.write64(items + idx*8, …)`），只对 AOT/自举世界的裸内存列表成立。
    // 而 VM 里 `Value::List` / `Value::Map` 是 Rust 侧对象，同一份实现对它们是
    // **错误布局**：实测 `l.size` 返回 0、`l.set(1, x)` 直接访问违规
    // （`0xC0000005`）。
    //
    // VM 的派发规则是「嵌入 Aura 实现优先于 Rust native」，一旦嵌入就会顶掉
    // `std_collections.rs` 里 53 个**面向 `Value::List`/`Value::Map` 的正确实现**
    // （`listSize` / `listGet` / `listSet` / `set` / `listOf` / `mapOf` …）。
    // 因此这里不嵌入，VM 一律走 Rust native；AOT 路径不受影响（它按源码编译，
    // 并把 `aura.lang.std.Collections.*` 映射到 C 运行库
    // `aura_lang_std_Collections_{set,listSet,listGet,listSize,…}`）。
    // 待集合模块改为「双表示兼容」的纯 Aura 实现后，再把本行打开。
    // ("Collections", "aura.lang.std", EMBEDDED_COLLECTIONS_AUC),
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
    // ── 由 Rust 内置迁移而来的纯 Aura 模块 ──
    ("Random", "aura.lang.std", EMBEDDED_RANDOM_AUC),
    ("File", "aura.lang.std", EMBEDDED_FILE_AUC),
    ("Process", "aura.lang.std", EMBEDDED_PROCESS_AUC),
    // ── 异常类层次结构（aura.lang.errors，纯 Aura）──
    (
        "Throwable",
        "aura.lang.errors",
        EMBEDDED_ERRORS_THROWABLE_AUC,
    ),
    ("Error", "aura.lang.errors", EMBEDDED_ERRORS_ERROR_AUC),
    (
        "Exception",
        "aura.lang.errors",
        EMBEDDED_ERRORS_EXCEPTION_AUC,
    ),
    ("IOException", "aura.lang.errors", EMBEDDED_ERRORS_IO_AUC),
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
