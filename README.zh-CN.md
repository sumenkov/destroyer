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
```

## 运行
```bash
sudo target/release/destroyer <设备> [遍数] [--mode fast|durable] [--buf BYTES]
```

### 参数
- `<设备>` —— 块设备路径（Linux：`/dev/sdX`、`/dev/nvme0n1`；macOS：`/dev/diskN`）。
- `[遍数]` —— 遍历次数，默认 **8**（最后一遍写零）。
- `--mode` —— `fast`（默认）或 `durable`（见下）。
- `--buf BYTES` —— 写入缓冲区大小，默认 **4096**。适当增大（如 `65536`）在部分介质上可提升吞吐。

## 模式
- `fast` —— 速度优先。
- `durable` —— 更高可靠性：
  - **Linux**：以 `O_SYNC` 打开（每次 `write()` 等待数据稳定落盘）。
  - **macOS**：关闭缓存（`F_NOCACHE`）并在遍历结束时用 `F_FULLFSYNC` 进行强制刷新。

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

## 许可证
MIT。以下为便利性翻译：
- [LICENSE（英文）](./LICENSE)
- [LICENSE.ru（俄文，非官方）](./LICENSE.ru)
- [LICENSE.zh-CN（中文，非官方）](./LICENSE.zh-CN)
