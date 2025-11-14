mod args;
mod dev;
mod wipe;

use crate::dev::BlockSizes;
use args::Config;
use dev::{
    SyncMode, choose_buffer_size, get_block_sizes, get_device_size_bytes, open_device_writable,
};
use std::fs::File;
use std::thread::sleep;
use std::time::Duration;
use wipe::{ProgressTracker, pass_random, pass_zeros};

fn main() {
    let cfg: Config = Config::parse(std::env::args().collect());

    // Размер устройства
    let device_size: u64 = match get_device_size_bytes(&cfg.device_path) {
        Ok(s) => s,
        Err(e) => {
            if let Some(code) = e.raw_os_error() {
                if code == libc::EBUSY {
                    eprintln!(
                        "Устройство {} занято (возможно, примонтировано).",
                        cfg.device_path
                    );
                    eprintln!("macOS:  diskutil unmountDisk {}", cfg.device_path);
                    eprintln!("Linux:  sudo umount <точка_монтирования_или_/dev/..>");
                    eprintln!("        lsblk -f | grep $(basename {})", cfg.device_path);
                    eprintln!("        sudo lsof {}  | head", cfg.device_path);
                    eprintln!("        sudo fuser -mv {}", cfg.device_path);
                    eprintln!("        sudo swapoff -a");
                    eprintln!("        sudo dmsetup ls");
                }
            }
            eprintln!("Ошибка определения размера устройства: {e}");
            std::process::exit(1);
        }
    };

    // Размеры блоков и выбор буфера
    let bs: BlockSizes = get_block_sizes(&cfg.device_path).unwrap_or_else(|_| {
        // Если не удалось определить — используем безопасные дефолты
        dev::BlockSizes {
            logical: 512,
            physical: 4096,
        }
    });
    let buf_size: usize = choose_buffer_size(bs, cfg.buf_size);
    let sector: usize = bs.sector() as usize;
    let use_direct: bool = matches!(cfg.mode, SyncMode::Direct);

    println!(
        "Размер устройства: {} байт ({:.2} GB)",
        device_size,
        device_size as f64 / (1024.0 * 1024.0 * 1024.0)
    );

    println!(
        "Выполняется {} проходов очистки (последний — нулями)...",
        cfg.passes
    );
    println!(
        "Режим: {:?}",
        match cfg.mode {
            SyncMode::Fast => "fast",
            SyncMode::Durable => "durable",
            SyncMode::Direct => "direct",
        }
    );
    println!(
        "Блоки: logical = {}B, physical = {}B; выбран буфер = {}B",
        bs.logical, bs.physical, buf_size
    );
    println!("ВНИМАНИЕ: все данные на устройстве будут уничтожены!");
    println!("Для отмены нажмите Ctrl+C в течение 5 секунд...");
    sleep(Duration::from_secs(5));

    let mut progress: ProgressTracker = ProgressTracker::new(cfg.passes, device_size);

    // Проходы
    for pass_idx in 0..cfg.passes.saturating_sub(1) {
        println!(
            "\nПроход {}/{} (случайные данные)...",
            pass_idx + 1,
            cfg.passes
        );

        let mut f: File = open_device(&cfg);
        progress.start_pass(pass_idx + 1);

        if let Err(e) = pass_random(
            &mut f,
            buf_size,
            device_size,
            matches!(cfg.mode, SyncMode::Durable),
            use_direct,
            sector,
            &cfg.device_path,
            &mut progress,
        ) {
            eprintln!("Ошибка записи случайных данных: {e}");
            std::process::exit(1);
        }
    }

    // Финальный проход нулями
    println!("\nФинальный проход {}/{} (нули)...", cfg.passes, cfg.passes);

    let mut f: File = open_device(&cfg);
    progress.start_pass(cfg.passes);

    if let Err(e) = pass_zeros(
        &mut f,
        buf_size,
        device_size,
        matches!(cfg.mode, SyncMode::Durable),
        use_direct,
        sector,
        &cfg.device_path,
        &mut progress,
    ) {
        eprintln!("Ошибка записи нулей: {e}");
        std::process::exit(1);
    }

    println!("\nУстройство {} успешно очищено", cfg.device_path);
}

fn open_device(cfg: &Config) -> File {
    match open_device_writable(&cfg.device_path, cfg.mode) {
        Ok(file) => file,
        Err(e) => {
            if let Some(code) = e.raw_os_error() {
                if code == libc::EBUSY {
                    eprintln!("Устройство занято. Размонтируйте.");
                    eprintln!("macOS:  diskutil unmountDisk {}", cfg.device_path);
                    eprintln!("Linux:  sudo umount <точка_монтирования_или_/dev/..>");
                }
            }
            eprintln!("Ошибка открытия устройства: {e}");
            std::process::exit(1);
        }
    }
}
