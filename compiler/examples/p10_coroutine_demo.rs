//! P10 协程调度演示 — 协程创建、await 挂起、并发执行
//!
//! 运行: `cargo run --example p10_coroutine_demo -p compiler`

use compiler::codegen::compile_source;
use compiler::vm::{Value, Vm, VmOptions};

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
    println!("║   P10 协程调度演示                        ║");
    println!("╚══════════════════════════════════════════╝\n");

    // 1. 基础协程创建
    run(
        "1. 基础协程 spawn",
        r#"
        fun main(): Int {
            val co1 = spawn(42)
            return co1
        }
    "#,
        Some(Value::Int(42)),
    );

    // 2. 多协程并发
    run(
        "2. 多协程并发",
        r#"
        fun main(): Int {
            val co1 = spawn(10)
            val co2 = spawn(20)
            val co3 = spawn(30)
            return co1 + co2 + co3
        }
    "#,
        Some(Value::Int(60)),
    );

    // 3. await 挂起 — 在 suspend 函数中
    run(
        "3. await 挂起",
        r#"
        suspend fun compute(): Int {
            val x = await 100
            return x
        }
        fun main(): Int {
            return compute()
        }
    "#,
        Some(Value::Int(100)),
    );

    // 4. async 函数
    run(
        "4. async 函数",
        r#"
        async fun fetchData(url: String): Int {
            return 200
        }
        fun main(): Int {
            return fetchData("http://example.com")
        }
    "#,
        Some(Value::Int(200)),
    );

    // 5. 协程 + 累加器模式
    run(
        "5. 协程累加器",
        r#"
        fun main(): Int {
            var total = 0
            total = total + spawn(1)
            total = total + spawn(2)
            total = total + spawn(3)
            total = total + spawn(4)
            total = total + spawn(5)
            return total
        }
    "#,
        Some(Value::Int(15)),
    );

    println!("✅ 所有协程演示通过!");
}
