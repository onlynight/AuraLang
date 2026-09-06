/*
跨进程 Channel 后端（Phase 3）

基于 TCP Socket 的 Channel 实现，支持跨进程消息传递。
消息格式：4 字节长度前缀 + 自定义二进制编码 Value
*/
use crate::vm::serialize::{SerializeError, decode_value, encode_value};
use crate::vm::value::Value;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;
/// 全局 TCP Channel 注册表
pub static TCP_CHANNELS: LazyLock<Mutex<HashMap<usize, TcpChannelServer>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
/// TCP Channel 错误
#[derive(Debug)]
pub enum TcpChannelError {
    Io(std::io::Error),
    Serialize(SerializeError),
    NotConnected,
    TooLarge { bytes: usize },
}
impl std::fmt::Display for TcpChannelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TcpChannelError::Io(e) => write!(f, "IO: {}", e),
            TcpChannelError::Serialize(e) => write!(f, "serialize: {}", e),
            TcpChannelError::NotConnected => write!(f, "not connected"),
            TcpChannelError::TooLarge { bytes } => write!(f, "message too large: {} bytes", bytes),
        }
    }
}
impl From<std::io::Error> for TcpChannelError {
    fn from(e: std::io::Error) -> Self {
        TcpChannelError::Io(e)
    }
}
impl From<SerializeError> for TcpChannelError {
    fn from(e: SerializeError) -> Self {
        TcpChannelError::Serialize(e)
    }
}
/// TCP Channel 服务端
pub struct TcpChannelServer {
    listener: Option<TcpListener>,
    connections: Vec<TcpStream>,
    closed: Arc<AtomicBool>,
}
impl TcpChannelServer {
    pub fn new(port: u16) -> Result<Self, TcpChannelError> {
        let listener = TcpListener::bind(format!("127.0.0.1:{}", port))?;
        Ok(Self {
            listener: Some(listener),
            connections: Vec::new(),
            closed: Arc::new(AtomicBool::new(false)),
        })
    }
    pub fn new_any() -> Result<(Self, u16), TcpChannelError> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        Ok((
            Self {
                listener: Some(listener),
                connections: Vec::new(),
                closed: Arc::new(AtomicBool::new(false)),
            },
            port,
        ))
    }
    pub fn accept_client(&mut self) -> Result<TcpChannelClient, TcpChannelError> {
        let listener = self.listener.as_ref().ok_or(TcpChannelError::NotConnected)?;
        let (stream, _addr) = listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(300)));
        stream.set_nodelay(true)?;
        Ok(TcpChannelClient {
            stream: Some(stream),
            closed: self.closed.clone(),
        })
    }
    pub fn port(&self) -> u16 {
        self.listener.as_ref().and_then(|l| l.local_addr().ok()).map(|a| a.port()).unwrap_or(0)
    }
    pub fn close(&mut self) {
        self.closed.store(true, Ordering::SeqCst);
        self.connections.clear();
        self.listener.take();
    }
}
/// TCP Channel 客户端
pub struct TcpChannelClient {
    stream: Option<TcpStream>,
    closed: Arc<AtomicBool>,
}
impl TcpChannelClient {
    pub fn connect(port: u16) -> Result<Self, TcpChannelError> {
        let stream = TcpStream::connect(format!("127.0.0.1:{}", port))?;
        stream.set_read_timeout(Some(Duration::from_secs(300)));
        stream.set_nodelay(true)?;
        Ok(Self {
            stream: Some(stream),
            closed: Arc::new(AtomicBool::new(false)),
        })
    }
    pub fn from_stream(stream: TcpStream) -> Self {
        Self {
            stream: Some(stream),
            closed: Arc::new(AtomicBool::new(false)),
        }
    }
    /// 发送 Value（二进制编码）
    pub fn send_value(&mut self, val: &Value) -> Result<(), TcpChannelError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(TcpChannelError::NotConnected);
        }
        let stream = self.stream.as_mut().ok_or(TcpChannelError::NotConnected)?;
        let mut buf = Vec::with_capacity(64);
        encode_value(val, &mut buf);
        let len = buf.len() as u32;
        stream.write_all(&len.to_be_bytes())?;
        stream.write_all(&buf)?;
        Ok(())
    }
    /// 接收 Value（二进制解码）
    pub fn recv_value(&mut self) -> Result<Option<Value>, TcpChannelError> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(TcpChannelError::NotConnected);
        }
        let stream = self.stream.as_mut().ok_or(TcpChannelError::NotConnected)?;
        let mut len_buf = [0u8; 4];
        match stream.read_exact(&mut len_buf) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => return Ok(None),
            Err(e) => return Err(TcpChannelError::Io(e)),
        }
        let len = u32::from_be_bytes(len_buf) as usize;
        if len > 64 * 1024 * 1024 {
            return Err(TcpChannelError::TooLarge { bytes: len });
        }
        let mut buf = vec![0u8; len];
        stream.read_exact(&mut buf)?;
        let mut offset = 0;
        let val = decode_value(&buf, &mut offset)?;
        Ok(Some(val))
    }
    pub fn close(&mut self) {
        self.closed.store(true, Ordering::SeqCst);
        if let Some(stream) = self.stream.as_mut() {
            let _ = stream.shutdown(std::net::Shutdown::Both);
        }
    }
    pub fn is_open(&self) -> bool {
        !self.closed.load(Ordering::SeqCst) && self.stream.is_some()
    }
}
