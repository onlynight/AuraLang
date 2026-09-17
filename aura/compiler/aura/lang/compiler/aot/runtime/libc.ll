; ---- System libc (Phase A: pure LLVM IR) ----
declare i64 @malloc(i64)
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

; _snprintf for inline float→string conversion
declare i32 @_snprintf(i8*, i32, i8*, ...)
@fmt_float_g = private unnamed_addr constant [3 x i8] c"%g\00"

; String.charAt for index access (called as void by compiler bug — declaration must match)
declare void @charAt(i8*, i32)
