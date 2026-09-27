//! 协程调度器（5.8）
//!
//! 对应 技术方案 §3.7 的 `suspend` / `await` 语义。每个协程由一个 [`Coroutine`]
//! 表示，保存其调用帧栈快照、返回值通道。调度器维护运行队列（Ready / Suspended / Done），
//! 支持 `yield` 主动挂起、`resume` 恢复、以及 `spawn` 创建。
//!
//! 实现要点：
//! - 协程 ID 从 1 开始（0 保留给主线程）
//! - `Yield` 指令挂起当前协程，将栈顶值置为返回值
//! - `ResumeCoroutine` 恢复指定协程，将栈顶值作为入参
//! - 已终止的协程保留最后返回值供 `ResumeCoroutine` 读取一次

use crate::vm::Frame;
use crate::vm::value::Value;

/// 协程生命周期状态
#[derive(Debug, Clone, PartialEq)]
pub enum CoroutineState {
    /// 等待运行
    Ready,
    /// 主动挂起（Yield），携带挂起返回值
    Suspended(Value),
    /// 已终止，携带终止返回值
    Done(Value),
}

/// 单个协程实例
#[derive(Debug, Clone)]
pub struct Coroutine {
    pub id: usize,
    /// 入口函数索引
    pub entry_func: usize,
    /// 调用帧栈快照
    pub frames: Vec<Frame>,
    /// 当前状态
    pub state: CoroutineState,
}

impl Coroutine {
    pub fn new(id: usize, entry_func: usize) -> Self {
        Coroutine {
            id,
            entry_func,
            frames: Vec::new(),
            state: CoroutineState::Ready,
        }
    }
}

/// 协程调度器
pub struct CoroutineScheduler {
    /// 协程 ID → 协程实例
    coroutines: Vec<Option<Coroutine>>,
    /// 运行就绪队列
    ready_queue: Vec<usize>,
    /// 下一个协程 ID
    next_id: usize,
}

impl Default for CoroutineScheduler {
    fn default() -> Self {
        CoroutineScheduler::new()
    }
}

impl CoroutineScheduler {
    pub fn new() -> Self {
        CoroutineScheduler {
            coroutines: Vec::new(),
            ready_queue: Vec::new(),
            next_id: 1,
        }
    }

    /// 创建新协程，返回协程 ID
    pub fn spawn(&mut self, entry_func: usize) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        let co = Coroutine::new(id, entry_func);
        // 确保索引有效
        while self.coroutines.len() <= id {
            self.coroutines.push(None);
        }
        self.coroutines[id] = Some(co);
        self.ready_queue.push(id);
        id
    }

    /// 获取协程引用
    pub fn get(&self, id: usize) -> Option<&Coroutine> {
        self.coroutines.get(id).and_then(|c| c.as_ref())
    }

    /// 获取协程可变引用
    pub fn get_mut(&mut self, id: usize) -> Option<&mut Coroutine> {
        self.coroutines.get_mut(id).and_then(|c| c.as_mut())
    }

    /// 获取下一个就绪协程 ID（轮转调度）
    pub fn next_ready(&mut self) -> Option<usize> {
        if self.ready_queue.is_empty() { None } else { Some(self.ready_queue.remove(0)) }
    }

    /// 将协程放回就绪队列
    pub fn enqueue(&mut self, id: usize) {
        self.ready_queue.push(id);
    }

    /// 获取就绪队列长度
    pub fn ready_count(&self) -> usize {
        self.ready_queue.len()
    }

    /// 获取活跃协程数
    pub fn active_count(&self) -> usize {
        self.coroutines.iter().filter(|c| c.is_some()).count()
    }

    /// 保存当前帧栈到指定协程（挂起时调用）
    pub fn save_frames(&mut self, id: usize, frames: Vec<Frame>, return_val: Value) {
        if let Some(co) = self.coroutines.get_mut(id).and_then(|c| c.as_mut()) {
            co.frames = frames;
            co.state = CoroutineState::Suspended(return_val);
        }
    }

    /// 从指定协程恢复帧栈（恢复时调用）
    pub fn restore_frames(&mut self, id: usize) -> Option<Vec<Frame>> {
        let frames = if let Some(co) = self.coroutines.get(id).and_then(|c| c.as_ref()) {
            co.frames.clone()
        } else {
            return None;
        };
        if let Some(co) = self.coroutines.get_mut(id).and_then(|c| c.as_mut()) {
            co.state = CoroutineState::Ready;
        }
        Some(frames)
    }

    /// 标记协程终止
    pub fn mark_done(&mut self, id: usize, result: Value) {
        if let Some(co) = self.coroutines.get_mut(id).and_then(|c| c.as_mut()) {
            co.state = CoroutineState::Done(result);
        }
    }

    /// 获取协程的终止返回值（读取一次）
    pub fn take_done_value(&mut self, id: usize) -> Option<Value> {
        self.coroutines.get_mut(id).and_then(|c| c.as_mut()).and_then(|co| {
            if let CoroutineState::Done(v) = &co.state { Some(v.clone()) } else { None }
        })
    }
}
