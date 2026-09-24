; ---- Type check (pure IR, Phase C) ----
; Type name constants for Plan A parity + strcmp
@tc_int = private unnamed_addr constant [4 x i8] c"Int\00"
@tc_long = private unnamed_addr constant [5 x i8] c"Long\00"
@tc_short = private unnamed_addr constant [6 x i8] c"Short\00"
@tc_byte = private unnamed_addr constant [5 x i8] c"Byte\00"
@tc_char = private unnamed_addr constant [5 x i8] c"Char\00"
@tc_string = private unnamed_addr constant [7 x i8] c"String\00"
@tc_float = private unnamed_addr constant [6 x i8] c"Float\00"
@tc_double = private unnamed_addr constant [7 x i8] c"Double\00"
@tc_bool = private unnamed_addr constant [8 x i8] c"Boolean\00"
@tc_unit = private unnamed_addr constant [5 x i8] c"Unit\00"

; is-of-type check: Plan A odd/even marker + strcmp type name comparison
define i1 @aura_isOfType(i8* %val, i8* %tn) {
  %v64 = ptrtoint i8* %val to i64
  %low = and i64 %v64, 1
  %isInt = icmp ne i64 %low, 0
  br i1 %isInt, label %chkInt, label %chkPtr
chkInt:
  %c1 = call i32 @strcmp(i8* %tn, i8* @tc_int)
  %m1 = icmp eq i32 %c1, 0
  %c2 = call i32 @strcmp(i8* %tn, i8* @tc_long)
  %m2 = icmp eq i32 %c2, 0
  %c3 = call i32 @strcmp(i8* %tn, i8* @tc_short)
  %m3 = icmp eq i32 %c3, 0
  %c4 = call i32 @strcmp(i8* %tn, i8* @tc_byte)
  %m4 = icmp eq i32 %c4, 0
  %c5 = call i32 @strcmp(i8* %tn, i8* @tc_char)
  %m5 = icmp eq i32 %c5, 0
  %r1 = or i1 %m1, %m2
  %r2 = or i1 %r1, %m3
  %r3 = or i1 %r2, %m4
  %r4 = or i1 %r3, %m5
  ret i1 %r4
chkPtr:
  %p1 = call i32 @strcmp(i8* %tn, i8* @tc_string)
  %pm1 = icmp eq i32 %p1, 0
  %p2 = call i32 @strcmp(i8* %tn, i8* @tc_float)
  %pm2 = icmp eq i32 %p2, 0
  %p3 = call i32 @strcmp(i8* %tn, i8* @tc_double)
  %pm3 = icmp eq i32 %p3, 0
  %p4 = call i32 @strcmp(i8* %tn, i8* @tc_bool)
  %pm4 = icmp eq i32 %p4, 0
  %p5 = call i32 @strcmp(i8* %tn, i8* @tc_unit)
  %pm5 = icmp eq i32 %p5, 0
  %pr1 = or i1 %pm1, %pm2
  %pr2 = or i1 %pr1, %pm3
  %pr3 = or i1 %pr2, %pm4
  %pr4 = or i1 %pr3, %pm5
  ret i1 %pr4
}
