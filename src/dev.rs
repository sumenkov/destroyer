use libc::c_int;
use std::ffi::CString;
use std::fs::File;
use std::io::{self, Seek, SeekFrom};
use std::os::fd::{AsRawFd, FromRawFd};

/// Режим синхронизации.
#[derive(Clone, Copy)]
pub enum SyncMode {
    /// Быстро: минимальные барьеры.
    Fast,
    /// Надёжно: O_SYNC (Linux); на macOS используем F_NOCACHE + F_FULLFSYNC при вызове full_sync().
    #[cfg(feature = "durable")]
    Durable,
    /// Прямой I/O: Linux O_DIRECT (требует выровненных буферов/длин/смещений).
    #[cfg(feature = "direct")]
    Direct,
}

impl SyncMode {
    pub fn label(self) -> &'static str {
        match self {
            SyncMode::Fast => "fast",
            #[cfg(feature = "durable")]
            SyncMode::Durable => "durable",
            #[cfg(feature = "direct")]
            SyncMode::Direct => "direct",
        }
    }

    pub fn is_durable(self) -> bool {
        #[cfg(feature = "durable")]
        {
            matches!(self, SyncMode::Durable)
        }
        #[cfg(not(feature = "durable"))]
        {
            false
        }
    }

    pub fn is_direct(self) -> bool {
        #[cfg(feature = "direct")]
        {
            matches!(self, SyncMode::Direct)
        }
        #[cfg(not(feature = "direct"))]
        {
            false
        }
    }
}

/// Открыть устройство на запись с нужной политикой.
pub fn open_device_writable(dev_path: &str, mode: SyncMode) -> io::Result<File> {
    #[cfg(target_os = "linux")]
    {
        use libc::{O_DIRECT, O_SYNC, O_WRONLY, open};
        let c: CString = path_to_cstring(dev_path)?;
        let mut flags: c_int = O_WRONLY;
        match mode {
            SyncMode::Fast => {}
            #[cfg(feature = "durable")]
            SyncMode::Durable => {
                flags |= O_SYNC;
            }
            #[cfg(feature = "direct")]
            SyncMode::Direct => {
                flags |= O_DIRECT;
            }
        };
        let fd: c_int = unsafe { open(c.as_ptr(), flags, 0) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut f: File = unsafe { File::from_raw_fd(fd) };
        f.seek(SeekFrom::Start(0))?;
        Ok(f)
    }

    #[cfg(target_os = "macos")]
    {
        use libc::{F_NOCACHE, O_WRONLY, fcntl, open};
        let c: CString = path_to_cstring(dev_path)?;
        let fd: c_int = unsafe { open(c.as_ptr(), O_WRONLY, 0) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let mut f: File = unsafe { File::from_raw_fd(fd) };

        // Не засоряем page cache (актуально для «сырых» устройств).
        unsafe {
            let _ = fcntl(f.as_raw_fd(), F_NOCACHE, 1);
        }

        // if let SyncMode::Direct = mode {
        //     // На macOS прямой O_DIRECT-эквивалент для блочных устройств отсутствует.
        //     return Err(io::Error::new(io::ErrorKind::Other, "Режим 'direct' доступен только на Linux (O_DIRECT)"));
        // }

        f.seek(SeekFrom::Start(0))?;
        let _: SyncMode = mode; // управление барьерами делаем через full_sync()/safe_sync()
        Ok(f)
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "Поддерживаются только Linux и macOS",
        ))
    }
}

/// Мягкая синхронизация: игнорирует «не поддерживается» на сырых девайсах.
pub fn safe_sync(file: &File) -> io::Result<()> {
    match file.sync_all() {
        Ok(()) => Ok(()),
        Err(e) => match e.raw_os_error() {
            Some(code) if code == libc::ENOTTY || code == libc::ENOTSUP || code == libc::EINVAL => {
                Ok(())
            }
            _ => Err(e),
        },
    }
}

/// Жёсткая синхронизация:
/// - Linux: обычный fsync.
/// - macOS: fcntl(F_FULLFSYNC) — честный flush, очень дорого, вызывать после прохода.
#[cfg(feature = "durable")]
pub fn full_sync(file: &File) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        file.sync_all()
    }

    #[cfg(target_os = "macos")]
    {
        use libc::{F_FULLFSYNC, fcntl};
        let rc: c_int = unsafe { fcntl(file.as_raw_fd(), F_FULLFSYNC) };
        if rc == -1 {
            // fallback + мягкая обработка неподдерживаемых ошибок
            match file.sync_all() {
                Ok(()) => Ok(()),
                Err(e) => match e.raw_os_error() {
                    Some(code)
                        if code == libc::ENOTTY
                            || code == libc::ENOTSUP
                            || code == libc::EINVAL =>
                    {
                        Ok(())
                    }
                    _ => Err(e),
                },
            }
        } else {
            Ok(())
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "Поддерживаются только Linux и macOS",
        ))
    }
}

#[cfg(not(feature = "durable"))]
pub fn full_sync(_file: &File) -> io::Result<()> {
    Ok(())
}

/// Выровненный буфер под O_DIRECT: адрес и длина кратны `align` (обычно сектору: 4096 и т.п.)
#[cfg(all(target_os = "linux", feature = "direct"))]
pub fn alloc_aligned(len: usize, align: usize) -> io::Result<Box<[u8]>> {
    use libc::posix_memalign;
    #[cfg(debug_assertions)]
    debug_assert!(align.is_power_of_two(), "align должен быть степенью двойки");
    if !align.is_power_of_two() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "alignment must be a power of two",
        ));
    }
    let mut ptr: *mut libc::c_void = std::ptr::null_mut();
    let rc: c_int = unsafe { posix_memalign(&mut ptr, align, len) };
    if rc != 0 {
        return Err(io::Error::from_raw_os_error(rc));
    }
    // Инициализируем нулями, выше по стеку можно заполнить случайными данными.
    unsafe {
        std::ptr::write_bytes(ptr, 0, len);
    }
    let slice: &mut [u8] = unsafe { std::slice::from_raw_parts_mut(ptr as *mut u8, len) };
    Ok(unsafe { Box::from_raw(slice) })
}

#[cfg(not(all(target_os = "linux", feature = "direct")))]
pub fn alloc_aligned(len: usize, _align: usize) -> io::Result<Box<[u8]>> {
    // На не-Linux O_DIRECT не используем — вернём обычный буфер.
    let v: Box<[u8]> = vec![0u8; len].into_boxed_slice();
    Ok(v)
}

fn path_to_cstring(dev_path: &str) -> io::Result<CString> {
    CString::new(dev_path).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "device path contains interior NUL byte",
        )
    })
}

/// Размеры блока (логический и физический) в байтах.
#[derive(Debug, Clone, Copy)]
pub struct BlockSizes {
    pub logical: u32,
    pub physical: u32,
}

impl BlockSizes {
    /// Наиболее строгий размер сектора — максимум из logical и physical.
    pub fn sector(&self) -> u32 {
        self.logical.max(self.physical)
    }
}

/// Определить размеры блоков устройства.
#[cfg(target_os = "linux")]
pub fn get_block_sizes(dev_path: &str) -> io::Result<BlockSizes> {
    use std::io;
    use std::path::Path;

    let dev_name: String = Path::new(dev_path)
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "bad device path"))?
        .to_string_lossy()
        .into_owned();

    let l_path: String = format!("/sys/class/block/{}/queue/logical_block_size", dev_name);
    let p_path: String = format!("/sys/class/block/{}/queue/physical_block_size", dev_name);

    let logical: u32 = std::fs::read_to_string(&l_path)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(512);
    let physical: u32 = std::fs::read_to_string(&p_path)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .unwrap_or(logical.max(512));

    Ok(BlockSizes { logical, physical })
}

#[cfg(target_os = "macos")]
pub fn get_block_sizes(dev_path: &str) -> io::Result<BlockSizes> {
    use libc::{c_ulong, ioctl};
    use std::fs::File;

    const DKIOCGETBLOCKSIZE: c_ulong = 0x4004_6418; // _IOR('d', 24, u32)

    let f: File = File::open(dev_path)?;
    let fd = f.as_raw_fd();
    let mut block_size: u32 = 0;
    let rc: c_int = unsafe { ioctl(fd, DKIOCGETBLOCKSIZE, &mut block_size) };
    if rc < 0 || block_size == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(BlockSizes {
        logical: block_size,
        physical: block_size,
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn get_block_sizes(_dev_path: &str) -> io::Result<BlockSizes> {
    Err(io::Error::new(
        io::ErrorKind::Other,
        "Поддерживаются только Linux и macOS",
    ))
}

/// Подбор размера буфера с учётом блоков.
/// - кратность sector = max(logical, physical)
/// - если `requested` = None → целимся в 64 KiB; ограничиваем [16 KiB .. 1 MiB]
/// - если `requested` задан → нормализуем (кратно сектору) и ограничиваем диапазон
pub fn choose_buffer_size(sizes: BlockSizes, requested: Option<usize>) -> usize {
    let sector: usize = sizes.sector() as usize;
    let mut target: usize = requested.unwrap_or(64 * 1024);

    let min_b: usize = 16 * 1024;
    let max_b: usize = 1024 * 1024;
    if target < min_b {
        target = min_b;
    }
    if target > max_b {
        target = max_b;
    }

    let rem: usize = target % sector;
    if rem != 0 {
        target += sector - rem;
    }
    target
}

/// Получить размер блочного устройства в байтах (ioctl).
#[cfg(target_os = "linux")]
pub fn get_device_size_bytes(dev_path: &str) -> std::io::Result<u64> {
    use libc::{c_ulong, ioctl};
    use std::fs::File;
    use std::io;
    use std::path::Path;

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
    let dev_name: String = Path::new(dev_path)
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "bad device path"))?
        .to_string_lossy()
        .into_owned();

    let size_path: String = format!("/sys/class/block/{}/size", dev_name);
    let sectors: u64 = std::fs::read_to_string(&size_path)?
        .trim()
        .parse()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad size in sysfs"))?;

    Ok(sectors.saturating_mul(512))
}

#[cfg(target_os = "macos")]
pub fn get_device_size_bytes(dev_path: &str) -> io::Result<u64> {
    use libc::{c_ulong, ioctl};
    use std::fs::File;

    // Darwin ioctl-константы (_IOR('d', N, T))
    const DKIOCGETBLOCKSIZE: c_ulong = 0x4004_6418; // _IOR('d', 24, u32)
    const DKIOCGETBLOCKCOUNT: c_ulong = 0x4008_6419; // _IOR('d', 25, u64)

    let f: File = File::open(dev_path)?;
    let fd = f.as_raw_fd();

    let mut block_size: u32 = 0;
    let mut block_count: u64 = 0;

    let r1: c_int = unsafe { ioctl(fd, DKIOCGETBLOCKSIZE, &mut block_size) };
    if r1 < 0 {
        return Err(io::Error::last_os_error());
    }
    let r2: c_int = unsafe { ioctl(fd, DKIOCGETBLOCKCOUNT, &mut block_count) };
    if r2 < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(block_count.saturating_mul(block_size as u64))
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn get_device_size_bytes(_dev_path: &str) -> io::Result<u64> {
    Err(io::Error::new(
        io::ErrorKind::Other,
        "Поддерживаются только Linux и macOS",
    ))
}
