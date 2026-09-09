//! Bootstrap 最小引导层（完全 Aura 化 Phase 1）
//!
//! 本模块是「不能上移」的 Rust 最小核心，具备以下特征：
//! - **独立**：不依赖 compiler 的 vm/codegen/sema 等任何其他模块，仅依赖 std；
//! - **三态**：为 VM / JIT / AOT 三种执行模式提供统一基础；
//! - **AOT 直连**：FFI 调用在启动时预加载函数地址并绑定调用点，
//!   消除每次调用的函数指针查找（VM）与间接调用（JIT/AOT）。
//!
//! 文件布局（对应《完全Aura化技术方案.md》Phase 1）：
//! - [`vm_core`]    最小虚拟机核心：字节码、解释器、栈帧、异常（Trap）、FFI 直连缓存
//! - [`jit_core`]   JIT 核心：热点检测、基线编译、内联缓存、去优化
//! - [`aot_core`]   AOT 核心：LLVM IR 直发（直接调用 C 符号）、内联优化、死代码消除
//! - [`any_core`]   Any 核心虚方法：toString / equals / hashCode
//! - [`type_core`]  类型内省核心：typeOf / isOfType / cast
//! - [`value_check`] 空值/数值检查：isNull / isZero / isNaN / isInfinite ...
//! - [`memory`]     内存管理：malloc / free / arc / string_*
//! - [`runtime`]    运行时：协程（coroutine_yield）与最小 GC（mark-sweep）

pub mod any_core;
pub mod aot_core;
pub mod jit_core;
pub mod memory;
pub mod runtime;
pub mod type_core;
pub mod value_check;
pub mod vm_core;

pub use aot_core::{AotConfig, AotGenerator};
pub use jit_core::{Deopt, JitCompiler, JitConfig, JitUnit};
pub use runtime::{Coroutine, CoroutineState, GcHeap, GcStats};
pub use vm_core::{BytecodeModule, FfiCache, FfiEntry, FuncDef, Insn, Step, Value, Vm};

/// 执行模式（三态：VM / JIT / AOT）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExecutionMode {
    /// 解释执行（开发调试）
    Vm,
    /// 热点即时编译（桌面/服务器应用）
    Jit,
    /// 提前编译为 LLVM IR / 机器码（高性能计算）
    Aot,
}

/// FFI 模式（默认 AOT 直连）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FfiMode {
    /// AOT 直连：预加载函数地址，直接调用（默认）
    Aot,
    /// 传统 C FFI：按名查找函数指针，间接调用（降级方案）
    Cffi,
    /// Rust FFI：通过注册表调用 Layer 1 原生函数
    RustFfi,
}

/// Bootstrap 层统一错误类型（异常处理的载体）。
///
/// VM 解释执行、JIT 去优化回退到解释器后，运行时错误统一以
/// [`Trap`] 形式沿调用链向上传播（结构化异常传播）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trap {
    pub message: String,
}

impl Trap {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for Trap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "bootstrap trap: {}", self.message)
    }
}

impl std::error::Error for Trap {}
