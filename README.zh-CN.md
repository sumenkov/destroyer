# destroyer (Rust)

> ⚠️ **危险**：该工具会**不可逆**地覆盖指定块设备。请仔细确认 `/dev/...` 路径并确保设备已**卸载**。  
> **支持平台：** 仅 Linux 与 macOS。

[English](./README.md) · [Русский](./README.ru.md)

---

## 目录
- [简介](#简介)
- [安全须知](#安全须知)
- [支持平台](#支持平台)
- [安装 Rust 与 Cargo](#安装-rust-与-cargo)
- [构建](#构建)
- [运行](#运行)
- [模式](#模式)
- [直接 I/O（Linux O_DIRECT）](#直接-io-linux-o_direct)
- [示例](#示例)
- [卸载 / 释放设备](#卸载--释放设备)
- [调优与性能](#调优与性能)
- [故障排查](#故障排查)
- [许可证](#许可证)

## 简介
`destroyer` 是一个多遍写入的安全擦除工具，用于 **块设备**。它会进行若干遍来自 `/dev/urandom` 的加密强度随机写入，并以全零写入收尾。

- **每一遍**都会重新生成随机缓冲区。
- 最后一遍写入 **零**。
- 在 Linux/macOS 上能正确获取设备大小。

## 安全须知
- 错误的目标设备会导致数据**永久丢失**。
- 运行前务必先卸载设备。
- 建议对**整盘**操作（如 `/dev/sdX`、`/dev/nvme0n1`、`/dev/diskN`），而不是已挂载分区。
- 需要 root 权限（`sudo`）。

## 支持平台
- **Linux**：支持。
- **macOS**：支持。
- **Windows / 其他**：不支持。

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

## 构建
```bash
cargo build --release
# 生产（nightly）构建：panic-abort std
cargo +nightly build --release -Zbuild-std=std,panic_abort
```

## 运行
```bash
sudo target/release/destroyer <设备> [遍数] [--mode fast|durable] [--buf BYTES]
```

### 参数
- `<设备>` —— 块设备路径（Linux：`/dev/sdX`、`/dev/nvme0n1`；macOS：`/dev/diskN`）。
- `[遍数]` —— 遍历次数，默认 **8**（最后一遍写零）。
- `--mode` —— `fast`（默认）或 `durable`（见下）。
- `--buf BYTES` —— 写入缓冲区大小。未指定时将**自动选择**：
  基于设备块大小对齐到扇区，目标约 **64 KiB**（并限制在 **16 KiB..1 MiB** 范围）。
- `--quiet` —— 关闭进度输出（控制台更安静，也能略微提升性能）。

## 模式
- `fast` —— 速度优先。
- `durable` —— 更高可靠性：
  - **Linux**：以 `O_SYNC` 打开（每次 `write()` 等待数据稳定落盘）。
  - **macOS**：关闭缓存（`F_NOCACHE`）并在遍历结束时用 `F_FULLFSYNC` 进行强制刷新。

## 直接 I/O（Linux O_DIRECT）
`--mode direct` 使用 Linux **O_DIRECT** 绕过页面缓存，避免大规模顺序写入污染系统缓存。

约束：
- **缓冲区地址**与**长度**需按扇区对齐（通常 4096B）。
- 写入**偏移**也必须按扇区对齐。
- 若存在不对齐的**尾部**，程序会使用第二个非 O_DIRECT 句柄补写，保证整盘都被覆盖。
- 在 **macOS** 上，`--mode direct` **不可用**，会给出明确报错。

提示：除非明确需要，通常无需指定 `--buf`；工具会自动选择按扇区对齐且约 **64 KiB** 的缓冲区。

## 自动选择缓冲区
在 Linux 上读取 `/sys/class/block/<dev>/queue/{logical_block_size,physical_block_size}`；
在 macOS 上使用 `DKIOCGETBLOCKSIZE`。随后缓冲区会选择为 `max(logical, physical)` 的整数倍，
目标约 **64 KiB**（限制为 **16 KiB..1 MiB**）。
若传入 `--buf`，该值会被规范化为对齐到扇区并限制在同一范围。

## 示例
```bash
sudo target/release/destroyer /dev/sdX
sudo target/release/destroyer /dev/sdX 5 --mode durable --buf 65536
sudo target/release/destroyer /dev/diskN 3 --mode fast
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

## 调优与性能
- 将 `--buf` 提高到 64KiB 或 1MiB，顺序写性能可能更好。
- `durable` 模式会**更慢**（因为屏障/刷新更多）。
- 在 macOS 上建议使用整盘节点（如 `/dev/diskN`）。

## 故障排查
- **`EBUSY`（设备忙）**：设备被挂载或进程占用，参见上文卸载步骤。
- **同步时报 `Inappropriate ioctl for device`**：部分原始设备不支持 `fsync`，程序会使用安全的降级处理。
- **Permission denied**：使用 `sudo` 运行。

## 架构
- 核心逻辑（参数解析、设备辅助、写入流程）集中在 `destroyer` 库模块中（`src/args.rs`、`src/dev.rs`、`src/wipe.rs`、`src/app.rs`）。
- 平台特定的运行器位于 `src/platform/`：Linux 使用 `platform::linux::run`，macOS 使用 `platform::macos::run`，可在其中添加各自的调试逻辑或额外保护，然后调用共享的 `app::run`。
- 二进制入口 `src/main.rs` 通过 `#[cfg(target_os = "...")]` 在编译期选择对应运行器，因此在某个平台上迭代功能不会影响到另一个平台，除非修改了公共模块。

## 许可证
MIT。以下为便利性翻译：
- [LICENSE（英文）](./LICENSE)
- [LICENSE.ru（俄文，非官方）](./LICENSE.ru)
- [LICENSE.zh-CN（中文，非官方）](./LICENSE.zh-CN)
- `cargo test --features test-support` —— 启用测试辅助功能的集成测试。
- `cargo clippy --release -- -W clippy::perf` —— 提前发现性能问题。
- `cargo bench --features test-support` —— 运行 Criterion 基准测试，比较不同缓冲区。
