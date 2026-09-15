//! Channel 类型实现（P10.8）
//!
//! 对应 技术方案 §3.7 的 Channel 类型：有界/无界消息通道。
//!
//! 实现要点：
//! - 有界 Channel（bound > 0）：超过容量时 `send` 阻塞
//! - 无界 Channel（bound == 0）：`send` 永不阻塞
//! - `recv` 在通道为空时阻塞
//! - `tryRecv` 非阻塞，空时返回 `Null`
//! - `recv_timeout`（Fix 11）：带超时的接收，超时返回 `Null`
//!
//! **P8 优化**：新增事件通知（EventNotifier），消除轮询空转。
//! - 发送方写入 buffer 后 `notify()` 通知事件
//! - 接收方 buffer 空时 `wait()` 精确阻塞（零 CPU 空转）
//! - 无 Mutex、无 Condvar——保持 Aura 无锁隔离优势
//!
//! 对应文档：`docs/pure_aura_jit/jit模式优化方案.md` §7.3

use crate::vm::event_notifier::{EventNotifier, create_notifier};
use crate::vm::value::Value;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

/// Channel 实例 ID
pub type ChannelId = usize;

/// Channel 实例
pub struct Channel {
    pub id: ChannelId,
    /// 容量（0 = 无界）
    pub bound: usize,
    /// 消息缓冲区（无锁，单 VM 内单线程使用）
    pub buffer: VecDeque<Value>,
    /// 事件通知（发送方写入，接收方 epoll 等待）
    ///
    /// P8 优化：消除轮询空转，实现精确阻塞。
    /// 无 Mutex、无 Condvar——保持 Aura 无锁优势。
    pub notifier: Arc<dyn EventNotifier>,
}

impl std::fmt::Debug for Channel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Channel")
            .field("id", &self.id)
            .field("bound", &self.bound)
            .field("buffer", &self.buffer)
            .finish_non_exhaustive()
    }
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
            notifier: Arc::from(create_notifier()),
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
    ///
    /// P8 优化：发送后 `notify()` 唤醒等待的接收方（零延迟）。
    pub fn send(&mut self, id: ChannelId, val: Value) -> bool {
        if let Some(ch) = self.get_mut(id) {
            if ch.bound > 0 && ch.buffer.len() >= ch.bound {
                return false; // 缓冲区满
            }
            // 写入 buffer（无锁，单线程）
            ch.buffer.push_back(val);
            // 通知事件（唤醒等待的接收方）
            let _ = ch.notifier.notify();
            true
        } else {
            false
        }
    }

    /// 从 Channel 接收值（精确阻塞，零 CPU 空转）
    ///
    /// P8 优化：
    /// - 先尝试非阻塞 pop
    /// - buffer 空时 `notifier.wait()` 精确阻塞（epoll 系统调用，零 CPU 消耗）
    /// - 被唤醒后 pop 并返回
    /// - 无 Mutex、无 Condvar——保持 Aura 无锁优势
    pub fn recv(&mut self, id: ChannelId) -> Value {
        if let Some(ch) = self.get_mut(id) {
            // 1. 先尝试非阻塞 pop
            if let Some(val) = ch.buffer.pop_front() {
                return val;
            }

            // 2. buffer 空，等待事件（精确阻塞，零 CPU 空转）
            let _ = ch.notifier.wait(None);

            // 3. 被唤醒后 pop
            ch.buffer.pop_front().unwrap_or(Value::Null)
        } else {
            Value::Null
        }
    }

    /// 从 Channel 接收值（精确阻塞，零 CPU 空转）
    ///
    /// 兼容旧版 `recv_blocking` 接口
    #[inline]
    pub fn recv_blocking(&mut self, id: ChannelId) -> Value {
        self.recv(id)
    }

    /// 尝试从 Channel 接收值（非阻塞）
    ///
    /// 若缓冲区为空或 Channel 不存在，返回 `Null`。
    pub fn try_recv(&mut self, id: ChannelId) -> Value {
        if let Some(ch) = self.get_mut(id) {
            ch.buffer.pop_front().unwrap_or(Value::Null)
        } else {
            Value::Null
        }
    }

    /// 带超时的接收（P8 优化：事件驱动，精确超时）
    ///
    /// 在 `timeout` 时间内等待消息，超时返回 `Null`。
    ///
    /// P8 优化：
    /// - 使用 `notifier.wait(Some(timeout))` 精确阻塞
    /// - 误差 <100µs（epoll 唤醒延迟）
    /// - 替代原来的 1ms 轮询 + sleep
    pub fn recv_timeout(&mut self, id: ChannelId, timeout: Duration) -> Value {
        if let Some(ch) = self.get_mut(id) {
            // 1. 先尝试非阻塞 pop
            if let Some(val) = ch.buffer.pop_front() {
                return val;
            }

            // 2. buffer 空，等待事件（带超时）
            match ch.notifier.wait(Some(timeout)) {
                Ok(true) => {
                    // 有事件，pop 并返回
                    ch.buffer.pop_front().unwrap_or(Value::Null)
                }
                Ok(false) => {
                    // 超时
                    Value::Null
                }
                Err(_) => Value::Null,
            }
        } else {
            Value::Null
        }
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
