//! Phase D — JIT 并发支持测试
//!
//! 验证 JIT 后端支持并发指令。
//! 测试重点验证并发指令白名单和名称调度器。

#![cfg(feature = "jit")]

use compiler::codegen::opcode::Const;
use compiler::vm::jit::is_jit_compilable;
use compiler::vm::{DecodedFunction, Instr};

// ─────────────────────────────────────────────────────────────────────────────
// 辅助：构建测试函数
// ─────────────────────────────────────────────────────────────────────────────

fn make_func(instrs: Vec<Instr>) -> DecodedFunction {
    DecodedFunction {
        name: String::new(),
        param_count: 0,
        locals: 4,
        is_native: false,
        code: instrs,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. 并发指令 JIT 白名单测试
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_jit_whitelist_thread_instructions() {
    let consts: &[Const] = &[];
    let funcs: Vec<DecodedFunction> = vec![
        make_func(vec![
            Instr::ThreadId,
            Instr::Return,
        ]),
        make_func(vec![
            Instr::ThreadParallelism,
            Instr::Return,
        ]),
        make_func(vec![
            Instr::ThreadSleep,
            Instr::Return,
        ]),
    ];

    for (i, f) in funcs.iter().enumerate() {
        assert!(
            is_jit_compilable(i, f, consts, &funcs),
            "Thread instruction {} should be JIT-compilable",
            i
        );
    }
}

#[test]
fn test_jit_whitelist_mutex_instructions() {
    let consts: &[Const] = &[];
    let funcs: Vec<DecodedFunction> = vec![
        make_func(vec![
            Instr::MutexNew,
            Instr::Return,
        ]),
        make_func(vec![
            Instr::MutexLock,
            Instr::Return,
        ]),
        make_func(vec![
            Instr::MutexUnlock,
            Instr::Return,
        ]),
        make_func(vec![
            Instr::MutexTryLock,
            Instr::Return,
        ]),
    ];

    for (i, f) in funcs.iter().enumerate() {
        assert!(
            is_jit_compilable(i, f, consts, &funcs),
            "Mutex instruction {} should be JIT-compilable",
            i
        );
    }
}

#[test]
fn test_jit_whitelist_atomic_instructions() {
    let consts: &[Const] = &[];
    let funcs: Vec<DecodedFunction> = vec![
        make_func(vec![
            Instr::AtomicNew,
            Instr::Return,
        ]),
        make_func(vec![
            Instr::AtomicLoad,
            Instr::Return,
        ]),
        make_func(vec![
            Instr::AtomicStore,
            Instr::Return,
        ]),
        make_func(vec![
            Instr::AtomicAdd,
            Instr::Return,
        ]),
        make_func(vec![
            Instr::AtomicCas,
            Instr::Return,
        ]),
    ];

    for (i, f) in funcs.iter().enumerate() {
        assert!(
            is_jit_compilable(i, f, consts, &funcs),
            "Atomic instruction {} should be JIT-compilable",
            i
        );
    }
}

#[test]
fn test_jit_whitelist_rwlock_instructions() {
    let consts: &[Const] = &[];
    let funcs: Vec<DecodedFunction> = vec![
        make_func(vec![
            Instr::RwLockNew,
            Instr::Return,
        ]),
        make_func(vec![
            Instr::RwLockReadLock,
            Instr::Return,
        ]),
        make_func(vec![
            Instr::RwLockWriteLock,
            Instr::Return,
        ]),
        make_func(vec![
            Instr::RwLockReadUnlock,
            Instr::Return,
        ]),
        make_func(vec![
            Instr::RwLockWriteUnlock,
            Instr::Return,
        ]),
    ];

    for (i, f) in funcs.iter().enumerate() {
        assert!(
            is_jit_compilable(i, f, consts, &funcs),
            "RwLock instruction {} should be JIT-compilable",
            i
        );
    }
}

#[test]
fn test_jit_whitelist_channel_instructions() {
    let consts: &[Const] = &[];
    let funcs: Vec<DecodedFunction> = vec![
        make_func(vec![
            Instr::ChannelNew,
            Instr::Return,
        ]),
        make_func(vec![
            Instr::ChannelSend,
            Instr::Return,
        ]),
        make_func(vec![
            Instr::ChannelRecv,
            Instr::Return,
        ]),
    ];

    for (i, f) in funcs.iter().enumerate() {
        assert!(
            is_jit_compilable(i, f, consts, &funcs),
            "Channel instruction {} should be JIT-compilable",
            i
        );
    }
}

#[test]
fn test_jit_whitelist_condvar_instructions() {
    let consts: &[Const] = &[];
    let funcs: Vec<DecodedFunction> = vec![
        make_func(vec![
            Instr::CondvarNew,
            Instr::Return,
        ]),
        make_func(vec![
            Instr::CondvarWait,
            Instr::Return,
        ]),
        make_func(vec![
            Instr::CondvarSignal,
            Instr::Return,
        ]),
        make_func(vec![
            Instr::CondvarBroadcast,
            Instr::Return,
        ]),
    ];

    for (i, f) in funcs.iter().enumerate() {
        assert!(
            is_jit_compilable(i, f, consts, &funcs),
            "Condvar instruction {} should be JIT-compilable",
            i
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. JIT 名称调度器注册测试
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_jit_name_based_dispatcher_registered() {
    // 验证并发函数可以通过名称查找
    let mut reg = compiler::vm::native::NativeRegistry::new();
    compiler::vm::concurrent_native::register_all(&mut reg);

    let expected_functions = [
        "aura.lang.concurrent.Thread.id",
        "aura.lang.concurrent.Thread.parallelism",
        "aura.lang.concurrent.Thread.sleep",
        "aura.lang.concurrent.Thread.spawn",
        "aura.lang.concurrent.Thread.join",
        "aura.lang.concurrent.Mutex.new",
        "aura.lang.concurrent.Mutex.lock",
        "aura.lang.concurrent.Mutex.unlock",
        "aura.lang.concurrent.Mutex.tryLock",
        "aura.lang.concurrent.Atomic.new",
        "aura.lang.concurrent.Atomic.load",
        "aura.lang.concurrent.Atomic.store",
        "aura.lang.concurrent.Atomic.add",
        "aura.lang.concurrent.Atomic.sub",
        "aura.lang.concurrent.Atomic.cas",
        "aura.lang.concurrent.RwLock.new",
        "aura.lang.concurrent.RwLock.readLock",
        "aura.lang.concurrent.RwLock.writeLock",
        "aura.lang.concurrent.RwLock.readUnlock",
        "aura.lang.concurrent.RwLock.writeUnlock",
        "aura.lang.concurrent.Channel.newChannel",
        "aura.lang.concurrent.Channel.channelSend",
        "aura.lang.concurrent.Channel.channelRecv",
        "aura.lang.concurrent.Condvar.new",
        "aura.lang.concurrent.Condvar.wait",
        "aura.lang.concurrent.Condvar.signal",
        "aura.lang.concurrent.Condvar.broadcast",
    ];

    for name in &expected_functions {
        assert!(
            reg.get(name).is_some(),
            "Function '{}' should be registered for JIT name dispatch",
            name
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. JIT 并发指令混合测试
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_jit_mixed_concurrent_and_arithmetic() {
    let consts: &[Const] = &[Const::Int(10)];
    let funcs: Vec<DecodedFunction> = vec![make_func(
        vec![
            Instr::LoadConst(0),
            Instr::ThreadId,
            Instr::Add,
            Instr::Return,
        ],
    )];

    assert!(
        is_jit_compilable(0, &funcs[0], consts, &funcs),
        "Mixed concurrent + arithmetic should be JIT-compilable"
    );
}
