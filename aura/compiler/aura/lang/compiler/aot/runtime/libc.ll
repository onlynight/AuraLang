; ---- System libc (Phase A: pure LLVM IR) ----
; NOTE: the emitter allocates with `call i8* @malloc(i64)`, so the declaration
; must return `i8*` (a mismatch makes llc reject the whole module).
declare i8* @malloc(i64)
declare void @free(i64)
declare i64 @strlen(i8*)
declare i32 @strcmp(i8*, i8*)
declare void @exit(i32)

; Thread operations (runtime bridge)
declare i64 @aura_thread_create(i64, i64)
declare i64 @aura_thread_join(i64)
declare void @aura_thread_sleep(i64)
declare i64 @aura_thread_currentId()
declare i64 @aura_thread_cores()

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
