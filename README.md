# destroyer (Rust)

> ⚠️ **DANGER**: This utility irreversibly overwrites the specified block device. Double‑check your `/dev/...` path and make sure the device is **unmounted** before running.  
> **Supported OS:** Linux and macOS only.

[Русский](./README.ru.md) · [中文](./README.zh-CN.md)

---

## Table of Contents
- [What is it?](#what-is-it)
- [Safety First](#safety-first)
- [Supported Platforms](#supported-platforms)
- [Install Rust & Cargo](#install-rust--cargo)
- [Build](#build)
- [Usage](#usage)
- [Modes](#modes)
- [Direct I/O (Linux O_DIRECT)](#direct-io-linux-o_direct)
- [Examples](#examples)
- [Un-mounting / Freeing a Device](#un-mounting--freeing-a-device)
- [Tuning & Performance](#tuning--performance)
- [Troubleshooting](#troubleshooting)
- [License](#license)

## What is it?
`destroyer` is a secure multi-pass disk wiper for **block devices**. It writes cryptographically secure random data from `/dev/urandom` for several passes and finishes with a pass of zeros.

- New random buffer is generated **for each pass**.
- Final pass writes **zeros**.
- Linux/macOS support with proper device-size detection.

## Safety First
- Running this on the wrong device will **destroy data permanently**.
- Always unmount the device first.
- Prefer running on the **whole device** (e.g., `/dev/sdX`, `/dev/nvme0n1`, `/dev/diskN`), not a mounted partition.
- Requires root privileges (`sudo`).

## Supported Platforms
- **Linux**: supported.
- **macOS**: supported.
- **Windows / others**: not supported.

## Install Rust & Cargo
**Recommended (rustup):**
```bash
# Linux / macOS:
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# then reload shell or:
source $HOME/.cargo/env
rustc --version && cargo --version
```

**macOS via Homebrew (optional):**
```bash
brew install rustup-init
rustup-init -y
source $HOME/.cargo/env
```

## Build
```bash
cargo build --release
```

## Usage
```bash
sudo target/release/destroyer <device> [passes] [--mode fast|durable] [--buf BYTES]
```

### Parameters
- `<device>` — path to the block device (Linux: `/dev/sdX`, `/dev/nvme0n1`; macOS: `/dev/diskN`).
- `[passes]` — number of passes, default **8** (the last pass writes zeros).
- `--mode` — `fast` (default) or `durable` (see below).
- `--buf BYTES` — write buffer size. If omitted, buffer size is **chosen automatically**
  based on the device block size (aligned to sector; ~64 KiB target within 16 KiB..1 MiB).

## Modes
- `fast` — speed oriented.
- `durable` — higher durability:
  - **Linux**: open device with `O_SYNC` (each `write()` waits until data is stable on the device).
  - **macOS**: disable caching (`F_NOCACHE`) and perform a hard flush with `F_FULLFSYNC` at the end of each pass.

## Direct I/O (Linux O_DIRECT)
`--mode direct` uses Linux **O_DIRECT** to bypass the page cache. This avoids polluting the system cache during large sequential writes.

Constraints:
- Buffer **address** and **length** must be aligned to the device sector (commonly 4096B).
- Write **offsets** must be sector-aligned as well.
- The tool handles alignment and will write any non-aligned **tail** using a secondary non-O_DIRECT handle, so the whole device is still overwritten.
- On **macOS**, `--mode direct` is **not available** and will error with a clear message.

Tip: Use `--buf` only if you need a specific size. Otherwise the tool auto-selects a multiple of the sector (~64 KiB target).

## Auto buffer selection
On Linux we read `/sys/class/block/<dev>/queue/{logical_block_size,physical_block_size}`.
On macOS we query `DKIOCGETBLOCKSIZE`. The buffer is then selected to be a multiple of
`max(logical, physical)` with a target around **64 KiB** (clamped to **16 KiB..1 MiB**).
If you pass `--buf`, your value is normalized to sector alignment and clamped to the same range.

## Examples
```bash
sudo target/release/destroyer /dev/sdX
sudo target/release/destroyer /dev/sdX 5 --mode durable --buf 65536
sudo target/release/destroyer /dev/diskN 3 --mode fast
```

## Un-mounting / Freeing a Device
**macOS**
```bash
diskutil unmountDisk /dev/diskN
```

**Linux**
```bash
sudo umount <mountpoint_or_/dev/...>
# Find attachments:
lsblk -f | grep $(basename /dev/sdX)
# Who holds it:
sudo lsof /dev/sdX | head
sudo fuser -mv /dev/sdX
# If swap:
sudo swapoff -a
# LVM / dm-crypt:
sudo dmsetup ls
```

## Tuning & Performance
- Increase `--buf` to 64KiB or 1MiB if the device benefits from larger sequential writes.
- `durable` mode will be **slower** by design (more barriers/flushes).
- On macOS, prefer whole-disk nodes (e.g., `/dev/diskN`).

## Troubleshooting
- **`EBUSY` (Device or resource busy):** the device is mounted or held by a process. See the unmounting section above.
- **`Inappropriate ioctl for device (os error 25)` on sync:** some raw devices don’t support `fsync`. The tool uses safe fallbacks.
- **Permission denied:** run with `sudo`.

## License
MIT License. Translations provided for convenience:
- [LICENSE (English)](./LICENSE)
- [LICENSE.ru (Russian, unofficial)](./LICENSE.ru)
- [LICENSE.zh-CN (Chinese Simplified, unofficial)](./LICENSE.zh-CN)
