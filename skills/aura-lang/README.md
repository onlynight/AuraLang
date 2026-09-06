# Aura-Lang Skill

DSH 技能包：让 AI 编码助手直接生成正确的 Aura 代码。

## 版本历史

| 版本 | 日期 | 变更 |
|------|------|------|
| 1.0.0 | 2026-09-06 | 初始版本。合并编译器探测报告，区分 WORKS/DOESN'T WORK |

## 文件说明

- `SKILL.md` — 技能定义（YAML frontmatter + 完整语法参考）

## 同步到 DSH

当修改 `SKILL.md` 后，同步到 DSH 预设目录：

```powershell
Copy-Item "D:\Code\AuraLang\skills\aura-lang\SKILL.md" "$env:USERPROFILE\.dsh\.agent-presets\aura-coding\skills\aura-lang\SKILL.md" -Force
```

## 更新流程

1. 编辑 `D:\Code\AuraLang\skills\aura-lang\SKILL.md`
2. 更新 frontmatter 中的 `version`、`lastModified`、`changes`
3. 在 Changelog 表追加一行
4. 同步到 DSH 预设目录
5. 新的 DSH 会话即可使用更新后的技能

## 版本规范

- `1.0.0` — 初始发布
- `1.1.0` — 新增功能（如编译器修复后添加更多可用特性）
- `1.0.1` — 修复错误（如语法细节修正）
- `2.0.0` — 重大变更（如编译器大幅更新，大量 BROKEN 特性变为 WORKS）
