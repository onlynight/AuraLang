//! [Phase 3] FFI AOT direct call support.
//!
//! Build-time configuration for FFI calls across the three execution
//! modes (VM / JIT / AOT). This module is the loom-side counterpart to
//! the compiler's FFI AOT support
//! (see `compiler/src/codegen/ffi_aot.rs`).
//!
//! ## Sub-modules
//!
//! * [`aot`]: AOT direct call configuration - decides which target mode
//!   and how the inline cache / PLT strategies are wired in.
//! * [`cache`]: FFI call cache configuration - controls preload,
//!   inline cache, and hotspot threshold per mode.
//! * [`optimize`]: FFI optimization configuration - resolves the
//!   effective optimization settings (inline, PLT, LLVM).

pub mod aot;
pub mod cache;
pub mod optimize;
