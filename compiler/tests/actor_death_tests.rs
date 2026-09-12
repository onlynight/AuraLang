#![cfg(feature = "llvm")]

//! Fix 12 — Actor 死亡传播测试
//!
//! 验证死亡策略（Terminate/Escalate/Restart）的正确传播。

use compiler::vm::actor::{ActorRuntime, DeathStrategy};
use compiler::vm::value::Value;

// ─────────────────────────────────────────────────────────────────────────────
// 1. Terminate 策略：子 Actor 全部终止
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_terminate_propagates_to_children() {
    let mut runtime = ActorRuntime::new();
    let parent_id = runtime.spawn("parent");
    let child1_id = runtime.spawn("child1");
    let child2_id = runtime.spawn("child2");

    runtime.supervise(parent_id, child1_id);
    runtime.supervise(parent_id, child2_id);

    // 设置 Terminate 策略
    runtime.set_death_strategy(parent_id, DeathStrategy::Terminate);

    // 杀死父 Actor
    runtime.kill(parent_id);

    assert!(!runtime.is_alive(parent_id));
    // 子 Actor 应被终止
    assert!(!runtime.is_alive(child1_id), "child1 should be terminated");
    assert!(!runtime.is_alive(child2_id), "child2 should be terminated");
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Escalate 策略：通知父 Actor
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_escalate_notifies_parent() {
    let mut runtime = ActorRuntime::new();
    let parent_id = runtime.spawn("parent");
    let child_id = runtime.spawn("child");

    runtime.supervise(parent_id, child_id);

    // 设置 Escalate 策略
    runtime.set_death_strategy(child_id, DeathStrategy::Escalate);

    // 杀死子 Actor
    runtime.kill(child_id);

    assert!(!runtime.is_alive(child_id));
    assert!(runtime.is_alive(parent_id), "parent actor should be alive");

    // 父 Actor 邮箱应收到死亡消息
    assert!(
        runtime.mailbox_len(parent_id) > 0,
        "父 Actor 应收到死亡通知"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. Restart 策略：Actor 重启
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_restart_revives_actor() {
    let mut runtime = ActorRuntime::new();
    let id = runtime.spawn("worker");

    // 设置 Restart 策略
    runtime.set_death_strategy(id, DeathStrategy::Restart);

    // 发送消息到邮箱
    runtime.send(id, Value::Int(1));
    runtime.send(id, Value::Int(2));

    // 杀死（触发重启）
    runtime.kill(id);

    // Actor 应存活（已重启）
    assert!(runtime.is_alive(id), "actor should be alive after restart");
    // 邮箱应被清空
    assert_eq!(
        runtime.mailbox_len(id),
        0,
        "mailbox should be cleared after restart"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. 默认策略是 Terminate
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_default_strategy_is_terminate() {
    let mut runtime = ActorRuntime::new();
    let id = runtime.spawn("test");

    let strategy = runtime.get_death_strategy(id);
    assert_eq!(strategy, Some(&DeathStrategy::Terminate));
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. 死亡原因记录
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_death_reason_recorded() {
    let mut runtime = ActorRuntime::new();
    let id = runtime.spawn("test");

    runtime.kill_with_reason(id, Some("out of memory".to_string()));

    let reason = runtime.get_death_reason(id);
    assert_eq!(reason, Some(&"out of memory".to_string()));
}

#[test]
fn test_death_reason_none() {
    let mut runtime = ActorRuntime::new();
    let id = runtime.spawn("test");

    runtime.kill(id);

    // kill() 不记录原因
    assert_eq!(runtime.get_death_reason(id), None);
}

// ─────────────────────────────────────────────────────────────────────────────
// 6. 多层传播
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_multi_level_terminate() {
    let mut runtime = ActorRuntime::new();
    let root_id = runtime.spawn("root");
    let mid_id = runtime.spawn("mid");
    let leaf_id = runtime.spawn("leaf");

    runtime.supervise(root_id, mid_id);
    runtime.supervise(mid_id, leaf_id);

    runtime.set_death_strategy(root_id, DeathStrategy::Terminate);
    runtime.set_death_strategy(mid_id, DeathStrategy::Terminate);

    runtime.kill(root_id);

    assert!(!runtime.is_alive(root_id));
    assert!(!runtime.is_alive(mid_id), "mid should be terminated");
    assert!(!runtime.is_alive(leaf_id), "leaf should be terminated");
}
