//! 事件通知抽象（跨平台）— JIT 模式优化 P8
//!
//! 为 Channel / Actor 提供事件驱动阻塞机制，消除轮询空转。
//! 核心原则：
//! - 不加锁（单 VM 内单线程，不需要 Mutex）
//! - 无 Condvar（保持 Aura 无锁隔离优势）
//! - 零 CPU 空转（epoll/kqueue 系统调用阻塞）
//!
//! 对应文档：`docs/pure_aura_jit/jit模式优化方案.md` §7.3
//!
//! 平台实现：
//! - Linux: EventFd（libc::eventfd）— 高性能
//! - macOS/其他 Unix: 管道（pipe）— fallback
//! - Windows: 管道（CreatePipe）— fallback

use std::time::Duration;

/// 事件通知 trait（跨平台抽象）
///
/// 发送方调用 `notify()` 写入事件，接收方调用 `wait()` 精确阻塞。
/// `fd()` 获取底层文件描述符用于 epoll/kqueue 多路复用。
pub trait EventNotifier: Send + Sync {
    /// 写入事件（发送方调用）
    fn notify(&self) -> Result<(), std::io::Error>;

    /// 等待事件（接收方调用，精确阻塞）
    ///
    /// `timeout`: 超时时间，`None` 表示无限等待
    /// 返回 `true` 表示收到事件，`false` 表示超时
    fn wait(&self, timeout: Option<Duration>) -> Result<bool, std::io::Error>;

    /// 消耗已读事件（防止重复触发）
    fn drain(&self) -> Result<(), std::io::Error>;

    /// 获取底层文件描述符（用于 epoll/kqueue 多路复用）
    fn fd(&self) -> i32;
}

// ═══════════════════════════════════════════════════════════════
// 管道实现（跨平台 fallback）
// ═══════════════════════════════════════════════════════════════

/// 管道事件通知器（跨平台）
///
/// 使用 OS 管道实现事件通知。
/// 写入 1 字节 = 触发事件，读取 1 字节 = 消耗事件。
#[cfg(unix)]
pub struct PipeNotifier {
    reader_fd: i32,
    writer_fd: i32,
    reader_file: std::fs::File,
    writer_file: std::fs::File,
}

#[cfg(unix)]
impl PipeNotifier {
    /// 创建新的管道事件通知器
    pub fn new() -> Self {
        let mut fds = [0i32; 2];
        let ret = unsafe { libc::pipe(fds.as_mut_ptr()) };
        assert_eq!(ret, 0, "pipe() failed: {}", std::io::Error::last_os_error());

        // 防止 File 的 Drop 关闭 fd（我们手动管理）
        // 使用 mem::forget 阻止自动关闭
        let reader_file = unsafe { std::fs::File::from_raw_fd(fds[0]) };
        let writer_file = unsafe { std::fs::File::from_raw_fd(fds[1]) };
        std::mem::forget(reader_file);
        std::mem::forget(writer_file);

        PipeNotifier {
            reader_fd: fds[0],
            writer_fd: fds[1],
            // 这些 File 仅用于持有 fd，实际用 raw fd 操作
            reader_file: unsafe { std::fs::File::from_raw_fd(fds[0]) },
            writer_file: unsafe { std::fs::File::from_raw_fd(fds[1]) },
        }
    }
}

#[cfg(unix)]
impl Drop for PipeNotifier {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.reader_fd);
            libc::close(self.writer_fd);
        }
    }
}

#[cfg(unix)]
impl EventNotifier for PipeNotifier {
    fn notify(&self) -> Result<(), std::io::Error> {
        let val = 1u8;
        let ret = unsafe {
            libc::write(
                self.writer_fd,
                &val as *const u8 as *const core::ffi::c_void,
                1,
            )
        };
        if ret < 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) }
    }

    fn wait(&self, timeout: Option<Duration>) -> Result<bool, std::io::Error> {
        // 先 poll 等待可读，尊重 timeout。
        // 旧实现直接 read：`_timeout` 被忽略，超时/无限等待都会永久阻塞
        // （Channel 超时、Actor.ask 因此挂死）。
        let ms: i32 = match timeout {
            Some(d) => d.as_millis().min(i32::MAX as u128) as i32,
            None => -1,
        };
        let mut pfd = libc::pollfd {
            fd: self.reader_fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let rc = unsafe { libc::poll(&mut pfd, 1, ms) };
        if rc < 0 {
            return Err(std::io::Error::last_os_error());
        }
        if rc == 0 {
            return Ok(false); // 超时
        }
        let mut buf = [0u8; 1];
        let ret = unsafe {
            libc::read(
                self.reader_fd,
                buf.as_mut_ptr() as *mut core::ffi::c_void,
                1,
            )
        };
        if ret < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::WouldBlock { Ok(false) } else { Err(err) }
        } else if ret == 0 {
            Ok(false) // EOF
        } else {
            Ok(true)
        }
    }

    fn drain(&self) -> Result<(), std::io::Error> {
        let mut buf = [0u8; 64];
        loop {
            let ret = unsafe {
                libc::read(
                    self.reader_fd,
                    buf.as_mut_ptr() as *mut core::ffi::c_void,
                    64,
                )
            };
            if ret < 0 {
                let err = std::io::Error::last_os_error();
                if err.kind() == std::io::ErrorKind::WouldBlock {
                    break;
                }
                return Err(err);
            }
            if ret == 0 {
                break;
            }
        }
        Ok(())
    }

    fn fd(&self) -> i32 {
        self.reader_fd
    }
}

#[cfg(unix)]
impl Default for PipeNotifier {
    fn default() -> Self {
        Self::new()
    }
}

// ═══════════════════════════════════════════════════════════════
// Windows 管道实现
// ═══════════════════════════════════════════════════════════════

#[cfg(windows)]
pub struct PipeNotifier {
    reader_handle: usize,
    writer_handle: usize,
}

#[cfg(windows)]
impl PipeNotifier {
    /// 创建新的管道事件通知器
    pub fn new() -> Self {
        use std::os::windows::io::{AsRawHandle, FromRawHandle};
        unsafe extern "C" {
            fn CreatePipe(
                hReadPipe: *mut usize,
                hWritePipe: *mut usize,
                lpPipeAttributes: *mut usize,
                nSize: u32,
            ) -> i32;
        }
        let mut read_handle: usize = 0;
        let mut write_handle: usize = 0;
        unsafe {
            let ok = CreatePipe(&mut read_handle, &mut write_handle, 0 as *mut usize, 0);
            assert_ne!(ok, 0, "CreatePipe failed");
        }
        PipeNotifier {
            reader_handle: read_handle,
            writer_handle: write_handle,
        }
    }
}

#[cfg(windows)]
impl Drop for PipeNotifier {
    fn drop(&mut self) {
        unsafe extern "C" {
            fn CloseHandle(handle: usize) -> i32;
        }
        unsafe {
            CloseHandle(self.reader_handle);
            CloseHandle(self.writer_handle);
        }
    }
}

#[cfg(windows)]
impl EventNotifier for PipeNotifier {
    fn notify(&self) -> Result<(), std::io::Error> {
        unsafe extern "C" {
            fn WriteFile(
                hFile: usize,
                lpBuffer: *const core::ffi::c_void,
                nNumberOfBytesToWrite: u32,
                lpNumberOfBytesWritten: *mut u32,
                lpOverlapped: *mut usize,
            ) -> i32;
        }
        let val = 1u8;
        unsafe {
            let ok = WriteFile(
                self.writer_handle,
                &val as *const u8 as *const core::ffi::c_void,
                1,
                0 as *mut u32,
                0 as *mut usize,
            );
            if ok == 0 {
                Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "WriteFile failed",
                ))
            } else {
                Ok(())
            }
        }
    }

    fn wait(&self, timeout: Option<Duration>) -> Result<bool, std::io::Error> {
        unsafe extern "C" {
            fn ReadFile(
                hFile: usize,
                lpBuffer: *mut core::ffi::c_void,
                nNumberOfBytesToRead: u32,
                lpNumberOfBytesRead: *mut u32,
                lpOverlapped: *mut usize,
            ) -> i32;
            fn PeekNamedPipe(
                hNamedPipe: usize,
                lpBuffer: *mut core::ffi::c_void,
                nBufferSize: u32,
                lpBytesRead: *mut u32,
                lpTotalBytesAvail: *mut u32,
                lpBytesLeftThisMessage: *mut u32,
            ) -> i32;
        }
        // 带超时：轮询 PeekNamedPipe 判断管道是否可读，超时即返回 `false`。
        //
        // 注意：这里**没有**用 `WaitForSingleObject` —— 实测匿名管道的读句柄会立刻
        // 返回 signaled（即便无数据），随后 `ReadFile` 仍然阻塞，超时形同虚设
        // （Channel.recv_timeout / Actor.ask 因此挂死）。
        // `timeout == None` 时不轮询，直接阻塞读，保持零 CPU 空转。
        if let Some(d) = timeout {
            let deadline = std::time::Instant::now() + d;
            loop {
                let mut avail: u32 = 0;
                let ok = unsafe {
                    PeekNamedPipe(
                        self.reader_handle,
                        0 as *mut core::ffi::c_void,
                        0,
                        0 as *mut u32,
                        &mut avail,
                        0 as *mut u32,
                    )
                };
                if ok == 0 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "PeekNamedPipe failed",
                    ));
                }
                if avail > 0 {
                    break;
                }
                if std::time::Instant::now() >= deadline {
                    return Ok(false); // 超时
                }
                std::thread::sleep(Duration::from_micros(200));
            }
        }
        let mut buf = [0u8; 1];
        let mut bytes_read: u32 = 0;
        unsafe {
            let ok = ReadFile(
                self.reader_handle,
                buf.as_mut_ptr() as *mut core::ffi::c_void,
                1,
                &mut bytes_read,
                0 as *mut usize,
            );
            if ok == 0 {
                Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "ReadFile failed",
                ))
            } else if bytes_read == 0 {
                Ok(false) // EOF
            } else {
                Ok(true)
            }
        }
    }

    fn drain(&self) -> Result<(), std::io::Error> {
        unsafe extern "C" {
            fn ReadFile(
                hFile: usize,
                lpBuffer: *mut core::ffi::c_void,
                nNumberOfBytesToRead: u32,
                lpNumberOfBytesRead: *mut u32,
                lpOverlapped: *mut usize,
            ) -> i32;
            fn PeekNamedPipe(
                hNamedPipe: usize,
                lpBuffer: *mut core::ffi::c_void,
                nBufferSize: u32,
                lpBytesRead: *mut u32,
                lpTotalBytesAvail: *mut u32,
                lpBytesLeftThisMessage: *mut u32,
            ) -> i32;
        }
        // 只读走「当前已可读」的字节，读空即返回。
        //
        // 不能沿用「一直 ReadFile 直到读到 0 字节」的写法：Windows 匿名管道是**同步**
        // 句柄，缓冲区读空后 ReadFile 会阻塞等待新数据（`test_drain` 因此挂死）。
        loop {
            let mut avail: u32 = 0;
            let ok = unsafe {
                PeekNamedPipe(
                    self.reader_handle,
                    0 as *mut core::ffi::c_void,
                    0,
                    0 as *mut u32,
                    &mut avail,
                    0 as *mut u32,
                )
            };
            if ok == 0 || avail == 0 {
                break;
            }
            let to_read = if avail > 64 { 64 } else { avail };
            let mut buf = [0u8; 64];
            let mut bytes_read: u32 = 0;
            let ok = unsafe {
                ReadFile(
                    self.reader_handle,
                    buf.as_mut_ptr() as *mut core::ffi::c_void,
                    to_read,
                    &mut bytes_read,
                    0 as *mut usize,
                )
            };
            if ok == 0 || bytes_read == 0 {
                break;
            }
        }
        Ok(())
    }

    fn fd(&self) -> i32 {
        // Windows 没有 fd 概念，返回 -1
        -1
    }
}

#[cfg(windows)]
impl Default for PipeNotifier {
    fn default() -> Self {
        Self::new()
    }
}

// ═══════════════════════════════════════════════════════════════
// EventFd 实现（Linux 专用，高性能）
// ═══════════════════════════════════════════════════════════════

#[cfg(target_os = "linux")]
mod linux_eventfd {
    use super::*;

    /// EventFd 事件通知器（Linux）
    pub struct EventFdNotifier {
        fd: i32,
    }

    impl EventFdNotifier {
        /// 创建 EventFd
        pub fn new() -> Self {
            let fd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC) };
            assert!(
                fd >= 0,
                "eventfd() failed: {}",
                std::io::Error::last_os_error()
            );
            EventFdNotifier { fd }
        }
    }

    impl Drop for EventFdNotifier {
        fn drop(&mut self) {
            unsafe {
                libc::close(self.fd);
            }
        }
    }

    impl EventNotifier for EventFdNotifier {
        fn notify(&self) -> Result<(), std::io::Error> {
            let val: u64 = 1;
            let ret = unsafe { libc::write(self.fd, val.as_ptr() as *const core::ffi::c_void, 8) };
            if ret < 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) }
        }

        fn wait(&self, timeout: Option<Duration>) -> Result<bool, std::io::Error> {
            // 先 poll 等待可读，尊重 timeout（旧实现忽略超时并永久阻塞）。
            let ms: i32 = match timeout {
                Some(d) => d.as_millis().min(i32::MAX as u128) as i32,
                None => -1,
            };
            let mut pfd = libc::pollfd {
                fd: self.fd,
                events: libc::POLLIN,
                revents: 0,
            };
            let rc = unsafe { libc::poll(&mut pfd, 1, ms) };
            if rc < 0 {
                return Err(std::io::Error::last_os_error());
            }
            if rc == 0 {
                return Ok(false); // 超时
            }
            let mut val: u64 = 0;
            let ret = unsafe { libc::read(self.fd, val.as_mut_ptr() as *mut core::ffi::c_void, 8) };
            if ret < 0 {
                let err = std::io::Error::last_os_error();
                if err.kind() == std::io::ErrorKind::WouldBlock { Ok(false) } else { Err(err) }
            } else if ret == 0 {
                Ok(false)
            } else {
                Ok(true)
            }
        }

        fn drain(&self) -> Result<(), std::io::Error> {
            let mut val: u64 = 0;
            loop {
                let ret =
                    unsafe { libc::read(self.fd, val.as_mut_ptr() as *mut core::ffi::c_void, 8) };
                if ret < 0 {
                    let err = std::io::Error::last_os_error();
                    if err.kind() == std::io::ErrorKind::WouldBlock {
                        break;
                    }
                    return Err(err);
                }
                if ret == 0 {
                    break;
                }
            }
            Ok(())
        }

        fn fd(&self) -> i32 {
            self.fd
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// 默认事件通知器工厂
// ═══════════════════════════════════════════════════════════════

/// 创建默认的事件通知器
///
/// Linux 下使用 EventFd（高性能），其他平台使用管道 fallback。
#[cfg(target_os = "linux")]
pub fn create_notifier() -> Box<dyn EventNotifier> {
    Box::new(linux_eventfd::EventFdNotifier::new())
}

#[cfg(not(target_os = "linux"))]
pub fn create_notifier() -> Box<dyn EventNotifier> {
    Box::new(PipeNotifier::new())
}

// ═══════════════════════════════════════════════════════════════
// 单元测试
// ═══════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_notify_wait() {
        let notifier = create_notifier();
        // 通知
        notifier.notify().unwrap();

        // 等待（应立即返回）
        let result = notifier.wait(Some(Duration::from_millis(1000)));
        assert!(result.unwrap(), "should receive event after notify");
    }

    #[test]
    fn test_drain() {
        let notifier = create_notifier();

        // 通知多次
        notifier.notify().unwrap();
        notifier.notify().unwrap();
        notifier.notify().unwrap();

        // drain 应消耗所有事件
        notifier.drain().unwrap();

        // drain 后再次 drain 不应报错
        notifier.drain().unwrap();
    }

    #[test]
    fn test_fd() {
        let notifier = create_notifier();
        let fd = notifier.fd();
        // Linux/Unix: fd >= 0, Windows: fd == -1
        if cfg!(target_os = "windows") {
            assert_eq!(fd, -1);
        } else {
            assert!(fd >= 0, "fd should be non-negative");
        }
    }

    #[test]
    fn test_pipe_notifier_basic() {
        let notifier = PipeNotifier::new();
        notifier.notify().unwrap();
        let result = notifier.wait(Some(Duration::from_millis(100)));
        assert!(result.unwrap(), "pipe notifier should deliver event");
    }

    #[test]
    fn test_multiple_events() {
        let notifier = create_notifier();
        notifier.notify().unwrap();
        notifier.notify().unwrap();
        // 两次通知，至少一次等待应成功
        let r1 = notifier.wait(Some(Duration::from_millis(100)));
        assert!(r1.unwrap(), "first wait should succeed");
    }

    /// 回归测试：空通知器上的带超时等待必须**按超时返回 false**，不能永久阻塞。
    ///
    /// 旧实现忽略 `timeout` 参数直接阻塞读，导致 Channel.recv_timeout / Actor.ask 挂死。
    #[test]
    fn test_wait_timeout_returns_false() {
        let notifier = create_notifier();
        let start = std::time::Instant::now();
        let got_event = notifier.wait(Some(Duration::from_millis(50))).unwrap();
        assert!(!got_event, "empty notifier wait should time out");
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "timed wait must return promptly, took {:?}",
            start.elapsed()
        );
    }
}
