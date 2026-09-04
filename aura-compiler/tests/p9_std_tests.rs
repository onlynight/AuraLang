//! P9 — 标准库集成测试
//!
//! 验证所有标准库模块的注册与基础功能。

use aura_compiler::vm::native::NativeRegistry;
use aura_compiler::vm::value::Value;

/// 辅助：从注册表中调用原生函数
fn call(reg: &NativeRegistry, name: &str, args: &[Value]) -> Value {
    let fn_ptr = reg.get(name).unwrap_or_else(|| panic!("function '{}' not registered", name));
    fn_ptr(args)
}

/// 验证所有标准库函数已注册
#[test]
fn test_all_std_functions_registered() {
    let reg = NativeRegistry::new();

    // std.io
    assert!(reg.contains("io.println"));
    assert!(reg.contains("io.print"));
    assert!(reg.contains("io.readLine"));
    assert!(reg.contains("io.fileRead"));
    assert!(reg.contains("io.fileWrite"));
    assert!(reg.contains("io.fileExists"));

    // std.math
    assert!(reg.contains("math.abs"));
    assert!(reg.contains("math.min"));
    assert!(reg.contains("math.max"));
    assert!(reg.contains("math.sqrt"));
    assert!(reg.contains("math.pow"));
    assert!(reg.contains("math.PI"));
    assert!(reg.contains("math.E"));
    assert!(reg.contains("math.sin"));
    assert!(reg.contains("math.cos"));
    assert!(reg.contains("math.log"));

    // std.string
    assert!(reg.contains("string.contains"));
    assert!(reg.contains("string.startsWith"));
    assert!(reg.contains("string.endsWith"));
    assert!(reg.contains("string.split"));
    assert!(reg.contains("string.join"));
    assert!(reg.contains("string.replace"));
    assert!(reg.contains("string.toUpperCase"));
    assert!(reg.contains("string.toLowerCase"));
    assert!(reg.contains("string.length"));
    assert!(reg.contains("string.format"));
    assert!(reg.contains("string.trim"));

    // std.collections
    assert!(reg.contains("collections.listOf"));
    assert!(reg.contains("collections.mapOf"));
    assert!(reg.contains("collections.setOf"));
    assert!(reg.contains("collections.emptyList"));
    assert!(reg.contains("collections.emptyMap"));

    // std.fs
    assert!(reg.contains("fs.exists"));
    assert!(reg.contains("fs.isFile"));
    assert!(reg.contains("fs.isDirectory"));
    assert!(reg.contains("fs.readText"));
    assert!(reg.contains("fs.writeText"));
    assert!(reg.contains("fs.mkdir"));
    assert!(reg.contains("fs.mkdirP"));
    assert!(reg.contains("fs.delete"));
    assert!(reg.contains("fs.rename"));
    assert!(reg.contains("fs.listDir"));
    assert!(reg.contains("fs.fileSize"));

    // std.json
    assert!(reg.contains("json.parse"));
    assert!(reg.contains("json.stringify"));
    assert!(reg.contains("json.isValid"));
    assert!(reg.contains("json.get"));
    assert!(reg.contains("json.set"));
    assert!(reg.contains("json.keys"));
    assert!(reg.contains("json.values"));

    // std.time
    assert!(reg.contains("time.now"));
    assert!(reg.contains("time.epoch"));
    assert!(reg.contains("time.sleep"));
    assert!(reg.contains("time.toDateString"));
    assert!(reg.contains("time.formatDate"));

    // std.test
    assert!(reg.contains("test.assertTrue"));
    assert!(reg.contains("test.assertFalse"));
    assert!(reg.contains("test.assertEq"));
    assert!(reg.contains("test.assertNotNull"));
    assert!(reg.contains("test.assertNull"));

    // std.builtin
    assert!(reg.contains("builtin.typeof"));
    assert!(reg.contains("builtin.isNull"));
    assert!(reg.contains("builtin.toString"));
    assert!(reg.contains("builtin.toInt"));
    assert!(reg.contains("builtin.toFloat"));

    // std.env
    assert!(reg.contains("env.get"));
    assert!(reg.contains("env.set"));
    assert!(reg.contains("env.has"));
    assert!(reg.contains("env.platform"));
    assert!(reg.contains("env.home"));
    assert!(reg.contains("env.pwd"));

    // std.process
    assert!(reg.contains("process.args"));
    assert!(reg.contains("process.pid"));
    assert!(reg.contains("process.exitCode"));

    // std.random
    assert!(reg.contains("random.nextInt"));
    assert!(reg.contains("random.nextFloat"));
    assert!(reg.contains("random.nextBool"));
    assert!(reg.contains("random.choice"));
    assert!(reg.contains("random.shuffle"));

    // std.encoding
    assert!(reg.contains("encoding.base64Encode"));
    assert!(reg.contains("encoding.base64Decode"));
    assert!(reg.contains("encoding.hexEncode"));
    assert!(reg.contains("encoding.hexDecode"));

    // std.ascii
    assert!(reg.contains("ascii.isAlpha"));
    assert!(reg.contains("ascii.isDigit"));
    assert!(reg.contains("ascii.isUpper"));
    assert!(reg.contains("ascii.isLower"));
    assert!(reg.contains("ascii.toUpper"));
    assert!(reg.contains("ascii.toLower"));

    // std.console
    assert!(reg.contains("console.clear"));
    assert!(reg.contains("console.red"));
    assert!(reg.contains("console.green"));
    assert!(reg.contains("console.size"));

    // std.path
    assert!(reg.contains("path.join"));
    assert!(reg.contains("path.dirname"));
    assert!(reg.contains("path.basename"));
    assert!(reg.contains("path.extname"));
    assert!(reg.contains("path.isAbsolute"));

    // std.assert
    assert!(reg.contains("assert.assert"));
    assert!(reg.contains("assert.assertTrue"));
    assert!(reg.contains("assert.assertFalse"));
    assert!(reg.contains("assert.assertEq"));

    // std.iter
    assert!(reg.contains("iter.sum"));
    assert!(reg.contains("iter.avg"));
    assert!(reg.contains("iter.min"));
    assert!(reg.contains("iter.max"));
    assert!(reg.contains("iter.distinct"));
    assert!(reg.contains("iter.range"));
    assert!(reg.contains("iter.take"));
    assert!(reg.contains("iter.skip"));
    assert!(reg.contains("iter.chain"));
    assert!(reg.contains("iter.flatMap"));

    // std.net
    assert!(reg.contains("net.tcpConnect"));
    assert!(reg.contains("net.tcpListen"));
    assert!(reg.contains("net.getHostname"));
    assert!(reg.contains("net.getLocalIp"));
}

#[test]
fn test_std_math() {
    let reg = NativeRegistry::new();

    // abs
    let result = call(&reg, "math.abs", &[Value::Int(-42)]);
    assert_eq!(result, Value::Int(42));

    // min / max
    let result = call(&reg, "math.min", &[Value::Int(3), Value::Int(5)]);
    assert_eq!(result, Value::Int(3));

    let result = call(&reg, "math.max", &[Value::Int(3), Value::Int(5)]);
    assert_eq!(result, Value::Int(5));

    // ceil / floor
    let result = call(&reg, "math.ceil", &[Value::Float(1.2)]);
    assert_eq!(result, Value::Float(2.0));

    let result = call(&reg, "math.floor", &[Value::Float(1.8)]);
    assert_eq!(result, Value::Float(1.0));

    // sqrt
    let result = call(&reg, "math.sqrt", &[Value::Float(16.0)]);
    assert!((result.as_float() - 4.0).abs() < 1e-10);

    // pow
    let result = call(&reg, "math.pow", &[Value::Float(2.0), Value::Float(3.0)]);
    assert!((result.as_float() - 8.0).abs() < 1e-10);

    // PI
    let pi = call(&reg, "math.PI", &[]);
    assert!((pi.as_float() - 3.141592653589793).abs() < 1e-10);

    // E
    let e = call(&reg, "math.E", &[]);
    assert!((e.as_float() - 2.718281828459045).abs() < 1e-10);
}

#[test]
fn test_std_string() {
    let reg = NativeRegistry::new();

    // contains
    let result = call(&reg, "string.contains", &[Value::str_("hello world"), Value::str_("world")]);
    assert_eq!(result, Value::Bool(true));

    let result = call(&reg, "string.contains", &[Value::str_("hello"), Value::str_("xyz")]);
    assert_eq!(result, Value::Bool(false));

    // startsWith
    let result = call(&reg, "string.startsWith", &[Value::str_("hello"), Value::str_("hel")]);
    assert_eq!(result, Value::Bool(true));

    // endsWith
    let result = call(&reg, "string.endsWith", &[Value::str_("hello"), Value::str_("llo")]);
    assert_eq!(result, Value::Bool(true));

    // split
    let result = call(&reg, "string.split", &[Value::str_("hello world"), Value::str_(" ")]);
    match &result {
        Value::List(items) => {
            assert_eq!(items.len(), 2);
            assert_eq!(items[0], Value::str_("hello"));
            assert_eq!(items[1], Value::str_("world"));
        }
        _ => panic!("expected List"),
    }

    // toUpperCase
    let result = call(&reg, "string.toUpperCase", &[Value::str_("hello")]);
    assert_eq!(result, Value::str_("HELLO"));

    // toLowerCase
    let result = call(&reg, "string.toLowerCase", &[Value::str_("HELLO")]);
    assert_eq!(result, Value::str_("hello"));

    // length
    let result = call(&reg, "string.length", &[Value::str_("hello")]);
    assert_eq!(result, Value::Int(5));

    // trim
    let result = call(&reg, "string.trim", &[Value::str_("  hello  ")]);
    assert_eq!(result, Value::str_("hello"));

    // format
    let result = call(&reg, "string.format", &[
        Value::str_("Hello {0}, you are {1}"),
        Value::str_("World"),
        Value::Int(30),
    ]);
    assert_eq!(result, Value::str_("Hello World, you are 30"));

    // repeat
    let result = call(&reg, "string.repeat", &[Value::Int(3), Value::str_("ab")]);
    assert_eq!(result, Value::str_("ababab"));

    // indexOf
    let result = call(&reg, "string.indexOf", &[Value::str_("hello"), Value::str_("l")]);
    assert_eq!(result, Value::Int(2));
}

#[test]
fn test_std_collections() {
    let reg = NativeRegistry::new();

    // listOf
    let result = call(&reg, "collections.listOf", &[
        Value::Int(1),
        Value::Int(2),
        Value::Int(3),
    ]);
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
    let result = call(&reg, "collections.mapOf", &[
        Value::str_("name"),
        Value::str_("Aura"),
        Value::str_("version"),
        Value::Int(1),
    ]);
    match &result {
        Value::Map(map) => {
            assert_eq!(map.len(), 2);
            assert_eq!(map.get(&Value::str_("name")), Some(&Value::str_("Aura")));
            assert_eq!(map.get(&Value::str_("version")), Some(&Value::Int(1)));
        }
        _ => panic!("expected Map"),
    }

    // setOf (unique)
    let result = call(&reg, "collections.setOf", &[
        Value::Int(1),
        Value::Int(2),
        Value::Int(1),
    ]);
    match &result {
        Value::List(items) => {
            assert_eq!(items.len(), 2);
        }
        _ => panic!("expected List"),
    }

    // emptyList
    let result = call(&reg, "collections.emptyList", &[]);
    match &result {
        Value::List(items) => {
            assert!(items.is_empty());
        }
        _ => panic!("expected List"),
    }
}

#[test]
fn test_std_json() {
    let reg = NativeRegistry::new();

    // parse
    let result = call(&reg, "json.parse", &[Value::str_(r#"{"name": "Aura", "version": 1}"#)]);
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
    let result = call(&reg, "json.stringify", &[map.clone()]);
    let str_result = result.as_string();
    assert!(str_result.contains("\"key\""));
    assert!(str_result.contains("\"value\""));

    // isValid
    let result = call(&reg, "json.isValid", &[Value::str_(r#"{"valid": true}"#)]);
    assert_eq!(result, Value::Bool(true));

    let result = call(&reg, "json.isValid", &[Value::str_("{invalid")]);
    assert_eq!(result, Value::Bool(false));

    // parse array
    let result = call(&reg, "json.parse", &[Value::str_(r#"[1, 2, 3]"#)]);
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
    let reg = NativeRegistry::new();

    // now should return a positive timestamp
    let result = call(&reg, "time.now", &[]);
    let now = result.as_float();
    assert!(now > 0.0);

    // toDateString for epoch
    let result = call(&reg, "time.toDateString", &[Value::Int(0)]);
    assert_eq!(result, Value::str_("1970-01-01"));

    // toTimeString for epoch
    let result = call(&reg, "time.toTimeString", &[Value::Int(0)]);
    assert_eq!(result, Value::str_("00:00:00"));

    // diff
    let result = call(&reg, "time.diff", &[Value::Float(100.0), Value::Float(200.0)]);
    assert!((result.as_float() - 100.0).abs() < 1e-10);
}

#[test]
fn test_std_test() {
    let reg = NativeRegistry::new();

    // assertTrue (pass)
    let result = call(&reg, "test.assertTrue", &[Value::Bool(true), Value::str_("test passed")]);
    assert!(result.as_string().starts_with("PASS"));

    // assertFalse (pass)
    let result = call(&reg, "test.assertFalse", &[Value::Bool(false)]);
    assert!(result.as_string().starts_with("PASS"));

    // assertEq (pass)
    let result = call(&reg, "test.assertEq", &[Value::Int(42), Value::Int(42)]);
    assert!(result.as_string().starts_with("PASS"));

    // assertEq (fail)
    let result = call(&reg, "test.assertEq", &[Value::Int(1), Value::Int(2)]);
    assert!(result.as_string().starts_with("FAIL"));

    // assertNotNull
    let result = call(&reg, "test.assertNotNull", &[Value::Int(42)]);
    assert!(result.as_string().starts_with("PASS"));

    // assertNull
    let result = call(&reg, "test.assertNull", &[Value::Null]);
    assert!(result.as_string().starts_with("PASS"));
}

#[test]
fn test_std_builtin() {
    let reg = NativeRegistry::new();

    // typeof
    let result = call(&reg, "builtin.typeof", &[Value::Int(42)]);
    assert_eq!(result, Value::str_("Int"));

    let result = call(&reg, "builtin.typeof", &[Value::Float(3.14)]);
    assert_eq!(result, Value::str_("Float"));

    let result = call(&reg, "builtin.typeof", &[Value::Bool(true)]);
    assert_eq!(result, Value::str_("Boolean"));

    let result = call(&reg, "builtin.typeof", &[Value::str_("hello")]);
    assert_eq!(result, Value::str_("String"));

    // isNull
    let result = call(&reg, "builtin.isNull", &[Value::Null]);
    assert_eq!(result, Value::Bool(true));

    let result = call(&reg, "builtin.isNull", &[Value::Int(0)]);
    assert_eq!(result, Value::Bool(false));

    // isPositive
    let result = call(&reg, "builtin.isPositive", &[Value::Int(5)]);
    assert_eq!(result, Value::Bool(true));

    let result = call(&reg, "builtin.isPositive", &[Value::Int(-1)]);
    assert_eq!(result, Value::Bool(false));
}

#[test]
fn test_std_env() {
    let reg = NativeRegistry::new();

    // set / get
    call(&reg, "env.set", &[Value::str_("AURA_TEST_VAR"), Value::str_("test_value")]);
    let result = call(&reg, "env.get", &[Value::str_("AURA_TEST_VAR")]);
    assert_eq!(result, Value::str_("test_value"));

    // has
    let result = call(&reg, "env.has", &[Value::str_("AURA_TEST_VAR")]);
    assert_eq!(result, Value::Bool(true));

    // remove
    call(&reg, "env.remove", &[Value::str_("AURA_TEST_VAR")]);
    let result = call(&reg, "env.has", &[Value::str_("AURA_TEST_VAR")]);
    assert_eq!(result, Value::Bool(false));

    // platform
    let result = call(&reg, "env.platform", &[]);
    let platform = result.as_string();
    assert!(!platform.is_empty());

    // home
    let result = call(&reg, "env.home", &[]);
    assert!(result.as_string().len() > 0);
}

#[test]
fn test_std_random() {
    let reg = NativeRegistry::new();

    // nextInt should return different values
    let r1 = call(&reg, "random.nextInt", &[]);
    let r2 = call(&reg, "random.nextInt", &[]);
    // They might theoretically be the same, but very unlikely
    let _ = (r1, r2);

    // nextFloat should be in [0, 1)
    let r = call(&reg, "random.nextFloat", &[]);
    let f = r.as_float();
    assert!(f >= 0.0 && f < 1.0);

    // nextBool
    let _ = call(&reg, "random.nextBool", &[]);

    // nextIntRange
    let r = call(&reg, "random.nextIntRange", &[Value::Int(10), Value::Int(20)]);
    let i = r.as_int();
    assert!(i >= 10 && i < 20);

    // choice
    let items: Vec<Value> = vec![Value::Int(1), Value::Int(2), Value::Int(3)];
    let r = call(&reg, "random.choice", &items);
    assert!(r == Value::Int(1) || r == Value::Int(2) || r == Value::Int(3));
}

#[test]
fn test_std_encoding() {
    let reg = NativeRegistry::new();

    // base64 encode/decode roundtrip
    let original = Value::str_("Hello, Aura!");
    let encoded = call(&reg, "encoding.base64Encode", &[original.clone()]);
    let decoded = call(&reg, "encoding.base64Decode", &[encoded]);
    assert_eq!(decoded, original);

    // hex encode/decode roundtrip
    let original = Value::str_("Hello");
    let encoded = call(&reg, "encoding.hexEncode", &[original.clone()]);
    let decoded = call(&reg, "encoding.hexDecode", &[encoded]);
    assert_eq!(decoded, original);
}

#[test]
fn test_std_ascii() {
    let reg = NativeRegistry::new();

    // isAlpha
    let result = call(&reg, "ascii.isAlpha", &[Value::str_("A")]);
    assert_eq!(result, Value::Bool(true));

    let result = call(&reg, "ascii.isAlpha", &[Value::str_("1")]);
    assert_eq!(result, Value::Bool(false));

    // isDigit
    let result = call(&reg, "ascii.isDigit", &[Value::str_("5")]);
    assert_eq!(result, Value::Bool(true));

    let result = call(&reg, "ascii.isDigit", &[Value::str_("a")]);
    assert_eq!(result, Value::Bool(false));

    // isUpper
    let result = call(&reg, "ascii.isUpper", &[Value::str_("A")]);
    assert_eq!(result, Value::Bool(true));

    let result = call(&reg, "ascii.isUpper", &[Value::str_("a")]);
    assert_eq!(result, Value::Bool(false));

    // isLower
    let result = call(&reg, "ascii.isLower", &[Value::str_("a")]);
    assert_eq!(result, Value::Bool(true));
}

#[test]
fn test_std_path() {
    let reg = NativeRegistry::new();

    // join
    let result = call(&reg, "path.join", &[
        Value::str_("dir"),
        Value::str_("file.txt"),
    ]);
    let joined = result.as_string();
    assert!(joined.contains("dir"));
    assert!(joined.contains("file.txt"));

    // dirname
    let result = call(&reg, "path.dirname", &[Value::str_("/home/user/file.txt")]);
    let dirname = result.as_string();
    assert!(!dirname.is_empty());

    // basename
    let result = call(&reg, "path.basename", &[Value::str_("/home/user/file.txt")]);
    assert_eq!(result, Value::str_("file"));

    // extname
    let result = call(&reg, "path.extname", &[Value::str_("/home/user/file.txt")]);
    assert_eq!(result, Value::str_(".txt"));

    // isAbsolute (use platform-appropriate path)
    #[cfg(windows)]
    let abs_path = "C:\\absolute\\path";
    #[cfg(not(windows))]
    let abs_path = "/absolute/path";
    let result = call(&reg, "path.isAbsolute", &[Value::str_(abs_path)]);
    assert_eq!(result, Value::Bool(true));
}

#[test]
fn test_std_iter() {
    let reg = NativeRegistry::new();

    // sum
    let list = Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
    let result = call(&reg, "iter.sum", &[list.clone()]);
    assert_eq!(result, Value::Int(6));

    // avg
    let result = call(&reg, "iter.avg", &[list.clone()]);
    assert!((result.as_float() - 2.0).abs() < 1e-10);

    // min
    let result = call(&reg, "iter.min", &[list.clone()]);
    assert_eq!(result, Value::Int(1));

    // max
    let result = call(&reg, "iter.max", &[list.clone()]);
    assert_eq!(result, Value::Int(3));

    // distinct
    let list = Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(1), Value::Int(3)]);
    let result = call(&reg, "iter.distinct", &[list.clone()]);
    match &result {
        Value::List(items) => {
            assert_eq!(items.len(), 3);
        }
        _ => panic!("expected List"),
    }

    // range
    let result = call(&reg, "iter.range", &[Value::Int(1), Value::Int(5)]);
    match &result {
        Value::List(items) => {
            assert_eq!(items.len(), 5);
            assert_eq!(items[0], Value::Int(1));
            assert_eq!(items[4], Value::Int(5));
        }
        _ => panic!("expected List"),
    }

    // take
    let list = Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3), Value::Int(4), Value::Int(5)]);
    let result = call(&reg, "iter.take", &[list.clone(), Value::Int(3)]);
    match &result {
        Value::List(items) => {
            assert_eq!(items.len(), 3);
        }
        _ => panic!("expected List"),
    }

    // skip
    let result = call(&reg, "iter.skip", &[list.clone(), Value::Int(2)]);
    match &result {
        Value::List(items) => {
            assert_eq!(items.len(), 3);
            assert_eq!(items[0], Value::Int(3));
        }
        _ => panic!("expected List"),
    }

    // chain
    let l1 = Value::List(vec![Value::Int(1), Value::Int(2)]);
    let l2 = Value::List(vec![Value::Int(3), Value::Int(4)]);
    let result = call(&reg, "iter.chain", &[l1, l2]);
    match &result {
        Value::List(items) => {
            assert_eq!(items.len(), 4);
        }
        _ => panic!("expected List"),
    }

    // count
    let list = Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
    let result = call(&reg, "iter.count", &[list]);
    assert_eq!(result, Value::Int(3));
}

#[test]
fn test_std_assert() {
    let reg = NativeRegistry::new();

    // assert (pass)
    let result = call(&reg, "assert.assert", &[Value::Bool(true), Value::str_("test")]);
    assert!(result.as_string().starts_with("OK"));

    // assert (fail)
    let result = call(&reg, "assert.assert", &[Value::Bool(false), Value::str_("test")]);
    assert!(result.as_string().starts_with("ASSERTION FAILED"));

    // assertEq (pass)
    let result = call(&reg, "assert.assertEq", &[Value::Int(1), Value::Int(1)]);
    assert!(result.as_string().starts_with("OK"));

    // assertEq (fail)
    let result = call(&reg, "assert.assertEq", &[Value::Int(1), Value::Int(2)]);
    assert!(result.as_string().starts_with("ASSERTION FAILED"));
}

#[test]
fn test_std_fs() {
    let reg = NativeRegistry::new();

    // exists (non-existent file)
    let result = call(&reg, "fs.exists", &[Value::str_("/nonexistent/path/file.txt")]);
    assert_eq!(result, Value::Bool(false));

    // isFile / isDirectory for non-existent path
    let result = call(&reg, "fs.isFile", &[Value::str_("/nonexistent")]);
    assert_eq!(result, Value::Bool(false));

    let result = call(&reg, "fs.isDirectory", &[Value::str_("/nonexistent")]);
    assert_eq!(result, Value::Bool(false));
}

#[test]
fn test_std_io_basic() {
    let reg = NativeRegistry::new();

    // fileExists for non-existent file
    let result = call(&reg, "io.fileExists", &[Value::str_("/nonexistent/file.txt")]);
    assert_eq!(result, Value::Bool(false));

    // fileExists for the project root (should exist)
    let result = call(&reg, "io.fileExists", &[Value::str_(".")]);
    assert_eq!(result, Value::Bool(true));
}

#[test]
fn test_std_net_basic() {
    let reg = NativeRegistry::new();

    // getHostname should return non-empty
    let result = call(&reg, "net.getHostname", &[]);
    assert!(result.as_string().len() > 0);

    // getLocalIp should return non-empty
    let result = call(&reg, "net.getLocalIp", &[]);
    assert!(result.as_string().len() > 0);
}

#[test]
fn test_std_process_basic() {
    let reg = NativeRegistry::new();

    // pid should return positive value
    let result = call(&reg, "process.pid", &[]);
    assert!(result.as_int() > 0);

    // args should return non-empty list
    let result = call(&reg, "process.args", &[]);
    match &result {
        Value::List(items) => {
            assert!(!items.is_empty());
        }
        _ => panic!("expected List"),
    }

    // exitCode should return 0
    let result = call(&reg, "process.exitCode", &[]);
    assert_eq!(result, Value::Int(0));
}

#[test]
fn test_value_list_map() {
    // Verify List and Map variants work correctly
    let list = Value::List(vec![Value::Int(1), Value::Int(2)]);
    assert_eq!(list, Value::List(vec![Value::Int(1), Value::Int(2)]));
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
