//! Actor 模型运行时（P10.4）
//!
//! 对应 技术方案 §3.7 的 Actor 模型：每个 Actor 拥有私有状态和消息队列，
//! 通过 `send`/`ask` 进行消息传递，支持监督与容错（P10.7）。
//!
//! 实现要点：
//! - Actor ID 从 1 开始（0 保留给主线程）
//! - 消息队列使用 `VecDeque` 保证 FIFO 顺序
//! - `ask` 精确阻塞等待响应（P8 事件驱动，零 CPU 空转）
//! - 监督树：父 Actor 可监督子 Actor，子 Actor 崩溃时通知父 Actor
//!
//! **P8 优化**：新增 `response_notifier`（EventNotifier），消除协程调度器轮询。
//! - `ask` 等待响应时 `notifier.wait()` 精确阻塞（零 CPU 消耗）
//! - `reply` 写 response_queue 后 `notify()` 唤醒等待者（零延迟）
//! - 无 Mutex、无 Condvar——保持 Aura 无锁隔离优势
//!
//! 对应文档：`docs/pure_aura_jit/jit模式优化方案.md` §7.3.7

use crate::vm::event_notifier::{EventNotifier, create_notifier};
use crate::vm::value::Value;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

/// Actor 实例 ID
pub type ActorId = usize;

/// 死亡策略（Fix 12）
#[derive(Debug, Clone, PartialEq)]
pub enum DeathStrategy {
    /// 重启 Actor（保留状态）
    Restart,
    /// 上报父 Actor（父决定是否处理）
    Escalate,
    /// 终止所有子 Actor
    Terminate,
}

/// Actor 实例
#[derive(Debug, Clone)]
pub struct Actor {
    pub id: ActorId,
    /// Actor 名称
    pub name: String,
    /// 消息队列（FIFO）
    pub mailbox: VecDeque<Value>,
    /// 私有状态（键值对）
    pub state: HashMap<String, Value>,
    /// 是否存活
    pub alive: bool,
    /// 父 Actor ID（用于监督）
    pub parent: Option<ActorId>,
    /// 子 Actor ID 列表
    pub children: Vec<ActorId>,
    /// 死亡策略（Fix 12）
    pub death_strategy: DeathStrategy,
    /// 死亡原因（Fix 12）
    pub death_reason: Option<String>,
}

/// Actor 运行时管理器
pub struct ActorRuntime {
    /// Actor ID → Actor 实例
    actors: Vec<Option<Actor>>,
    /// 下一个 Actor ID
    next_id: usize,
    /// 待处理请求（Phase 4: ask 真阻塞）
    pending_requests: HashMap<u64, PendingRequest>,
    /// 下一个请求 ID
    next_request_id: u64,
    /// 响应队列（无锁，单 VM 内）
    response_queue: VecDeque<(u64, Value)>,
    /// 事件通知（响应到达时通知等待的 ask）
    ///
    /// P8 优化：消除协程调度器轮询，实现精确阻塞。
    /// 无 Mutex、无 Condvar——保持 Aura 无锁优势。
    response_notifier: Arc<dyn EventNotifier>,
}

impl std::fmt::Debug for ActorRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActorRuntime")
            .field("actors", &self.actors)
            .field("next_id", &self.next_id)
            .field("pending_requests", &self.pending_requests)
            .field("next_request_id", &self.next_request_id)
            .field("response_queue", &self.response_queue)
            .finish_non_exhaustive()
    }
}

/// 待处理请求（Phase 4）
#[derive(Debug, Clone)]
pub struct PendingRequest {
    pub request_id: u64,
    pub from_actor: ActorId,
    pub target_actor: ActorId,
    /// 挂起的协程 ID（0 表示主协程）
    pub response_coroutine: usize,
    /// 超时时间戳（Unix 毫秒，0 表示无超时）
    pub timeout_ms: u64,
    /// 创建时间戳
    pub created_ms: u64,
}

impl Default for ActorRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl ActorRuntime {
    pub fn new() -> Self {
        ActorRuntime {
            actors: Vec::new(),
            next_id: 1,
            pending_requests: HashMap::new(),
            next_request_id: 1,
            response_queue: VecDeque::new(),
            response_notifier: Arc::from(create_notifier()),
        }
    }

    /// 创建新 Actor，返回 Actor ID
    pub fn spawn(&mut self, name: &str) -> ActorId {
        let id = self.next_id;
        self.next_id += 1;
        while self.actors.len() <= id {
            self.actors.push(None);
        }
        self.actors[id] = Some(Actor {
            id,
            name: name.to_string(),
            mailbox: VecDeque::new(),
            state: HashMap::new(),
            alive: true,
            parent: None,
            children: Vec::new(),
            death_strategy: DeathStrategy::Terminate,
            death_reason: None,
        });
        id
    }

    /// 获取 Actor 引用
    pub fn get(&self, id: ActorId) -> Option<&Actor> {
        self.actors.get(id).and_then(|a| a.as_ref())
    }

    /// 获取 Actor 可变引用
    pub fn get_mut(&mut self, id: ActorId) -> Option<&mut Actor> {
        self.actors.get_mut(id).and_then(|a| a.as_mut())
    }

    /// 向 Actor 发送消息（非阻塞）
    pub fn send(&mut self, id: ActorId, msg: Value) {
        if let Some(actor) = self.get_mut(id) {
            actor.mailbox.push_back(msg);
        }
    }

    /// 向 Actor 请求响应（精确阻塞，零 CPU 空转，无锁）
    ///
    /// P8 优化：
    /// - 先检查 response_queue（非阻塞）
    /// - 队列为空时 `response_notifier.wait()` 精确阻塞
    /// - 响应到达时 `notify()` 唤醒等待者（零延迟）
    /// - 无 Mutex、无 Condvar——保持 Aura 无锁优势
    pub fn ask(&mut self, id: ActorId, msg: Value) -> Value {
        self.ask_with_timeout(id, msg, 0)
    }

    /// 向 Actor 请求响应（带超时，P8 事件驱动）
    ///
    /// `timeout_ms`: 超时时间（毫秒），0 表示无超时
    pub fn ask_with_timeout(&mut self, id: ActorId, msg: Value, timeout_ms: u64) -> Value {
        // 生成请求 ID
        let req_id = self.next_request_id;
        self.next_request_id += 1;

        // 记录当前时间
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        // 注册待处理请求
        self.pending_requests.insert(
            req_id,
            PendingRequest {
                request_id: req_id,
                from_actor: 0,
                target_actor: id,
                response_coroutine: 0,
                timeout_ms,
                created_ms: now_ms,
            },
        );

        // 发送消息（携带请求 ID）
        let mut wrapped_map = HashMap::new();
        wrapped_map.insert(Value::str_("_request_id"), Value::Int(req_id as i64));
        wrapped_map.insert(Value::str_("_payload"), msg);
        let wrapped_msg = Value::Map(wrapped_map);
        self.send(id, wrapped_msg);

        // P8 优化：精确阻塞等待响应（零 CPU 空转，无锁）
        loop {
            // 3.1 先检查 response_queue（非阻塞）
            let mut found = false;
            while let Some((rid, val)) = self.response_queue.front().cloned() {
                self.response_queue.pop_front();
                if rid == req_id {
                    self.pending_requests.remove(&req_id);
                    return val; // 找到响应
                }
                // 不是自己的，丢弃（其他请求的响应）
            }

            // 3.2 队列为空，等待事件（精确阻塞，零 CPU 空转）
            if !found {
                if timeout_ms > 0 {
                    let elapsed = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0)
                        - now_ms;
                    if elapsed >= timeout_ms {
                        self.pending_requests.remove(&req_id);
                        return Value::Null; // 超时
                    }
                    let remaining_ms = timeout_ms - elapsed;
                    let _ = self
                        .response_notifier
                        .wait(Some(std::time::Duration::from_millis(remaining_ms)));
                } else {
                    let _ = self.response_notifier.wait(None);
                }
            }
        }
    }

    /// 向 Actor 请求响应（**非阻塞**）：投递请求并立即检查响应队列。
    ///
    /// 当前 Actor 没有自动消息处理循环，`ask_with_timeout` 的「等待」依赖
    /// [`EventNotifier::wait`]，而各平台的 `wait` 实现会忽略超时参数（阻塞读），
    /// 因此无限/超时等待都会真正挂死。这里提供与设计文档一致的「伪阻塞」版本：
    /// 有响应立即返回，否则返回 `Null`，绝不阻塞。
    pub fn try_ask(&mut self, id: ActorId, msg: Value) -> Value {
        let req_id = self.next_request_id;
        self.next_request_id += 1;

        let mut wrapped_map = HashMap::new();
        wrapped_map.insert(Value::str_("_request_id"), Value::Int(req_id as i64));
        wrapped_map.insert(Value::str_("_payload"), msg);
        self.send(id, Value::Map(wrapped_map));

        // 立即检查响应队列（丢弃其他请求的响应，与 ask_with_timeout 一致）
        while let Some((rid, val)) = self.response_queue.front().cloned() {
            self.response_queue.pop_front();
            if rid == req_id {
                return val;
            }
        }
        Value::Null
    }

    /// Actor 回复请求（Phase 4 + P8 事件驱动）
    ///
    /// P8 优化：写 response_queue 后 `notify()` 唤醒等待的 ask。
    ///
    /// `request_id`: 请求 ID（从消息的 `_request_id` 字段获取）
    /// `response`: 响应值
    pub fn reply(&mut self, request_id: u64, response: Value) {
        self.response_queue.push_back((request_id, response));
        // 通知事件（唤醒等待的 ask）
        let _ = self.response_notifier.notify();
    }

    /// 检查 Actor 是否有待处理请求（Phase 4）
    pub fn has_pending_requests(&self) -> bool {
        !self.pending_requests.is_empty()
    }

    /// 获取待处理请求数量（Phase 4）
    pub fn pending_count(&self) -> usize {
        self.pending_requests.len()
    }

    /// 检查 Actor 是否存活
    pub fn is_alive(&self, id: ActorId) -> bool {
        self.get(id).map(|a| a.alive).unwrap_or(false)
    }

    /// 标记 Actor 死亡（Fix 12: 传播死亡策略）
    pub fn kill(&mut self, id: ActorId) {
        self.kill_with_reason(id, None)
    }

    /// 标记 Actor 死亡并记录原因
    pub fn kill_with_reason(&mut self, id: ActorId, reason: Option<String>) {
        if let Some(actor) = self.get_mut(id) {
            actor.alive = false;
            actor.death_reason = reason.clone();

            // 传播死亡策略
            let strategy = actor.death_strategy.clone();
            let children = actor.children.clone();
            let parent_id = actor.parent;

            match strategy {
                DeathStrategy::Terminate => {
                    // 终止所有子 Actor
                    for child_id in children {
                        self.kill_with_reason(child_id, Some(format!("parent {} terminated", id)));
                    }
                }
                DeathStrategy::Escalate => {
                    // 上报父 Actor（发送死亡消息）
                    if let Some(parent_id) = parent_id {
                        if self.is_alive(parent_id) {
                            let msg = Value::str_(format!("child {} died: {:?}", id, reason));
                            self.send(parent_id, msg);
                        }
                    }
                }
                DeathStrategy::Restart => {
                    // 重启 Actor（保留状态，清除邮箱）
                    actor.alive = true;
                    actor.mailbox.clear();
                    actor.death_reason = None;
                }
            }

            // 从父 Actor 的子列表中移除
            if let Some(parent_id) = parent_id {
                if let Some(parent) = self.get_mut(parent_id) {
                    parent.children.retain(|&c| c != id);
                }
            }
        }
    }

    /// 建立监督关系（parent 监督 child）
    pub fn supervise(&mut self, parent_id: ActorId, child_id: ActorId) {
        if let Some(child) = self.get_mut(child_id) {
            child.parent = Some(parent_id);
        }
        if let Some(parent) = self.get_mut(parent_id) {
            if !parent.children.contains(&child_id) {
                parent.children.push(child_id);
            }
        }
    }

    /// 获取 Actor 邮箱中的消息数量
    pub fn mailbox_len(&self, id: ActorId) -> usize {
        self.get(id).map(|a| a.mailbox.len()).unwrap_or(0)
    }

    /// 获取 Actor 私有状态
    pub fn get_state(&self, id: ActorId, key: &str) -> Option<Value> {
        self.get(id).and_then(|a| a.state.get(key).cloned())
    }

    /// 设置 Actor 私有状态
    pub fn set_state(&mut self, id: ActorId, key: &str, val: Value) {
        if let Some(actor) = self.get_mut(id) {
            actor.state.insert(key.to_string(), val);
        }
    }

    /// 设置 Actor 死亡策略（Fix 12）
    pub fn set_death_strategy(&mut self, id: ActorId, strategy: DeathStrategy) {
        if let Some(actor) = self.get_mut(id) {
            actor.death_strategy = strategy;
        }
    }

    /// 获取 Actor 死亡策略
    pub fn get_death_strategy(&self, id: ActorId) -> Option<&DeathStrategy> {
        self.get(id).map(|a| &a.death_strategy)
    }

    /// 获取 Actor 死亡原因（Fix 12）
    pub fn get_death_reason(&self, id: ActorId) -> Option<&String> {
        self.get(id).and_then(|a| a.death_reason.as_ref())
    }

    /// 获取活跃 Actor 数量
    pub fn active_count(&self) -> usize {
        self.actors.iter().filter(|a| a.is_some() && a.as_ref().unwrap().alive).count()
    }
}
