//! std.net — 网络 Socket 操作
//!
//! 提供基础 TCP/UDP 客户端/服务器操作。
//! 使用 Rust 标准库 `std::net` 实现。

use crate::vm::native::NativeRegistry;
use crate::vm::value::Value;
use std::io::{Read, Write};
use std::net::TcpListener;

/// 内部 socket 句柄类型（存储为 i64）
type SocketHandle = i64;

/// 将 handle 转为 usize（安全方式：用 handle 作为索引存入全局表）
fn handle_to_usize(handle: SocketHandle) -> usize {
    handle as usize
}

pub fn register(reg: &mut NativeRegistry) {
    reg.register("net.tcpConnect", nat_tcp_connect);
    reg.register("net.tcpListen", nat_tcp_listen);
    reg.register("net.tcpSend", nat_tcp_send);
    reg.register("net.tcpRecv", nat_tcp_recv);
    reg.register("net.tcpClose", nat_tcp_close);
    reg.register("net.udpSend", nat_udp_send);
    reg.register("net.udpRecv", nat_udp_recv);
    reg.register("net.udpClose", nat_udp_close);
    reg.register("net.isHostReachable", nat_is_host_reachable);
    reg.register("net.getHostname", nat_get_hostname);
    reg.register("net.getLocalIp", nat_get_local_ip);
}

/// net.tcpConnect(host, port) → handle or error string
fn nat_tcp_connect(args: &[Value]) -> Value {
    let host = args.first().map(|v| v.as_string()).unwrap_or_default();
    let port = args.get(1).map(|v| v.as_int()).unwrap_or(0);
    let addr = format!("{}:{}", host, port);
    match std::net::TcpStream::connect(&addr) {
        Ok(stream) => {
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(1)));
            // Store socket in a global registry
            let handle = get_socket_registry().lock().unwrap().register_tcp(stream);
            Value::Int(handle as i64)
        }
        Err(e) => Value::str_(format!("TCP connect error: {}", e)),
    }
}

/// net.tcpListen(port) → handle or error string
fn nat_tcp_listen(args: &[Value]) -> Value {
    let port = args.first().map(|v| v.as_int()).unwrap_or(0);
    let addr = format!("0.0.0.0:{}", port);
    match TcpListener::bind(&addr) {
        Ok(listener) => {
            let handle = get_socket_registry().lock().unwrap().register_listener(listener);
            Value::Int(handle as i64)
        }
        Err(e) => Value::str_(format!("TCP listen error: {}", e)),
    }
}

/// net.tcpSend(handle, message) → Bool
fn nat_tcp_send(args: &[Value]) -> Value {
    if args.len() < 2 {
        return Value::Bool(false);
    }
    let handle = handle_to_usize(args[0].as_int());
    let msg = args[1].as_string();
    let registry = get_socket_registry().lock().unwrap();
    if let Some(mut stream) = registry.get_tcp(handle) {
        match stream.write_all(msg.as_bytes()) {
            Ok(_) => Value::Bool(true),
            Err(_) => Value::Bool(false),
        }
    } else {
        Value::Bool(false)
    }
}

/// net.tcpRecv(handle) → String
fn nat_tcp_recv(args: &[Value]) -> Value {
    if args.is_empty() {
        return Value::Null;
    }
    let handle = handle_to_usize(args[0].as_int());
    let mut registry = get_socket_registry().lock().unwrap();
    let result = registry.get_tcp_mut(handle).and_then(|stream| {
        let mut buf = [0u8; 4096];
        match stream.read(&mut buf) {
            Ok(n) if n > 0 => Some(Value::str_(String::from_utf8_lossy(&buf[..n]).to_string())),
            _ => None,
        }
    });
    result.unwrap_or(Value::Null)
}

/// net.tcpClose(handle) → Unit
fn nat_tcp_close(args: &[Value]) -> Value {
    if args.is_empty() {
        return Value::Null;
    }
    let handle = handle_to_usize(args[0].as_int());
    get_socket_registry().lock().unwrap().remove_tcp(handle);
    Value::Null
}

/// net.udpSend(target, port, message) → Bool
fn nat_udp_send(args: &[Value]) -> Value {
    if args.len() < 3 {
        return Value::Bool(false);
    }
    let target = args[0].as_string();
    let port = args[1].as_int();
    let msg = args[2].as_string();
    let addr = format!("{}:{}", target, port);
    match std::net::UdpSocket::bind("0.0.0.0:0") {
        Ok(socket) => {
            let ok = socket.send_to(msg.as_bytes(), &addr).is_ok();
            Value::Bool(ok)
        }
        Err(_) => Value::Bool(false),
    }
}

/// net.udpRecv(handle) → String
fn nat_udp_recv(args: &[Value]) -> Value {
    // UDP recv requires a bound socket; simplified: return null
    let _ = args;
    Value::Null
}

/// net.udpClose(handle) → Unit
fn nat_udp_close(args: &[Value]) -> Value {
    let _ = args;
    Value::Null
}

/// net.isHostReachable(host) → Bool
fn nat_is_host_reachable(args: &[Value]) -> Value {
    let host = args.first().map(|v| v.as_string()).unwrap_or_default();
    // Simple connectivity check: resolve hostname
    match std::net::ToSocketAddrs::to_socket_addrs(&(host.as_str(), 0)) {
        Ok(mut addrs) => Value::Bool(addrs.next().is_some()),
        Err(_) => Value::Bool(false),
    }
}

/// net.getHostname() → String
fn nat_get_hostname(_args: &[Value]) -> Value {
    let hostname = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown".to_string());
    Value::str_(hostname)
}

/// net.getLocalIp() → String
fn nat_get_local_ip(_args: &[Value]) -> Value {
    // Try to connect to external address to discover local IP
    if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
        let _ = socket.connect("8.8.8.8:80");
        if let Ok(addr) = socket.local_addr() {
            return Value::str_(addr.ip().to_string());
        }
    }
    Value::str_("127.0.0.1")
}

/// 全局 socket 注册表（用于安全存储 socket 句柄）
use std::sync::{Mutex, OnceLock};

pub struct SocketRegistry {
    tcp: std::collections::HashMap<usize, std::net::TcpStream>,
    listeners: std::collections::HashMap<usize, TcpListener>,
    next_id: usize,
}

impl SocketRegistry {
    pub fn new() -> Self {
        Self {
            tcp: std::collections::HashMap::new(),
            listeners: std::collections::HashMap::new(),
            next_id: 1,
        }
    }

    pub fn register_tcp(&mut self, stream: std::net::TcpStream) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.tcp.insert(id, stream);
        id
    }

    pub fn register_listener(&mut self, listener: TcpListener) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.listeners.insert(id, listener);
        id
    }

    pub fn get_tcp(&self, id: usize) -> Option<&std::net::TcpStream> {
        self.tcp.get(&id)
    }

    pub fn get_tcp_mut(&mut self, id: usize) -> Option<&mut std::net::TcpStream> {
        self.tcp.get_mut(&id)
    }

    pub fn remove_tcp(&mut self, id: usize) {
        self.tcp.remove(&id);
    }
}

pub fn get_socket_registry() -> &'static Mutex<SocketRegistry> {
    static REGISTRY: OnceLock<Mutex<SocketRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(SocketRegistry::new()))
}
