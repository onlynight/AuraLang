# @aura-lang/dsh-highlight-hat

DSH Web GUI client plugin: HAT v3.0 SSA IR 语法高亮。

复用 VSCode 扩展的 TextMate 语法文件，通过 Shiki 渲染，适配 DSH 实时主题。

## 项目结构

```
hat-dsh-highlight/
├── src/                        # TypeScript 源码
│   ├── client.ts               # DSH 客户端入口
│   ├── index.ts                # 包根 (host 半)
│   ├── highlight/              # DOM-free 高亮 API
│   ├── HatCodeBody.tsx         # 代码预览组件
│   ├── highlighter.ts          # Shiki 实例
│   ├── incremental.ts          # 增量分词
│   ├── fallback.ts             # 降级分词器
│   ├── engine.ts               # 正则引擎选择
│   ├── decode.ts               # 字节解码
│   ├── theme.ts                # 配色主题
│   ├── styles.ts               # CSS 样式
│   ├── types.ts                # 类型定义
│   ├── locale.ts               # 国际化
│   ├── plugin-context.ts       # 上下文类型
│   ├── declarations.ts         # 声明合并
│   └── hat-grammar.ts          # 语法文件 (自动生成)
├── grammar/                    # 文法文件
│   ├── hat.tmLanguage.json     # 从 VSCode 扩展同步
│   └── SYNC-SHA256             # 上游指纹
├── scripts/                    # 构建脚本
│   ├── build.mjs               # 构建
│   ├── sync-grammar.mjs        # 同步语法
│   ├── bundle-grammar.mjs      # 打包语法（生成 src/hat-grammar.ts）
│   ├── diff-grammar.mjs        # 漂移检查：上游 / 本地拷贝 / 主题覆盖率
│   └── _grammar-source.mjs     # 语法源解析
├── package.json
├── cordis.patch.yml            # 插件注册
├── tsconfig.json
├── tsconfig.build.json
├── pnpm-workspace.yaml
└── .gitignore
```

## 安装

```bash
cd tools/dsh-plugins/hat-dsh-highlight
pnpm install
```

## 构建

```bash
# 同步语法文件 (从 hat-vscode-extension 复制)
pnpm run sync:grammar

# 构建
pnpm run build

# 或干净构建
pnpm run build:clean
```

## 测试

```bash
pnpm test
```

## 使用

插件通过 `cordis.patch.yml` 注册到 DSH 的 profile 中。在 DSH 打开 `.hat` 文件时，
自动加载高亮预览。

### 特性

- **增量高亮**：大文件分块渲染，不阻塞 UI
- **双引擎**：默认 JavaScript 引擎，可选 WebAssembly (oniguruma)
- **主题适配**：自动跟随 DSH 亮/暗主题
- **降级支持**：Shiki 不可用时使用正则降级分词器
- **字节解码**：支持 bytes-complete 传输，处理非 UTF-8 和 NUL 字节

## 开发

```bash
# 漂移检查：上游指纹 / 本地拷贝是否规范化一致 / 主题是否覆盖全部 scope
pnpm run check:grammar
```

> `check:grammar` 直接跑 `scripts/diff-grammar.mjs`。**改了文法的 scope 名就必须同步改 `src/theme.ts`**，
> 否则该 token 会静默掉回正文色——这条检查正是为此存在（`prepublishOnly` 也会跑它）。

## 许可证

Apache-2.0
