; ---- Channel runtime (pure LLVM IR) ----
; Global channel table: channels are indexed by Int (32-bit).
; Each channel is a struct: { i8* buf, i64 cap, i64 head, i64 tail }
; Struct size: 32 bytes.

declare i8* @realloc(i8*, i64)

; Global channel table (max 256 channels)
@__channel_table = global [256 x i8*] zeroinitializer
@__channel_next = global i32 1

define i32 @aura_lang_concurrent_Channel_newChannel(i64 %cap) {
entry:
  %sz = add i64 32, 0
  %p = call i8* @malloc(i64 %sz)
  %cmp = icmp ult i64 %cap, 1
  br i1 %cmp, label %use_min, label %use_cap
use_min:
  %realcap = add i64 1, 0
  br label %after
use_cap:
  %realcap2 = add i64 %cap, 0
  br label %after
after:
  %capfinal = phi i64 [ %realcap, %use_min ], [ %realcap2, %use_cap ]
  %bs = mul i64 %capfinal, 8
  %b = call i8* @malloc(i64 %bs)
  store i8* %b, i8** %p
  %c8 = getelementptr i8, i8* %p, i64 8
  store i64 %capfinal, i64* %c8
  %c16 = getelementptr i8, i8* %p, i64 16
  store i64 0, i64* %c16
  %c24 = getelementptr i8, i8* %p, i64 24
  store i64 0, i64* %c24
  ; Allocate a slot from the global table
  %nxt = load i32, i32* @__channel_next
  %newnxt = add i32 %nxt, 1
  store i32 %newnxt, i32* @__channel_next
  %id = sub i32 %nxt, 1
  %tblptr = getelementptr [256 x i8*], [256 x i8*]* @__channel_table, i32 0, i32 %id
  store i8* %p, i8* %tblptr
  ret i32 %id
}

define void @aura_lang_concurrent_Channel_channelSend(i64 %ch, i8* %val) {
entry:
  %id = trunc i64 %ch to i32
  %tblptr = getelementptr [256 x i8*], [256 x i8*]* @__channel_table, i32 0, i32 %id
  %p = load i8*, i8* %tblptr
  %bp = load i8*, i8** %p
  %c8 = getelementptr i8, i8* %p, i64 8
  %cap = load i64, i64* %c8
  %c16 = getelementptr i8, i8* %p, i64 16
  %head = load i64, i64* %c16
  %c24 = getelementptr i8, i8* %p, i64 24
  %tail = load i64, i64* %c24
  %used = sub i64 %tail, %head
  %ge = icmp uge i64 %used, %cap
  br i1 %ge, label %grow, label %nogrow
grow:
  %newcap = mul i64 %cap, 2
  %newsz = mul i64 %newcap, 8
  %newbuf = call i8* @realloc(i8* %bp, i64 %newsz)
  store i8* %newbuf, i8** %p
  store i64 %newcap, i64* %c8
  %np = getelementptr i8*, i8* %newbuf, i64 %tail
  store i8* %val, i8* %np
  %nt = add i64 %tail, 1
  store i64 %nt, i64* %c24
  br label %done
nogrow:
  %np2 = getelementptr i8*, i8* %bp, i64 %tail
  store i8* %val, i8* %np2
  %nt2 = add i64 %tail, 1
  store i64 %nt2, i64* %c24
  br label %done
done:
  ret void
}

define i8* @aura_lang_concurrent_Channel_channelRecv(i64 %ch) {
entry:
  %id = trunc i64 %ch to i32
  %tblptr = getelementptr [256 x i8*], [256 x i8*]* @__channel_table, i32 0, i32 %id
  %p = load i8*, i8* %tblptr
  %bp = load i8*, i8** %p
  %c16 = getelementptr i8, i8* %p, i64 16
  %head = load i64, i64* %c16
  %c24 = getelementptr i8, i8* %p, i64 24
  %tail = load i64, i64* %c24
  %e = icmp uge i64 %head, %tail
  br i1 %e, label %er, label %ne
er:
  ret i8* null
ne:
  %slot = getelementptr i8*, i8* %bp, i64 %head
  %v = load i8*, i8* %slot
  store i8* null, i8* %slot
  %nh = add i64 %head, 1
  store i64 %nh, i64* %c16
  ret i8* %v
}
