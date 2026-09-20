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
│   ├── comptime.aura             编译时执行
│   ├── calc.aura                 函数/递归/泛型综合计算
│   ├── demo.aura                 语法综合验证
│   ├── import_syntax_demo.aura   import 各形式演示
│   ├── mini_struct.aura          最小 struct
│   └── t_for.aura / t_if.aura    for/if 微型示例
├── classes/              ← 类与结构体
│   └── class_runtime.aura            类运行时特性演示
├── concurrency/          ← 并发编程
│   ├── actor-basic.aura          Actor 基础
│   ├── actor_system.aura         Actor / 监督树
│   ├── suspend-async.aura        协程与异步
│   ├── coroutine_dispatch.aura   协程 spawn/await
│   ├── message_channel.aura      Channel
│   ├── select_multiplex.aura     Select 多路复用
│   └── concurrent_integration.aura  全特性集成
├── compiler/             ← 编译器/运行时特性验证
│   ├── showcase.aura             语言特性总览（sema 零错误）
│   ├── demo_errors.aura          语义错误诊断示例
│   ├── debug_when.aura           when 守卫调试
│   ├── aot_list_roundtrip.aura   AOT 列表往返
│   ├── cli_demo.aura             CLI 命令行演示
│   ├── debug_when.aura           when 守卫调试
│   ├── demo_errors.aura          语义错误诊断示例
│   ├── dynlist_string_roundtrip.aura  动态列表/字符串往返
│   └── showcase.aura             语言特性总览（sema 零错误）
├── ffi/                 ← 外部接口
│   ├── extern-declarations.aura  FFI 声明
│   ├── p8_c_ffi_demo.aura        extern "c" 调用 libc
│   ├── p8_rust_ffi_demo.aura     extern "rust"
│   └── raylib_demo.aura          Raylib FFI 绑定
├── stdlib/              ← 标准库使用
│   ├── std_demo.aura             全模块用法（P9）
│   ├── std_demo2.aura            全模块用法（续）
│   ├── p14_file_processor.aura   文件统计工具
│   └── p14_json_demo.aura        JSON 工具
├── games/               ← 游戏
│   ├── game_2d_demo.aura         2D 平台游戏
│   └── p14_game_demo.aura        Raylib 绘图游戏
└── app/                 ← 完整应用
    ├── server.aura               配置驱动服务器
    └── p14_package_demo.aura     包管理示例
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
