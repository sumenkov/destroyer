# destroyer (Rust)

> ⚠️ **危险**：该工具会**不可逆**地覆盖指定块设备。请仔细确认 `/dev/...` 或 `\\.\PhysicalDriveN` 路径并确保设备已**卸载**。  
> **支持平台：** Linux、macOS 与 Windows 10/11。

[English](README.en.md) · [Русский](README.md)

---

## 目录
- [简介](#简介)
- [安全须知](#安全须知)
- [支持平台](#支持平台)
- [安装 Rust 与 Cargo](#安装-rust-与-cargo)
- [构建](#构建)
- [运行](#运行)
- [模式](#模式)
- [直接 IO (Linux/Windows)](#直接-io-linuxwindows)
- [示例](#示例)
- [卸载 / 释放设备](#卸载--释放设备)
- [调优与性能](#调优与性能)
- [故障排查](#故障排查)
- [架构](#架构)
- [开发流程](#开发流程)
- [Features](#features)
- [许可证](#许可证)

## 简介
`destroyer` 是一个多遍写入的安全擦除工具，用于 **块设备**。它会使用系统 CSPRNG 进行若干遍加密强度随机写入（Linux/macOS: `/dev/urandom`，Windows: `BCryptGenRandom`），并以全零写入收尾。

- **每一遍**都会重新生成随机缓冲区。
- 最后一遍写入 **零**。
- 在 Linux/macOS/Windows 上能正确获取设备大小。

## 安全须知
- 错误的目标设备会导致数据**永久丢失**。
- 运行前务必先卸载设备（Windows: 将磁盘置为 offline）。
- 建议对**整盘**操作（如 `/dev/sdX`、`/dev/nvme0n1`、`/dev/diskN`、`\\.\PhysicalDriveN`），而不是已挂载分区。
- 需要 root 权限（`sudo`）或 Windows 管理员权限。

## 支持平台
- **Linux**：支持。
- **macOS**：支持。
- **Windows 10/11**：支持。
- **其他系统**：不支持。

## 安装 Rust 与 Cargo
**推荐（rustup）：**
```bash
# Linux / macOS:
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# 重新加载 shell：
source $HOME/.cargo/env
rustc --version && cargo --version
```

**macOS（可选，Homebrew）：**
```bash
brew install rustup-init
rustup-init -y
source $HOME/.cargo/env
```

**Windows（PowerShell）：**
```powershell
# 1) 从 https://rustup.rs 下载 rustup-init.exe 并运行：
.\rustup-init.exe
# 2) 重新打开终端并验证：
rustc --version
cargo --version
```

## 构建
**标准发布版**
```bash
cargo build --release
```

**生产（nightly，panic-abort std）**
```bash
# 需先执行：rustup component add rust-src --toolchain nightly-<triple>
cargo +nightly build --release -Zbuild-std=std,panic_abort
```

## 运行
```bash
# Linux / macOS:
sudo target/release/destroyer <设备> [遍数] [--mode fast|durable|direct] [--buf BYTES]

# Windows (PowerShell):
target\\release\\destroyer.exe \\\\.\\PhysicalDriveN [遍数] [--mode fast|durable|direct] [--buf BYTES]
```

### 参数
- `<设备>` —— 块设备路径（Linux：`/dev/sdX`、`/dev/nvme0n1`；macOS：`/dev/diskN`；Windows：`\\.\PhysicalDriveN`）。
- `[遍数]` —— 遍历次数，默认 **8**（最后一遍写零）。
- `--mode` —— `fast`（默认）| `durable`（需启用 `durable` feature）| `direct`（Linux/Windows）。
- `--buf BYTES` —— 写入缓冲区大小。未指定时将**自动选择**：
  基于设备块大小对齐到扇区，目标约 **64 KiB**（并限制在 **16 KiB..1 MiB** 范围）。
- `--quiet` —— 关闭进度输出（控制台更安静，也能略微提升性能）。
- `--mode direct` —— 仅限 Linux/Windows，且需要 `direct` feature；使用 O_DIRECT（Linux）/ NO_BUFFERING（Windows）绕过页缓存。
- `--quiet` —— 关闭进度输出（控制台更安静，也能略微提升性能）。

## 模式
- `fast` —— 速度优先。
- `durable` —— 更高可靠性（需要 `durable` feature）：
  - **Linux**：以 `O_SYNC` 打开（每次 `write()` 等待数据稳定落盘）。
  - **macOS**：关闭缓存（`F_NOCACHE`）并在遍历结束时用 `F_FULLFSYNC` 进行强制刷新。
  - **Windows**：`WRITE_THROUGH` + `FlushFileBuffers`（每遍结束刷新）。

## 直接 IO (Linux/Windows)
`--mode direct` 在 Linux 使用 **O_DIRECT**，在 Windows 使用 **NO_BUFFERING** 绕过页面缓存，避免大规模顺序写入污染系统缓存。

约束：
- **缓冲区地址**与**长度**需按扇区对齐（通常 4096B）。
- 写入**偏移**也必须按扇区对齐。
- 若存在不对齐的**尾部**，程序会使用第二个非 O_DIRECT 句柄补写，保证整盘都被覆盖。
- 在 **Windows** 上使用 `FILE_FLAG_NO_BUFFERING`，对齐要求相同。
- 在 **macOS** 上，`--mode direct` **不可用**，会给出明确报错。

提示：除非明确需要，通常无需指定 `--buf`；工具会自动选择按扇区对齐且约 **64 KiB** 的缓冲区。

## 自动选择缓冲区
在 Linux 上读取 `/sys/class/block/<dev>/queue/{logical_block_size,physical_block_size}`；
在 macOS 上使用 `DKIOCGETBLOCKSIZE`；
在 Windows 上使用 `IOCTL_STORAGE_QUERY_PROPERTY`（alignment），失败时回退到 `IOCTL_DISK_GET_DRIVE_GEOMETRY`。
随后缓冲区会选择为 `max(logical, physical)` 的整数倍，
目标约 **64 KiB**（限制为 **16 KiB..1 MiB**）。
若传入 `--buf`，该值会被规范化为对齐到扇区并限制在同一范围。

## 示例
```bash
sudo target/release/destroyer /dev/sdX
sudo target/release/destroyer /dev/sdX 5 --mode durable --buf 65536
sudo target/release/destroyer /dev/diskN 3 --mode fast
target\\release\\destroyer.exe \\\\.\\PhysicalDrive2 3 --mode direct
```

## 卸载 / 释放设备
**macOS**
```bash
diskutil unmountDisk /dev/diskN
```

**Linux**
```bash
sudo umount <挂载点或/dev/...>
# 查找挂载：
lsblk -f | grep $(basename /dev/sdX)
# 谁在占用：
sudo lsof /dev/sdX | head
sudo fuser -mv /dev/sdX
# 若为交换分区：
sudo swapoff -a
# LVM / dm-crypt：
sudo dmsetup ls
```

**Windows**
```powershell
diskpart
list disk
select disk N
offline disk
exit
```
替代方案：磁盘管理 → 选择磁盘 → Offline。

## 调优与性能
- 将 `--buf` 提高到 64KiB 或 1MiB，顺序写性能可能更好。
- `durable` 模式会**更慢**（因为屏障/刷新更多）。
- 在 macOS 上建议使用整盘节点（如 `/dev/diskN`）。

## 故障排查
- **`EBUSY`（设备忙）**：设备被挂载或进程占用，参见上文卸载步骤。
- **同步时报 `Inappropriate ioctl for device`**：部分原始设备不支持 `fsync`，程序会使用安全的降级处理。
- **Permission denied**：使用 `sudo` 或 Windows 管理员运行。
- **Windows `Access denied` / `Sharing violation`**：磁盘未 offline 或权限不足。

## GUI (目前仅限 Linux)
对于那些更喜欢点击鼠标而不是输入参数的人，有一个使用 egui/eframe
构建的桌面 GUI（纯 Rust，无 GTK 依赖 —— 只需要大多数 Linux 桌面已经具备的常规 X11/Wayland 开发头文件）。

**Build & run:**
```bash
cargo build --release --features gui --bin destroyer-gui
sudo target/release/destroyer-gui
```
（需要 `sudo` 的原因与 CLI 相同 —— 写入原始块设备需要 root 权限。）

**What it does:**
- 从 `/sys/class/block` 列出全盘块设备（包括大小、型号、是机械硬盘还是 SSD，以及根据 `/proc/mounts` 该路径下当前是否挂载了某些内容），或者允许你手动输入路径。
- 允许你设置擦除次数 / 模式（`fast`/`durable`/`direct`）/ 缓冲区大小，与 CLI 相同。
- 在写入任何内容之前，强制要求你**重新输入确切的设备路径**并勾选“我理解这是不可逆的”复选框 —— 没有你可以不小心点过去的倒计时。
- 在后台线程中运行擦除任务，以保持窗口响应，并提供实时的单次擦除及总进度的进度条和预估剩余时间（ETA）。
- 带有 **Cancel**（取消）按钮：它会设置一个标志，写入循环会在缓冲区大小的块（chunks）之间检查该标志（因此它会在大约一个缓冲区的 I/O 范围内停止，虽然不是瞬间停止，但在典型缓冲区大小下绝对会控制在 1 秒以内）。

目前仅对接了 Linux 系统（在其他操作系统上 `src/bin/destroyer_gui.rs` 会打印清晰的提示消息并退出） —— macOS 的设备枚举和 macOS 版本的 GUI 构建将在后续支持。


## 架构
- 核心逻辑（参数解析、设备辅助、写入流程）集中在 `destroyer` 库模块中（`src/args.rs`、`src/dev.rs`、`src/wipe.rs`、`src/app.rs`）。
- 平台特定的运行器位于 `src/platform/`：Linux 使用 `platform::linux::run`，macOS 使用 `platform::macos::run`，Windows 使用 `platform::windows::run`，可在其中添加各自的调试逻辑或额外保护，然后调用共享的 `app::run`。
- 二进制入口 `src/main.rs` 通过 `#[cfg(target_os = "...")]` 在编译期选择对应运行器，因此在某个平台上迭代功能不会影响到另一个平台，除非修改了公共模块。

## 开发流程
- `cargo test --features test-support` —— 启用测试辅助功能的集成测试。
- `cargo clippy --release -- -W clippy::perf` —— 提前发现性能问题。
- `cargo bench --features test-support` —— 运行 Criterion 基准测试，比较不同缓冲区。
- `cargo bloat --release -n 20` —— 监控可执行体大小变化。
- `cargo asm --release destroyer::wipe::pass_random` —— 分析关键路径汇编。

## Features
| Feature        | 默认 | 说明                                  |
|----------------|------|---------------------------------------|
| `durable`      | ✅    | O_SYNC / F_FULLFSYNC 耐久模式。       |
| `direct`       | ✅    | Linux O_DIRECT / Windows NO_BUFFERING。 |
| `test-support` | ❌    | 测试/基准所需的临时文件辅助工具。     |

## 许可证
MIT。以下为便利性翻译：
- [LICENSE（英文）](./LICENSE)
- [LICENSE.ru（俄文，非官方）](./LICENSE.ru)
- [LICENSE.zh-CN（中文，非官方）](./LICENSE.zh-CN)
