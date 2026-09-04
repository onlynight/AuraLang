//! Channel 类型实现（P10.8）
//!
//! 对应 技术方案 §3.7 的 Channel 类型：有界/无界消息通道。
//!
//! 实现要点：
//! - 有界 Channel（bound > 0）：超过容量时 `send` 阻塞
//! - 无界 Channel（bound == 0）：`send` 永不阻塞
//! - `recv` 在通道为空时阻塞
//! - `tryRecv` 非阻塞，空时返回 `Null`

use std::collections::VecDeque;
use crate::vm::value::Value;

/// Channel 实例 ID
pub type ChannelId = usize;

/// Channel 实例
#[derive(Debug, Clone)]
pub struct Channel {
    pub id: ChannelId,
    /// 容量（0 = 无界）
    pub bound: usize,
    /// 消息缓冲区
    pub buffer: VecDeque<Value>,
}

/// Channel 运行时管理器
#[derive(Debug, Default)]
pub struct ChannelRuntime {
    /// Channel ID → Channel 实例
    channels: Vec<Option<Channel>>,
    /// 下一个 Channel ID
    next_id: usize,
}

impl ChannelRuntime {
    pub fn new() -> Self {
        ChannelRuntime {
            channels: Vec::new(),
            next_id: 1,
        }
    }

    /// 创建新 Channel，返回 Channel ID
    ///
    /// `bound`: 容量上限，0 表示无界
    pub fn new_channel(&mut self, bound: usize) -> ChannelId {
        let id = self.next_id;
        self.next_id += 1;
        while self.channels.len() <= id {
            self.channels.push(None);
        }
        self.channels[id] = Some(Channel {
            id,
            bound,
            buffer: VecDeque::new(),
        });
        id
    }

    /// 获取 Channel 引用
    pub fn get(&self, id: ChannelId) -> Option<&Channel> {
        self.channels.get(id).and_then(|c| c.as_ref())
    }

    /// 获取 Channel 可变引用
    pub fn get_mut(&mut self, id: ChannelId) -> Option<&mut Channel> {
        self.channels.get_mut(id).and_then(|c| c.as_mut())
    }

    /// 向 Channel 发送值
    ///
    /// 有界 Channel 且缓冲区满时返回 `false`（非阻塞模式）
    pub fn send(&mut self, id: ChannelId, val: Value) -> bool {
        if let Some(ch) = self.get_mut(id) {
            if ch.bound > 0 && ch.buffer.len() >= ch.bound {
                return false; // 缓冲区满
            }
            ch.buffer.push_back(val);
            true
        } else {
            false
        }
    }

    /// 从 Channel 接收值（阻塞语义）
    ///
    /// 在当前协作调度模型下，`recv` 立即返回：
    /// 若缓冲区非空则取出并返回，否则返回 `Null`。
    pub fn recv(&mut self, id: ChannelId) -> Value {
        if let Some(ch) = self.get_mut(id) {
            ch.buffer.pop_front().unwrap_or(Value::Null)
        } else {
            Value::Null
        }
    }

    /// 尝试从 Channel 接收值（非阻塞）
    ///
    /// 若缓冲区为空或 Channel 不存在，返回 `Null`。
    pub fn try_recv(&mut self, id: ChannelId) -> Value {
        self.recv(id)
    }

    /// 获取 Channel 缓冲区长度
    pub fn len(&self, id: ChannelId) -> usize {
        self.get(id).map(|c| c.buffer.len()).unwrap_or(0)
    }

    /// 通道是否为空
    pub fn is_empty(&self, id: ChannelId) -> bool {
        self.get(id).map(|c| c.buffer.is_empty()).unwrap_or(true)
    }

    /// 获取活跃 Channel 数量
    pub fn active_count(&self) -> usize {
        self.channels.iter().filter(|c| c.is_some()).count()
    }
}
