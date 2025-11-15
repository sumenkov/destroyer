use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use destroyer::wipe::{Buffers, ProgressTracker, pass_zeros};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

fn bench_pass_zeros(c: &mut Criterion) {
    const FILE_SIZE: u64 = 8 * 1024 * 1024;
    let mut group = c.benchmark_group("pass_zeros");
    for buf_size in [16 * 1024usize, 64 * 1024, 256 * 1024] {
        group.throughput(criterion::Throughput::Bytes(FILE_SIZE));
        group.bench_with_input(
            BenchmarkId::from_parameter(buf_size),
            &buf_size,
            |b, &buf| {
                b.iter(|| {
                    let tmp = TempFile::new(FILE_SIZE);
                    let path: PathBuf = tmp.path().to_path_buf();
                    let mut f: File = File::options().read(true).write(true).open(&path).unwrap();

                    let mut progress = ProgressTracker::new(1, FILE_SIZE, true);
                    let mut buffers = Buffers::new(buf, false, 4096).expect("buffers");
                    pass_zeros(
                        &mut f,
                        FILE_SIZE,
                        false,
                        4096,
                        path.to_str().unwrap(),
                        &mut progress,
                        &mut buffers,
                        None,
                    )
                    .unwrap();

                    let mut checksum: u64 = 0;
                    f.seek(SeekFrom::Start(0)).unwrap();
                    let mut chunk = [0u8; 4096];
                    while let Ok(n) = f.read(&mut chunk) {
                        if n == 0 {
                            break;
                        }
                        checksum += chunk[..n].iter().map(|&b| b as u64).sum::<u64>();
                    }
                    std::hint::black_box(checksum);
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, bench_pass_zeros);
criterion_main!(benches);

struct TempFile {
    path: PathBuf,
}

impl TempFile {
    fn new(size: u64) -> Self {
        let path = unique_temp_path();
        let file: File = File::create(&path).expect("create temp file");
        file.set_len(size).expect("set_len");
        Self { path }
    }

    fn path(&self) -> &PathBuf {
        &self.path
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn unique_temp_path() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let mut path = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let idx = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    path.push(format!("destroyer-bench-{nanos}-{idx}"));
    path
}
