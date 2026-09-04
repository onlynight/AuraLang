//! Aura VM 运行时值（`Value`）
//!
//! 对应 技术方案 §4 / §7.1 的对象模型。VM 采用 **带标签的联合（tagged union）**
//! 表示运行时值：基础类型内联存储，堆对象通过 `Ref(usize)` 句柄引用 `Vm` 的堆区。
//!
//! 设计说明：字节码（`opcode.rs`）为栈式模型，因此 `Value` 需在操作数栈上被
//! 频繁拷贝；所有变体均为 `Copy` 友好的轻量类型（`Str` 用 `Rc<str>` 共享，避免拷贝）。

use std::fmt;
use std::rc::Rc;

/// 运行时值
#[derive(Clone, Debug)]
pub enum Value {
    /// 64 位有符号整数（对应 `Int` / `Long`）
    Int(i64),
    /// 64 位浮点（对应 `Float` / `Double`）
    Float(f64),
    /// 布尔值
    Bool(bool),
    /// UTF-8 字符串（引用计数共享，零拷贝传递）
    Str(Rc<str>),
    /// 空值（同时充当 `Unit` / `Nothing` 的运行时表示）
    Null,
    /// 堆对象 / 数组句柄（索引到 `Vm::heap`）
    Ref(usize),
    /// 弱引用句柄（不增加引用计数，P7.3）
    Weak(usize),
    /// 原始指针（C ABI 互操作，P8.6）。`0` = `nullptr`。
    /// 用于 `Pointer<T>` / `CString` / `CStr` / `Handle` 等 FFI 类型。
    Ptr(i64),
}

impl Value {
    /// 构造字符串值
    pub fn str_(s: impl Into<String>) -> Self {
        Value::Str(Rc::from(s.into().as_str()))
    }

    /// 是否为“真”（用于跳转判断）
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::Float(f) => *f != 0.0,
            Value::Null => false,
            Value::Str(s) => !s.is_empty(),
            Value::Ref(_) => true,
            Value::Weak(_) => false,
            // 非空指针为真，nullptr 为假
            Value::Ptr(p) => *p != 0,
        }
    }

    /// 是否为 `nullptr`（仅对指针类型有意义，P8.6）
    pub fn is_null_ptr(&self) -> bool {
        match self {
            Value::Ptr(0) | Value::Null => true,
            _ => false,
        }
    }

    /// 取出整数（非整数类型返回 0，便于容错执行）
    pub fn as_int(&self) -> i64 {
        match self {
            Value::Int(i) => *i,
            Value::Float(f) => *f as i64,
            Value::Bool(b) => *b as i64,
            Value::Ptr(p) => *p,
            _ => 0,
        }
    }

    /// 取出浮点（整型自动提升）
    pub fn as_float(&self) -> f64 {
        match self {
            Value::Float(f) => *f,
            Value::Int(i) => *i as f64,
            Value::Bool(b) => *b as i64 as f64,
            _ => 0.0,
        }
    }

    pub fn as_bool(&self) -> bool {
        self.is_truthy()
    }

    /// 取出指针地址（仅对 `Value::Ptr` 有意义，其他类型返回 0）
    pub fn as_ptr(&self) -> i64 {
        match self {
            Value::Ptr(p) => *p,
            _ => 0,
        }
    }

    /// 构造指针值
    pub fn ptr(p: i64) -> Self {
        Value::Ptr(p)
    }

    /// 取出字符串内容（非字符串类型返回其 Display 文本）
    pub fn as_string(&self) -> String {
        match self {
            Value::Str(s) => s.to_string(),
            other => format!("{}", other),
        }
    }

    /// 类型标签（用于反射 / 诊断）
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Int(_) => "Int",
            Value::Float(_) => "Float",
            Value::Bool(_) => "Boolean",
            Value::Str(_) => "String",
            Value::Null => "Null",
            Value::Ref(_) => "Ref",
            Value::Weak(_) => "Weak",
            Value::Ptr(_) => "Pointer",
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Null, Value::Null) => true,
            (Value::Ref(a), Value::Ref(b)) => a == b,
            // 跨数值类型比较（整 / 浮 互通）
            (Value::Int(a), Value::Float(b)) => (*a as f64) == *b,
            (Value::Float(a), Value::Int(b)) => *a == (*b as f64),
            (Value::Ptr(a), Value::Ptr(b)) => a == b,
            // 指针与 null 比较（nullptr 等价于 Null）
            (Value::Ptr(0), Value::Null) | (Value::Null, Value::Ptr(0)) => true,
            _ => false,
        }
    }
}

impl Eq for Value {}

impl std::hash::Hash for Value {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Value::Int(i) => i.hash(state),
            Value::Float(f) => f.to_bits().hash(state),
            Value::Bool(b) => b.hash(state),
            Value::Str(s) => s.hash(state),
            Value::Null => {}
            Value::Ref(h) => h.hash(state),
            Value::Weak(h) => h.hash(state),
            Value::Ptr(p) => p.hash(state),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(i) => write!(f, "{}", i),
            Value::Float(fl) => {
                // 避免 3.0 显示为 3（保留小数），但整数值浮点仍显示为整数形态
                if fl.fract() == 0.0 && fl.abs() < 1e15 {
                    write!(f, "{:.1}", fl)
                } else {
                    write!(f, "{}", fl)
                }
            }
            Value::Bool(b) => write!(f, "{}", b),
            Value::Str(s) => write!(f, "{}", s),
            Value::Null => write!(f, "null"),
            Value::Ref(h) => write!(f, "<ref#{}>", h),
            Value::Weak(h) => write!(f, "<weak#{}>", h),
            Value::Ptr(p) => {
                if *p == 0 {
                    write!(f, "nullptr")
                } else {
                    write!(f, "<ptr#{p:x}>")
                }
            }
        }
    }
}
