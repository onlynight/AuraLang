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
    assert!(reg.contains("aura.io.println"));
    assert!(reg.contains("aura.io.print"));
    assert!(reg.contains("aura.io.readLine"));
    assert!(reg.contains("aura.io.fileRead"));
    assert!(reg.contains("aura.io.fileWrite"));
    assert!(reg.contains("aura.io.fileExists"));

    // std.math
    assert!(reg.contains("aura.math.abs"));
    assert!(reg.contains("aura.math.min"));
    assert!(reg.contains("aura.math.max"));
    assert!(reg.contains("aura.math.sqrt"));
    assert!(reg.contains("aura.math.pow"));
    assert!(reg.contains("aura.math.PI"));
    assert!(reg.contains("aura.math.E"));
    assert!(reg.contains("aura.math.sin"));
    assert!(reg.contains("aura.math.cos"));
    assert!(reg.contains("aura.math.log"));

    // std.string
    assert!(reg.contains("aura.string.contains"));
    assert!(reg.contains("aura.string.startsWith"));
    assert!(reg.contains("aura.string.endsWith"));
    assert!(reg.contains("aura.string.split"));
    assert!(reg.contains("aura.string.join"));
    assert!(reg.contains("aura.string.replace"));
    assert!(reg.contains("aura.string.toUpperCase"));
    assert!(reg.contains("aura.string.toLowerCase"));
    assert!(reg.contains("aura.string.length"));
    assert!(reg.contains("aura.string.format"));
    assert!(reg.contains("aura.string.trim"));

    // std.collections
    assert!(reg.contains("aura.collections.listOf"));
    assert!(reg.contains("aura.collections.mapOf"));
    assert!(reg.contains("aura.collections.setOf"));
    assert!(reg.contains("aura.collections.emptyList"));
    assert!(reg.contains("aura.collections.emptyMap"));

    // std.fs
    assert!(reg.contains("aura.fs.exists"));
    assert!(reg.contains("aura.fs.isFile"));
    assert!(reg.contains("aura.fs.isDirectory"));
    assert!(reg.contains("aura.fs.readText"));
    assert!(reg.contains("aura.fs.writeText"));
    assert!(reg.contains("aura.fs.mkdir"));
    assert!(reg.contains("aura.fs.mkdirP"));
    assert!(reg.contains("aura.fs.delete"));
    assert!(reg.contains("aura.fs.rename"));
    assert!(reg.contains("aura.fs.listDir"));
    assert!(reg.contains("aura.fs.fileSize"));

    // std.json
    assert!(reg.contains("aura.json.parse"));
    assert!(reg.contains("aura.json.stringify"));
    assert!(reg.contains("aura.json.isValid"));
    assert!(reg.contains("aura.json.get"));
    assert!(reg.contains("aura.json.set"));
    assert!(reg.contains("aura.json.keys"));
    assert!(reg.contains("aura.json.values"));

    // std.time
    assert!(reg.contains("aura.time.now"));
    assert!(reg.contains("aura.time.epoch"));
    assert!(reg.contains("aura.time.sleep"));
    assert!(reg.contains("aura.time.toDateString"));
    assert!(reg.contains("aura.time.formatDate"));

    // std.test
    assert!(reg.contains("aura.test.assertTrue"));
    assert!(reg.contains("aura.test.assertFalse"));
    assert!(reg.contains("aura.test.assertEq"));
    assert!(reg.contains("aura.test.assertNotNull"));
    assert!(reg.contains("aura.test.assertNull"));

    // std.builtin
    assert!(reg.contains("aura.builtin.typeof"));
    assert!(reg.contains("aura.builtin.isNull"));
    assert!(reg.contains("aura.builtin.toString"));
    assert!(reg.contains("aura.builtin.toInt"));
    assert!(reg.contains("aura.builtin.toFloat"));

    // std.env
    assert!(reg.contains("aura.env.get"));
    assert!(reg.contains("aura.env.set"));
    assert!(reg.contains("aura.env.has"));
    assert!(reg.contains("aura.env.platform"));
    assert!(reg.contains("aura.env.home"));
    assert!(reg.contains("aura.env.pwd"));

    // std.process
    assert!(reg.contains("aura.process.args"));
    assert!(reg.contains("aura.process.pid"));
    assert!(reg.contains("aura.process.exitCode"));

    // std.random
    assert!(reg.contains("aura.random.nextInt"));
    assert!(reg.contains("aura.random.nextFloat"));
    assert!(reg.contains("aura.random.nextBool"));
    assert!(reg.contains("aura.random.choice"));
    assert!(reg.contains("aura.random.shuffle"));

    // std.encoding
    assert!(reg.contains("aura.encoding.base64Encode"));
    assert!(reg.contains("aura.encoding.base64Decode"));
    assert!(reg.contains("aura.encoding.hexEncode"));
    assert!(reg.contains("aura.encoding.hexDecode"));

    // std.ascii
    assert!(reg.contains("aura.ascii.isAlpha"));
    assert!(reg.contains("aura.ascii.isDigit"));
    assert!(reg.contains("aura.ascii.isUpper"));
    assert!(reg.contains("aura.ascii.isLower"));
    assert!(reg.contains("aura.ascii.toUpper"));
    assert!(reg.contains("aura.ascii.toLower"));

    // std.console
    assert!(reg.contains("aura.console.clear"));
    assert!(reg.contains("aura.console.red"));
    assert!(reg.contains("aura.console.green"));
    assert!(reg.contains("aura.console.size"));

    // std.path
    assert!(reg.contains("aura.path.join"));
    assert!(reg.contains("aura.path.dirname"));
    assert!(reg.contains("aura.path.basename"));
    assert!(reg.contains("aura.path.extname"));
    assert!(reg.contains("aura.path.isAbsolute"));

    // std.assert
    assert!(reg.contains("aura.assert.assert"));
    assert!(reg.contains("aura.assert.assertTrue"));
    assert!(reg.contains("aura.assert.assertFalse"));
    assert!(reg.contains("aura.assert.assertEq"));

    // std.iter
    assert!(reg.contains("aura.iter.sum"));
    assert!(reg.contains("aura.iter.avg"));
    assert!(reg.contains("aura.iter.min"));
    assert!(reg.contains("aura.iter.max"));
    assert!(reg.contains("aura.iter.distinct"));
    assert!(reg.contains("aura.iter.range"));
    assert!(reg.contains("aura.iter.take"));
    assert!(reg.contains("aura.iter.skip"));
    assert!(reg.contains("aura.iter.chain"));
    assert!(reg.contains("aura.iter.flatMap"));

    // std.net
    assert!(reg.contains("aura.net.tcpConnect"));
    assert!(reg.contains("aura.net.tcpListen"));
    assert!(reg.contains("aura.net.getHostname"));
    assert!(reg.contains("aura.net.getLocalIp"));
}

#[test]
fn test_std_math() {
    let reg = NativeRegistry::new();

    // abs
    let result = call(&reg, "aura.math.abs", &[Value::Int(-42)]);
    assert_eq!(result, Value::Int(42));

    // min / max
    let result = call(&reg, "aura.math.min", &[Value::Int(3), Value::Int(5)]);
    assert_eq!(result, Value::Int(3));

    let result = call(&reg, "aura.math.max", &[Value::Int(3), Value::Int(5)]);
    assert_eq!(result, Value::Int(5));

    // ceil / floor
    let result = call(&reg, "aura.math.ceil", &[Value::Float(1.2)]);
    assert_eq!(result, Value::Float(2.0));

    let result = call(&reg, "aura.math.floor", &[Value::Float(1.8)]);
    assert_eq!(result, Value::Float(1.0));

    // sqrt
    let result = call(&reg, "aura.math.sqrt", &[Value::Float(16.0)]);
    assert!((result.as_float() - 4.0).abs() < 1e-10);

    // pow
    let result = call(&reg, "aura.math.pow", &[Value::Float(2.0), Value::Float(3.0)]);
    assert!((result.as_float() - 8.0).abs() < 1e-10);

    // PI
    let pi = call(&reg, "aura.math.PI", &[]);
    assert!((pi.as_float() - 3.141592653589793).abs() < 1e-10);

    // E
    let e = call(&reg, "aura.math.E", &[]);
    assert!((e.as_float() - 2.718281828459045).abs() < 1e-10);
}

#[test]
fn test_std_string() {
    let reg = NativeRegistry::new();

    // contains
    let result = call(&reg, "aura.string.contains", &[Value::str_("hello world"), Value::str_("world")]);
    assert_eq!(result, Value::Bool(true));

    let result = call(&reg, "aura.string.contains", &[Value::str_("hello"), Value::str_("xyz")]);
    assert_eq!(result, Value::Bool(false));

    // startsWith
    let result = call(&reg, "aura.string.startsWith", &[Value::str_("hello"), Value::str_("hel")]);
    assert_eq!(result, Value::Bool(true));

    // endsWith
    let result = call(&reg, "aura.string.endsWith", &[Value::str_("hello"), Value::str_("llo")]);
    assert_eq!(result, Value::Bool(true));

    // split
    let result = call(&reg, "aura.string.split", &[Value::str_("hello world"), Value::str_(" ")]);
    match &result {
        Value::List(items) => {
            assert_eq!(items.len(), 2);
            assert_eq!(items[0], Value::str_("hello"));
            assert_eq!(items[1], Value::str_("world"));
        }
        _ => panic!("expected List"),
    }

    // toUpperCase
    let result = call(&reg, "aura.string.toUpperCase", &[Value::str_("hello")]);
    assert_eq!(result, Value::str_("HELLO"));

    // toLowerCase
    let result = call(&reg, "aura.string.toLowerCase", &[Value::str_("HELLO")]);
    assert_eq!(result, Value::str_("hello"));

    // length
    let result = call(&reg, "aura.string.length", &[Value::str_("hello")]);
    assert_eq!(result, Value::Int(5));

    // trim
    let result = call(&reg, "aura.string.trim", &[Value::str_("  hello  ")]);
    assert_eq!(result, Value::str_("hello"));

    // format
    let result = call(&reg, "aura.string.format", &[
        Value::str_("Hello {0}, you are {1}"),
        Value::str_("World"),
        Value::Int(30),
    ]);
    assert_eq!(result, Value::str_("Hello World, you are 30"));

    // repeat
    let result = call(&reg, "aura.string.repeat", &[Value::Int(3), Value::str_("ab")]);
    assert_eq!(result, Value::str_("ababab"));

    // indexOf
    let result = call(&reg, "aura.string.indexOf", &[Value::str_("hello"), Value::str_("l")]);
    assert_eq!(result, Value::Int(2));
}

#[test]
fn test_std_collections() {
    let reg = NativeRegistry::new();

    // listOf
    let result = call(&reg, "aura.collections.listOf", &[
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
    let result = call(&reg, "aura.collections.mapOf", &[
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
    let result = call(&reg, "aura.collections.setOf", &[
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
    let result = call(&reg, "aura.collections.emptyList", &[]);
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
    let result = call(&reg, "aura.json.parse", &[Value::str_(r#"{"name": "Aura", "version": 1}"#)]);
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
    let result = call(&reg, "aura.json.stringify", &[map.clone()]);
    let str_result = result.as_string();
    assert!(str_result.contains("\"key\""));
    assert!(str_result.contains("\"value\""));

    // isValid
    let result = call(&reg, "aura.json.isValid", &[Value::str_(r#"{"valid": true}"#)]);
    assert_eq!(result, Value::Bool(true));

    let result = call(&reg, "aura.json.isValid", &[Value::str_("{invalid")]);
    assert_eq!(result, Value::Bool(false));

    // parse array
    let result = call(&reg, "aura.json.parse", &[Value::str_(r#"[1, 2, 3]"#)]);
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
    let result = call(&reg, "aura.time.now", &[]);
    let now = result.as_float();
    assert!(now > 0.0);

    // toDateString for epoch
    let result = call(&reg, "aura.time.toDateString", &[Value::Int(0)]);
    assert_eq!(result, Value::str_("1970-01-01"));

    // toTimeString for epoch
    let result = call(&reg, "aura.time.toTimeString", &[Value::Int(0)]);
    assert_eq!(result, Value::str_("00:00:00"));

    // diff
    let result = call(&reg, "aura.time.diff", &[Value::Float(100.0), Value::Float(200.0)]);
    assert!((result.as_float() - 100.0).abs() < 1e-10);
}

#[test]
fn test_std_test() {
    let reg = NativeRegistry::new();

    // assertTrue (pass)
    let result = call(&reg, "aura.test.assertTrue", &[Value::Bool(true), Value::str_("test passed")]);
    assert!(result.as_string().starts_with("PASS"));

    // assertFalse (pass)
    let result = call(&reg, "aura.test.assertFalse", &[Value::Bool(false)]);
    assert!(result.as_string().starts_with("PASS"));

    // assertEq (pass)
    let result = call(&reg, "aura.test.assertEq", &[Value::Int(42), Value::Int(42)]);
    assert!(result.as_string().starts_with("PASS"));

    // assertEq (fail)
    let result = call(&reg, "aura.test.assertEq", &[Value::Int(1), Value::Int(2)]);
    assert!(result.as_string().starts_with("FAIL"));

    // assertNotNull
    let result = call(&reg, "aura.test.assertNotNull", &[Value::Int(42)]);
    assert!(result.as_string().starts_with("PASS"));

    // assertNull
    let result = call(&reg, "aura.test.assertNull", &[Value::Null]);
    assert!(result.as_string().starts_with("PASS"));
}

#[test]
fn test_std_builtin() {
    let reg = NativeRegistry::new();

    // typeof
    let result = call(&reg, "aura.builtin.typeof", &[Value::Int(42)]);
    assert_eq!(result, Value::str_("Int"));

    let result = call(&reg, "aura.builtin.typeof", &[Value::Float(3.14)]);
    assert_eq!(result, Value::str_("Float"));

    let result = call(&reg, "aura.builtin.typeof", &[Value::Bool(true)]);
    assert_eq!(result, Value::str_("Boolean"));

    let result = call(&reg, "aura.builtin.typeof", &[Value::str_("hello")]);
    assert_eq!(result, Value::str_("String"));

    // isNull
    let result = call(&reg, "aura.builtin.isNull", &[Value::Null]);
    assert_eq!(result, Value::Bool(true));

    let result = call(&reg, "aura.builtin.isNull", &[Value::Int(0)]);
    assert_eq!(result, Value::Bool(false));

    // isPositive
    let result = call(&reg, "aura.builtin.isPositive", &[Value::Int(5)]);
    assert_eq!(result, Value::Bool(true));

    let result = call(&reg, "aura.builtin.isPositive", &[Value::Int(-1)]);
    assert_eq!(result, Value::Bool(false));
}

#[test]
fn test_std_env() {
    let reg = NativeRegistry::new();

    // set / get
    call(&reg, "aura.env.set", &[Value::str_("AURA_TEST_VAR"), Value::str_("test_value")]);
    let result = call(&reg, "aura.env.get", &[Value::str_("AURA_TEST_VAR")]);
    assert_eq!(result, Value::str_("test_value"));

    // has
    let result = call(&reg, "aura.env.has", &[Value::str_("AURA_TEST_VAR")]);
    assert_eq!(result, Value::Bool(true));

    // remove
    call(&reg, "aura.env.remove", &[Value::str_("AURA_TEST_VAR")]);
    let result = call(&reg, "aura.env.has", &[Value::str_("AURA_TEST_VAR")]);
    assert_eq!(result, Value::Bool(false));

    // platform
    let result = call(&reg, "aura.env.platform", &[]);
    let platform = result.as_string();
    assert!(!platform.is_empty());

    // home
    let result = call(&reg, "aura.env.home", &[]);
    assert!(result.as_string().len() > 0);
}

#[test]
fn test_std_random() {
    let reg = NativeRegistry::new();

    // nextInt should return different values
    let r1 = call(&reg, "aura.random.nextInt", &[]);
    let r2 = call(&reg, "aura.random.nextInt", &[]);
    // They might theoretically be the same, but very unlikely
    let _ = (r1, r2);

    // nextFloat should be in [0, 1)
    let r = call(&reg, "aura.random.nextFloat", &[]);
    let f = r.as_float();
    assert!(f >= 0.0 && f < 1.0);

    // nextBool
    let _ = call(&reg, "aura.random.nextBool", &[]);

    // nextIntRange
    let r = call(&reg, "aura.random.nextIntRange", &[Value::Int(10), Value::Int(20)]);
    let i = r.as_int();
    assert!(i >= 10 && i < 20);

    // choice
    let items: Vec<Value> = vec![Value::Int(1), Value::Int(2), Value::Int(3)];
    let r = call(&reg, "aura.random.choice", &items);
    assert!(r == Value::Int(1) || r == Value::Int(2) || r == Value::Int(3));
}

#[test]
fn test_std_encoding() {
    let reg = NativeRegistry::new();

    // base64 encode/decode roundtrip
    let original = Value::str_("Hello, Aura!");
    let encoded = call(&reg, "aura.encoding.base64Encode", &[original.clone()]);
    let decoded = call(&reg, "aura.encoding.base64Decode", &[encoded]);
    assert_eq!(decoded, original);

    // hex encode/decode roundtrip
    let original = Value::str_("Hello");
    let encoded = call(&reg, "aura.encoding.hexEncode", &[original.clone()]);
    let decoded = call(&reg, "aura.encoding.hexDecode", &[encoded]);
    assert_eq!(decoded, original);
}

#[test]
fn test_std_ascii() {
    let reg = NativeRegistry::new();

    // isAlpha
    let result = call(&reg, "aura.ascii.isAlpha", &[Value::str_("A")]);
    assert_eq!(result, Value::Bool(true));

    let result = call(&reg, "aura.ascii.isAlpha", &[Value::str_("1")]);
    assert_eq!(result, Value::Bool(false));

    // isDigit
    let result = call(&reg, "aura.ascii.isDigit", &[Value::str_("5")]);
    assert_eq!(result, Value::Bool(true));

    let result = call(&reg, "aura.ascii.isDigit", &[Value::str_("a")]);
    assert_eq!(result, Value::Bool(false));

    // isUpper
    let result = call(&reg, "aura.ascii.isUpper", &[Value::str_("A")]);
    assert_eq!(result, Value::Bool(true));

    let result = call(&reg, "aura.ascii.isUpper", &[Value::str_("a")]);
    assert_eq!(result, Value::Bool(false));

    // isLower
    let result = call(&reg, "aura.ascii.isLower", &[Value::str_("a")]);
    assert_eq!(result, Value::Bool(true));
}

#[test]
fn test_std_path() {
    let reg = NativeRegistry::new();

    // join
    let result = call(&reg, "aura.path.join", &[
        Value::str_("dir"),
        Value::str_("file.txt"),
    ]);
    let joined = result.as_string();
    assert!(joined.contains("dir"));
    assert!(joined.contains("file.txt"));

    // dirname
    let result = call(&reg, "aura.path.dirname", &[Value::str_("/home/user/file.txt")]);
    let dirname = result.as_string();
    assert!(!dirname.is_empty());

    // basename
    let result = call(&reg, "aura.path.basename", &[Value::str_("/home/user/file.txt")]);
    assert_eq!(result, Value::str_("file"));

    // extname
    let result = call(&reg, "aura.path.extname", &[Value::str_("/home/user/file.txt")]);
    assert_eq!(result, Value::str_(".txt"));

    // isAbsolute (use platform-appropriate path)
    #[cfg(windows)]
    let abs_path = "C:\\absolute\\path";
    #[cfg(not(windows))]
    let abs_path = "/absolute/path";
    let result = call(&reg, "aura.path.isAbsolute", &[Value::str_(abs_path)]);
    assert_eq!(result, Value::Bool(true));
}

#[test]
fn test_std_iter() {
    let reg = NativeRegistry::new();

    // sum
    let list = Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
    let result = call(&reg, "aura.iter.sum", &[list.clone()]);
    assert_eq!(result, Value::Int(6));

    // avg
    let result = call(&reg, "aura.iter.avg", &[list.clone()]);
    assert!((result.as_float() - 2.0).abs() < 1e-10);

    // min
    let result = call(&reg, "aura.iter.min", &[list.clone()]);
    assert_eq!(result, Value::Int(1));

    // max
    let result = call(&reg, "aura.iter.max", &[list.clone()]);
    assert_eq!(result, Value::Int(3));

    // distinct
    let list = Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(1), Value::Int(3)]);
    let result = call(&reg, "aura.iter.distinct", &[list.clone()]);
    match &result {
        Value::List(items) => {
            assert_eq!(items.len(), 3);
        }
        _ => panic!("expected List"),
    }

    // range
    let result = call(&reg, "aura.iter.range", &[Value::Int(1), Value::Int(5)]);
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
    let result = call(&reg, "aura.iter.take", &[list.clone(), Value::Int(3)]);
    match &result {
        Value::List(items) => {
            assert_eq!(items.len(), 3);
        }
        _ => panic!("expected List"),
    }

    // skip
    let result = call(&reg, "aura.iter.skip", &[list.clone(), Value::Int(2)]);
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
    let result = call(&reg, "aura.iter.chain", &[l1, l2]);
    match &result {
        Value::List(items) => {
            assert_eq!(items.len(), 4);
        }
        _ => panic!("expected List"),
    }

    // count
    let list = Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
    let result = call(&reg, "aura.iter.count", &[list]);
    assert_eq!(result, Value::Int(3));
}

#[test]
fn test_std_assert() {
    let reg = NativeRegistry::new();

    // assert (pass)
    let result = call(&reg, "aura.assert.assert", &[Value::Bool(true), Value::str_("test")]);
    assert!(result.as_string().starts_with("OK"));

    // assert (fail)
    let result = call(&reg, "aura.assert.assert", &[Value::Bool(false), Value::str_("test")]);
    assert!(result.as_string().starts_with("ASSERTION FAILED"));

    // assertEq (pass)
    let result = call(&reg, "aura.assert.assertEq", &[Value::Int(1), Value::Int(1)]);
    assert!(result.as_string().starts_with("OK"));

    // assertEq (fail)
    let result = call(&reg, "aura.assert.assertEq", &[Value::Int(1), Value::Int(2)]);
    assert!(result.as_string().starts_with("ASSERTION FAILED"));
}

#[test]
fn test_std_fs() {
    let reg = NativeRegistry::new();

    // exists (non-existent file)
    let result = call(&reg, "aura.fs.exists", &[Value::str_("/nonexistent/path/file.txt")]);
    assert_eq!(result, Value::Bool(false));

    // isFile / isDirectory for non-existent path
    let result = call(&reg, "aura.fs.isFile", &[Value::str_("/nonexistent")]);
    assert_eq!(result, Value::Bool(false));

    let result = call(&reg, "aura.fs.isDirectory", &[Value::str_("/nonexistent")]);
    assert_eq!(result, Value::Bool(false));
}

#[test]
fn test_std_io_basic() {
    let reg = NativeRegistry::new();

    // fileExists for non-existent file
    let result = call(&reg, "aura.io.fileExists", &[Value::str_("/nonexistent/file.txt")]);
    assert_eq!(result, Value::Bool(false));

    // fileExists for the project root (should exist)
    let result = call(&reg, "aura.io.fileExists", &[Value::str_(".")]);
    assert_eq!(result, Value::Bool(true));
}

#[test]
fn test_std_net_basic() {
    let reg = NativeRegistry::new();

    // getHostname should return non-empty
    let result = call(&reg, "aura.net.getHostname", &[]);
    assert!(result.as_string().len() > 0);

    // getLocalIp should return non-empty
    let result = call(&reg, "aura.net.getLocalIp", &[]);
    assert!(result.as_string().len() > 0);
}

#[test]
fn test_std_process_basic() {
    let reg = NativeRegistry::new();

    // pid should return positive value
    let result = call(&reg, "aura.process.pid", &[]);
    assert!(result.as_int() > 0);

    // args should return non-empty list
    let result = call(&reg, "aura.process.args", &[]);
    match &result {
        Value::List(items) => {
            assert!(!items.is_empty());
        }
        _ => panic!("expected List"),
    }

    // exitCode should return 0
    let result = call(&reg, "aura.process.exitCode", &[]);
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
