use crate::dev::{safe_sync, full_sync, open_device_writable, SyncMode, alloc_aligned};
use std::fs::File;
use std::io::{self, Read, Write, Seek, SeekFrom, Error};

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

/// Короткий прогресс-бар в процентах.
fn print_progress(done: u64, total: u64) {
    if total > 0 {
        let percent: u64 = (done as u128 * 100u128 / total as u128) as u64;
        print!("\rПрогресс: {}%", percent);
        let _ = io::stdout().flush();
    }
}

/// Один проход перезаписи случайными данными (новая генерация буфера на каждый проход).
/// Один проход перезаписи случайными данными (новая генерация буфера на каждый проход).
/// Если дескриптор открыт в режиме O_DIRECT (Linux), буфер должен быть выровнен,
/// длина записи кратна `sector`, а смещение — кратно `sector`.
pub fn pass_random(file: &mut File, buf_size: usize, device_size: u64, durable: bool, use_direct: bool, sector: usize, dev_path: &str) -> io::Result<()> {
    let mut buf: Box<[u8]> = if use_direct { alloc_aligned(buf_size, sector)? } else { vec![0u8; buf_size].into_boxed_slice() };
    fill_secure_random(&mut buf)?;

    let written_total: u64 = 0;
    let full_limit: u64 = if use_direct { device_size - (device_size % sector as u64) } else { device_size };
    
    init_written_total(file, device_size, &buf, written_total, full_limit)?;
    println!();

    // Если остался «хвост» не кратный сектору — допишем обычным дескриптором.
    if use_direct {
        let tail: usize = (device_size - written_total) as usize;
        if tail > 0 {
            let mut tail_fd = open_device_writable(dev_path, SyncMode::Fast)?;
            tail_fd.seek(SeekFrom::Start(written_total))?;
            // использовать невыравненный обычный буфер
            let mut tbuf = vec![0u8; tail.min(buf.len())];
            // для случайных данных — важно не повторять шаблон из aligned-буфера
            fill_secure_random(&mut tbuf)?;
            tail_fd.write_all(&tbuf[..tail])?;
            if durable { full_sync(&tail_fd)?; } else { safe_sync(&tail_fd)?; }
        }
    }

    // В процессе итераций мы не форсируем full barrier — чтобы не убить скорость.
    // В конце прохода:
    if durable {
        full_sync(file)?; // «жёсткий» flush: Linux fsync, macOS F_FULLFSYNC
    } else {
        safe_sync(file)?; // мягкий flush
    }
    Ok(())
}

/// Финальный проход нулями.
pub fn pass_zeros(file: &mut File, buf_size: usize, device_size: u64, durable: bool, use_direct: bool, sector: usize, dev_path: &str) -> io::Result<()> {
    let buf: Box<[u8]> = if use_direct { alloc_aligned(buf_size, sector)? } else { vec![0u8; buf_size].into_boxed_slice() };

    let written_total: u64 = 0;
    let full_limit: u64 = if use_direct { device_size - (device_size % sector as u64) } else { device_size };
    
    init_written_total(file, device_size, &buf, written_total, full_limit)?;
    println!();

    if use_direct {
        let tail: usize = (device_size - written_total) as usize;
        if tail > 0 {
            let mut tail_fd: File = open_device_writable(dev_path, SyncMode::Fast)?;
            tail_fd.seek(SeekFrom::Start(written_total))?;
            let tbuf: Vec<u8> = vec![0u8; tail];
            tail_fd.write_all(&tbuf)?;
            if durable { full_sync(&tail_fd)?; } else { safe_sync(&tail_fd)?; }
        }
    }

    if durable {
        full_sync(file)?;
    } else {
        safe_sync(file)?;
    }
    Ok(())
}

fn init_written_total(file: &mut File, device_size: u64, buf: &Box<[u8]>, mut written_total: u64, full_limit: u64) -> Result<(), Error> {
    while written_total < full_limit {
        let remaining: u64 = full_limit - written_total;
        let to_write: usize = remaining.min(buf.len() as u64) as usize;

        file.write_all(&buf[..to_write])?;
        written_total += to_write as u64;

        print_progress(written_total, device_size);
    }
    Ok(())
}
