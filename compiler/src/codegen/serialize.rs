////! `.auc` 字节码文件格式：序列化 / 反序列化（Phase 2 重设计，设计方案 §6）
//!//!
////! 文件布局（小端）：
//!//! ```text
//!//! "AURA" | u16 version | u32 header_flags |
//!//!   u16 module_name_len | module_name |
//!//!   u16 module_version_len | module_version |
//!//!   u8[16] uuid |
//!//!   u32 consts_count | consts |
//!//!   u16 natives_count | natives |
//!//!   u16 funcs_count | funcs |
//!//!   u16 entry |
//!//!   u16 entry_kind_len | entry_kind |
//!//!   u16 exports_count | exports |
//!//!   u16 imports_count | imports |
//!//!   u16 deps_count | deps |
//!//!   u16 sig_ids_count | sig_ids |
//!//! ```
//!//!
////! 常量标签：0=Int(i64) 1=Float(f64) 2=Str(u32 len + bytes) 3=Bool(u8) 4=Null

use crate::codegen::opcode::{
    AucSegment, BytecodeFunction, BytecodeModule, BytecodeNative, Const, Dependency, ExportSymbol,
    ImportSymbol, ModuleIdentity, SymbolKind,
};
use std::fs;

pub const MAGIC: &[u8; 4] = b"AURA";
/// Phase 4 新格式版本（v4: AOT 机器码嵌入 —— 函数级 aot_mode + 段表 + 段数据区）
///
/// 向后兼容：
/// - v3 及更早的 `.auc` 文件仍可被 v4 读取器加载（函数记录中缺少
///   aot_mode/aot_desc_idx 字段时按 0 处理，段表为空）。
/// - v3 VM 遇到 v4 文件会因 `version > VERSION` 拒绝加载。
pub const VERSION: u16 = 4;

#[derive(Debug)]
pub enum SerializeError {
    Io(String),
    Format(String),
}

impl std::fmt::Display for SerializeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SerializeError::Io(m) => write!(f, "io error: {}", m),
            SerializeError::Format(m) => write!(f, "format error: {}", m),
        }
    }
}

impl From<std::io::Error> for SerializeError {
    fn from(e: std::io::Error) -> Self {
        SerializeError::Io(e.to_string())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 序列化
// ─────────────────────────────────────────────────────────────────────────────

/// 序列化为字节
pub fn to_bytes(module: &BytecodeModule) -> Vec<u8> {
    let mut buf = Vec::new();

    // 头部
    buf.extend_from_slice(MAGIC);
    buf.extend_from_slice(&VERSION.to_le_bytes());
    buf.extend_from_slice(&module.header_flags.to_le_bytes());

    // 模块标识
    write_str(&mut buf, &module.module_identity.name);
    write_str(&mut buf, &module.module_identity.version);
    buf.extend_from_slice(&module.module_identity.uuid);

    // 常量池
    buf.extend_from_slice(&(module.consts.len() as u32).to_le_bytes());
    for c in &module.consts {
        write_const(&mut buf, c);
    }

    // 原生函数
    buf.extend_from_slice(&(module.natives.len() as u16).to_le_bytes());
    for n in &module.natives {
        write_str(&mut buf, &n.name);
        buf.extend_from_slice(&n.param_count.to_le_bytes());
        // P8-Rust: FFI ABI 标记
        buf.push(n.ffi_abi as u8);
        // FFI 库名（可选）
        if let Some(ref lib) = n.ffi_lib {
            buf.push(1);
            write_str(&mut buf, lib);
        } else {
            buf.push(0);
        }
    }

    // 函数
    buf.extend_from_slice(&(module.functions.len() as u16).to_le_bytes());
    for f in &module.functions {
        write_str(&mut buf, &f.name);
        buf.extend_from_slice(&f.param_count.to_le_bytes());
        buf.extend_from_slice(&f.locals.to_le_bytes());
        buf.push(if f.is_native { 1 } else { 0 });
        // v4: 执行模式 + AOT 描述符索引（设计文档 §3.4）
        buf.push(f.aot_mode);
        buf.extend_from_slice(&f.aot_desc_idx.to_le_bytes());
        buf.extend_from_slice(&(f.code.len() as u32).to_le_bytes());
        buf.extend_from_slice(&f.code);
    }

    // 入口
    buf.extend_from_slice(&module.entry.to_le_bytes());
    write_str(&mut buf, &module.entry_kind);

    // 导出符号表
    buf.extend_from_slice(&(module.exports.len() as u16).to_le_bytes());
    for e in &module.exports {
        write_str(&mut buf, &e.name);
        buf.push(e.kind.to_byte());
        write_str(&mut buf, &e.sig_id);
        if let Some(v) = e.func_idx {
            buf.push(1);
            buf.extend_from_slice(&v.to_le_bytes());
        } else {
            buf.push(0);
        }
        if let Some(v) = e.type_table_idx {
            buf.extend_from_slice(&v.to_le_bytes());
        } else {
            buf.extend_from_slice(&0u16.to_le_bytes());
        }
        if let Some(v) = e.const_idx {
            buf.extend_from_slice(&v.to_le_bytes());
        } else {
            buf.extend_from_slice(&0u16.to_le_bytes());
        }
    }

    // 导入符号表
    buf.extend_from_slice(&(module.imports.len() as u16).to_le_bytes());
    for i in &module.imports {
        write_str(&mut buf, &i.name);
        buf.push(i.kind.to_byte());
        write_str(&mut buf, &i.module);
        write_str(&mut buf, &i.symbol);
        write_str(&mut buf, &i.sig_id);
        if let Some(v) = i.func_idx {
            buf.extend_from_slice(&v.to_le_bytes());
        } else {
            buf.extend_from_slice(&0u16.to_le_bytes());
        }
    }

    // 显式依赖列表
    buf.extend_from_slice(&(module.dependencies.len() as u16).to_le_bytes());
    for d in &module.dependencies {
        write_str(&mut buf, &d.module);
        buf.extend_from_slice(&d.uuid);
        write_str(&mut buf, &d.version);
    }

    // 外部模块签名 ID
    buf.extend_from_slice(&(module.sig_ids.len() as u16).to_le_bytes());
    for s in &module.sig_ids {
        write_str(&mut buf, s);
    }

    // Phase 1c: 启用的 std 模块（按需链接）
    buf.extend_from_slice(&(module.enabled_modules.len() as u16).to_le_bytes());
    for m in &module.enabled_modules {
        write_str(&mut buf, m);
    }

    // ── Phase 1 AOT v4: 段表 + 段数据区（设计文档 §3.5）──
    // 段表紧接在 v3 布局末尾；`offset` 相对段数据区起始（即段表之后）。
    buf.extend_from_slice(&(module.aot_segments.len() as u16).to_le_bytes());
    for seg in &module.aot_segments {
        buf.extend_from_slice(&seg.id.to_le_bytes());
        buf.extend_from_slice(&seg.offset.to_le_bytes());
        buf.extend_from_slice(&seg.size.to_le_bytes());
        buf.extend_from_slice(&seg.flags.to_le_bytes());
    }
    buf.extend_from_slice(&module.aot_blob_data);

    buf
}

/// 写入 `.auc` 文件
pub fn write_auc(path: &str, module: &BytecodeModule) -> Result<(), SerializeError> {
    let bytes = to_bytes(module);
    fs::write(path, bytes).map_err(|e| SerializeError::Io(e.to_string()))?;
    Ok(())
}

fn write_const(buf: &mut Vec<u8>, c: &Const) {
    match c {
        Const::Int(i) => {
            buf.push(0);
            buf.extend_from_slice(&i.to_le_bytes());
        }
        Const::Float(fl) => {
            buf.push(1);
            buf.extend_from_slice(&fl.to_le_bytes());
        }
        Const::Str(s) => {
            buf.push(2);
            buf.extend_from_slice(&(s.len() as u32).to_le_bytes());
            buf.extend_from_slice(s.as_bytes());
        }
        Const::Bool(b) => {
            buf.push(3);
            buf.push(if *b { 1 } else { 0 });
        }
        Const::Null => {
            buf.push(4);
        }
    }
}

fn write_str(buf: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    buf.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
    buf.extend_from_slice(bytes);
}

// ─────────────────────────────────────────────────────────────────────────────
// 反序列化
// ─────────────────────────────────────────────────────────────────────────────

/// 从字节反序列化
pub fn from_bytes(bytes: &[u8]) -> Result<BytecodeModule, SerializeError> {
    if bytes.len() < 6 {
        return Err(SerializeError::Format("文件太小".to_string()));
    }
    if &bytes[..4] != MAGIC {
        return Err(SerializeError::Format("魔数不匹配".to_string()));
    }
    let version = u16::from_le_bytes([
        bytes[4], bytes[5],
    ]);
    if version > VERSION {
        return Err(SerializeError::Format(format!(
            "不支持的字节码版本: {}",
            version
        )));
    }

    let mut r = Reader::new(bytes);

    // 跳过魔数 + 版本
    r.advance(4)?;
    let _version = r.u16()?;

    // header_flags
    let header_flags = r.u32()?;

    // 模块标识
    let name = r.str()?;
    let version_str = r.str()?;
    let uuid = r.bytes(16)?;
    let module_identity = ModuleIdentity {
        uuid,
        version: version_str,
        name,
    };

    // 常量池
    let nconsts = r.u32()? as usize;
    let mut consts = Vec::with_capacity(nconsts);
    for _ in 0..nconsts {
        consts.push(read_const(&mut r)?);
    }

    // 原生函数
    let nnatives = r.u16()? as usize;
    let mut natives = Vec::with_capacity(nnatives);
    for _ in 0..nnatives {
        let name = r.str()?;
        let param_count = r.u16()?;
        // P8-Rust: FFI ABI 标记
        let abi_byte = r.u8()?;
        let ffi_abi = match abi_byte {
            0 => crate::codegen::opcode::FfiAbi::None,
            1 => crate::codegen::opcode::FfiAbi::C,
            2 => crate::codegen::opcode::FfiAbi::Rust,
            _ => crate::codegen::opcode::FfiAbi::None,
        };
        // FFI 库名（可选）
        let lib_present = r.u8()?;
        let ffi_lib = if lib_present != 0 { Some(r.str()?) } else { None };
        natives.push(BytecodeNative {
            name,
            param_count,
            ffi_abi,
            ffi_lib,
        });
    }

    // 函数
    let nfuncs = r.u16()? as usize;
    let mut functions = Vec::with_capacity(nfuncs);
    for _ in 0..nfuncs {
        let name = r.str()?;
        let pc = r.u16()?;
        let locals = r.u16()?;
        let is_native = r.u8()? != 0;
        // v4: 执行模式 + AOT 描述符索引（v3 文件无此两字段，按 0 处理）
        let (aot_mode, aot_desc_idx) = if version >= 4 { (r.u8()?, r.u32()?) } else { (0, 0) };
        let code_len = r.u32()? as usize;
        let code = r.take(code_len)?.to_vec();
        functions.push(BytecodeFunction {
            name,
            param_count: pc,
            locals,
            is_native,
            code,
            line_table: None,
            aot_mode,
            aot_desc_idx,
        });
    }

    // 入口
    let entry = r.u16()?;
    let entry_kind = r.str()?;

    // 导出符号表
    let nexports = r.u16()? as usize;
    let mut exports = Vec::with_capacity(nexports);
    for _ in 0..nexports {
        let name = r.str()?;
        let kind_byte = r.u8()?;
        let sig_id = r.str()?;
        let has_func_idx = r.u8()? != 0;
        let func_idx = if has_func_idx { Some(r.u16()?) } else { None };
        let type_table_idx = Some(r.u16()?);
        let const_idx = Some(r.u16()?);
        exports.push(ExportSymbol {
            name,
            kind: SymbolKind::from_byte(kind_byte),
            sig_id,
            func_idx,
            type_table_idx,
            const_idx,
        });
    }

    // 导入符号表
    let nimports = r.u16()? as usize;
    let mut imports = Vec::with_capacity(nimports);
    for _ in 0..nimports {
        let name = r.str()?;
        let kind_byte = r.u8()?;
        let module = r.str()?;
        let symbol = r.str()?;
        let sig_id = r.str()?;
        let func_idx = Some(r.u16()?);
        imports.push(ImportSymbol {
            name,
            kind: SymbolKind::from_byte(kind_byte),
            module,
            symbol,
            sig_id,
            func_idx,
        });
    }

    // 显式依赖列表
    let ndeps = r.u16()? as usize;
    let mut dependencies = Vec::with_capacity(ndeps);
    for _ in 0..ndeps {
        let module = r.str()?;
        let uuid = r.bytes(16)?;
        let version = r.str()?;
        dependencies.push(Dependency {
            module,
            uuid,
            version,
        });
    }

    // 外部模块签名 ID
    let nsigids = r.u16()? as usize;
    let mut sig_ids = Vec::with_capacity(nsigids);
    for _ in 0..nsigids {
        sig_ids.push(r.str()?);
    }

    // Phase 1c: 启用的 std 模块（按需链接）
    let nmods = r.u16()? as usize;
    let mut enabled_modules = Vec::with_capacity(nmods);
    for _ in 0..nmods {
        enabled_modules.push(r.str()?);
    }

    // ── Phase 1 AOT v4: 段表 + 段数据区 ──
    // 段表位置紧接 v3 布局末尾。v3 文件中该位置没有数据，
    // 故仅在 version >= 4 时读取。
    let (mut aot_segments, mut aot_blob_data) = (Vec::new(), Vec::new());
    if version >= 4 && r.pos < r.data.len() {
        let nsegs = r.u16()? as usize;
        aot_segments = Vec::with_capacity(nsegs);
        for _ in 0..nsegs {
            aot_segments.push(AucSegment {
                id: r.u32()?,
                offset: r.u32()?,
                size: r.u32()?,
                flags: r.u32()?,
            });
        }
        if r.pos < r.data.len() {
            aot_blob_data = r.data[r.pos..].to_vec();
            r.pos = r.data.len();
        }
    }

    Ok(BytecodeModule {
        consts,
        natives,
        functions,
        closures: Vec::new(),
        entry,
        enabled_modules,
        module_identity,
        header_flags,
        exports,
        imports,
        dependencies,
        sig_ids,
        entry_kind,
        aot_segments,
        aot_blob_data,
    })
}

/// 读取 `.auc` 文件
pub fn read_auc(path: &str) -> Result<BytecodeModule, SerializeError> {
    let bytes = fs::read(path).map_err(|e| SerializeError::Io(e.to_string()))?;
    from_bytes(&bytes)
}

fn read_const(r: &mut Reader) -> Result<Const, SerializeError> {
    let tag = r.u8()?;
    match tag {
        0 => Ok(Const::Int(r.i64()?)),
        1 => Ok(Const::Float(r.f64()?)),
        2 => {
            let len = r.u32()? as usize;
            let bytes = r.take(len)?;
            let s = String::from_utf8_lossy(bytes).to_string();
            Ok(Const::Str(s))
        }
        3 => Ok(Const::Bool(r.u8()? != 0)),
        4 => Ok(Const::Null),
        _ => Err(SerializeError::Format(format!("未知常量标签: {}", tag))),
    }
}

/// 字节读取器
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Reader {
            data,
            pos: 0,
        }
    }

    fn advance(&mut self, n: usize) -> Result<(), SerializeError> {
        if self.pos + n > self.data.len() {
            return Err(SerializeError::Format("数据越界".to_string()));
        }
        self.pos += n;
        Ok(())
    }

    fn bytes(&mut self, n: usize) -> Result<[u8; 16], SerializeError> {
        let end = self.pos + n;
        if end > self.data.len() {
            return Err(SerializeError::Format("数据越界".to_string()));
        }
        let mut buf = [0u8; 16];
        buf[..n].copy_from_slice(&self.data[self.pos..end]);
        self.pos = end;
        Ok(buf)
    }

    fn u8(&mut self) -> Result<u8, SerializeError> {
        let b = self
            .data
            .get(self.pos)
            .ok_or_else(|| SerializeError::Format("数据越界".to_string()))?;
        self.pos += 1;
        Ok(*b)
    }

    fn u16(&mut self) -> Result<u16, SerializeError> {
        let end = self.pos + 2;
        if end > self.data.len() {
            return Err(SerializeError::Format("数据越界".to_string()));
        }
        let v = u16::from_le_bytes([
            self.data[self.pos],
            self.data[self.pos + 1],
        ]);
        self.pos = end;
        Ok(v)
    }

    fn u32(&mut self) -> Result<u32, SerializeError> {
        let end = self.pos + 4;
        if end > self.data.len() {
            return Err(SerializeError::Format("数据越界".to_string()));
        }
        let v = u32::from_le_bytes([
            self.data[self.pos],
            self.data[self.pos + 1],
            self.data[self.pos + 2],
            self.data[self.pos + 3],
        ]);
        self.pos = end;
        Ok(v)
    }

    fn i64(&mut self) -> Result<i64, SerializeError> {
        let end = self.pos + 8;
        if end > self.data.len() {
            return Err(SerializeError::Format("数据越界".to_string()));
        }
        let v = i64::from_le_bytes([
            self.data[self.pos],
            self.data[self.pos + 1],
            self.data[self.pos + 2],
            self.data[self.pos + 3],
            self.data[self.pos + 4],
            self.data[self.pos + 5],
            self.data[self.pos + 6],
            self.data[self.pos + 7],
        ]);
        self.pos = end;
        Ok(v)
    }

    fn f64(&mut self) -> Result<f64, SerializeError> {
        let end = self.pos + 8;
        if end > self.data.len() {
            return Err(SerializeError::Format("数据越界".to_string()));
        }
        let v = f64::from_le_bytes([
            self.data[self.pos],
            self.data[self.pos + 1],
            self.data[self.pos + 2],
            self.data[self.pos + 3],
            self.data[self.pos + 4],
            self.data[self.pos + 5],
            self.data[self.pos + 6],
            self.data[self.pos + 7],
        ]);
        self.pos = end;
        Ok(v)
    }

    fn str(&mut self) -> Result<String, SerializeError> {
        let len = self.u16()? as usize;
        let bytes = self.take(len)?;
        Ok(String::from_utf8_lossy(bytes).to_string())
    }

    fn take(&mut self, len: usize) -> Result<&[u8], SerializeError> {
        let end = self.pos + len;
        if end > self.data.len() {
            return Err(SerializeError::Format("数据越界".to_string()));
        }
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::opcode::{BytecodeFunction, BytecodeNative, Const, FfiAbi};

    #[test]
    fn test_roundtrip_basic() {
        let module = BytecodeModule {
            consts: vec![
                Const::Int(42),
                Const::Str("hello".to_string()),
            ],
            natives: vec![
                BytecodeNative {
                    name: "println".to_string(),
                    param_count: 1,
                    ffi_abi: FfiAbi::None,
                    ffi_lib: None,
                },
            ],
            functions: vec![
                BytecodeFunction {
                    name: "main".to_string(),
                    param_count: 0,
                    locals: 1,
                    is_native: false,
                    code: vec![
                        0, 0, 0, 28,
                    ], // LoadConst(0), Return
                    line_table: None,
                    aot_mode: 0,
                    aot_desc_idx: 0,
                },
            ],
            entry: 0,
            enabled_modules: vec!["io".to_string()],
            module_identity: ModuleIdentity::new("test", "1.0.0"),
            header_flags: 0,
            exports: vec![],
            imports: vec![],
            dependencies: vec![],
            sig_ids: vec![],
            entry_kind: "app".to_string(),
            ..Default::default()
        };

        let bytes = to_bytes(&module);
        let loaded = from_bytes(&bytes).unwrap();
        assert_eq!(loaded.consts.len(), 2);
        assert_eq!(loaded.natives.len(), 1);
        assert_eq!(loaded.functions.len(), 1);
        assert_eq!(loaded.module_identity.name, "test");
        assert_eq!(loaded.module_identity.version, "1.0.0");
        assert_eq!(loaded.entry, 0);
        assert_eq!(loaded.entry_kind, "app");
    }

    #[test]
    fn test_roundtrip_with_exports() {
        let mut module = BytecodeModule {
            consts: vec![],
            natives: vec![],
            functions: vec![
                BytecodeFunction {
                    name: "add".to_string(),
                    param_count: 2,
                    locals: 2,
                    is_native: false,
                    code: vec![],
                    line_table: None,
                    aot_mode: 0,
                    aot_desc_idx: 0,
                },
            ],
            entry: 0,
            enabled_modules: vec![],
            module_identity: ModuleIdentity::new("math-lib", "1.0.0"),
            header_flags: 0,
            exports: vec![
                ExportSymbol {
                    name: "add".to_string(),
                    kind: SymbolKind::Function,
                    sig_id: "sig-001".to_string(),
                    func_idx: Some(0),
                    type_table_idx: None,
                    const_idx: None,
                },
            ],
            imports: vec![],
            dependencies: vec![],
            sig_ids: vec![],
            entry_kind: "library".to_string(),
            ..Default::default()
        };

        module.header_flags = module.compute_header_flags();
        let bytes = to_bytes(&module);
        let loaded = from_bytes(&bytes).unwrap();
        assert_eq!(loaded.exports.len(), 1);
        assert_eq!(loaded.exports[0].name, "add");
        assert_eq!(loaded.exports[0].kind, SymbolKind::Function);
        assert!(loaded.is_library());
    }

    #[test]
    fn test_roundtrip_with_imports() {
        let mut module = BytecodeModule {
            consts: vec![],
            natives: vec![],
            functions: vec![],
            entry: 0,
            enabled_modules: vec![],
            module_identity: ModuleIdentity::new("app", "1.0.0"),
            header_flags: 0,
            exports: vec![],
            imports: vec![
                ImportSymbol {
                    name: "add".to_string(),
                    kind: SymbolKind::Function,
                    module: "math-lib".to_string(),
                    symbol: "add".to_string(),
                    sig_id: "sig-001".to_string(),
                    func_idx: Some(0),
                },
            ],
            dependencies: vec![
                Dependency {
                    module: "math-lib".to_string(),
                    uuid: ModuleIdentity::new("math-lib", "1.0.0").uuid,
                    version: "1.0.0".to_string(),
                },
            ],
            sig_ids: vec!["sig-001".to_string()],
            entry_kind: "app".to_string(),
            ..Default::default()
        };

        module.header_flags = module.compute_header_flags();
        let bytes = to_bytes(&module);
        let loaded = from_bytes(&bytes).unwrap();
        assert_eq!(loaded.imports.len(), 1);
        assert_eq!(loaded.imports[0].module, "math-lib");
        assert_eq!(loaded.imports[0].symbol, "add");
        assert_eq!(loaded.dependencies.len(), 1);
        assert_eq!(loaded.sig_ids.len(), 1);
    }

    #[test]
    fn test_magic_mismatch() {
        let bytes = b"XXXX\x00\x02";
        assert!(from_bytes(bytes).is_err());
    }

    #[test]
    fn test_version_too_new() {
        let mut bytes = b"AURA\x00\x02".to_vec();
        bytes.extend_from_slice(&1u32.to_le_bytes()); // header_flags
        bytes.extend_from_slice(&5u16.to_le_bytes()); // version = 5
        assert!(from_bytes(&bytes).is_err());
    }
    #[test]
    fn test_roundtrip_v4_with_aot_segments() {
        use crate::codegen::opcode::{
            AucSegment, AuraFuncDesc, HEADER_AOT_EXPORTS, HEADER_HAS_MACHINE_CODE, SEG_DESC_TABLE,
            SEG_MACHINE, SEG_PROT_EXEC, SEG_PROT_READ,
        };
        let desc = AuraFuncDesc {
            entry_offset: 0x100,
            num_args: 2,
            flags: 1,
            ..AuraFuncDesc::default()
        };
        let mut blob: Vec<u8> = vec![0x90u8; 16];
        let db = unsafe {
            std::slice::from_raw_parts(
                &desc as *const AuraFuncDesc as *const u8,
                AuraFuncDesc::SIZE,
            )
        };
        blob.extend_from_slice(db);

        let mut module = BytecodeModule {
            consts: vec![Const::Int(10)],
            natives: vec![],
            functions: vec![
                BytecodeFunction {
                    name: "add".to_string(),
                    param_count: 2,
                    locals: 2,
                    is_native: false,
                    code: vec![
                        1, 0, 1, 0, 1, 0, 77, 0, 0,
                    ],
                    line_table: None,
                    aot_mode: 1,
                    aot_desc_idx: 1,
                },
                BytecodeFunction {
                    name: "main".to_string(),
                    param_count: 0,
                    locals: 1,
                    is_native: false,
                    code: vec![0, 0, 28],
                    line_table: None,
                    aot_mode: 0,
                    aot_desc_idx: 0,
                },
            ],
            entry: 1,
            enabled_modules: vec![],
            module_identity: ModuleIdentity::new("aot-app", "1.0.0"),
            header_flags: 0,
            exports: vec![],
            imports: vec![],
            dependencies: vec![],
            sig_ids: vec![],
            entry_kind: "app".to_string(),
            aot_segments: vec![
                AucSegment {
                    id: SEG_MACHINE,
                    offset: 0,
                    size: 16,
                    flags: SEG_PROT_READ | SEG_PROT_EXEC,
                },
                AucSegment {
                    id: SEG_DESC_TABLE,
                    offset: 16,
                    size: 32,
                    flags: SEG_PROT_READ,
                },
            ],
            aot_blob_data: blob,
            ..Default::default()
        };
        module.header_flags = module.compute_header_flags();
        assert!(module.has_aot());
        assert_ne!(module.header_flags & HEADER_HAS_MACHINE_CODE, 0);
        assert_ne!(module.header_flags & HEADER_AOT_EXPORTS, 0);

        let bytes = to_bytes(&module);
        let loaded = from_bytes(&bytes).unwrap();
        assert_eq!(loaded.consts.len(), 1);
        assert_eq!(loaded.functions.len(), 2);
        assert_eq!(loaded.functions[0].name, "add");
        assert_eq!(loaded.functions[0].aot_mode, 1);
        assert_eq!(loaded.functions[0].aot_desc_idx, 1);
        assert_eq!(loaded.functions[1].aot_mode, 0);
        assert_eq!(loaded.functions[1].aot_desc_idx, 0);
        assert_eq!(loaded.functions[0].code.len(), 9);
        assert_eq!(
            loaded.functions[0].code[6], 77,
            "CALL_AOT opcode byte survives round-trip"
        );
        assert_eq!(loaded.entry, 1);
        assert_eq!(loaded.aot_segments.len(), 2);
        assert_eq!(loaded.aot_segments[0].id, SEG_MACHINE);
        assert!(loaded.aot_segments[0].is_exec());
        assert!(!loaded.aot_segments[1].is_exec());
        assert_eq!(loaded.aot_segments[1].id, SEG_DESC_TABLE);
        assert_eq!(loaded.aot_segments[1].offset, 16);
        assert_eq!(loaded.aot_segments[1].size, 32);
        assert_eq!(loaded.aot_blob_data.len(), 48);
        assert!(loaded.has_aot());
        assert_ne!(loaded.header_flags & HEADER_HAS_MACHINE_CODE, 0);
        assert_ne!(loaded.header_flags & HEADER_AOT_EXPORTS, 0);

        // 段数据可直接交给 AotModule 消费
        let idx: Vec<u32> = loaded.functions.iter().map(|f| f.aot_desc_idx).collect();
        let mut m = crate::vm::aot_runtime::AotModule::load(
            &loaded.aot_blob_data,
            &loaded.aot_segments,
            &idx,
            1,
            "roundtrip".to_string(),
        )
        .unwrap();
        assert_eq!(m.func_descriptors.len(), 1);
        assert_eq!(m.func_descriptors[0].entry_offset, 0x100);
        assert!(m.find_entry(0).is_some(), "func 0 has AOT entry");
        assert!(m.find_entry(1).is_none(), "func 1 (main) has no AOT entry");
        m.unload();
    }
    #[test]
    fn test_roundtrip_v3_backward_compat() {
        // 手工构造 v3 字节流（无 aot_mode/aot_desc_idx、无段表），
        // 验证 v4 读取器对 v3 文件向后兼容（设计文档 §3.4）。
        let mut b: Vec<u8> = Vec::new();
        b.extend_from_slice(b"AURA");
        b.extend_from_slice(&3u16.to_le_bytes()); // VERSION 3
        b.extend_from_slice(&0u32.to_le_bytes()); // header_flags
        // module_identity: name="", version="", uuid=[0;16]
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(&[0u8; 16]);
        // consts = 0
        b.extend_from_slice(&0u32.to_le_bytes());
        // natives = 0
        b.extend_from_slice(&0u16.to_le_bytes());
        // functions = 1: main, pc=0, locals=1, native=0, code=[LoadConst(0), Return]
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&4u16.to_le_bytes());
        b.extend_from_slice(b"main");
        b.extend_from_slice(&0u16.to_le_bytes()); // param_count
        b.extend_from_slice(&1u16.to_le_bytes()); // locals
        b.push(0); // is_native
        let code = [
            0u8, 0, 0, 0x1C,
        ];
        b.extend_from_slice(&(code.len() as u32).to_le_bytes());
        b.extend_from_slice(&code);
        // entry = 0, entry_kind = "app"
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(&3u16.to_le_bytes());
        b.extend_from_slice(b"app");
        // exports=0 imports=0 deps=0 sig_ids=0 enabled=0
        for _ in 0..5 {
            b.extend_from_slice(&0u16.to_le_bytes());
        }

        let m = from_bytes(&b).unwrap();
        assert_eq!(m.functions.len(), 1);
        assert_eq!(m.functions[0].name, "main");
        assert_eq!(m.functions[0].aot_mode, 0, "v3 文件按 aot_mode=0 处理");
        assert_eq!(m.functions[0].aot_desc_idx, 0);
        assert_eq!(m.entry_kind, "app");
        assert!(m.aot_segments.is_empty(), "v3 文件无 AOT 段");
        assert!(!m.has_aot());
        // v4 重新序列化后仍可读
        let v4 = to_bytes(&m);
        let m2 = from_bytes(&v4).unwrap();
        assert_eq!(m2.functions[0].name, "main");
        assert_eq!(m2.functions[0].aot_mode, 0);
    }
}
