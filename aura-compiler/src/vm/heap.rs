//! 堆对象 / 数组存储 + 引用计数（ARC）运行时
//!
//! 对应 技术方案 §6（内存管理）与 §7.1 的 `NewObject` / `NewArray` /
//! `GetField` / `SetField` / `IncRef` / `DecRef`。
//!
//! 设计要点：
//! - 字节码中对象字段索引用「字段名 FNV 哈希」编码（`emit.rs::field_index`），
//!   因此对象内部直接用 `HashMap<u16, Value>` 存储，无需在模块中携带类型表。
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
    Object {
        /// 类型标签（类型名 FNV 哈希，`emit.rs::type_index`）
        type_tag: u16,
        /// 字段表
        fields: HashMap<u16, Value>,
        /// 虚方法表：方法表索引 → 函数索引（5.6）
        /// 允许对象动态注册方法（如接口实现、多态调度）
        vtable: Option<HashMap<u16, usize>>,
    },
    /// 数组：定长元素序列
    Array(Vec<Value>),
    /// 动态列表（5.7）：可变长度有序集合
    List(Vec<Value>),
    /// 哈希映射（5.7）：键值对集合
    Map(HashMap<Value, Value>),
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
    c_strings: Vec<std::rc::Rc<str>>,
}

impl Default for Heap {
    fn default() -> Self {
        Heap {
            slots: Vec::new(),
            free: Vec::new(),
            c_strings: Vec::new(),
        }
    }
}

impl Heap {
    pub fn new() -> Self {
        Heap::default()
    }

    /// 分配一个对象，返回句柄
    pub fn alloc_object(&mut self, type_tag: u16) -> usize {
        self.alloc(HeapData::Object {
            type_tag,
            fields: HashMap::new(),
            vtable: None,
        })
    }

    /// 分配一个带虚方法表的对象（5.6）
    pub fn alloc_object_with_vtable(
        &mut self,
        type_tag: u16,
        vtable: HashMap<u16, usize>,
    ) -> usize {
        self.alloc(HeapData::Object {
            type_tag,
            fields: HashMap::new(),
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
        self.c_strings.push(std::rc::Rc::from(s.as_str()));
        self.c_strings.len() // 1-based index，0 = nullptr
    }

    /// 从 C 字符串指针读取字符串（P8.5）
    pub fn read_c_string(&self, ptr: usize) -> String {
        if ptr == 0 || ptr > self.c_strings.len() {
            return String::new();
        }
        self.c_strings[ptr - 1].to_string()
    }

    fn alloc(&mut self, data: HeapData) -> usize {
        if let Some(h) = self.free.pop() {
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
        }
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
    pub fn get_field(&self, handle: usize, field: u16) -> Value {
        match self.slots.get(handle).and_then(|s| s.data.as_ref()) {
            Some(HeapData::Object { fields, .. }) => {
                fields.get(&field).cloned().unwrap_or(Value::Null)
            }
            _ => Value::Null,
        }
    }

    /// 写入对象字段
    pub fn set_field(&mut self, handle: usize, field: u16, value: Value) {
        if let Some(slot) = self.slots.get_mut(handle) {
            if let Some(HeapData::Object { fields, .. }) = &mut slot.data {
                fields.insert(field, value);
            }
        }
    }

    /// 查找对象虚方法表中的方法，返回函数索引（5.6）
    pub fn get_vtable_method(&self, handle: usize, method_idx: u16) -> Option<usize> {
        match self.slots.get(handle).and_then(|s| s.data.as_ref()) {
            Some(HeapData::Object { vtable, .. }) => {
                vtable.as_ref().and_then(|vt| vt.get(&method_idx).copied())
            }
            _ => None,
        }
    }

    /// 读取数组元素
    pub fn get_index(&self, handle: usize, index: usize) -> Value {
        match self.slots.get(handle).and_then(|s| s.data.as_ref()) {
            Some(HeapData::Array(elems)) => elems.get(index).cloned().unwrap_or(Value::Null),
            _ => Value::Null,
        }
    }

    /// 写入数组元素
    pub fn set_index(&mut self, handle: usize, index: usize, value: Value) {
        if let Some(slot) = self.slots.get_mut(handle) {
            if let Some(HeapData::Array(elems)) = &mut slot.data {
                if index < elems.len() {
                    elems[index] = value;
                }
            }
        }
    }

    // ── List 操作（5.7） ──

    /// List 尾部追加元素
    pub fn list_push(&mut self, handle: usize, value: Value) {
        if let Some(slot) = self.slots.get_mut(handle) {
            if let Some(HeapData::List(elems)) = &mut slot.data {
                elems.push(value);
            }
        }
    }

    /// List 弹出尾部元素
    pub fn list_pop(&mut self, handle: usize) -> Value {
        match self.slots.get_mut(handle).and_then(|s| s.data.as_mut()) {
            Some(HeapData::List(elems)) => elems.pop().unwrap_or(Value::Null),
            _ => Value::Null,
        }
    }

    /// List 长度
    pub fn list_len(&self, handle: usize) -> i64 {
        match self.slots.get(handle).and_then(|s| s.data.as_ref()) {
            Some(HeapData::List(elems)) => elems.len() as i64,
            _ => 0,
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
        let mut fields = HashMap::new();
        // 使用固定字段名 "value" 存储
        fields.insert(field_hash("value"), value);
        self.alloc(HeapData::Object {
            type_tag: type_hash("Box"),
            fields,
            vtable: None,
        })
    }

    /// 判断堆槽是否仍存活（P7.3 弱引用升级）
    pub fn is_alive(&self, handle: usize) -> bool {
        self.slots
            .get(handle)
            .map(|s| s.data.is_some())
            .unwrap_or(false)
    }

    /// 当前存活对象数量（诊断用）
    pub fn live_count(&self) -> usize {
        self.slots.iter().filter(|s| s.data.is_some()).count()
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
        HeapData::Object { fields, .. } => fields.values().next().cloned().unwrap_or(Value::Null),
        HeapData::Array(elems) => elems.last().cloned().unwrap_or(Value::Null),
        HeapData::List(elems) => elems.last().cloned().unwrap_or(Value::Null),
        HeapData::Map(map) => map.values().next().cloned().unwrap_or(Value::Null),
    }
}
