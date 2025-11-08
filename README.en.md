# destroyer (Rust)

> ⚠️ DANGER: This utility irreversibly overwrites the specified block device. Make sure the `/dev/...` path is correct and the device is unmounted.

## Features
- Cryptographically secure random data (`/dev/urandom`).
- A new random buffer is generated **for each pass**.
- Final pass writes zeros.
- Operating modes:
  - `fast` (default) — maximum speed;
  - `durable` — higher durability:  
    - Linux: open with `O_SYNC` (each `write()` waits until data is stably written).  
    - macOS: disable caching (`F_NOCACHE`) and perform a hard flush with `F_FULLFSYNC` at the end of a pass.
- Tips for a busy device (`EBUSY`): commands for Linux and macOS.

## Build

```bash
cargo build --release
```

## Run

```bash
# Syntax:
sudo target/release/destroyer <device> [passes] [--mode fast|durable] [--buf BYTES]

# Examples:
sudo target/release/destroyer /dev/sdX
sudo target/release/destroyer /dev/sdX 5 --mode durable --buf 65536
sudo target/release/destroyer /dev/diskN 3 --mode fast
```

## Parameter reference

```
destroyer <device> [passes] [--mode fast|durable] [--buf BYTES]
```

- `<device>` — path to the block device (Linux: `/dev/sdX`, `/dev/nvme0n1`; macOS: `/dev/diskN`).
- `[passes]` — number of passes, default is 8 (the last pass writes zeros).
- `--mode` — `fast` or `durable` (see above), default is `fast`.
- `--buf BYTES` — write buffer size, default is 4096. A larger value (e.g., 65536) may improve throughput on some media.

## Unmounting / freeing the device

- **macOS**: `diskutil unmountDisk /dev/diskN`
- **Linux**:
  - `sudo umount <mountpoint_or_/dev/...>`
  - Find attachments: `lsblk -f | grep $(basename /dev/sdX)`
  - Who holds it: `sudo lsof /dev/sdX | head`, `sudo fuser -mv /dev/sdX`
  - If it’s swap: `sudo swapoff -a`
  - For LVM/dm-crypt: `sudo dmsetup ls`
