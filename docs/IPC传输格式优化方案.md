44: # AuraLang IPC 传输格式性能分析与优化方案

47: > **状态**：分析完成，未实施
48: > **日期**：2026-06-30
49: > **范围**：`compiler/src/vm/ipc.rs`, `channel_tcp.rs`, `actor_process.rs`, `value.rs`

52: ---

55: ## 1. 现状：JSON 序列化的性能开销

58: ### 1.1 当前实现

59: ```rust
60: // ipc.rs
61: pub fn send_value(&mut self, val: &Value) -> Result<(), IpcError> {
62:     let json = serde_json::to_string(val)?;    // ← 字符串构造
63:     let bytes = json.as_bytes();
64:     self.stream.write_all(&len.to_be_bytes())?;
65:     self.stream.write_all(bytes)?;             // ← 字符串字节写入
66: }

67: pub fn recv_value(&mut self) -> Result<Option<Value>, IpcError> {
68:     // ... read_exact(len_buf) ...
69:     let val: Value = serde_json::from_slice(&buf)?;  // ← JSON 解析
70: }
71: ```

72: Value 的 serde 实现使用「标签 + 中间类型」模式：

73: ```rust
74: // value.rs
75: fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> {
76:     let mut map = serializer.serialize_map(Some(2))?;
77:     map.serialize_entry("t", "i" | "f" | "b" | "s" | ...)?;  // 标签字符串
78:     map.serialize_entry("v", &self.value_as_any())?;           // 载荷
79: }
80: // 反序列化时解析 {"t":"i","v":42} → 字符串比较
81: ```

84: ### 1.2 开销量化

85: | 维度 | JSON 当前 | 预期改进 |
86: |------|----------|---------|
87: | `Value::Int(42)` | `{"t":"i","v":42}` = **14 字节** | 9 字节 (二进制) |
88: | `Value::Str("hello")` | `{"t":"s","v":"hello"}` = **22 字节** | 10 字节 |
89: | `Value::List([Int(1),Int(2)])` | `{"t":"l","v":[{"t":"i","v":1},...]}` = **44 字节** | 21 字节 |
90: | `Value::Map({"a":1,"b":2})` | `{"t":"m","v":{"a":1,"b":2}}` ≈ **40 字节** | 25 字节 |
91: | 序列化耗时 | ~50-300ns | ~5-15ns |
92: | 反序列化耗时 | ~80-500ns | ~10-20ns |
93: | 内存分配 | 每次 new String + Rc<str> + HashMap clone | 零分配（栈内编解码） |

94: **根因分析**：
95:
96: 1. **中间字符串构造**：`to_string()` 构造完整 JSON 字符串，每次调用分配堆内存
97: 2. **标签冗余**：`"t":"i","v":` 占用了 11 字节元信息，仅传递 1 位信息（枚举变体）
98: 3. **双重 clone**：`value_as_any()` 对 `List`/`Map`/`Str` 做 `.clone()`，序列化前就分配内存
99: 4. **字符串逃逸**：JSON 需转义特殊字符（`"`→`\"`），增加编码/解码开销
100: 5. **HashMap 无序**：JSON 序列化 `HashMap<Value, Value>` 时 key 顺序不确定，影响确定性
101: 6. **serde 反射开销**：每个 tag 字符串比较，非直接位模式匹配

104: ### 1.3 跨进程特有问题

105: ```rust
106: /// Ref(usize) 和 Weak(usize) 是进程内堆句柄
107: /// 跨进程传递时，目标进程的 handle 与源进程无关
108: pub enum Value {
109:     Ref(usize),    // ← 进程本地，跨进程无意义
110:     Weak(usize),   // ← 进程本地，跨进程无意义
111:     Ptr(i64),      // ← 进程本地地址，跨进程无效
112: }
113: ```

114: 当前 JSON 方案将 `Ref`/`Weak`/`Ptr` 原样序列化，接收端拿到的是**无效句柄**。

117: ---

120: ## 2. 方案对比

123: ### 2.1 方案 A：Bincode（推荐备选）

124: ```rust
125: // 添加依赖: bincode = "2"
126: bincode::serialize_to_writer(&mut stream, val)?;
127: bincode::deserialize_from_reader(&mut stream)?;
128: ```

129: | 维度 | 评价 |
130: |------|------|
131: | 性能 | 5-10x 快于 JSON，零分配（直接写入流） |
132: | 实现成本 | 极低（仅需 `#[derive]` 已有 serde） |
133: | 格式可控 | 低（bincode 固定格式） |
134: | 依赖 | 新增 bincode crate |
135: | 问题 | `HashMap` 无序，`Ref/Weak/Ptr` 仍无意义 |

136: **结论**：快速见效，但不可控且仍有语义问题。

139: ### 2.2 方案 B：自定义二进制格式（推荐 ⭐⭐）

140: 针对 `Value` 的 10 个变体设计固定位模式编码：

141: ```text
142: Value binary encoding (v1):

143: 字节 0: 标签 (tag byte)
144:   0x00 = Null
145:   0x01 = Int(i64)       → 8 字节, little-endian
146:   0x02 = Float(f64)     → 8 字节, little-endian IEEE 754
147:   0x03 = Bool           → 1 字节 (0x00/0x01)
148:   0x04 = Str            → u32 长度(LE) + UTF-8 字节
149:   0x05 = Ptr(i64)       → 8 字节, little-endian
150:   0x06 = Ref(usize)     → u32 堆句柄(LE) ← 进程本地，跨进程标记为 -1
151:   0x07 = Weak(usize)    → u32 弱句柄(LE) ← 同上
152:   0x08 = List           → u32 元素数(LE) + N × Value
153:   0x09 = Map            → u32 条目数(LE) + N × (Value_key + Value_val)
154:   0x10 = ListSmall      → u16 元素数(LE) + N × Value    ← 小列表优化
155:   0x11 = MapSmall       → u16 条目数(LE) + N × (K+V)    ← 小映射优化
156: 
157: 消息帧格式:
158:   [4 bytes] 消息长度 (big-endian u32)
159:   [N bytes] Value 二进制载荷
160: ```

161: **量化预估**：

162: | 场景 | JSON | 自定义二进制 | 缩减率 | 速度比 |
163: |------|------|-------------|--------|-------|
164: | Int(42) | 14B | 9B | 36% | 8x |
165: | Str("hello") | 22B | 10B | 55% | 10x |
166: | List([1,2,3]) | 62B | 28B | 55% | 15x |
167: | Map(2 entries) | 40B | 25B | 38% | 12x |
168: | 嵌套 Map(List(3×Int)) | 120B | 55B | 54% | 20x |

169: **优势**：
170: - 零 serde 反射开销（直接 `read_u8` + `match`）
171: - 零中间字符串构造（直接 `write_all`）
172: - 零 `.clone()` 前置开销（按需拷贝）
173: - 可设计进程本地句柄标记（`0xFFFF_FFFF` = 跨进程无效）
174: - 小集合用 u16 计数减少 2 字节开销

175: **劣势**：
176: - 需手动实现 encode/decode（~150 行代码）
177: - 格式版本演进需自行维护

178: ---

181: ### 2.3 方案 C：零拷贝共享内存（远期）

182: ```text
183: 进程 A ── write() ──→ [共享内存环形缓冲区] ── read() ──→ 进程 B
184:                        ↑
185:                  atomic 读写指针 + futex/WaitOnAddress
186: ```

187: | 维度 | 评价 |
188: |------|------|
189: | 传输开销 | 0 字节（内核不复制数据） |
190: | 延迟 | ~200ns（vs TCP 的 ~20μs） |
191: | 实现复杂度 | 极高（跨平台 Unix/Windows） |
192: | 适用场景 | 高频小消息（游戏帧同步、实时渲染） |

193: **结论**：当前阶段过度工程，作为 Phase 5 远期目标。

196: ### 2.4 方案 D：Unix Domain Socket（传输层优化）

197: 当前使用 TCP (127.0.0.1)，可改为 Unix Domain Socket：

198: | 维度 | TCP (127.0.0.1) | Unix Socket |
199: |------|-----------------|-------------|
200: | 延迟 | ~20μs | ~2μs |
201: | 吞吐 | ~100MB/s | ~500MB/s |
202: | 零拷贝 | 否 | 可（sendmsg/recvmsg） |
203: | 文件描述符传递 | 否 | 是（SCM_RIGHTS） |
204: | Windows 支持 | ✅ | ⚠️ 需 Named Pipe |

205: **结论**：传输层优化与编码格式正交，可并行实施。

208: ---

211: ## 3. 推荐方案：分层实施

214: ### Phase 3.1（当前）：自定义二进制格式 + 保留 JSON 回退

215: 1. **新增 `vm/serialize.rs`**：实现 `Value` 的二进制编解码
216:    ```rust
217:    pub fn encode_value(val: &Value, buf: &mut Vec<u8>);
218:    pub fn decode_value(data: &[u8], offset: &mut usize) -> Result<Value, IpcError>;
219:    ```

220: 2. **修改 `ipc.rs`**：`send_value`/`recv_value` 优先使用二进制格式
221:    ```rust
222:    pub fn send_value(&mut self, val: &Value) -> Result<(), IpcError> {
223:        let mut buf = Vec::with_capacity(64);
224:        encode_value(val, &mut buf);
225:        self.send_bytes(&buf)
226:    }
227:    ```

228: 3. **修改 `channel_tcp.rs` / `actor_process.rs`**：同上

229: 4. **保留 JSON 路径**：`send_json`/`recv_json` 作为调试回退

230: 5. **删除 `value.rs` 中的 serde 实现**：`ValueAny`、`value_as_any()`、自定义 `Serialize`/`Deserialize`

232: ### Phase 3.2（后续）：Unix Socket 传输层

233: 在 `ipc.rs` 中增加 Unix Domain Socket 后端：
234: - Unix: `UnixStream` + `UnixListener`
235: - Windows: TCP 保留（Named Pipe 作为备选）
236: - 自动探测平台，优先使用 Unix Socket

238: ### Phase 4（远期）：共享内存环形缓冲区

239: 为高频 Channel 提供零拷贝路径。

242: ---

245: ## 4. 自定义格式详细设计

248: ### 4.1 编码规范

249: | 标签 | 值 | 载荷大小 | 字节布局 |
250: |------|-----|---------|---------|
251: | `0x00` | `Null` | 0 | — |
252: | `0x01` | `Int(i64)` | 8 | `[tag][i64 LE]` |
253: | `0x02` | `Float(f64)` | 8 | `[tag][f64 LE]` |
254: | `0x03` | `Bool` | 1 | `[tag][0x00/0x01]` |
255: | `0x04` | `Str` | 4+N | `[tag][u32 len LE][UTF-8 bytes]` |
256: | `0x05` | `Ptr(i64)` | 8 | `[tag][i64 LE]` |
257: | `0x06` | `Ref(usize)` | 4 | `[tag][u32 LE]` |
258: | `0x07` | `Weak(usize)` | 4 | `[tag][u32 LE]` |
259: | `0x08` | `List` | 4+N | `[tag][u32 count LE][Value × count]` |
260: | `0x09` | `Map` | 4+N | `[tag][u32 count LE][(Value_key+Value_val) × count]` |
261: | `0x10` | `ListSmall` | 2+N | `[tag][u16 count LE][Value × count]` |
262: | `0x11` | `MapSmall` | 2+N | `[tag][u16 count LE][(K+V) × count]` |

263: ### 4.2 跨进程句柄标记

264: ```rust
265: /// 进程本地句柄的跨进程占位值
266: pub const INVALID_HANDLE: u32 = 0xFFFF_FFFF;
267: 
268: /// 编码 Ref/Weak 时：
269: /// - 进程内通信：直接编码 handle
270: /// - 跨进程通信：编码 INVALID_HANDLE，接收端替换为 Null
271: 
272: /// 解码 Ref/Weak 时：
273: /// - INVALID_HANDLE → Value::Null（跨进程无效句柄）
274: /// - 其他 → Value::Ref(handle)（进程内有效）
275: ```

276: ### 4.3 小集合优化阈值

277: ```rust
278: /// 当 count ≤ 65535 时使用 u16 计数（ListSmall/MapSmall）
279: /// 当 count > 65535 时使用 u32 计数（List/Map）
280: 
281: const SMALL_THRESHOLD: usize = 65535;
282: ```

283: ### 4.4 预估性能对比

284: ```text
285: ┌──────────────────────────────────────────────────────────────┐
286: │  典型消息: send(actorId=1, "hello world")                    │
287: │                                                              │
288: │  JSON:   {"t":"s","v":"hello world"}                         │
289: │         长度: 28 字节    序列化: ~120ns    反序列化: ~200ns  │
290: │                                                              │
291: │  二进制: [0x04][0B 00 00 00][68 65 6C 6C 6F 20 77 6F 72 6C 64] │
292: │         长度: 14 字节    编码:   ~5ns     解码:   ~8ns       │
293: │                                                              │
294: │  吞吐提升: ~20-30x（编码+解码总耗时）                         │
295: │  带宽节省: ~50%                                               │
296: └──────────────────────────────────────────────────────────────┘
297: ```

300: ---

303: ## 5. 实施建议

306: ### 5.1 代码结构

307: ```
308: compiler/src/vm/
309: ├── serialize.rs       ← 新增：Value 二进制编解码
310: ├── ipc.rs             ← 修改：使用二进制格式，保留 JSON 回退
311: ├── channel_tcp.rs     ← 修改：同上
312: ├── actor_process.rs   ← 修改：同上
313: └── value.rs           ← 修改：删除 serde 实现，保留类型定义
314: ```

315: ### 5.2 serialize.rs 接口

316: ```rust
317: /// 将 Value 编码到缓冲区
318: pub fn encode_value(val: &Value, buf: &mut Vec<u8>);

319: /// 从缓冲区解码 Value
320: /// `offset` 为可变引用，指向当前读取位置
321: pub fn decode_value(data: &[u8], offset: &mut usize) -> Result<Value, IpcError>;

322: /// 编码消息帧（长度前缀 + Value）
323: pub fn encode_frame(val: &Value, buf: &mut Vec<u8>);

324: /// 从缓冲区解码消息帧
325: pub fn decode_frame(data: &[u8]) -> Result<Value, IpcError>;
326: ```

327: ### 5.3 渐进迁移策略

328: 1. **第一步**：新增 `serialize.rs`，独立实现编解码（不修改现有代码）
329: 2. **第二步**：修改 `ipc.rs` 的 `send_value`/`recv_value` 使用二进制格式
330: 3. **第三步**：修改 `channel_tcp.rs` / `actor_process.rs`
331: 4. **第四步**：添加 `--ipc-format json|binary` 标志作为回退
332: 5. **第五步**：删除 `value.rs` 中的 serde 实现

335: ### 5.4 风险与缓解

336: | 风险 | 缓解措施 |
337: |------|---------|
338: | 编码错误导致数据损坏 | 添加 CRC-32 校验（可选，Phase 3.2） |
339: | 跨进程句柄无效 | INVALID_HANDLE 标记 + Null 替换 |
340: | 格式版本演进 | 帧头增加版本字节（Phase 3.2） |
341: | 小端/大端平台差异 | 固定使用 little-endian（x86/ARM 默认） |
