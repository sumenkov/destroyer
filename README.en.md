# destroyer (Rust)

> ⚠️ **DANGER**: This utility irreversibly overwrites the specified block device. Double‑check your `/dev/...` or `\\.\PhysicalDriveN` path and make sure the device is **unmounted** before running.  
> **Supported OS:** Linux, macOS, and Windows 10/11.

[Русский](README.md) · [中文](./README.zh-CN.md)

---

## Table of Contents
- [What is it?](#what-is-it)
- [Safety First](#safety-first)
- [Supported Platforms](#supported-platforms)
- [Install Rust & Cargo](#install-rust--cargo)
- [Build](#build)
- [Usage](#usage)
- [Modes](#modes)
- [Direct I/O (Linux/Windows)](#direct-io-linuxwindows)
- [Examples](#examples)
- [Un-mounting / Freeing a Device](#un-mounting--freeing-a-device)
- [Tuning & Performance](#tuning--performance)
- [Troubleshooting](#troubleshooting)
- [Architecture](#architecture)
- [Development Workflow](#development-workflow)
- [License](#license)

## What is it?
`destroyer` is a secure multi-pass disk wiper for **block devices**. It writes cryptographically secure random data from the system CSPRNG (Linux/macOS: `/dev/urandom`, Windows: `BCryptGenRandom`) for several passes and finishes with a pass of zeros.

- New random buffer is generated **for each pass**.
- Final pass writes **zeros**.
- Linux/macOS/Windows support with proper device-size detection.

## Safety First
- Running this on the wrong device will **destroy data permanently**.
- Always unmount the device first (Windows: take the disk offline).
- Prefer running on the **whole device** (e.g., `/dev/sdX`, `/dev/nvme0n1`, `/dev/diskN`, `\\.\PhysicalDriveN`), not a mounted partition.
- Requires root privileges (`sudo`) or Administrator on Windows.

## Supported Platforms
- **Linux**: supported.
- **macOS**: supported.
- **Windows 10/11**: supported.
- **Other OS**: not supported.

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

**Windows (PowerShell):**
```powershell
# 1) Download rustup-init.exe from https://rustup.rs and run:
.\rustup-init.exe
# 2) Restart your terminal and verify:
rustc --version
cargo --version
```

## Build
**Standard release build**
```bash
cargo build --release
```

**Nightly build with panic-abort std**
```bash
# Requires: rustup component add rust-src --toolchain nightly-<triple>
cargo +nightly build --release -Zbuild-std=std,panic_abort
```

## Usage
```bash
# Linux / macOS:
sudo target/release/destroyer <device> [passes] [--mode fast|durable|direct] [--buf BYTES]

# Windows (PowerShell):
target\\release\\destroyer.exe \\\\.\\PhysicalDriveN [passes] [--mode fast|durable|direct] [--buf BYTES]
```

### Parameters
- `<device>` — path to the block device (Linux: `/dev/sdX`, `/dev/nvme0n1`; macOS: `/dev/diskN`; Windows: `\\.\PhysicalDriveN`).
- `[passes]` — number of passes, default **8** (the last pass writes zeros).
- `--mode` — `fast` (default) | `durable` (see below; requires the `durable` feature) | `direct` (Linux/Windows).
- `--buf BYTES` — write buffer size. If omitted, buffer size is **chosen automatically**
  based on the device block size (aligned to sector; ~64 KiB target within 16 KiB..1 MiB).
- `--quiet` — suppress progress output (slightly faster, less console noise).
- `--mode direct` — Linux/Windows only (requires the `direct` feature); bypasses page cache via O_DIRECT (Linux) / NO_BUFFERING (Windows).

## Modes
- `fast` — speed oriented.
- `durable` — higher durability (available only when the `durable` feature is enabled):
  - **Linux**: open device with `O_SYNC` (each `write()` waits until data is stable on the device).
  - **macOS**: disable caching (`F_NOCACHE`) and perform a hard flush with `F_FULLFSYNC` at the end of each pass.
  - **Windows**: `WRITE_THROUGH` + `FlushFileBuffers` at the end of each pass.

## Direct I/O (Linux/Windows)
`--mode direct` uses **O_DIRECT** on Linux or **NO_BUFFERING** on Windows to bypass the page cache. This avoids polluting the system cache during large sequential writes.

Constraints:
- Buffer **address** and **length** must be aligned to the device sector (commonly 4096B).
- Write **offsets** must be sector-aligned as well.
- The tool handles alignment and will write any non-aligned **tail** using a secondary non-O_DIRECT handle, so the whole device is still overwritten.
- On **Windows**, `FILE_FLAG_NO_BUFFERING` is used with the same alignment requirements.
- On **macOS**, `--mode direct` is **not available** and will error with a clear message.

Tip: Use `--buf` only if you need a specific size. Otherwise the tool auto-selects a multiple of the sector (~64 KiB target).

## Auto buffer selection
On Linux we read `/sys/class/block/<dev>/queue/{logical_block_size,physical_block_size}`.
On macOS we query `DKIOCGETBLOCKSIZE`.
On Windows we use `IOCTL_STORAGE_QUERY_PROPERTY` (alignment), fallback — `IOCTL_DISK_GET_DRIVE_GEOMETRY`.
The buffer is then selected to be a multiple of `max(logical, physical)` with a target around **64 KiB** (clamped to **16 KiB..1 MiB**).
If you pass `--buf`, your value is normalized to sector alignment and clamped to the same range.

## Examples
```bash
sudo target/release/destroyer /dev/sdX
sudo target/release/destroyer /dev/sdX 5 --mode durable --buf 65536
sudo target/release/destroyer /dev/diskN 3 --mode fast
target\\release\\destroyer.exe \\\\.\\PhysicalDrive2 3 --mode direct
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

**Windows**
```powershell
diskpart
list disk
select disk N
offline disk
exit
```
Alternative: Disk Management → select disk → Offline.

## Tuning & Performance
- Increase `--buf` to 64KiB or 1MiB if the device benefits from larger sequential writes.
- `durable` mode will be **slower** by design (more barriers/flushes).
- On macOS, prefer whole-disk nodes (e.g., `/dev/diskN`).

## Troubleshooting
- **`EBUSY` (Device or resource busy):** the device is mounted or held by a process. See the unmounting section above.
- **`Inappropriate ioctl for device (os error 25)` on sync:** some raw devices don’t support `fsync`. The tool uses safe fallbacks.
- **Permission denied:** run with `sudo` or as Administrator on Windows.
- **Windows `Access denied` / `Sharing violation`:** disk is not offline or insufficient rights.

## GUI (Linux only, for now)
For people who prefer clicking to typing flags, there's a desktop GUI built
with [egui/eframe](https://github.com/emilk/egui) (pure Rust, no GTK
dependency — just the usual X11/Wayland dev headers most Linux desktops
already have).

**Build & run:**
```bash
cargo build --release --features gui --bin destroyer-gui
sudo target/release/destroyer-gui
```
(`sudo` is needed for the same reason as the CLI — writing to a raw block
device requires root.)

**What it does:**
- Lists whole-disk block devices from `/sys/class/block` (size, model,
  rotational/SSD, and whether something under that path is currently
  mounted, per `/proc/mounts`), or lets you type a path manually.
- Lets you set passes / mode (`fast`/`durable`/`direct`) / buffer size, same
  as the CLI.
- Before writing anything, forces you to **retype the exact device path**
  and check an "I understand this is irreversible" box — no timed
  countdown you can accidentally click through.
- Runs the wipe on a background thread so the window stays responsive, with
  live per-pass and total progress bars and ETAs.
- Has a **Cancel** button: it sets a flag that the write loop checks between
  buffer-sized chunks (so it stops within roughly one buffer's worth of I/O,
  not instantly, but well within a second for typical buffer sizes).

Only Linux is wired up right now (`src/bin/destroyer_gui.rs` prints a clear
message and exits on other OSes) — macOS device enumeration and a macOS
build of the GUI would be a follow-up.

## Architecture
- Core logic (argument parsing, device helpers, wiping routines) lives in the `destroyer` library crate (`src/args.rs`, `src/dev.rs`, `src/wipe.rs`, `src/app.rs`).
- Platform-specific runners reside in `src/platform/`. For Linux the entry point is `platform::linux::run`, for macOS — `platform::macos::run`, for Windows — `platform::windows::run`; each can host OS-only setup, debugging flags, or extra safeguards before calling the shared `app::run`.
- The binary `src/main.rs` selects the right runner at compile time via `#[cfg(target_os = "...")]`, so extending behaviour for one OS never affects the other unless you change shared modules explicitly.

## Development Workflow
- **Tests:** `cargo test --features test-support` (helpers stay out of release artifacts).
- **Clippy perf checks:** `cargo clippy --release -- -W clippy::perf`.
- **Benchmarks:** `cargo bench --features test-support` (Criterion suite benchmarking buffer sizes/tail handling).
- **Binary-size guard:** `cargo bloat --release -n 20`.
- **Assembly inspection:** `cargo asm --release destroyer::wipe::pass_random`.

### Feature flags
| Feature        | Default | Purpose                                      |
|----------------|---------|----------------------------------------------|
| `durable`      | ✅      | Enables O_SYNC/F_FULLFSYNC durability mode.  |
| `direct`       | ✅      | Enables Linux O_DIRECT / Windows NO_BUFFERING. |
| `test-support` | ❌      | Pulls in temp-file helpers for tests/bench.  |

## License
MIT License. Translations provided for convenience:
- [LICENSE (English)](./LICENSE)
- [LICENSE.ru (Russian, unofficial)](./LICENSE.ru)
- [LICENSE.zh-CN (Chinese Simplified, unofficial)](./LICENSE.zh-CN)
