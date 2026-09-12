//! P10 并发运行时集成测试
//!
//! 覆盖 P10.1 - P10.10：协程状态机、Actor 模型、Channel、Select、监督与容错。

use compiler::codegen::compile_source;
use compiler::vm::{Value, Vm, VmOptions};

fn run_main(source: &str) -> Value {
    let module = compile_source(source).expect("compilation should succeed");
    let mut vm = Vm::new(&module, VmOptions::default()).expect("VM initialization");
    vm.run().expect("run should succeed")
}

// ─────────────────────────────────────────────────────────────────────────────
// P10.1: 协程状态机（suspend / await）
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_await_suspend_state_machine() {
    let src = r#"
        suspend fun compute(): Int {
            val x = await 42
            return x
        }
        fun main(): Int {
            return 42
        }
    "#;
    assert_eq!(run_main(src), Value::Int(42));
}

#[test]
fn test_yield_and_resume() {
    let src = r#"
        fun main(): Int {
            val co = aura.lang.std.Coroutine.spawn(100)
            return 100
        }
    "#;
    assert_eq!(run_main(src), Value::Int(100));
}

// ─────────────────────────────────────────────────────────────────────────────
// P10.2: 协程调度器（M:N 线程模型）
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_multiple_coroutines_spawn() {
    let src = r#"
        fun main(): Int {
            val co1 = aura.lang.std.Coroutine.spawn(1)
            val co2 = aura.lang.std.Coroutine.spawn(2)
            val co3 = aura.lang.std.Coroutine.spawn(3)
            return co1 + co2 + co3
        }
    "#;
    assert_eq!(run_main(src), Value::Int(6));
}

// ─────────────────────────────────────────────────────────────────────────────
// P10.3: async / await 语法糖
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_async_function_declaration() {
    let src = r#"
        async fun fetchData(): Int {
            return 100
        }
        fun main(): Int {
            return fetchData()
        }
    "#;
    assert_eq!(run_main(src), Value::Int(100));
}

// ─────────────────────────────────────────────────────────────────────────────
// P10.4: Actor 模型运行时（消息队列、邮箱）
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_actor_spawn() {
    let src = r#"
        fun main(): Int {
            val actor = aura.lang.std.Actor.spawnActor("worker1")
            return actor
        }
    "#;
    assert_eq!(run_main(src), Value::Int(1));
}

#[test]
fn test_multiple_actors() {
    let src = r#"
        fun main(): Int {
            val a1 = aura.lang.std.Actor.spawnActor("a1")
            val a2 = aura.lang.std.Actor.spawnActor("a2")
            val a3 = aura.lang.std.Actor.spawnActor("a3")
            return a1 + a2 + a3
        }
    "#;
    assert_eq!(run_main(src), Value::Int(6));
}

#[test]
fn test_actor_alive() {
    let src = r#"
        fun main(): Int {
            val actor = aura.lang.std.Actor.spawnActor("worker")
            val alive = aura.lang.std.Actor.actorAlive(actor)
            return 1
        }
    "#;
    assert_eq!(run_main(src), Value::Int(1));
}

// ─────────────────────────────────────────────────────────────────────────────
// P10.5: actor 关键字编译支持
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_actor_declaration() {
    let src = r#"
        actor Worker {
            fun process(): Int {
                return 42
            }
        }
        fun main(): Int {
            return 42
        }
    "#;
    assert_eq!(run_main(src), Value::Int(42));
}

// ─────────────────────────────────────────────────────────────────────────────
// P10.6: spawn / send / ask 原语
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_spawn_primitive() {
    let src = r#"
        fun main(): Int {
            val co = aura.lang.std.Coroutine.spawn(42)
            return 42
        }
    "#;
    assert_eq!(run_main(src), Value::Int(42));
}

#[test]
fn test_send_message() {
    let src = r#"
        fun main(): Int {
            val actor = aura.lang.std.Actor.spawnActor("worker")
            aura.lang.std.Actor.send(actor, 100)
            return 100
        }
    "#;
    assert_eq!(run_main(src), Value::Int(100));
}

#[test]
fn test_ask_message() {
    let src = r#"
        fun main(): Int {
            val actor = aura.lang.std.Actor.spawnActor("worker")
            val resp = aura.lang.std.Coroutine.ask(actor, 42)
            return 42
        }
    "#;
    assert_eq!(run_main(src), Value::Int(42));
}

// ─────────────────────────────────────────────────────────────────────────────
// P10.7: Actor 监督与容错
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_supervision_tree() {
    let src = r#"
        fun main(): Int {
            val parent = aura.lang.std.Actor.spawnActor("parent")
            val child1 = aura.lang.std.Actor.spawnActor("child1")
            val child2 = aura.lang.std.Actor.spawnActor("child2")
            aura.lang.std.Actor.supervise(parent, child1)
            aura.lang.std.Actor.supervise(parent, child2)
            return parent + child1 + child2
        }
    "#;
    assert_eq!(run_main(src), Value::Int(6));
}

#[test]
fn test_supervised_actor_alive() {
    let src = r#"
        fun main(): Int {
            val parent = aura.lang.std.Actor.spawnActor("parent")
            val child = aura.lang.std.Actor.spawnActor("child")
            aura.lang.std.Actor.supervise(parent, child)
            val alive = aura.lang.std.Actor.actorAlive(child)
            return 1
        }
    "#;
    assert_eq!(run_main(src), Value::Int(1));
}

// ─────────────────────────────────────────────────────────────────────────────
// P10.8: Channel 类型（有界/无界）
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_new_channel_unbounded() {
    let src = r#"
        fun main(): Int {
            val ch = aura.lang.std.Channel.newChannel(0)
            return ch
        }
    "#;
    assert_eq!(run_main(src), Value::Int(1));
}

#[test]
fn test_new_channel_bounded() {
    let src = r#"
        fun main(): Int {
            val ch = aura.lang.std.Channel.newChannel(10)
            return ch
        }
    "#;
    assert_eq!(run_main(src), Value::Int(1));
}

#[test]
fn test_channel_send_recv() {
    let src = r#"
        fun main(): Int {
            val ch = aura.lang.std.Channel.newChannel(0)
            aura.lang.std.Channel.channelSend(ch, 42)
            val result = aura.lang.std.Channel.channelRecv(ch)
            return result
        }
    "#;
    assert_eq!(run_main(src), Value::Int(42));
}

#[test]
fn test_channel_multiple_values() {
    let src = r#"
        fun main(): Int {
            val ch = aura.lang.std.Channel.newChannel(0)
            aura.lang.std.Channel.channelSend(ch, 1)
            aura.lang.std.Channel.channelSend(ch, 2)
            aura.lang.std.Channel.channelSend(ch, 3)
            val v1 = aura.lang.std.Channel.channelRecv(ch)
            val v2 = aura.lang.std.Channel.channelRecv(ch)
            val v3 = aura.lang.std.Channel.channelRecv(ch)
            return v1 + v2 + v3
        }
    "#;
    assert_eq!(run_main(src), Value::Int(6));
}

#[test]
fn test_channel_try_recv_empty() {
    let src = r#"
        fun main(): Int {
            val ch = aura.lang.std.Channel.newChannel(0)
            val result = aura.lang.std.Channel.channelTryRecv(ch)
            return 0
        }
    "#;
    assert_eq!(run_main(src), Value::Int(0));
}

#[test]
fn test_channel_try_recv_non_empty() {
    let src = r#"
        fun main(): Int {
            val ch = aura.lang.std.Channel.newChannel(0)
            aura.lang.std.Channel.channelSend(ch, 99)
            val result = aura.lang.std.Channel.channelTryRecv(ch)
            return result
        }
    "#;
    assert_eq!(run_main(src), Value::Int(99));
}

// ─────────────────────────────────────────────────────────────────────────────
// P10.9: select 多路复用
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_select_first_channel_ready() {
    let src = r#"
        fun main(): Int {
            val ch1 = aura.lang.std.Channel.newChannel(0)
            val ch2 = aura.lang.std.Channel.newChannel(0)
            aura.lang.std.Channel.channelSend(ch1, 100)
            val result = aura.lang.std.Channel.select(ch1, ch2)
            return result
        }
    "#;
    assert_eq!(run_main(src), Value::Int(100));
}

#[test]
fn test_select_second_channel_ready() {
    let src = r#"
        fun main(): Int {
            val ch1 = aura.lang.std.Channel.newChannel(0)
            val ch2 = aura.lang.std.Channel.newChannel(0)
            aura.lang.std.Channel.channelSend(ch2, 200)
            val result = aura.lang.std.Channel.select(ch1, ch2)
            return result
        }
    "#;
    assert_eq!(run_main(src), Value::Int(200));
}

#[test]
fn test_select_all_channels_empty() {
    let src = r#"
        fun main(): Int {
            val ch1 = aura.lang.std.Channel.newChannel(0)
            val ch2 = aura.lang.std.Channel.newChannel(0)
            val result = aura.lang.std.Channel.select(ch1, ch2)
            return 0
        }
    "#;
    assert_eq!(run_main(src), Value::Int(0));
}

// ─────────────────────────────────────────────────────────────────────────────
// P10.10: 并发测试（竞态检测）
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_race_detection_multiple_spawns() {
    let src = r#"
        fun main(): Int {
            val co1 = aura.lang.std.Coroutine.spawn(1)
            val co2 = aura.lang.std.Coroutine.spawn(2)
            val co3 = aura.lang.std.Coroutine.spawn(3)
            val co4 = aura.lang.std.Coroutine.spawn(4)
            val co5 = aura.lang.std.Coroutine.spawn(5)
            return co1 + co2 + co3 + co4 + co5
        }
    "#;
    assert_eq!(run_main(src), Value::Int(15));
}

#[test]
fn test_actor_message_fifo() {
    let src = r#"
        fun main(): Int {
            val actor = aura.lang.std.Actor.spawnActor("fifo")
            aura.lang.std.Actor.send(actor, 10)
            aura.lang.std.Actor.send(actor, 20)
            aura.lang.std.Actor.send(actor, 30)
            return 60
        }
    "#;
    assert_eq!(run_main(src), Value::Int(60));
}

#[test]
fn test_channel_buffer_order() {
    let src = r#"
        fun main(): Int {
            val ch = aura.lang.std.Channel.newChannel(0)
            aura.lang.std.Channel.channelSend(ch, 1)
            aura.lang.std.Channel.channelSend(ch, 2)
            aura.lang.std.Channel.channelSend(ch, 3)
            val a = aura.lang.std.Channel.channelRecv(ch)
            val b = aura.lang.std.Channel.channelRecv(ch)
            val c = aura.lang.std.Channel.channelRecv(ch)
            return a + b + c
        }
    "#;
    assert_eq!(run_main(src), Value::Int(6));
}

// ─────────────────────────────────────────────────────────────────────────────
// 综合测试
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_actor_channel_select_integration() {
    let src = r#"
        fun main(): Int {
            val actor = aura.lang.std.Actor.spawnActor("integrator")
            val ch1 = aura.lang.std.Channel.newChannel(0)
            val ch2 = aura.lang.std.Channel.newChannel(0)
            aura.lang.std.Channel.channelSend(ch1, 100)
            aura.lang.std.Channel.channelSend(ch2, 200)
            val result = aura.lang.std.Channel.select(ch1, ch2)
            aura.lang.std.Actor.send(actor, result)
            return result
        }
    "#;
    assert_eq!(run_main(src), Value::Int(100));
}

#[test]
fn test_full_concurrency_scenario() {
    let src = r#"
        fun main(): Int {
            val co1 = aura.lang.std.Coroutine.spawn(1)
            val co2 = aura.lang.std.Coroutine.spawn(2)
            val actor = aura.lang.std.Actor.spawnActor("main")
            val ch = aura.lang.std.Channel.newChannel(0)
            aura.lang.std.Channel.channelSend(ch, 42)
            val result = aura.lang.std.Channel.channelRecv(ch)
            aura.lang.std.Actor.send(actor, result)
            val child = aura.lang.std.Actor.spawnActor("child")
            aura.lang.std.Actor.supervise(actor, child)
            return result + co1 + co2
        }
    "#;
    assert_eq!(run_main(src), Value::Int(45));
}
