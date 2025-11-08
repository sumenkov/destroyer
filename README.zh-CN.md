# destroyer (Rust)

> ⚠️ 危险：本工具会**不可逆**地覆盖指定的块设备。请确保 `/dev/...` 路径正确且设备已卸载（unmount）。

## 功能
- 使用加密强度的随机数据（`/dev/urandom`）。
- **每一次遍历**都会重新生成随机缓冲区。
- 最后一遍写入全零。
- 运行模式：
  - `fast`（默认）—— 速度优先；
  - `durable` —— 更高的可靠性：  
    - Linux：以 `O_SYNC` 打开（每次 `write()` 均等待数据稳定落盘）。  
    - macOS：关闭缓存（`F_NOCACHE`），并在每次遍历结束时使用 `F_FULLFSYNC` 进行硬刷新。
- 设备忙（`EBUSY`）时的提示：包含 Linux 与 macOS 的常用命令。

## 构建

```bash
cargo build --release
```

## 运行

```bash
# 语法：
sudo target/release/destroyer <设备> [遍数] [--mode fast|durable] [--buf BYTES]

# 示例：
sudo target/release/destroyer /dev/sdX
sudo target/release/destroyer /dev/sdX 5 --mode durable --buf 65536
sudo target/release/destroyer /dev/diskN 3 --mode fast
```

## 参数说明

```
destroyer <设备> [遍数] [--mode fast|durable] [--buf BYTES]
```

- `<设备>` —— 块设备路径（Linux：`/dev/sdX`、`/dev/nvme0n1`；macOS：`/dev/diskN`）。
- `[遍数]` —— 遍历次数，默认 8（最后一遍写入零）。
- `--mode` —— `fast` 或 `durable`（见上），默认 `fast`。
- `--buf BYTES` —— 写入缓冲区大小，默认 4096。适当增大（如 65536）在某些介质上可提升吞吐。

## 卸载 / 释放设备

- **macOS**：`diskutil unmountDisk /dev/diskN`
- **Linux**：
  - `sudo umount <挂载点或/dev/...>`
  - 查找挂载：`lsblk -f | grep $(basename /dev/sdX)`
  - 谁在占用：`sudo lsof /dev/sdX | head`，`sudo fuser -mv /dev/sdX`
  - 若为交换分区：`sudo swapoff -a`
  - LVM/dm-crypt：`sudo dmsetup ls`
