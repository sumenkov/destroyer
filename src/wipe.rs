use crate::dev::{safe_sync, full_sync};
use std::fs::File;
use std::io::{self, Read, Write};

/// Заполнить буфер криптографически стойкими случайными байтами из `/dev/urandom`.
pub fn fill_secure_random(buf: &mut [u8]) -> io::Result<()> {
    let mut urnd = std::fs::File::open("/dev/urandom")?;
    let mut filled = 0;
    while filled < buf.len() {
        let n = urnd.read(&mut buf[filled..])?;
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
        let percent = (done as u128 * 100u128 / total as u128) as u64;
        print!("\rПрогресс: {}%", percent);
        let _ = io::stdout().flush();
    }
}

/// Один проход перезаписи случайными данными (новая генерация буфера на каждый проход).
pub fn pass_random(file: &mut File, buf_size: usize, device_size: u64, durable: bool) -> io::Result<()> {
    let mut buf = vec![0u8; buf_size];
    fill_secure_random(&mut buf)?;

    let mut written_total: u64 = 0;
    while written_total < device_size {
        let remaining = device_size - written_total;
        let to_write = remaining.min(buf.len() as u64) as usize;

        file.write_all(&buf[..to_write])?;
        written_total += to_write as u64;

        print_progress(written_total, device_size);
    }
    println!();

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
pub fn pass_zeros(file: &mut File, buf_size: usize, device_size: u64, durable: bool) -> io::Result<()> {
    let buf = vec![0u8; buf_size];

    let mut written_total: u64 = 0;
    while written_total < device_size {
        let remaining = device_size - written_total;
        let to_write = remaining.min(buf.len() as u64) as usize;

        file.write_all(&buf[..to_write])?;
        written_total += to_write as u64;

        print_progress(written_total, device_size);
    }
    println!();

    if durable {
        full_sync(file)?;
    } else {
        safe_sync(file)?;
    }
    Ok(())
}
