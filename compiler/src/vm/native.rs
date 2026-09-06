//! 鍘熺敓锛堝唴缃?/ FFI锛夊嚱鏁拌皟搴﹀櫒
//!
//! 瀵瑰簲 鎶€鏈柟妗?搂7.1 鐨?`CallNative` / `CallC` 涓?搂9.3 鐨?FFI 璋冨害銆?
//!
//! 鍘熺敓鍑芥暟绛惧悕缁熶竴涓?`fn(&[Value]) -> Value`锛氬弬鏁板凡浠庢搷浣滄暟鏍堟寜澹版槑椤哄簭寮瑰嚭锛?
//! 杩斿洖鍊煎帇鍥炴搷浣滄暟鏍堛€俙println` 绛夊唴缃嚱鏁扮敱 VM 鍚姩鏃惰嚜鍔ㄦ敞鍐岋紱`extern "c"`
//! 澹版槑鐨勫嚱鏁帮紙濡?`puts`锛夎嫢鏈湪杩愯鏃堕摼鎺ワ紝鍒欏洖閫€涓烘墦鍗板叾鍙傛暟鐨勫崰浣嶅疄鐜帮紝
//! 淇濊瘉瀛楄妭鐮佸彲缁х画鎵ц鑰屼笉宕╂簝銆?

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::vm::dynamic_ffi::DynamicLoader;
use crate::vm::value::Value;

/// 鍘熺敓鍑芥暟鎸囬拡绫诲瀷
pub type NativeFn = fn(&[Value]) -> Value;

/// 鍘熺敓鍑芥暟娉ㄥ唽琛?
#[derive(Default)]
pub struct NativeRegistry {
    fns: HashMap<String, NativeFn>,
    /// 鍔ㄦ€佸姞杞藉櫒锛?.9锛夛細鏈唴缃殑鍘熺敓鍑芥暟浠庡姩鎬佸簱鏌ユ壘
    dynamic: DynamicLoader,
}

impl NativeRegistry {
    /// 鍒涘缓骞舵敞鍐屽叏閮ㄥ唴缃師鐢熷嚱鏁帮紙鍚戝悗鍏煎锛?
    pub fn new() -> Self {
        let mut r = NativeRegistry {
            fns: HashMap::new(),
            dynamic: DynamicLoader::new(),
        };

        // 娉ㄥ唽 prelude锛?7 涓叏灞€鍐呯疆锛屽缁堝瓨鍦級
        r.register("println", native_println);
        r.register("print", native_print);
        r.register("puts", native_puts);
        r.register("abs", native_abs);
        r.register("sqrt", native_sqrt);
        r.register("pow", native_pow);
        r.register("toInt", native_to_int);
        r.register("toFloat", native_to_float);
        r.register("toStr", native_to_str);
        r.register("toString", native_to_str); // alias for toStr, used as method call
        r.register("clock", native_clock);
        r.register("strlen", native_strlen);
        r.register("CString", native_cstring);
        r.register("CStr", native_cstr);
        r.register("ptrIsNull", native_ptr_is_null);
        r.register("ptrToInt", native_ptr_to_int);
        r.register("intToPtr", native_int_to_ptr);
        r.register("makeCallback", native_make_callback);

        // Fix 9: 榛樿浠呭姞杞?prelude锛屼笉鍔犺浇鍏ㄩ儴 std 妯″潡
        // 濡傞渶鍔犺浇 std 妯″潡锛屼娇鐢?NativeRegistry::with_modules()

        // P10: 骞跺彂杩愯鏃讹紙闇€ std-concurrent feature锛?
        #[cfg(feature = "std-concurrent")]
        {
            r.register("aura.concurrent.spawn", native_spawn);
            r.register("aura.concurrent.send", native_send);
            r.register("aura.concurrent.ask", native_ask);
            r.register("aura.concurrent.reply", native_reply);
            r.register("aura.concurrent.newChannel", native_new_channel);
            r.register("aura.concurrent.channelSend", native_channel_send);
            r.register("aura.concurrent.channelRecv", native_channel_recv);
            r.register("aura.concurrent.channelTryRecv", native_channel_try_recv);
            r.register("aura.concurrent.select", native_select);
            r.register("aura.concurrent.selectTimeout", native_select_timeout);
            r.register("aura.concurrent.spawnActor", native_spawn_actor);
            r.register("aura.concurrent.supervise", native_supervise);
            r.register("aura.concurrent.actorAlive", native_actor_alive);

            // Phase 3: 璺ㄨ繘绋?Actor / Channel
            r.register(
                "aura.concurrent.spawnActorProcess",
                native_spawn_actor_process,
            );
            r.register(
                "aura.concurrent.sendProcessActor",
                native_send_process_actor,
            );
            r.register(
                "aura.concurrent.recvProcessActor",
                native_recv_process_actor,
            );
            r.register(
                "aura.concurrent.processActorAlive",
                native_process_actor_alive,
            );
            r.register(
                "aura.concurrent.killProcessActor",
                native_kill_process_actor,
            );
            r.register("aura.concurrent.newTcpChannel", native_new_tcp_channel);
            r.register("aura.concurrent.tcpChannelSend", native_tcp_channel_send);
        }

        r
    }

    /// 鎸夐渶鍒涘缓鍘熺敓鍑芥暟娉ㄥ唽琛紙鍙敞鍐?prelu + 鎸囧畾妯″潡锛?
    ///
    /// `modules` 鏄ā鍧楀悕闆嗗悎锛屽 `["math", "io"]`銆?
    /// 鏈寚瀹氱殑妯″潡涓嶆敞鍐岋紝瀵瑰簲浠ｇ爜涓嶇紪璇戣繘浜岃繘鍒躲€?
    /// 棰刲u锛?7 涓叏灞€鍐呯疆锛夊缁堟敞鍐屻€?
    pub fn with_modules(modules: &[&str]) -> Self {
        let mut r = NativeRegistry {
            fns: HashMap::new(),
            dynamic: DynamicLoader::new(),
        };

        // 娉ㄥ唽 prelude锛?7 涓叏灞€鍐呯疆锛屽缁堝瓨鍦級
        r.register("println", native_println);
        r.register("print", native_print);
        r.register("puts", native_puts);
        r.register("abs", native_abs);
        r.register("sqrt", native_sqrt);
        r.register("pow", native_pow);
        r.register("toInt", native_to_int);
        r.register("toFloat", native_to_float);
        r.register("toStr", native_to_str);
        r.register("toString", native_to_str); // alias for toStr, used as method call
        r.register("clock", native_clock);
        r.register("strlen", native_strlen);
        r.register("CString", native_cstring);
        r.register("CStr", native_cstr);
        r.register("ptrIsNull", native_ptr_is_null);
        r.register("ptrToInt", native_ptr_to_int);
        r.register("intToPtr", native_int_to_ptr);
        r.register("makeCallback", native_make_callback);

        // 鎸夐渶娉ㄥ唽 std 妯″潡
        crate::std::register_with_modules(&mut r, modules);

        // P10: 骞跺彂杩愯鏃讹紙闇€ std-concurrent feature 涓斿鍏?aura.concurrent锛?
        #[cfg(feature = "std-concurrent")]
        if modules.iter().any(|m| *m == "concurrent") {
            r.register("aura.concurrent.spawn", native_spawn);
            r.register("aura.concurrent.send", native_send);
            r.register("aura.concurrent.ask", native_ask);
            r.register("aura.concurrent.reply", native_reply);
            r.register("aura.concurrent.newChannel", native_new_channel);
            r.register("aura.concurrent.channelSend", native_channel_send);
            r.register("aura.concurrent.channelRecv", native_channel_recv);
            r.register("aura.concurrent.channelTryRecv", native_channel_try_recv);
            r.register("aura.concurrent.select", native_select);
            r.register("aura.concurrent.selectTimeout", native_select_timeout);
            r.register("aura.concurrent.spawnActor", native_spawn_actor);
            r.register("aura.concurrent.supervise", native_supervise);
            r.register("aura.concurrent.actorAlive", native_actor_alive);

            // Phase 3: 璺ㄨ繘绋?Actor / Channel
            r.register(
                "aura.concurrent.spawnActorProcess",
                native_spawn_actor_process,
            );
            r.register(
                "aura.concurrent.sendProcessActor",
                native_send_process_actor,
            );
            r.register(
                "aura.concurrent.recvProcessActor",
                native_recv_process_actor,
            );
            r.register(
                "aura.concurrent.processActorAlive",
                native_process_actor_alive,
            );
            r.register(
                "aura.concurrent.killProcessActor",
                native_kill_process_actor,
            );
            r.register("aura.concurrent.newTcpChannel", native_new_tcp_channel);
            r.register("aura.concurrent.tcpChannelSend", native_tcp_channel_send);
        }

        r
    }

    pub fn register(&mut self, name: &str, f: NativeFn) {
        self.fns.insert(name.to_string(), f);
    }

    /// 杩斿洖宸叉敞鍐岀殑鍘熺敓鍑芥暟鏁伴噺
    pub fn len(&self) -> usize {
        self.fns.len()
    }

    /// 鏌ユ壘鍘熺敓鍑芥暟锛堜紭鍏堝唴缃〃锛屽叾娆″姩鎬佸姞杞借〃锛?
    pub fn get(&self, name: &str) -> Option<NativeFn> {
        self.fns.get(name).copied().or_else(|| self.dynamic.get(name))
    }

    /// 妫€鏌ユ槸鍚︽敞鍐屼簡鎸囧畾鐨勫師鐢熷嚱鏁帮紙鍐呯疆鎴栧姩鎬侊級
    pub fn contains(&self, name: &str) -> bool {
        self.fns.contains_key(name) || self.dynamic.contains(name)
    }

    /// 鍔ㄦ€佸姞杞藉簱锛?.9锛?
    pub fn load_library(&mut self, path: &str) -> Result<(), String> {
        self.dynamic.load_lib(path)
    }

    /// 浠庡姩鎬佸姞杞界殑搴撴敞鍐屽嚱鏁?
    pub fn register_dynamic(&mut self, name: &str, f: NativeFn) {
        self.dynamic.register_func(name, f);
    }

    /// 闈欐€侀摼鎺ワ細浠庡綋鍓嶈繘绋嬩腑瑙ｆ瀽 C 鍑芥暟绗﹀彿骞舵敞鍐岋紙P8.4锛?
    ///
    /// 浣跨敤 `dlsym(NULL, name)`锛圲nix锛夋垨 `GetProcAddress`锛圵indows锛夋煡鎵惧嚱鏁般€?
    /// 杩斿洖 C 鍑芥暟鍦板潃锛岀敱璋冪敤鏂圭洿鎺ヨ皟鐢ㄣ€?
    pub fn try_static_link(&self, name: &str) -> Option<usize> {
        use crate::vm::ffi::resolve_static_symbol;
        resolve_static_symbol(name)
    }

    /// 灏濊瘯瑙ｆ瀽 C 鍑芥暟锛氬厛鏌ュ唴缃〃锛屽啀鏌ュ姩鎬佽〃
    pub fn resolve_c_function(&self, name: &str) -> Option<NativeFn> {
        if let Some(f) = self.fns.get(name).copied() {
            return Some(f);
        }
        if let Some(f) = self.dynamic.get(name) {
            return Some(f);
        }
        None
    }

    /// 鑾峰彇鍔ㄦ€佸姞杞藉櫒鐨勫紩鐢?
    pub fn dynamic_loader(&self) -> &DynamicLoader {
        &self.dynamic
    }

    /// 鑾峰彇鍔ㄦ€佸姞杞藉櫒鐨勫彲鍙樺紩鐢?
    pub fn dynamic_loader_mut(&mut self) -> &mut DynamicLoader {
        &mut self.dynamic
    }
}

// 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€
// 鍐呯疆瀹炵幇
// 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€

fn native_println(args: &[Value]) -> Value {
    let mut s = String::new();
    for (i, a) in args.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        s.push_str(&a.to_string());
    }
    println!("{}", s);
    Value::Null
}

fn native_print(args: &[Value]) -> Value {
    let mut s = String::new();
    for a in args {
        s.push_str(&a.to_string());
    }
    print!("{}", s);
    // 纭繚鍗虫椂鍒锋柊锛堟棤鎹㈣鏃讹級
    use std::io::Write;
    let _ = std::io::stdout().flush();
    Value::Null
}

fn native_puts(args: &[Value]) -> Value {
    // C 椋庢牸 puts锛氳緭鍑哄苟鎹㈣
    if let Some(a) = args.first() {
        println!("{}", a);
    } else {
        println!();
    }
    Value::Int(0)
}

fn native_abs(args: &[Value]) -> Value {
    match args.first() {
        Some(Value::Int(i)) => Value::Int(i.abs()),
        Some(Value::Float(f)) => Value::Float(f.abs()),
        _ => Value::Int(0),
    }
}

fn native_sqrt(args: &[Value]) -> Value {
    match args.first() {
        Some(v) => Value::Float(v.as_float().sqrt()),
        None => Value::Float(0.0),
    }
}

fn native_pow(args: &[Value]) -> Value {
    let base = args.first().map(|v| v.as_float()).unwrap_or(0.0);
    let exp = args.get(1).map(|v| v.as_float()).unwrap_or(0.0);
    Value::Float(base.powf(exp))
}

fn native_to_int(args: &[Value]) -> Value {
    match args.first() {
        Some(Value::Bool(b)) => Value::Int(*b as i64),
        Some(v) => Value::Int(v.as_int()),
        None => Value::Int(0),
    }
}

fn native_to_float(args: &[Value]) -> Value {
    match args.first() {
        Some(v) => Value::Float(v.as_float()),
        None => Value::Float(0.0),
    }
}

fn native_to_str(args: &[Value]) -> Value {
    match args.first() {
        Some(v) => Value::str_(v.to_string()),
        None => Value::str_(""),
    }
}

fn native_clock(args: &[Value]) -> Value {
    let _ = args;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
    Value::Float(now)
}

fn native_strlen(args: &[Value]) -> Value {
    match args.first() {
        Some(Value::Str(s)) => Value::Int(s.chars().count() as i64),
        _ => Value::Int(0),
    }
}

// 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€
// P8 FFI 鍐呯疆鍑芥暟
// 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€

/// CString(str) 鈫?Ptr锛氬皢 Aura 瀛楃涓茶浆鎹负 C 瀛楃涓叉寚閽堬紙P8.5锛?
///
/// 瀹為檯瀹炵幇锛氶€氳繃 `CString` 鎸囦护瀹屾垚杞崲锛屾澶勪綔涓哄崰浣嶈繑鍥?Ptr(0)銆?
/// 瀹屾暣瀹炵幇闇€ VM 绔敮鎸侊紙瑙?interp.rs CString 鎸囦护锛夈€?
fn native_cstring(args: &[Value]) -> Value {
    match args.first() {
        Some(Value::Str(_s)) => {
            // 杩斿洖涓€涓潪绌烘寚閽堝崰浣嶏紙瀹為檯 C 瀛楃涓茬敱 CString 鎸囦护鍒嗛厤锛?
            Value::Ptr(1)
        }
        _ => Value::Ptr(0),
    }
}

/// CStr(str) 鈫?Ptr锛欳String 鐨勫埆鍚嶏紙P8.5锛?
fn native_cstr(args: &[Value]) -> Value {
    native_cstring(args)
}

/// ptrIsNull(ptr) 鈫?Bool锛氭鏌ユ寚閽堟槸鍚︿负 nullptr锛圥8.6锛?
fn native_ptr_is_null(args: &[Value]) -> Value {
    match args.first() {
        Some(v) => Value::Bool(v.is_null_ptr()),
        _ => Value::Bool(true),
    }
}

/// ptrToInt(ptr) 鈫?Int锛氬皢鎸囬拡杞崲涓烘暣鏁板湴鍧€锛圥8.6锛?
fn native_ptr_to_int(args: &[Value]) -> Value {
    match args.first() {
        Some(v) => Value::Int(v.as_ptr()),
        _ => Value::Int(0),
    }
}

/// intToPtr(n) 鈫?Ptr锛氬皢鏁存暟鍦板潃杞崲涓烘寚閽堬紙P8.6锛?
fn native_int_to_ptr(args: &[Value]) -> Value {
    match args.first() {
        Some(v) => Value::Ptr(v.as_int()),
        _ => Value::Ptr(0),
    }
}

/// makeCallback(funcName) 鈫?Ptr锛氬垱寤?C 鍥炶皟韫﹀簥锛圥8.7锛?
///
/// 杩斿洖鐨?Ptr 鍖呭惈鍥炶皟 ID锛孋 浠ｇ爜灏嗗叾浣滀负鍑芥暟鎸囬拡璋冪敤鏃讹紝
/// 韫﹀簥閫氳繃 thread-local 娲惧彂鍥?Aura VM 鎵ц瀵瑰簲鍑芥暟銆?
fn native_make_callback(_args: &[Value]) -> Value {
    // makeCallback 鐢?MakeCallback 鎸囦护澶勭悊锛堣 mir.rs / emit.rs锛?
    // 姝ゅ浣滀负鍗犱綅锛氬鏋滈€氳繃 CallNative 璋冪敤锛岃繑鍥炴棤鏁堝洖璋?ID
    Value::Ptr(0)
}

// 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€
// P10 骞跺彂杩愯鏃?鈥?鍘熺敓鍑芥暟锛圓ctor / Channel / Select / Spawn锛?
// 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€
//
// 鍘熺敓鍑芥暟閫氳繃 thread-local 鎸囬拡璁块棶 VM 瀹炰緥鐨?Actor/Channel 杩愯鏃剁姸鎬併€?
// VM 鍦?`run()` 寮€濮嬫椂璁剧疆姝ゆ寚閽堬紝缁撴潫鏃舵竻闄ゃ€?

use std::sync::Mutex;

/// 鍏ㄥ眬 VM 瀹炰緥鏍堬紙Phase 1: 鏇夸唬 thread_local锛?
///
/// 浣跨敤鏍堝紡缁撴瀯鏀寔宓屽 VM 鎵ц锛?
/// - `set_vm_ref` 鍘嬫爤
/// - `clear_vm_ref` 寮规爤锛堜粎褰撴爤椤跺尮閰嶆椂锛?
/// - `get_vm_ref` 杩斿洖鏍堥《锛堝綋鍓嶆椿璺?VM锛?
///
/// 浣跨敤 `usize` 瀛樺偍瑁稿湴鍧€锛屽洜涓?`*mut ()` 涓嶆槸 `Send + Sync`銆?
static VM_STACK: Mutex<Vec<usize>> = Mutex::new(Vec::new());

/// 璁剧疆褰撳墠 VM 瀹炰緥锛堝湪 `Vm::run()` 寮€濮嬫椂璋冪敤锛屽帇鏍堬級
pub fn set_vm_ref(vm: *mut ()) {
    VM_STACK.lock().unwrap().push(vm as usize);
}

/// 娓呴櫎褰撳墠 VM 瀹炰緥寮曠敤锛堝湪 `Vm::run()` 缁撴潫鏃惰皟鐢紝寮规爤锛?
pub fn clear_vm_ref() {
    VM_STACK.lock().unwrap().pop();
}

/// 鑾峰彇褰撳墠 VM 瀹炰緥鎸囬拡锛堟爤椤讹級
fn get_vm_ref() -> Option<*mut crate::vm::Vm> {
    VM_STACK.lock().unwrap().last().copied().map(|addr| addr as *mut crate::vm::Vm)
}

/// spawn(expr) 鈫?Int锛氬垱寤烘柊鍗忕▼锛圥10.1锛?
///
/// 灏嗚〃杈惧紡浣滀负鍗忕▼鍏ュ彛锛屽垱寤烘柊鍗忕▼骞惰繑鍥炲崗绋?ID銆?
/// spawn(...) 鈫?Int锛氬惎鍔ㄥ崗绋嬶紙P10锛?
#[cfg(feature = "std-concurrent")]
fn native_spawn(args: &[Value]) -> Value {
    // spawn 鐨勫疄闄呭垱寤虹敱 VM 鍗忕▼璋冨害鍣ㄥ鐞?
    // 姝ゅ杩斿洖鍗犱綅 ID锛? = 涓荤嚎绋嬶級
    match args.first() {
        Some(v) => Value::Int(v.as_int()),
        None => Value::Int(0),
    }
}

/// send(actorId, msg) 鈫?Unit锛氬悜 Actor 鍙戦€佹秷鎭紙P10.6锛?
#[cfg(feature = "std-concurrent")]
fn native_send(args: &[Value]) -> Value {
    if args.len() >= 2 {
        let actor_id = args[0].as_int() as usize;
        let msg = args[1].clone();
        if let Some(vm_ptr) = get_vm_ref() {
            unsafe {
                (*vm_ptr).actors.send(actor_id, msg);
            }
        }
    }
    Value::Null
}

/// ask(actorId, msg) 鈫?Any锛氬悜 Actor 璇锋眰鍝嶅簲锛圥10.6锛?
#[cfg(feature = "std-concurrent")]
fn native_ask(args: &[Value]) -> Value {
    if args.len() >= 2 {
        let actor_id = args[0].as_int() as usize;
        let msg = args[1].clone();
        if let Some(vm_ptr) = get_vm_ref() {
            unsafe {
                return (*vm_ptr).actors.ask(actor_id, msg);
            }
        }
    }
    Value::Null
}

/// reply(requestId, response) 鈫?Unit锛氬洖澶?Actor 璇锋眰锛圥hase 4锛?
#[cfg(feature = "std-concurrent")]
fn native_reply(args: &[Value]) -> Value {
    if args.len() >= 2 {
        let request_id = args[0].as_int() as u64;
        let response = args[1].clone();
        if let Some(vm_ptr) = get_vm_ref() {
            unsafe {
                (*vm_ptr).actors.reply(request_id, response);
            }
        }
    }
    Value::Null
}

/// newChannel(bound) 鈫?Int锛氬垱寤?Channel锛圥10.8锛?
///
/// `bound`: 瀹归噺涓婇檺锛? 琛ㄧず鏃犵晫
#[cfg(feature = "std-concurrent")]
fn native_new_channel(args: &[Value]) -> Value {
    let bound = args.first().map(|v| v.as_int() as usize).unwrap_or(0);
    if let Some(vm_ptr) = get_vm_ref() {
        unsafe {
            let id = (*vm_ptr).channels.new_channel(bound);
            return Value::Int(id as i64);
        }
    }
    Value::Int(0)
}

/// channelSend(ch, val) 鈫?Unit锛氬悜 Channel 鍙戦€佸€硷紙P10.8锛?
#[cfg(feature = "std-concurrent")]
fn native_channel_send(args: &[Value]) -> Value {
    if args.len() >= 2 {
        let ch_id = args[0].as_int() as usize;
        let val = args[1].clone();
        if let Some(vm_ptr) = get_vm_ref() {
            unsafe {
                (*vm_ptr).channels.send(ch_id, val);
            }
        }
    }
    Value::Null
}

/// channelRecv(ch) 鈫?Any锛氫粠 Channel 鎺ユ敹鍊硷紙闃诲璇箟锛孭10.8锛?
#[cfg(feature = "std-concurrent")]
fn native_channel_recv(args: &[Value]) -> Value {
    if args.len() >= 1 {
        let ch_id = args[0].as_int() as usize;
        if let Some(vm_ptr) = get_vm_ref() {
            unsafe {
                return (*vm_ptr).channels.recv(ch_id);
            }
        }
    }
    Value::Null
}

/// channelTryRecv(ch) 鈫?Any锛氬皾璇曚粠 Channel 鎺ユ敹鍊硷紙闈為樆濉烇紝P10.8锛?
#[cfg(feature = "std-concurrent")]
fn native_channel_try_recv(args: &[Value]) -> Value {
    if args.len() >= 1 {
        let ch_id = args[0].as_int() as usize;
        if let Some(vm_ptr) = get_vm_ref() {
            unsafe {
                return (*vm_ptr).channels.try_recv(ch_id);
            }
        }
    }
    Value::Null
}

/// __select(ch1, ch2, ...) 鈫?Any锛歴elect 澶氳矾澶嶇敤锛圥10.9锛?
///
/// 闃舵 4.11: 鍩轰簬鍗忕▼鎸傝捣鐨勯潪杞瀹炵幇
///
/// 妫€鏌ユ墍鏈夐€氶亾锛岃繑鍥炵涓€涓湁鍊肩殑閫氶亾鐨勫€笺€?
/// 鑻ユ墍鏈夐€氶亾鍧囦负绌猴紝杩斿洖 `Null`锛堥潪闃诲妯″紡锛夈€?
/// 杩斿洖鍊兼牸寮忥細`[channel_id, value]` 鎴?`Null`
#[cfg(feature = "std-concurrent")]
fn native_select(args: &[Value]) -> Value {
    if let Some(vm_ptr) = get_vm_ref() {
        unsafe {
            let vm = &mut *vm_ptr;
            // 閬嶅巻鎵€鏈夐€氶亾锛屾壘鍒扮涓€涓湁鍊肩殑
            for arg in args {
                let ch_id = arg.as_int() as usize;
                if ch_id == 0 {
                    continue;
                }
                if !vm.channels.is_empty(ch_id) {
                    let val = vm.channels.recv(ch_id);
                    // 杩斿洖 [channel_id, value]
                    return Value::List(vec![
                        Value::Int(ch_id as i64),
                        val,
                    ]);
                }
            }
        }
    }
    // 鎵€鏈夐€氶亾鍧囦负绌猴紝杩斿洖 Null锛堥潪闃诲锛?
    Value::Null
}

/// __selectTimeout(ch1, ch2, ..., timeoutMs) 鈫?Any锛氬甫瓒呮椂鐨?select锛堥樁娈?4.11锛?
///
/// 鍦?`timeoutMs` 姣鍐呯瓑寰呴€氶亾娑堟伅锛岃秴鏃惰繑鍥?`Null`銆?
#[cfg(feature = "std-concurrent")]
fn native_select_timeout(args: &[Value]) -> Value {
    if args.is_empty() {
        return Value::Null;
    }

    // 鏈€鍚庝竴涓弬鏁版槸瓒呮椂鏃堕棿锛堟绉掞級
    let timeout_arg = args.last().unwrap().clone();
    let timeout_ms = timeout_arg.as_int();
    let channels = &args[..args.len() - 1];

    if let Some(vm_ptr) = get_vm_ref() {
        unsafe {
            let vm = &mut *vm_ptr;
            let start = std::time::Instant::now();
            let poll_interval = std::time::Duration::from_millis(1);

            loop {
                // 閬嶅巻鎵€鏈夐€氶亾锛屾壘鍒扮涓€涓湁鍊肩殑
                for arg in channels {
                    let ch_id = arg.as_int() as usize;
                    if ch_id == 0 {
                        continue;
                    }
                    if !vm.channels.is_empty(ch_id) {
                        let val = vm.channels.recv(ch_id);
                        // 杩斿洖 [channel_id, value]
                        return Value::List(vec![
                            Value::Int(ch_id as i64),
                            val,
                        ]);
                    }
                }

                // 妫€鏌ヨ秴鏃?
                if start.elapsed() >= std::time::Duration::from_millis(timeout_ms as u64) {
                    return Value::Null;
                }

                // 浼戠湢鍚庨噸璇曪紙璁╁嚭 CPU锛?
                std::thread::sleep(poll_interval);
            }
        }
    }
    Value::Null
}

/// __spawnActor(name) 鈫?Int锛氬垱寤?Actor 瀹炰緥锛圥10.4锛?
#[cfg(feature = "std-concurrent")]
fn native_spawn_actor(args: &[Value]) -> Value {
    let name = args.first().map(|v| v.to_string()).unwrap_or_else(|| "unnamed".to_string());
    if let Some(vm_ptr) = get_vm_ref() {
        unsafe {
            let id = (*vm_ptr).actors.spawn(&name);
            return Value::Int(id as i64);
        }
    }
    Value::Int(0)
}

/// __supervise(parent, child) 鈫?Unit锛氬缓绔嬬洃鐫ｅ叧绯伙紙P10.7锛?
#[cfg(feature = "std-concurrent")]
fn native_supervise(args: &[Value]) -> Value {
    if args.len() >= 2 {
        let parent_id = args[0].as_int() as usize;
        let child_id = args[1].as_int() as usize;
        if let Some(vm_ptr) = get_vm_ref() {
            unsafe {
                (*vm_ptr).actors.supervise(parent_id, child_id);
            }
        }
    }
    Value::Null
}

/// __actorAlive(id) 鈫?Boolean锛氭鏌?Actor 鏄惁瀛樻椿锛圥10.7锛?
#[cfg(feature = "std-concurrent")]
fn native_actor_alive(args: &[Value]) -> Value {
    if args.len() >= 1 {
        let id = args[0].as_int() as usize;
        if let Some(vm_ptr) = get_vm_ref() {
            unsafe {
                return Value::Bool((*vm_ptr).actors.is_alive(id));
            }
        }
    }
    Value::Bool(false)
}

// 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€
// Phase 3: 璺ㄨ繘绋?Actor / Channel 鍘熺敓鍑芥暟
// 鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€鈹€

/// __spawnActorProcess(entry, name) 鈫?Map: 璺ㄨ繘绋嬪垱寤?Actor锛圥hase 3锛?
///
/// 鍦ㄥ瓙杩涚▼涓惎鍔ㄤ竴涓柊鐨?Actor 瀹炰緥锛岃繑鍥炶繛鎺ヤ俊鎭€?
/// 褰撳墠涓洪鏋跺疄鐜帮細杩斿洖绔彛鍜屽悕绉般€?
#[cfg(feature = "std-concurrent")]
fn native_spawn_actor_process(args: &[Value]) -> Value {
    use std::collections::HashMap;
    let entry = args.first().map(|v| v.as_string()).unwrap_or_default();
    let name = args.get(1).map(|v| v.as_string()).unwrap_or_else(|| "unnamed".to_string());

    if entry.is_empty() {
        return Value::Null;
    }

    match crate::vm::actor_process::ProcessActor::spawn(&entry, &name) {
        Ok((actor, port)) => {
            // 灏?actor 瀛樺叆鍏ㄥ眬娉ㄥ唽琛?
            let mut registry = crate::vm::actor_process::PROCESS_ACTORS.lock().unwrap();
            let id = registry.len() + 1;
            registry.insert(id, actor);
            drop(registry);

            let mut map = HashMap::new();
            map.insert(Value::str_("id"), Value::Int(id as i64));
            map.insert(Value::str_("port"), Value::Int(port as i64));
            map.insert(Value::str_("name"), Value::str_(name));
            map.insert(Value::str_("alive"), Value::Bool(true));
            Value::Map(map)
        }
        Err(e) => {
            eprintln!("[Phase 3] 璺ㄨ繘绋?Actor 鍚姩澶辫触: {}", e);
            Value::Null
        }
    }
}

/// __sendProcessActor(id, msg) 鈫?Unit: 鍚戣法杩涚▼ Actor 鍙戦€佹秷鎭紙Phase 3锛?
#[cfg(feature = "std-concurrent")]
fn native_send_process_actor(args: &[Value]) -> Value {
    if args.len() >= 2 {
        let actor_id = args[0].as_int() as usize;
        let msg = args[1].clone();
        let mut registry = crate::vm::actor_process::PROCESS_ACTORS.lock().unwrap();
        if let Some(actor) = registry.get_mut(&actor_id) {
            if let Err(e) = actor.send(&msg) {
                eprintln!("[Phase 3] 璺ㄨ繘绋嬪彂閫佸け璐? {}", e);
            }
        }
    }
    Value::Null
}

/// __recvProcessActor(id) 鈫?Any: 浠庤法杩涚▼ Actor 鎺ユ敹鍝嶅簲锛圥hase 3锛?
#[cfg(feature = "std-concurrent")]
fn native_recv_process_actor(args: &[Value]) -> Value {
    if args.len() >= 1 {
        let actor_id = args[0].as_int() as usize;
        let mut registry = crate::vm::actor_process::PROCESS_ACTORS.lock().unwrap();
        if let Some(actor) = registry.get_mut(&actor_id) {
            match actor.recv() {
                Ok(Some(val)) => return val,
                Ok(None) => return Value::Null,
                Err(e) => {
                    eprintln!("[Phase 3] 璺ㄨ繘绋嬫帴鏀跺け璐? {}", e);
                    return Value::Null;
                }
            }
        }
    }
    Value::Null
}

/// __processActorAlive(id) 鈫?Boolean: 妫€鏌ヨ法杩涚▼ Actor 鏄惁瀛樻椿锛圥hase 3锛?
#[cfg(feature = "std-concurrent")]
fn native_process_actor_alive(args: &[Value]) -> Value {
    if args.len() >= 1 {
        let actor_id = args[0].as_int() as usize;
        let mut registry = crate::vm::actor_process::PROCESS_ACTORS.lock().unwrap();
        if let Some(actor) = registry.get_mut(&actor_id) {
            return Value::Bool(actor.is_alive());
        }
    }
    Value::Bool(false)
}

/// __killProcessActor(id) 鈫?Unit: 鍏抽棴璺ㄨ繘绋?Actor锛圥hase 3锛?
#[cfg(feature = "std-concurrent")]
fn native_kill_process_actor(args: &[Value]) -> Value {
    if args.len() >= 1 {
        let actor_id = args[0].as_int() as usize;
        crate::vm::actor_process::PROCESS_ACTORS.lock().unwrap().remove(&actor_id);
    }
    Value::Null
}

/// __newTcpChannel(port) 鈫?Int: 鍒涘缓璺ㄨ繘绋?Channel锛圥hase 3锛?
///
/// `port=0` 鏃惰嚜鍔ㄥ垎閰嶇鍙ｏ紝杩斿洖绔彛鍙枫€?
#[cfg(feature = "std-concurrent")]
fn native_new_tcp_channel(args: &[Value]) -> Value {
    let port = args.first().map(|v| v.as_int() as u16).unwrap_or(0);
    let result = if port == 0 {
        crate::vm::channel_tcp::TcpChannelServer::new_any()
    } else {
        crate::vm::channel_tcp::TcpChannelServer::new(port).map(|s| (s, port))
    };
    match result {
        Ok((server, actual_port)) => {
            let mut registry = crate::vm::channel_tcp::TCP_CHANNELS.lock().unwrap();
            let id = registry.len() + 1;
            registry.insert(id, server);
            drop(registry);
            Value::Int(id as i64)
        }
        Err(e) => {
            eprintln!("[Phase 3] TCP Channel 鍒涘缓澶辫触: {}", e);
            Value::Int(0)
        }
    }
}

/// __tcpChannelSend(id, val) 鈫?Unit: 鍚戣法杩涚▼ Channel 鍙戦€侊紙Phase 3锛?
#[cfg(feature = "std-concurrent")]
fn native_tcp_channel_send(args: &[Value]) -> Value {
    if args.len() >= 2 {
        let ch_id = args[0].as_int() as usize;
        let val = args[1].clone();
        let mut registry = crate::vm::channel_tcp::TCP_CHANNELS.lock().unwrap();
        if let Some(server) = registry.get_mut(&ch_id) {
            // 娉細鏈嶅姟绔渶瑕佽繛鎺ユ墠鑳藉彂閫侊紝褰撳墠绠€鍖栦负鐩存帴鍙戦€?
            // 瀹為檯浣跨敤闇€閫氳繃瀹㈡埛绔繛鎺ュ彂閫?
            eprintln!("[Phase 3] TCP Channel 鍙戦€佹殏涓嶆敮鎸侊紙闇€瀹㈡埛绔繛鎺ワ級");
        }
    }
    Value::Null
}
