//! 多线程运行时（Fix 10）
//!
//! 对应 技术方案 §3.7 的多线程并发模型。在单线程协程调度器基础上，
//! 增加 OS 线程池支持，实现真正的并行执行。
//!
//! 实现要点：
//! - 线程池大小可配置（默认 = CPU 核心数）
//! - 任务队列使用 `Arc<Mutex<VecDeque<>>` 保证线程安全
//! - 每个 Actor 可在独立线程执行
//! - 提供 spawn/join/shutdown 生命周期管理

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

/// 线程池
pub struct ThreadPool {
    /// 工作线程列表
    workers: Vec<Worker>,
    /// 任务队列
    queue: Arc<Mutex<TaskQueue>>,
    /// 关闭标志
    shutdown: Arc<Mutex<bool>>,
    /// 关闭通知
    shutdown_cv: Arc<Condvar>,
}

/// 工作线程
struct Worker {
    /// 线程句柄（Option 因为线程会被 join）
    handle: Option<thread::JoinHandle<()>>,
}

/// 任务队列
struct TaskQueue {
    tasks: VecDeque<Box<dyn FnOnce() + Send>>,
}

impl ThreadPool {
    /// 创建指定大小的线程池
    pub fn new(size: usize) -> Self {
        let size = size.max(1);
        let queue = Arc::new(Mutex::new(TaskQueue {
            tasks: VecDeque::new(),
        }));
        let shutdown = Arc::new(Mutex::new(false));
        let shutdown_cv = Arc::new(Condvar::new());
        let mut workers = Vec::with_capacity(size);

        for _ in 0..size {
            let queue = queue.clone();
            let shutdown = shutdown.clone();
            let shutdown_cv = shutdown_cv.clone();

            let handle = thread::spawn(move || {
                loop {
                    let task = {
                        let mut q = queue.lock().unwrap();
                        while q.tasks.is_empty() {
                            drop(q);
                            let locked = shutdown.lock().unwrap();
                            if *locked {
                                return;
                            }
                            shutdown_cv.wait(locked).unwrap();
                            q = queue.lock().unwrap();
                        }
                        q.tasks.pop_front()
                    };

                    if let Some(task) = task {
                        task();
                    }
                }
            });

            workers.push(Worker {
                handle: Some(handle),
            });
        }

        ThreadPool {
            workers,
            queue,
            shutdown,
            shutdown_cv,
        }
    }

    /// 获取默认大小的线程池（CPU 核心数）
    pub fn default_pool() -> Self {
        let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        Self::new(cores)
    }

    /// 提交任务到线程池
    pub fn execute<F: FnOnce() + Send + 'static>(&self, task: F) {
        let mut q = self.queue.lock().unwrap();
        q.tasks.push_back(Box::new(task));
    }

    /// 线程池大小
    pub fn size(&self) -> usize {
        self.workers.len()
    }

    /// 关闭线程池（等待所有任务完成）
    pub fn shutdown(&mut self) {
        *self.shutdown.lock().unwrap() = true;
        self.shutdown_cv.notify_all();
        for worker in &mut self.workers {
            if let Some(handle) = worker.handle.take() {
                let _ = handle.join();
            }
        }
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        // shutdown 内部已处理，此处仅确保锁释放
        drop(self.shutdown.lock().unwrap());
    }
}

/// 线程安全计数器（用于测试）
pub struct AtomicCounter {
    value: Mutex<u64>,
}

impl AtomicCounter {
    pub fn new() -> Self {
        AtomicCounter {
            value: Mutex::new(0),
        }
    }

    pub fn increment(&self) {
        let mut v = self.value.lock().unwrap();
        *v += 1;
    }

    pub fn value(&self) -> u64 {
        *self.value.lock().unwrap()
    }
}
