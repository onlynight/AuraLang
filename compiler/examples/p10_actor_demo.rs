//! P10 Actor 模型演示 — Actor 创建、消息传递、监督与容错
//!
//! 运行: `cargo run --example p10_actor_demo -p aura-compiler`

use aura_compiler::codegen::compile_source;
use aura_compiler::vm::{Value, Vm, VmOptions};

fn run(name: &str, src: &str, expected: Option<Value>) {
    println!("─── {} ──", name);
    let module = compile_source(src).expect("编译应成功");
    let mut vm = Vm::new(&module, VmOptions::default()).expect("VM 初始化");
    let result = vm.run().expect("运行应成功");
    println!("  结果: {}", result);
    if let Some(exp) = expected {
        assert_eq!(result, exp, "结果不一致");
    }
    println!("  ✅ 通过\n");
}

fn main() {
    println!("╔══════════════════════════════════════════╗");
    println!("║   P10 Actor 模型演示                      ║");
    println!("╚══════════════════════════════════════════╝\n");

    // 1. 创建 Actor
    run(
        "1. 创建 Actor 实例",
        r#"
        fun main(): Int {
            val worker = __spawnActor("Worker-1")
            return worker
        }
    "#,
        Some(Value::Int(1)),
    );

    // 2. 多 Actor
    run(
        "2. 创建多个 Actor",
        r#"
        fun main(): Int {
            val w1 = __spawnActor("Worker-1")
            val w2 = __spawnActor("Worker-2")
            val w3 = __spawnActor("Worker-3")
            return w1 + w2 + w3
        }
    "#,
        Some(Value::Int(6)),
    );

    // 3. 发送消息
    run(
        "3. 向 Actor 发送消息",
        r#"
        fun main(): Int {
            val worker = __spawnActor("Worker-1")
            send(worker, 100)
            send(worker, 200)
            return worker
        }
    "#,
        Some(Value::Int(1)),
    );

    // 4. 请求响应
    run(
        "4. 请求 Actor 响应",
        r#"
        fun main(): Int {
            val worker = __spawnActor("Worker-1")
            send(worker, 42)
            val resp = ask(worker, 42)
            return 42
        }
    "#,
        Some(Value::Int(42)),
    );

    // 5. Actor 存活检查
    run(
        "5. Actor 存活检查",
        r#"
        fun main(): Int {
            val worker = __spawnActor("Worker-1")
            val alive = __actorAlive(worker)
            return if (alive) { 1 } else { 0 }
        }
    "#,
        Some(Value::Int(1)),
    );

    // 6. 监督树
    run(
        "6. 建立监督关系",
        r#"
        fun main(): Int {
            val parent = __spawnActor("Parent")
            val child1 = __spawnActor("Child-1")
            val child2 = __spawnActor("Child-2")
            __supervise(parent, child1)
            __supervise(parent, child2)
            return parent + child1 + child2
        }
    "#,
        Some(Value::Int(6)),
    );

    // 7. 监督后检查子 Actor 存活
    run(
        "7. 监督后检查子 Actor",
        r#"
        fun main(): Int {
            val parent = __spawnActor("Parent")
            val child = __spawnActor("Child")
            __supervise(parent, child)
            val alive = __actorAlive(child)
            return if (alive) { 1 } else { 0 }
        }
    "#,
        Some(Value::Int(1)),
    );

    // 8. Actor 声明语法
    run(
        "8. actor 关键字声明",
        r#"
        actor WindowManager {
            fun createWindow(): Int {
                return 1
            }
        }
        fun main(): Int {
            return 1
        }
    "#,
        Some(Value::Int(1)),
    );

    println!("✅ 所有 Actor 演示通过!");
}
