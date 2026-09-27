/*
跨进程 Actor 后端（Phase 3）

在子进程中运行 Actor，通过 TCP 管道与父进程通信。
*/
use crate::vm::serialize::{SerializeError, decode_value, encode_value};
use crate::vm::value::Value;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;
/// 全局跨进程 Actor 注册表
pub static PROCESS_ACTORS: LazyLock<Mutex<HashMap<usize, ProcessActor>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
/// 跨进程 Actor 错误
#[derive(Debug)]
pub enum ProcessActorError {
    Io(std::io::Error),
    Spawn(std::io::Error),
    Serialize(SerializeError),
    NotConnected,
}
impl std::fmt::Display for ProcessActorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProcessActorError::Io(e) => write!(f, "IO: {}", e),
            ProcessActorError::Spawn(e) => write!(f, "spawn failed: {}", e),
            ProcessActorError::Serialize(e) => write!(f, "serialize: {}", e),
            ProcessActorError::NotConnected => write!(f, "not connected"),
        }
    }
}
impl From<std::io::Error> for ProcessActorError {
    fn from(e: std::io::Error) -> Self {
        ProcessActorError::Io(e)
    }
}
impl From<SerializeError> for ProcessActorError {
    fn from(e: SerializeError) -> Self {
        ProcessActorError::Serialize(e)
    }
}
/// 跨进程 Actor 实例
pub struct ProcessActor {
    child: Option<Child>,
    stream: Option<TcpStream>,
    pub port: u16,
    closed: Arc<AtomicBool>,
    pub name: String,
}
impl ProcessActor {
    pub fn spawn(entry_file: &str, name: &str) -> Result<(Self, u16), ProcessActorError> {
        let args = vec![
            "run".to_string(),
            "--ipc-actor".to_string(),
            entry_file.to_string(),
        ];
        Self::spawn_with_args(&args, name)
    }
    pub fn spawn_with_args(args: &[String], name: &str) -> Result<(Self, u16), ProcessActorError> {
        let exe = find_aura_exe()?;
        let child = Command::new(&exe)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(ProcessActorError::Spawn)?;
        Ok((
            Self {
                child: Some(child),
                stream: None,
                port: 0,
                closed: Arc::new(AtomicBool::new(false)),
                name: name.to_string(),
            },
            0,
        ))
    }
    pub fn connect(port: u16) -> Result<Self, ProcessActorError> {
        let stream = TcpStream::connect(format!("127.0.0.1:{}", port))?;
        stream.set_read_timeout(Some(Duration::from_secs(300)));
        stream.set_nodelay(true)?;
        Ok(Self {
            child: None,
            stream: Some(stream),
            port,
            closed: Arc::new(AtomicBool::new(false)),
            name: "connected".to_string(),
        })
    }
    /// 发送消息（二进制编码）
    pub fn send(&mut self, msg: &Value) -> Result<(), ProcessActorError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(ProcessActorError::NotConnected);
        }
        let stream = self.stream.as_mut().ok_or(ProcessActorError::NotConnected)?;
        let mut buf = Vec::with_capacity(64);
        encode_value(msg, &mut buf);
        let len = buf.len() as u32;
        stream.write_all(&len.to_be_bytes())?;
        stream.write_all(&buf)?;
        Ok(())
    }
    /// 接收响应（二进制解码）
    pub fn recv(&mut self) -> Result<Option<Value>, ProcessActorError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(ProcessActorError::NotConnected);
        }
        let stream = self.stream.as_mut().ok_or(ProcessActorError::NotConnected)?;
        let mut len_buf = [0u8; 4];
        match stream.read_exact(&mut len_buf) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => return Ok(None),
            Err(e) => return Err(ProcessActorError::Io(e)),
        }
        let len = u32::from_be_bytes(len_buf) as usize;
        if len > 64 * 1024 * 1024 {
            return Err(ProcessActorError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Message too large: {} bytes", len),
            )));
        }
        let mut buf = vec![0u8; len];
        stream.read_exact(&mut buf)?;
        let mut offset = 0;
        let val = decode_value(&buf, &mut offset)?;
        Ok(Some(val))
    }
    pub fn is_alive(&mut self) -> bool {
        if self.closed.load(Ordering::SeqCst) {
            return false;
        }
        if let Some(child) = self.child.as_mut() {
            match child.try_wait() {
                Ok(Some(_)) => {
                    self.closed.store(true, Ordering::SeqCst);
                    false
                }
                Ok(None) => true,
                Err(_) => false,
            }
        } else {
            self.stream.is_some()
        }
    }
    pub fn kill(&mut self) {
        self.closed.store(true, Ordering::SeqCst);
        if let Some(stream) = self.stream.as_mut() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
impl Drop for ProcessActor {
    fn drop(&mut self) {
        self.kill();
    }
}
fn find_aura_exe() -> Result<String, ProcessActorError> {
    use std::path::PathBuf;
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let name = if cfg!(target_os = "windows") { "aura.exe" } else { "aura" };
            let candidate = dir.join(name);
            if candidate.exists() {
                return Ok(candidate.to_string_lossy().to_string());
            }
        }
    }
    if let Ok(path) = std::env::var("AURA_BIN") {
        return Ok(path);
    }
    let exe_name = if cfg!(target_os = "windows") { "aura.exe" } else { "aura" };
    if let Ok(paths) = std::env::var("PATH") {
        let sep = if cfg!(target_os = "windows") { ";" } else { ":" };
        for dir in paths.split(sep) {
            let candidate = PathBuf::from(dir).join(exe_name);
            if candidate.exists() {
                return Ok(candidate.to_string_lossy().to_string());
            }
        }
    }
    Err(ProcessActorError::Io(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "Cannot find aura executable",
    )))
}
