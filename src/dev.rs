use libc::c_int;
use std::alloc::{alloc_zeroed, dealloc, Layout};
#[cfg(not(target_os = "windows"))]
use std::ffi::CString;
use std::fs::File;
use std::io::{self, Seek, SeekFrom};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::os::fd::{AsRawFd, FromRawFd};
use std::ptr::NonNull;

#[cfg(target_os = "windows")]
use std::os::windows::io::{AsRawHandle, FromRawHandle, RawHandle};

/// Режим синхронизации.
#[derive(Clone, Copy)]
pub enum SyncMode {
    /// Быстро: минимальные барьеры.
    Fast,
    /// Надёжно: O_SYNC (Linux); на macOS — F_NOCACHE + F_FULLFSYNC; на Windows — WRITE_THROUGH + FlushFileBuffers.
    #[cfg(feature = "durable")]
    Durable,
    /// Прямой I/O: Linux O_DIRECT / Windows NO_BUFFERING (требует выровненных буферов/длин/смещений).
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

#[cfg(target_os = "windows")]
mod win {
    use super::*;
    use std::ffi::{c_void, OsStr};
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null_mut;

    pub type Handle = RawHandle;
    pub const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;

    pub const GENERIC_READ: u32 = 0x8000_0000;
    pub const GENERIC_WRITE: u32 = 0x4000_0000;
    pub const FILE_SHARE_READ: u32 = 0x0000_0001;
    pub const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    pub const OPEN_EXISTING: u32 = 3;
    pub const FILE_ATTRIBUTE_NORMAL: u32 = 0x0000_0080;
    pub const FILE_FLAG_NO_BUFFERING: u32 = 0x2000_0000;
    pub const FILE_FLAG_WRITE_THROUGH: u32 = 0x8000_0000;

    pub const ERROR_SHARING_VIOLATION: i32 = 32;
    pub const ERROR_LOCK_VIOLATION: i32 = 33;
    pub const ERROR_ACCESS_DENIED: i32 = 5;
    pub const ERROR_INVALID_FUNCTION: i32 = 1;
    pub const ERROR_NOT_SUPPORTED: i32 = 50;
    pub const ERROR_INVALID_PARAMETER: i32 = 87;

    const IOCTL_STORAGE_BASE: u32 = 0x0000_002d;
    const IOCTL_DISK_BASE: u32 = 0x0000_0007;
    const METHOD_BUFFERED: u32 = 0;
    const FILE_ANY_ACCESS: u32 = 0;
    const FILE_READ_ACCESS: u32 = 0x0001;

    const fn ctl_code(device_type: u32, function: u32, method: u32, access: u32) -> u32 {
        (device_type << 16) | (access << 14) | (function << 2) | method
    }

    pub const IOCTL_STORAGE_QUERY_PROPERTY: u32 =
        ctl_code(IOCTL_STORAGE_BASE, 0x500, METHOD_BUFFERED, FILE_ANY_ACCESS);
    pub const IOCTL_DISK_GET_DRIVE_GEOMETRY: u32 =
        ctl_code(IOCTL_DISK_BASE, 0x0000, METHOD_BUFFERED, FILE_ANY_ACCESS);
    pub const IOCTL_DISK_GET_LENGTH_INFO: u32 =
        ctl_code(IOCTL_DISK_BASE, 0x0017, METHOD_BUFFERED, FILE_READ_ACCESS);

    pub const STORAGE_ACCESS_ALIGNMENT_PROPERTY: u32 = 6;
    pub const PROPERTY_STANDARD_QUERY: u32 = 0;

    #[repr(C)]
    pub struct STORAGE_PROPERTY_QUERY {
        pub property_id: u32,
        pub query_type: u32,
        pub additional_parameters: [u8; 1],
    }

    #[repr(C)]
    pub struct STORAGE_ACCESS_ALIGNMENT_DESCRIPTOR {
        pub version: u32,
        pub size: u32,
        pub bytes_per_cache_line: u32,
        pub bytes_offset_for_cache_alignment: u32,
        pub bytes_per_logical_sector: u32,
        pub bytes_per_physical_sector: u32,
        pub bytes_offset_for_sector_alignment: u32,
    }

    #[repr(C)]
    pub struct DISK_GEOMETRY {
        pub cylinders: i64,
        pub media_type: u32,
        pub tracks_per_cylinder: u32,
        pub sectors_per_track: u32,
        pub bytes_per_sector: u32,
    }

    #[repr(C)]
    pub struct GET_LENGTH_INFORMATION {
        pub length: i64,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn CreateFileW(
            lp_file_name: *const u16,
            desired_access: u32,
            share_mode: u32,
            security_attributes: *mut c_void,
            creation_disposition: u32,
            flags_and_attributes: u32,
            template_file: Handle,
        ) -> Handle;
        pub fn DeviceIoControl(
            device: Handle,
            io_control_code: u32,
            in_buffer: *mut c_void,
            in_buffer_size: u32,
            out_buffer: *mut c_void,
            out_buffer_size: u32,
            bytes_returned: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
        pub fn FlushFileBuffers(handle: Handle) -> i32;
    }

    pub fn to_utf16(path: &str) -> Vec<u16> {
        OsStr::new(path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    pub fn open_device_file(
        dev_path: &str,
        desired_access: u32,
        share_mode: u32,
        flags: u32,
    ) -> io::Result<File> {
        let wide = to_utf16(dev_path);
        let handle: Handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                desired_access,
                share_mode,
                null_mut(),
                OPEN_EXISTING,
                flags,
                null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let file: File = unsafe { File::from_raw_handle(handle as RawHandle) };
        Ok(file)
    }

    pub fn device_io_control(
        handle: Handle,
        code: u32,
        in_buf: *mut c_void,
        in_len: u32,
        out_buf: *mut c_void,
        out_len: u32,
        bytes_returned: *mut u32,
    ) -> bool {
        let ok = unsafe {
            DeviceIoControl(
                handle,
                code,
                in_buf,
                in_len,
                out_buf,
                out_len,
                bytes_returned,
                null_mut(),
            )
        };
        ok != 0
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

    #[cfg(target_os = "windows")]
    {
        let mut flags: u32 = win::FILE_ATTRIBUTE_NORMAL;
        match mode {
            SyncMode::Fast => {}
            #[cfg(feature = "durable")]
            SyncMode::Durable => {
                flags |= win::FILE_FLAG_WRITE_THROUGH;
            }
            #[cfg(feature = "direct")]
            SyncMode::Direct => {
                flags |= win::FILE_FLAG_NO_BUFFERING;
            }
        }
        // Попробуем эксклюзивный доступ: если диск занят, вернём ошибку.
        let _probe = win::open_device_file(
            dev_path,
            win::GENERIC_READ | win::GENERIC_WRITE,
            0,
            win::FILE_ATTRIBUTE_NORMAL,
        )?;
        let share_mode: u32 = win::FILE_SHARE_READ | win::FILE_SHARE_WRITE;
        let mut f: File = win::open_device_file(
            dev_path,
            win::GENERIC_READ | win::GENERIC_WRITE,
            share_mode,
            flags,
        )?;
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

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "Поддерживаются только Linux, macOS и Windows",
        ))
    }
}

/// Мягкая синхронизация: игнорирует «не поддерживается» на сырых девайсах.
#[cfg(not(target_os = "windows"))]
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

/// Мягкая синхронизация для Windows.
#[cfg(target_os = "windows")]
pub fn safe_sync(file: &File) -> io::Result<()> {
    match file.sync_all() {
        Ok(()) => Ok(()),
        Err(e) => match e.raw_os_error() {
            Some(code)
                if code == win::ERROR_INVALID_FUNCTION
                    || code == win::ERROR_NOT_SUPPORTED
                    || code == win::ERROR_INVALID_PARAMETER =>
            {
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

    #[cfg(target_os = "windows")]
    {
        file.sync_all()
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "Поддерживаются только Linux, macOS и Windows",
        ))
    }
}

#[cfg(not(feature = "durable"))]
pub fn full_sync(_file: &File) -> io::Result<()> {
    Ok(())
}

/// Выровненный буфер (для direct I/O): адрес и длина кратны `align` (обычно сектору).
pub struct AlignedBuf {
    ptr: NonNull<u8>,
    len: usize,
    align: usize,
}

impl AlignedBuf {
    pub fn new(len: usize, align: usize) -> io::Result<Self> {
        if len == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "buffer length must be > 0",
            ));
        }
        if !align.is_power_of_two() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "alignment must be a power of two",
            ));
        }
        let layout: Layout = Layout::from_size_align(len, align).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "invalid buffer layout")
        })?;
        let raw: *mut u8 = unsafe { alloc_zeroed(layout) };
        let ptr = NonNull::new(raw).ok_or_else(|| {
            io::Error::new(io::ErrorKind::OutOfMemory, "aligned allocation failed")
        })?;
        Ok(Self { ptr, len, align })
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), self.len) }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn as_ptr(&self) -> *const u8 {
        self.ptr.as_ptr()
    }
}

impl Drop for AlignedBuf {
    fn drop(&mut self) {
        if self.len == 0 {
            return;
        }
        unsafe {
            let layout = Layout::from_size_align_unchecked(self.len, self.align);
            dealloc(self.ptr.as_ptr(), layout);
        }
    }
}

/// Выровненный буфер под direct I/O.
pub fn alloc_aligned(len: usize, align: usize) -> io::Result<AlignedBuf> {
    AlignedBuf::new(len, align)
}

#[cfg(not(target_os = "windows"))]
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

#[cfg(target_os = "windows")]
pub fn get_block_sizes(dev_path: &str) -> io::Result<BlockSizes> {
    use std::mem::{size_of, zeroed};

    let file: File = win::open_device_file(
        dev_path,
        win::GENERIC_READ,
        win::FILE_SHARE_READ | win::FILE_SHARE_WRITE,
        win::FILE_ATTRIBUTE_NORMAL,
    )?;
    let handle: win::Handle = file.as_raw_handle();

    let mut bytes_returned: u32 = 0;
    let mut query = win::STORAGE_PROPERTY_QUERY {
        property_id: win::STORAGE_ACCESS_ALIGNMENT_PROPERTY,
        query_type: win::PROPERTY_STANDARD_QUERY,
        additional_parameters: [0u8; 1],
    };
    let mut desc: win::STORAGE_ACCESS_ALIGNMENT_DESCRIPTOR = unsafe { zeroed() };

    let ok = win::device_io_control(
        handle,
        win::IOCTL_STORAGE_QUERY_PROPERTY,
        &mut query as *mut _ as *mut _,
        size_of::<win::STORAGE_PROPERTY_QUERY>() as u32,
        &mut desc as *mut _ as *mut _,
        size_of::<win::STORAGE_ACCESS_ALIGNMENT_DESCRIPTOR>() as u32,
        &mut bytes_returned as *mut u32,
    );

    if ok && desc.bytes_per_logical_sector > 0 {
        let logical = desc.bytes_per_logical_sector;
        let physical = if desc.bytes_per_physical_sector > 0 {
            desc.bytes_per_physical_sector
        } else {
            logical
        };
        return Ok(BlockSizes { logical, physical });
    }

    let mut geom: win::DISK_GEOMETRY = unsafe { zeroed() };
    let ok_geom = win::device_io_control(
        handle,
        win::IOCTL_DISK_GET_DRIVE_GEOMETRY,
        std::ptr::null_mut(),
        0,
        &mut geom as *mut _ as *mut _,
        size_of::<win::DISK_GEOMETRY>() as u32,
        &mut bytes_returned as *mut u32,
    );
    if ok_geom && geom.bytes_per_sector > 0 {
        return Ok(BlockSizes {
            logical: geom.bytes_per_sector,
            physical: geom.bytes_per_sector,
        });
    }

    Err(io::Error::last_os_error())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub fn get_block_sizes(_dev_path: &str) -> io::Result<BlockSizes> {
    Err(io::Error::new(
        io::ErrorKind::Other,
        "Поддерживаются только Linux, macOS и Windows",
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

#[cfg(target_os = "windows")]
pub fn get_device_size_bytes(dev_path: &str) -> io::Result<u64> {
    use std::mem::{size_of, zeroed};

    let file: File = win::open_device_file(
        dev_path,
        win::GENERIC_READ,
        win::FILE_SHARE_READ | win::FILE_SHARE_WRITE,
        win::FILE_ATTRIBUTE_NORMAL,
    )?;
    let handle: win::Handle = file.as_raw_handle();

    let mut bytes_returned: u32 = 0;
    let mut len_info: win::GET_LENGTH_INFORMATION = unsafe { zeroed() };
    let ok = win::device_io_control(
        handle,
        win::IOCTL_DISK_GET_LENGTH_INFO,
        std::ptr::null_mut(),
        0,
        &mut len_info as *mut _ as *mut _,
        size_of::<win::GET_LENGTH_INFORMATION>() as u32,
        &mut bytes_returned as *mut u32,
    );
    if !ok || len_info.length <= 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(len_info.length as u64)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub fn get_device_size_bytes(_dev_path: &str) -> io::Result<u64> {
    Err(io::Error::new(
        io::ErrorKind::Other,
        "Поддерживаются только Linux, macOS и Windows",
    ))
}
