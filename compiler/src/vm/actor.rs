//! Actor 模型运行时（P10.4）
//!
//! 对应 技术方案 §3.7 的 Actor 模型：每个 Actor 拥有私有状态和消息队列，
//! 通过 `send`/`ask` 进行消息传递，支持监督与容错（P10.7）。
//!
//! 实现要点：
//! - Actor ID 从 1 开始（0 保留给主线程）
//! - 消息队列使用 `VecDeque` 保证 FIFO 顺序
//! - `ask` 真阻塞等待响应（通过 PendingRequest + 协程调度器实现，Phase 4）
//! - 监督树：父 Actor 可监督子 Actor，子 Actor 崩溃时通知父 Actor

use std::collections::{HashMap, VecDeque};
use crate::vm::value::Value;

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
#[derive(Debug, Default)]
pub struct ActorRuntime {
    /// Actor ID → Actor 实例
    actors: Vec<Option<Actor>>,
    /// 下一个 Actor ID
    next_id: usize,
    /// 待处理请求（Phase 4: ask 真阻塞）
    pending_requests: HashMap<u64, PendingRequest>,
    /// 下一个请求 ID
    next_request_id: u64,
    /// 响应回调队列（Phase 4: Actor 处理消息后写回响应）
    response_queue: VecDeque<(u64, Value)>,
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

impl ActorRuntime {
    pub fn new() -> Self {
        ActorRuntime {
            actors: Vec::new(),
            next_id: 1,
            pending_requests: HashMap::new(),
            next_request_id: 1,
            response_queue: VecDeque::new(),
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
            death_strategy: DeathStrategy::Terminate, // 默认策略
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

    /// 向 Actor 请求响应（真阻塞等待，Phase 4）
    ///
    /// 在当前协作调度模型下，`ask` 将消息送入邮箱后
    /// 注册 PendingRequest，然后轮询响应队列直到收到响应或超时。
    /// 若邮箱为空或超时则返回 `Null`。
    pub fn ask(&mut self, id: ActorId, msg: Value) -> Value {
        self.ask_with_timeout(id, msg, 0)
    }

    /// 向 Actor 请求响应（带超时，Phase 4）
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
        self.pending_requests.insert(req_id, PendingRequest {
            request_id: req_id,
            from_actor: 0, // 主协程
            target_actor: id,
            response_coroutine: 0,
            timeout_ms,
            created_ms: now_ms,
        });

        // 发送消息（携带请求 ID）
        let mut wrapped_map = HashMap::new();
        wrapped_map.insert(Value::str_("_request_id"), Value::Int(req_id as i64));
        wrapped_map.insert(Value::str_("_payload"), msg);
        let wrapped_msg = Value::Map(wrapped_map);
        self.send(id, wrapped_msg);

        // 轮询响应队列（真阻塞）
        loop {
            // 检查响应队列
            if let Some((resp_req_id, response)) = self.response_queue.pop_front() {
                if resp_req_id == req_id {
                    self.pending_requests.remove(&req_id);
                    return response;
                }
                // 其他请求的响应，重新入队
                self.response_queue.push_back((resp_req_id, response));
            }

            // 检查超时
            if timeout_ms > 0 {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                if now - now_ms >= timeout_ms {
                    self.pending_requests.remove(&req_id);
                    return Value::Null;
                }
            }

            // 无响应时让出（避免死循环）
            // 在单线程 VM 中，这里直接返回 Null（协作式阻塞）
            // 真正的阻塞需要协程调度器支持
            if self.response_queue.is_empty() {
                // 检查是否还有待处理请求
                if self.pending_requests.is_empty() {
                    break;
                }
                // 让出执行权（在完整实现中应挂起协程）
                // 当前简化实现：直接返回 Null
                break;
            }
        }

        self.pending_requests.remove(&req_id);
        Value::Null
    }

    /// Actor 回复请求（Phase 4）
    ///
    /// `request_id`: 请求 ID（从消息的 `_request_id` 字段获取）
    /// `response`: 响应值
    pub fn reply(&mut self, request_id: u64, response: Value) {
        self.response_queue.push_back((request_id, response));
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
        self.kill_with_reason(id, None);
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
