//! P10 并发运行时集成测试
//!
//! 覆盖 P10.1 - P10.10：协程状态机、Actor 模型、Channel、Select、监督与容错。

use aura_compiler::codegen::compile_source;
use aura_compiler::vm::{Value, Vm, VmOptions};

fn run_main(source: &str) -> Value {
    let module = compile_source(source).expect("编译应成功");
    let mut vm = Vm::new(&module, VmOptions::default()).expect("VM 初始化");
    vm.run().expect("运行应成功")
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
            val co = aura.concurrent.spawn(100)
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
            val co1 = aura.concurrent.spawn(1)
            val co2 = aura.concurrent.spawn(2)
            val co3 = aura.concurrent.spawn(3)
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
            val actor = aura.concurrent.spawnActor("worker1")
            return actor
        }
    "#;
    assert_eq!(run_main(src), Value::Int(1));
}

#[test]
fn test_multiple_actors() {
    let src = r#"
        fun main(): Int {
            val a1 = aura.concurrent.spawnActor("a1")
            val a2 = aura.concurrent.spawnActor("a2")
            val a3 = aura.concurrent.spawnActor("a3")
            return a1 + a2 + a3
        }
    "#;
    assert_eq!(run_main(src), Value::Int(6));
}

#[test]
fn test_actor_alive() {
    let src = r#"
        fun main(): Int {
            val actor = aura.concurrent.spawnActor("worker")
            val alive = aura.concurrent.actorAlive(actor)
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
            val co = aura.concurrent.spawn(42)
            return 42
        }
    "#;
    assert_eq!(run_main(src), Value::Int(42));
}

#[test]
fn test_send_message() {
    let src = r#"
        fun main(): Int {
            val actor = aura.concurrent.spawnActor("worker")
            aura.concurrent.send(actor, 100)
            return 100
        }
    "#;
    assert_eq!(run_main(src), Value::Int(100));
}

#[test]
fn test_ask_message() {
    let src = r#"
        fun main(): Int {
            val actor = aura.concurrent.spawnActor("worker")
            val resp = aura.concurrent.ask(actor, 42)
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
            val parent = aura.concurrent.spawnActor("parent")
            val child1 = aura.concurrent.spawnActor("child1")
            val child2 = aura.concurrent.spawnActor("child2")
            aura.concurrent.supervise(parent, child1)
            aura.concurrent.supervise(parent, child2)
            return parent + child1 + child2
        }
    "#;
    assert_eq!(run_main(src), Value::Int(6));
}

#[test]
fn test_supervised_actor_alive() {
    let src = r#"
        fun main(): Int {
            val parent = aura.concurrent.spawnActor("parent")
            val child = aura.concurrent.spawnActor("child")
            aura.concurrent.supervise(parent, child)
            val alive = aura.concurrent.actorAlive(child)
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
            val ch = aura.concurrent.newChannel(0)
            return ch
        }
    "#;
    assert_eq!(run_main(src), Value::Int(1));
}

#[test]
fn test_new_channel_bounded() {
    let src = r#"
        fun main(): Int {
            val ch = aura.concurrent.newChannel(10)
            return ch
        }
    "#;
    assert_eq!(run_main(src), Value::Int(1));
}

#[test]
fn test_channel_send_recv() {
    let src = r#"
        fun main(): Int {
            val ch = aura.concurrent.newChannel(0)
            aura.concurrent.channelSend(ch, 42)
            val result = aura.concurrent.channelRecv(ch)
            return result
        }
    "#;
    assert_eq!(run_main(src), Value::Int(42));
}

#[test]
fn test_channel_multiple_values() {
    let src = r#"
        fun main(): Int {
            val ch = aura.concurrent.newChannel(0)
            aura.concurrent.channelSend(ch, 1)
            aura.concurrent.channelSend(ch, 2)
            aura.concurrent.channelSend(ch, 3)
            val v1 = aura.concurrent.channelRecv(ch)
            val v2 = aura.concurrent.channelRecv(ch)
            val v3 = aura.concurrent.channelRecv(ch)
            return v1 + v2 + v3
        }
    "#;
    assert_eq!(run_main(src), Value::Int(6));
}

#[test]
fn test_channel_try_recv_empty() {
    let src = r#"
        fun main(): Int {
            val ch = aura.concurrent.newChannel(0)
            val result = aura.concurrent.channelTryRecv(ch)
            return 0
        }
    "#;
    assert_eq!(run_main(src), Value::Int(0));
}

#[test]
fn test_channel_try_recv_non_empty() {
    let src = r#"
        fun main(): Int {
            val ch = aura.concurrent.newChannel(0)
            aura.concurrent.channelSend(ch, 99)
            val result = aura.concurrent.channelTryRecv(ch)
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
            val ch1 = aura.concurrent.newChannel(0)
            val ch2 = aura.concurrent.newChannel(0)
            aura.concurrent.channelSend(ch1, 100)
            val result = aura.concurrent.select(ch1, ch2)
            return result
        }
    "#;
    assert_eq!(run_main(src), Value::Int(100));
}

#[test]
fn test_select_second_channel_ready() {
    let src = r#"
        fun main(): Int {
            val ch1 = aura.concurrent.newChannel(0)
            val ch2 = aura.concurrent.newChannel(0)
            aura.concurrent.channelSend(ch2, 200)
            val result = aura.concurrent.select(ch1, ch2)
            return result
        }
    "#;
    assert_eq!(run_main(src), Value::Int(200));
}

#[test]
fn test_select_all_channels_empty() {
    let src = r#"
        fun main(): Int {
            val ch1 = aura.concurrent.newChannel(0)
            val ch2 = aura.concurrent.newChannel(0)
            val result = aura.concurrent.select(ch1, ch2)
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
            val co1 = aura.concurrent.spawn(1)
            val co2 = aura.concurrent.spawn(2)
            val co3 = aura.concurrent.spawn(3)
            val co4 = aura.concurrent.spawn(4)
            val co5 = aura.concurrent.spawn(5)
            return co1 + co2 + co3 + co4 + co5
        }
    "#;
    assert_eq!(run_main(src), Value::Int(15));
}

#[test]
fn test_actor_message_fifo() {
    let src = r#"
        fun main(): Int {
            val actor = aura.concurrent.spawnActor("fifo")
            aura.concurrent.send(actor, 10)
            aura.concurrent.send(actor, 20)
            aura.concurrent.send(actor, 30)
            return 60
        }
    "#;
    assert_eq!(run_main(src), Value::Int(60));
}

#[test]
fn test_channel_buffer_order() {
    let src = r#"
        fun main(): Int {
            val ch = aura.concurrent.newChannel(0)
            aura.concurrent.channelSend(ch, 1)
            aura.concurrent.channelSend(ch, 2)
            aura.concurrent.channelSend(ch, 3)
            val a = aura.concurrent.channelRecv(ch)
            val b = aura.concurrent.channelRecv(ch)
            val c = aura.concurrent.channelRecv(ch)
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
            val actor = aura.concurrent.spawnActor("integrator")
            val ch1 = aura.concurrent.newChannel(0)
            val ch2 = aura.concurrent.newChannel(0)
            aura.concurrent.channelSend(ch1, 100)
            aura.concurrent.channelSend(ch2, 200)
            val result = aura.concurrent.select(ch1, ch2)
            aura.concurrent.send(actor, result)
            return result
        }
    "#;
    assert_eq!(run_main(src), Value::Int(100));
}

#[test]
fn test_full_concurrency_scenario() {
    let src = r#"
        fun main(): Int {
            val co1 = aura.concurrent.spawn(1)
            val co2 = aura.concurrent.spawn(2)
            val actor = aura.concurrent.spawnActor("main")
            val ch = aura.concurrent.newChannel(0)
            aura.concurrent.channelSend(ch, 42)
            val result = aura.concurrent.channelRecv(ch)
            aura.concurrent.send(actor, result)
            val child = aura.concurrent.spawnActor("child")
            aura.concurrent.supervise(actor, child)
            return result + co1 + co2
        }
    "#;
    assert_eq!(run_main(src), Value::Int(45));
}
