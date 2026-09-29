# Photon 编译后端重构方案

> **目标**：去除冗余 "Photon" 前缀、一个类一个文件、按编译阶段重新分包
>
> **范围**：`aura/photon/aura/lang/compiler/photon/` 下 26 个文件、50 个类型
>
> **状态**：方案阶段，尚未执行

---

## 一、现状分析

### 1.1 当前文件与类分布（26 个文件，50 个类型）

| 现有文件 | 包含类型 | 问题 |
|---|---|---|
| `PlatformConfig.aura` | `PlatformType`, `ArchType`, `CallingConvention`, `SyscallConvention`, `PlatformConfig`, `PlatformConfigUtils` | **6 类型/文件** |
| `MachineDag.aura` | `DagNode`, `DagInstruction`, `MachineDag`, `MachineDagUtils`, `DagPatterns` | **5 类型/文件** |
| `Lir.aura` | `LirValue`, `LirBlock`, `LirFunction`, `LirProgram`, `LirUtils`, `LirBlockUtils` | **6 类型/文件** |
| `PhotonPipeline.aura` | `BackendResult`, `PhotonPipeline`, `PhotonPipelineUtils` | **3 类型/文件** |
| `PhotonObjectWriter.aura` | `PhotonObjectWriter`, `PhotonObjectWriterUtils` | 2 类型/文件 |
| `PhotonSystemLinker.aura` | `PhotonSystemLinker`, `PhotonSystemLinkerUtils` | 2 类型/文件 |
| `PhotonLldConfig.aura` | `PhotonLldConfig`, `PhotonLldConfigUtils` | 2 类型/文件 |
| `PhotonRuntime.aura` | `PhotonRuntime`, `PhotonRuntimeUtils` | 2 类型/文件 |
| `PhotonBootstrap.aura` | `PhotonBootstrap`, `PhotonBootstrapUtils` | 2 类型/文件 |
| `PhotonExceptionHandler.aura` | `PhotonExceptionHandler`, `PhotonExceptionHandlerUtils` | 2 类型/文件 |
| `PhotonOptPasses.aura` | `PhotonOptPasses`, `PhotonOptPassesUtils` | 2 类型/文件 |
| `InstructionSelection.aura` | `InstructionSelector`, `InstructionSelectorUtils` | 2 类型/文件 |
| `RegisterAllocator.aura` | `RegisterAllocator`, `RegisterAllocatorUtils` | 2 类型/文件 |
| `PeepholeOptimizer.aura` | `PeepholeOptimizer`, `PeepholeOptimizerUtils` | 2 类型/文件 |
| `X86Emitter.aura` | `X86Emitter`, `X86EmitterUtils` | 2 类型/文件 |
| `SyscallEmitter.aura` | `SyscallEmitter`, `SyscallEmitterUtils` | 2 类型/文件 |
| `JitBackend.aura` | `JitBackend`, `JitBackendUtils` | 2 类型/文件 |
| `x86_64/X86Encoder.aura` | `X86Encoder`, `X86EncoderUtils` | 2 类型/文件 |
| `PhotonNativeWriter.aura` | `PhotonNativeWriter` | 1 类型（但文件名有 Photon 前缀） |
| `Lowering.aura` | `Lowering` | 1 类型 ✓ |
| `PhotonCoffDumper.aura` | `main()` | 1 类型（驱动） |
| `PhotonHatBuild.aura` | `main()` | 1 类型（驱动） |
| `PhotonHatCompile.aura` | `main()` | 1 类型（驱动） |
| `PhotonHelloBuild.aura` | `main()` | 1 类型（驱动） |
| `PhotonVarTest.aura` | `main()` | 1 类型（驱动） |
| `x86_64/X86EncoderTest.aura` | `main()` | 1 类型（测试） |

### 1.2 三个核心问题

**问题 A — "Photon" 前缀冗余**
- 14 个文件名和对应的类名以 `Photon` 开头，但包名已经是 `aura.lang.compiler.photon`，前缀完全冗余
- 已有 12 个文件（`InstructionSelection`、`JitBackend`、`Lir`、`Lowering`、`MachineDag`、`PeepholeOptimizer`、`PlatformConfig`、`RegisterAllocator`、`SyscallEmitter`、`X86Emitter`、`X86Encoder`、`X86EncoderTest`）没有此前缀——**风格不统一**

**问题 B — 多类型堆积**
- 最严重：`PlatformConfig.aura`（6 类型）、`Lir.aura`（6 类型）、`MachineDag.aura`（5 类型）
- 大量文件包含 `XxxUtils` 伴生对象，与主类耦合在同一个文件中

**问题 C — 扁平包结构**
- 全部 26 个文件挤在 `aura.lang.compiler.photon` 一个包里（仅 `x86_64` 有一个子包）
- 编译阶段（LIR → DAG → RegAlloc → Peephole → Codegen → Object → Link → Runtime → JIT → Pipeline）全混在一起
- 对比非 photon 的 `aura.lang.compiler`，已按 `hir/`、`mir/`、`aot/`、`jit/`、`vm/`、`lexer/`、`parser/` 等良好分包

---

## 二、重构原则

1. **去除冗余 "Photon" 前缀**：包名 `photon` 已标识上下文，文件/类名不再重复。仅保留包名中的 "photon" 作为**必要的 photon**。
2. **一个类一个文件**：每个 `class` / `object` / `interface` 独占一个 `.aura` 文件。`main()` 驱动的入口文件可保留为独立文件。
3. **按编译阶段分包**：参照非 photon 编译器已有的分包模式，按功能/阶段组织。
4. **类名与文件名一致**：`Xxx.aura` 文件中定义的 `class Xxx` / `object Xxx`。
5. **最小改动语义**：仅重命名、移动、拆分，不修改任何方法体逻辑。

---

## 三、新目录结构

```
aura/lang/compiler/photon/
├── config/                     # 平台配置
│   ├── PlatformConfig.aura          ← object PlatformConfig
│   ├── PlatformConfigUtils.aura     ← object PlatformConfigUtils
│   ├── PlatformType.aura            ← object PlatformType
│   ├── ArchType.aura                ← object ArchType
│   ├── CallingConvention.aura       ← object CallingConvention
│   └── SyscallConvention.aura       ← class SyscallConvention
│
├── ir/                         # LIR 低层中间表示 (Phase B)
│   ├── LirValue.aura                  ← class LirValue
│   ├── LirBlock.aura                  ← class LirBlock
│   ├── LirFunction.aura               ← class LirFunction
│   ├── LirProgram.aura                ← class LirProgram
│   ├── LirUtils.aura                  ← object LirUtils
│   ├── LirBlockUtils.aura             ← object LirBlockUtils
│   └── Lowering.aura                  ← class Lowering
│
├── dag/                        # 机器级 DAG + 指令选择 (Phase C)
│   ├── DagNode.aura                   ← class DagNode
│   ├── DagInstruction.aura            ← class DagInstruction
│   ├── MachineDag.aura                ← class MachineDag
│   ├── MachineDagUtils.aura           ← object MachineDagUtils
│   ├── DagPatterns.aura               ← object DagPatterns
│   ├── InstructionSelector.aura       ← class InstructionSelector
│   └── InstructionSelectorUtils.aura  ← object InstructionSelectorUtils
│
├── alloc/                      # 寄存器分配 (Phase D.1)
│   ├── RegisterAllocator.aura         ← class RegisterAllocator
│   └── RegisterAllocatorUtils.aura    ← object RegisterAllocatorUtils
│
├── opt/                        # 优化 Pass (Phase D.2+)
│   ├── PeepholeOptimizer.aura         ← class PeepholeOptimizer
│   ├── PeepholeOptimizerUtils.aura    ← object PeepholeOptimizerUtils
│   ├── OptPasses.aura                 ← class OptPasses
│   └── OptPassesUtils.aura            ← object OptPassesUtils
│
├── codegen/                    # 代码生成 / 发射器
│   ├── X86Emitter.aura                ← class X86Emitter
│   ├── X86EmitterUtils.aura           ← object X86EmitterUtils
│   ├── SyscallEmitter.aura            ← object SyscallEmitter
│   ├── SyscallEmitterUtils.aura       ← object SyscallEmitterUtils
│   └── x86_64/                        # x86_64 指令编码器
│       ├── X86Encoder.aura            ← class X86Encoder
│       ├── X86EncoderUtils.aura       ← object X86EncoderUtils
│       └── X86EncoderTest.aura        ← main() 测试驱动
│
├── object/                     # 目标文件写入
│   ├── ObjectWriter.aura              ← class ObjectWriter
│   ├── ObjectWriterUtils.aura         ← object ObjectWriterUtils
│   └── NativeWriter.aura              ← object NativeWriter
│
├── link/                       # 链接器集成
│   ├── SystemLinker.aura              ← class SystemLinker
│   ├── SystemLinkerUtils.aura         ← object SystemLinkerUtils
│   ├── LldConfig.aura                 ← class LldConfig
│   └── LldConfigUtils.aura            ← object LldConfigUtils
│
├── runtime/                    # Runtime 支持
│   ├── Runtime.aura                   ← object Runtime
│   └── RuntimeUtils.aura              ← object RuntimeUtils
│
├── jit/                        # JIT 后端
│   ├── JitBackend.aura                ← class JitBackend
│   └── JitBackendUtils.aura           ← object JitBackendUtils
│
├── pipeline/                   # 编译管线编排
│   ├── Pipeline.aura                  ← class Pipeline
│   ├── PipelineUtils.aura             ← object PipelineUtils
│   └── BackendResult.aura             ← class BackendResult
│
├── bootstrap/                  # 自举链
│   ├── Bootstrap.aura                 ← class Bootstrap
│   └── BootstrapUtils.aura            ← object BootstrapUtils
│
├── exception/                  # 异常处理
│   ├── ExceptionHandler.aura          ← class ExceptionHandler
│   └── ExceptionHandlerUtils.aura     ← object ExceptionHandlerUtils
│
└── drivers/                    # 构建/测试驱动 (main 入口)
    ├── CoffDumper.aura                ← main()
    ├── HatBuild.aura                  ← main()
    ├── HatCompile.aura                ← main()
    ├── HelloBuild.aura                ← main()
    └── VarTest.aura                   ← main()
```

**总计：56 个文件，50 个类型 + 6 个 main() 驱动**

---

## 四、详细文件映射

### 4.1 文件重命名 + 拆分映射

| 原文件 | 原类名 | 新文件路径 | 新类名 | 变更类型 |
|---|---|---|---|---|
| `PlatformConfig.aura` | `PlatformType` | `config/PlatformType.aura` | `PlatformType` | 移动+拆包 |
| `PlatformConfig.aura` | `ArchType` | `config/ArchType.aura` | `ArchType` | 移动+拆包 |
| `PlatformConfig.aura` | `CallingConvention` | `config/CallingConvention.aura` | `CallingConvention` | 移动+拆包 |
| `PlatformConfig.aura` | `SyscallConvention` | `config/SyscallConvention.aura` | `SyscallConvention` | 移动+拆包 |
| `PlatformConfig.aura` | `PlatformConfig` | `config/PlatformConfig.aura` | `PlatformConfig` | 移动+拆包 |
| `PlatformConfig.aura` | `PlatformConfigUtils` | `config/PlatformConfigUtils.aura` | `PlatformConfigUtils` | 移动+拆包 |
| `Lir.aura` | `LirValue` | `ir/LirValue.aura` | `LirValue` | 移动+拆包 |
| `Lir.aura` | `LirBlock` | `ir/LirBlock.aura` | `LirBlock` | 移动+拆包 |
| `Lir.aura` | `LirFunction` | `ir/LirFunction.aura` | `LirFunction` | 移动+拆包 |
| `Lir.aura` | `LirProgram` | `ir/LirProgram.aura` | `LirProgram` | 移动+拆包 |
| `Lir.aura` | `LirUtils` | `ir/LirUtils.aura` | `LirUtils` | 移动+拆包 |
| `Lir.aura` | `LirBlockUtils` | `ir/LirBlockUtils.aura` | `LirBlockUtils` | 移动+拆包 |
| `Lowering.aura` | `Lowering` | `ir/Lowering.aura` | `Lowering` | 移动 |
| `MachineDag.aura` | `DagNode` | `dag/DagNode.aura` | `DagNode` | 移动+拆包 |
| `MachineDag.aura` | `DagInstruction` | `dag/DagInstruction.aura` | `DagInstruction` | 移动+拆包 |
| `MachineDag.aura` | `MachineDag` | `dag/MachineDag.aura` | `MachineDag` | 移动+拆包 |
| `MachineDag.aura` | `MachineDagUtils` | `dag/MachineDagUtils.aura` | `MachineDagUtils` | 移动+拆包 |
| `MachineDag.aura` | `DagPatterns` | `dag/DagPatterns.aura` | `DagPatterns` | 移动+拆包 |
| `InstructionSelection.aura` | `InstructionSelector` | `dag/InstructionSelector.aura` | `InstructionSelector` | 移动+拆包 |
| `InstructionSelection.aura` | `InstructionSelectorUtils` | `dag/InstructionSelectorUtils.aura` | `InstructionSelectorUtils` | 移动+拆包 |
| `RegisterAllocator.aura` | `RegisterAllocator` | `alloc/RegisterAllocator.aura` | `RegisterAllocator` | 移动+拆包 |
| `RegisterAllocator.aura` | `RegisterAllocatorUtils` | `alloc/RegisterAllocatorUtils.aura` | `RegisterAllocatorUtils` | 移动+拆包 |
| `PeepholeOptimizer.aura` | `PeepholeOptimizer` | `opt/PeepholeOptimizer.aura` | `PeepholeOptimizer` | 移动+拆包 |
| `PeepholeOptimizer.aura` | `PeepholeOptimizerUtils` | `opt/PeepholeOptimizerUtils.aura` | `PeepholeOptimizerUtils` | 移动+拆包 |
| `PhotonOptPasses.aura` | `PhotonOptPasses` | `opt/OptPasses.aura` | **OptPasses** | **去前缀**+移动+拆包 |
| `PhotonOptPasses.aura` | `PhotonOptPassesUtils` | `opt/OptPassesUtils.aura` | **OptPassesUtils** | **去前缀**+移动+拆包 |
| `X86Emitter.aura` | `X86Emitter` | `codegen/X86Emitter.aura` | `X86Emitter` | 移动+拆包 |
| `X86Emitter.aura` | `X86EmitterUtils` | `codegen/X86EmitterUtils.aura` | `X86EmitterUtils` | 移动+拆包 |
| `SyscallEmitter.aura` | `SyscallEmitter` | `codegen/SyscallEmitter.aura` | `SyscallEmitter` | 移动+拆包 |
| `SyscallEmitter.aura` | `SyscallEmitterUtils` | `codegen/SyscallEmitterUtils.aura` | `SyscallEmitterUtils` | 移动+拆包 |
| `x86_64/X86Encoder.aura` | `X86Encoder` | `codegen/x86_64/X86Encoder.aura` | `X86Encoder` | 移动+拆包 |
| `x86_64/X86Encoder.aura` | `X86EncoderUtils` | `codegen/x86_64/X86EncoderUtils.aura` | `X86EncoderUtils` | 移动+拆包 |
| `x86_64/X86EncoderTest.aura` | `main()` | `codegen/x86_64/X86EncoderTest.aura` | — | 移动 |
| `PhotonObjectWriter.aura` | `PhotonObjectWriter` | `object/ObjectWriter.aura` | **ObjectWriter** | **去前缀**+移动+拆包 |
| `PhotonObjectWriter.aura` | `PhotonObjectWriterUtils` | `object/ObjectWriterUtils.aura` | **ObjectWriterUtils** | **去前缀**+移动+拆包 |
| `PhotonNativeWriter.aura` | `PhotonNativeWriter` | `object/NativeWriter.aura` | **NativeWriter** | **去前缀**+移动 |
| `PhotonSystemLinker.aura` | `PhotonSystemLinker` | `link/SystemLinker.aura` | **SystemLinker** | **去前缀**+移动+拆包 |
| `PhotonSystemLinker.aura` | `PhotonSystemLinkerUtils` | `link/SystemLinkerUtils.aura` | **SystemLinkerUtils** | **去前缀**+移动+拆包 |
| `PhotonLldConfig.aura` | `PhotonLldConfig` | `link/LldConfig.aura` | **LldConfig** | **去前缀**+移动+拆包 |
| `PhotonLldConfig.aura` | `PhotonLldConfigUtils` | `link/LldConfigUtils.aura` | **LldConfigUtils** | **去前缀**+移动+拆包 |
| `PhotonRuntime.aura` | `PhotonRuntime` | `runtime/Runtime.aura` | **Runtime** | **去前缀**+移动+拆包 |
| `PhotonRuntime.aura` | `PhotonRuntimeUtils` | `runtime/RuntimeUtils.aura` | **RuntimeUtils** | **去前缀**+移动+拆包 |
| `JitBackend.aura` | `JitBackend` | `jit/JitBackend.aura` | `JitBackend` | 移动+拆包 |
| `JitBackend.aura` | `JitBackendUtils` | `jit/JitBackendUtils.aura` | `JitBackendUtils` | 移动+拆包 |
| `PhotonPipeline.aura` | `BackendResult` | `pipeline/BackendResult.aura` | `BackendResult` | 移动+拆包 |
| `PhotonPipeline.aura` | `PhotonPipeline` | `pipeline/Pipeline.aura` | **Pipeline** | **去前缀**+移动+拆包 |
| `PhotonPipeline.aura` | `PhotonPipelineUtils` | `pipeline/PipelineUtils.aura` | **PipelineUtils** | **去前缀**+移动+拆包 |
| `PhotonBootstrap.aura` | `PhotonBootstrap` | `bootstrap/Bootstrap.aura` | **Bootstrap** | **去前缀**+移动+拆包 |
| `PhotonBootstrap.aura` | `PhotonBootstrapUtils` | `bootstrap/BootstrapUtils.aura` | **BootstrapUtils** | **去前缀**+移动+拆包 |
| `PhotonExceptionHandler.aura` | `PhotonExceptionHandler` | `exception/ExceptionHandler.aura` | **ExceptionHandler** | **去前缀**+移动+拆包 |
| `PhotonExceptionHandler.aura` | `PhotonExceptionHandlerUtils` | `exception/ExceptionHandlerUtils.aura` | **ExceptionHandlerUtils** | **去前缀**+移动+拆包 |
| `PhotonCoffDumper.aura` | `main()` | `drivers/CoffDumper.aura` | — | **去前缀**+移动 |
| `PhotonHatBuild.aura` | `main()` | `drivers/HatBuild.aura` | — | **去前缀**+移动 |
| `PhotonHatCompile.aura` | `main()` | `drivers/HatCompile.aura` | — | **去前缀**+移动 |
| `PhotonHelloBuild.aura` | `main()` | `drivers/HelloBuild.aura` | — | **去前缀**+移动 |
| `PhotonVarTest.aura` | `main()` | `drivers/VarTest.aura` | — | **去前缀**+移动 |

---

## 五、类名重命名总表

共 **9 个类 + 9 个 Utils** 需要去除 "Photon" 前缀：

| 原名 | 新名 | 影响范围 |
|---|---|---|
| `PhotonBootstrap` | `Bootstrap` | 类 + Utils + 引用 |
| `PhotonBootstrapUtils` | `BootstrapUtils` | 引用 |
| `PhotonExceptionHandler` | `ExceptionHandler` | 类 + Utils + 引用 |
| `PhotonExceptionHandlerUtils` | `ExceptionHandlerUtils` | 引用 |
| `PhotonLldConfig` | `LldConfig` | 类 + Utils + 引用 |
| `PhotonLldConfigUtils` | `LldConfigUtils` | 引用 |
| `PhotonNativeWriter` | `NativeWriter` | 类 + 引用 |
| `PhotonObjectWriter` | `ObjectWriter` | 类 + Utils + 引用 |
| `PhotonObjectWriterUtils` | `ObjectWriterUtils` | 引用 |
| `PhotonOptPasses` | `OptPasses` | 类 + Utils + 引用 |
| `PhotonOptPassesUtils` | `OptPassesUtils` | 引用 |
| `PhotonPipeline` | `Pipeline` | 类 + Utils + 引用 |
| `PhotonPipelineUtils` | `PipelineUtils` | 引用 |
| `PhotonRuntime` | `Runtime` | 类 + Utils + 引用 |
| `PhotonRuntimeUtils` | `RuntimeUtils` | 引用 |
| `PhotonSystemLinker` | `SystemLinker` | 类 + Utils + 引用 |
| `PhotonSystemLinkerUtils` | `SystemLinkerUtils` | 引用 |

---

## 六、Import 路径变更

### 6.1 包声明变更

每个新文件的 `package` 声明变为：

```
// 旧: package aura.lang.compiler.photon
// 新: package aura.lang.compiler.photon.config   (或 .ir / .dag / .alloc 等)
```

### 6.2 Import 路径变更示例

| 旧 import | 新 import |
|---|---|
| `import aura.lang.compiler.photon.PhotonPipeline` | `import aura.lang.compiler.photon.pipeline.Pipeline` |
| `import aura.lang.compiler.photon.PhotonObjectWriter` | `import aura.lang.compiler.photon.object.ObjectWriter` |
| `import aura.lang.compiler.photon.PhotonSystemLinker` | `import aura.lang.compiler.photon.link.SystemLinker` |
| `import aura.lang.compiler.photon.PhotonLldConfig` | `import aura.lang.compiler.photon.link.LldConfig` |
| `import aura.lang.compiler.photon.PhotonRuntime` | `import aura.lang.compiler.photon.runtime.Runtime` |
| `import aura.lang.compiler.photon.PhotonNativeWriter` | `import aura.lang.compiler.photon.object.NativeWriter` |
| `import aura.lang.compiler.photon.PhotonExceptionHandler` | `import aura.lang.compiler.photon.exception.ExceptionHandler` |
| `import aura.lang.compiler.photon.x86_64.X86Encoder` | `import aura.lang.compiler.photon.codegen.x86_64.X86Encoder` |
| `import aura.lang.compiler.photon.Lir` | `import aura.lang.compiler.photon.ir.LirProgram` |
| `import aura.lang.compiler.photon.Lowering` | `import aura.lang.compiler.photon.ir.Lowering` |
| `import aura.lang.compiler.photon.MachineDag` | `import aura.lang.compiler.photon.dag.MachineDag` |
| `import aura.lang.compiler.photon.InstructionSelection` | `import aura.lang.compiler.photon.dag.InstructionSelector` |
| `import aura.lang.compiler.photon.RegisterAllocator` | `import aura.lang.compiler.photon.alloc.RegisterAllocator` |
| `import aura.lang.compiler.photon.PeepholeOptimizer` | `import aura.lang.compiler.photon.opt.PeepholeOptimizer` |
| `import aura.lang.compiler.photon.X86Emitter` | `import aura.lang.compiler.photon.codegen.X86Emitter` |
| `import aura.lang.compiler.photon.SyscallEmitter` | `import aura.lang.compiler.photon.codegen.SyscallEmitter` |
| `import aura.lang.compiler.photon.JitBackend` | `import aura.lang.compiler.photon.jit.JitBackend` |

### 6.3 相对 import 变更

`PhotonHatCompile.aura` 中的相对引用：
```
// 旧
import "PhotonPipeline.aura"
import "PhotonRuntime.aura"

// 新（需要改为完整路径或新的相对路径）
import "pipeline/Pipeline.aura"
import "runtime/Runtime.aura"
```

### 6.4 文件头 aura:/// URI 变更

每个文件首行的注释 URI 需同步更新：
```
// 旧: // aura:///aura/lang/compiler/photon/PhotonPipeline.aura
// 新: // aura:///aura/lang/compiler/photon/pipeline/Pipeline.aura
```

---

## 七、外部引用更新

### 7.1 构建脚本

| 文件 | 旧路径 | 新路径 |
|---|---|---|
| `scripts/photon/build-photon-hello.ps1` | `PhotonHelloBuild.aura` | `drivers/HelloBuild.aura` |
| `scripts/photon/build-photon-hat.ps1` | `PhotonHatBuild.aura` | `drivers/HatBuild.aura` |
| `scripts/photon/build-photon-full.ps1` | `PhotonDriver.aura` | 需确认是否已删除 |

### 7.2 内部交叉引用

以下文件内部的类引用需要批量替换（约 250 处引用）：

| 被引用类 | 被引用文件（需更新引用） |
|---|---|
| `PhotonRuntime` → `Runtime` | `PhotonPipeline.aura`, `PhotonHatBuild.aura`, `PhotonHatCompile.aura`, `PhotonHelloBuild.aura`, `PhotonVarTest.aura`, `PhotonObjectWriter.aura`, `InstructionSelection.aura`, `X86Emitter.aura`, `X86Encoder.aura` |
| `PhotonObjectWriter` → `ObjectWriter` | `PhotonCoffDumper.aura`, `PhotonHatCompile.aura`, `PhotonHelloBuild.aura`, `PhotonVarTest.aura`, `PhotonPipeline.aura`, `PhotonRuntime.aura` |
| `PhotonSystemLinker` → `SystemLinker` | `PhotonHelloBuild.aura`, `PhotonVarTest.aura`, `PhotonPipeline.aura` |
| `PhotonLldConfig` → `LldConfig` | `PhotonSystemLinker.aura`, `PhotonPipeline.aura` |
| `PhotonPipeline` → `Pipeline` | `JitBackend.aura`, `PhotonHatBuild.aura`, `PhotonHatCompile.aura`, `PhotonOptPasses.aura` |
| `PhotonNativeWriter` → `NativeWriter` | `PhotonPipeline.aura`, `PhotonHelloBuild.aura` |
| `PhotonRuntimeUtils` → `RuntimeUtils` | `PhotonPipeline.aura`, `PhotonObjectWriter.aura`, `PhotonHatCompile.aura` |
| `PhotonObjectWriterUtils` → `ObjectWriterUtils` | `PhotonPipeline.aura`, `PhotonObjectWriter.aura`, `PhotonRuntime.aura` |
| `PhotonSystemLinkerUtils` → `SystemLinkerUtils` | `PhotonHelloBuild.aura`, `PhotonVarTest.aura`, `PhotonPipeline.aura` |
| `PhotonLldConfigUtils` → `LldConfigUtils` | `PhotonSystemLinker.aura` |
| `PhotonPipelineUtils` → `PipelineUtils` | `PhotonHatBuild.aura`, `PhotonHatCompile.aura` |
| `PhotonBootstrap` → `Bootstrap` | 需搜索所有引用 |
| `PhotonExceptionHandler` → `ExceptionHandler` | 需搜索所有引用 |
| `PhotonOptPasses` → `OptPasses` | 需搜索所有引用 |

---

## 八、迁移执行步骤

### Step 1：创建新目录结构

```bash
mkdir -p aura/photon/aura/lang/compiler/photon/{config,ir,dag,alloc,opt,codegen/x86_64,object,link,runtime,jit,pipeline,bootstrap,exception,drivers}
```

### Step 2：按包分批迁移（建议顺序）

按依赖从底层到上层，每完成一个包立即验证编译：

| 批次 | 包 | 涉及原文件 | 风险 |
|---|---|---|---|
| 1 | `config` | `PlatformConfig.aura` → 拆 6 文件 | 低（叶子包） |
| 2 | `ir` | `Lir.aura` → 拆 6 文件 + `Lowering.aura` → 移动 | 中（被 dag 引用） |
| 3 | `dag` | `MachineDag.aura` → 拆 5 文件 + `InstructionSelection.aura` → 拆 2 文件 | 中 |
| 4 | `alloc` | `RegisterAllocator.aura` → 拆 2 文件 | 低 |
| 5 | `opt` | `PeepholeOptimizer.aura` → 拆 2 + `PhotonOptPasses.aura` → 拆 2 | 低 |
| 6 | `codegen` | `X86Emitter.aura` → 拆 2 + `SyscallEmitter.aura` → 拆 2 + `x86_64/` → 移 3 | 低 |
| 7 | `object` | `PhotonObjectWriter.aura` → 拆 2 + `PhotonNativeWriter.aura` → 移 | 中 |
| 8 | `link` | `PhotonSystemLinker.aura` → 拆 2 + `PhotonLldConfig.aura` → 拆 2 | 低 |
| 9 | `runtime` | `PhotonRuntime.aura` → 拆 2 | 高（被多处引用） |
| 10 | `jit` | `JitBackend.aura` → 拆 2 | 低 |
| 11 | `pipeline` | `PhotonPipeline.aura` → 拆 3 | 高（集成所有阶段） |
| 12 | `bootstrap` | `PhotonBootstrap.aura` → 拆 2 | 低 |
| 13 | `exception` | `PhotonExceptionHandler.aura` → 拆 2 | 低 |
| 14 | `drivers` | 5 个 main 文件移动 + 更新 import | 低 |

### Step 3：全局替换类名

```bash
# 每个 PhotonXxx → Xxx 的替换需在所有 .aura 文件中执行
# 替换顺序：先长名后短名，避免部分替换
sed -i 's/PhotonSystemLinkerUtils/SystemLinkerUtils/g' *.aura
sed -i 's/PhotonSystemLinker/SystemLinker/g' *.aura
sed -i 's/PhotonObjectWriterUtils/ObjectWriterUtils/g' *.aura
# ... 以此类推
```

### Step 4：更新 package 声明和 import 路径

### Step 5：更新构建脚本路径

### Step 6：编译验证

---

## 九、关键注意事项

1. **`PhotonRuntime` → `Runtime` 命名冲突风险**：`aura.lang.compiler.aot.Runtime.aura` 已存在 `object RuntimeUtils`，但两者在不同包（`photon.runtime` vs `aot`），Aura 的 import 机制使用完整包路径，**不会冲突**。

2. **`PhotonObjectWriter` → `ObjectWriter` 与 `ObjectWriterUtils`**：当前 `ObjectWriterUtils` 中有方法引用 `PhotonObjectWriter()` 构造函数，需同步改名为 `ObjectWriter()`。

3. **`PhotonNativeWriter` 是 `object` 而非 `class`**：它内部用 `PhotonNativeWriter.O_WRONLY_CREAT_TRUNC` 等静态引用，改名后需同步更新为 `NativeWriter.O_WRONLY_CREAT_TRUNC`。

4. **`PhotonRuntime` 是 `object` 且引用量最大**（约 80+ 处引用），改名影响面最广，建议作为单独批次处理并验证。

5. **相对 import `import "PhotonPipeline.aura"`**：在 `PhotonHatCompile.aura` 中存在，Aura 的相对 import 是否支持子目录路径需验证。若不支持，需改为完整包路径 import。

6. **`X86EncoderTest.aura`**：当前在 `x86_64/` 下，属于测试驱动，建议移到 `codegen/x86_64/` 下保持与 `X86Encoder` 同目录。

7. **`PhotonDriver.aura` 在构建脚本中被引用**（`build-photon-full.ps1`），但当前目录下未找到此文件——可能已被删除或重命名，需确认。

8. **文件头注释中的 "aura:///" URI**：每个文件首行的 `// aura:///aura/lang/compiler/photon/PhotonPipeline.aura` 需同步更新为新路径。
