use crate::dev::{AlignedBuf, SyncMode, alloc_aligned, full_sync, open_device_writable, safe_sync};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Снимок прогресса, пригодный для передачи между потоками (например, в GUI).
#[derive(Clone, Debug)]
pub struct ProgressSnapshot {
    pub current_pass: usize,
    pub total_passes: usize,
    pub pass_percent: f64,
    pub total_percent: f64,
    pub pass_bytes_done: u64,
    pub device_size: u64,
    pub pass_eta: Option<Duration>,
    pub total_eta: Option<Duration>,
}

/// Трекинг прогресса: хранит таймеры и подсчитывает ETA.
pub struct ProgressTracker {
    total_start: Instant,
    pass_start: Instant,
    device_size: u64,
    total_passes: usize,
    total_target_bytes: u64,
    total_bytes_done: u64,
    pass_bytes_done: u64,
    current_pass: usize,
    quiet: bool,
    line_buf: Vec<u8>,
    /// Необязательный колбэк, вызываемый при каждом обновлении прогресса.
    /// Используется GUI-обвязкой, чтобы получать снимки прогресса без
    /// парсинга консольного вывода. Должен быть `Send`, так как обычно
    /// вызывается из фонового потока записи.
    on_update: Option<Box<dyn FnMut(&ProgressSnapshot) + Send>>,
}

impl ProgressTracker {
    pub fn new(total_passes: usize, device_size: u64, quiet: bool) -> Self {
        Self {
            total_start: Instant::now(),
            pass_start: Instant::now(),
            device_size,
            total_passes: total_passes.max(1),
            total_target_bytes: device_size.saturating_mul(total_passes as u64),
            total_bytes_done: 0,
            pass_bytes_done: 0,
            current_pass: 0,
            quiet,
            line_buf: Vec::with_capacity(96),
            on_update: None,
        }
    }

    /// Зарегистрировать колбэк для получения снимков прогресса.
    /// Не влияет на консольный вывод (управляется флагом `quiet`).
    pub fn set_on_update(&mut self, cb: Box<dyn FnMut(&ProgressSnapshot) + Send>) {
        self.on_update = Some(cb);
    }

    pub fn start_pass(&mut self, pass_index: usize) {
        self.current_pass = pass_index;
        self.pass_start = Instant::now();
        self.pass_bytes_done = 0;
    }

    pub fn record_chunk(&mut self, chunk_bytes: u64) {
        self.pass_bytes_done = self.pass_bytes_done.saturating_add(chunk_bytes);
        self.total_bytes_done = self.total_bytes_done.saturating_add(chunk_bytes);
        self.emit_snapshot();
        if !self.quiet {
            self.print_status();
        }
    }

    pub fn finish_line(&mut self) {
        if self.quiet {
            return;
        }
        let _ = io::stdout().write_all(b"\n");
    }

    fn emit_snapshot(&mut self) {
        if self.on_update.is_none() {
            return;
        }
        let pass_percent: f64 = if self.device_size > 0 {
            (self.pass_bytes_done as f64 / self.device_size as f64 * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        };
        let total_percent: f64 = if self.total_target_bytes > 0 {
            (self.total_bytes_done as f64 / self.total_target_bytes as f64 * 100.0)
                .clamp(0.0, 100.0)
        } else {
            0.0
        };
        let snap = ProgressSnapshot {
            current_pass: self.current_pass,
            total_passes: self.total_passes,
            pass_percent,
            total_percent,
            pass_bytes_done: self.pass_bytes_done,
            device_size: self.device_size,
            pass_eta: Self::eta(self.pass_bytes_done, self.device_size, self.pass_start),
            total_eta: Self::eta(
                self.total_bytes_done,
                self.total_target_bytes,
                self.total_start,
            ),
        };
        if let Some(cb) = self.on_update.as_mut() {
            cb(&snap);
        }
    }

    fn print_status(&mut self) {
        if self.quiet || self.device_size == 0 {
            return;
        }
        let percent: f64 =
            (self.pass_bytes_done as f64 / self.device_size as f64 * 100.0).clamp(0.0, 100.0);
        let pass_eta: Option<Duration> =
            Self::eta(self.pass_bytes_done, self.device_size, self.pass_start);
        let total_eta: Option<Duration> = Self::eta(
            self.total_bytes_done,
            self.total_target_bytes,
            self.total_start,
        );

        self.line_buf.clear();
        self.line_buf.extend_from_slice("\rПасс ".as_bytes());
        push_num(&mut self.line_buf, self.current_pass as u64);
        self.line_buf.push(b'/');
        push_num(&mut self.line_buf, self.total_passes as u64);
        self.line_buf.extend_from_slice(" | Прогресс: ".as_bytes());
        push_percent(&mut self.line_buf, percent.round() as u64);
        self.line_buf
            .extend_from_slice("% | Осталось проход: ".as_bytes());
        append_eta(&mut self.line_buf, pass_eta);
        self.line_buf
            .extend_from_slice(" | Осталось всего: ".as_bytes());
        append_eta(&mut self.line_buf, total_eta);

        let _ = io::stdout().write_all(&self.line_buf);
        let _ = io::stdout().flush();
    }

    fn eta(done: u64, total: u64, start: Instant) -> Option<Duration> {
        if total == 0 {
            return None;
        }
        if done >= total {
            return Some(Duration::from_secs(0));
        }
        if done == 0 {
            return None;
        }
        let elapsed = start.elapsed();
        let elapsed_secs = elapsed.as_secs_f64();
        if elapsed_secs <= 0.0 {
            return None;
        }
        let speed = done as f64 / elapsed_secs;
        if speed <= 0.0 {
            return None;
        }
        let remaining = (total - done) as f64 / speed;
        Some(Duration::from_secs_f64(remaining.max(0.0)))
    }
}

fn push_num(buf: &mut Vec<u8>, mut n: u64) {
    if n == 0 {
        buf.push(b'0');
        return;
    }
    let mut tmp = [0u8; 20];
    let mut idx = tmp.len();
    while n > 0 {
        idx -= 1;
        tmp[idx] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    buf.extend_from_slice(&tmp[idx..]);
}

fn push_percent(buf: &mut Vec<u8>, percent: u64) {
    let p = percent.min(100);
    if p < 100 {
        if p < 10 {
            buf.push(b' ');
            buf.push(b' ');
        } else {
            buf.push(b' ');
        }
    }
    push_num(buf, p);
}

fn append_eta(buf: &mut Vec<u8>, eta: Option<Duration>) {
    match eta {
        Some(dur) => {
            let mut secs: u64 = dur.as_secs();
            if dur.subsec_nanos() > 0 {
                secs = secs.saturating_add(1);
            }
            if secs >= 3600 {
                let hours = secs / 3600;
                let minutes = (secs % 3600) / 60;
                let seconds = secs % 60;
                push_num(buf, hours);
                buf.push(b':');
                push_two_digits(buf, minutes as u8);
                buf.push(b':');
                push_two_digits(buf, seconds as u8);
            } else {
                let minutes = secs / 60;
                let seconds = secs % 60;
                push_two_digits(buf, minutes as u8);
                buf.push(b':');
                push_two_digits(buf, seconds as u8);
            }
        }
        None => buf.extend_from_slice(b"--:--"),
    }
}

fn push_two_digits(buf: &mut Vec<u8>, value: u8) {
    let tens = value / 10;
    let ones = value % 10;
    buf.push(b'0' + tens);
    buf.push(b'0' + ones);
}

/// Заполнить буфер криптографически стойкими случайными байтами.
#[cfg(not(target_os = "windows"))]
pub fn fill_secure_random(buf: &mut [u8]) -> io::Result<()> {
    let mut urnd: File = std::fs::File::open("/dev/urandom")?;
    let mut filled: usize = 0;
    while filled < buf.len() {
        let n: usize = urnd.read(&mut buf[filled..])?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "urandom EOF"));
        }
        filled += n;
    }
    Ok(())
}

/// Заполнить буфер криптографически стойкими случайными байтами (Windows).
#[cfg(target_os = "windows")]
pub fn fill_secure_random(buf: &mut [u8]) -> io::Result<()> {
    use std::ffi::c_void;
    use std::ptr::null_mut;

    const BCRYPT_USE_SYSTEM_PREFERRED_RNG: u32 = 0x0000_0002;

    #[link(name = "bcrypt")]
    unsafe extern "system" {
        fn BCryptGenRandom(
            h_algorithm: *mut c_void,
            pb_buffer: *mut u8,
            cb_buffer: u32,
            dw_flags: u32,
        ) -> i32;
    }

    if buf.is_empty() {
        return Ok(());
    }

    let status: i32 = unsafe {
        BCryptGenRandom(
            null_mut(),
            buf.as_mut_ptr(),
            buf.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status != 0 {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("BCryptGenRandom failed: 0x{:08x}", status as u32),
        ));
    }
    Ok(())
}

/// Набор буферов, переиспользуемых между проходами, включая хвост для O_DIRECT.
pub struct Buffers {
    main: AlignedBuf,
    tail: Vec<u8>,
    use_direct: bool,
}

impl Buffers {
    pub fn new(buf_size: usize, use_direct: bool, sector: usize) -> io::Result<Self> {
        let align: usize = if use_direct { sector } else { 1 };
        let main: AlignedBuf = alloc_aligned(buf_size, align)?;
        let tail_capacity = if use_direct { sector.max(1) } else { 0 };
        Ok(Self {
            main,
            tail: vec![0u8; tail_capacity],
            use_direct,
        })
    }

    pub fn main_mut(&mut self) -> &mut [u8] {
        self.main.as_mut_slice()
    }

    pub fn tail_buf(&mut self, len: usize) -> &mut [u8] {
        if self.tail.len() < len {
            self.tail.resize(len, 0);
        }
        &mut self.tail[..len]
    }

    pub fn use_direct(&self) -> bool {
        self.use_direct
    }
}

/// Проверить флаг отмены; вернуть `Interrupted`-ошибку, если пользователь попросил остановиться.
fn check_cancel(cancel: Option<&AtomicBool>) -> io::Result<()> {
    if let Some(flag) = cancel {
        if flag.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled by user"));
        }
    }
    Ok(())
}

/// Один проход перезаписи случайными данными (новая генерация буфера на каждый проход).
/// Если дескриптор открыт в режиме O_DIRECT (Linux), буфер должен быть выровнен,
/// длина записи кратна `sector`, а смещение — кратно `sector`.
///
/// `cancel`, если передан, проверяется между блоками записи; при срабатывании
/// флага проход прерывается с ошибкой `io::ErrorKind::Interrupted` (используется
/// GUI-обвязкой для кнопки "Отмена").
pub fn pass_random(
    file: &mut File,
    device_size: u64,
    durable: bool,
    sector: usize,
    dev_path: &str,
    progress: &mut ProgressTracker,
    buffers: &mut Buffers,
    tail_handle: Option<&mut File>,
    cancel: Option<&AtomicBool>,
) -> io::Result<()> {
    let use_direct = buffers.use_direct();
    let buf: &mut [u8] = buffers.main_mut();
    fill_secure_random(buf)?;

    let mut written_total: u64 = 0;
    let full_limit: u64 = if use_direct {
        device_size - (device_size % sector as u64)
    } else {
        device_size
    };

    write_full_pass(file, buf, &mut written_total, full_limit, progress, cancel)?;

    // Если остался «хвост» не кратный сектору — допишем обычным дескриптором.
    #[cfg(feature = "direct")]
    if use_direct {
        check_cancel(cancel)?;
        let tail: u64 = device_size.saturating_sub(written_total);
        if tail > 0 {
            // использовать невыравненный обычный буфер
            let tbuf = buffers.tail_buf(tail as usize);
            // для случайных данных — важно не повторять шаблон из aligned-буфера
            fill_secure_random(tbuf)?;

            if let Some(writer) = tail_handle {
                writer.seek(SeekFrom::Start(written_total))?;
                writer.write_all(tbuf)?;
                progress.record_chunk(tail);
                if durable {
                    full_sync(writer)?;
                } else {
                    safe_sync(writer)?;
                }
            } else {
                let mut writer = open_device_writable(dev_path, SyncMode::Fast)?;
                writer.seek(SeekFrom::Start(written_total))?;
                writer.write_all(tbuf)?;
                progress.record_chunk(tail);
                if durable {
                    full_sync(&writer)?;
                } else {
                    safe_sync(&writer)?;
                }
            }
        }
    }
    #[cfg(not(feature = "direct"))]
    let _ = (dev_path, tail_handle);

    progress.finish_line();

    // В конце прохода синхронизируем согласно режиму.
    if durable {
        full_sync(file)?; // «жёсткий» flush: Linux fsync, macOS F_FULLFSYNC
    } else {
        safe_sync(file)?; // мягкий flush
    }
    Ok(())
}

/// Финальный проход нулями. См. `pass_random` про `cancel`.
pub fn pass_zeros(
    file: &mut File,
    device_size: u64,
    durable: bool,
    sector: usize,
    dev_path: &str,
    progress: &mut ProgressTracker,
    buffers: &mut Buffers,
    tail_handle: Option<&mut File>,
    cancel: Option<&AtomicBool>,
) -> io::Result<()> {
    let use_direct = buffers.use_direct();
    let buf: &mut [u8] = buffers.main_mut();
    buf.fill(0);

    let mut written_total: u64 = 0;
    let full_limit: u64 = if use_direct {
        device_size - (device_size % sector as u64)
    } else {
        device_size
    };

    write_full_pass(file, buf, &mut written_total, full_limit, progress, cancel)?;

    #[cfg(feature = "direct")]
    if use_direct {
        check_cancel(cancel)?;
        let tail: u64 = device_size.saturating_sub(written_total);
        if tail > 0 {
            let tbuf = buffers.tail_buf(tail as usize);
            tbuf.fill(0);

            if let Some(writer) = tail_handle {
                writer.seek(SeekFrom::Start(written_total))?;
                writer.write_all(tbuf)?;
                progress.record_chunk(tail);
                if durable {
                    full_sync(writer)?;
                } else {
                    safe_sync(writer)?;
                }
            } else {
                let mut writer: File = open_device_writable(dev_path, SyncMode::Fast)?;
                writer.seek(SeekFrom::Start(written_total))?;
                writer.write_all(tbuf)?;
                progress.record_chunk(tail);
                if durable {
                    full_sync(&writer)?;
                } else {
                    safe_sync(&writer)?;
                }
            }
        }
    }
    #[cfg(not(feature = "direct"))]
    let _ = (dev_path, tail_handle);

    progress.finish_line();

    if durable {
        full_sync(file)?;
    } else {
        safe_sync(file)?;
    }
    Ok(())
}

fn write_full_pass(
    file: &mut File,
    buf: &mut [u8],
    written_total: &mut u64,
    full_limit: u64,
    progress: &mut ProgressTracker,
    cancel: Option<&AtomicBool>,
) -> io::Result<()> {
    while *written_total < full_limit {
        check_cancel(cancel)?;

        let remaining: u64 = full_limit - *written_total;
        let to_write: usize = remaining.min(buf.len() as u64) as usize;

        file.write_all(&buf[..to_write])?;
        *written_total += to_write as u64;

        progress.record_chunk(to_write as u64);
    }
    Ok(())
}
