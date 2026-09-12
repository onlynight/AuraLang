/*
跨进程 IPC 传输层（Phase 3）

为 Actor/Channel 提供跨进程通信能力。
传输后端（按优先级）：
1. Unix Domain Socket (Unix)
2. Named Pipe (Windows)
3. TCP Socket (跨平台后备)

消息格式：4 字节大端序长度前缀 + 自定义二进制编码的 Value
*/
use crate::vm::serialize::{SerializeError, decode_value, encode_value};
use crate::vm::value::Value;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;
/// IPC 错误
#[derive(Debug)]
pub enum IpcError {
    Io(std::io::Error),
    Serialize(SerializeError),
    Other(String),
}
impl std::fmt::Display for IpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IpcError::Io(e) => write!(f, "IO error: {}", e),
            IpcError::Serialize(e) => write!(f, "Serialization error: {}", e),
            IpcError::Other(m) => write!(f, "{}", m),
        }
    }
}
impl std::error::Error for IpcError {}
impl From<std::io::Error> for IpcError {
    fn from(e: std::io::Error) -> Self {
        IpcError::Io(e)
    }
}
impl From<SerializeError> for IpcError {
    fn from(e: SerializeError) -> Self {
        IpcError::Serialize(e)
    }
}
/// IPC 连接
pub struct IpcConnection {
    stream: TcpStream,
}
impl IpcConnection {
    /// 发送一个 Value（4 字节长度前缀 + 二进制编码）
    pub fn send_value(&mut self, val: &Value) -> Result<(), IpcError> {
        let mut buf = Vec::with_capacity(64);
        encode_value(val, &mut buf);
        let len = buf.len() as u32;
        self.stream.write_all(&len.to_be_bytes())?;
        self.stream.write_all(&buf)?;
        Ok(())
    }
    /// 接收一个 Value
    pub fn recv_value(&mut self) -> Result<Option<Value>, IpcError> {
        let mut len_buf = [0u8; 4];
        match self.stream.read_exact(&mut len_buf) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => return Ok(None),
            Err(e) => return Err(IpcError::Io(e)),
        }
        let len = u32::from_be_bytes(len_buf) as usize;
        if len > 64 * 1024 * 1024 {
            return Err(IpcError::Other(format!("Message too large: {} bytes", len)));
        }
        let mut buf = vec![0u8; len];
        self.stream.read_exact(&mut buf)?;
        let mut offset = 0;
        let val = decode_value(&buf, &mut offset)?;
        Ok(Some(val))
    }
    /// 发送原始字节
    pub fn send_bytes(&mut self, data: &[u8]) -> Result<(), IpcError> {
        let len = data.len() as u32;
        self.stream.write_all(&len.to_be_bytes())?;
        self.stream.write_all(data)?;
        Ok(())
    }
    /// 接收原始字节
    pub fn recv_bytes(&mut self) -> Result<Option<Vec<u8>>, IpcError> {
        let mut len_buf = [0u8; 4];
        match self.stream.read_exact(&mut len_buf) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset => return Ok(None),
            Err(e) => return Err(IpcError::Io(e)),
        }
        let len = u32::from_be_bytes(len_buf) as usize;
        if len > 64 * 1024 * 1024 {
            return Err(IpcError::Other(format!("Message too large: {} bytes", len)));
        }
        let mut buf = vec![0u8; len];
        self.stream.read_exact(&mut buf)?;
        Ok(Some(buf))
    }
    pub fn close(&mut self) -> Result<(), IpcError> {
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
        Ok(())
    }
    pub fn is_open(&self) -> bool {
        self.stream.peer_addr().is_ok()
    }
}
/// IPC 服务器（监听端口）
pub struct IpcServer {
    listener: TcpListener,
}
impl IpcServer {
    pub fn listen(port: u16) -> Result<Self, IpcError> {
        let addr = format!("127.0.0.1:{}", port);
        let listener = TcpListener::bind(&addr)?;
        Ok(IpcServer { listener })
    }
    pub fn listen_any() -> Result<(Self, u16), IpcError> {
        let probe = TcpListener::bind("127.0.0.1:0")?;
        let port = probe.local_addr()?.port();
        drop(probe);
        let listener = TcpListener::bind(format!("127.0.0.1:{}", port))?;
        Ok((IpcServer { listener }, port))
    }
    pub fn accept(&self) -> Result<IpcConnection, IpcError> {
        let (stream, _addr) = self.listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(300)));
        stream.set_nodelay(true)?;
        Ok(IpcConnection { stream })
    }
    pub fn port(&self) -> u16 {
        self.listener.local_addr().map(|a| a.port()).unwrap_or(0)
    }
    pub fn set_accept_timeout(&mut self, _timeout: Option<Duration>) -> Result<(), IpcError> {
        Ok(())
    }
}
/// 连接到 IPC 服务器
pub fn connect(port: u16) -> Result<IpcConnection, IpcError> {
    let addr = format!("127.0.0.1:{}", port);
    let stream = TcpStream::connect(&addr)?;
    stream.set_read_timeout(Some(Duration::from_secs(300)));
    stream.set_nodelay(true)?;
    Ok(IpcConnection { stream })
}
/// 跨进程 Actor 消息包装
#[derive(Debug, Clone)]
pub struct ActorMessage {
    pub target_actor: usize,
    pub msg_type: String,
    pub payload: Value,
}
/// 跨进程 Channel 消息包装
#[derive(Debug, Clone)]
pub struct ChannelMessage {
    pub channel_id: usize,
    pub op: ChannelOp,
    pub payload: Option<Value>,
}
/// Channel 操作类型
#[derive(Debug, Clone)]
pub enum ChannelOp {
    Send(Value),
    Recv,
    TryRecv,
}
