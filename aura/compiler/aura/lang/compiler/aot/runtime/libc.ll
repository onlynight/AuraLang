; ---- System libc (Phase A: pure LLVM IR) ----
declare i64 @malloc(i64)
declare void @free(i64)
declare i64 @strlen(i8*)
declare i32 @strcmp(i8*, i8*)
declare void @exit(i32)

; _snprintf for inline float→string conversion
declare i32 @_snprintf(i8*, i32, i8*, ...)
@fmt_float_g = private unnamed_addr constant [3 x i8] c"%g\00"
