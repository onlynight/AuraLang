# Photon 后端单元测试

本目录包含 Photon 编译后端的单元测试用例。

## 目录结构

```
tests/photon/
├── S1/                          # S1 阶段测试（最小闭环）
│   ├── 01_x86_encoder.aura      # X86Encoder 基础编码
│   ├── 02_x86_encoder_relocs.aura # X86Encoder 重定位
│   ├── 03_instruction_selection.aura # 指令选择（LIR→DAG）
│   ├── 04_register_allocator.aura    # 寄存器分配（图着色）
│   ├── 05_x86_emitter.aura           # X86Emitter（DAG→机器码）
│   ├── 06_object_writer.aura         # COFF 目标文件生成
│   └── 07_pipeline_integration.aura  # 全管线集成测试
├── S2/                          # S2 阶段测试（语言子集覆盖）
└── S3/                          # S3 阶段测试（自举）
```

## 运行方式

```bash
# 运行单个测试
aura run tests/photon/S1/01_x86_encoder.aura

# 运行所有 S1 测试
for f in tests/photon/S1/*.aura; do
    echo "=== $f ==="
    aura run "$f"
done
```

## S1 测试清单

| 编号 | 文件 | 覆盖内容 |
|------|------|----------|
| 01 | 01_x86_encoder.aura | Prologue/Epilogue、MOV、ADD、SUB、IMUL、CMP、SETcc、RET |
| 02 | 02_x86_encoder_relocs.aura | LEA-RIP、CALL、重定位生成 |
| 03 | 03_instruction_selection.aura | Const/Add/Call/Ret/Br/CondBr 的 LIR→DAG 映射 |
| 04 | 04_register_allocator.aura | 图着色、颜色回写、溢出检测 |
| 05 | 05_x86_emitter.aura | DAG→X86Encoder 发射、prologue/epilogue |
| 06 | 06_object_writer.aura | COFF 文件组装、hex 文件写入 |
| 07 | 07_pipeline_integration.aura | 全管线端到端（HIR→...→COFF） |
