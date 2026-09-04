//! P10 并发运行时性能基准（P10.11）
//!
//! 测量协程调度、Actor 消息传递、Channel 操作、Select 多路复用的性能。
//! 运行：`cargo bench --bench p10_benchmarks`

use std::hint::black_box;
use std::time::Instant;

use aura_compiler::codegen::compile_source;
use aura_compiler::vm::{Value, Vm, VmOptions};

/// 协程创建与调度基准
fn bench_coroutine_spawn() {
    let src = r#"
        fun main(): Int {
            val co1 = aura.concurrent.spawn(1)
            val co2 = aura.concurrent.spawn(2)
            val co3 = aura.concurrent.spawn(3)
            val co4 = aura.concurrent.spawn(4)
            val co5 = aura.concurrent.spawn(5)
            return co1 + co2 + co3 + co4 + co5
        }
    "#;
    let module = compile_source(src).unwrap();
    let mut vm = Vm::new(&module, VmOptions::default()).unwrap();
    let start = Instant::now();
    for _ in 0..1000 {
        let result = vm.run().unwrap();
        assert_eq!(result, Value::Int(15));
        vm.reset_for_reuse();
    }
    let elapsed = start.elapsed();
    println!(
        "coroutine_spawn x1000: {:.3} ms/op",
        elapsed.as_secs_f64() * 1000.0 / 1000.0
    );
}

/// Actor 创建与消息传递基准
fn bench_actor_message_passing() {
    let src = r#"
        fun main(): Int {
            val actor = aura.concurrent.spawnActor("worker")
            aura.concurrent.send(actor, 1)
            aura.concurrent.send(actor, 2)
            aura.concurrent.send(actor, 3)
            aura.concurrent.send(actor, 4)
            aura.concurrent.send(actor, 5)
            return actor
        }
    "#;
    let module = compile_source(src).unwrap();
    let mut vm = Vm::new(&module, VmOptions::default()).unwrap();
    let start = Instant::now();
    for _ in 0..1000 {
        let result = vm.run().unwrap();
        assert!(result.as_int() > 0);
        vm.reset_for_reuse();
    }
    let elapsed = start.elapsed();
    println!(
        "actor_message_passing x1000: {:.3} ms/op",
        elapsed.as_secs_f64() * 1000.0 / 1000.0
    );
}

/// Channel 操作基准
fn bench_channel_operations() {
    let src = r#"
        fun main(): Int {
            val ch = aura.concurrent.newChannel(0)
            aura.concurrent.channelSend(ch, 1)
            aura.concurrent.channelSend(ch, 2)
            aura.concurrent.channelSend(ch, 3)
            aura.concurrent.channelSend(ch, 4)
            aura.concurrent.channelSend(ch, 5)
            val a = aura.concurrent.channelRecv(ch)
            val b = aura.concurrent.channelRecv(ch)
            val c = aura.concurrent.channelRecv(ch)
            val d = aura.concurrent.channelRecv(ch)
            val e = aura.concurrent.channelRecv(ch)
            return a + b + c + d + e
        }
    "#;
    let module = compile_source(src).unwrap();
    let mut vm = Vm::new(&module, VmOptions::default()).unwrap();
    let start = Instant::now();
    for _ in 0..1000 {
        let result = vm.run().unwrap();
        assert_eq!(result, Value::Int(15));
        vm.reset_for_reuse();
    }
    let elapsed = start.elapsed();
    println!(
        "channel_operations x1000: {:.3} ms/op",
        elapsed.as_secs_f64() * 1000.0 / 1000.0
    );
}

/// Select 多路复用基准
fn bench_select_multiplexing() {
    let src = r#"
        fun main(): Int {
            val ch1 = aura.concurrent.newChannel(0)
            val ch2 = aura.concurrent.newChannel(0)
            aura.concurrent.channelSend(ch1, 100)
            aura.concurrent.channelSend(ch2, 200)
            val r1 = aura.concurrent.select(ch1, ch2)
            return r1
        }
    "#;
    let module = compile_source(src).unwrap();
    let mut vm = Vm::new(&module, VmOptions::default()).unwrap();
    let start = Instant::now();
    for _ in 0..1000 {
        let result = vm.run().unwrap();
        assert_eq!(result, Value::Int(100));
        vm.reset_for_reuse();
    }
    let elapsed = start.elapsed();
    println!(
        "select_multiplexing x1000: {:.3} ms/op",
        elapsed.as_secs_f64() * 1000.0 / 1000.0
    );
}

/// 监督树基准
fn bench_supervision_tree() {
    let src = r#"
        fun main(): Int {
            val parent = aura.concurrent.spawnActor("parent")
            val c1 = aura.concurrent.spawnActor("c1")
            val c2 = aura.concurrent.spawnActor("c2")
            val c3 = aura.concurrent.spawnActor("c3")
            val c4 = aura.concurrent.spawnActor("c4")
            aura.concurrent.supervise(parent, c1)
            aura.concurrent.supervise(parent, c2)
            aura.concurrent.supervise(parent, c3)
            aura.concurrent.supervise(parent, c4)
            return parent
        }
    "#;
    let module = compile_source(src).unwrap();
    let mut vm = Vm::new(&module, VmOptions::default()).unwrap();
    let start = Instant::now();
    for _ in 0..1000 {
        let result = vm.run().unwrap();
        assert!(result.as_int() > 0);
        vm.reset_for_reuse();
    }
    let elapsed = start.elapsed();
    println!(
        "supervision_tree x1000: {:.3} ms/op",
        elapsed.as_secs_f64() * 1000.0 / 1000.0
    );
}

/// 综合场景基准
fn bench_integrated_scenario() {
    let src = r#"
        fun main(): Int {
            val actor = aura.concurrent.spawnActor("integrator")
            val ch1 = aura.concurrent.newChannel(0)
            val ch2 = aura.concurrent.newChannel(0)
            aura.concurrent.channelSend(ch1, 100)
            aura.concurrent.channelSend(ch2, 200)
            val result = aura.concurrent.select(ch1, ch2)
            aura.concurrent.send(actor, result)
            val co = aura.concurrent.spawn(5)
            return result + co
        }
    "#;
    let module = compile_source(src).unwrap();
    let mut vm = Vm::new(&module, VmOptions::default()).unwrap();
    let start = Instant::now();
    for _ in 0..1000 {
        let result = vm.run().unwrap();
        assert_eq!(result, Value::Int(105));
        vm.reset_for_reuse();
    }
    let elapsed = start.elapsed();
    println!(
        "integrated_scenario x1000: {:.3} ms/op",
        elapsed.as_secs_f64() * 1000.0 / 1000.0
    );
}

fn main() {
    bench_coroutine_spawn();
    bench_actor_message_passing();
    bench_channel_operations();
    bench_select_multiplexing();
    bench_supervision_tree();
    bench_integrated_scenario();
}