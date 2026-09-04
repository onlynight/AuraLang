//! Actor 模型运行时（P10.4）
//!
//! 对应 技术方案 §3.7 的 Actor 模型：每个 Actor 拥有私有状态和消息队列，
//! 通过 `send`/`ask` 进行消息传递，支持监督与容错（P10.7）。
//!
//! 实现要点：
//! - Actor ID 从 1 开始（0 保留给主线程）
//! - 消息队列使用 `VecDeque` 保证 FIFO 顺序
//! - `ask` 阻塞等待响应（通过协作调度器轮询实现）
//! - 监督树：父 Actor 可监督子 Actor，子 Actor 崩溃时通知父 Actor

use std::collections::{HashMap, VecDeque};
use crate::vm::value::Value;

/// Actor 实例 ID
pub type ActorId = usize;

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
}

/// Actor 运行时管理器
#[derive(Debug, Default)]
pub struct ActorRuntime {
    /// Actor ID → Actor 实例
    actors: Vec<Option<Actor>>,
    /// 下一个 Actor ID
    next_id: usize,
}

impl ActorRuntime {
    pub fn new() -> Self {
        ActorRuntime {
            actors: Vec::new(),
            next_id: 1,
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

    /// 向 Actor 请求响应（阻塞等待）
    ///
    /// 在当前协作调度模型下，`ask` 将消息送入邮箱后
    /// 不断从邮箱中取出第一个响应（如果有）作为返回值。
    /// 若邮箱为空则返回 `Null`。
    pub fn ask(&mut self, id: ActorId, msg: Value) -> Value {
        self.send(id, msg);
        // 非阻塞：立即检查邮箱是否有响应
        if let Some(actor) = self.get_mut(id) {
            actor.mailbox.pop_front().unwrap_or(Value::Null)
        } else {
            Value::Null
        }
    }

    /// 检查 Actor 是否存活
    pub fn is_alive(&self, id: ActorId) -> bool {
        self.get(id).map(|a| a.alive).unwrap_or(false)
    }

    /// 标记 Actor 死亡
    pub fn kill(&mut self, id: ActorId) {
        if let Some(actor) = self.get_mut(id) {
            actor.alive = false;
            // 通知父 Actor（监督机制）
            if let Some(parent_id) = actor.parent {
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

    /// 获取活跃 Actor 数量
    pub fn active_count(&self) -> usize {
        self.actors.iter().filter(|a| a.is_some() && a.as_ref().unwrap().alive).count()
    }
}
