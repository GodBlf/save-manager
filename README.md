# 单机游戏存档管理器

[English](README.en.md) · [下载 Windows 版](https://github.com/GodBlf/save-manager/releases/latest)

一个用 Rust 和 egui 编写的 Windows 本地存档管理器。为每个游戏选择要追踪的文件和目录，手动创建快照，再从历史中找回需要的版本。

## 下载与运行

在 [GitHub Releases](https://github.com/GodBlf/save-manager/releases/latest) 下载 Windows x64 ZIP，解压后运行 `save-manager.exe`。

## 快速开始

1. 点击「添加游戏」，填写名称并选择快照根目录。每个游戏使用独立的快照根目录。
2. 添加要追踪的文件和目录，例如 `D:\A\saves\` 和 `D:\A\A.ini`。
3. 点击「创建快照」。若第一份快照取名「A重要」，应用会将两个追踪项保存到快照根目录下的 `0-A重要\saves\` 和 `0-A重要\A.ini`。留空则使用 `0\`。
4. 从快照历史打开目录、改名、恢复或删除。改名会保留编号。确认删除后，应用会尝试将快照移入 Windows 回收站。

恢复前请关闭游戏，并核对确认窗口列出的原存档路径。恢复会完整替换这些文件或目录，快照中没有的文件会从被替换的目录中移除。

## 手动搬运快照

用资源管理器复制完整的快照子目录，包括其中的 `.save-manager.json`。在「编辑配置」中选择新的快照根目录，应用会扫描该目录并显示识别到的快照。游戏配置保存在 `%LOCALAPPDATA%\SaveManager`。

## 从源码构建

使用 Windows 上的 Rust MSVC 工具链和 Visual Studio Build Tools 的 C++ 桌面开发组件。

```powershell
cargo build --release --locked
cargo test --locked
```

可执行文件位于 `target\release\save-manager.exe`。

界面工作流程参考了 [game-save-manager](https://github.com/mcthesw/game-save-manager)。
