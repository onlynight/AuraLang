; ---- Phase D: setjmp/longjmp (pure IR, system libc) ----
declare i32 @setjmp(i8*)
declare void @longjmp(i8*, i32)

; Windows targets: @_setjmp / @_longjmp (UCRT)
declare i32 @_setjmp(i8*)
declare void @_longjmp(i8*, i32)

; Exception value global (written by __throw, read by catch block)
@aura_exception_value = internal global i8* null

; jmp_buf stack (supports nested try/catch; IR-managed, no C source dependency)
@aura_jmp_stack = internal global [64 x i8*] zeroinitializer
@aura_jmp_depth = internal global i32 0

; ---- Phase D.2: LLVM exception handling ----
; personality function: LLVM uses it to dispatch exceptions to landing pads
declare i8* @llvm.eh_personality(i8*)

; personality function declaration (platform-specific, resolved by linker)
declare i32 @__gxx_personality_v0(i32, i8*, i32, i8*, i8*, i8*)
