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
    /// 调用原生函数，带实际参数个数（用于变长函数如 listOf）
    CallNativeArgs(u16, u16),
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

    // ── 方法 / 接口调用（5.6） ──
    /// 虚方法调用：栈顶为对象引用、其下为方法表 `idx`，对象 vtable 中查方法并调用
    CallMethod(u16),
    /// 构造器调用：为已分配对象设置字段后返回引用（与 Call 相同语义，但确保对象已被 NewObject 分配）
    CallCtor(u16),

    // ── 类型检查（Phase 2） ──
    /// 实例类型检查：栈顶为值引用，`type_id` 为目标类 ID，结果 Boolean 压栈
    /// 沿继承链向上查找，支持子类→父类检查
    InstanceOf(u16),
    /// 类型转换：栈顶为值引用，`type_id` 为目标类 ID，匹配则压栈原引用，不匹配则报错
    CheckCast(u16),

    // ── 集合类型（5.7） ──
    /// 分配 List（栈顶为初始长度，结果引用压栈）
    NewList,
    /// 分配 Map（结果引用压栈）
    NewMap,
    /// List 尾部追加元素（栈：元素、List 引用）
    ListPush,
    /// List 弹出尾部元素并压栈（栈：List 引用）
    ListPop,
    /// List 长度压栈（栈：List 引用）
    ListLen,
    /// Map 插入键值对（栈：值、键、Map 引用）
    MapSet,
    /// Map 查找键并压栈值（栈：键、Map 引用）
    MapGet,
    /// Map 长度压栈（栈：Map 引用）
    MapLen,

    // ── 协程（5.8） ──
    /// 当前协程挂起（返回栈顶值给调度器），协程状态保存
    Yield,
    /// 创建协程（栈顶为入口函数索引，返回协程 ID 压栈）
    NewCoroutine(u16),
    /// 恢复协程运行（栈顶为协程 ID、其下为入参），返回值压栈
    ResumeCoroutine,

    // ── ARC 生命周期（5.10） ──
    /// 显式释放堆对象（触发 drop 回调，置槽为空）
    DropRef,

    // ── P7 内存管理 ──
    /// 保留引用计数（P7.2）：对栈顶引用值 +1
    Retain,
    /// 释放引用计数（P7.2）：对栈顶引用值 -1
    Release,
    /// 创建弱引用（P7.3）：栈顶引用值 → 弱引用（不增加计数）
    WeakRef,
    /// 从弱引用升级（P7.3）：栈顶弱引用 → 强引用（若未释放则 +1）
    WeakGet,
    /// 显式堆分配（P7.5）：栈顶值分配到堆上并返回引用
    BoxAlloc,
    /// defer 清理块开始（P7.4）：标记 defer 区域起始
    DeferBegin,
    /// defer 清理块结束（P7.4）：标记 defer 区域结束
    DeferEnd,

    /// 终止整个程序（顶层入口返回时）
    Halt,

    // ── FFI（P8）──
    /// 将栈顶字符串转换为 C 字符串指针（`const char*`），结果压栈
    CString,
    /// 从栈顶的 C 字符串指针读取字符串（`const char*` → `String`），结果压栈
    ReadCStr,
    /// 栈顶指针是否为 `nullptr`（压入 `Bool`）
    PtrIsNull,
    /// 将栈顶指针转换为整数地址（压入 `Int`）
    PtrToInt,
    /// 将栈顶整数地址转换为指针（压入 `Ptr`）
    IntToPtr,
    /// 创建 C 回调蹦床（栈顶为函数索引，压入 `Ptr` 回调地址）
    MakeCallback(u16),

    // ── Phase 2: 闭包 ──
    /// 创建闭包（索引到 `closures` 表），栈顶为捕获值（按序）
    MakeClosure(u16),
    /// 调用闭包（栈顶为参数...、其下为闭包引用）
    CallClosure,
    // ── Phase 3: 枚举 ──
    /// 构造枚举变体（操作数为枚举定义索引）
    EnumConstruct(u16),
    /// 获取枚举变体索引（栈顶弹出枚举值，压入变体索引）
    EnumTag,
    /// 创建函数引用（操作数为函数索引）
    MakeFnRef(u16),

    // ── Phase 2: 跨模块调用 ──
    /// 调用同模块内导出符号，`sym_idx` 为导出符号表索引
    CallExport(u16),
    /// 调用外部模块符号，`(mod_idx, sym_idx)` 指向 imports 表
    CallExternal(u16, u16),

    // ── Phase 1: AOT 嵌入调用（docs/AOT机器码嵌入方案-详细设计.md §7.3）──
    /// 调用 AOT 预编译的函数，`func_idx` 为函数表索引
    ///
    /// 该指令的分派入口与 `CallJit` 完全对称：VM 分发器查询
    /// [`crate::vm::aot_runtime::AotRuntime`] 的 dispatch_table，命中则直接
    /// `call` 到 mmap 的机器码（共享 JitValue ABI），否则回退字节码解释。
    CallAot(u16),

    // ── 异常处理（try/catch）──
    /// 注册异常处理器：操作数为**处理器块的绝对字节偏移** + 异常值落点槽位。
    ///
    /// VM 收到该指令时把 `(当前帧, 目标 ip, 栈高, 槽位)` 压入 handler 栈；`throw`
    /// 触发时弹出最近的 handler，展开到对应帧、把异常值写入槽位后跳转过去。
    PushHandler(i32, u16),
    /// 注销最近的异常处理器（try 块正常结束时执行）。
    PopHandler,
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
            OpCode::CallNativeArgs(_, _) => 78,
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
            OpCode::CallMethod(_) => 40,
            OpCode::CallCtor(_) => 41,
            OpCode::InstanceOf(_) => 80,
            OpCode::CheckCast(_) => 81,
            OpCode::NewList => 42,
            OpCode::NewMap => 43,
            OpCode::ListPush => 44,
            OpCode::ListPop => 45,
            OpCode::ListLen => 46,
            OpCode::MapSet => 47,
            OpCode::MapGet => 48,
            OpCode::MapLen => 49,
            OpCode::Yield => 50,
            OpCode::NewCoroutine(_) => 51,
            OpCode::ResumeCoroutine => 52,
            OpCode::DropRef => 53,
            OpCode::Retain => 54,
            OpCode::Release => 55,
            OpCode::WeakRef => 56,
            OpCode::WeakGet => 57,
            OpCode::BoxAlloc => 58,
            OpCode::DeferBegin => 59,
            OpCode::DeferEnd => 60,
            OpCode::Halt => 37,
            OpCode::CString => 61,
            OpCode::ReadCStr => 62,
            OpCode::PtrIsNull => 63,
            OpCode::PtrToInt => 64,
            OpCode::IntToPtr => 65,
            OpCode::MakeCallback(_) => 66,
            OpCode::MakeClosure(_) => 72,
            OpCode::CallClosure => 73,
            OpCode::EnumConstruct(_) => 74,
            OpCode::EnumTag => 75,
            OpCode::MakeFnRef(_) => 76,
            OpCode::CallExport(_) => 70,
            OpCode::CallExternal(_, _) => 71,
            OpCode::CallAot(_) => 77,
            OpCode::PushHandler(..) => 82,
            OpCode::PopHandler => 83,
        }
    }

    /// 操作码携带的操作数字节数
    pub fn operand_size(byte: u8) -> usize {
        match byte {
            0 | 1 | 2 | 30 | 32 | 33 | 72 | 74 | 76 | 77 => 2, // u16 操作数
            23 | 24 | 25 => 4,                                 // i32 偏移
            82 => 6, // PushHandler: i32 处理器块偏移 + u16 异常值槽位
            // ⚠ 必须与 `write` 实际写出的操作数字节数一致：解码器用本表
            // 推进指令游标并构建「字节偏移 → 指令索引」映射，长度不符会导致
            // 其后所有指令边界错位。
            26 | 27 | 36 => 2,           // u16 函数/原生索引
            78 => 4,                     // CallNativeArgs: u16 idx + u16 argc
            40 | 41 | 51 | 80 | 81 => 2, // CallMethod/CallCtor/NewCoroutine/InstanceOf/CheckCast u16 索引
            66 => 2,                     // MakeCallback u16 函数索引
            70 => 2,                     // CallExport u16 sym_idx
            71 => 4,                     // CallExternal u16 mod_idx + u16 sym_idx
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
            40 => OpCode::CallMethod(0),
            41 => OpCode::CallCtor(0),
            80 => OpCode::InstanceOf(0),
            81 => OpCode::CheckCast(0),
            42 => OpCode::NewList,
            43 => OpCode::NewMap,
            44 => OpCode::ListPush,
            45 => OpCode::ListPop,
            46 => OpCode::ListLen,
            47 => OpCode::MapSet,
            48 => OpCode::MapGet,
            49 => OpCode::MapLen,
            50 => OpCode::Yield,
            51 => OpCode::NewCoroutine(0),
            52 => OpCode::ResumeCoroutine,
            53 => OpCode::DropRef,
            54 => OpCode::Retain,
            55 => OpCode::Release,
            56 => OpCode::WeakRef,
            57 => OpCode::WeakGet,
            58 => OpCode::BoxAlloc,
            59 => OpCode::DeferBegin,
            60 => OpCode::DeferEnd,
            61 => OpCode::CString,
            62 => OpCode::ReadCStr,
            63 => OpCode::PtrIsNull,
            64 => OpCode::PtrToInt,
            65 => OpCode::IntToPtr,
            66 => OpCode::MakeCallback(0),
            72 => OpCode::MakeClosure(0),
            73 => OpCode::CallClosure,
            74 => OpCode::EnumConstruct(0),
            75 => OpCode::EnumTag,
            76 => OpCode::MakeFnRef(0),
            70 => OpCode::CallExport(0),
            71 => OpCode::CallExternal(0, 0),
            77 => OpCode::CallAot(0),
            78 => OpCode::CallNativeArgs(0, 0),
            82 => OpCode::PushHandler(0, 0),
            83 => OpCode::PopHandler,
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
            | OpCode::CallC(i)
            | OpCode::CallMethod(i)
            | OpCode::CallCtor(i)
            | OpCode::InstanceOf(i)
            | OpCode::CheckCast(i)
            | OpCode::NewCoroutine(i)
            | OpCode::MakeCallback(i)
            | OpCode::MakeClosure(i)
            | OpCode::EnumConstruct(i)
            | OpCode::MakeFnRef(i)
            | OpCode::CallExport(i)
            | OpCode::CallAot(i) => buf.extend_from_slice(&i.to_le_bytes()),
            OpCode::CallNativeArgs(idx, argc) => {
                buf.extend_from_slice(&idx.to_le_bytes());
                buf.extend_from_slice(&argc.to_le_bytes());
            }
            OpCode::CallExternal(mod_idx, sym_idx) => {
                buf.extend_from_slice(&mod_idx.to_le_bytes());
                buf.extend_from_slice(&sym_idx.to_le_bytes());
            }
            OpCode::Jump(o) | OpCode::JumpIfTrue(o) | OpCode::JumpIfFalse(o) => {
                buf.extend_from_slice(&o.to_le_bytes())
            }
            OpCode::PushHandler(o, slot) => {
                buf.extend_from_slice(&o.to_le_bytes());
                buf.extend_from_slice(&slot.to_le_bytes());
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
            OpCode::PushHandler(o, slot) => write!(f, "PUSH_HANDLER {} slot={}", o, slot),
            OpCode::PopHandler => write!(f, "POP_HANDLER"),
            OpCode::Call(i) => write!(f, "CALL {}", i),
            OpCode::CallNative(i) => write!(f, "CALL_NATIVE {}", i),
            OpCode::CallNativeArgs(i, argc) => write!(f, "CALL_NATIVE_ARGS {} argc={}", i, argc),
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
            OpCode::CallMethod(i) => write!(f, "CALL_METHOD {}", i),
            OpCode::CallCtor(i) => write!(f, "CALL_CTOR {}", i),
            OpCode::InstanceOf(i) => write!(f, "INSTANCE_OF {}", i),
            OpCode::CheckCast(i) => write!(f, "CHECK_CAST {}", i),
            OpCode::NewList => write!(f, "NEW_LIST"),
            OpCode::NewMap => write!(f, "NEW_MAP"),
            OpCode::ListPush => write!(f, "LIST_PUSH"),
            OpCode::ListPop => write!(f, "LIST_POP"),
            OpCode::ListLen => write!(f, "LIST_LEN"),
            OpCode::MapSet => write!(f, "MAP_SET"),
            OpCode::MapGet => write!(f, "MAP_GET"),
            OpCode::MapLen => write!(f, "MAP_LEN"),
            OpCode::Yield => write!(f, "YIELD"),
            OpCode::NewCoroutine(i) => write!(f, "NEW_COROUTINE {}", i),
            OpCode::ResumeCoroutine => write!(f, "RESUME_COROUTINE"),
            OpCode::DropRef => write!(f, "DROP_REF"),
            OpCode::Retain => write!(f, "RETAIN"),
            OpCode::Release => write!(f, "RELEASE"),
            OpCode::WeakRef => write!(f, "WEAK_REF"),
            OpCode::WeakGet => write!(f, "WEAK_GET"),
            OpCode::BoxAlloc => write!(f, "BOX_ALLOC"),
            OpCode::DeferBegin => write!(f, "DEFER_BEGIN"),
            OpCode::DeferEnd => write!(f, "DEFER_END"),
            OpCode::Halt => write!(f, "HALT"),
            OpCode::CString => write!(f, "CSTRING"),
            OpCode::ReadCStr => write!(f, "READ_CSTR"),
            OpCode::PtrIsNull => write!(f, "PTR_IS_NULL"),
            OpCode::PtrToInt => write!(f, "PTR_TO_INT"),
            OpCode::IntToPtr => write!(f, "INT_TO_PTR"),
            OpCode::MakeCallback(i) => write!(f, "MAKE_CALLBACK {}", i),
            OpCode::MakeClosure(i) => write!(f, "MAKE_CLOSURE {}", i),
            OpCode::CallClosure => write!(f, "CALL_CLOSURE"),
            OpCode::EnumConstruct(i) => write!(f, "ENUM_CONSTRUCT {}", i),
            OpCode::EnumTag => write!(f, "ENUM_TAG"),
            OpCode::MakeFnRef(i) => write!(f, "MAKE_FN_REF {}", i),
            OpCode::CallExport(i) => write!(f, "CALL_EXPORT {}", i),
            OpCode::CallExternal(mod_idx, sym_idx) => {
                write!(f, "CALL_EXTERNAL ({}, {})", mod_idx, sym_idx)
            }
            OpCode::CallAot(i) => write!(f, "CALL_AOT {}", i),
        }
    }
}

/// FFI ABI 类型（P8-Rust）
///
/// 标记 `extern` 块的目标 ABI。
/// - `C`：C ABI（`extern "c"`，现状）
/// - `Rust`：Rust 库标记（`extern "rust"`，调用约定同 C，语义标记）
/// - `Aura`：AOT 直调（`extern interface`，JitValue ABI）
/// - `None`：非 FFI 函数
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FfiAbi {
    #[default]
    None, // 非 FFI 函数
    C,    // C ABI
    Rust, // Rust 库（语法标记，调用约定同 C）
    Aura, // AOT 直调（JitValue ABI）
}

/// 原生（内置/FFI）函数签名记录
#[derive(Debug, Clone, PartialEq)]
pub struct BytecodeNative {
    pub name: String,
    pub param_count: u16,
    /// FFI ABI 标记（P8-Rust）：仅 FFI 函数有意义
    pub ffi_abi: FfiAbi,
    /// FFI 库名（对应 `extern "<abi>" "<lib>"`）
    pub ffi_lib: Option<String>,
    /// 参数类型列表（u8 类型 ID，对应 CType）
    pub param_types: Vec<u8>,
    /// 返回类型（u8 类型 ID，对应 CType）
    pub ret_type: u8,
}

/// 类型 ID 常量（用于 BytecodeNative 的 param_types/ret_type）
pub const TYPE_ID_I32: u8 = 0;
pub const TYPE_ID_I64: u8 = 1;
pub const TYPE_ID_F64: u8 = 2;
pub const TYPE_ID_BOOL: u8 = 3;
pub const TYPE_ID_CSTRING: u8 = 4;
pub const TYPE_ID_PTR: u8 = 5;
pub const TYPE_ID_VOID: u8 = 6;

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
    /// Phase 4: 指令索引 → 源码行号映射表
    ///
    /// 每个条目 `(instr_index, source_line)` 记录一条指令对应的源码行号。
    /// 仅对非原生函数有效；原生函数为 `None`。
    /// 为 `None` 时断点回退到函数入口。
    pub line_table: Option<Vec<(usize, usize)>>,
    /// Phase 1 AOT: 执行模式（设计文档 §3.4）
    /// - `0` = 仅字节码
    /// - `1` = 仅 AOT 机器码
    /// - `2` = 混合（Phase 1 不实现混合分派）
    pub aot_mode: u8,
    /// Phase 1 AOT: 函数描述符表索引（`aot_mode == 0` 时为 0）
    pub aot_desc_idx: u32,
}

impl Default for BytecodeFunction {
    fn default() -> Self {
        BytecodeFunction {
            name: String::new(),
            param_count: 0,
            locals: 0,
            is_native: false,
            code: Vec::new(),
            line_table: None,
            aot_mode: 0,
            aot_desc_idx: 0,
        }
    }
}

/// 闭包记录（Phase 2）
#[derive(Debug, Clone, PartialEq)]
pub struct BytecodeClosure {
    pub name: String,
    pub param_count: u16,
    pub locals: u16,
    pub capture_count: u16,
    /// 闭包函数在函数表中的索引
    pub func_idx: u16,
}

// ─────────────────────────────────────────────────────────────────────────────
// Phase 1: AOT 嵌入 —— 段表 / 函数描述符（docs/AOT机器码嵌入方案-详细设计.md §3.5 §5）
// ─────────────────────────────────────────────────────────────────────────────

/// header_flags 位定义（.auc v4）
pub const HEADER_HAS_MACHINE_CODE: u32 = 1 << 0;
pub const HEADER_HAS_DEBUG_INFO: u32 = 1 << 1;
pub const HEADER_SIGNED: u32 = 1 << 2;
pub const HEADER_AOT_EXPORTS: u32 = 1 << 3;
/// Phase 1: 含类定义表（v6）
pub const HEADER_HAS_CLASS_DEFS: u32 = 1 << 4;
/// Phase 2: 含源码索引（source_index 段，供 LSP 使用，VM/JIT/AOT 不读取）
pub const HEADER_HAS_SOURCE_INDEX: u32 = 1 << 5;

/// 段 ID 枚举
pub const SEG_BYTECODE: u32 = 0;
pub const SEG_MACHINE: u32 = 1;
pub const SEG_DESC_TABLE: u32 = 2;
pub const SEG_DEBUG: u32 = 3;
pub const SEG_STRING_POOL: u32 = 4;
pub const SEG_SIGNATURE: u32 = 5;

/// 段内存权限标志
pub const SEG_PROT_READ: u32 = 1 << 0;
pub const SEG_PROT_WRITE: u32 = 1 << 1;
pub const SEG_PROT_EXEC: u32 = 1 << 2;
pub const SEG_READONLY: u32 = 1 << 3;

/// `.auc` v4 段表条目（16 字节，紧凑对齐）
///
/// `offset` 相对于段数据区起始（即段表之后的字节），`size` 为段大小。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct AucSegment {
    pub id: u32,
    pub offset: u32,
    pub size: u32,
    pub flags: u32,
}

impl AucSegment {
    /// 段 ID 对应的字符串名称（诊断用）
    pub fn id_name(&self) -> &'static str {
        match self.id {
            SEG_BYTECODE => "bytecode",
            SEG_MACHINE => "machine",
            SEG_DESC_TABLE => "desc_table",
            SEG_DEBUG => "debug",
            SEG_STRING_POOL => "string_pool",
            SEG_SIGNATURE => "signature",
            _ => "unknown",
        }
    }

    /// 是否可执行（机器码段）
    pub fn is_exec(&self) -> bool {
        self.flags & SEG_PROT_EXEC != 0
    }

    /// 是否只读
    pub fn is_read(&self) -> bool {
        self.flags & SEG_PROT_READ != 0
    }

    /// 是否可写
    pub fn is_write(&self) -> bool {
        self.flags & SEG_PROT_WRITE != 0
    }
}

/// AOT 函数描述符（C ABI，32 字节，x86-64 / ARM64 对齐一致）
///
/// 布局见设计文档 §5.1：
/// ```text
/// 0x00 name_offset        u32
/// 0x04 name_len           u16
/// 0x06 _pad1              u16   ← u64 对齐填充
/// 0x08 entry_offset       u64
/// 0x10 num_args           u8
/// 0x11 arg_tags           u8
/// 0x12 return_tag         u8
/// 0x13 flags              u8
/// 0x14 source_line        u32
/// 0x18 source_file_offset u32
/// 0x1C _pad2              u32   ← 对齐到 32 字节
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AuraFuncDesc {
    /// 函数名在字符串池中的偏移
    pub name_offset: u32,
    /// 函数名长度（不含 null 终止符）
    pub name_len: u16,
    /// u64 对齐填充
    pub _pad1: u16,
    /// 机器码段内的入口偏移
    pub entry_offset: u64,
    /// 参数个数
    pub num_args: u8,
    /// 参数类型标签位图（bit-packed，每参数 4 bit）
    pub arg_tags: u8,
    /// 返回类型标签
    pub return_tag: u8,
    /// 标志位
    pub flags: u8,
    /// 源文件行号（0 = 无调试信息）
    pub source_line: u32,
    /// 源文件名在字符串池中的偏移（0 = 无调试信息）
    pub source_file_offset: u32,
    /// 对齐填充
    pub _pad2: u32,
}

// ── AuraFuncDesc.flags 位定义 ──
pub const FUNC_EXPORT: u8 = 1 << 0;
pub const FUNC_INIT: u8 = 1 << 1;
pub const FUNC_FINALIZE: u8 = 1 << 2;
pub const FUNC_SUSPEND: u8 = 1 << 3;
pub const FUNC_ASYNC: u8 = 1 << 4;
pub const FUNC_CONST: u8 = 1 << 5;
pub const FUNC_THREAD_SAFE: u8 = 1 << 6;
pub const FUNC_HOT: u8 = 1 << 7;

impl AuraFuncDesc {
    /// 结构体大小（编译期断言：必须为 32 字节）
    pub const SIZE: usize = std::mem::size_of::<Self>();

    pub fn is_export(&self) -> bool {
        self.flags & FUNC_EXPORT != 0
    }

    pub fn is_init(&self) -> bool {
        self.flags & FUNC_INIT != 0
    }

    pub fn is_finalize(&self) -> bool {
        self.flags & FUNC_FINALIZE != 0
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 类定义（Phase 1: 类 ID 系统 + 类层级）
// ─────────────────────────────────────────────────────────────────────────────

/// 无父类标记（顶级类型 Any / 根类）
pub const NO_PARENT: u16 = 0xFFFF;

/// 类的字节码定义（编译时分配递增 ID，替代 FNV 哈希）
///
/// 每个类在 `BytecodeModule::classes` 中占一个条目。
/// `type_id` 是该条目在 `classes` 数组中的索引（0-based）。
#[derive(Debug, Clone, PartialEq)]
pub struct ClassDef {
    /// 类名（如 "Any", "Animal", "Dog"）
    pub name: String,
    /// 父类 ID（`NO_PARENT` = 顶级类型，无父类）
    pub parent_id: u16,
    /// 字段数量
    pub field_count: u16,
    /// vtable 索引（到 `BytecodeModule::vtables`）
    pub vtable_idx: u16,
    /// 实现的接口 ID 列表
    pub interfaces: Vec<u16>,
    /// 是否为内置类型（Any）
    pub is_builtin: bool,
    /// 是否为单例对象（object 关键字声明）
    pub is_singleton: bool,
    /// 字段名列表（按槽位索引顺序）
    pub field_names: Vec<String>,
}

impl ClassDef {
    /// 创建内置类定义（如 Any）
    pub fn builtin(name: &str) -> Self {
        ClassDef {
            name: name.to_string(),
            parent_id: NO_PARENT,
            field_count: 0,
            vtable_idx: u16::MAX,
            interfaces: Vec::new(),
            is_builtin: true,
            is_singleton: false,
            field_names: Vec::new(),
        }
    }
}

/// 完整的字节码模块（对应 `.auc` 文件内容）
///
/// Phase 2 扩展字段（设计方案 §6.2）：
/// - `module_identity`：模块标识（UUID + 版本）
/// - `header_flags`：能力标志位
/// - `exports` / `imports`：导出/导入符号表
/// - `dependencies`：显式依赖列表
/// - `sig_ids`：外部模块签名 ID
/// - `entry_kind`：入口类型（app/library）
#[derive(Debug, Clone, PartialEq)]
pub struct BytecodeModule {
    pub consts: Vec<Const>,
    pub natives: Vec<BytecodeNative>,
    pub functions: Vec<BytecodeFunction>,
    /// 闭包表（Phase 2）
    pub closures: Vec<BytecodeClosure>,
    /// 入口函数（通常为 `main`）在 `functions` 中的索引
    pub entry: u16,
    /// Phase 1c: 按需链接 — 启用的 std 模块名
    pub enabled_modules: Vec<String>,

    // ── Phase 2 新增 ──
    /// 模块标识（UUID + 版本）
    pub module_identity: ModuleIdentity,
    /// 能力标志位（有签名/有导出表/有导入表/有 AOT/有依赖）
    pub header_flags: u32,
    /// 导出符号表
    pub exports: Vec<ExportSymbol>,
    /// 导入符号表
    pub imports: Vec<ImportSymbol>,
    /// 显式依赖列表
    pub dependencies: Vec<Dependency>,
    /// 外部模块签名 ID
    pub sig_ids: Vec<String>,
    /// 入口类型（`app` = 应用入口, `library` = 库入口）
    pub entry_kind: String,

    // ── Phase 1 AOT 嵌入（设计文档 §3.5）──
    /// AOT 段表（`.auc` v4；空表示纯字节码模块）
    pub aot_segments: Vec<AucSegment>,
    /// 段数据区（各段原始字节拼接，`AucSegment.offset` 相对此处起始）
    pub aot_blob_data: Vec<u8>,

    // ── P-K2：虚方法表（v5；open 方法动态分派）──
    /// 每个类的虚方法表：(类型标签, [槽 i → 函数索引])
    pub vtables: Vec<VirtualTable>,

    // ── Phase 1: 类定义表（v6；类 ID 系统 + 类层级）──
    /// 类定义表：编译时分配递增类 ID，支持继承链遍历
    pub classes: Vec<ClassDef>,

    // ── Phase 2: 源码索引（source_index 段；仅供 LSP，VM/JIT/AOT 不读取）──
    /// 源码索引：描述所有符号到虚拟源码位置的映射
    pub source_index: Option<crate::std::source_index::SourceIndex>,
}

/// 类的虚方法表（P-K2）：槽位编号为全局 open 方法序号
#[derive(Debug, Clone, PartialEq)]
pub struct VirtualTable {
    pub type_tag: u16,
    /// 槽 i → 函数表索引
    pub slots: Vec<u16>,
}

impl Default for BytecodeModule {
    fn default() -> Self {
        BytecodeModule {
            consts: Vec::new(),
            natives: Vec::new(),
            functions: Vec::new(),
            closures: Vec::new(),
            entry: 0,
            enabled_modules: Vec::new(),
            module_identity: ModuleIdentity::default(),
            header_flags: 0,
            exports: Vec::new(),
            imports: Vec::new(),
            dependencies: Vec::new(),
            sig_ids: Vec::new(),
            entry_kind: "app".to_string(),
            aot_segments: Vec::new(),
            aot_blob_data: Vec::new(),
            vtables: Vec::new(),
            classes: Vec::new(),
            source_index: None,
        }
    }
}

impl BytecodeModule {
    /// 构建期计算 header_flags
    pub fn compute_header_flags(&self) -> u32 {
        let mut flags = 0u32;
        if !self.exports.is_empty() {
            flags |= 0b00000010; // 有导出表
        }
        if !self.imports.is_empty() {
            flags |= 0b00000100; // 有导入表
        }
        if !self.dependencies.is_empty() {
            flags |= 0b00010000; // 有依赖
        }
        if !self.aot_segments.is_empty() {
            flags |= HEADER_HAS_MACHINE_CODE; // Phase 1 AOT: 含机器码段
            if self.functions.iter().any(|f| f.aot_desc_idx > 0) {
                flags |= HEADER_AOT_EXPORTS;
            }
        }
        if !self.classes.is_empty() {
            flags |= HEADER_HAS_CLASS_DEFS; // Phase 1: 含类定义表
        }
        if self.source_index.is_some() {
            flags |= HEADER_HAS_SOURCE_INDEX; // Phase 2: 含源码索引
        }
        flags
    }

    /// 是否包含 AOT 机器码（`.auc` v4）
    pub fn has_aot(&self) -> bool {
        !self.aot_segments.is_empty() && self.aot_blob_data.len() > 0
    }

    /// 检查模块是否标记为库（无 main 入口）
    pub fn is_library(&self) -> bool {
        self.entry_kind == "library"
    }

    /// 查找导出符号索引
    pub fn find_export_idx(&self, name: &str) -> Option<usize> {
        self.exports.iter().position(|e| e.name == name)
    }

    /// 查找导入符号索引
    pub fn find_import_idx(&self, module: &str, symbol: &str) -> Option<usize> {
        self.imports.iter().position(|i| i.module == module && i.symbol == symbol)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Phase 2: 模块标识与符号表
// ─────────────────────────────────────────────────────────────────────────────

/// 模块标识（UUID + 版本）— 设计方案 §6.2.3
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ModuleIdentity {
    /// 模块唯一标识（UUID v4）
    pub uuid: [u8; 16],
    /// 模块语义版本
    pub version: String,
    /// 模块名称
    pub name: String,
}

impl ModuleIdentity {
    /// 生成新的模块标识（随机 UUID）
    pub fn new(name: &str, version: &str) -> Self {
        let mut uuid = [0u8; 16];
        // 简单伪随机（生产环境应使用 uuid crate）
        let seed = name.bytes().fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
        uuid[0..8].copy_from_slice(&seed.to_le_bytes());
        uuid[8..16].copy_from_slice(&seed.wrapping_mul(1000003).to_le_bytes());
        // 设置版本位（UUID v4）
        uuid[6] = (uuid[6] & 0x0F) | 0x40;
        uuid[8] = (uuid[8] & 0x3F) | 0x80;
        ModuleIdentity {
            uuid,
            version: version.to_string(),
            name: name.to_string(),
        }
    }

    /// 解析 UUID 字符串
    pub fn uuid_str(&self) -> String {
        format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            self.uuid[0],
            self.uuid[1],
            self.uuid[2],
            self.uuid[3],
            self.uuid[4],
            self.uuid[5],
            self.uuid[6],
            self.uuid[7],
            self.uuid[8],
            self.uuid[9],
            self.uuid[10],
            self.uuid[11],
            self.uuid[12],
            self.uuid[13],
            self.uuid[14],
            self.uuid[15]
        )
    }
}

/// 导出符号 — 设计方案 §6.2.4
#[derive(Debug, Clone, PartialEq)]
pub struct ExportSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub sig_id: String,
    pub func_idx: Option<u16>,
    pub type_table_idx: Option<u16>,
    pub const_idx: Option<u16>,
}

/// 导入符号 — 设计方案 §6.2.5
#[derive(Debug, Clone, PartialEq)]
pub struct ImportSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub module: String,
    pub symbol: String,
    pub sig_id: String,
    pub func_idx: Option<u16>,
}

/// 显式依赖 — 设计方案 §6.2.6
#[derive(Debug, Clone, PartialEq)]
pub struct Dependency {
    pub module: String,
    pub uuid: [u8; 16],
    pub version: String,
}

/// 符号类型 — 设计方案 §6.2.4
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SymbolKind {
    #[default]
    Function,
    Type,
    Const,
    Global,
}

impl SymbolKind {
    pub fn to_byte(&self) -> u8 {
        match self {
            SymbolKind::Function => 0,
            SymbolKind::Type => 1,
            SymbolKind::Const => 2,
            SymbolKind::Global => 3,
        }
    }

    pub fn from_byte(b: u8) -> Self {
        match b {
            0 => SymbolKind::Function,
            1 => SymbolKind::Type,
            2 => SymbolKind::Const,
            3 => SymbolKind::Global,
            _ => SymbolKind::Function,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::offset_of;

    /// `operand_size` 必须与 `write` 实际写出的操作数字节数一致。
    ///
    /// 原因：`.auc` 解码器用 `operand_size` 推进指令游标并构建
    /// 「字节偏移 → 指令索引」映射；一旦二者不符，其后**所有指令边界错位**，
    /// 表现为跳转/异常处理器目标解析错误（曾因 `PushHandler` 少报 2 字节，
    /// 导致 `try/catch` 在多处理器函数中出现难以定位的行为异常）。
    #[test]
    fn test_operand_size_matches_write() {
        for b in 0u8..=255 {
            if let Some(op) = OpCode::from_byte(b) {
                let mut buf = Vec::new();
                op.write(&mut buf);
                assert_eq!(
                    buf.len(),
                    1 + OpCode::operand_size(b),
                    "opcode {} ({:?})：write 写出 {} 字节，但 1+operand_size({}) = {}",
                    b,
                    op,
                    buf.len(),
                    b,
                    1 + OpCode::operand_size(b)
                );
                assert_eq!(op.byte(), b, "opcode {:?} 的 byte() 应回到 {}", op, b);
            }
        }
    }

    #[test]
    fn test_call_aot_opcode_byte() {
        assert_eq!(OpCode::CallAot(0).byte(), 77);
        assert_eq!(OpCode::CallAot(4095).byte(), 77);
    }

    #[test]
    fn test_call_aot_from_byte_and_write() {
        assert_eq!(OpCode::from_byte(77), Some(OpCode::CallAot(0)));
        let mut buf = Vec::new();
        OpCode::CallAot(0x0102).write(&mut buf);
        assert_eq!(
            buf,
            vec![
                77u8, 0x02, 0x01
            ]
        );
    }

    #[test]
    fn test_call_aot_display() {
        assert_eq!(format!("{}", OpCode::CallAot(7)), "CALL_AOT 7");
    }

    #[test]
    fn test_aura_func_desc_c_layout_matches_design() {
        // 设计文档 §5.1: 32 字节，各字段偏移固定
        assert_eq!(AuraFuncDesc::SIZE, 32);
        assert_eq!(offset_of!(AuraFuncDesc, name_offset), 0x00);
        assert_eq!(offset_of!(AuraFuncDesc, name_len), 0x04);
        assert_eq!(offset_of!(AuraFuncDesc, _pad1), 0x06);
        assert_eq!(offset_of!(AuraFuncDesc, entry_offset), 0x08);
        assert_eq!(offset_of!(AuraFuncDesc, num_args), 0x10);
        assert_eq!(offset_of!(AuraFuncDesc, arg_tags), 0x11);
        assert_eq!(offset_of!(AuraFuncDesc, return_tag), 0x12);
        assert_eq!(offset_of!(AuraFuncDesc, flags), 0x13);
        assert_eq!(offset_of!(AuraFuncDesc, source_line), 0x14);
        assert_eq!(offset_of!(AuraFuncDesc, source_file_offset), 0x18);
        assert_eq!(offset_of!(AuraFuncDesc, _pad2), 0x1C);
    }

    #[test]
    fn test_aura_func_desc_bytes_roundtrip() {
        let d = AuraFuncDesc {
            name_offset: 0x40,
            name_len: 5,
            entry_offset: 0x100,
            num_args: 3,
            arg_tags: 0x23,
            return_tag: 0,
            flags: FUNC_EXPORT | FUNC_HOT,
            source_line: 42,
            source_file_offset: 0x20,
            ..AuraFuncDesc::default()
        };
        let bytes = unsafe {
            std::slice::from_raw_parts(&d as *const AuraFuncDesc as *const u8, AuraFuncDesc::SIZE)
        };
        let back = unsafe { std::ptr::read_unaligned(bytes.as_ptr() as *const AuraFuncDesc) };
        assert_eq!(back, d);
    }

    #[test]
    fn test_segment_ids_and_flags() {
        let segs = [
            AucSegment {
                id: SEG_BYTECODE,
                offset: 0,
                size: 0,
                flags: SEG_PROT_READ,
            },
            AucSegment {
                id: SEG_MACHINE,
                offset: 0,
                size: 0,
                flags: SEG_PROT_READ | SEG_PROT_EXEC,
            },
            AucSegment {
                id: SEG_DESC_TABLE,
                offset: 0,
                size: 0,
                flags: SEG_PROT_READ,
            },
            AucSegment {
                id: SEG_DEBUG,
                offset: 0,
                size: 0,
                flags: SEG_PROT_READ,
            },
            AucSegment {
                id: SEG_STRING_POOL,
                offset: 0,
                size: 0,
                flags: SEG_PROT_READ,
            },
            AucSegment {
                id: SEG_SIGNATURE,
                offset: 0,
                size: 0,
                flags: SEG_PROT_READ,
            },
        ];
        let names: Vec<&str> = segs.iter().map(|s| s.id_name()).collect();
        assert_eq!(
            names,
            vec![
                "bytecode",
                "machine",
                "desc_table",
                "debug",
                "string_pool",
                "signature"
            ]
        );
        assert!(!segs[0].is_exec());
        assert!(segs[1].is_exec());
        assert!(segs[1].is_read());
        assert!(!segs[1].is_write());
        let writable = AucSegment {
            id: 0,
            offset: 0,
            size: 0,
            flags: SEG_PROT_READ | SEG_PROT_WRITE,
        };
        assert!(writable.is_write());
        assert_eq!(
            AucSegment {
                id: 99,
                offset: 0,
                size: 0,
                flags: 0
            }
            .id_name(),
            "unknown"
        );
    }

    #[test]
    fn test_header_flags_and_has_aot() {
        let mut m = BytecodeModule::default();
        assert_eq!(m.compute_header_flags(), 0);
        assert!(!m.has_aot());
        m.aot_segments = vec![
            AucSegment {
                id: SEG_MACHINE,
                offset: 0,
                size: 256,
                flags: SEG_PROT_READ | SEG_PROT_EXEC,
            },
        ];
        m.aot_blob_data = vec![0u8; 256];
        assert!(m.has_aot());
        let flags = m.compute_header_flags();
        assert_eq!(flags & HEADER_HAS_MACHINE_CODE, HEADER_HAS_MACHINE_CODE);
        assert_eq!(
            flags & HEADER_AOT_EXPORTS,
            0,
            "no function has aot_desc_idx yet"
        );

        let mut f = BytecodeFunction::default();
        f.name = "add".to_string();
        f.aot_mode = 1;
        f.aot_desc_idx = 1;
        m.functions.push(f);
        let flags2 = m.compute_header_flags();
        assert_eq!(flags2 & HEADER_AOT_EXPORTS, HEADER_AOT_EXPORTS);
    }

    #[test]
    fn test_bytecode_function_aot_fields_default() {
        let f = BytecodeFunction::default();
        assert_eq!(f.aot_mode, 0);
        assert_eq!(f.aot_desc_idx, 0);
        assert!(f.code.is_empty());
    }
}
