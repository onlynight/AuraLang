//! Phase 1 — 源码索引（SourceIndex）数据结构
//!
//! 设计文档：docs/stdlib与基础类型源码可见性设计方案.md
//!
//! SourceIndex 是编译期的核心元数据结构，描述所有符号到源码位置的映射。
//! 存储在 `.auc` 的 `source_index` 段中（可选段），供 LSP 读取。
//!
//! 关键约束：
//! - **零运行时性能代价**：VM/JIT/AOT 不读取此段
//! - **向后兼容**：旧版本 `.auc` 无此段，正常执行
//! - **编译期生成**：由 docgen 阶段从 phantom source 提取
//!
//! 使用方式：
//! - `SourceIndex::new()` — 创建空索引
//! - `index.add_type_def("Int", "aura://builtin/Int.aura", 12, 0, 150, 1)`
//! - `index.add_function_def("aura.math.sin", "aura://stdlib/aura/math/Math.aura", 84, 4, 85, 1)`
//! - `index.lookup("aura.math.sin")` — 查找源码位置

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 源码位置——描述符号在虚拟文件中的位置
///
/// URI 使用 `aura://` 自定义 scheme：
/// - `aura://builtin/Int.aura` — 基础类型
/// - `aura://stdlib/aura/math/Math.aura` — stdlib 模块
/// - `aura://prelude/prelu.aura` — prelude 函数
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceLocation {
    /// 虚拟 URI（`aura://` 协议）
    pub uri: String,
    /// 起始行（1-based）
    pub line: u32,
    /// 起始列（0-based）
    pub col: u32,
    /// 结束行
    pub end_line: u32,
    /// 结束列
    pub end_col: u32,
}

impl SourceLocation {
    /// 创建源码位置
    pub fn new(uri: impl Into<String>, line: u32, col: u32, end_line: u32, end_col: u32) -> Self {
        Self {
            uri: uri.into(),
            line,
            col,
            end_line,
            end_col,
        }
    }

    /// 是否为 builtin 类型
    pub fn is_builtin(&self) -> bool {
        self.uri.starts_with("aura://builtin/")
    }

    /// 是否为 stdlib 模块
    pub fn is_stdlib(&self) -> bool {
        self.uri.starts_with("aura://stdlib/")
    }

    /// 是否为 prelude
    pub fn is_prelude(&self) -> bool {
        self.uri.starts_with("aura://prelude/")
    }

    /// 提取文件名（如 "Int.aura"）
    pub fn file_name(&self) -> &str {
        self.uri.rsplit('/').next().unwrap_or(&self.uri)
    }

    /// 提取模块路径（如 "aura.math"）
    pub fn module_path(&self) -> Option<&str> {
        if let Some(rest) = self.uri.strip_prefix("aura://stdlib/") {
            // "aura/math/Math.aura" → "aura.math"
            let dir = rest.split('/').next().unwrap_or(rest);
            Some(dir)
        } else {
            None
        }
    }
}

/// 源码归档——可选内嵌的 phantom source 文本
///
/// 若包含，则 `.auc` 自带全部 phantom source 文本。
/// 若不含，则 LSP 从 `.auz` 的 `SOURCE/` 段或独立 source 包获取。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceArchive {
    /// 文件路径 → 内容
    pub files: HashMap<String, String>,
    /// SHA-256 校验和（16 字节短哈希，用于快速验证）
    pub checksum: Vec<u8>,
}

impl SourceArchive {
    /// 创建空归档
    pub fn new() -> Self {
        Self::default()
    }

    /// 添加文件
    pub fn add(&mut self, path: impl Into<String>, content: impl Into<String>) {
        self.files.insert(path.into(), content.into());
    }

    /// 获取文件内容
    pub fn get(&self, path: &str) -> Option<&str> {
        self.files.get(path).map(|s| s.as_str())
    }

    /// 文件数量
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// 获取所有文件路径
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(|s| s.as_str())
    }
}

/// 源码索引——编译期生成，LSP 读取
///
/// 描述所有符号（类型/函数/常量/变量/枚举/接口）到源码位置的映射。
/// 存储在 `.auc` 的 `source_index` 段中（可选段）。
///
/// # 设计决策
///
/// - 使用 `HashMap` 而非 `BTreeMap`：查找性能优先，迭代顺序无关
/// - `version` 字段用于向前兼容
/// - `source_archive` 为 `Option`：Level 0/1/2 不含，Level 3 含
/// - 所有 def map 使用全名（如 `aura.math.sin`）而非短名
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceIndex {
    /// 版本（用于向前兼容）
    pub version: u16,

    /// 类型定义映射：类型名 → 源码位置
    /// 例：`{"Int" → {uri: "aura://builtin/Int.aura", line: 12, col: 0}}`
    pub type_defs: HashMap<String, SourceLocation>,

    /// 函数定义映射：函数全名 → 源码位置
    /// 例：`{"aura.math.sin" → {uri: "aura://stdlib/aura/math/Math.aura", line: 85, col: 4}}`
    pub function_defs: HashMap<String, SourceLocation>,

    /// 模块定义映射：模块路径 → 源码文件
    /// 例：`{"aura.math" → {uri: "aura://stdlib/aura/math/Math.aura", file_path: "math/Math.aura"}}`
    pub module_defs: HashMap<String, SourceLocation>,

    /// 常量定义映射：常量全名 → 源码位置
    /// 例：`{"aura.math.PI" → {uri: "aura://stdlib/aura/math/Math.aura", line: 16, col: 4}}`
    pub constant_defs: HashMap<String, SourceLocation>,

    /// 变量定义映射（字段、属性）
    /// 例：`{"String.length" → {uri: "aura://builtin/String.aura", line: 40, col: 4}}`
    pub variable_defs: HashMap<String, SourceLocation>,

    /// 枚举定义映射
    /// 例：`{"Color" → {uri: "file:///project/main.aura", line: 5, col: 0}}`
    pub enum_defs: HashMap<String, SourceLocation>,

    /// 枚举变体映射
    /// 例：`{"Color.Red" → {uri: "file:///project/main.aura", line: 7, col: 4}}`
    pub enum_variant_defs: HashMap<String, SourceLocation>,

    /// 接口定义映射
    /// 例：`{"Comparable" → {uri: "aura://builtin/Comparable.aura", line: 8, col: 0}}`
    pub interface_defs: HashMap<String, SourceLocation>,

    /// phantom source 归档（可选）
    /// 若包含，则 `.auc` 自带全部 phantom source 文本
    pub source_archive: Option<SourceArchive>,
}

impl SourceIndex {
    /// 创建空的源码索引
    pub fn new() -> Self {
        Self {
            version: 1,
            ..Default::default()
        }
    }

    /// 添加类型定义
    pub fn add_type_def(
        &mut self,
        name: impl Into<String>,
        uri: impl Into<String>,
        line: u32,
        col: u32,
        end_line: u32,
        end_col: u32,
    ) {
        let loc = SourceLocation::new(uri, line, col, end_line, end_col);
        self.type_defs.insert(name.into(), loc);
    }

    /// 添加函数定义
    pub fn add_function_def(
        &mut self,
        name: impl Into<String>,
        uri: impl Into<String>,
        line: u32,
        col: u32,
        end_line: u32,
        end_col: u32,
    ) {
        let loc = SourceLocation::new(uri, line, col, end_line, end_col);
        self.function_defs.insert(name.into(), loc);
    }

    /// 添加模块定义
    pub fn add_module_def(
        &mut self,
        path: impl Into<String>,
        uri: impl Into<String>,
        line: u32,
        col: u32,
        end_line: u32,
        end_col: u32,
    ) {
        let loc = SourceLocation::new(uri, line, col, end_line, end_col);
        self.module_defs.insert(path.into(), loc);
    }

    /// 添加常量定义
    pub fn add_constant_def(
        &mut self,
        name: impl Into<String>,
        uri: impl Into<String>,
        line: u32,
        col: u32,
        end_line: u32,
        end_col: u32,
    ) {
        let loc = SourceLocation::new(uri, line, col, end_line, end_col);
        self.constant_defs.insert(name.into(), loc);
    }

    /// 添加变量定义（字段、属性）
    pub fn add_variable_def(
        &mut self,
        name: impl Into<String>,
        uri: impl Into<String>,
        line: u32,
        col: u32,
        end_line: u32,
        end_col: u32,
    ) {
        let loc = SourceLocation::new(uri, line, col, end_line, end_col);
        self.variable_defs.insert(name.into(), loc);
    }

    /// 添加枚举定义
    pub fn add_enum_def(
        &mut self,
        name: impl Into<String>,
        uri: impl Into<String>,
        line: u32,
        col: u32,
        end_line: u32,
        end_col: u32,
    ) {
        let loc = SourceLocation::new(uri, line, col, end_line, end_col);
        self.enum_defs.insert(name.into(), loc);
    }

    /// 添加枚举变体
    pub fn add_enum_variant_def(
        &mut self,
        name: impl Into<String>,
        uri: impl Into<String>,
        line: u32,
        col: u32,
        end_line: u32,
        end_col: u32,
    ) {
        let loc = SourceLocation::new(uri, line, col, end_line, end_col);
        self.enum_variant_defs.insert(name.into(), loc);
    }

    /// 添加接口定义
    pub fn add_interface_def(
        &mut self,
        name: impl Into<String>,
        uri: impl Into<String>,
        line: u32,
        col: u32,
        end_line: u32,
        end_col: u32,
    ) {
        let loc = SourceLocation::new(uri, line, col, end_line, end_col);
        self.interface_defs.insert(name.into(), loc);
    }

    // ── 查找 ──

    /// 通用查找——按名字在全部 defs map 中查找
    ///
    /// 查找顺序：类型 → 函数 → 常量 → 变量 → 枚举 → 枚举变体 → 接口
    /// 第一个命中即返回（类型优先于函数，函数优先于常量）
    pub fn lookup(&self, name: &str) -> Option<&SourceLocation> {
        self.type_defs
            .get(name)
            .or_else(|| self.function_defs.get(name))
            .or_else(|| self.constant_defs.get(name))
            .or_else(|| self.variable_defs.get(name))
            .or_else(|| self.enum_defs.get(name))
            .or_else(|| self.enum_variant_defs.get(name))
            .or_else(|| self.interface_defs.get(name))
    }

    /// 查找类型定义
    pub fn lookup_type(&self, name: &str) -> Option<&SourceLocation> {
        self.type_defs.get(name)
    }

    /// 查找函数定义
    pub fn lookup_function(&self, name: &str) -> Option<&SourceLocation> {
        self.function_defs.get(name)
    }

    /// 查找模块定义
    pub fn lookup_module(&self, path: &str) -> Option<&SourceLocation> {
        self.module_defs.get(path)
    }

    /// 查找常量定义
    pub fn lookup_constant(&self, name: &str) -> Option<&SourceLocation> {
        self.constant_defs.get(name)
    }

    /// 查找变量定义
    pub fn lookup_variable(&self, name: &str) -> Option<&SourceLocation> {
        self.variable_defs.get(name)
    }

    /// 查找枚举定义
    pub fn lookup_enum(&self, name: &str) -> Option<&SourceLocation> {
        self.enum_defs.get(name)
    }

    /// 查找枚举变体
    pub fn lookup_enum_variant(&self, name: &str) -> Option<&SourceLocation> {
        self.enum_variant_defs.get(name)
    }

    /// 查找接口定义
    pub fn lookup_interface(&self, name: &str) -> Option<&SourceLocation> {
        self.interface_defs.get(name)
    }

    // ── 统计 ──

    /// 索引条目总数
    pub fn total_count(&self) -> usize {
        self.type_defs.len()
            + self.function_defs.len()
            + self.module_defs.len()
            + self.constant_defs.len()
            + self.variable_defs.len()
            + self.enum_defs.len()
            + self.enum_variant_defs.len()
            + self.interface_defs.len()
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.total_count() == 0
    }

    /// 序列化为 JSON 字符串（用于调试和测试）
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// 从 JSON 字符串反序列化（用于调试和测试）
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

/// 从 import 声明路径构造 stdlib 函数的 phantom source URI
///
/// 例：`"aura.math.sin"` → `"aura://stdlib/aura/math/Math.aura"`
pub fn stdlib_function_uri(full_name: &str) -> Option<String> {
    let parts: Vec<&str> = full_name.split('.').collect();
    if parts.len() < 3 || parts[0] != "aura" {
        return None;
    }
    let module = parts[1];
    // 模块名 → 文件名映射（PascalCase）
    let file_name = match module {
        "math" => "Math.aura",
        "string" => "String.aura",
        "io" => "IO.aura",
        "collections" => "Collections.aura",
        "fs" => "FileSystem.aura",
        "net" => "Network.aura",
        "json" => "Json.aura",
        "time" => "Time.aura",
        "test" => "Test.aura",
        "builtin" => "Builtin.aura",
        "env" => "Env.aura",
        "process" => "Process.aura",
        "random" => "Random.aura",
        "encoding" => "Encoding.aura",
        "ascii" => "Ascii.aura",
        "console" => "Console.aura",
        "path" => "Path.aura",
        "assert" => "Assert.aura",
        "iter" => "Iter.aura",
        _ => return None,
    };
    Some(format!("aura://stdlib/aura/{}/{}", module, file_name))
}

/// 从 prelude 函数名构造 phantom source URI
///
/// 例：`"println"` → `"aura://prelude/prelu.aura"`
pub fn prelude_uri(_name: &str) -> &'static str {
    "aura://prelude/prelu.aura"
}

/// 基础类型 phantom source URI 映射
///
/// 例：`"Int"` → `"aura://builtin/Int.aura"`
pub fn builtin_type_uri(type_name: &str) -> Option<&'static str> {
    match type_name {
        "Any" => Some("aura://builtin/Any.aura"),
        "Nothing" => Some("aura://builtin/Nothing.aura"),
        "Unit" => Some("aura://builtin/Unit.aura"),
        "Int" => Some("aura://builtin/Int.aura"),
        "Long" => Some("aura://builtin/Long.aura"),
        "Short" => Some("aura://builtin/Short.aura"),
        "Byte" => Some("aura://builtin/Byte.aura"),
        "Float" => Some("aura://builtin/Float.aura"),
        "Double" => Some("aura://builtin/Double.aura"),
        "Boolean" => Some("aura://builtin/Boolean.aura"),
        "Char" => Some("aura://builtin/Char.aura"),
        "String" => Some("aura://builtin/String.aura"),
        "List" => Some("aura://builtin/List.aura"),
        "Map" => Some("aura://builtin/Map.aura"),
        "Array" => Some("aura://builtin/Array.aura"),
        "Function" => Some("aura://builtin/Function.aura"),
        "Type" => Some("aura://builtin/Type.aura"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_location_new() {
        let loc = SourceLocation::new("aura://builtin/Int.aura", 12, 0, 150, 1);
        assert_eq!(loc.uri, "aura://builtin/Int.aura");
        assert_eq!(loc.line, 12);
        assert_eq!(loc.col, 0);
        assert_eq!(loc.end_line, 150);
        assert_eq!(loc.end_col, 1);
        assert!(loc.is_builtin());
        assert!(!loc.is_stdlib());
    }

    #[test]
    fn test_source_location_stdlib() {
        let loc = SourceLocation::new("aura://stdlib/aura/math/Math.aura", 84, 4, 85, 1);
        assert!(loc.is_stdlib());
        assert!(!loc.is_builtin());
        assert_eq!(loc.file_name(), "Math.aura");
    }

    #[test]
    fn test_source_index_add_and_lookup() {
        let mut index = SourceIndex::new();
        index.add_type_def("Int", "aura://builtin/Int.aura", 12, 0, 150, 1);
        index.add_function_def(
            "aura.math.sin",
            "aura://stdlib/aura/math/Math.aura",
            84,
            4,
            85,
            1,
        );
        index.add_constant_def(
            "aura.math.PI",
            "aura://stdlib/aura/math/Math.aura",
            16,
            4,
            16,
            35,
        );
        index.add_module_def("aura.math", "aura://stdlib/aura/math/Math.aura", 1, 0, 1, 0);

        assert_eq!(index.lookup_type("Int").unwrap().line, 12);
        assert_eq!(index.lookup_function("aura.math.sin").unwrap().line, 84);
        assert_eq!(index.lookup_constant("aura.math.PI").unwrap().line, 16);
        assert_eq!(index.lookup_module("aura.math").unwrap().line, 1);
        assert!(index.lookup("Int").is_some());
        assert!(index.lookup("nonexistent").is_none());
    }

    #[test]
    fn test_source_index_total_count() {
        let mut index = SourceIndex::new();
        assert!(index.is_empty());
        assert_eq!(index.total_count(), 0);

        index.add_type_def("Int", "aura://builtin/Int.aura", 12, 0, 150, 1);
        index.add_function_def("sin", "aura://stdlib/aura/math/Math.aura", 84, 4, 85, 1);
        assert_eq!(index.total_count(), 2);
        assert!(!index.is_empty());
    }

    #[test]
    fn test_source_index_serialization() {
        let mut index = SourceIndex::new();
        index.add_type_def("Int", "aura://builtin/Int.aura", 12, 0, 150, 1);
        index.add_function_def(
            "aura.math.sin",
            "aura://stdlib/aura/math/Math.aura",
            84,
            4,
            85,
            1,
        );

        let json = index.to_json().unwrap();
        let deserialized = SourceIndex::from_json(&json).unwrap();

        assert_eq!(deserialized.version, 1);
        assert_eq!(deserialized.type_defs.len(), 1);
        assert_eq!(deserialized.function_defs.len(), 1);
        assert_eq!(
            deserialized.lookup_function("aura.math.sin").unwrap().line,
            84
        );
    }

    #[test]
    fn test_source_archive() {
        let mut archive = SourceArchive::new();
        assert!(archive.is_empty());
        assert_eq!(archive.len(), 0);

        archive.add(
            "aura://builtin/Int.aura",
            "internal value class Int { ... }",
        );
        archive.add(
            "aura://stdlib/aura/math/Math.aura",
            "internal object Math { ... }",
        );

        assert!(!archive.is_empty());
        assert_eq!(archive.len(), 2);
        assert!(archive.get("aura://builtin/Int.aura").is_some());
        assert!(archive.get("nonexistent").is_none());
    }

    #[test]
    fn test_stdlib_function_uri() {
        assert_eq!(
            stdlib_function_uri("aura.math.sin").unwrap(),
            "aura://stdlib/aura/math/Math.aura"
        );
        assert_eq!(
            stdlib_function_uri("aura.string.contains").unwrap(),
            "aura://stdlib/aura/string/String.aura"
        );
        assert_eq!(
            stdlib_function_uri("aura.io.println").unwrap(),
            "aura://stdlib/aura/io/IO.aura"
        );
        // 非 aura.* 路径
        assert!(stdlib_function_uri("unknown.func").is_none());
        // 无模块部分
        assert!(stdlib_function_uri("println").is_none());
    }

    #[test]
    fn test_prelude_uri() {
        assert_eq!(prelude_uri("println"), "aura://prelude/prelu.aura");
        assert_eq!(prelude_uri("abs"), "aura://prelude/prelu.aura");
    }

    #[test]
    fn test_builtin_type_uri() {
        assert_eq!(builtin_type_uri("Int"), Some("aura://builtin/Int.aura"));
        assert_eq!(
            builtin_type_uri("String"),
            Some("aura://builtin/String.aura")
        );
        assert_eq!(builtin_type_uri("Any"), Some("aura://builtin/Any.aura"));
        assert_eq!(builtin_type_uri("List"), Some("aura://builtin/List.aura"));
        assert_eq!(builtin_type_uri("unknown"), None);
    }

    #[test]
    fn test_preload_index() {
        // 模拟预加载所有基础类型和 stdlib 模块到 SourceIndex
        let mut index = SourceIndex::new();

        // 基础类型
        for (name, uri) in [
            ("Int", "aura://builtin/Int.aura"),
            ("Float", "aura://builtin/Float.aura"),
            ("String", "aura://builtin/String.aura"),
            ("Boolean", "aura://builtin/Boolean.aura"),
            ("Any", "aura://builtin/Any.aura"),
            ("Long", "aura://builtin/Long.aura"),
            ("Short", "aura://builtin/Short.aura"),
            ("Byte", "aura://builtin/Byte.aura"),
            ("Double", "aura://builtin/Double.aura"),
            ("Char", "aura://builtin/Char.aura"),
            ("List", "aura://builtin/List.aura"),
            ("Map", "aura://builtin/Map.aura"),
            ("Array", "aura://builtin/Array.aura"),
            ("Function", "aura://builtin/Function.aura"),
            ("Type", "aura://builtin/Type.aura"),
            ("Nothing", "aura://builtin/Nothing.aura"),
            ("Unit", "aura://builtin/Unit.aura"),
        ] {
            index.add_type_def(name, uri, 1, 0, 1, 0);
        }

        // stdlib 模块
        for (module, uri) in [
            ("aura.math", "aura://stdlib/aura/math/Math.aura"),
            ("aura.string", "aura://stdlib/aura/string/String.aura"),
            ("aura.io", "aura://stdlib/aura/io/IO.aura"),
        ] {
            index.add_module_def(module, uri, 1, 0, 1, 0);
        }

        assert!(index.lookup_type("Int").is_some());
        assert!(index.lookup_type("Float").is_some());
        assert!(index.lookup_module("aura.math").is_some());
        assert!(index.lookup("Int").is_some());
        assert!(index.lookup("unknown").is_none());
        assert!(index.total_count() >= 20); // 17 types + 3 modules
    }
}
