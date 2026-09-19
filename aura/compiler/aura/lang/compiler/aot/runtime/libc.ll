; ---- System libc (Phase A: pure LLVM IR) ----
; NOTE: the emitter allocates with `call i8* @malloc(i64)`, so the declaration
; must return `i8*` (a mismatch makes llc reject the whole module).
declare i8* @malloc(i64)
declare void @free(i64)
declare i64 @strlen(i8*)
declare i32 @strcmp(i8*, i8*)
declare void @exit(i32)

; Thread operations (runtime bridge — symbol names match aura_syscalls.c)
declare i64 @aura_thread_create(i64, i64)
declare i64 @aura_thread_join(i64)
declare void @aura_thread_sleep(i64)
declare i64 @aura_thread_id()
declare i64 @aura_thread_available_parallelism()

; Thread dispatch table (referenced by aura_syscalls.c thread_dispatch)
@__aura_fn_table = global [0 x i8*] zeroinitializer
@__aura_fn_count = global i64 0

; Channel operations are defined in channel.ll (pure LLVM IR implementation)

; snprintf for inline float→string conversion
; NOTE: UCRT does not export `_snprintf` / `sprintf`; `snprintf` is available.
declare i32 @snprintf(i8*, i64, i8*, ...)
@fmt_float_g = private unnamed_addr constant [3 x i8] c"%g\00"

; String.charAt for index access (called as void by compiler bug — declaration must match)
declare void @charAt(i8*, i32)

; ---- Any → String (Plan A aware) ----
; The emitter calls `@toStr(i8*)` whenever it needs a value rendered as text
; (string concatenation, `println(…)`, collection element printing). Plan A
; encodes a boxed integer as an odd address `(v<<1)|1`, so:
;   * low bit set  → decimal string of (v >> 1)
;   * low bit clear → real C string pointer (passed through)
@fmt_int_d = private unnamed_addr constant [3 x i8] c"%d\00"
@str_null = private unnamed_addr constant [5 x i8] c"null\00"

define i8* @toStr(i8* %v) {
entry:
  %bits = ptrtoint i8* %v to i64
  %low = and i64 %bits, 1
  %isint = icmp ne i64 %low, 0
  br i1 %isint, label %asint, label %asptr
asint:
  %n = ashr i64 %bits, 1
  %n32 = trunc i64 %n to i32
  %buf = call i8* @malloc(i64 32)
  call i32 @snprintf(i8* %buf, i64 32, i8* @fmt_int_d, i32 %n32)
  ret i8* %buf
asptr:
  %isnull = icmp eq i8* %v, null
  br i1 %isnull, label %nullstr, label %passthru
nullstr:
  ret i8* @str_null
passthru:
  ret i8* %v
}

; ---- std IO print（平台无关：只用 libc printf / fflush，不碰任何 OS 专有 IO） ----
; 旧实现直接 `@_write(1, buf, n)`（fd 1 是 POSIX/CRT 专有），在非 Windows 目标上
; 符号不同 → 平台不兼容。这里改用 C 标准库：
;   * `printf`  ：所有平台（MSVC / MinGW / glibc / musl / macOS）都导出；
;   * `fflush(NULL)`：C 标准规定刷新**所有**输出流，避免 `printf` 缓冲在崩溃时丢失。
; 这样 IR 本身与平台无关，链接期由 clang 解析宿主 CRT。
declare i32 @printf(i8*, ...)
declare i32 @fflush(i8*)
@aura_fmt_s = private unnamed_addr constant [3 x i8] c"%s\00"
@aura_fmt_s_nl = private unnamed_addr constant [4 x i8] c"%s\0A\00"
@aura_fmt_nl = private unnamed_addr constant [2 x i8] c"\0A\00"

define void @aura_lang_std_IO_print(i8* %s) {
entry:
  %isnull = icmp eq i8* %s, null
  br i1 %isnull, label %done, label %pr
pr:
  call i32 (i8*, ...) @printf(i8* @aura_fmt_s, i8* %s)
  br label %done
done:
  call i32 @fflush(i8* null)
  ret void
}

define void @aura_lang_std_IO_println(i8* %s) {
entry:
  %isnull = icmp eq i8* %s, null
  br i1 %isnull, label %nl, label %pr
pr:
  call i32 (i8*, ...) @printf(i8* @aura_fmt_s_nl, i8* %s)
  br label %done
nl:
  call i32 (i8*, ...) @printf(i8* @aura_fmt_nl)
  br label %done
done:
  call i32 @fflush(i8* null)
  ret void
}

; ---- typeof / 运行时类型名（平台无关：只依赖 Plan A 低位标记） ----
; AOT 下 `Any` 的表示是 Plan A 的统一句柄：
;   * 奇数 ((v<<1)|1) → 装箱整数
;   * 偶数            → 真实指针（字符串/对象/浮点盒）
; 因此这里给出**保守但可用**的类型名：整数 → "Int"，其余非空 → "String"，空 → "Null"。
; 这足以让 `toStr`/字符串拼接/数值运算的 Int 与 String 分派正确。
;
; **必须 `align 8`**：这三个常量是 `typeof` 的返回值，会被下游当作 `Any` 处理。
; 若按默认 `align 1` 落在**奇数**地址上，Plan A 会把它们误判为「装箱整数」
;（`toStr` 会把地址十进制化，`typeof(v) == "String"` 恒为 false →
; `VmOps.isString/isNumeric` 全部误判、字符串拼接退化成数字加法）。
@aura_ty_int = private unnamed_addr constant [4 x i8] c"Int\00", align 8
@aura_ty_str = private unnamed_addr constant [7 x i8] c"String\00", align 8
@aura_ty_null = private unnamed_addr constant [5 x i8] c"Null\00", align 8

define i8* @aura_typeof(i8* %v) {
entry:
  %isnull = icmp eq i8* %v, null
  br i1 %isnull, label %asnull, label %chk
chk:
  %bits = ptrtoint i8* %v to i64
  %low = and i64 %bits, 1
  %isint = icmp ne i64 %low, 0
  br i1 %isint, label %asint, label %asstr
asnull:
  ret i8* @aura_ty_null
asint:
  ret i8* @aura_ty_int
asstr:
  ret i8* @aura_ty_str
}

; ---- Block copy helper ----
; The Aura emitter emits `call void @aura_block_copy(...)` for inline string
; concatenation / printing. It is a plain function (not an LLVM intrinsic) so
; it must have a body here, otherwise the module fails to link.
define void @aura_block_copy(i8* align 1 %dst, i8* align 1 %src, i64 %n, i1 %isvolatile) {
entry:
  %isempty = icmp sle i64 %n, 0
  br i1 %isempty, label %done, label %loop
loop:
  %i = phi i64 [ 0, %entry ], [ %ni, %loop ]
  %sp = getelementptr i8, i8* %src, i64 %i
  %dp = getelementptr i8, i8* %dst, i64 %i
  %byte = load i8, i8* %sp
  store i8 %byte, i8* %dp
  %ni = add i64 %i, 1
  %again = icmp slt i64 %ni, %n
  br i1 %again, label %loop, label %done
done:
  ret void
}
