//! Phase 1 — std 函数名元数据层
//!
//! 单一真相源：编译期（checker / hir / symbol / mir）与运行期（NativeRegistry）
//! 共用同一份函数清单，消除「编译期看不见 / 运行期抢名」的分裂。
//!
//! 设计原则（按需免import）：
//! - **Prelude**（17 个）：免import，始终可用（println/abs/sqrt/...）
//! - **命名空间库**（320 个）：需 `import` 后才可用（aura.math.sin/...）
//!
//! 使用方式：
//! - `is_prelude(name)` — 判断是否为免import的 prelude 函数
//! - `is_builtin(name)` — 判断是否为任意内置函数（含命名空间库）
//! - `all_names()` — 返回全部内置名集合
//! - `prelu_names()` — 返回 prelude 名集合

use std::collections::HashSet;
use std::sync::OnceLock;

/// 免import的 prelude 函数名（17 个全局内置）
pub const PRELUDE_NAMES: &[&str] = &[
    "println", "print", "puts", "abs", "sqrt", "pow",
    "toInt", "toFloat", "toStr", "toString", "clock", "strlen",
    "CString", "CStr", "ptrIsNull", "ptrToInt", "intToPtr",
    "makeCallback",
];

/// 全部内置函数名（编译期可见的单一真相源）
pub static ALL_NAMES: OnceLock<HashSet<&'static str>> = OnceLock::new();

/// Prelude 函数名集合（惰性初始化）
pub static PRELUDE_SET: OnceLock<HashSet<&'static str>> = OnceLock::new();

/// 返回全部内置函数名集合（惰性初始化，进程内只构建一次）
pub fn all_names() -> &'static HashSet<&'static str> {
    ALL_NAMES.get_or_init(build_all_names)
}

/// 返回 prelude 函数名集合
pub fn prelu_names() -> &'static HashSet<&'static str> {
    PRELUDE_SET.get_or_init(|| PRELUDE_NAMES.iter().copied().collect())
}

/// 判断一个名字是否为 prelu 函数（免import，始终可用）
pub fn is_prelude(name: &str) -> bool {
    prelu_names().contains(name)
}

/// 判断一个名字是否为任意内置函数（含命名空间库，需import）
#[deprecated(since = "0.2.0", note = "使用 is_prelude() 或 is_namespaced() 代替")]
pub fn is_builtin(name: &str) -> bool {
    all_names().contains(name)
}

/// 判断一个名字是否为命名空间库函数（需import）
pub fn is_namespaced(name: &str) -> bool {
    all_names().contains(name) && !is_prelude(name)
}

/// 获取指定模块的所有函数短名（去掉模块前缀）
///
/// 例如：`module_functions("aura.math")` → `["sin", "cos", "tan", ...]`
/// 用于 `import aura.math.*` 展开到符号表。
///
/// 注：返回短名（不含模块前缀），调用时使用短名。
pub fn module_functions(module_path: &str) -> Vec<String> {
    let prefix = format!("{}.", module_path);
    all_names()
        .iter()
        .filter(|name| name.starts_with(&prefix))
        .map(|name| name[prefix.len()..].to_string())
        .collect()
}

/// 获取指定模块的所有函数全名（含模块前缀）
///
/// 例如：`module_functions_full("aura.math")` → `["aura.math.sin", "aura.math.cos", ...]`
pub fn module_functions_full(module_path: &str) -> Vec<&'static str> {
    let prefix = format!("{}.", module_path);
    all_names()
        .iter()
        .filter(|name| name.starts_with(&prefix))
        .copied()
        .collect()
}

fn build_all_names() -> HashSet<&'static str> {
    let mut s = HashSet::new();

    // ── 顶层内置（native.rs 直接注册，无命名空间前缀）──
    // println / print / puts / abs / sqrt / pow / toInt / toFloat / toStr /
    // toString / clock / strlen / CString / CStr / ptrIsNull / ptrToInt /
    // intToPtr / makeCallback
    for n in [
        "println", "print", "puts", "abs", "sqrt", "pow",
        "toInt", "toFloat", "toStr", "toString", "clock", "strlen",
        "CString", "CStr", "ptrIsNull", "ptrToInt", "intToPtr",
        "makeCallback",
    ] { s.insert(n); }

    // ── aura.concurrent.* — 协程 / Actor / Channel（native.rs 注册）──
    for n in [
        "aura.concurrent.spawn", "aura.concurrent.send",
        "aura.concurrent.ask", "aura.concurrent.newChannel",
        "aura.concurrent.channelSend", "aura.concurrent.channelRecv",
        "aura.concurrent.channelTryRecv", "aura.concurrent.select",
        "aura.concurrent.spawnActor", "aura.concurrent.supervise",
        "aura.concurrent.actorAlive",
    ] { s.insert(n); }

    // ── aura.ascii.* — 字符工具（std_ascii.rs）──
    for n in [
        "aura.ascii.isAlpha", "aura.ascii.isDigit",
        "aura.ascii.isAlphaNumeric", "aura.ascii.isWhitespace",
        "aura.ascii.isUpper", "aura.ascii.isLower",
        "aura.ascii.toUpper", "aura.ascii.toLower",
        "aura.ascii.codeAt", "aura.ascii.charAt",
        "aura.ascii.fromCode", "aura.ascii.codePointAt",
    ] { s.insert(n); }

    // ── aura.assert.* — 通用断言（std_assert.rs）──
    for n in [
        "aura.assert.assert", "aura.assert.assertTrue",
        "aura.assert.assertFalse", "aura.assert.assertEq",
        "aura.assert.assertNotEq", "aura.assert.assertNotNull",
        "aura.assert.assertNull", "aura.assert.debugAssert",
    ] { s.insert(n); }

    // ── aura.builtin.* — 编译期内置（std_builtin.rs）──
    for n in [
        "aura.builtin.typeof", "aura.builtin.typeOf",
        "aura.builtin.isNull", "aura.builtin.isNotNull",
        "aura.builtin.isZero", "aura.builtin.isPositive",
        "aura.builtin.isNegative", "aura.builtin.toString",
        "aura.builtin.toInt", "aura.builtin.toFloat",
        "aura.builtin.toBool", "aura.builtin.sizeOf",
        "aura.builtin.hash", "aura.builtin.compare",
        "aura.builtin.clone", "aura.builtin.identity",
    ] { s.insert(n); }

    // ── aura.collections.* — 集合辅助（std_collections.rs）──
    for n in [
        "aura.collections.listOf", "aura.collections.mutableListOf",
        "aura.collections.emptyList", "aura.collections.arrayOf",
        "aura.collections.listContains", "aura.collections.listIndexOf",
        "aura.collections.listRemove", "aura.collections.listReverse",
        "aura.collections.listSort", "aura.collections.listGet",
        "aura.collections.listSet", "aura.collections.listInsert",
        "aura.collections.listSubList",
        "aura.collections.mapOf", "aura.collections.mutableMapOf",
        "aura.collections.emptyMap", "aura.collections.mapContains",
        "aura.collections.mapContainsKey", "aura.collections.mapContainsValue",
        "aura.collections.mapRemove", "aura.collections.mapKeys",
        "aura.collections.mapValues",
        "aura.collections.setOf", "aura.collections.mutableSetOf",
        "aura.collections.emptySet",
    ] { s.insert(n); }

    // ── aura.console.* — 终端控制（std_console.rs）──
    for n in [
        "aura.console.clear",
        "aura.console.cursorUp", "aura.console.cursorDown",
        "aura.console.cursorLeft", "aura.console.cursorRight",
        "aura.console.cursorShow", "aura.console.cursorHide",
        "aura.console.reset",
        "aura.console.red", "aura.console.green",
        "aura.console.yellow", "aura.console.blue",
        "aura.console.magenta", "aura.console.cyan",
        "aura.console.white", "aura.console.bold",
        "aura.console.italic", "aura.console.underline",
        "aura.console.dim", "aura.console.inverse",
        "aura.console.size", "aura.console.width",
        "aura.console.height",
    ] { s.insert(n); }

    // ── aura.encoding.* — 编码/解码（std_encoding.rs）──
    for n in [
        "aura.encoding.base64Encode", "aura.encoding.base64Decode",
        "aura.encoding.hexEncode", "aura.encoding.hexDecode",
        "aura.encoding.urlEncode", "aura.encoding.urlDecode",
        "aura.encoding.byteToHex", "aura.encoding.hexToByte",
    ] { s.insert(n); }

    // ── aura.env.* — 环境变量（std_env.rs）──
    for n in [
        "aura.env.get", "aura.env.set", "aura.env.remove",
        "aura.env.has", "aura.env.keys", "aura.env.values",
        "aura.env.all", "aura.env.home", "aura.env.tmp",
        "aura.env.pwd", "aura.env.platform", "aura.env.os",
        "aura.env.arch",
    ] { s.insert(n); }

    // ── aura.fs.* — 文件系统（std_fs.rs）──
    for n in [
        "aura.fs.exists", "aura.fs.isFile",
        "aura.fs.isDirectory", "aura.fs.readText",
        "aura.fs.writeText", "aura.fs.readBytes",
        "aura.fs.writeBytes", "aura.fs.delete",
        "aura.fs.mkdir", "aura.fs.mkdirP", "aura.fs.rename",
        "aura.fs.copy", "aura.fs.listDir", "aura.fs.listFiles",
        "aura.fs.fileSize", "aura.fs.lastModified",
        "aura.fs.absolutePath", "aura.fs.homeDir",
        "aura.fs.tempDir", "aura.fs.currentDir", "aura.fs.walk",
    ] { s.insert(n); }

    // ── aura.io.* — 标准输入输出（std_io.rs）──
    for n in [
        "aura.io.println", "aura.io.print",
        "aura.io.readLine", "aura.io.readAll",
        "aura.io.flush", "aura.io.fileRead",
        "aura.io.fileWrite", "aura.io.fileExists",
        "aura.io.writeFile", "aura.io.readFile",
    ] { s.insert(n); }

    // ── aura.iter.* — 迭代器/函数式（std_iter.rs）──
    for n in [
        "aura.iter.sum", "aura.iter.avg",
        "aura.iter.min", "aura.iter.max",
        "aura.iter.product", "aura.iter.contains",
        "aura.iter.indexOf", "aura.iter.count",
        "aura.iter.every", "aura.iter.some",
        "aura.iter.flatMap", "aura.iter.zip",
        "aura.iter.unzip", "aura.iter.enumerate",
        "aura.iter.chain", "aura.iter.take",
        "aura.iter.skip", "aura.iter.dropWhile",
        "aura.iter.takeWhile", "aura.iter.distinct",
        "aura.iter.groupBy", "aura.iter.partition",
        "aura.iter.fold", "aura.iter.scan",
        "aura.iter.toMap", "aura.iter.toList",
        "aura.iter.range", "aura.iter.rangeTo",
        "aura.iter.rangeUntil", "aura.iter.repeatN",
    ] { s.insert(n); }

    // ── aura.json.* — JSON 解析与序列化（std_json.rs）──
    for n in [
        "aura.json.parse", "aura.json.stringify",
        "aura.json.isValid", "aura.json.get",
        "aura.json.set", "aura.json.keys",
        "aura.json.values", "aura.json.length",
        "aura.json.contains", "aura.json.remove",
    ] { s.insert(n); }

    // ── aura.math.* — 数学函数与常量（std_math.rs）──
    for n in [
        "aura.math.abs", "aura.math.min", "aura.math.max",
        "aura.math.ceil", "aura.math.floor",
        "aura.math.round", "aura.math.trunc",
        "aura.math.sqrt", "aura.math.cbrt",
        "aura.math.pow", "aura.math.exp",
        "aura.math.log", "aura.math.log2",
        "aura.math.log10", "aura.math.sin",
        "aura.math.cos", "aura.math.tan",
        "aura.math.asin", "aura.math.acos",
        "aura.math.atan", "aura.math.atan2",
        "aura.math.PI", "aura.math.E",
        "aura.math.INT_MAX", "aura.math.INT_MIN",
        "aura.math.FLOAT_MAX", "aura.math.sign",
        "aura.math.clamp",
    ] { s.insert(n); }

    // ── aura.net.* — 网络 Socket（std_net.rs）──
    for n in [
        "aura.net.tcpConnect", "aura.net.tcpListen",
        "aura.net.tcpSend", "aura.net.tcpRecv",
        "aura.net.tcpClose", "aura.net.udpSend",
        "aura.net.udpRecv", "aura.net.udpClose",
        "aura.net.isHostReachable", "aura.net.getHostname",
        "aura.net.getLocalIp",
    ] { s.insert(n); }

    // ── aura.path.* — 路径操作（std_path.rs）──
    for n in [
        "aura.path.join", "aura.path.dirname",
        "aura.path.basename", "aura.path.extname",
        "aura.path.relative", "aura.path.resolve",
        "aura.path.normalize", "aura.path.isAbsolute",
        "aura.path.isRelative", "aura.path.split",
        "aura.path.separators", "aura.path.fromUnix",
        "aura.path.fromWindows",
    ] { s.insert(n); }

    // ── aura.process.* — 进程管理（std_process.rs）──
    for n in [
        "aura.process.exit", "aura.process.exitCode",
        "aura.process.args", "aura.process.arg",
        "aura.process.argCount", "aura.process.pid",
        "aura.process.spawn", "aura.process.kill",
        "aura.process.wait", "aura.process.exitProcess",
    ] { s.insert(n); }

    // ── aura.random.* — 随机数（std_random.rs）──
    for n in [
        "aura.random.nextInt", "aura.random.nextLong",
        "aura.random.nextFloat", "aura.random.nextDouble",
        "aura.random.nextBool", "aura.random.nextIntRange",
        "aura.random.nextFloatRange", "aura.random.choice",
        "aura.random.shuffle", "aura.random.seed",
        "aura.random.random",
    ] { s.insert(n); }

    // ── aura.string.* — 字符串操作（std_string.rs）──
    for n in [
        "aura.string.contains", "aura.string.startsWith",
        "aura.string.endsWith", "aura.string.split",
        "aura.string.join", "aura.string.replace",
        "aura.string.replaceAll", "aura.string.trim",
        "aura.string.trimStart", "aura.string.trimEnd",
        "aura.string.substring", "aura.string.substringBefore",
        "aura.string.substringAfter", "aura.string.toLowerCase",
        "aura.string.toUpperCase", "aura.string.length",
        "aura.string.isEmpty", "aura.string.format",
        "aura.string.repeat", "aura.string.indexOf",
        "aura.string.lastIndexOf", "aura.string.padStart",
        "aura.string.padEnd", "aura.string.escape",
        "aura.string.unescape", "aura.string.splitLines",
        "aura.string.joinLines", "aura.string.countChar",
        "aura.string.first", "aura.string.last",
        "aura.string.isBlank", "aura.string.matches",
        "aura.string.containsAny", "aura.string.containsAll",
    ] { s.insert(n); }

    // ── aura.test.* — 测试断言（std_test.rs）──
    for n in [
        "aura.test.assertTrue", "aura.test.assertFalse",
        "aura.test.assertEq", "aura.test.assertNotEq",
        "aura.test.assertNotNull", "aura.test.assertNull",
        "aura.test.assertContains", "aura.test.assertNotContains",
        "aura.test.assertThrows", "aura.test.assertGt",
        "aura.test.assertGte", "aura.test.assertLt",
        "aura.test.assertLte", "aura.test.assertApprox",
        "aura.test.assertArrayEq", "aura.test.assertMapEq",
        "aura.test.pass", "aura.test.fail",
    ] { s.insert(n); }

    // ── aura.time.* — 时间/日期（std_time.rs）──
    for n in [
        "aura.time.now", "aura.time.epoch",
        "aura.time.currentTime", "aura.time.sleep",
        "aura.time.duration", "aura.time.toDateString",
        "aura.time.toTimeString", "aura.time.formatDate",
        "aura.time.diff", "aura.time.parseDate",
    ] { s.insert(n); }

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_names_count() {
        let names = all_names();
        assert!(names.len() >= 300, "expected at least 300 names, got {}", names.len());
    }

    #[test]
    fn test_is_builtin_basic() {
        assert!(is_builtin("println"));
        assert!(is_builtin("aura.math.sin"));
        assert!(is_builtin("aura.concurrent.spawn"));
        assert!(!is_builtin("myFunction"));
        assert!(!is_builtin(""));
    }

    #[test]
    fn test_all_modules_represented() {
        let names = all_names();
        // 每个命名空间至少有 1 个函数
        assert!(names.iter().any(|n| n.starts_with("aura.ascii.")));
        assert!(names.iter().any(|n| n.starts_with("aura.assert.")));
        assert!(names.iter().any(|n| n.starts_with("aura.builtin.")));
        assert!(names.iter().any(|n| n.starts_with("aura.collections.")));
        assert!(names.iter().any(|n| n.starts_with("aura.console.")));
        assert!(names.iter().any(|n| n.starts_with("aura.encoding.")));
        assert!(names.iter().any(|n| n.starts_with("aura.env.")));
        assert!(names.iter().any(|n| n.starts_with("aura.fs.")));
        assert!(names.iter().any(|n| n.starts_with("aura.io.")));
        assert!(names.iter().any(|n| n.starts_with("aura.iter.")));
        assert!(names.iter().any(|n| n.starts_with("aura.json.")));
        assert!(names.iter().any(|n| n.starts_with("aura.math.")));
        assert!(names.iter().any(|n| n.starts_with("aura.net.")));
        assert!(names.iter().any(|n| n.starts_with("aura.path.")));
        assert!(names.iter().any(|n| n.starts_with("aura.process.")));
        assert!(names.iter().any(|n| n.starts_with("aura.random.")));
        assert!(names.iter().any(|n| n.starts_with("aura.string.")));
        assert!(names.iter().any(|n| n.starts_with("aura.test.")));
        assert!(names.iter().any(|n| n.starts_with("aura.time.")));
        assert!(names.iter().any(|n| n.starts_with("aura.concurrent.")));
    }
}
