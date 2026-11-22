# ylrclone

一个专门用来**把 WebDAV 挂成本地盘**的小工具：托盘常驻、开机自启、掉线自动重挂。

底层就是 rclone `mount`，这层壳负责三件事：**配置**（不用再背 rclone 参数）、**进程托管**（不弹黑窗、日志可查）、**状态守护**（死了自动拉起来）。

> Windows 优先。挂载依赖 [WinFsp](https://winfsp.dev/rel/)，**安装前请先装它**，否则挂载会直接报错。

## 功能

- WebDAV 一键挂成盘符（`X:`）或目录（`D:\Mount\dav`）
- 密码用 `rclone obscure` 加密后只写入 `rclone.conf`，`settings.json` 里不存明文
- 挂载失败直接把 rclone 日志末尾摆出来，不用自己翻文件
- 掉线/进程被杀自动重挂（最多连续 3 次，防死循环）
- 崩了不怕：pid 落盘，下次启动认领还在跑的 rclone 进程
- 托盘菜单直接挂载/卸载，关闭窗口只是缩到托盘
- 开机自启（写当前用户 `HKCU\...\Run`，不要管理员权限）

## 安装

1. 先装 WinFsp：<https://winfsp.dev/rel/>（一路下一步即可）
2. 到 [Releases](../../releases) 下载 `ylrclone_x.y.z_x64-setup.exe` 安装
3. 首次启动 → 新建 → 填 WebDAV 地址、账号密码、挂载点 → 保存 → 挂载

程序自带 rclone（安装目录里的 `rclone.exe`），不需要单独装。

## 数据放哪

`%APPDATA%\io.github.ylzha.ylrclone\`（界面右上角「数据目录」直接打开）

| 文件 | 内容 |
|---|---|
| `settings.json` | 挂载项配置（**不含密码**）、自启、看门狗开关 |
| `rclone.conf` | rclone 的配置，密码已 obscure |
| `logs\mount-<名字>.log` | 每个挂载项的 rclone 输出，界面「日志」按钮看的就是它 |
| `logs\ylrclone.log` | 本程序自己的运行日志 |
| `process_state.json` | 进程 pid 记录，用于重启后认领 |

## 它实际执行的命令

```
rclone.exe mount --config <数据目录>\rclone.conf <名字>:<子路径> <挂载点> \
  --vfs-cache-mode=writes [--network-mode] <你填的额外参数>
```

- 没写 `--vfs-cache-mode` 时默认补 `writes`（写入先落缓存，不容易写一半断线）
- 勾了「网络模式」才加 `--network-mode`（Windows 用 `\\server\share` 而不是盘符）
- 额外参数一行一个，比如 `--dir-cache-time=5m`、`--buffer-size=32M`、`--read-only`

## 开发

需要 Node 20+、Rust 1.77+、WinFsp。

```bash
npm install
# 本地跑要先准备 rclone sidecar（CI 会自动做）
# Windows：把 rclone.exe 复制成 src-tauri\binary\rclone-x86_64-pc-windows-msvc.exe
npm run tauri dev
```

打包：`npm run tauri build`，产物在 `src-tauri/target/release/bundle/nsis/`。

CI（`.github/workflows/build.yml`）在 windows-latest 上自动下 rclone、构建、传 artifact；推 `v*` tag 会直接发 Release。

## 代码结构

```
src/                      Vue 3 界面（列表 + 表单 + 日志抽屉）
src-tauri/src/
  mount.rs                拼 rclone 参数、挂载/卸载、挂载点探测
  process.rs              子进程托管：起停、pid 落盘、日志轮转、认领旧进程
  rclone_conf.rs          rclone.conf 的行级编辑（只动目标段）
  settings.rs             配置模型（不含密码）
  autostart.rs            开机自启（Windows 注册表 / Linux autostart）
  tray.rs                 托盘菜单
  winfsp.rs               WinFsp 检测
  lib.rs                  命令注册、看门狗、自动挂载
```

## 与 OpenList-Desktop 的关系

挂载思路与进程托管方式参考了 [OpenListTeam/OpenList-Desktop](https://github.com/OpenListTeam/OpenList-Desktop)（同为 GPL-3.0），本仓库是独立重写的精简版本，只保留「挂载」这一件事，并补了 WinFsp 检测、失败原因回显、看门狗。

## 许可

GPL-3.0-only，见 [LICENSE](LICENSE)。