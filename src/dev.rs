use std::fs::File;
use std::io::{self, Seek, SeekFrom};
use std::os::fd::{AsRawFd, FromRawFd};
use std::ffi::CString;

/// Режим синхронизации.
#[derive(Clone, Copy)]
pub enum SyncMode {
    /// Быстро: минимальные барьеры.
    Fast,
    /// Надёжно: O_SYNC (Linux); на macOS используем F_NOCACHE + F_FULLFSYNC при вызове full_sync().
    Durable,
}

/// Открыть устройство на запись с нужной политикой.
pub fn open_device_writable(dev_path: &str, mode: SyncMode) -> io::Result<File> {
    #[cfg(target_os = "linux")]
    {
        use libc::{open, O_WRONLY, O_SYNC};
        let c = CString::new(dev_path).unwrap();
        let mut flags = O_WRONLY;
        if let SyncMode::Durable = mode {
            flags |= O_SYNC;
        }
        let fd = unsafe { open(c.as_ptr(), flags, 0) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut f = unsafe { File::from_raw_fd(fd) };
        f.seek(SeekFrom::Start(0))?;
        Ok(f)
    }

    #[cfg(target_os = "macos")]
    {
        use libc::{fcntl, open, F_NOCACHE, O_WRONLY};
        let c = CString::new(dev_path).unwrap();
        let fd = unsafe { open(c.as_ptr(), O_WRONLY, 0) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut f = unsafe { File::from_raw_fd(fd) };

        // Не засоряем page cache (актуально для «сырых» устройств).
        unsafe { let _ = fcntl(f.as_raw_fd(), F_NOCACHE, 1); }

        f.seek(SeekFrom::Start(0))?;
        let _ = mode; // управление барьерами делаем через full_sync()/safe_sync()
        Ok(f)
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Err(io::Error::new(io::ErrorKind::Other, "Поддерживаются только Linux и macOS"))
    }
}

/// Мягкая синхронизация: игнорирует «не поддерживается» на сырых девайсах.
pub fn safe_sync(file: &File) -> io::Result<()> {
    match file.sync_all() {
        Ok(()) => Ok(()),
        Err(e) => match e.raw_os_error() {
            Some(code) if code == libc::ENOTTY || code == libc::ENOTSUP || code == libc::EINVAL => Ok(()),
            _ => Err(e),
        },
    }
}

/// Жёсткая синхронизация:
/// - Linux: обычный fsync.
/// - macOS: fcntl(F_FULLFSYNC) — честный flush, очень дорого, вызывать после прохода.
pub fn full_sync(file: &File) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        file.sync_all()
    }

    #[cfg(target_os = "macos")]
    {
        use libc::{fcntl, F_FULLFSYNC};
        let rc = unsafe { fcntl(file.as_raw_fd(), F_FULLFSYNC) };
        if rc == -1 {
            // fallback + мягкая обработка неподдерживаемых ошибок
            match file.sync_all() {
                Ok(()) => Ok(()),
                Err(e) => match e.raw_os_error() {
                    Some(code) if code == libc::ENOTTY || code == libc::ENOTSUP || code == libc::EINVAL => Ok(()),
                    _ => Err(e),
                }
            }
        } else {
            Ok(())
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Err(io::Error::new(io::ErrorKind::Other, "Поддерживаются только Linux и macOS"))
    }
}

/// Получить размер блочного устройства в байтах (ioctl).
#[cfg(target_os = "linux")]
pub fn get_device_size_bytes(dev_path: &str) -> std::io::Result<u64> {
    use std::fs::File;
    use std::io;
    use std::os::fd::AsRawFd;
    use std::path::Path;
    use libc::{ioctl, c_ulong};

    // BLKGETSIZE64 = _IOR(0x12, 114, size_t) -> 0x80081272 на Linux
    const BLKGETSIZE64: c_ulong = 0x8008_1272;

    // 1) Пытаемся через ioctl
    if let Ok(f) = File::open(dev_path) {
        let fd = f.as_raw_fd();
        let mut size: u64 = 0;
        let rc = unsafe { ioctl(fd, BLKGETSIZE64, &mut size) };
        if rc == 0 && size > 0 {
            return Ok(size);
        }
        // если ioctl вернул ошибку — пойдём во fallback
    }

    // 2) Fallback: читаем sysfs: /sys/class/block/<dev>/size (в 512-байтных секторах)
    let dev_name = Path::new(dev_path)
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "bad device path"))?
        .to_string_lossy()
        .into_owned();

    let size_path = format!("/sys/class/block/{}/size", dev_name);
    let sectors: u64 = std::fs::read_to_string(&size_path)?
        .trim()
        .parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad size in sysfs"))?;

    Ok(sectors.saturating_mul(512))
}

#[cfg(target_os = "macos")]
pub fn get_device_size_bytes(dev_path: &str) -> io::Result<u64> {
    use std::os::fd::AsRawFd;
    use std::fs::File;
    use libc::{c_ulong, ioctl};

    // Darwin ioctl-константы (_IOR('d', N, T))
    const DKIOCGETBLOCKSIZE: c_ulong  = 0x4004_6418; // _IOR('d', 24, u32)
    const DKIOCGETBLOCKCOUNT: c_ulong = 0x4008_6419; // _IOR('d', 25, u64)

    let f = File::open(dev_path)?;
    let fd = f.as_raw_fd();

    let mut block_size: u32 = 0;
    let mut block_count: u64 = 0;

    let r1 = unsafe { ioctl(fd, DKIOCGETBLOCKSIZE, &mut block_size) };
    if r1 < 0 {
        return Err(io::Error::last_os_error());
    }
    let r2 = unsafe { ioctl(fd, DKIOCGETBLOCKCOUNT, &mut block_count) };
    if r2 < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(block_count.saturating_mul(block_size as u64))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn get_device_size_bytes(_dev_path: &str) -> io::Result<u64> {
    Err(io::Error::new(io::ErrorKind::Other, "Поддерживаются только Linux и macOS"))
}
