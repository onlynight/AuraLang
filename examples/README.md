# Aura 示例库

> 覆盖 Aura 语言常见模式的参考代码。用于 DSH Few-shot 学习和 RAG 检索。

## 目录结构

```
examples/
├── basics/               ← 基础语法
│   ├── hello-world.aura          最小程序
│   ├── variables.aura            变量与类型
│   ├── data-structures.aura      数据结构（struct/class/enum/interface）
│   ├── control-flow.aura         控制流（when/if/for/try）
│   └── comptime.aura             编译时执行
├── concurrency/          ← 并发编程
│   ├── actor-basic.aura          Actor 基础
│   └── suspend-async.aura        协程与异步
├── ffi/                 ← 外部接口
│   └── extern-declarations.aura  FFI 声明
├── stdlib/              ← 标准库使用（待填充）
│   ├── io.aura
│   ├── math.aura
│   └── time.aura
└── app/                 ← 完整应用
    └── server.aura               配置驱动服务器
```

## 用法

1. **作为 Few-shot 示例**：当用户要求写某类代码时，附带相关示例
2. **作为 RAG 检索源**：用户提问 → 向量检索 → 注入上下文
3. **作为回归测试**：模型输出与示例对比
4. **作为文档**：快速查看 Aura 正确用法

## 示例索引

| 关键词 | 示例文件 |
|--------|---------|
| actor, 并发, 消息 | concurrency/actor-basic.aura |
| suspend, async, await, 协程 | concurrency/suspend-async.aura |
| data struct, 数据结构体 | basics/data-structures.aura |
| class, 继承, 接口 | basics/data-structures.aura |
| enum, 枚举 | basics/data-structures.aura |
| if, when, for, while | basics/control-flow.aura |
| extern, FFI, 调用 C | ffi/extern-declarations.aura |
| comptime, 编译时 | basics/comptime.aura |
| Result, 错误处理 | app/server.aura |
| 完整应用 | app/server.aura |
