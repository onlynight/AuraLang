//! P9.11 — API 文档生成器
//!
//! 对应 开发规划与实现进度.md P9 任务 9.11「文档注释与 API 文档生成」。
//!
//! 由于标准库函数是 Rust 原生函数（无 AST 元数据），文档信息以
//! 声明式 `StdDoc` 结构体维护。`DocRegistry` 收集全部文档并通过
//! `render_markdown` 输出结构化 Markdown。
//!
//! CLI 调用：`aura doc [--output <dir>] [--module <name>]`

use crate::vm::native::NativeRegistry;

// ─────────────────────────────────────────────────────────────────────────────
// 数据结构
// ─────────────────────────────────────────────────────────────────────────────

/// 单个标准库函数的文档条目
#[derive(Debug, Clone)]
pub struct StdDoc {
    /// 模块名（如 "math"、"string"、"collections"）
    pub module: &'static str,
    /// 函数全名（如 "aura.math.abs"）
    pub name: &'static str,
    /// 简短描述
    pub summary: &'static str,
    /// 参数列表（名称, 类型, 描述）
    pub params: &'static [(&'static str, &'static str, &'static str)],
    /// 返回类型
    pub returns: &'static str,
    /// 返回描述
    pub returns_desc: &'static str,
    /// 示例代码（可空）
    pub example: Option<&'static str>,
    /// 关联的 Rust 函数名（用于注释来源）
    pub rust_fn: &'static str,
}

/// 文档注册表
#[derive(Debug, Default)]
pub struct DocRegistry {
    docs: Vec<StdDoc>,
}

impl DocRegistry {
    /// 创建空的文档注册表
    pub fn new() -> Self {
        Self { docs: Vec::new() }
    }

    /// 添加一个文档条目
    pub fn add(&mut self, doc: StdDoc) {
        self.docs.push(doc);
    }

    /// 填充所有标准库文档
    pub fn load_all(mut self) -> Self {
        // ── std.io ──
        self.docs.push(StdDoc {
            module: "io",
            name: "aura.io.println",
            summary: "打印一行文本到标准输出，末尾自动追加换行符。",
            params: &[("msg", "String", "要输出的文本（可多参数拼接）")],
            returns: "Unit",
            returns_desc: "无返回值",
            example: Some(r#"aura.io.println("Hello, Aura!")"#),
            rust_fn: "std_io::nat_println",
        });
        self.docs.push(StdDoc {
            module: "io",
            name: "aura.io.print",
            summary: "打印文本到标准输出，不追加换行符。",
            params: &[("msg", "String", "要输出的文本")],
            returns: "Unit",
            returns_desc: "无返回值",
            example: Some(r#"aura.io.print("Hello ") // 不换行"#),
            rust_fn: "std_io::nat_print",
        });
        self.docs.push(StdDoc {
            module: "io",
            name: "aura.io.readLine",
            summary: "从标准输入读取一行文本。",
            params: &[],
            returns: "String",
            returns_desc: "读取到的文本（去除尾部换行），无输入时返回 null",
            example: Some(r#"val line = io.readLine()"#),
            rust_fn: "std_io::nat_readline",
        });
        self.docs.push(StdDoc {
            module: "io",
            name: "aura.io.fileRead",
            summary: "读取文件全部内容为字符串。",
            params: &[("path", "String", "文件路径")],
            returns: "String",
            returns_desc: "文件内容，读取失败返回错误信息",
            example: Some(r#"val content = io.fileRead("data.txt")"#),
            rust_fn: "std_io::nat_file_read",
        });
        self.docs.push(StdDoc {
            module: "io",
            name: "aura.io.fileWrite",
            summary: "将字符串写入文件（覆盖模式）。",
            params: &[
                ("path", "String", "目标文件路径"),
                ("content", "String", "要写入的内容"),
            ],
            returns: "Unit",
            returns_desc: "无返回值，失败时返回错误信息",
            example: Some(r#"aura.io.fileWrite("out.txt", "Hello")"#),
            rust_fn: "std_io::nat_file_write",
        });
        self.docs.push(StdDoc {
            module: "io",
            name: "aura.io.fileExists",
            summary: "检查文件或目录是否存在。",
            params: &[("path", "String", "路径")],
            returns: "Bool",
            returns_desc: "存在返回 true",
            example: None,
            rust_fn: "std_io::nat_file_exists",
        });

        // ── std.math ──
        self.docs.push(StdDoc {
            module: "math",
            name: "aura.math.abs",
            summary: "返回数值的绝对值。",
            params: &[("x", "Int / Float", "输入值")],
            returns: "Int / Float",
            returns_desc: "绝对值",
            example: Some(r#"aura.math.abs(-42) // → 42"#),
            rust_fn: "std_math::nat_abs",
        });
        self.docs.push(StdDoc {
            module: "math",
            name: "aura.math.min",
            summary: "返回两个整数中的较小值。",
            params: &[("a", "Int", "第一个数"), ("b", "Int", "第二个数")],
            returns: "Int",
            returns_desc: "最小值",
            example: Some(r#"aura.math.min(3, 5) // → 3"#),
            rust_fn: "std_math::nat_min",
        });
        self.docs.push(StdDoc {
            module: "math",
            name: "aura.math.max",
            summary: "返回两个整数中的较大值。",
            params: &[("a", "Int", "第一个数"), ("b", "Int", "第二个数")],
            returns: "Int",
            returns_desc: "最大值",
            example: Some(r#"aura.math.max(3, 5) // → 5"#),
            rust_fn: "std_math::nat_max",
        });
        self.docs.push(StdDoc {
            module: "math",
            name: "aura.math.sqrt",
            summary: "返回数的平方根。",
            params: &[("x", "Float", "输入值（≥0）")],
            returns: "Float",
            returns_desc: "平方根",
            example: Some(r#"aura.math.sqrt(16.0) // → 4.0"#),
            rust_fn: "std_math::nat_sqrt",
        });
        self.docs.push(StdDoc {
            module: "math",
            name: "aura.math.pow",
            summary: "返回 base^exp（幂运算）。",
            params: &[
                ("base", "Float", "底数"),
                ("exp", "Float", "指数"),
            ],
            returns: "Float",
            returns_desc: "幂运算结果",
            example: Some(r#"aura.math.pow(2.0, 10.0) // → 1024.0"#),
            rust_fn: "std_math::nat_pow",
        });
        self.docs.push(StdDoc {
            module: "math",
            name: "aura.math.PI",
            summary: "圆周率 π ≈ 3.141592653589793。",
            params: &[],
            returns: "Float",
            returns_desc: "π 的精确值",
            example: None,
            rust_fn: "std_math::nat_pi",
        });
        self.docs.push(StdDoc {
            module: "math",
            name: "aura.math.E",
            summary: "自然对数的底 e ≈ 2.718281828459045。",
            params: &[],
            returns: "Float",
            returns_desc: "e 的精确值",
            example: None,
            rust_fn: "std_math::nat_e",
        });
        self.docs.push(StdDoc {
            module: "math",
            name: "aura.math.sin",
            summary: "返回角度的正弦值（弧度）。",
            params: &[("angle", "Float", "弧度值")],
            returns: "Float",
            returns_desc: "sin(angle)",
            example: None,
            rust_fn: "std_math::nat_sin",
        });
        self.docs.push(StdDoc {
            module: "math",
            name: "aura.math.cos",
            summary: "返回角度的余弦值（弧度）。",
            params: &[("angle", "Float", "弧度值")],
            returns: "Float",
            returns_desc: "cos(angle)",
            example: None,
            rust_fn: "std_math::nat_cos",
        });
        self.docs.push(StdDoc {
            module: "math",
            name: "aura.math.log",
            summary: "返回自然对数（以 e 为底）。",
            params: &[("x", "Float", "输入值（>0）")],
            returns: "Float",
            returns_desc: "ln(x)",
            example: None,
            rust_fn: "std_math::nat_log",
        });
        self.docs.push(StdDoc {
            module: "math",
            name: "aura.math.ceil",
            summary: "向上取整。",
            params: &[("x", "Float", "输入值")],
            returns: "Float",
            returns_desc: "≥ x 的最小整数",
            example: Some(r#"aura.math.ceil(1.2) // → 2.0"#),
            rust_fn: "std_math::nat_ceil",
        });
        self.docs.push(StdDoc {
            module: "math",
            name: "aura.math.floor",
            summary: "向下取整。",
            params: &[("x", "Float", "输入值")],
            returns: "Float",
            returns_desc: "≤ x 的最大整数",
            example: Some(r#"aura.math.floor(1.8) // → 1.0"#),
            rust_fn: "std_math::nat_floor",
        });

        // ── std.string ──
        self.docs.push(StdDoc {
            module: "string",
            name: "aura.string.contains",
            summary: "判断字符串是否包含子串。",
            params: &[("text", "String", "源字符串"), ("substr", "String", "子串")],
            returns: "Bool",
            returns_desc: "包含返回 true",
            example: Some(r#"aura.string.contains("hello", "ell") // → true"#),
            rust_fn: "std_string::nat_contains",
        });
        self.docs.push(StdDoc {
            module: "string",
            name: "aura.string.startsWith",
            summary: "判断字符串是否以指定前缀开头。",
            params: &[("text", "String", "源字符串"), ("prefix", "String", "前缀")],
            returns: "Bool",
            returns_desc: "匹配返回 true",
            example: None,
            rust_fn: "std_string::nat_starts_with",
        });
        self.docs.push(StdDoc {
            module: "string",
            name: "aura.string.endsWith",
            summary: "判断字符串是否以指定后缀结尾。",
            params: &[("text", "String", "源字符串"), ("suffix", "String", "后缀")],
            returns: "Bool",
            returns_desc: "匹配返回 true",
            example: None,
            rust_fn: "std_string::nat_ends_with",
        });
        self.docs.push(StdDoc {
            module: "string",
            name: "aura.string.split",
            summary: "按分隔符拆分字符串，返回 List。",
            params: &[("text", "String", "源字符串"), ("sep", "String", "分隔符")],
            returns: "List<String>",
            returns_desc: "拆分后的子串列表",
            example: Some(r#"aura.string.split("a,b,c", ",") // → ["a", "b", "c"]"#),
            rust_fn: "std_string::nat_split",
        });
        self.docs.push(StdDoc {
            module: "string",
            name: "aura.string.join",
            summary: "将空格分隔的文本合并，用指定分隔符连接。",
            params: &[("text", "String", "源文本（空格分隔）"), ("sep", "String", "分隔符")],
            returns: "String",
            returns_desc: "合并后的字符串",
            example: None,
            rust_fn: "std_string::nat_join",
        });
        self.docs.push(StdDoc {
            module: "string",
            name: "aura.string.replace",
            summary: "替换第一个匹配的子串。",
            params: &[
                ("text", "String", "源字符串"),
                ("target", "String", "要替换的子串"),
                ("replacement", "String", "替换文本"),
            ],
            returns: "String",
            returns_desc: "替换后的字符串",
            example: Some(r#"aura.string.replace("hello world", "world", "Aura")"#),
            rust_fn: "std_string::nat_replace",
        });
        self.docs.push(StdDoc {
            module: "string",
            name: "aura.string.toUpperCase",
            summary: "将所有字母转换为大写。",
            params: &[("text", "String", "源字符串")],
            returns: "String",
            returns_desc: "大写字符串",
            example: Some(r#"aura.string.toUpperCase("hello") // → "HELLO""#),
            rust_fn: "std_string::nat_to_upper",
        });
        self.docs.push(StdDoc {
            module: "string",
            name: "aura.string.toLowerCase",
            summary: "将所有字母转换为小写。",
            params: &[("text", "String", "源字符串")],
            returns: "String",
            returns_desc: "小写字符串",
            example: Some(r#"aura.string.toLowerCase("HELLO") // → "hello""#),
            rust_fn: "std_string::nat_to_lower",
        });
        self.docs.push(StdDoc {
            module: "string",
            name: "aura.string.length",
            summary: "返回字符串的字符数。",
            params: &[("text", "String", "源字符串")],
            returns: "Int",
            returns_desc: "字符数量",
            example: None,
            rust_fn: "std_string::nat_length",
        });
        self.docs.push(StdDoc {
            module: "string",
            name: "aura.string.trim",
            summary: "去除首尾空白字符。",
            params: &[("text", "String", "源字符串")],
            returns: "String",
            returns_desc: "修剪后的字符串",
            example: Some(r#"aura.string.trim("  hello  ") // → "hello""#),
            rust_fn: "std_string::nat_trim",
        });
        self.docs.push(StdDoc {
            module: "string",
            name: "aura.string.format",
            summary: "将 `{0}`, `{1}` 等占位符替换为参数值。",
            params: &[
                ("template", "String", "模板字符串"),
                ("...", "Value", "任意数量参数"),
            ],
            returns: "String",
            returns_desc: "格式化后的字符串",
            example: Some(r#"aura.string.format("Hello {0}", "Aura") // → "Hello Aura""#),
            rust_fn: "std_string::nat_format",
        });
        self.docs.push(StdDoc {
            module: "string",
            name: "aura.string.matches",
            summary: "正则表达式匹配。",
            params: &[
                ("text", "String", "源字符串"),
                ("regex", "String", "正则表达式"),
            ],
            returns: "Bool",
            returns_desc: "匹配返回 true",
            example: None,
            rust_fn: "std_string::nat_matches",
        });

        // ── std.collections ──
        self.docs.push(StdDoc {
            module: "collections",
            name: "aura.collections.listOf",
            summary: "构造不可变列表。",
            params: &[("...", "Value", "任意数量元素")],
            returns: "List",
            returns_desc: "包含所有参数的列表",
            example: Some(r#"val nums = collections.listOf(1, 2, 3)"#),
            rust_fn: "std_collections::nat_list_of",
        });
        self.docs.push(StdDoc {
            module: "collections",
            name: "aura.collections.mapOf",
            summary: "构造键值映射（参数成对出现）。",
            params: &[("...", "Value", "键值对（交替出现）")],
            returns: "Map",
            returns_desc: "包含所有键值对的映射",
            example: Some(r#"val m = collections.mapOf("name", "Aura", "v", 1)"#),
            rust_fn: "std_collections::nat_map_of",
        });
        self.docs.push(StdDoc {
            module: "collections",
            name: "aura.collections.setOf",
            summary: "构造集合（自动去重）。",
            params: &[("...", "Value", "任意数量元素")],
            returns: "List",
            returns_desc: "去重后的元素列表",
            example: Some(r#"val s = collections.setOf(1, 2, 1, 3) // → [1, 2, 3]"#),
            rust_fn: "std_collections::nat_set_of",
        });
        self.docs.push(StdDoc {
            module: "collections",
            name: "aura.collections.emptyList",
            summary: "构造空列表。",
            params: &[],
            returns: "List",
            returns_desc: "空列表",
            example: None,
            rust_fn: "std_collections::nat_empty_list",
        });
        self.docs.push(StdDoc {
            module: "collections",
            name: "aura.collections.listContains",
            summary: "检查列表是否包含指定元素。",
            params: &[("list", "List", "源列表"), ("item", "Value", "要查找的元素")],
            returns: "Bool",
            returns_desc: "包含返回 true",
            example: None,
            rust_fn: "std_collections::nat_list_contains",
        });

        // ── std.fs ──
        self.docs.push(StdDoc {
            module: "fs",
            name: "aura.fs.exists",
            summary: "检查路径是否存在。",
            params: &[("path", "String", "文件/目录路径")],
            returns: "Bool",
            returns_desc: "存在返回 true",
            example: None,
            rust_fn: "std_fs::nat_exists",
        });
        self.docs.push(StdDoc {
            module: "fs",
            name: "aura.fs.readText",
            summary: "读取文件文本内容。",
            params: &[("path", "String", "文件路径")],
            returns: "String",
            returns_desc: "文件内容，失败返回错误信息",
            example: None,
            rust_fn: "std_fs::nat_read_text",
        });
        self.docs.push(StdDoc {
            module: "fs",
            name: "aura.fs.writeText",
            summary: "将文本写入文件。",
            params: &[
                ("path", "String", "文件路径"),
                ("content", "String", "文本内容"),
            ],
            returns: "Unit",
            returns_desc: "无返回值，失败返回错误信息",
            example: None,
            rust_fn: "std_fs::nat_write_text",
        });
        self.docs.push(StdDoc {
            module: "fs",
            name: "aura.fs.mkdir",
            summary: "创建目录。",
            params: &[("path", "String", "目录路径")],
            returns: "Unit",
            returns_desc: "无返回值",
            example: None,
            rust_fn: "std_fs::nat_mkdir",
        });
        self.docs.push(StdDoc {
            module: "fs",
            name: "aura.fs.mkdirP",
            summary: "递归创建目录（包含所有父目录）。",
            params: &[("path", "String", "目录路径")],
            returns: "Unit",
            returns_desc: "无返回值",
            example: None,
            rust_fn: "std_fs::nat_mkdir_p",
        });
        self.docs.push(StdDoc {
            module: "fs",
            name: "aura.fs.listDir",
            summary: "列出目录中的所有条目名称。",
            params: &[("path", "String", "目录路径")],
            returns: "List<String>",
            returns_desc: "条目名称列表",
            example: None,
            rust_fn: "std_fs::nat_list_dir",
        });
        self.docs.push(StdDoc {
            module: "fs",
            name: "aura.fs.fileSize",
            summary: "返回文件大小（字节）。",
            params: &[("path", "String", "文件路径")],
            returns: "Int",
            returns_desc: "字节数，失败返回 -1",
            example: None,
            rust_fn: "std_fs::nat_file_size",
        });

        // ── std.json ──
        self.docs.push(StdDoc {
            module: "json",
            name: "aura.json.parse",
            summary: "将 JSON 字符串解析为 Aura 值树。",
            params: &[("text", "String", "JSON 字符串")],
            returns: "Value",
            returns_desc: "解析后的值（Map/List/Int/Float/Bool/String/Null），失败返回错误信息",
            example: Some(r#"aura.json.parse("{\"name\":\"Aura\"}")"#),
            rust_fn: "std_json::nat_parse",
        });
        self.docs.push(StdDoc {
            module: "json",
            name: "aura.json.stringify",
            summary: "将 Aura 值序列化为 JSON 字符串。",
            params: &[
                ("value", "Value", "要序列化的值"),
                ("pretty", "Bool", "是否美化输出（可选）"),
            ],
            returns: "String",
            returns_desc: "JSON 字符串",
            example: Some(r#"aura.json.stringify({name: "Aura"})"#),
            rust_fn: "std_json::nat_stringify",
        });
        self.docs.push(StdDoc {
            module: "json",
            name: "aura.json.isValid",
            summary: "检查字符串是否为合法 JSON。",
            params: &[("text", "String", "待检查的字符串")],
            returns: "Bool",
            returns_desc: "合法返回 true",
            example: None,
            rust_fn: "std_json::nat_is_valid",
        });

        // ── std.time ──
        self.docs.push(StdDoc {
            module: "time",
            name: "aura.time.now",
            summary: "返回当前 Unix 时间戳（秒）。",
            params: &[],
            returns: "Float",
            returns_desc: "自 1970-01-01 以来的秒数",
            example: None,
            rust_fn: "std_time::nat_now",
        });
        self.docs.push(StdDoc {
            module: "time",
            name: "aura.time.sleep",
            summary: "暂停执行指定秒数。",
            params: &[("seconds", "Float", "暂停秒数")],
            returns: "Unit",
            returns_desc: "无返回值",
            example: Some(r#"aura.time.sleep(1.0)"#),
            rust_fn: "std_time::nat_sleep",
        });
        self.docs.push(StdDoc {
            module: "time",
            name: "aura.time.toDateString",
            summary: "将时间戳转换为日期字符串（YYYY-MM-DD）。",
            params: &[("timestamp", "Int", "Unix 时间戳（秒）")],
            returns: "String",
            returns_desc: "格式化日期",
            example: Some(r#"aura.time.toDateString(0) // → "1970-01-01""#),
            rust_fn: "std_time::nat_to_date_string",
        });

        // ── std.test ──
        self.docs.push(StdDoc {
            module: "test",
            name: "aura.test.assertTrue",
            summary: "断言条件为 true，返回 PASS/FAIL 字符串。",
            params: &[
                ("condition", "Value", "断言条件"),
                ("message", "String", "断言消息（可选）"),
            ],
            returns: "String",
            returns_desc: "\"PASS: ...\" 或 \"FAIL: ...\"",
            example: Some(r#"aura.test.assertTrue(true, "all good")"#),
            rust_fn: "std_test::nat_assert_true",
        });
        self.docs.push(StdDoc {
            module: "test",
            name: "aura.test.assertEq",
            summary: "断言两个值相等。",
            params: &[
                ("a", "Value", "实际值"),
                ("b", "Value", "期望值"),
                ("message", "String", "消息（可选）"),
            ],
            returns: "String",
            returns_desc: "\"PASS: ...\" 或 \"FAIL: ...\"",
            example: Some(r#"aura.test.assertEq(42, 42, "answer")"#),
            rust_fn: "std_test::nat_assert_eq",
        });

        // ── std.builtin ──
        self.docs.push(StdDoc {
            module: "builtin",
            name: "aura.builtin.typeof",
            summary: "返回值的类型名称。",
            params: &[("value", "Value", "待检查的值")],
            returns: "String",
            returns_desc: "类型名（Int/Float/Boolean/String/List/Map/Null）",
            example: Some(r#"aura.builtin.typeof(42) // → "Int""#),
            rust_fn: "std_builtin::nat_typeof",
        });
        self.docs.push(StdDoc {
            module: "builtin",
            name: "aura.builtin.toString",
            summary: "将任意值转换为字符串。",
            params: &[("value", "Value", "待转换的值")],
            returns: "String",
            returns_desc: "字符串表示",
            example: None,
            rust_fn: "std_builtin::nat_to_string",
        });

        // ── std.env ──
        self.docs.push(StdDoc {
            module: "env",
            name: "aura.env.get",
            summary: "获取环境变量值。",
            params: &[
                ("name", "String", "变量名"),
                ("default", "String", "不存在时的默认值（可选）"),
            ],
            returns: "String",
            returns_desc: "变量值或默认值",
            example: None,
            rust_fn: "std_env::nat_get",
        });
        self.docs.push(StdDoc {
            module: "env",
            name: "aura.env.has",
            summary: "检查环境变量是否存在。",
            params: &[("name", "String", "变量名")],
            returns: "Bool",
            returns_desc: "存在返回 true",
            example: None,
            rust_fn: "std_env::nat_has",
        });
        self.docs.push(StdDoc {
            module: "env",
            name: "aura.env.platform",
            summary: "返回当前操作系统名称。",
            params: &[],
            returns: "String",
            returns_desc: "\"windows\" / \"linux\" / \"macos\"",
            example: None,
            rust_fn: "std_env::nat_platform",
        });

        // ── std.random ──
        self.docs.push(StdDoc {
            module: "random",
            name: "aura.random.nextInt",
            summary: "返回 64 位随机整数。",
            params: &[],
            returns: "Int",
            returns_desc: "随机 i64 值",
            example: None,
            rust_fn: "std_random::nat_next_int",
        });
        self.docs.push(StdDoc {
            module: "random",
            name: "aura.random.nextFloat",
            summary: "返回 [0, 1) 区间的随机浮点数。",
            params: &[],
            returns: "Float",
            returns_desc: "随机 f64 值",
            example: None,
            rust_fn: "std_random::nat_next_float",
        });
        self.docs.push(StdDoc {
            module: "random",
            name: "aura.random.nextIntRange",
            summary: "返回 [min, max) 区间的随机整数。",
            params: &[("min", "Int", "下界（含）"), ("max", "Int", "上界（不含）")],
            returns: "Int",
            returns_desc: "区间内随机整数",
            example: Some(r#"aura.random.nextIntRange(1, 100)"#),
            rust_fn: "std_random::nat_next_int_range",
        });
        self.docs.push(StdDoc {
            module: "random",
            name: "aura.random.choice",
            summary: "从参数列表中随机选择一个元素。",
            params: &[("...", "Value", "候选元素")],
            returns: "Value",
            returns_desc: "随机选中的元素",
            example: Some(r#"aura.random.choice(1, 2, 3)"#),
            rust_fn: "std_random::nat_choice",
        });
        self.docs.push(StdDoc {
            module: "random",
            name: "aura.random.shuffle",
            summary: "返回列表的随机排列。",
            params: &[("list", "List", "源列表")],
            returns: "List",
            returns_desc: "打乱后的列表",
            example: None,
            rust_fn: "std_random::nat_shuffle",
        });

        // ── std.encoding ──
        self.docs.push(StdDoc {
            module: "encoding",
            name: "aura.encoding.base64Encode",
            summary: "将字符串编码为 Base64。",
            params: &[("text", "String", "源字符串")],
            returns: "String",
            returns_desc: "Base64 编码字符串",
            example: Some(r#"aura.encoding.base64Encode("Hello")"#),
            rust_fn: "std_encoding::nat_base64_encode",
        });
        self.docs.push(StdDoc {
            module: "encoding",
            name: "aura.encoding.base64Decode",
            summary: "将 Base64 字符串解码。",
            params: &[("text", "String", "Base64 字符串")],
            returns: "String",
            returns_desc: "解码后的字符串，失败返回错误信息",
            example: None,
            rust_fn: "std_encoding::nat_base64_decode",
        });
        self.docs.push(StdDoc {
            module: "encoding",
            name: "aura.encoding.hexEncode",
            summary: "将字符串编码为十六进制。",
            params: &[("text", "String", "源字符串")],
            returns: "String",
            returns_desc: "十六进制字符串",
            example: Some(r#"aura.encoding.hexEncode("Hi") // → "4869""#),
            rust_fn: "std_encoding::nat_hex_encode",
        });

        // ── std.ascii ──
        self.docs.push(StdDoc {
            module: "ascii",
            name: "aura.ascii.isAlpha",
            summary: "判断首字符是否为字母。",
            params: &[("text", "String", "字符串")],
            returns: "Bool",
            returns_desc: "是字母返回 true",
            example: Some(r#"aura.ascii.isAlpha("A") // → true"#),
            rust_fn: "std_ascii::nat_is_alpha",
        });
        self.docs.push(StdDoc {
            module: "ascii",
            name: "aura.ascii.isDigit",
            summary: "判断首字符是否为数字。",
            params: &[("text", "String", "字符串")],
            returns: "Bool",
            returns_desc: "是数字返回 true",
            example: None,
            rust_fn: "std_ascii::nat_is_digit",
        });
        self.docs.push(StdDoc {
            module: "ascii",
            name: "aura.ascii.codeAt",
            summary: "返回指定位置的字符 Unicode 码点。",
            params: &[
                ("text", "String", "字符串"),
                ("index", "Int", "字符位置"),
            ],
            returns: "Int",
            returns_desc: "码点值，越界返回 0",
            example: Some(r#"aura.ascii.codeAt("A", 0) // → 65"#),
            rust_fn: "std_ascii::nat_code_at",
        });

        // ── std.path ──
        self.docs.push(StdDoc {
            module: "path",
            name: "aura.path.join",
            summary: "拼接多个路径组件。",
            params: &[("...", "String", "路径组件")],
            returns: "String",
            returns_desc: "拼接后的路径",
            example: Some(r#"aura.path.join("dir", "file.txt")"#),
            rust_fn: "std_path::nat_join",
        });
        self.docs.push(StdDoc {
            module: "path",
            name: "aura.path.basename",
            summary: "返回文件名（不含扩展名）。",
            params: &[("path", "String", "文件路径")],
            returns: "String",
            returns_desc: "文件基名",
            example: Some(r#"aura.path.basename("dir/file.txt") // → "file""#),
            rust_fn: "std_path::nat_basename",
        });
        self.docs.push(StdDoc {
            module: "path",
            name: "aura.path.extname",
            summary: "返回文件扩展名（含点）。",
            params: &[("path", "String", "文件路径")],
            returns: "String",
            returns_desc: "扩展名",
            example: Some(r#"aura.path.extname("file.txt") // → ".txt""#),
            rust_fn: "std_path::nat_extname",
        });

        // ── std.iter ──
        self.docs.push(StdDoc {
            module: "iter",
            name: "aura.iter.sum",
            summary: "返回列表中所有元素的和。",
            params: &[("list", "List", "源列表")],
            returns: "Int / Float",
            returns_desc: "元素总和",
            example: Some(r#"aura.iter.sum(listOf(1, 2, 3)) // → 6"#),
            rust_fn: "std_iter::nat_sum",
        });
        self.docs.push(StdDoc {
            module: "iter",
            name: "aura.iter.avg",
            summary: "返回列表中所有元素的平均值。",
            params: &[("list", "List", "源列表")],
            returns: "Float",
            returns_desc: "平均值",
            example: None,
            rust_fn: "std_iter::nat_avg",
        });
        self.docs.push(StdDoc {
            module: "iter",
            name: "aura.iter.distinct",
            summary: "返回列表的去重结果（保持顺序）。",
            params: &[("list", "List", "源列表")],
            returns: "List",
            returns_desc: "去重后的列表",
            example: None,
            rust_fn: "std_iter::nat_distinct",
        });
        self.docs.push(StdDoc {
            module: "iter",
            name: "aura.iter.range",
            summary: "生成闭区间 [from, to] 的整数列表。",
            params: &[("from", "Int", "起始值"), ("to", "Int", "结束值")],
            returns: "List",
            returns_desc: "整数列表",
            example: Some(r#"aura.iter.range(1, 5) // → [1, 2, 3, 4, 5]"#),
            rust_fn: "std_iter::nat_range",
        });

        // ── std.console ──
        self.docs.push(StdDoc {
            module: "console",
            name: "aura.console.red",
            summary: "将文本包装为红色 ANSI 颜色代码。",
            params: &[("text", "String", "文本")],
            returns: "String",
            returns_desc: "带颜色代码的字符串",
            example: Some(r#"aura.io.println(console.red("Error!"))"#),
            rust_fn: "std_console::nat_red",
        });
        self.docs.push(StdDoc {
            module: "console",
            name: "aura.console.green",
            summary: "将文本包装为绿色 ANSI 颜色代码。",
            params: &[("text", "String", "文本")],
            returns: "String",
            returns_desc: "带颜色代码的字符串",
            example: None,
            rust_fn: "std_console::nat_green",
        });

        // ── std.process ──
        self.docs.push(StdDoc {
            module: "process",
            name: "aura.process.pid",
            summary: "返回当前进程 ID。",
            params: &[],
            returns: "Int",
            returns_desc: "进程 ID",
            example: None,
            rust_fn: "std_process::nat_pid",
        });
        self.docs.push(StdDoc {
            module: "process",
            name: "aura.process.args",
            summary: "返回命令行参数列表。",
            params: &[],
            returns: "List<String>",
            returns_desc: "参数列表",
            example: None,
            rust_fn: "std_process::nat_args",
        });

        // ── std.net ──
        self.docs.push(StdDoc {
            module: "net",
            name: "aura.net.getHostname",
            summary: "返回本机主机名。",
            params: &[],
            returns: "String",
            returns_desc: "主机名字符串",
            example: None,
            rust_fn: "std_net::nat_get_hostname",
        });
        self.docs.push(StdDoc {
            module: "net",
            name: "aura.net.getLocalIp",
            summary: "返回本机 IP 地址。",
            params: &[],
            returns: "String",
            returns_desc: "IP 地址字符串",
            example: None,
            rust_fn: "std_net::nat_get_local_ip",
        });

        // ── std.assert ──
        self.docs.push(StdDoc {
            module: "assert",
            name: "aura.assert.assert",
            summary: "通用断言，返回 OK/ASSERTION FAILED。",
            params: &[
                ("condition", "Value", "断言条件"),
                ("message", "String", "消息（可选）"),
            ],
            returns: "String",
            returns_desc: "\"OK: ...\" 或 \"ASSERTION FAILED: ...\"",
            example: None,
            rust_fn: "std_assert::nat_assert",
        });

        self
    }

    /// 获取所有文档条目
    pub fn all(&self) -> &[StdDoc] {
        &self.docs
    }

    /// 按模块名获取文档
    pub fn by_module(&self, module: &str) -> Vec<&StdDoc> {
        self.docs.iter().filter(|d| d.module == module).collect()
    }

    /// 获取全部模块名（去重、排序）
    pub fn module_names(&self) -> Vec<&'static str> {
        let mut names: Vec<&'static str> = self
            .docs
            .iter()
            .map(|d| d.module)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        names.sort();
        names
    }

    /// 按模块分组返回文档
    pub fn grouped(&self) -> Vec<(&'static str, Vec<&StdDoc>)> {
        let mut map: std::collections::BTreeMap<&'static str, Vec<&StdDoc>> =
            std::collections::BTreeMap::new();
        for doc in &self.docs {
            map.entry(doc.module).or_default().push(doc);
        }
        map.into_iter().collect()
    }

    /// 验证所有文档条目是否都在 NativeRegistry 中注册
    pub fn verify_against_registry(&self, registry: &NativeRegistry) -> Vec<String> {
        let mut warnings = Vec::new();
        for doc in &self.docs {
            if !registry.contains(doc.name) {
                warnings.push(format!(
                    "文档条目 '{}' 未在 NativeRegistry 中注册",
                    doc.name
                ));
            }
        }
        warnings
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Markdown 渲染
// ─────────────────────────────────────────────────────────────────────────────

/// 渲染全部文档为 Markdown 字符串
pub fn render_markdown(registry: &DocRegistry) -> String {
    let mut out = String::with_capacity(16384);

    // ── 文档头 ──
    out.push_str("# Aura 标准库 API 文档\n\n");
    out.push_str(&format!(
        "> 自动生成于 {} | 共 {} 个模块，{} 个函数\n\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M"),
        registry.module_names().len(),
        registry.all().len()
    ));
    out.push_str("---\n\n");

    // ── 目录 ──
    out.push_str("## 目录\n\n");
    for module in registry.module_names() {
        let count = registry.by_module(module).len();
        out.push_str(&format!(
            "- [std.{}]({}) — {} 个函数\n",
            module,
            anchor_name(module),
            count
        ));
    }
    out.push('\n');

    // ── 各模块文档 ──
    for (module, docs) in registry.grouped() {
        out.push_str(&format!("## std.{}\n\n", module));
        out.push_str(&format!(
            "> 模块名称: `std.{}` | 函数数: {}\n\n",
            module,
            docs.len()
        ));

        for doc in docs {
            render_func_doc(&mut out, doc);
        }

        out.push('\n');
    }

    out
}

/// 渲染单个函数文档
fn render_func_doc(out: &mut String, doc: &StdDoc) {
    // 标题
    out.push_str(&format!("### {}\n\n", doc.name));
    out.push_str(&format!("{}\n\n", doc.summary));

    // 签名
    let params_str = if doc.params.is_empty() {
        String::new()
    } else {
        let parts: Vec<String> = doc
            .params
            .iter()
            .map(|(name, ty, _)| format!("{}: {}", name, ty))
            .collect();
        format!("({})", parts.join(", "))
    };
    out.push_str(&format!(
        "**签名:** `{}{}: {}`\n\n",
        doc.name, params_str, doc.returns
    ));

    // 参数表
    if !doc.params.is_empty() {
        out.push_str("#### 参数\n\n");
        out.push_str("| 参数 | 类型 | 描述 |\n");
        out.push_str("|------|------|------|\n");
        for (name, ty, desc) in doc.params {
            out.push_str(&format!("| `{}` | `{}` | {}\n", name, ty, desc));
        }
        out.push('\n');
    }

    // 返回值
    out.push_str(&format!("#### 返回值\n\n`{}` — {}\n\n", doc.returns, doc.returns_desc));

    // 示例
    if let Some(example) = doc.example {
        out.push_str("#### 示例\n\n");
        out.push_str("```aura\n");
        out.push_str(example);
        out.push_str("\n```\n\n");
    }

    // 分隔线
    out.push_str("---\n\n");
}

/// 生成 Markdown 锚点名称
fn anchor_name(module: &str) -> String {
    format!("#std{}", module)
}

// ─────────────────────────────────────────────────────────────────────────────
// 生成并写入文件
// ─────────────────────────────────────────────────────────────────────────────

/// 生成文档并写入指定输出目录
pub fn generate_docs(output_dir: &std::path::Path) -> Result<Vec<std::path::PathBuf>, String> {
    let registry = DocRegistry::new().load_all();
    let module_names = registry.module_names();

    std::fs::create_dir_all(output_dir).map_err(|e| format!("创建输出目录失败: {}", e))?;

    let mut written = Vec::new();

    // 生成每个模块的独立文档
    for module in &module_names {
        let content = render_module_markdown(&registry, module);
        let file_path = output_dir.join(format!("std_{}.md", module));
        std::fs::write(&file_path, &content)
            .map_err(|e| format!("写入 {} 失败: {}", file_path.display(), e))?;
        written.push(file_path);
    }

    // 生成总览文档
    let index_content = render_markdown(&registry);
    let index_path = output_dir.join("index.md");
    std::fs::write(&index_path, &index_content)
        .map_err(|e| format!("写入 {} 失败: {}", index_path.display(), e))?;
    written.push(index_path);

    Ok(written)
}

/// 渲染单个模块的独立 Markdown
pub fn render_module_markdown(registry: &DocRegistry, module: &str) -> String {
    let docs = registry.by_module(module);
    let mut out = String::with_capacity(8192);

    out.push_str(&format!("# std.{} — API 文档\n\n", module));
    out.push_str(&format!(
        "> 函数数: {} | [返回目录](index.md)\n\n",
        docs.len()
    ));

    out.push_str("## 目录\n\n");
    for doc in &docs {
        let short_name = doc.name.split('.').nth(1).unwrap_or(doc.name);
        out.push_str(&format!(
            "- [{}]({}) — {}\n",
            short_name,
            anchor_name(short_name),
            doc.summary
        ));
    }
    out.push('\n');

    for doc in &docs {
        render_func_doc(&mut out, doc);
    }

    out
}

// ─────────────────────────────────────────────────────────────────────────────
// HTML 渲染（基础版）
// ─────────────────────────────────────────────────────────────────────────────

/// 将 Markdown 文档转换为简单的 HTML 页面
pub fn render_html(registry: &DocRegistry) -> String {
    let mut out = String::with_capacity(16384);

    out.push_str(
        "<!DOCTYPE html>\n<html lang=\"zh-CN\">\n<head>\n<meta charset=\"UTF-8\">\n",
    );
    out.push_str(&format!(
        "<title>Aura 标准库文档 — {} 模块，{} 函数</title>\n",
        registry.module_names().len(),
        registry.all().len()
    ));
    out.push_str(
        "<style>\nbody{font-family:system-ui,sans-serif;max-width:960px;margin:0 auto;padding:2rem;line-height:1.6;color:#222}\nh1{border-bottom:2px solid #333;padding-bottom:.5rem}h2{color:#0066cc;margin-top:2rem}h3{color:#333}\ncode{background:#f4f4f4;padding:2px 6px;border-radius:3px;font-size:.9em}\ntable{border-collapse:collapse;width:100%;margin:1rem 0}th,td{border:1px solid #ddd;padding:.5rem;text-align:left}th{background:#f8f8f8}\npre{background:#f8f8f8;padding:1rem;border-radius:4px;overflow-x:auto}pre code{background:none;padding:0}\nblockquote{border-left:4px solid #0066cc;margin:1rem 0;padding:1rem;background:#f0f8ff}\nhr{border:none;border-top:1px solid #ddd;margin:2rem 0}a{color:#0066cc;text-decoration:none}a:hover{text-decoration:underline}\n</style>\n",
    );
    out.push_str("</head>\n<body>\n");

    // 标题
    out.push_str("<h1>Aura 标准库 API 文档</h1>\n");
    out.push_str(&format!(
        "<blockquote>共 {} 个模块，{} 个函数</blockquote>\n",
        registry.module_names().len(),
        registry.all().len()
    ));

    // 目录
    out.push_str("<h2>目录</h2>\n<ul>\n");
    for module in registry.module_names() {
        let count = registry.by_module(module).len();
        out.push_str(&format!(
            "<li><a href=\"#std{}\">std.{}</a> — {} 个函数</li>\n",
            module, module, count
        ));
    }
    out.push_str("</ul>\n<hr>\n");

    // 各模块
    for (module, docs) in registry.grouped() {
        out.push_str(&format!("<h2 id=\"std{}\">std.{}</h2>\n", module, module));
        out.push_str(&format!(
            "<blockquote>函数数: {} | <a href=\"index.html\">返回目录</a></blockquote>\n",
            docs.len()
        ));

        for doc in docs {
            let short_name = doc.name.split('.').nth(1).unwrap_or(doc.name);
            out.push_str(&format!(
                "<h3 id=\"{}\">{}</h3>\n",
                short_name.replace('.', "_"),
                doc.name
            ));
            out.push_str(&format!("<p>{}</p>\n", escape_html(doc.summary)));
            out.push_str(&format!(
                "<p><strong>签名:</strong> <code>{}</code></p>\n",
                format_sign(&doc.name, doc.params, doc.returns)
            ));
            if !doc.params.is_empty() {
                out.push_str("<table>\n<thead><tr><th>参数</th><th>类型</th><th>描述</th></tr></thead>\n<tbody>\n");
                for (name, ty, desc) in doc.params {
                    out.push_str(&format!(
                        "<tr><td><code>{}</code></td><td><code>{}</code></td><td>{}</td></tr>\n",
                        name,
                        ty,
                        escape_html(desc)
                    ));
                }
                out.push_str("</tbody>\n</table>\n");
            }
            out.push_str(&format!(
                "<p><strong>返回值:</strong> <code>{}</code> — {}</p>\n",
                doc.returns,
                escape_html(doc.returns_desc)
            ));
            if let Some(example) = doc.example {
                out.push_str("<p><strong>示例:</strong></p>\n<pre><code>");
                out.push_str(&escape_html(example));
                out.push_str("</code></pre>\n");
            }
            out.push_str("<hr>\n");
        }
        out.push_str("<hr>\n");
    }

    out.push_str("</body>\n</html>\n");
    out
}

/// 格式化签名
fn format_sign(name: &str, params: &[(&str, &str, &str)], returns: &str) -> String {
    if params.is_empty() {
        format!("{}(): {}", name, returns)
    } else {
        let parts: Vec<String> = params
            .iter()
            .map(|(n, t, _)| format!("{}: {}", n, t))
            .collect();
        format!("{}({}): {}", name, parts.join(", "), returns)
    }
}

/// HTML 转义
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_doc_registry_load() {
        let registry = DocRegistry::new().load_all();
        assert!(registry.all().len() >= 50, "should have at least 50 docs");
    }

    #[test]
    fn test_doc_registry_modules() {
        let registry = DocRegistry::new().load_all();
        let modules = registry.module_names();
        assert!(modules.contains(&"math"));
        assert!(modules.contains(&"string"));
        assert!(modules.contains(&"io"));
        assert!(modules.contains(&"collections"));
        assert!(modules.contains(&"json"));
        assert!(modules.contains(&"time"));
        assert!(modules.contains(&"test"));
        assert!(modules.contains(&"builtin"));
        assert!(modules.contains(&"env"));
        assert!(modules.contains(&"random"));
        assert!(modules.contains(&"encoding"));
        assert!(modules.contains(&"ascii"));
        assert!(modules.contains(&"console"));
        assert!(modules.contains(&"path"));
        assert!(modules.contains(&"iter"));
        assert!(modules.contains(&"fs"));
        assert!(modules.contains(&"net"));
        assert!(modules.contains(&"process"));
        assert!(modules.contains(&"assert"));
    }

    #[test]
    fn test_markdown_render() {
        let registry = DocRegistry::new().load_all();
        let md = render_markdown(&registry);
        assert!(md.contains("# Aura 标准库 API 文档"));
        assert!(md.contains("std.math"));
        assert!(md.contains("aura.math.abs"));
        assert!(md.contains("```aura"));
    }

    #[test]
    fn test_html_render() {
        let registry = DocRegistry::new().load_all();
        let html = render_html(&registry);
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("<title>Aura 标准库"));
        assert!(html.contains("std.math"));
        assert!(html.contains("aura.math.abs"));
    }

    #[test]
    fn test_anchor_name() {
        assert_eq!(anchor_name("math"), "#stdmath");
        assert_eq!(anchor_name("io"), "#stdio");
    }

    #[test]
    fn test_escape_html() {
        assert_eq!(escape_html("<script>"), "&lt;script&gt;");
        assert_eq!(escape_html("a & b"), "a &amp; b");
        assert_eq!(escape_html("\"hello\""), "&quot;hello&quot;");
    }

    #[test]
    /// 验证文档注册表与原生函数注册表一致
    /// 仅在 std-all feature 启用时运行（需要全部 std 模块）
    #[cfg(feature = "std-all")]
    #[test]
    fn test_verify_against_registry() {
        let registry = DocRegistry::new().load_all();
        let native_registry = NativeRegistry::new();
        let warnings = registry.verify_against_registry(&native_registry);
        assert!(
            warnings.is_empty(),
            "All docs should be registered: {}",
            warnings.join("; ")
        );
    }

    #[test]
    fn test_generate_docs() {
        let tmp = std::env::temp_dir().join("aura_docgen_test");
        let _ = std::fs::remove_dir_all(&tmp);
        let result = generate_docs(&tmp);
        assert!(result.is_ok(), "generate_docs failed: {}", result.unwrap_err());
        let files = result.unwrap();
        assert!(files.len() >= 2, "should write at least index + 1 module");
        let index = tmp.join("index.md");
        assert!(index.exists(), "index.md should exist");
        let content = std::fs::read_to_string(&index).unwrap();
        assert!(content.contains("# Aura 标准库"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_doc_counts() {
        let registry = DocRegistry::new().load_all();
        let total = registry.all().len();
        let module_count = registry.module_names().len();
        println!("Total docs: {}", total);
        println!("Modules: {}", module_count);
        for (mod_name, docs) in registry.grouped() {
            println!("  {}: {} functions", mod_name, docs.len());
        }
        assert!(module_count >= 18);
        assert!(total >= 80);
    }
}
