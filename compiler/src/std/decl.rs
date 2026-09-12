//! Phase 1 — std 函数名元数据层
//!
//! 单一真相源：编译期（checker / hir / symbol / mir）与运行期（NativeRegistry）
//! 共用同一份函数清单，消除「编译期看不见 / 运行期抢名」的分裂。
//!
//! 设计原则（按需免import）：
//! - **Prelude**（17 个）：免import，始终可用（println/abs/sqrt/...）
//! - **命名空间库**（320 个）：需 `import` 后才可用（aura.lang.std.Math.sin/...）
//!
//! 使用方式：
//! - `is_prelude(name)` — 判断是否为免import的 prelude 函数
//! - `is_builtin(name)` — 判断是否为任意内置函数（含命名空间库）
//! - `all_names()` — 返回全部内置名集合
//! - `prelu_names()` — 返回 prelude 名集合

use std::collections::HashSet;
use std::sync::OnceLock;

/// 免import的 prelude 函数名（全局内置）
pub const PRELUDE_NAMES: &[&str] = &[
    "println",
    "print",
    "puts",
    "abs",
    "sqrt",
    "pow",
    "toInt",
    "toFloat",
    "toStr",
    "toString",
    "clock",
    "strlen",
    "CString",
    "CStr",
    "ptrIsNull",
    "ptrToInt",
    "intToPtr",
    "makeCallback",
    "listOf",
    "mutableListOf",
    "arrayOf",
    "min",
    "max",
    // 类型查询与内省（prelu，免 import）
    "typeof",
    "isNull",
    "isNotNull",
    "isZero",
    "isPositive",
    "isNegative",
    "toBool",
    "sizeOf",
    "hash",
    "compare",
    "clone",
    "identity",
    // 测试断言函数（prelu，免 import）
    "assertTrue",
    "assertFalse",
    "assertEq",
    "assertNotEq",
    "assertNotNull",
    "assertNull",
    "assertContains",
    "assertNotContains",
    "assertGt",
    "assertGte",
    "assertLt",
    "assertLte",
    "assertApprox",
    "assertArrayEq",
    "assertMapEq",
    "pass",
    "fail",
    // Phase 4: Any 基类内置方法 + as 类型转换
    "equals",
    "hashCode",
    "typeOf",
    "aura_isOfType",
    "aura_cast",
    "aura_cast_safety",
    // ── 全名别名（免 import 也可用）──
    "aura.lang.std.println",
    "aura.lang.std.print",
    "aura.lang.std.puts",
    "aura.lang.std.abs",
    "aura.lang.std.sqrt",
    "aura.lang.std.pow",
    "aura.lang.std.toInt",
    "aura.lang.std.toFloat",
    "aura.lang.std.toStr",
    "aura.lang.std.toString",
    "aura.lang.std.clock",
    "aura.lang.std.strlen",
    "aura.lang.std.CString",
    "aura.lang.std.CStr",
    "aura.lang.std.ptrIsNull",
    "aura.lang.std.ptrToInt",
    "aura.lang.std.intToPtr",
    "aura.lang.std.makeCallback",
    "aura.lang.std.listOf",
    "aura.lang.std.typeof",
    "aura.lang.std.isNull",
    "aura.lang.std.isNotNull",
    "aura.lang.std.isZero",
    "aura.lang.std.isPositive",
    "aura.lang.std.isNegative",
    "aura.lang.std.toBool",
    "aura.lang.std.sizeOf",
    "aura.lang.std.hash",
    "aura.lang.std.compare",
    "aura.lang.std.clone",
    "aura.lang.std.identity",
    "aura.lang.std.assertTrue",
    "aura.lang.std.assertFalse",
    "aura.lang.std.assertEq",
    "aura.lang.std.assertNotEq",
    "aura.lang.std.assertNotNull",
    "aura.lang.std.assertNull",
    "aura.lang.std.assertContains",
    "aura.lang.std.assertNotContains",
    "aura.lang.std.assertGt",
    "aura.lang.std.assertGte",
    "aura.lang.std.assertLt",
    "aura.lang.std.assertLte",
    "aura.lang.std.assertApprox",
    "aura.lang.std.assertArrayEq",
    "aura.lang.std.assertMapEq",
    "aura.lang.std.pass",
    "aura.lang.std.fail",
    "aura.lang.std.equals",
    "aura.lang.std.hashCode",
    "aura.lang.std.typeOf",
    "aura.lang.std.aura_isOfType",
    "aura.lang.std.aura_cast",
    "aura.lang.std.aura_cast_safety",
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
/// 例如：`module_functions("aura.lang.std.Math")` → `["sin", "cos", "tan", ...]`
/// 用于 `import aura.lang.std.Math.*` 展开到符号表。
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
/// 例如：`module_functions_full("aura.lang.std.Math")` → `["aura.lang.std.Math.sin", "aura.lang.std.Math.cos", ...]`
pub fn module_functions_full(module_path: &str) -> Vec<&'static str> {
    let prefix = format!("{}.", module_path);
    all_names().iter().filter(|name| name.starts_with(&prefix)).copied().collect()
}

fn build_all_names() -> HashSet<&'static str> {
    let mut s = HashSet::new();

    // ── 顶层内置（native.rs 直接注册，无命名空间前缀）──
    // println / print / puts / abs / sqrt / pow / toInt / toFloat / toStr /
    // toString / clock / strlen / CString / CStr / ptrIsNull / ptrToInt /
    // intToPtr / makeCallback
    for n in [
        "println",
        "print",
        "puts",
        "abs",
        "sqrt",
        "pow",
        "toInt",
        "toFloat",
        "toStr",
        "toString",
        "clock",
        "strlen",
        "CString",
        "CStr",
        "ptrIsNull",
        "ptrToInt",
        "intToPtr",
        "makeCallback",
    ] {
        s.insert(n);
    }

    // ── 顶层内置的全名别名（aura.lang.std.<fn>，用于 import 展开和点分全名调用）──
    for n in [
        "aura.lang.std.println",
        "aura.lang.std.print",
        "aura.lang.std.puts",
        "aura.lang.std.abs",
        "aura.lang.std.sqrt",
        "aura.lang.std.pow",
        "aura.lang.std.toInt",
        "aura.lang.std.toFloat",
        "aura.lang.std.toStr",
        "aura.lang.std.toString",
        "aura.lang.std.clock",
        "aura.lang.std.strlen",
        "aura.lang.std.CString",
        "aura.lang.std.CStr",
        "aura.lang.std.ptrIsNull",
        "aura.lang.std.ptrToInt",
        "aura.lang.std.intToPtr",
        "aura.lang.std.makeCallback",
        "aura.lang.std.listOf",
        "aura.lang.std.typeof",
        "aura.lang.std.isNull",
        "aura.lang.std.isNotNull",
        "aura.lang.std.isZero",
        "aura.lang.std.isPositive",
        "aura.lang.std.isNegative",
        "aura.lang.std.toBool",
        "aura.lang.std.sizeOf",
        "aura.lang.std.hash",
        "aura.lang.std.compare",
        "aura.lang.std.clone",
        "aura.lang.std.identity",
        "aura.lang.std.assertTrue",
        "aura.lang.std.assertFalse",
        "aura.lang.std.assertEq",
        "aura.lang.std.assertNotEq",
        "aura.lang.std.assertNotNull",
        "aura.lang.std.assertNull",
        "aura.lang.std.assertContains",
        "aura.lang.std.assertNotContains",
        "aura.lang.std.assertGt",
        "aura.lang.std.assertGte",
        "aura.lang.std.assertLt",
        "aura.lang.std.assertLte",
        "aura.lang.std.assertApprox",
        "aura.lang.std.assertArrayEq",
        "aura.lang.std.assertMapEq",
        "aura.lang.std.pass",
        "aura.lang.std.fail",
        "aura.lang.std.equals",
        "aura.lang.std.hashCode",
        "aura.lang.std.typeOf",
        "aura.lang.std.aura_isOfType",
        "aura.lang.std.aura_cast",
        "aura.lang.std.aura_cast_safety",
    ] {
        s.insert(n);
    }

    // ── aura.lang.std.{Coroutine,Actor,Channel}.* — 协程 / Actor / 通道（native.rs 注册）──
    for n in [
        "aura.lang.std.Coroutine.spawn",
        "aura.lang.std.Coroutine.ask",
        "aura.lang.std.Actor.send",
        "aura.lang.std.Actor.reply",
        "aura.lang.std.Actor.spawnActor",
        "aura.lang.std.Actor.supervise",
        "aura.lang.std.Actor.actorAlive",
        "aura.lang.std.Actor.spawnActorProcess",
        "aura.lang.std.Actor.sendProcessActor",
        "aura.lang.std.Actor.recvProcessActor",
        "aura.lang.std.Actor.processActorAlive",
        "aura.lang.std.Actor.killProcessActor",
        "aura.lang.std.Channel.newChannel",
        "aura.lang.std.Channel.channelSend",
        "aura.lang.std.Channel.channelRecv",
        "aura.lang.std.Channel.channelTryRecv",
        "aura.lang.std.Channel.select",
        "aura.lang.std.Channel.selectTimeout",
        "aura.lang.std.Channel.newTcpChannel",
        "aura.lang.std.Channel.tcpChannelSend",
    ] {
        s.insert(n);
    }

    // ── aura.lang.std.Ascii.* — 字符工具（std_ascii.rs）──
    for n in [
        "aura.lang.std.Ascii.isAlpha",
        "aura.lang.std.Ascii.isDigit",
        "aura.lang.std.Ascii.isAlphaNumeric",
        "aura.lang.std.Ascii.isWhitespace",
        "aura.lang.std.Ascii.isUpper",
        "aura.lang.std.Ascii.isLower",
        "aura.lang.std.Ascii.toUpper",
        "aura.lang.std.Ascii.toLower",
        "aura.lang.std.Ascii.codeAt",
        "aura.lang.std.Ascii.charAt",
        "aura.lang.std.Ascii.fromCode",
        "aura.lang.std.Ascii.codePointAt",
    ] {
        s.insert(n);
    }

    // ── aura.lang.std.Assert.* — 通用断言（std_assert.rs）──
    for n in [
        "aura.lang.std.Assert.assert",
        "aura.lang.std.Assert.assertTrue",
        "aura.lang.std.Assert.assertFalse",
        "aura.lang.std.Assert.assertEq",
        "aura.lang.std.Assert.assertNotEq",
        "aura.lang.std.Assert.assertNotNull",
        "aura.lang.std.Assert.assertNull",
        "aura.lang.std.Assert.debugAssert",
    ] {
        s.insert(n);
    }

    // ── aura.lang.std.Builtin.* — 编译期内置（std_builtin.rs）──
    for n in [
        "aura.lang.std.Builtin.typeof",
        "aura.lang.std.Builtin.typeOf",
        "aura.lang.std.Builtin.isNull",
        "aura.lang.std.Builtin.isNotNull",
        "aura.lang.std.Builtin.isZero",
        "aura.lang.std.Builtin.isPositive",
        "aura.lang.std.Builtin.isNegative",
        "aura.lang.std.Builtin.toString",
        "aura.lang.std.Builtin.toInt",
        "aura.lang.std.Builtin.toFloat",
        "aura.lang.std.Builtin.toBool",
        "aura.lang.std.Builtin.sizeOf",
        "aura.lang.std.Builtin.hash",
        "aura.lang.std.Builtin.compare",
        "aura.lang.std.Builtin.clone",
        "aura.lang.std.Builtin.identity",
    ] {
        s.insert(n);
    }

    // ── aura.lang.std.Collections.* — 集合辅助（std_collections.rs）──
    for n in [
        "aura.lang.std.Collections.listOf",
        "aura.lang.std.Collections.mutableListOf",
        "aura.lang.std.Collections.emptyList",
        "aura.lang.std.Collections.arrayOf",
        "aura.lang.std.Collections.listContains",
        "aura.lang.std.Collections.listIndexOf",
        "aura.lang.std.Collections.listRemove",
        "aura.lang.std.Collections.listReverse",
        "aura.lang.std.Collections.listSort",
        "aura.lang.std.Collections.listGet",
        "aura.lang.std.Collections.listSet",
        "aura.lang.std.Collections.listInsert",
        "aura.lang.std.Collections.listSubList",
        "aura.lang.std.Collections.listAppend",
        "aura.lang.std.Collections.listSize",
        "aura.lang.std.Collections.pairOf",
        "aura.lang.std.Collections.mapOf",
        "aura.lang.std.Collections.mutableMapOf",
        "aura.lang.std.Collections.emptyMap",
        "aura.lang.std.Collections.mapContains",
        "aura.lang.std.Collections.mapContainsKey",
        "aura.lang.std.Collections.mapContainsValue",
        "aura.lang.std.Collections.mapRemove",
        "aura.lang.std.Collections.mapKeys",
        "aura.lang.std.Collections.mapValues",
        "aura.lang.std.Collections.setOf",
        "aura.lang.std.Collections.mutableSetOf",
        "aura.lang.std.Collections.emptySet",
        // 特化集合构造（分层实现）
        "aura.lang.std.Collections.arrayListOf",
        "aura.lang.std.Collections.arrayListSize",
        "aura.lang.std.Collections.linkedListOf",
        "aura.lang.std.Collections.linkedAddFirst",
        "aura.lang.std.Collections.linkedAddLast",
        "aura.lang.std.Collections.linkedRemoveFirst",
        "aura.lang.std.Collections.linkedRemoveLast",
        "aura.lang.std.Collections.hashSetOf",
        "aura.lang.std.Collections.hashSetContains",
        "aura.lang.std.Collections.hashSetAdd",
        "aura.lang.std.Collections.hashSetRemove",
        "aura.lang.std.Collections.hashMapOf",
        "aura.lang.std.Collections.hashMapGet",
        "aura.lang.std.Collections.hashMapPut",
        "aura.lang.std.Collections.hashMapRemove",
        "aura.lang.std.Collections.linkedHashMapOf",
        "aura.lang.std.Collections.linkedHashMapKeys",
        "aura.lang.std.Collections.linkedHashMapFirstKey",
        "aura.lang.std.Collections.linkedHashMapLastKey",
    ] {
        s.insert(n);
    }

    // ── aura.lang.std.Console.* — 终端控制（std_console.rs）──
    for n in [
        "aura.lang.std.Console.clear",
        "aura.lang.std.Console.cursorUp",
        "aura.lang.std.Console.cursorDown",
        "aura.lang.std.Console.cursorLeft",
        "aura.lang.std.Console.cursorRight",
        "aura.lang.std.Console.cursorShow",
        "aura.lang.std.Console.cursorHide",
        "aura.lang.std.Console.reset",
        "aura.lang.std.Console.red",
        "aura.lang.std.Console.green",
        "aura.lang.std.Console.yellow",
        "aura.lang.std.Console.blue",
        "aura.lang.std.Console.magenta",
        "aura.lang.std.Console.cyan",
        "aura.lang.std.Console.white",
        "aura.lang.std.Console.bold",
        "aura.lang.std.Console.italic",
        "aura.lang.std.Console.underline",
        "aura.lang.std.Console.dim",
        "aura.lang.std.Console.inverse",
        "aura.lang.std.Console.size",
        "aura.lang.std.Console.width",
        "aura.lang.std.Console.height",
    ] {
        s.insert(n);
    }

    // ── aura.lang.std.Encoding.* — 编码/解码（Encoding.aura，纯逻辑函数）──
    for n in [
        "aura.lang.std.Encoding.base64Encode",
        "aura.lang.std.Encoding.base64Decode",
        "aura.lang.std.Encoding.hexEncode",
        "aura.lang.std.Encoding.hexDecode",
        "aura.lang.std.Encoding.urlEncode",
        "aura.lang.std.Encoding.urlDecode",
        "aura.lang.std.Encoding.byteToHex",
        "aura.lang.std.Encoding.hexToByte",
    ] {
        s.insert(n);
    }

    // ── aura.lang.std.Env.* — 环境变量（std_env.rs）──
    for n in [
        "aura.lang.std.Env.get",
        "aura.lang.std.Env.set",
        "aura.lang.std.Env.remove",
        "aura.lang.std.Env.has",
        "aura.lang.std.Env.keys",
        "aura.lang.std.Env.values",
        "aura.lang.std.Env.all",
        "aura.lang.std.Env.home",
        "aura.lang.std.Env.tmp",
        "aura.lang.std.Env.pwd",
        "aura.lang.std.Env.platform",
        "aura.lang.std.Env.os",
        "aura.lang.std.Env.arch",
    ] {
        s.insert(n);
    }

    // ── aura.lang.std.FileSystem.* — 文件系统（std_fs.rs）──
    for n in [
        "aura.lang.std.FileSystem.exists",
        "aura.lang.std.FileSystem.isFile",
        "aura.lang.std.FileSystem.isDirectory",
        "aura.lang.std.FileSystem.readText",
        "aura.lang.std.FileSystem.writeText",
        "aura.lang.std.FileSystem.readBytes",
        "aura.lang.std.FileSystem.writeBytes",
        "aura.lang.std.FileSystem.delete",
        "aura.lang.std.FileSystem.mkdir",
        "aura.lang.std.FileSystem.mkdirP",
        "aura.lang.std.FileSystem.rename",
        "aura.lang.std.FileSystem.copy",
        "aura.lang.std.FileSystem.listDir",
        "aura.lang.std.FileSystem.listFiles",
        "aura.lang.std.FileSystem.fileSize",
        "aura.lang.std.FileSystem.lastModified",
        "aura.lang.std.FileSystem.absolutePath",
        "aura.lang.std.FileSystem.homeDir",
        "aura.lang.std.FileSystem.tempDir",
        "aura.lang.std.FileSystem.currentDir",
        "aura.lang.std.FileSystem.walk",
    ] {
        s.insert(n);
    }

    // ── aura.lang.std.IO.* — 标准输入输出（std_io.rs）──
    for n in [
        "aura.lang.std.IO.println",
        "aura.lang.std.IO.print",
        "aura.lang.std.IO.readLine",
        "aura.lang.std.IO.readAll",
        "aura.lang.std.IO.flush",
        "aura.lang.std.IO.fileRead",
        "aura.lang.std.IO.fileWrite",
        "aura.lang.std.IO.fileExists",
        "aura.lang.std.IO.writeFile",
        "aura.lang.std.IO.readFile",
    ] {
        s.insert(n);
    }

    // ── aura.lang.std.Iter.* — 迭代器/函数式（std_iter.rs）──
    for n in [
        "aura.lang.std.Iter.sum",
        "aura.lang.std.Iter.avg",
        "aura.lang.std.Iter.min",
        "aura.lang.std.Iter.max",
        "aura.lang.std.Iter.product",
        "aura.lang.std.Iter.contains",
        "aura.lang.std.Iter.indexOf",
        "aura.lang.std.Iter.count",
        "aura.lang.std.Iter.every",
        "aura.lang.std.Iter.some",
        "aura.lang.std.Iter.flatMap",
        "aura.lang.std.Iter.zip",
        "aura.lang.std.Iter.unzip",
        "aura.lang.std.Iter.enumerate",
        "aura.lang.std.Iter.chain",
        "aura.lang.std.Iter.take",
        "aura.lang.std.Iter.skip",
        "aura.lang.std.Iter.dropWhile",
        "aura.lang.std.Iter.takeWhile",
        "aura.lang.std.Iter.distinct",
        "aura.lang.std.Iter.groupBy",
        "aura.lang.std.Iter.partition",
        "aura.lang.std.Iter.fold",
        "aura.lang.std.Iter.scan",
        "aura.lang.std.Iter.toMap",
        "aura.lang.std.Iter.toList",
        "aura.lang.std.Iter.range",
        "aura.lang.std.Iter.rangeTo",
        "aura.lang.std.Iter.rangeUntil",
        "aura.lang.std.Iter.repeatN",
    ] {
        s.insert(n);
    }

    // ── aura.lang.std.Json.* — JSON 解析与序列化（std_json.rs）──
    for n in [
        "aura.lang.std.Json.parse",
        "aura.lang.std.Json.stringify",
        "aura.lang.std.Json.isValid",
        "aura.lang.std.Json.get",
        "aura.lang.std.Json.set",
        "aura.lang.std.Json.keys",
        "aura.lang.std.Json.values",
        "aura.lang.std.Json.length",
        "aura.lang.std.Json.contains",
        "aura.lang.std.Json.remove",
    ] {
        s.insert(n);
    }

    // ── aura.lang.std.Math.* — 数学函数与常量（std_math.rs + Math.aura）──
    // 纯逻辑函数（abs/min/max/sign/clamp）已上移到 Math.aura，但保留声明供编译器识别
    for n in [
        "aura.lang.std.Math.abs",
        "aura.lang.std.Math.min",
        "aura.lang.std.Math.max",
        "aura.lang.std.Math.ceil",
        "aura.lang.std.Math.floor",
        "aura.lang.std.Math.round",
        "aura.lang.std.Math.trunc",
        "aura.lang.std.Math.sqrt",
        "aura.lang.std.Math.cbrt",
        "aura.lang.std.Math.pow",
        "aura.lang.std.Math.exp",
        "aura.lang.std.Math.log",
        "aura.lang.std.Math.log2",
        "aura.lang.std.Math.log10",
        "aura.lang.std.Math.sin",
        "aura.lang.std.Math.cos",
        "aura.lang.std.Math.tan",
        "aura.lang.std.Math.asin",
        "aura.lang.std.Math.acos",
        "aura.lang.std.Math.atan",
        "aura.lang.std.Math.atan2",
        "aura.lang.std.Math.PI",
        "aura.lang.std.Math.E",
        "aura.lang.std.Math.INT_MAX",
        "aura.lang.std.Math.INT_MIN",
        "aura.lang.std.Math.FLOAT_MAX",
        "aura.lang.std.Math.sign",
        "aura.lang.std.Math.clamp",
    ] {
        s.insert(n);
    }

    // ── aura.lang.std.Network.* — 网络 Socket（std_net.rs）──
    for n in [
        "aura.lang.std.Network.tcpConnect",
        "aura.lang.std.Network.tcpListen",
        "aura.lang.std.Network.tcpSend",
        "aura.lang.std.Network.tcpRecv",
        "aura.lang.std.Network.tcpClose",
        "aura.lang.std.Network.udpSend",
        "aura.lang.std.Network.udpRecv",
        "aura.lang.std.Network.udpClose",
        "aura.lang.std.Network.isHostReachable",
        "aura.lang.std.Network.getHostname",
        "aura.lang.std.Network.getLocalIp",
    ] {
        s.insert(n);
    }

    // ── aura.lang.std.Path.* — 路径操作（std_path.rs + Path.aura）──
    // 纯逻辑函数（join/dirname/basename/extname/normalize/isAbsolute/isRelative/split/fromUnix/fromWindows）已上移到 Path.aura，但保留声明
    for n in [
        "aura.lang.std.Path.join",
        "aura.lang.std.Path.dirname",
        "aura.lang.std.Path.basename",
        "aura.lang.std.Path.extname",
        "aura.lang.std.Path.relative",
        "aura.lang.std.Path.resolve",
        "aura.lang.std.Path.normalize",
        "aura.lang.std.Path.isAbsolute",
        "aura.lang.std.Path.isRelative",
        "aura.lang.std.Path.split",
        "aura.lang.std.Path.separators",
        "aura.lang.std.Path.fromUnix",
        "aura.lang.std.Path.fromWindows",
    ] {
        s.insert(n);
    }

    // ── aura.lang.std.Process.* — 进程管理（std_process.rs）──
    for n in [
        "aura.lang.std.Process.exit",
        "aura.lang.std.Process.exitCode",
        "aura.lang.std.Process.args",
        "aura.lang.std.Process.arg",
        "aura.lang.std.Process.argCount",
        "aura.lang.std.Process.pid",
        "aura.lang.std.Process.spawn",
        "aura.lang.std.Process.run",
        "aura.lang.std.Process.kill",
        "aura.lang.std.Process.wait",
        "aura.lang.std.Process.exitProcess",
    ] {
        s.insert(n);
    }

    // ── aura.lang.std.Random.* — 随机数（std_random.rs）──
    for n in [
        "aura.lang.std.Random.nextInt",
        "aura.lang.std.Random.nextLong",
        "aura.lang.std.Random.nextFloat",
        "aura.lang.std.Random.nextDouble",
        "aura.lang.std.Random.nextBool",
        "aura.lang.std.Random.nextIntRange",
        "aura.lang.std.Random.nextFloatRange",
        "aura.lang.std.Random.choice",
        "aura.lang.std.Random.shuffle",
        "aura.lang.std.Random.seed",
        "aura.lang.std.Random.random",
    ] {
        s.insert(n);
    }

    // ── aura.lang.std.String.* — 字符串操作（std_string.rs + String.aura）──
    // 纯逻辑函数已上移到 String.aura，但保留声明供编译器识别
    for n in [
        "aura.lang.std.String.contains",
        "aura.lang.std.String.startsWith",
        "aura.lang.std.String.endsWith",
        "aura.lang.std.String.split",
        "aura.lang.std.String.join",
        "aura.lang.std.String.replace",
        "aura.lang.std.String.replaceAll",
        "aura.lang.std.String.trim",
        "aura.lang.std.String.trimStart",
        "aura.lang.std.String.trimEnd",
        "aura.lang.std.String.substring",
        "aura.lang.std.String.substringBefore",
        "aura.lang.std.String.substringAfter",
        "aura.lang.std.String.toLowerCase",
        "aura.lang.std.String.toUpperCase",
        "aura.lang.std.String.length",
        "aura.lang.std.String.isEmpty",
        "aura.lang.std.String.format",
        "aura.lang.std.String.repeat",
        "aura.lang.std.String.indexOf",
        "aura.lang.std.String.lastIndexOf",
        "aura.lang.std.String.padStart",
        "aura.lang.std.String.padEnd",
        "aura.lang.std.String.escape",
        "aura.lang.std.String.unescape",
        "aura.lang.std.String.splitLines",
        "aura.lang.std.String.joinLines",
        "aura.lang.std.String.countChar",
        "aura.lang.std.String.first",
        "aura.lang.std.String.last",
        "aura.lang.std.String.isBlank",
        "aura.lang.std.String.matches",
        "aura.lang.std.String.containsAny",
        "aura.lang.std.String.containsAll",
    ] {
        s.insert(n);
    }

    // ── aura.lang.std.Test.* — 测试断言（std_test.rs）──
    for n in [
        "aura.lang.std.Test.assertTrue",
        "aura.lang.std.Test.assertFalse",
        "aura.lang.std.Test.assertEq",
        "aura.lang.std.Test.assertNotEq",
        "aura.lang.std.Test.assertNotNull",
        "aura.lang.std.Test.assertNull",
        "aura.lang.std.Test.assertContains",
        "aura.lang.std.Test.assertNotContains",
        "aura.lang.std.Test.assertThrows",
        "aura.lang.std.Test.assertGt",
        "aura.lang.std.Test.assertGte",
        "aura.lang.std.Test.assertLt",
        "aura.lang.std.Test.assertLte",
        "aura.lang.std.Test.assertApprox",
        "aura.lang.std.Test.assertArrayEq",
        "aura.lang.std.Test.assertMapEq",
        "aura.lang.std.Test.pass",
        "aura.lang.std.Test.fail",
    ] {
        s.insert(n);
    }

    // ── aura.lang.std.Time.* — 时间/日期（std_time.rs）──
    // ── aura.lang.std.Time.* — 时间模块（std_time.rs + Time.aura）──
    // 纯逻辑函数（duration/diff）已上移到 Time.aura，但保留声明供编译器识别
    for n in [
        "aura.lang.std.Time.now",
        "aura.lang.std.Time.epoch",
        "aura.lang.std.Time.currentTime",
        "aura.lang.std.Time.sleep",
        "aura.lang.std.Time.duration",
        "aura.lang.std.Time.toDateString",
        "aura.lang.std.Time.toTimeString",
        "aura.lang.std.Time.formatDate",
        "aura.lang.std.Time.diff",
        "aura.lang.std.Time.parseDate",
    ] {
        s.insert(n);
    }

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_names_count() {
        let names = all_names();
        assert!(
            names.len() >= 300,
            "expected at least 300 names, got {}",
            names.len()
        );
    }

    #[test]
    fn test_is_builtin_basic() {
        assert!(is_builtin("println"));
        assert!(is_builtin("aura.lang.std.Math.sin"));
        assert!(is_builtin("aura.lang.std.Coroutine.spawn"));
        assert!(is_builtin("aura.lang.std.Actor.send"));
        assert!(is_builtin("aura.lang.std.Channel.newChannel"));
        assert!(!is_builtin("myFunction"));
        assert!(!is_builtin(""));
    }

    #[test]
    fn test_all_modules_represented() {
        let names = all_names();
        // 每个命名空间至少有 1 个函数
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Ascii.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Assert.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Builtin.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Collections.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Console.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Encoding.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Env.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.FileSystem.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.IO.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Iter.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Json.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Math.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Network.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Path.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Process.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Random.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.String.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Test.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Time.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Coroutine.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Actor.")));
        assert!(names.iter().any(|n| n.starts_with("aura.lang.std.Channel.")));
    }
}
