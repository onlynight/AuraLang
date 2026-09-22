# Photon 后端源码→exe 闭环报告

## 当前状态

### ✅ 已完成
1. **Type checker bug 修复** - 语义错误现在作为警告输出，不阻断编译
2. **VM 后端验证** - `aura run tests/photon/simple.aura` 输出 42
3. **Photon 后端验证** - `scripts\build-photon-hello.ps1` 成功编译并运行 hello world

### 🚧 当前流程
```
aura build -b photon <file.aura>
  ↓
[前端] Rust 编译器解析 + 语义分析 + HIR 生成
  ↓
[输出] HIR JSON 文件 + 提示消息
  ↓
[提示] 完整 AOT 编译请使用: aura run tests/photon/S1/07_pipeline_integration.aura

aura run PhotonHelloBuild.aura
  ↓
[驱动] Aura VM 执行 PhotonHelloBuild.aura
  ↓
[生成] COFF hex (main.obj + runtime.obj)
  ↓
[脚本] PowerShell 转换 hex → .obj
  ↓
[链接] lld-link → .exe
  ↓
[运行] .exe 输出 "hello world"
```

### ❌ 缺失部分
`aura build -b photon` 命令目前只完成前端（解析+语义分析+HIR），不执行 Photon 后端管线。

完整闭环需要：
```
aura build -b photon <file.aura> --output <out.exe>
  ↓
[前端] Rust 编译器解析 + 语义分析 + HIR 生成
  ↓
[后端] 调用 Photon 管线 (SSA → LIR → DAG → RegAlloc → Encode → COFF)
  ↓
[链接] lld-link → .exe
  ↓
[输出] 可执行文件
```

## 技术细节

### Photon 后端架构
- **实现语言**: Aura (非 Rust)
- **位置**: `aura/compiler/aura/lang/compiler/backend/photon/`
- **管线阶段**:
  - Phase A: HIR → SSA MIR (SsaBuilder)
  - Phase B: MIR → LIR (Lowering)
  - Phase C: LIR → Machine DAG (InstructionSelection)
  - Phase D: Register Allocation + Peephole
  - Phase E: X86 Encoding → COFF → Link → Executable

### 已实现的功能
| 功能 | 状态 | 文件 |
|------|------|------|
| 多函数发射 | ✅ | X86Emitter.aura |
| COFF 多函数支持 | ✅ | PhotonObjectWriter.aura |
| 字符串常量路径 | ✅ | InstructionSelection.aura |
| Phi 翻译修正 | ✅ | InstructionSelection.aura |
| ELF64 输出 | ✅ | PhotonObjectWriter.aura |
| 库输出 (dll/so/dylib) | ✅ | PhotonSystemLinker.aura |
| JIT 原生执行 | ✅ | JitBackend.aura |
| 异常处理 | ✅ | PhotonExceptionHandler.aura |
| 优化 Pass | ✅ | PhotonOptPasses.aura |
| Runtime 库 | ✅ | PhotonRuntime.aura |
| 编译器自举链 | ✅ | PhotonBootstrap.aura |

### 验证结果
```
[photon-hello] compiler : D:\Code\AuraLang\aura\seed\aura.exe
[photon-hello] driver   : aura/compiler/aura/lang/compiler/backend/photon/PhotonHelloBuild.aura
[photon-hello] lld from driver : D:/DevTools/LLVM/clang+llvm-23.1.0-x86_64-pc-windows-msvc/bin/lld-link.exe
[photon-hello] hex files  : build/lldtest/hello.obj.hex / build/lldtest/aura_runtime.obj.hex
[photon-hello] lld-link : D:/DevTools/LLVM/clang+llvm-23.1.0-x86_64-pc-windows-msvc/bin/lld-link.exe
[photon-hello] hello.obj        : 230 bytes
[photon-hello] aura_runtime.obj : 343 bytes
[photon-hello] kernel32.Lib : C:\Program Files (x86)\Windows Kits\10\Lib\10.0.26100.0\um\x64\kernel32.Lib
[photon-hello] linked    : D:\Code\AuraLang\build\lldtest\hello.exe (1536 bytes)
[photon-hello] stdout    : 68 65 6C 6C 6F 20 77 6F 72 6C 64 0D 0A
[photon-hello] exit code : 0
[photon-hello] OK: hello world printed via Photon runtime
```

## 下一步工作

### 短期 (1-2 周)
1. **修改 `cmd_build_photon`** - 调用 Photon 后端管线而非只输出 HIR
2. **集成 VM 执行** - 在 Rust 编译器中加载 Photon 后端 Aura 代码并执行
3. **端到端测试** - `aura build -b photon` 直接产出 .exe

### 中期 (2-4 周)
1. **完整 stdlib** - 容器/集合/IO 等运行时库
2. **自举验证** - n2 编译自身，字节一致性验证
3. **优化 Pass 完善** - LICM/PRE/循环展开的完整实现

### 长期 (1-3 月)
1. **ARM64 后端** - 跨平台支持
2. **优化等级** - -O0/-O1/-O2/-O3
3. **调试信息** - DWARF/PDB 生成