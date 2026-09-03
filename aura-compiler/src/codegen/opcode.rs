//! 字节码指令集与模块结构（对应 技术方案 §7.1）
//!
//! 字节码采用 **栈 + 局部变量槽** 模型：
//! - `LoadConst` / `LoadVar` 将值压入操作数栈
//! - 算术/比较指令从栈顶弹出操作数、结果压回栈顶
//! - `StoreVar` 将栈顶弹出写入局部变量槽
//! - 控制流指令携带 `i32` 偏移（相对于函数 `code` 起始的绝对字节偏移）
//!
//! 该指令集与 §7.1 给出的 `OpCode` 列表保持一致（LoadConst/LoadVar/StoreVar/
//! Add/Sub/Mul/Div/Rem/Jump/JumpIfTrue/JumpIfFalse/Call/CallNative/Return/
//! NewObject/NewArray/GetField/SetField/IncRef/DecRef/CallC），并补充了
//! Neg/Not/逻辑比较/ReturnUnit/Halt 等必需指令。

use std::fmt;

/// 常量池条目
#[derive(Debug, Clone, PartialEq)]
pub enum Const {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    Null,
}

/// 字节码指令（OpCode）
#[derive(Debug, Clone, PartialEq)]
pub enum OpCode {
    /// 将常量池 `idx` 处的值压栈
    LoadConst(u16),
    /// 将局部变量槽 `slot` 的值压栈
    LoadVar(u16),
    /// 将栈顶弹出写入局部变量槽 `slot`
    StoreVar(u16),

    // ── 算术 ──
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    /// 一元取负（栈顶弹出、取负压回）
    Neg,

    // ── 逻辑/位运算 ──
    Not,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,

    // ── 比较 ──
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,

    // ── 控制流 ──
    /// 无条件跳转，操作数为 `code` 内绝对字节偏移
    Jump(i32),
    /// 弹出栈顶布尔值，为真则跳转
    JumpIfTrue(i32),
    /// 弹出栈顶布尔值，为假则跳转
    JumpIfFalse(i32),

    // ── 函数调用 ──
    /// 调用函数表中 `idx` 处函数，参数已按顺序压栈
    Call(u16),
    /// 调用原生（内置/FFI）函数表中 `idx` 处函数
    CallNative(u16),
    /// 从栈顶弹出返回值并返回
    Return,
    /// 无返回值（Unit）返回
    ReturnUnit,

    // ── 对象模型 ──
    /// 分配对象（类型表 `idx`），引用压栈
    NewObject(u16),
    /// 分配数组，长度已在栈顶
    NewArray,
    /// 取字段 `idx`，对象引用已在栈顶，结果压栈
    GetField(u16),
    /// 设置字段 `idx`，栈顶为值、其下为对象引用
    SetField(u16),
    /// 数组元素读取：栈顶为索引、其下为数组引用，结果压栈
    GetIndex,
    /// 数组元素写入：栈顶为索引、其下为数组引用、再下为待写入值
    SetIndex,

    // ── 引用计数 ──
    IncRef,
    DecRef,

    // ── FFI ──
    /// 调用 C ABI 函数表中 `idx` 处函数
    CallC(u16),

    /// 终止整个程序（顶层入口返回时）
    Halt,
}

impl OpCode {
    /// 指令的一字节操作码
    pub fn byte(&self) -> u8 {
        match self {
            OpCode::LoadConst(_) => 0,
            OpCode::LoadVar(_) => 1,
            OpCode::StoreVar(_) => 2,
            OpCode::Add => 3,
            OpCode::Sub => 4,
            OpCode::Mul => 5,
            OpCode::Div => 6,
            OpCode::Rem => 7,
            OpCode::Neg => 8,
            OpCode::Not => 9,
            OpCode::And => 10,
            OpCode::Or => 11,
            OpCode::BitAnd => 12,
            OpCode::BitOr => 13,
            OpCode::BitXor => 14,
            OpCode::Shl => 15,
            OpCode::Shr => 16,
            OpCode::Eq => 17,
            OpCode::Ne => 18,
            OpCode::Lt => 19,
            OpCode::Gt => 20,
            OpCode::Le => 21,
            OpCode::Ge => 22,
            OpCode::Jump(_) => 23,
            OpCode::JumpIfTrue(_) => 24,
            OpCode::JumpIfFalse(_) => 25,
            OpCode::Call(_) => 26,
            OpCode::CallNative(_) => 27,
            OpCode::Return => 28,
            OpCode::ReturnUnit => 29,
            OpCode::NewObject(_) => 30,
            OpCode::NewArray => 31,
            OpCode::GetField(_) => 32,
            OpCode::SetField(_) => 33,
            OpCode::GetIndex => 38,
            OpCode::SetIndex => 39,
            OpCode::IncRef => 34,
            OpCode::DecRef => 35,
            OpCode::CallC(_) => 36,
            OpCode::Halt => 37,
        }
    }

    /// 操作码携带的操作数字节数
    pub fn operand_size(byte: u8) -> usize {
        match byte {
            0 | 1 | 2 | 30 | 32 | 33 => 2, // u16 操作数
            23 | 24 | 25 => 4,            // i32 偏移
            26 | 27 | 36 => 2,            // u16 函数/原生索引
            _ => 0,
        }
    }

    pub fn from_byte(byte: u8) -> Option<Self> {
        Some(match byte {
            0 => OpCode::LoadConst(0),
            1 => OpCode::LoadVar(0),
            2 => OpCode::StoreVar(0),
            3 => OpCode::Add,
            4 => OpCode::Sub,
            5 => OpCode::Mul,
            6 => OpCode::Div,
            7 => OpCode::Rem,
            8 => OpCode::Neg,
            9 => OpCode::Not,
            10 => OpCode::And,
            11 => OpCode::Or,
            12 => OpCode::BitAnd,
            13 => OpCode::BitOr,
            14 => OpCode::BitXor,
            15 => OpCode::Shl,
            16 => OpCode::Shr,
            17 => OpCode::Eq,
            18 => OpCode::Ne,
            19 => OpCode::Lt,
            20 => OpCode::Gt,
            21 => OpCode::Le,
            22 => OpCode::Ge,
            23 => OpCode::Jump(0),
            24 => OpCode::JumpIfTrue(0),
            25 => OpCode::JumpIfFalse(0),
            26 => OpCode::Call(0),
            27 => OpCode::CallNative(0),
            28 => OpCode::Return,
            29 => OpCode::ReturnUnit,
            30 => OpCode::NewObject(0),
            31 => OpCode::NewArray,
            32 => OpCode::GetField(0),
            33 => OpCode::SetField(0),
            38 => OpCode::GetIndex,
            39 => OpCode::SetIndex,
            34 => OpCode::IncRef,
            35 => OpCode::DecRef,
            36 => OpCode::CallC(0),
            37 => OpCode::Halt,
            _ => return None,
        })
    }

    /// 将指令（含操作数）序列化到 `buf`
    pub fn write(&self, buf: &mut Vec<u8>) {
        buf.push(self.byte());
        match self {
            OpCode::LoadConst(i)
            | OpCode::LoadVar(i)
            | OpCode::StoreVar(i)
            | OpCode::NewObject(i)
            | OpCode::GetField(i)
            | OpCode::SetField(i)
            | OpCode::Call(i)
            | OpCode::CallNative(i)
            | OpCode::CallC(i) => buf.extend_from_slice(&i.to_le_bytes()),
            OpCode::Jump(o) | OpCode::JumpIfTrue(o) | OpCode::JumpIfFalse(o) => {
                buf.extend_from_slice(&o.to_le_bytes())
            }
            _ => {}
        }
    }
}

impl fmt::Display for OpCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpCode::LoadConst(i) => write!(f, "LOAD_CONST {}", i),
            OpCode::LoadVar(i) => write!(f, "LOAD_VAR {}", i),
            OpCode::StoreVar(i) => write!(f, "STORE_VAR {}", i),
            OpCode::Add => write!(f, "ADD"),
            OpCode::Sub => write!(f, "SUB"),
            OpCode::Mul => write!(f, "MUL"),
            OpCode::Div => write!(f, "DIV"),
            OpCode::Rem => write!(f, "REM"),
            OpCode::Neg => write!(f, "NEG"),
            OpCode::Not => write!(f, "NOT"),
            OpCode::And => write!(f, "AND"),
            OpCode::Or => write!(f, "OR"),
            OpCode::BitAnd => write!(f, "BITAND"),
            OpCode::BitOr => write!(f, "BITOR"),
            OpCode::BitXor => write!(f, "BITXOR"),
            OpCode::Shl => write!(f, "SHL"),
            OpCode::Shr => write!(f, "SHR"),
            OpCode::Eq => write!(f, "EQ"),
            OpCode::Ne => write!(f, "NE"),
            OpCode::Lt => write!(f, "LT"),
            OpCode::Gt => write!(f, "GT"),
            OpCode::Le => write!(f, "LE"),
            OpCode::Ge => write!(f, "GE"),
            OpCode::Jump(o) => write!(f, "JUMP {}", o),
            OpCode::JumpIfTrue(o) => write!(f, "JUMP_IF_TRUE {}", o),
            OpCode::JumpIfFalse(o) => write!(f, "JUMP_IF_FALSE {}", o),
            OpCode::Call(i) => write!(f, "CALL {}", i),
            OpCode::CallNative(i) => write!(f, "CALL_NATIVE {}", i),
            OpCode::Return => write!(f, "RETURN"),
            OpCode::ReturnUnit => write!(f, "RETURN_UNIT"),
            OpCode::NewObject(i) => write!(f, "NEW_OBJECT {}", i),
            OpCode::NewArray => write!(f, "NEW_ARRAY"),
            OpCode::GetField(i) => write!(f, "GET_FIELD {}", i),
            OpCode::SetField(i) => write!(f, "SET_FIELD {}", i),
            OpCode::GetIndex => write!(f, "GET_INDEX"),
            OpCode::SetIndex => write!(f, "SET_INDEX"),
            OpCode::IncRef => write!(f, "INC_REF"),
            OpCode::DecRef => write!(f, "DEC_REF"),
            OpCode::CallC(i) => write!(f, "CALL_C {}", i),
            OpCode::Halt => write!(f, "HALT"),
        }
    }
}

/// 原生（内置/FFI）函数签名记录
#[derive(Debug, Clone, PartialEq)]
pub struct BytecodeNative {
    pub name: String,
    pub param_count: u16,
}

/// 一个已发射的函数
#[derive(Debug, Clone, PartialEq)]
pub struct BytecodeFunction {
    pub name: String,
    pub param_count: u16,
    /// 局部变量槽总数（含参数槽 0..param_count）
    pub locals: u16,
    /// 是否为原生/FFI 函数（无 `code`）
    pub is_native: bool,
    /// 字节码（仅非原生函数）
    pub code: Vec<u8>,
}

/// 完整的字节码模块（对应 `.auc` 文件内容）
#[derive(Debug, Clone, PartialEq)]
pub struct BytecodeModule {
    pub consts: Vec<Const>,
    pub natives: Vec<BytecodeNative>,
    pub functions: Vec<BytecodeFunction>,
    /// 入口函数（通常为 `main`）在 `functions` 中的索引
    pub entry: u16,
}
