////! Phase 3: `.sig` 类型签名格式（设计方案 §10）
////!
//! .sig 文件描述模块的类型信息，与 .auc 分离，用于编译期类型检查和链接。
//! 包含：函数签名、类型定义、常量名称和值、导出/导入声明。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;

/// 签名文件格式版本
pub const SIG_VERSION: u16 = 1;
/// .sig 文件魔数
pub const SIG_MAGIC: &[u8; 4] = b"ASIG";

/// 签名错误
#[derive(Debug)]
pub enum SigError {
    Io(String),
    Format(String),
    Parse(String),
}

impl std::fmt::Display for SigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SigError::Io(m) => write!(f, "io error: {}", m),
            SigError::Format(m) => write!(f, "format error: {}", m),
            SigError::Parse(m) => write!(f, "parse error: {}", m),
        }
    }
}

impl From<std::io::Error> for SigError {
    fn from(e: std::io::Error) -> Self {
        SigError::Io(e.to_string())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 签名数据结构
// ─────────────────────────────────────────────────────────────────────────────

/// 类型签名
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TypeSig {
    Int,
    Float,
    Bool,
    Str,
    Null,
    /// 引用类型 (T)
    Ref(Box<TypeSig>),
    /// 元组 (T1, T2, ...)
    Tuple(Vec<TypeSig>),
    /// 函数类型 (T1, T2, ...) -> T
    Func(Vec<TypeSig>, Box<TypeSig>),
    /// 泛型类型 T<K, V>
    Generic(String, Vec<TypeSig>),
    /// 用户定义类型（结构体/类/接口/枚举）
    UserType {
        name: String,
        type_params: Vec<String>,
    },
}

impl TypeSig {
    /// 类型签名的字符串表示
    pub fn to_string(&self) -> String {
        match self {
            TypeSig::Int => "int".to_string(),
            TypeSig::Float => "float".to_string(),
            TypeSig::Bool => "bool".to_string(),
            TypeSig::Str => "str".to_string(),
            TypeSig::Null => "null".to_string(),
            TypeSig::Ref(t) => format!("&{}", t.to_string()),
            TypeSig::Tuple(ts) => {
                format!(
                    "({})",
                    ts.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(", ")
                )
            }
            TypeSig::Func(params, ret) => {
                format!(
                    "({}) -> {}",
                    params.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(", "),
                    ret.to_string()
                )
            }
            TypeSig::Generic(name, params) => {
                format!(
                    "{}<{}>",
                    name,
                    params.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(", ")
                )
            }
            TypeSig::UserType {
                name,
                type_params,
            } => {
                if type_params.is_empty() {
                    name.clone()
                } else {
                    format!("{}<{}>", name, type_params.join(", "))
                }
            }
        }
    }
}

/// 函数签名
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FuncSig {
    pub name: String,
    pub params: Vec<TypeSig>,
    pub return_type: TypeSig,
    pub is_public: bool,
    /// 泛型参数名
    pub type_params: Vec<String>,
}

/// 类型定义签名（结构体/类/接口/枚举）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TypeDefSig {
    pub name: String,
    pub kind: TypeDefKind,
    /// 泛型参数名
    pub type_params: Vec<String>,
    /// 字段列表（结构体/类）
    pub fields: Vec<FieldSig>,
    /// 方法列表（类/接口）
    pub methods: Vec<FuncSig>,
    /// 枚举变体
    pub variants: Vec<VariantSig>,
    /// 父类型/实现的接口
    pub super_types: Vec<String>,
    pub is_public: bool,
}

/// 类型定义种类
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypeDefKind {
    Struct,
    Class,
    Interface,
    Enum,
    TypeAlias,
}

/// 字段签名
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FieldSig {
    pub name: String,
    pub type_sig: TypeSig,
    pub is_public: bool,
    pub is_mutable: bool,
}

/// 枚举变体签名
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VariantSig {
    pub name: String,
    /// 变体关联的类型（如 tuple variant）
    pub fields: Vec<TypeSig>,
    pub is_public: bool,
}

/// 常量签名
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConstSig {
    pub name: String,
    pub type_sig: TypeSig,
    /// 常量值（如果是字面量）
    pub value: Option<ConstValue>,
    pub is_public: bool,
}

/// 常量值
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ConstValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
}

/// 导入声明签名
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportSig {
    pub module: String,
    pub symbols: Vec<ImportSymbolSig>,
    /// 别名映射: 原符号名 -> 别名
    pub aliases: BTreeMap<String, String>,
}

/// 导入符号签名
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportSymbolSig {
    pub name: String,
    pub kind: SymbolKind,
}

/// 符号类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolKind {
    Function,
    Type,
    Const,
}

/// 完整的模块签名（.sig 文件内容）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModuleSig {
    /// 模块名称
    pub module_name: String,
    /// 模块版本
    pub module_version: String,
    /// 模块 UUID
    pub uuid: [u8; 16],
    /// 签名格式版本
    pub version: u16,
    /// 函数签名列表
    pub functions: Vec<FuncSig>,
    /// 类型定义签名列表
    pub types: Vec<TypeDefSig>,
    /// 常量签名列表
    pub constants: Vec<ConstSig>,
    /// 导入声明
    pub imports: Vec<ImportSig>,
    /// 依赖模块列表
    pub dependencies: Vec<String>,
}

impl ModuleSig {
    /// 创建空的模块签名
    pub fn new(module_name: &str, module_version: &str, uuid: [u8; 16]) -> Self {
        ModuleSig {
            module_name: module_name.to_string(),
            module_version: module_version.to_string(),
            uuid,
            version: SIG_VERSION,
            functions: Vec::new(),
            types: Vec::new(),
            constants: Vec::new(),
            imports: Vec::new(),
            dependencies: Vec::new(),
        }
    }

    /// 查找函数签名
    pub fn find_function(&self, name: &str) -> Option<&FuncSig> {
        self.functions.iter().find(|f| f.name == name)
    }

    /// 查找类型定义
    pub fn find_type(&self, name: &str) -> Option<&TypeDefSig> {
        self.types.iter().find(|t| t.name == name)
    }

    /// 查找常量签名
    pub fn find_constant(&self, name: &str) -> Option<&ConstSig> {
        self.constants.iter().find(|c| c.name == name)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 序列化/反序列化
// ─────────────────────────────────────────────────────────────────────────────

/// 序列化为字节
pub fn to_bytes(sig: &ModuleSig) -> Result<Vec<u8>, SigError> {
    let mut buf = Vec::new();
    buf.extend_from_slice(SIG_MAGIC);
    buf.extend_from_slice(&SIG_VERSION.to_le_bytes());

    // 使用 serde_json 序列化签名内容
    let json = serde_json::to_vec(sig).map_err(|e| SigError::Format(e.to_string()))?;
    buf.extend_from_slice(&(json.len() as u32).to_le_bytes());
    buf.extend_from_slice(&json);

    Ok(buf)
}

/// 从字节反序列化
pub fn from_bytes(bytes: &[u8]) -> Result<ModuleSig, SigError> {
    if bytes.len() < 8 {
        return Err(SigError::Format("文件太小".to_string()));
    }
    if &bytes[..4] != SIG_MAGIC {
        return Err(SigError::Format("魔数不匹配".to_string()));
    }
    let version = u16::from_le_bytes([
        bytes[4], bytes[5],
    ]);
    if version != SIG_VERSION {
        return Err(SigError::Format(format!("不支持的签名版本: {}", version)));
    }
    let json_len = u32::from_le_bytes([
        bytes[6], bytes[7], bytes[8], bytes[9],
    ]) as usize;
    if bytes.len() < 10 + json_len {
        return Err(SigError::Format("数据越界".to_string()));
    }
    let json = &bytes[10..10 + json_len];
    serde_json::from_slice(json).map_err(|e| SigError::Parse(e.to_string()))
}

/// 写入 .sig 文件
pub fn write_sig(path: &str, sig: &ModuleSig) -> Result<(), SigError> {
    let bytes = to_bytes(sig)?;
    fs::write(path, bytes).map_err(|e| SigError::Io(e.to_string()))?;
    Ok(())
}

/// 读取 .sig 文件
pub fn read_sig(path: &str) -> Result<ModuleSig, SigError> {
    let bytes = fs::read(path).map_err(|e| SigError::Io(e.to_string()))?;
    from_bytes(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_sig_to_string() {
        assert_eq!(TypeSig::Int.to_string(), "int");
        assert_eq!(TypeSig::Str.to_string(), "str");
        assert_eq!(TypeSig::Ref(Box::new(TypeSig::Int)).to_string(), "&int");
        assert_eq!(
            TypeSig::Func(
                vec![
                    TypeSig::Int,
                    TypeSig::Str
                ],
                Box::new(TypeSig::Bool)
            )
            .to_string(),
            "(int, str) -> bool"
        );
    }

    #[test]
    fn test_module_sig_roundtrip() {
        let sig = ModuleSig {
            module_name: "math-lib".to_string(),
            module_version: "1.0.0".to_string(),
            uuid: [0u8; 16],
            version: SIG_VERSION,
            functions: vec![FuncSig {
                name: "add".to_string(),
                params: vec![
                    TypeSig::Int,
                    TypeSig::Int,
                ],
                return_type: TypeSig::Int,
                is_public: true,
                type_params: vec![],
            }],
            types: vec![],
            constants: vec![ConstSig {
                name: "PI".to_string(),
                type_sig: TypeSig::Float,
                value: Some(ConstValue::Float(3.14159)),
                is_public: true,
            }],
            imports: vec![],
            dependencies: vec![],
        };

        let bytes = to_bytes(&sig).unwrap();
        let loaded = from_bytes(&bytes).unwrap();
        assert_eq!(loaded.module_name, "math-lib");
        assert_eq!(loaded.functions.len(), 1);
        assert_eq!(loaded.functions[0].name, "add");
        assert_eq!(loaded.constants.len(), 1);
        assert_eq!(loaded.constants[0].name, "PI");
    }

    #[test]
    fn test_func_sig_lookup() {
        let sig = ModuleSig::new("test", "1.0.0", [0u8; 16]);
        let sig = ModuleSig {
            module_name: "test".to_string(),
            module_version: "1.0.0".to_string(),
            uuid: [0u8; 16],
            version: SIG_VERSION,
            functions: vec![
                FuncSig {
                    name: "foo".to_string(),
                    params: vec![],
                    return_type: TypeSig::Int,
                    is_public: true,
                    type_params: vec![],
                },
                FuncSig {
                    name: "bar".to_string(),
                    params: vec![TypeSig::Str],
                    return_type: TypeSig::Null,
                    is_public: false,
                    type_params: vec![],
                },
            ],
            types: vec![],
            constants: vec![],
            imports: vec![],
            dependencies: vec![],
        };
        assert!(sig.find_function("foo").is_some());
        assert!(sig.find_function("bar").is_some());
        assert!(sig.find_function("baz").is_none());
    }

    #[test]
    fn test_magic_mismatch() {
        let bytes = b"XXXX\x00\x01\x00\x00\x00\x00";
        assert!(from_bytes(bytes).is_err());
    }
}
