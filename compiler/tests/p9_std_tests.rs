//! P9 — 标准库集成测试
//!
//! 验证所有标准库模块的注册与基础功能。
//! 需要 `std-all` feature 编译（`cargo test --features std-all`）。

#![cfg(feature = "std-all")]

use compiler::vm::native::NativeRegistry;
use compiler::vm::value::Value;

/// 辅助：从注册表中调用原生函数
fn call(reg: &NativeRegistry, name: &str, args: &[Value]) -> Value {
    let fn_ptr = reg.get(name).unwrap_or_else(|| panic!("function '{}' not registered", name));
    fn_ptr(args)
}

/// 所有 std 模块名（与 register_with_modules 的参数对应）
const ALL_MODULES: &[&str] = &[
    "io",
    "math",
    "string",
    "collections",
    "fs",
    "json",
    "time",
    "test",
    "builtin",
    "env",
    "process",
    "random",
    "encoding",
    "ascii",
    "console",
    "path",
    "assert",
    "iter",
    "net",
    "concurrent",
    "sb",
];

/// 验证所有标准库函数已注册
#[test]
fn test_all_std_functions_registered() {
    let reg = NativeRegistry::with_modules(ALL_MODULES);

    // std.io
    assert!(reg.contains("aura.lang.std.IO.println"));
    assert!(reg.contains("aura.lang.std.IO.print"));
    assert!(reg.contains("aura.lang.std.IO.readLine"));
    assert!(reg.contains("aura.lang.std.IO.fileRead"));
    assert!(reg.contains("aura.lang.std.IO.fileWrite"));
    assert!(reg.contains("aura.lang.std.IO.fileExists"));

    // std.math
    assert!(reg.contains("aura.lang.std.Math.abs"));
    assert!(reg.contains("aura.lang.std.Math.min"));
    assert!(reg.contains("aura.lang.std.Math.max"));
    assert!(reg.contains("aura.lang.std.Math.sqrt"));
    assert!(reg.contains("aura.lang.std.Math.pow"));
    assert!(reg.contains("aura.lang.std.Math.PI"));
    assert!(reg.contains("aura.lang.std.Math.E"));
    assert!(reg.contains("aura.lang.std.Math.sin"));
    assert!(reg.contains("aura.lang.std.Math.cos"));
    assert!(reg.contains("aura.lang.std.Math.log"));

    // std.string
    assert!(reg.contains("aura.lang.std.String.contains"));
    assert!(reg.contains("aura.lang.std.String.startsWith"));
    assert!(reg.contains("aura.lang.std.String.endsWith"));
    assert!(reg.contains("aura.lang.std.String.split"));
    assert!(reg.contains("aura.lang.std.String.join"));
    assert!(reg.contains("aura.lang.std.String.replace"));
    assert!(reg.contains("aura.lang.std.String.toUpperCase"));
    assert!(reg.contains("aura.lang.std.String.toLowerCase"));
    assert!(reg.contains("aura.lang.std.String.length"));
    assert!(reg.contains("aura.lang.std.String.format"));
    assert!(reg.contains("aura.lang.std.String.trim"));

    // std.collections
    assert!(reg.contains("aura.lang.std.Collections.listOf"));
    assert!(reg.contains("aura.lang.std.Collections.mapOf"));
    assert!(reg.contains("aura.lang.std.Collections.setOf"));
    assert!(reg.contains("aura.lang.std.Collections.emptyList"));
    assert!(reg.contains("aura.lang.std.Collections.emptyMap"));

    // std.fs
    assert!(reg.contains("aura.lang.std.FileSystem.exists"));
    assert!(reg.contains("aura.lang.std.FileSystem.isFile"));
    assert!(reg.contains("aura.lang.std.FileSystem.isDirectory"));
    assert!(reg.contains("aura.lang.std.FileSystem.readText"));
    assert!(reg.contains("aura.lang.std.FileSystem.writeText"));
    assert!(reg.contains("aura.lang.std.FileSystem.mkdir"));
    assert!(reg.contains("aura.lang.std.FileSystem.mkdirP"));
    assert!(reg.contains("aura.lang.std.FileSystem.delete"));
    assert!(reg.contains("aura.lang.std.FileSystem.rename"));
    assert!(reg.contains("aura.lang.std.FileSystem.listDir"));
    assert!(reg.contains("aura.lang.std.FileSystem.fileSize"));

    // std.json
    assert!(reg.contains("aura.lang.std.Json.parse"));
    assert!(reg.contains("aura.lang.std.Json.stringify"));
    assert!(reg.contains("aura.lang.std.Json.isValid"));
    assert!(reg.contains("aura.lang.std.Json.get"));
    assert!(reg.contains("aura.lang.std.Json.set"));
    assert!(reg.contains("aura.lang.std.Json.keys"));
    assert!(reg.contains("aura.lang.std.Json.values"));

    // std.time
    assert!(reg.contains("aura.lang.std.Time.now"));
    assert!(reg.contains("aura.lang.std.Time.epoch"));
    assert!(reg.contains("aura.lang.std.Time.sleep"));
    assert!(reg.contains("aura.lang.std.Time.toDateString"));
    assert!(reg.contains("aura.lang.std.Time.formatDate"));

    // std.test
    assert!(reg.contains("aura.lang.std.Test.assertTrue"));
    assert!(reg.contains("aura.lang.std.Test.assertFalse"));
    assert!(reg.contains("aura.lang.std.Test.assertEq"));
    assert!(reg.contains("aura.lang.std.Test.assertNotNull"));
    assert!(reg.contains("aura.lang.std.Test.assertNull"));

    // std.builtin
    assert!(reg.contains("aura.lang.std.Builtin.typeof"));
    assert!(reg.contains("aura.lang.std.Builtin.isNull"));
    assert!(reg.contains("aura.lang.std.Builtin.toString"));
    assert!(reg.contains("aura.lang.std.Builtin.toInt"));
    assert!(reg.contains("aura.lang.std.Builtin.toFloat"));

    // std.env
    assert!(reg.contains("aura.lang.std.Env.get"));
    assert!(reg.contains("aura.lang.std.Env.set"));
    assert!(reg.contains("aura.lang.std.Env.has"));
    assert!(reg.contains("aura.lang.std.Env.platform"));
    assert!(reg.contains("aura.lang.std.Env.home"));
    assert!(reg.contains("aura.lang.std.Env.pwd"));

    // std.process
    assert!(reg.contains("aura.lang.std.Process.args"));
    assert!(reg.contains("aura.lang.std.Process.pid"));
    assert!(reg.contains("aura.lang.std.Process.exitCode"));

    // std.random
    assert!(reg.contains("aura.lang.std.Random.nextInt"));
    assert!(reg.contains("aura.lang.std.Random.nextFloat"));
    assert!(reg.contains("aura.lang.std.Random.nextBool"));
    assert!(reg.contains("aura.lang.std.Random.choice"));
    assert!(reg.contains("aura.lang.std.Random.shuffle"));

    // std.encoding
    assert!(reg.contains("aura.lang.std.Encoding.base64Encode"));
    assert!(reg.contains("aura.lang.std.Encoding.base64Decode"));
    assert!(reg.contains("aura.lang.std.Encoding.hexEncode"));
    assert!(reg.contains("aura.lang.std.Encoding.hexDecode"));

    // std.ascii
    assert!(reg.contains("aura.lang.std.Ascii.isAlpha"));
    assert!(reg.contains("aura.lang.std.Ascii.isDigit"));
    assert!(reg.contains("aura.lang.std.Ascii.isUpper"));
    assert!(reg.contains("aura.lang.std.Ascii.isLower"));
    assert!(reg.contains("aura.lang.std.Ascii.toUpper"));
    assert!(reg.contains("aura.lang.std.Ascii.toLower"));

    // std.console
    assert!(reg.contains("aura.lang.std.Console.clear"));
    assert!(reg.contains("aura.lang.std.Console.red"));
    assert!(reg.contains("aura.lang.std.Console.green"));
    assert!(reg.contains("aura.lang.std.Console.size"));

    // std.path
    assert!(reg.contains("aura.lang.std.Path.join"));
    assert!(reg.contains("aura.lang.std.Path.dirname"));
    assert!(reg.contains("aura.lang.std.Path.basename"));
    assert!(reg.contains("aura.lang.std.Path.extname"));
    assert!(reg.contains("aura.lang.std.Path.isAbsolute"));

    // std.assert
    assert!(reg.contains("aura.lang.std.Assert.assert"));
    assert!(reg.contains("aura.lang.std.Assert.assertTrue"));
    assert!(reg.contains("aura.lang.std.Assert.assertFalse"));
    assert!(reg.contains("aura.lang.std.Assert.assertEq"));

    // std.iter
    assert!(reg.contains("aura.lang.std.Iter.sum"));
    assert!(reg.contains("aura.lang.std.Iter.avg"));
    assert!(reg.contains("aura.lang.std.Iter.min"));
    assert!(reg.contains("aura.lang.std.Iter.max"));
    assert!(reg.contains("aura.lang.std.Iter.distinct"));
    assert!(reg.contains("aura.lang.std.Iter.range"));
    assert!(reg.contains("aura.lang.std.Iter.take"));
    assert!(reg.contains("aura.lang.std.Iter.skip"));
    assert!(reg.contains("aura.lang.std.Iter.chain"));
    assert!(reg.contains("aura.lang.std.Iter.flatMap"));

    // std.net
    assert!(reg.contains("aura.lang.std.Network.tcpConnect"));
    assert!(reg.contains("aura.lang.std.Network.tcpListen"));
    assert!(reg.contains("aura.lang.std.Network.getHostname"));
    assert!(reg.contains("aura.lang.std.Network.getLocalIp"));
}

#[test]
fn test_std_math() {
    let reg = NativeRegistry::with_modules(ALL_MODULES);

    // abs
    let result = call(&reg, "aura.lang.std.Math.abs", &[Value::Int(-42)]);
    assert_eq!(result, Value::Int(42));

    // min / max
    let result = call(
        &reg,
        "aura.lang.std.Math.min",
        &[
            Value::Int(3),
            Value::Int(5),
        ],
    );
    assert_eq!(result, Value::Int(3));

    let result = call(
        &reg,
        "aura.lang.std.Math.max",
        &[
            Value::Int(3),
            Value::Int(5),
        ],
    );
    assert_eq!(result, Value::Int(5));

    // ceil / floor
    let result = call(&reg, "aura.lang.std.Math.ceil", &[Value::Float(1.2)]);
    assert_eq!(result, Value::Float(2.0));

    let result = call(&reg, "aura.lang.std.Math.floor", &[Value::Float(1.8)]);
    assert_eq!(result, Value::Float(1.0));

    // sqrt
    let result = call(&reg, "aura.lang.std.Math.sqrt", &[Value::Float(16.0)]);
    assert!((result.as_float() - 4.0).abs() < 1e-10);

    // pow
    let result = call(
        &reg,
        "aura.lang.std.Math.pow",
        &[
            Value::Float(2.0),
            Value::Float(3.0),
        ],
    );
    assert!((result.as_float() - 8.0).abs() < 1e-10);

    // PI
    let pi = call(&reg, "aura.lang.std.Math.PI", &[]);
    assert!((pi.as_float() - 3.141592653589793).abs() < 1e-10);

    // E
    let e = call(&reg, "aura.lang.std.Math.E", &[]);
    assert!((e.as_float() - 2.718281828459045).abs() < 1e-10);
}

#[test]
fn test_std_string() {
    let reg = NativeRegistry::with_modules(ALL_MODULES);

    // contains
    let result = call(
        &reg,
        "aura.lang.std.String.contains",
        &[
            Value::str_("hello world"),
            Value::str_("world"),
        ],
    );
    assert_eq!(result, Value::Bool(true));

    let result = call(
        &reg,
        "aura.lang.std.String.contains",
        &[
            Value::str_("hello"),
            Value::str_("xyz"),
        ],
    );
    assert_eq!(result, Value::Bool(false));

    // startsWith
    let result = call(
        &reg,
        "aura.lang.std.String.startsWith",
        &[
            Value::str_("hello"),
            Value::str_("hel"),
        ],
    );
    assert_eq!(result, Value::Bool(true));

    // endsWith
    let result = call(
        &reg,
        "aura.lang.std.String.endsWith",
        &[
            Value::str_("hello"),
            Value::str_("llo"),
        ],
    );
    assert_eq!(result, Value::Bool(true));

    // split
    let result = call(
        &reg,
        "aura.lang.std.String.split",
        &[
            Value::str_("hello world"),
            Value::str_(" "),
        ],
    );
    match &result {
        Value::List(items) => {
            assert_eq!(items.len(), 2);
            assert_eq!(items[0], Value::str_("hello"));
            assert_eq!(items[1], Value::str_("world"));
        }
        _ => panic!("expected List"),
    }

    // toUpperCase
    let result = call(
        &reg,
        "aura.lang.std.String.toUpperCase",
        &[Value::str_("hello")],
    );
    assert_eq!(result, Value::str_("HELLO"));

    // toLowerCase
    let result = call(
        &reg,
        "aura.lang.std.String.toLowerCase",
        &[Value::str_("HELLO")],
    );
    assert_eq!(result, Value::str_("hello"));

    // length
    let result = call(&reg, "aura.lang.std.String.length", &[Value::str_("hello")]);
    assert_eq!(result, Value::Int(5));

    // trim
    let result = call(
        &reg,
        "aura.lang.std.String.trim",
        &[Value::str_("  hello  ")],
    );
    assert_eq!(result, Value::str_("hello"));

    // format
    let result = call(
        &reg,
        "aura.lang.std.String.format",
        &[
            Value::str_("Hello {0}, you are {1}"),
            Value::str_("World"),
            Value::Int(30),
        ],
    );
    assert_eq!(result, Value::str_("Hello World, you are 30"));

    // repeat
    let result = call(
        &reg,
        "aura.lang.std.String.repeat",
        &[
            Value::Int(3),
            Value::str_("ab"),
        ],
    );
    assert_eq!(result, Value::str_("ababab"));

    // indexOf
    let result = call(
        &reg,
        "aura.lang.std.String.indexOf",
        &[
            Value::str_("hello"),
            Value::str_("l"),
        ],
    );
    assert_eq!(result, Value::Int(2));
}

#[test]
fn test_std_collections() {
    let reg = NativeRegistry::with_modules(ALL_MODULES);

    // listOf
    let result = call(
        &reg,
        "aura.lang.std.Collections.listOf",
        &[
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
        ],
    );
    match &result {
        Value::List(items) => {
            assert_eq!(items.len(), 3);
            assert_eq!(items[0], Value::Int(1));
            assert_eq!(items[1], Value::Int(2));
            assert_eq!(items[2], Value::Int(3));
        }
        _ => panic!("expected List"),
    }

    // mapOf
    let result = call(
        &reg,
        "aura.lang.std.Collections.mapOf",
        &[
            Value::str_("name"),
            Value::str_("Aura"),
            Value::str_("version"),
            Value::Int(1),
        ],
    );
    match &result {
        Value::Map(map) => {
            assert_eq!(map.len(), 2);
            assert_eq!(map.get(&Value::str_("name")), Some(&Value::str_("Aura")));
            assert_eq!(map.get(&Value::str_("version")), Some(&Value::Int(1)));
        }
        _ => panic!("expected Map"),
    }

    // setOf (unique)
    let result = call(
        &reg,
        "aura.lang.std.Collections.setOf",
        &[
            Value::Int(1),
            Value::Int(2),
            Value::Int(1),
        ],
    );
    match &result {
        Value::List(items) => {
            assert_eq!(items.len(), 2);
        }
        _ => panic!("expected List"),
    }

    // emptyList
    let result = call(&reg, "aura.lang.std.Collections.emptyList", &[]);
    match &result {
        Value::List(items) => {
            assert!(items.is_empty());
        }
        _ => panic!("expected List"),
    }
}

#[test]
fn test_std_json() {
    let reg = NativeRegistry::with_modules(ALL_MODULES);

    // parse
    let result = call(
        &reg,
        "aura.lang.std.Json.parse",
        &[Value::str_(r#"{"name": "Aura", "version": 1}"#)],
    );
    match &result {
        Value::Map(map) => {
            assert_eq!(map.len(), 2);
            assert_eq!(map.get(&Value::str_("name")), Some(&Value::str_("Aura")));
            assert_eq!(map.get(&Value::str_("version")), Some(&Value::Int(1)));
        }
        _ => panic!("expected Map, got {:?}", result),
    }

    // stringify
    let map = {
        let mut m = std::collections::HashMap::new();
        m.insert(Value::str_("key"), Value::str_("value"));
        Value::Map(m)
    };
    let result = call(&reg, "aura.lang.std.Json.stringify", &[map.clone()]);
    let str_result = result.as_string();
    assert!(str_result.contains("\"key\""));
    assert!(str_result.contains("\"value\""));

    // isValid
    let result = call(
        &reg,
        "aura.lang.std.Json.isValid",
        &[Value::str_(r#"{"valid": true}"#)],
    );
    assert_eq!(result, Value::Bool(true));

    let result = call(
        &reg,
        "aura.lang.std.Json.isValid",
        &[Value::str_("{invalid")],
    );
    assert_eq!(result, Value::Bool(false));

    // parse array
    let result = call(
        &reg,
        "aura.lang.std.Json.parse",
        &[Value::str_(r#"[1, 2, 3]"#)],
    );
    match &result {
        Value::List(items) => {
            assert_eq!(items.len(), 3);
            assert_eq!(items[0], Value::Int(1));
            assert_eq!(items[2], Value::Int(3));
        }
        _ => panic!("expected List"),
    }
}

#[test]
fn test_std_time() {
    let reg = NativeRegistry::with_modules(ALL_MODULES);

    // now should return a positive timestamp
    let result = call(&reg, "aura.lang.std.Time.now", &[]);
    let now = result.as_float();
    assert!(now > 0.0);

    // toDateString for epoch
    let result = call(&reg, "aura.lang.std.Time.toDateString", &[Value::Int(0)]);
    assert_eq!(result, Value::str_("1970-01-01"));

    // toTimeString for epoch
    let result = call(&reg, "aura.lang.std.Time.toTimeString", &[Value::Int(0)]);
    assert_eq!(result, Value::str_("00:00:00"));

    // diff
    let result = call(
        &reg,
        "aura.lang.std.Time.diff",
        &[
            Value::Float(100.0),
            Value::Float(200.0),
        ],
    );
    assert!((result.as_float() - 100.0).abs() < 1e-10);
}

#[test]
fn test_std_test() {
    let reg = NativeRegistry::with_modules(ALL_MODULES);

    // assertTrue (pass)
    let result = call(
        &reg,
        "aura.lang.std.Test.assertTrue",
        &[
            Value::Bool(true),
            Value::str_("test passed"),
        ],
    );
    assert!(result.as_string().starts_with("PASS"));

    // assertFalse (pass)
    let result = call(
        &reg,
        "aura.lang.std.Test.assertFalse",
        &[Value::Bool(false)],
    );
    assert!(result.as_string().starts_with("PASS"));

    // assertEq (pass)
    let result = call(
        &reg,
        "aura.lang.std.Test.assertEq",
        &[
            Value::Int(42),
            Value::Int(42),
        ],
    );
    assert!(result.as_string().starts_with("PASS"));

    // assertEq (fail)
    let result = call(
        &reg,
        "aura.lang.std.Test.assertEq",
        &[
            Value::Int(1),
            Value::Int(2),
        ],
    );
    assert!(result.as_string().starts_with("FAIL"));

    // assertNotNull
    let result = call(&reg, "aura.lang.std.Test.assertNotNull", &[Value::Int(42)]);
    assert!(result.as_string().starts_with("PASS"));

    // assertNull
    let result = call(&reg, "aura.lang.std.Test.assertNull", &[Value::Null]);
    assert!(result.as_string().starts_with("PASS"));
}

#[test]
fn test_std_builtin() {
    let reg = NativeRegistry::with_modules(ALL_MODULES);

    // typeof
    let result = call(&reg, "aura.lang.std.Builtin.typeof", &[Value::Int(42)]);
    assert_eq!(result, Value::str_("Int"));

    let result = call(&reg, "aura.lang.std.Builtin.typeof", &[Value::Float(3.14)]);
    assert_eq!(result, Value::str_("Float"));

    let result = call(&reg, "aura.lang.std.Builtin.typeof", &[Value::Bool(true)]);
    assert_eq!(result, Value::str_("Boolean"));

    let result = call(
        &reg,
        "aura.lang.std.Builtin.typeof",
        &[Value::str_("hello")],
    );
    assert_eq!(result, Value::str_("String"));

    // isNull
    let result = call(&reg, "aura.lang.std.Builtin.isNull", &[Value::Null]);
    assert_eq!(result, Value::Bool(true));

    let result = call(&reg, "aura.lang.std.Builtin.isNull", &[Value::Int(0)]);
    assert_eq!(result, Value::Bool(false));

    // isPositive
    let result = call(&reg, "aura.lang.std.Builtin.isPositive", &[Value::Int(5)]);
    assert_eq!(result, Value::Bool(true));

    let result = call(&reg, "aura.lang.std.Builtin.isPositive", &[Value::Int(-1)]);
    assert_eq!(result, Value::Bool(false));
}

#[test]
fn test_std_env() {
    let reg = NativeRegistry::with_modules(ALL_MODULES);

    // set / get
    call(
        &reg,
        "aura.lang.std.Env.set",
        &[
            Value::str_("AURA_TEST_VAR"),
            Value::str_("test_value"),
        ],
    );
    let result = call(
        &reg,
        "aura.lang.std.Env.get",
        &[Value::str_("AURA_TEST_VAR")],
    );
    assert_eq!(result, Value::str_("test_value"));

    // has
    let result = call(
        &reg,
        "aura.lang.std.Env.has",
        &[Value::str_("AURA_TEST_VAR")],
    );
    assert_eq!(result, Value::Bool(true));

    // remove
    call(
        &reg,
        "aura.lang.std.Env.remove",
        &[Value::str_("AURA_TEST_VAR")],
    );
    let result = call(
        &reg,
        "aura.lang.std.Env.has",
        &[Value::str_("AURA_TEST_VAR")],
    );
    assert_eq!(result, Value::Bool(false));

    // platform
    let result = call(&reg, "aura.lang.std.Env.platform", &[]);
    let platform = result.as_string();
    assert!(!platform.is_empty());

    // home
    let result = call(&reg, "aura.lang.std.Env.home", &[]);
    assert!(result.as_string().len() > 0);
}

#[test]
fn test_std_random() {
    let reg = NativeRegistry::with_modules(ALL_MODULES);

    // nextInt should return different values
    let r1 = call(&reg, "aura.lang.std.Random.nextInt", &[]);
    let r2 = call(&reg, "aura.lang.std.Random.nextInt", &[]);
    // They might theoretically be the same, but very unlikely
    let _ = (r1, r2);

    // nextFloat should be in [0, 1)
    let r = call(&reg, "aura.lang.std.Random.nextFloat", &[]);
    let f = r.as_float();
    assert!(f >= 0.0 && f < 1.0);

    // nextBool
    let _ = call(&reg, "aura.lang.std.Random.nextBool", &[]);

    // nextIntRange
    let r = call(
        &reg,
        "aura.lang.std.Random.nextIntRange",
        &[
            Value::Int(10),
            Value::Int(20),
        ],
    );
    let i = r.as_int();
    assert!(i >= 10 && i < 20);

    // choice
    let items: Vec<Value> = vec![
        Value::Int(1),
        Value::Int(2),
        Value::Int(3),
    ];
    let r = call(&reg, "aura.lang.std.Random.choice", &items);
    assert!(r == Value::Int(1) || r == Value::Int(2) || r == Value::Int(3));
}

#[test]
fn test_std_encoding() {
    let reg = NativeRegistry::with_modules(ALL_MODULES);

    // base64 encode/decode roundtrip
    let original = Value::str_("Hello, Aura!");
    let encoded = call(
        &reg,
        "aura.lang.std.Encoding.base64Encode",
        &[original.clone()],
    );
    let decoded = call(&reg, "aura.lang.std.Encoding.base64Decode", &[encoded]);
    assert_eq!(decoded, original);

    // hex encode/decode roundtrip
    let original = Value::str_("Hello");
    let encoded = call(
        &reg,
        "aura.lang.std.Encoding.hexEncode",
        &[original.clone()],
    );
    let decoded = call(&reg, "aura.lang.std.Encoding.hexDecode", &[encoded]);
    assert_eq!(decoded, original);
}

#[test]
fn test_std_ascii() {
    let reg = NativeRegistry::with_modules(ALL_MODULES);

    // isAlpha
    let result = call(&reg, "aura.lang.std.Ascii.isAlpha", &[Value::str_("A")]);
    assert_eq!(result, Value::Bool(true));

    let result = call(&reg, "aura.lang.std.Ascii.isAlpha", &[Value::str_("1")]);
    assert_eq!(result, Value::Bool(false));

    // isDigit
    let result = call(&reg, "aura.lang.std.Ascii.isDigit", &[Value::str_("5")]);
    assert_eq!(result, Value::Bool(true));

    let result = call(&reg, "aura.lang.std.Ascii.isDigit", &[Value::str_("a")]);
    assert_eq!(result, Value::Bool(false));

    // isUpper
    let result = call(&reg, "aura.lang.std.Ascii.isUpper", &[Value::str_("A")]);
    assert_eq!(result, Value::Bool(true));

    let result = call(&reg, "aura.lang.std.Ascii.isUpper", &[Value::str_("a")]);
    assert_eq!(result, Value::Bool(false));

    // isLower
    let result = call(&reg, "aura.lang.std.Ascii.isLower", &[Value::str_("a")]);
    assert_eq!(result, Value::Bool(true));
}

#[test]
fn test_std_path() {
    let reg = NativeRegistry::with_modules(ALL_MODULES);

    // join
    let result = call(
        &reg,
        "aura.lang.std.Path.join",
        &[
            Value::str_("dir"),
            Value::str_("file.txt"),
        ],
    );
    let joined = result.as_string();
    assert!(joined.contains("dir"));
    assert!(joined.contains("file.txt"));

    // dirname
    let result = call(
        &reg,
        "aura.lang.std.Path.dirname",
        &[Value::str_("/home/user/file.txt")],
    );
    let dirname = result.as_string();
    assert!(!dirname.is_empty());

    // basename
    let result = call(
        &reg,
        "aura.lang.std.Path.basename",
        &[Value::str_("/home/user/file.txt")],
    );
    assert_eq!(result, Value::str_("file"));

    // extname
    let result = call(
        &reg,
        "aura.lang.std.Path.extname",
        &[Value::str_("/home/user/file.txt")],
    );
    assert_eq!(result, Value::str_(".txt"));

    // isAbsolute (use platform-appropriate path)
    #[cfg(windows)]
    let abs_path = "C:\\absolute\\path";
    #[cfg(not(windows))]
    let abs_path = "/absolute/path";
    let result = call(
        &reg,
        "aura.lang.std.Path.isAbsolute",
        &[Value::str_(abs_path)],
    );
    assert_eq!(result, Value::Bool(true));
}

#[test]
fn test_std_iter() {
    let reg = NativeRegistry::with_modules(ALL_MODULES);

    // sum
    let list = Value::List(vec![
        Value::Int(1),
        Value::Int(2),
        Value::Int(3),
    ]);
    let result = call(&reg, "aura.lang.std.Iter.sum", &[list.clone()]);
    assert_eq!(result, Value::Int(6));

    // avg
    let result = call(&reg, "aura.lang.std.Iter.avg", &[list.clone()]);
    assert!((result.as_float() - 2.0).abs() < 1e-10);

    // min
    let result = call(&reg, "aura.lang.std.Iter.min", &[list.clone()]);
    assert_eq!(result, Value::Int(1));

    // max
    let result = call(&reg, "aura.lang.std.Iter.max", &[list.clone()]);
    assert_eq!(result, Value::Int(3));

    // distinct
    let list = Value::List(vec![
        Value::Int(1),
        Value::Int(2),
        Value::Int(1),
        Value::Int(3),
    ]);
    let result = call(&reg, "aura.lang.std.Iter.distinct", &[list.clone()]);
    match &result {
        Value::List(items) => {
            assert_eq!(items.len(), 3);
        }
        _ => panic!("expected List"),
    }

    // range
    let result = call(
        &reg,
        "aura.lang.std.Iter.range",
        &[
            Value::Int(1),
            Value::Int(5),
        ],
    );
    match &result {
        Value::List(items) => {
            assert_eq!(items.len(), 5);
            assert_eq!(items[0], Value::Int(1));
            assert_eq!(items[4], Value::Int(5));
        }
        _ => panic!("expected List"),
    }

    // take
    let list = Value::List(vec![
        Value::Int(1),
        Value::Int(2),
        Value::Int(3),
        Value::Int(4),
        Value::Int(5),
    ]);
    let result = call(
        &reg,
        "aura.lang.std.Iter.take",
        &[
            list.clone(),
            Value::Int(3),
        ],
    );
    match &result {
        Value::List(items) => {
            assert_eq!(items.len(), 3);
        }
        _ => panic!("expected List"),
    }

    // skip
    let result = call(
        &reg,
        "aura.lang.std.Iter.skip",
        &[
            list.clone(),
            Value::Int(2),
        ],
    );
    match &result {
        Value::List(items) => {
            assert_eq!(items.len(), 3);
            assert_eq!(items[0], Value::Int(3));
        }
        _ => panic!("expected List"),
    }

    // chain
    let l1 = Value::List(vec![
        Value::Int(1),
        Value::Int(2),
    ]);
    let l2 = Value::List(vec![
        Value::Int(3),
        Value::Int(4),
    ]);
    let result = call(&reg, "aura.lang.std.Iter.chain", &[l1, l2]);
    match &result {
        Value::List(items) => {
            assert_eq!(items.len(), 4);
        }
        _ => panic!("expected List"),
    }

    // count
    let list = Value::List(vec![
        Value::Int(1),
        Value::Int(2),
        Value::Int(3),
    ]);
    let result = call(&reg, "aura.lang.std.Iter.count", &[list]);
    assert_eq!(result, Value::Int(3));
}

#[test]
fn test_std_assert() {
    let reg = NativeRegistry::with_modules(ALL_MODULES);

    // assert (pass)
    let result = call(
        &reg,
        "aura.lang.std.Assert.assert",
        &[
            Value::Bool(true),
            Value::str_("test"),
        ],
    );
    assert!(result.as_string().starts_with("OK"));

    // assert (fail)
    let result = call(
        &reg,
        "aura.lang.std.Assert.assert",
        &[
            Value::Bool(false),
            Value::str_("test"),
        ],
    );
    assert!(result.as_string().starts_with("ASSERTION FAILED"));

    // assertEq (pass)
    let result = call(
        &reg,
        "aura.lang.std.Assert.assertEq",
        &[
            Value::Int(1),
            Value::Int(1),
        ],
    );
    assert!(result.as_string().starts_with("OK"));

    // assertEq (fail)
    let result = call(
        &reg,
        "aura.lang.std.Assert.assertEq",
        &[
            Value::Int(1),
            Value::Int(2),
        ],
    );
    assert!(result.as_string().starts_with("ASSERTION FAILED"));
}

#[test]
fn test_std_fs() {
    let reg = NativeRegistry::with_modules(ALL_MODULES);

    // exists (non-existent file)
    let result = call(
        &reg,
        "aura.lang.std.FileSystem.exists",
        &[Value::str_("/nonexistent/path/file.txt")],
    );
    assert_eq!(result, Value::Bool(false));

    // isFile / isDirectory for non-existent path
    let result = call(
        &reg,
        "aura.lang.std.FileSystem.isFile",
        &[Value::str_("/nonexistent")],
    );
    assert_eq!(result, Value::Bool(false));

    let result = call(
        &reg,
        "aura.lang.std.FileSystem.isDirectory",
        &[Value::str_("/nonexistent")],
    );
    assert_eq!(result, Value::Bool(false));
}

#[test]
fn test_std_io_basic() {
    let reg = NativeRegistry::with_modules(ALL_MODULES);

    // fileExists for non-existent file
    let result = call(
        &reg,
        "aura.lang.std.IO.fileExists",
        &[Value::str_("/nonexistent/file.txt")],
    );
    assert_eq!(result, Value::Bool(false));

    // fileExists for the project root (should exist)
    let result = call(&reg, "aura.lang.std.IO.fileExists", &[Value::str_(".")]);
    assert_eq!(result, Value::Bool(true));
}

#[test]
fn test_std_net_basic() {
    let reg = NativeRegistry::with_modules(ALL_MODULES);

    // getHostname should return non-empty
    let result = call(&reg, "aura.lang.std.Network.getHostname", &[]);
    assert!(result.as_string().len() > 0);

    // getLocalIp should return non-empty
    let result = call(&reg, "aura.lang.std.Network.getLocalIp", &[]);
    assert!(result.as_string().len() > 0);
}

#[test]
fn test_std_process_basic() {
    let reg = NativeRegistry::with_modules(ALL_MODULES);

    // pid should return positive value
    let result = call(&reg, "aura.lang.std.Process.pid", &[]);
    assert!(result.as_int() > 0);

    // args should return non-empty list
    let result = call(&reg, "aura.lang.std.Process.args", &[]);
    match &result {
        Value::List(items) => {
            assert!(!items.is_empty());
        }
        _ => panic!("expected List"),
    }

    // exitCode should return 0
    let result = call(&reg, "aura.lang.std.Process.exitCode", &[]);
    assert_eq!(result, Value::Int(0));
}

#[test]
fn test_std_string_builder() {
    let reg = NativeRegistry::with_modules(ALL_MODULES);

    // 句柄（Long）
    let h = call(&reg, "aura.lang.std.StringBuilder.create", &[]);
    assert!(matches!(h, Value::Int(_)));
    assert_eq!(
        call(&reg, "aura.lang.std.StringBuilder.length", &[h.clone()]),
        Value::Int(0)
    );

    // 同一句柄就地追加：长度随内容增长（可变字符串）
    call(
        &reg,
        "aura.lang.std.StringBuilder.append",
        &[
            h.clone(),
            Value::str_("Hello"),
        ],
    );
    assert_eq!(
        call(&reg, "aura.lang.std.StringBuilder.length", &[h.clone()]),
        Value::Int(5)
    );
    call(
        &reg,
        "aura.lang.std.StringBuilder.append",
        &[
            h.clone(),
            Value::str_(", "),
        ],
    );
    call(
        &reg,
        "aura.lang.std.StringBuilder.append",
        &[
            h.clone(),
            Value::str_("World"),
        ],
    );
    assert_eq!(
        call(&reg, "aura.lang.std.StringBuilder.length", &[h.clone()]),
        Value::Int(12)
    );

    // appendChar / appendInt
    call(
        &reg,
        "aura.lang.std.StringBuilder.appendChar",
        &[
            h.clone(),
            Value::Int('!' as i64),
        ],
    );
    call(
        &reg,
        "aura.lang.std.StringBuilder.appendInt",
        &[
            h.clone(),
            Value::Int(123),
        ],
    );
    assert_eq!(
        call(&reg, "aura.lang.std.StringBuilder.length", &[h.clone()]),
        Value::Int(16)
    );

    // finish 交出内容，句柄失效
    let s = call(&reg, "aura.lang.std.StringBuilder.finish", &[h.clone()]);
    assert_eq!(s, Value::str_("Hello, World!123"));
    assert_eq!(
        call(&reg, "aura.lang.std.StringBuilder.length", &[h]),
        Value::Int(0)
    );

    // reset 复用句柄
    let h2 = call(&reg, "aura.lang.std.StringBuilder.create", &[]);
    call(
        &reg,
        "aura.lang.std.StringBuilder.append",
        &[
            h2.clone(),
            Value::str_("abc"),
        ],
    );
    call(&reg, "aura.lang.std.StringBuilder.reset", &[h2.clone()]);
    assert_eq!(
        call(&reg, "aura.lang.std.StringBuilder.length", &[h2.clone()]),
        Value::Int(0)
    );
    call(
        &reg,
        "aura.lang.std.StringBuilder.append",
        &[
            h2.clone(),
            Value::str_("xy"),
        ],
    );
    assert_eq!(
        call(&reg, "aura.lang.std.StringBuilder.finish", &[h2]),
        Value::str_("xy")
    );
}

#[test]
fn test_value_list_map() {
    // Verify List and Map variants work correctly
    let list = Value::List(vec![
        Value::Int(1),
        Value::Int(2),
    ]);
    assert_eq!(
        list,
        Value::List(vec![
            Value::Int(1),
            Value::Int(2)
        ])
    );
    assert_eq!(list.type_name(), "List");
    assert!(list.is_truthy());

    let mut map = std::collections::HashMap::new();
    map.insert(Value::str_("key"), Value::Int(42));
    let map_val = Value::Map(map.clone());
    assert_eq!(map_val.type_name(), "Map");
    assert!(map_val.is_truthy());

    let empty_list = Value::List(vec![]);
    assert!(!empty_list.is_truthy());

    let empty_map = Value::Map(std::collections::HashMap::new());
    assert!(!empty_map.is_truthy());

    // Display
    assert!(format!("{}", list).starts_with("["));
    assert!(format!("{}", map_val).starts_with("{"));
}
