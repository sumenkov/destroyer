use crate::dev::{SyncMode, alloc_aligned, full_sync, open_device_writable, safe_sync};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::time::{Duration, Instant};

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
}

impl ProgressTracker {
    pub fn new(total_passes: usize, device_size: u64) -> Self {
        Self {
            total_start: Instant::now(),
            pass_start: Instant::now(),
            device_size,
            total_passes: total_passes.max(1),
            total_target_bytes: device_size.saturating_mul(total_passes as u64),
            total_bytes_done: 0,
            pass_bytes_done: 0,
            current_pass: 0,
        }
    }

    pub fn start_pass(&mut self, pass_index: usize) {
        self.current_pass = pass_index;
        self.pass_start = Instant::now();
        self.pass_bytes_done = 0;
    }

    pub fn record_chunk(&mut self, chunk_bytes: u64) {
        self.pass_bytes_done = self.pass_bytes_done.saturating_add(chunk_bytes);
        self.total_bytes_done = self.total_bytes_done.saturating_add(chunk_bytes);
        self.print_status();
    }

    pub fn finish_line(&self) {
        println!();
    }

    fn print_status(&self) {
        if self.device_size == 0 {
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

        print!(
            "\rПасс {}/{} | Прогресс: {:>3}% | Осталось проход: {} | Осталось всего: {}",
            self.current_pass,
            self.total_passes,
            percent.round() as u64,
            format_eta(pass_eta),
            format_eta(total_eta),
        );
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

fn format_eta(eta: Option<Duration>) -> String {
    match eta {
        Some(dur) => {
            let mut secs: u64 = dur.as_secs();
            if dur.subsec_nanos() > 0 {
                secs = secs.saturating_add(1);
            }
            format_duration(secs)
        }
        None => "--:--".to_string(),
    }
}

fn format_duration(total_secs: u64) -> String {
    if total_secs >= 3600 {
        let hours = total_secs / 3600;
        let minutes = (total_secs % 3600) / 60;
        let seconds = total_secs % 60;
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        let minutes = total_secs / 60;
        let seconds = total_secs % 60;
        format!("{minutes:02}:{seconds:02}")
    }
}

/// Заполнить буфер криптографически стойкими случайными байтами из `/dev/urandom`.
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

/// Один проход перезаписи случайными данными (новая генерация буфера на каждый проход).
/// Если дескриптор открыт в режиме O_DIRECT (Linux), буфер должен быть выровнен,
/// длина записи кратна `sector`, а смещение — кратно `sector`.
pub fn pass_random(
    file: &mut File,
    buf_size: usize,
    device_size: u64,
    durable: bool,
    use_direct: bool,
    sector: usize,
    dev_path: &str,
    progress: &mut ProgressTracker,
) -> io::Result<()> {
    let mut buf: Box<[u8]> = if use_direct {
        alloc_aligned(buf_size, sector)?
    } else {
        vec![0u8; buf_size].into_boxed_slice()
    };
    fill_secure_random(&mut buf)?;

    let mut written_total: u64 = 0;
    let full_limit: u64 = if use_direct {
        device_size - (device_size % sector as u64)
    } else {
        device_size
    };

    write_full_pass(file, &buf, &mut written_total, full_limit, progress)?;

    // Если остался «хвост» не кратный сектору — допишем обычным дескриптором.
    if use_direct {
        let tail: u64 = device_size.saturating_sub(written_total);
        if tail > 0 {
            let mut tail_fd = open_device_writable(dev_path, SyncMode::Fast)?;
            tail_fd.seek(SeekFrom::Start(written_total))?;
            // использовать невыравненный обычный буфер
            let mut tbuf = vec![0u8; tail as usize];
            // для случайных данных — важно не повторять шаблон из aligned-буфера
            fill_secure_random(&mut tbuf)?;
            tail_fd.write_all(&tbuf)?;
            progress.record_chunk(tail);
            if durable {
                full_sync(&tail_fd)?;
            } else {
                safe_sync(&tail_fd)?;
            }
        }
    }

    progress.finish_line();

    // В конце прохода синхронизируем согласно режиму.
    if durable {
        full_sync(file)?; // «жёсткий» flush: Linux fsync, macOS F_FULLFSYNC
    } else {
        safe_sync(file)?; // мягкий flush
    }
    Ok(())
}

/// Финальный проход нулями.
pub fn pass_zeros(
    file: &mut File,
    buf_size: usize,
    device_size: u64,
    durable: bool,
    use_direct: bool,
    sector: usize,
    dev_path: &str,
    progress: &mut ProgressTracker,
) -> io::Result<()> {
    let buf: Box<[u8]> = if use_direct {
        alloc_aligned(buf_size, sector)?
    } else {
        vec![0u8; buf_size].into_boxed_slice()
    };

    let mut written_total: u64 = 0;
    let full_limit: u64 = if use_direct {
        device_size - (device_size % sector as u64)
    } else {
        device_size
    };

    write_full_pass(file, &buf, &mut written_total, full_limit, progress)?;

    if use_direct {
        let tail: u64 = device_size.saturating_sub(written_total);
        if tail > 0 {
            let mut tail_fd: File = open_device_writable(dev_path, SyncMode::Fast)?;
            tail_fd.seek(SeekFrom::Start(written_total))?;
            let tbuf: Vec<u8> = vec![0u8; tail as usize];
            tail_fd.write_all(&tbuf)?;
            progress.record_chunk(tail);
            if durable {
                full_sync(&tail_fd)?;
            } else {
                safe_sync(&tail_fd)?;
            }
        }
    }

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
    buf: &[u8],
    written_total: &mut u64,
    full_limit: u64,
    progress: &mut ProgressTracker,
) -> io::Result<()> {
    while *written_total < full_limit {
        let remaining: u64 = full_limit - *written_total;
        let to_write: usize = remaining.min(buf.len() as u64) as usize;

        file.write_all(&buf[..to_write])?;
        *written_total += to_write as u64;

        progress.record_chunk(to_write as u64);
    }
    Ok(())
}
