; ---- Command line args (pure IR, Phase C) ----
; Global variables for argc/argv (stored by main() entry)
@aura_argc_global = internal global i32 0
@aura_argv_global = internal global i8** null

; Get command-line argument count
define i64 @aura_process_argCount() {
  %argc = load i32, i32* @aura_argc_global
  %argc64 = sext i32 %argc to i64
  ret i64 %argc64
}

; Get all command-line args concatenated with '\n'
define i8* @aura_process_args() {
entry:
  %argc = load i32, i32* @aura_argc_global
  %argc64 = sext i32 %argc to i64
  %argc_zero = icmp sle i64 %argc64, 0
  br i1 %argc_zero, label %empty, label %count_init
empty:
  %ebuf = call i8* @malloc(i64 1)
  store i8 0, i8* %ebuf
  ret i8* %ebuf
count_init:
  br label %count_loop
count_loop:
  %i = phi i64 [ 0, %count_init ], [ %ni, %count_end ]
  %total = phi i64 [ 0, %count_init ], [ %nt2, %count_end ]
  %icmp = icmp slt i64 %i, %argc64
  br i1 %icmp, label %count_body, label %alloc
count_body:
  %argv = load i8**, i8*** @aura_argv_global
  %aent = getelementptr i8*, i8** %argv, i64 %i
  %a = load i8*, i8** %aent
  %alen = call i64 @strlen(i8* %a)
  %nt = add i64 %total, %alen
  %nt2 = add i64 %nt, 1
  %ni = add i64 %i, 1
  br label %count_end
count_end:
  br label %count_loop
alloc:
  %alloc_total = add i64 %total, 1
  %buf = call i8* @malloc(i64 %alloc_total)
  br label %copy_init
copy_init:
  br label %copy_loop
copy_loop:
  %j = phi i64 [ 0, %copy_init ], [ %nj, %copy_end ]
  %bcur = phi i8* [ %buf, %copy_init ], [ %nb2, %copy_end ]
  %jcmp = icmp slt i64 %j, %argc64
  br i1 %jcmp, label %copy_body, label %done
copy_body:
  %argv2 = load i8**, i8*** @aura_argv_global
  %aent2 = getelementptr i8*, i8** %argv2, i64 %j
  %a2 = load i8*, i8** %aent2
  %alen2 = call i64 @strlen(i8* %a2)
  call void @llvm.memcpy.p0.i8.p0.i8(i8* align 1 %bcur, i8* align 1 %a2, i64 %alen2, i1 false)
  %nb = getelementptr i8, i8* %bcur, i64 %alen2
  %nj = add i64 %j, 1
  %is_last = icmp eq i64 %nj, %argc64
  %sep = select i1 %is_last, i64 0, i64 1
  store i8 10, i8* %nb
  %nb2 = getelementptr i8, i8* %nb, i64 %sep
  br label %copy_end
copy_end:
  br label %copy_loop
done:
  store i8 0, i8* %bcur
  ret i8* %buf
}
