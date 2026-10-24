// ================================================================
// 【冻结基线】Rust VM 双实现常驻（VM-PA-00 v3.2 / D6 / P4.3）
// [FROZEN BASELINE] Rust VM dual-impl co-resident (D6/P4.3): bug-fix only, no new capabilities, never reference the Aura VM.
// 本文件属于 Rust VM（只读冻结的兼容基线、第二实现）：
//   - 禁止新增能力，仅允许修 bug 与安全修补；
//   - 不得引用 Aura VM 实现（aura/compiler/aura/lang/compiler/vm/）；
//   - 与 Aura VM 的唯一交集是 .auc 二进制格式（Rust 编译器产出，两侧各自执行）。
// 依据：docs/vm_pure_aura/00-VM纯Aura化技术方案与达成路径.md 三-阶段P4 / 决策D6。
// ================================================================
//! 堆对象 / 数组存储 + 引用计数（ARC）运行时
//!
//! 对应 技术方案 §6（内存管理）与 §7.1 的 `NewObject` / `NewArray` /
//! `GetField` / `SetField` / `IncRef` / `DecRef`。
//!
//! 设计要点：
//! - 字节码中对象字段索引用「字段名 FNV 哈希」编码（`emit.rs::field_index`），
//!   因此对象内部直接用 `Vec<(u16, Value)>` 存储（P3.3: 原 HashMap 改为紧凑元组向量）。
//! - `Value::Ref(usize)` 是堆槽句柄。槽位带 `rc` 引用计数；`DecRef` 将计数减一，
//!   归零即回收（句柄进入空闲链表以便复用）。
//! - 短生命周期脚本程序即使不触发 `DecRef` 也不会影响正确性，仅存在轻微泄漏，
//!   符合 ARC「确定性回收、无 GC 暂停」的设计目标。

use std::collections::HashMap;

use crate::vm::value::Value;

/// 堆中的对象数据
#[derive(Clone)]
pub enum HeapData {
    /// 对象：字段索引（FNV 哈希）→ 值
    /// ⚠️ 2026-09-29 P3.3: HashMap<u16, Value> → Vec<(u16, Value)>
    ///   - 空间降 60-70%（HashMap 每桶 16 字节开销 vs Vec 元组 24 字节）
    ///   - 线性搜索 O(n) 但对象字段通常 <10 个，实际影响可忽略
    Object {
        /// 类型标签（类型名 FNV 哈希，`emit.rs::type_index`）
        type_tag: u16,
        /// 字段表（紧凑元组向量）
        fields: Vec<(u16, Value)>,
        /// 虚方法表：方法表索引 → 函数索引（5.6）
        /// 允许对象动态注册方法（如接口实现、多态调度）
        vtable: Option<Vec<(u16, usize)>>,
    },
    /// 数组：定长元素序列
    Array(Vec<Value>),
    /// 动态列表（5.7）：可变长度有序集合
    List(Vec<Value>),
    /// 哈希映射（5.7）：键值对集合
    Map(HashMap<Value, Value>),
    /// 闭包（Phase 2）：捕获的变量 + 函数体引用
    Closure {
        /// 闭包函数名
        func_name: String,
        /// 参数数量（用户参数）
        param_count: u16,
        /// 局部变量槽总数
        locals: u16,
        /// 捕获的值
        captures: Vec<Value>,
        /// 闭包函数在函数表中的索引
        func_idx: usize,
    },
    /// 枚举值（Phase 3）：变体索引
    Enum(u16),
    /// 函数引用（Phase 3）：函数在函数表中的索引
    FnRef(usize),
}

/// 堆槽（含引用计数与回收标记）
struct HeapSlot {
    rc: usize,
    data: Option<HeapData>,
    /// 释放回调（5.10）：对象回收时调用，可用于清理外部资源
    drop_cb: Option<DropCallback>,
}

/// 释放回调函数指针
type DropCallback = fn(Value);

/// 堆管理器
pub struct Heap {
    slots: Vec<HeapSlot>,
    free: Vec<usize>,
    /// C 字符串存储（P8.5）：索引即指针值
    /// Fix 8: Rc<str> → Arc<str>（线程安全）
    c_strings: Vec<std::sync::Arc<str>>,
    /// Fix 13: 运行时泄漏检测 — 记录所有活跃的分配
    allocated: Vec<usize>,
}

impl Default for Heap {
    fn default() -> Self {
        Heap {
            slots: Vec::new(),
            free: Vec::new(),
            c_strings: Vec::new(),
            allocated: Vec::new(),
        }
    }
}

impl Heap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fix 13: 获取当前活跃对象数量（用于泄漏检测）
    pub fn active_count(&self) -> usize {
        self.allocated.len()
    }

    /// Fix 13: 获取运行时泄漏报告
    pub fn leak_report(&self) -> LeakReport {
        let mut leaked = Vec::new();
        for &slot_idx in &self.allocated {
            if let Some(slot) = self.slots.get(slot_idx) {
                if slot.data.is_some() && slot.rc > 0 {
                    leaked.push(LeakDetail {
                        slot_index: slot_idx,
                        rc: slot.rc,
                        data_type: describe_heap_data(slot.data.as_ref()),
                    });
                }
            }
        }
        LeakReport {
            total_allocs: self.allocated.len(),
            active_allocs: self.active_count(),
            leaked: leaked.len(),
            details: leaked,
        }
    }

    /// Fix 13: 释放所有对象（用于测试/清理）
    pub fn clear_all(&mut self) {
        self.slots.clear();
        self.free.clear();
        self.c_strings.clear();
        self.allocated.clear();
    }

    /// 分配一个对象，返回句柄
    pub fn alloc_object(&mut self, type_tag: u16) -> usize {
        self.alloc(HeapData::Object {
            type_tag,
            fields: Vec::new(),  // P3.3: HashMap::new() → Vec::new()
            vtable: None,
        })
    }

    /// 分配一个带虚方法表的对象（5.6）
    pub fn alloc_object_with_vtable(
        &mut self,
        type_tag: u16,
        vtable: Vec<(u16, usize)>,  // P3.3: HashMap<u16, usize> → Vec<(u16, usize)>
    ) -> usize {
        self.alloc(HeapData::Object {
            type_tag,
            fields: Vec::new(),
            vtable: Some(vtable),
        })
    }

    /// 分配一个长度为 `len` 的数组（元素初始化为 `Null`）
    pub fn alloc_array(&mut self, len: usize) -> usize {
        self.alloc(HeapData::Array(vec![Value::Null; len]))
    }

    /// 分配一个动态 List（5.7），返回句柄
    pub fn alloc_list(&mut self, initial_capacity: usize) -> usize {
        self.alloc(HeapData::List(Vec::with_capacity(initial_capacity)))
    }

    /// 分配一个 Map（5.7），返回句柄
    pub fn alloc_map(&mut self) -> usize {
        self.alloc(HeapData::Map(HashMap::new()))
    }

    /// 分配一个 C 字符串（P8.5）：返回指针值（索引 + 1，0 = nullptr）
    pub fn alloc_c_string(&mut self, s: String) -> usize {
        self.c_strings.push(std::sync::Arc::from(s.as_str()));
        self.c_strings.len() // 1-based index，0 = nullptr
    }

    /// 从 C 字符串指针读取字符串（P8.5）
    pub fn read_c_string(&self, ptr: usize) -> String {
        if ptr == 0 || ptr > self.c_strings.len() {
            return String::new();
        }
        self.c_strings[ptr - 1].to_string()
    }

    /// Fix 13: 内部分配方法（记录到 allocated 列表）
    pub fn alloc(&mut self, data: HeapData) -> usize {
        let h = if let Some(h) = self.free.pop() {
            self.slots[h] = HeapSlot {
                rc: 1,
                data: Some(data),
                drop_cb: None,
            };
            h
        } else {
            let h = self.slots.len();
            self.slots.push(HeapSlot {
                rc: 1,
                data: Some(data),
                drop_cb: None,
            });
            h
        };
        // Fix 13: 记录分配
        self.allocated.push(h);
        h
    }

    /// 注册释放回调（5.10）
    pub fn set_drop_callback(&mut self, handle: usize, cb: DropCallback) {
        if let Some(slot) = self.slots.get_mut(handle) {
            slot.drop_cb = Some(cb);
        }
    }

    /// 引用计数 +1
    pub fn inc_ref(&mut self, handle: usize) {
        if let Some(slot) = self.slots.get_mut(handle) {
            if slot.data.is_some() {
                slot.rc += 1;
            }
        }
    }

    /// 引用计数 -1；归零则回收槽位（触发 drop 回调，5.10）
    pub fn dec_ref(&mut self, handle: usize) {
        if let Some(slot) = self.slots.get_mut(handle) {
            if slot.data.is_none() {
                return;
            }
            // 防御下溢：对 rc 已为 0 的槽位重复 DecRef 是无害操作
            if slot.rc > 0 {
                slot.rc -= 1;
            }
            if slot.rc == 0 {
                // 在清除数据前，若已挂载对象则先取出最后一个值触发 drop 回调
                let last_value = slot.data.as_ref().map(last_heap_value);
                slot.data = None;
                if let Some(cb) = slot.drop_cb.take() {
                    if let Some(v) = last_value {
                        cb(v);
                    }
                }
                self.free.push(handle);
            }
        }
    }

    /// 显式释放（DropRef 指令，5.10）：强制触发 drop 回调并回收
    pub fn drop_ref(&mut self, handle: usize) {
        if let Some(slot) = self.slots.get_mut(handle) {
            if slot.data.is_none() {
                return;
            }
            let last_value = slot.data.as_ref().map(last_heap_value);
            slot.data = None;
            if let Some(cb) = slot.drop_cb.take() {
                if let Some(v) = last_value {
                    cb(v);
                }
            }
            self.free.push(handle);
        }
    }

    /// 读取对象字段
    /// P3.3: HashMap O(1) → Vec 线性搜索 O(n)，但对象字段通常 <10 个
    pub fn get_field(&self, handle: usize, field: u16) -> Value {
        match self.slots.get(handle).and_then(|s| s.data.as_ref()) {
            Some(HeapData::Object { fields, .. }) => {
                // 线性搜索：(field_hash, value)
                for &(field_hash, ref value) in fields.iter() {
                    if field_hash == field {
                        return value.clone();
                    }
                }
                Value::Null
            }
            _ => Value::Null,
        }
    }

    /// 获取堆数据（不可变引用）
    pub fn get_data(&self, handle: usize) -> Option<&HeapData> {
        self.slots.get(handle).and_then(|s| s.data.as_ref())
    }

    /// 获取堆数据（可变引用，用于闭包等特殊类型）
    pub fn get_data_mut(&mut self, handle: usize) -> Option<&mut HeapData> {
        self.slots.get_mut(handle).and_then(|s| s.data.as_mut())
    }

    /// 写入对象字段
    /// P3.3: HashMap insert → Vec 线性搜索 + insert
    pub fn set_field(&mut self, handle: usize, field: u16, value: Value) {
        if let Some(slot) = self.slots.get_mut(handle) {
            if let Some(HeapData::Object { fields, .. }) = &mut slot.data {
                // 查找是否已存在
                for entry in fields.iter_mut() {
                    if entry.0 == field {
                        entry.1 = value;
                        return;
                    }
                }
                // 不存在则追加
                fields.push((field, value));
            }
        }
    }

    /// 查找对象虚方法表中的方法，返回函数索引（5.6）
    /// P3.3: HashMap get → Vec 线性搜索
    pub fn get_vtable_method(&self, handle: usize, method_idx: u16) -> Option<usize> {
        match self.slots.get(handle).and_then(|s| s.data.as_ref()) {
            Some(HeapData::Object { vtable, .. }) => {
                vtable.as_ref().and_then(|vt| {
                    for &(idx, func_idx) in vt.iter() {
                        if idx == method_idx {
                            return Some(func_idx);
                        }
                    }
                    None
                })
            }
            _ => None,
        }
    }

    /// 读取数组 / **堆列表**元素
    pub fn get_index(&self, handle: usize, index: usize) -> Value {
        // `ArrayList` 之类的**类实例**接收者：下标落在其字段 0 的底层列表上。
        let handle = self.list_slot(handle).unwrap_or(handle);
        match self.slots.get(handle).and_then(|s| s.data.as_ref()) {
            Some(HeapData::Array(elems)) => elems.get(index).cloned().unwrap_or(Value::Null),
            // 堆列表（`arrayListOf` / `NEW_LIST`）同样支持下标读取
            Some(HeapData::List(elems)) => elems.get(index).cloned().unwrap_or(Value::Null),
            _ => Value::Null,
        }
    }

    /// 写入数组 / **堆列表**元素
    pub fn set_index(&mut self, handle: usize, index: usize, value: Value) {
        // 同 `get_index`：类实例接收者按字段 0 解引用到底层列表。
        let handle = self.list_slot(handle).unwrap_or(handle);
        if let Some(slot) = self.slots.get_mut(handle) {
            match &mut slot.data {
                Some(HeapData::Array(elems)) => {
                    if index < elems.len() {
                        elems[index] = value;
                    }
                }
                Some(HeapData::List(elems)) => {
                    if index < elems.len() {
                        elems[index] = value;
                    }
                }
                _ => {}
            }
        }
    }

    // ── List 操作（5.7） ──

    /// 把「列表接收者句柄」解析为**承载实际元素序列**的堆槽句柄。
    ///
    /// 多数调用点（`__list_push` / `listLen` / `listPop` / `listSet`）拿到的接收者
    /// 有两种形态：
    ///   1. 直接就是 `HeapData::List` 槽（`arrayListOf<T>()` 的返回值）；
    ///   2. **用户类实例**（`ArrayList<T>()` / `LinkedList<T>()` …），其第 0 个字段
    ///      才是真正的 `List<T>`（`ArrayList.aura` 里 `private var data: List<T>`）。
    ///
    /// 旧实现只处理形态 1：形态 2 下 `list_push` 静默无操作、`list_len` 恒返回 0。
    /// 实测（P3.5 调试器验收）：`BreakpointManager` 用
    /// `HashMap<Int, …>` + `ArrayList<Int>()`，`mgr.list().size` 恒为 0、
    /// `SourceMapUtils.splitLines()` 恒返回空表 —— 根因即此。
    ///
    /// 约定「字段 0 是底层列表」与 AOT 侧 `emit.rs` 的 `__list_push` /
    /// `add` 兜底分支完全一致（那里对 `%struct.*` 接收者取 `i32 0, i32 0` 字段）。
    fn list_slot(&self, handle: usize) -> Option<usize> {
        let slot = self.slots.get(handle)?;
        match slot.data.as_ref()? {
            HeapData::List(_) => Some(handle),
            // 用户类实例：取字段 0；若它不是列表句柄（如未初始化 / 别的类型），
            // 继续沿链最多再走一层，避免把 `HashMap` 之类的对象误当列表。
            HeapData::Object {
                fields, ..
            } => {
                // 字段按插入顺序存放；`data` 是 `ArrayList` 的首个字段。
                for (_, v) in fields.iter().take(1) {
                    if let Value::Ref(h) = v {
                        if h != &handle {
                            if matches!(
                                self.slots.get(*h).and_then(|s| s.data.as_ref()),
                                Some(HeapData::List(_))
                            ) {
                                return Some(*h);
                            }
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    /// List 尾部追加元素
    ///
    /// 接收者可能是列表槽本身，也可能是 `ArrayList` 之类的**类实例**（见 `list_slot`）。
    ///
    /// 类实例接收者追加后额外**同步 `_size` 字段**：`ArrayList.add` 的 Aura 实现
    /// 会 `_size = _size + 1`，但调用点的 `x.add(v)` 被 HIR 改派到 `__list_push`
    /// 原地追加，方法体根本不执行 → `_size` 永远停在构造时的 0。而
    /// `first`/`last`/`lastIndexOf`/`skip`/`take`/`sum`/`min`/`max`/`distinct`
    /// 等**未被改派**的方法仍读 `_size` 做循环边界，于是全部失效
    /// （实测 `lastIndexOf` 恒 -1、`skip(1).size` 恒 0、`sum` 恒 0）。
    ///
    /// 在这里补写 `_size` 是最小且收敛的修法：一处补齐，所有依赖 `_size` 的
    /// Aura 方法立刻恢复正确语义，无需为每个方法再加一条指令。
    pub fn list_push(&mut self, handle: usize, value: Value) {
        let Some(target) = self.list_slot(handle) else {
            return;
        };
        if let Some(slot) = self.slots.get_mut(target) {
            if let Some(HeapData::List(elems)) = &mut slot.data {
                elems.push(value);
            }
        }
        // 类实例接收者：同步 `_size`。
        if target != handle {
            let len = self.list_len(target);
            self.sync_list_size_field(handle, len);
        }
    }

    /// 把「列表长度」写回类实例接收者的 `_size` 字段（若存在）。
    ///
    /// 字段名哈希与 `codegen::emit::field_index` 一致；字段不存在时**不新增**
    /// （避免给非 ArrayList 的类实例塞入无意义字段）。
    fn sync_list_size_field(&mut self, handle: usize, len: i64) {
        let f = crate::codegen::emit::field_index("_size");
        if let Some(slot) = self.slots.get_mut(handle) {
            if let Some(HeapData::Object { fields, .. }) = &mut slot.data {
                for entry in fields.iter_mut() {
                    if entry.0 == f {
                        entry.1 = Value::Int(len);
                        return;
                    }
                }
            }
        }
    }


    /// List 弹出尾部元素
    pub fn list_pop(&mut self, handle: usize) -> Value {
        let target = self.list_slot(handle).unwrap_or(handle);
        let popped = match self.slots.get_mut(target).and_then(|s| s.data.as_mut()) {
            Some(HeapData::List(elems)) => elems.pop().unwrap_or(Value::Null),
            _ => Value::Null,
        };
        if target != handle && !matches!(popped, Value::Null) {
            let len = self.list_len(target);
            self.sync_list_size_field(handle, len);
        }
        popped
    }

    /// List 长度
    pub fn list_len(&self, handle: usize) -> i64 {
        let target = match self.list_slot(handle) {
            Some(t) => t,
            None => handle,
        };
        match self.slots.get(target).and_then(|s| s.data.as_ref()) {
            Some(HeapData::List(elems)) => elems.len() as i64,
            _ => 0,
        }
    }

    /// List 是否包含某元素（兼容 `ArrayList` 类实例接收者，见 `list_slot`）。
    pub fn list_contains(&self, handle: usize, needle: &Value) -> bool {
        let target = self.list_slot(handle).unwrap_or(handle);
        match self.slots.get(target).and_then(|s| s.data.as_ref()) {
            Some(HeapData::List(elems)) => elems.iter().any(|v| v == needle),
            Some(HeapData::Array(elems)) => elems.iter().any(|v| v == needle),
            _ => false,
        }
    }

    /// List 中某元素的下标（未找到返回 -1）。
    pub fn list_index_of(&self, handle: usize, needle: &Value) -> i64 {
        let target = self.list_slot(handle).unwrap_or(handle);
        match self.slots.get(target).and_then(|s| s.data.as_ref()) {
            Some(HeapData::List(elems)) => {
                elems.iter().position(|v| v == needle).map(|i| i as i64).unwrap_or(-1)
            }
            Some(HeapData::Array(elems)) => {
                elems.iter().position(|v| v == needle).map(|i| i as i64).unwrap_or(-1)
            }
            _ => -1,
        }
    }

    /// List 按下标**删除**元素并返回被删元素（越界返回 `Null`）。
    ///
    /// 与 `list_set` 同理：`l.remove(i)` 必须**原地**生效（`__list_remove_at` 指令），
    /// 因为 `ArrayList.remove` 的 Aura 实现读私有字段 `_size`，而该字段在运行期
    /// 恒为 0（`add` 走 `__list_push` 原地追加，不回写 `_size`）—— 旧路径下
    /// `l.remove(0)` 会以 "index: 0, size: 0" 抛越界异常。
    pub fn list_remove_at(&mut self, handle: usize, index: usize) -> Value {
        let target = self.list_slot(handle).unwrap_or(handle);
        let mut removed = Value::Null;
        let mut hit = false;
        if let Some(slot) = self.slots.get_mut(target) {
            if let Some(HeapData::List(elems)) = &mut slot.data {
                if index < elems.len() {
                    removed = elems.remove(index);
                    hit = true;
                }
            }
        }
        // 类实例接收者：同步 `_size`（与 `list_push` 对称）。
        if hit && target != handle {
            let len = self.list_len(target);
            self.sync_list_size_field(handle, len);
        }
        removed
    }

    /// 该句柄指向的是否为 Map（`set` 需要据此区分「列表下标」与「Map 键」）。
    pub fn is_map(&self, handle: usize) -> bool {
        matches!(
            self.slots.get(handle).and_then(|s| s.data.as_ref()),
            Some(HeapData::Map(_))
        )
    }

    /// List 按下标原地写入（越界为无操作）。
    ///
    /// `l.set(i, v)` 经前端重写为 `Collections.set(l, i, v)`；堆列表必须**原地**
    /// 生效，否则调用方丢弃返回值后列表毫无变化（纯函数式 native 只能返回值语义
    /// 的 `Value::List`，对 `Value::Ref` 直接返回 `Null`）。
    pub fn list_set(&mut self, handle: usize, index: usize, value: Value) {
        let target = self.list_slot(handle).unwrap_or(handle);
        if let Some(slot) = self.slots.get_mut(target) {
            if let Some(HeapData::List(elems)) = &mut slot.data {
                if index < elems.len() {
                    elems[index] = value;
                }
            }
        }
    }

    // ── Map 操作（5.7） ──

    /// Map 插入键值对
    pub fn map_set(&mut self, handle: usize, key: Value, value: Value) {
        if let Some(slot) = self.slots.get_mut(handle) {
            if let Some(HeapData::Map(map)) = &mut slot.data {
                map.insert(key, value);
            }
        }
    }

    /// Map 查找键并返回值
    pub fn map_get(&self, handle: usize, key: &Value) -> Value {
        match self.slots.get(handle).and_then(|s| s.data.as_ref()) {
            Some(HeapData::Map(map)) => map.get(key).cloned().unwrap_or(Value::Null),
            _ => Value::Null,
        }
    }

    /// Map 长度
    pub fn map_len(&self, handle: usize) -> i64 {
        match self.slots.get(handle).and_then(|s| s.data.as_ref()) {
            Some(HeapData::Map(map)) => map.len() as i64,
            _ => 0,
        }
    }

    /// 分配一个包装任意值的堆对象（P7.5 box 显式堆分配）
    pub fn alloc_box_value(&mut self, value: Value) -> usize {
        let mut fields: Vec<(u16, Value)> = Vec::new();  // P3.3: HashMap → Vec
        // 使用固定字段名 "value" 存储
        fields.push((field_hash("value"), value));
        self.alloc(HeapData::Object {
            type_tag: type_hash("Box"),
            fields,
            vtable: None,
        })
    }

    /// 判断堆槽是否仍存活（P7.3 弱引用升级）
    pub fn is_alive(&self, handle: usize) -> bool {
        self.slots.get(handle).map(|s| s.data.is_some()).unwrap_or(false)
    }

    /// 当前存活对象数量（诊断用）
    pub fn live_count(&self) -> usize {
        self.slots.iter().filter(|s| s.data.is_some()).count()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Fix 13: 运行时泄漏检测
// ─────────────────────────────────────────────────────────────────────────────

/// 运行时泄漏检测报告
#[derive(Debug, Clone)]
pub struct LeakReport {
    /// 总分配次数
    pub total_allocs: usize,
    /// 当前活跃对象数
    pub active_allocs: usize,
    /// 泄漏对象数（rc > 0 且未被释放）
    pub leaked: usize,
    /// 泄漏详情
    pub details: Vec<LeakDetail>,
}

/// 单个泄漏详情
#[derive(Debug, Clone)]
pub struct LeakDetail {
    /// 堆槽索引
    pub slot_index: usize,
    /// 当前引用计数
    pub rc: usize,
    /// 数据类型描述
    pub data_type: String,
}

/// 描述堆数据类型
fn describe_heap_data(data: Option<&HeapData>) -> String {
    match data {
        Some(HeapData::Object {
            type_tag, ..
        }) => format!("Object({:#x})", type_tag),
        Some(HeapData::Array(_)) => "Array".to_string(),
        Some(HeapData::List(_)) => "List".to_string(),
        Some(HeapData::Map(_)) => "Map".to_string(),
        Some(HeapData::Closure { .. }) => "Closure".to_string(),
        Some(HeapData::Enum(_)) => "Enum".to_string(),
        Some(HeapData::FnRef(_)) => "FnRef".to_string(),
        None => "Unknown".to_string(),
    }
}

/// 字段名 FNV 哈希（与 emit.rs 一致）
fn field_hash(name: &str) -> u16 {
    let mut h: u32 = 2166136261;
    for b in name.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(16777619);
    }
    (h % 65535) as u16
}

/// 类型名 FNV 哈希（与 emit.rs 一致）
fn type_hash(name: &str) -> u16 {
    field_hash(name)
}

/// 取堆对象中一个代表性值（用于 drop 回调）
fn last_heap_value(data: &HeapData) -> Value {
    match data {
        // P3.3: HashMap.values().next() → Vec.first()
        HeapData::Object { fields, .. } => fields.first().map(|(_, v)| v.clone()).unwrap_or(Value::Null),
        HeapData::Array(elems) => elems.last().cloned().unwrap_or(Value::Null),
        HeapData::List(elems) => elems.last().cloned().unwrap_or(Value::Null),
        HeapData::Map(map) => map.values().next().cloned().unwrap_or(Value::Null),
        HeapData::Closure { .. } => Value::Null,
        HeapData::Enum(_) => Value::Null,
        HeapData::FnRef(_) => Value::Null,
    }
}
