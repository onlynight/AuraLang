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
    },
    /// 数组：定长元素序列
    Array(Vec<Value>),
}

/// 堆槽（含引用计数与回收标记）
struct HeapSlot {
    rc: usize,
    data: Option<HeapData>,
}

/// 堆管理器
pub struct Heap {
    slots: Vec<HeapSlot>,
    free: Vec<usize>,
}

impl Default for Heap {
    fn default() -> Self {
        Heap {
            slots: Vec::new(),
            free: Vec::new(),
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
        })
    }

    /// 分配一个长度为 `len` 的数组（元素初始化为 `Null`）
    pub fn alloc_array(&mut self, len: usize) -> usize {
        self.alloc(HeapData::Array(vec![Value::Null; len]))
    }

    fn alloc(&mut self, data: HeapData) -> usize {
        if let Some(h) = self.free.pop() {
            self.slots[h] = HeapSlot {
                rc: 1,
                data: Some(data),
            };
            h
        } else {
            let h = self.slots.len();
            self.slots.push(HeapSlot {
                rc: 1,
                data: Some(data),
            });
            h
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

    /// 引用计数 -1；归零则回收槽位
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
                slot.data = None;
                self.free.push(handle);
            }
        }
    }

    /// 读取对象字段
    pub fn get_field(&self, handle: usize, field: u16) -> Value {
        match self.slots.get(handle).and_then(|s| s.data.as_ref()) {
            Some(HeapData::Object { fields, .. }) => fields.get(&field).cloned().unwrap_or(Value::Null),
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

    /// 当前存活对象数量（诊断用）
    pub fn live_count(&self) -> usize {
        self.slots.iter().filter(|s| s.data.is_some()).count()
    }
}
