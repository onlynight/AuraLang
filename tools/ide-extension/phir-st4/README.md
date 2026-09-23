# Photon IR Sublime Text 4 Plugin

Photon IR (.phir) 语法高亮插件，基于 `tools/ide-extension/phir-vscode-extension` 的 TextMate 语法改造为 Sublime Text 4 `sublime-syntax`。

## 文件

- `Photon.sublime-syntax` — PHIR 语法高亮定义
- `Photon.sublime-settings` — 编辑默认设置
- `deploy.ps1` — 部署到本地 ST4 包目录

## 部署

在插件目录中运行：

```powershell
cd D:\Code\AuraLang\tools\ide-extension\phir-st4
.\deploy.ps1
```

会部署到：

```text
%APPDATA%\Sublime Text\Packages\PhotonLanguage\
```

重启或重新加载 Sublime Text 后，打开 `.phir` 文件即可自动应用语法。
