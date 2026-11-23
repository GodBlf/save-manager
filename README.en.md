# Single-Player Game Save Manager

[简体中文](README.md) · [Download for Windows](https://github.com/GodBlf/save-manager/releases/latest)

A local Windows save manager built with Rust and egui. Choose which files and folders to track for each game, create snapshots manually, and find the version you need in the snapshot history.

The app is designed for players who manage their save files by hand. Its main jobs are saving snapshots, showing snapshot history, and opening folders. To inspect or edit a save, find the file in Windows File Explorer and open it with a suitable editor. For example, if the save uses editable key-value pairs, you can change values such as gold or item counts yourself.

## Download and run

Download the Windows x64 ZIP from [GitHub Releases](https://github.com/GodBlf/save-manager/releases/latest), extract it, and run `save-manager.exe`. The app interface is in Simplified Chinese.

## Quick start

1. Click Add Game (「添加游戏」), enter a name, and choose a dedicated snapshot root folder for the game.
2. For a Zelda example, suppose your tracked folder and file are at `D:\Zelda\saves\` and `D:\Zelda\Zelda.ini`. Add both paths, using the actual locations for your setup.
3. Click Create Snapshot (「创建快照」). If you name the first snapshot `Zelda Important`, the app saves both tracked items as `0-Zelda Important\saves\` and `0-Zelda Important\Zelda.ini` under the snapshot root. Leave the name blank to use `0\`.
4. Use the snapshot history to open a folder, rename a snapshot, restore it, or delete it. Renaming keeps the original number. After you confirm deletion, the app attempts to send the snapshot to the Windows Recycle Bin.

Close the game before restoring and review the original save paths shown in the confirmation dialog. Restore replaces the listed files or directories in full. Files absent from the snapshot are removed from any directory being replaced.

## Move snapshots manually

Use File Explorer to copy each complete snapshot subfolder, including its `.save-manager.json` file. Choose the new snapshot root under Edit Configuration (「编辑配置」). The app scans the selected folder and lists the snapshots it recognizes. Game configurations are stored in `%LOCALAPPDATA%\SaveManager`.

## Build from source

Use the Rust MSVC toolchain on Windows and install the Desktop development with C++ workload in Visual Studio Build Tools.

```powershell
cargo build --release --locked
cargo test --locked
```

The executable is at `target\release\save-manager.exe`.

The interface workflow draws inspiration from [game-save-manager](https://github.com/mcthesw/game-save-manager).
