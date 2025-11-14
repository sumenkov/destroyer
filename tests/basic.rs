// tests/basic.rs
// Интеграционные тесты: подключаем исходники напрямую через #[path],
// чтобы не зависеть от имени крейта и не модифицировать существующие файлы.

#![allow(clippy::redundant_clone)]
#![allow(clippy::bool_assert_comparison)]

#[path = "../src/args.rs"]
mod args;
#[path = "../src/dev.rs"]
mod dev;
#[path = "../src/wipe.rs"]
mod wipe;

use crate::args::Config;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
//
// -------- tests for args::Config::parse --------
//

#[test]
fn parse_minimal_ok() {
    let argv: Vec<String> = vec![
        "destroyer".to_string(),
        "/tmp/fake_device".to_string(),
        "2".to_string(),
    ];
    let cfg: Config = args::Config::parse(argv);
    assert_eq!(cfg.device_path, "/tmp/fake_device");
    assert_eq!(cfg.passes, 2);
    assert_eq!(cfg.buf_size, None);
    // По умолчанию fast
    match cfg.mode {
        dev::SyncMode::Fast => {}
        _ => panic!("expected SyncMode::Fast"),
    }
}

#[test]
fn parse_with_flags_ok() {
    let argv: Vec<String> = vec![
        "destroyer".to_string(),
        "/dev/sda".to_string(),
        "3".to_string(),
        "--mode".to_string(),
        "durable".to_string(),
        "--buf".to_string(),
        "65536".to_string(),
    ];
    let cfg: Config = args::Config::parse(argv);
    assert_eq!(cfg.device_path, "/dev/sda");
    assert_eq!(cfg.passes, 3);
    assert_eq!(cfg.buf_size, Some(65536));
    match cfg.mode {
        dev::SyncMode::Durable => {}
        _ => panic!("expected SyncMode::Durable"),
    }
}

//
// -------- tests for dev helpers --------
//

#[test]
fn choose_buffer_size_respects_min_max_and_alignment() {
    let sizes = dev::BlockSizes {
        logical: 4096,
        physical: 4096,
    };

    // None -> 64 KiB, уже кратно сектору
    let b0: usize = dev::choose_buffer_size(sizes, None);
    assert_eq!(b0, 64 * 1024);
    assert_eq!(b0 % sizes.sector() as usize, 0);

    // Слишком маленький -> поднимем до 16 KiB и выровняем по сектору
    let b1: usize = dev::choose_buffer_size(sizes, Some(1));
    assert_eq!(b1, 16 * 1024);
    assert_eq!(b1 % sizes.sector() as usize, 0);

    // Слишком большой -> ограничим до 1 MiB
    let b2: usize = dev::choose_buffer_size(sizes, Some(10 * 1024 * 1024));
    assert_eq!(b2, 1024 * 1024);
    assert_eq!(b2 % sizes.sector() as usize, 0);

    // Не кратен сектору -> поднимем до ближайшего кратного
    let b3: usize = dev::choose_buffer_size(sizes, Some(70 * 1024));
    assert_eq!(b3 % sizes.sector() as usize, 0);
    assert!(b3 >= 70 * 1024);
}

#[test]
fn alloc_aligned_returns_aligned_buffer() {
    let align: usize = 4096usize;
    let len: usize = 8192usize;
    let buf: Box<[u8]> = dev::alloc_aligned(len, align).expect("alloc_aligned failed");
    assert_eq!(buf.len(), len);

    // Проверим, что указатель выровнен по align (на Linux используется posix_memalign)
    #[cfg(target_os = "linux")]
    {
        let ptr: usize = buf.as_ptr() as usize;
        assert_eq!(ptr % align, 0);
    }
}

//
// -------- tests for wipe passes on a regular file --------
//

#[derive(Debug)]
struct TempFile {
    path: PathBuf,
}

impl TempFile {
    fn new(size: u64) -> Self {
        let path: PathBuf = unique_temp_path();
        let file: File = File::create(&path).expect("tempfile create");
        file.set_len(size).expect("set_len");
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn unique_temp_path() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let mut path: PathBuf = std::env::temp_dir();
    let nanos: u128 = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let idx: usize = COUNTER.fetch_add(1, Ordering::SeqCst);
    path.push(format!("destroyer-test-{nanos}-{idx}"));
    path
}

fn create_sparse_temp(size: u64) -> TempFile {
    TempFile::new(size)
}

#[test]
fn pass_zeros_writes_zeros() {
    let tmp: TempFile = create_sparse_temp(128 * 1024);
    let path: PathBuf = tmp.path().to_path_buf();
    let mut f: File = File::options().read(true).write(true).open(&path).unwrap();

    let buf_size: usize = 32 * 1024;
    // direct=false, sector не используется; dev_path только для хвоста при direct=true
    let mut progress = wipe::ProgressTracker::new(1, 128 * 1024);
    progress.start_pass(1);
    wipe::pass_zeros(
        &mut f,
        buf_size,
        128 * 1024,
        false,
        false,
        4096,
        path.to_str().unwrap(),
        &mut progress,
    )
    .expect("pass_zeros");

    // Проверим, что все байты — нули
    let mut data: Vec<u8> = Vec::new();
    f.seek(SeekFrom::Start(0)).unwrap();
    f.read_to_end(&mut data).unwrap();
    assert_eq!(data.len(), 128 * 1024);
    assert!(data.iter().all(|&b| b == 0));
}

#[test]
fn pass_random_writes_nonzero_somewhere() {
    let tmp: TempFile = create_sparse_temp(128 * 1024);
    let path: PathBuf = tmp.path().to_path_buf();
    let mut f: File = File::options().read(true).write(true).open(&path).unwrap();

    let buf_size: usize = 32 * 1024;
    let mut progress = wipe::ProgressTracker::new(1, 128 * 1024);
    progress.start_pass(1);
    wipe::pass_random(
        &mut f,
        buf_size,
        128 * 1024,
        false,
        false,
        4096,
        path.to_str().unwrap(),
        &mut progress,
    )
    .expect("pass_random");

    let mut data = Vec::new();
    f.seek(SeekFrom::Start(0)).unwrap();
    f.read_to_end(&mut data).unwrap();
    assert_eq!(data.len(), 128 * 1024);
    // С крайне высокой вероятностью в массиве будет хотя бы один ненулевой байт.
    assert!(data.iter().any(|&b| b != 0));
}

#[test]
fn fill_secure_random_fills_buffer() {
    let mut buf: Vec<u8> = vec![0u8; 8192];
    wipe::fill_secure_random(&mut buf).expect("urandom");
    // Вероятность, что все байты нули, ничтожна.
    assert!(buf.iter().any(|&b| b != 0));
}
