use crate::args::Config;
use crate::dev::{
    BlockSizes, SyncMode, choose_buffer_size, get_block_sizes, get_device_size_bytes,
    open_device_writable,
};
use crate::wipe::{Buffers, ProgressTracker, pass_random, pass_zeros};
use std::fs::File;
use std::io::{Seek, SeekFrom};
use std::thread::sleep;
use std::time::Duration;

/// Операционная система, под которую собрана программа.
#[derive(Clone, Copy, Debug)]
pub enum Platform {
    Linux,
    MacOs,
}

impl Platform {
    pub fn name(&self) -> &'static str {
        match self {
            Platform::Linux => "Linux",
            Platform::MacOs => "macOS",
        }
    }
}

/// Точка входа для платформенного раннера.
pub fn run(platform: Platform) {
    let cfg: Config = Config::parse(std::env::args_os());
    execute(cfg, platform);
}

fn execute(cfg: Config, platform: Platform) {
    println!("Платформа: {}", platform.name());

    let device_size: u64 = match get_device_size_bytes(&cfg.device_path) {
        Ok(s) => s,
        Err(e) => {
            if let Some(code) = e.raw_os_error() {
                if code == libc::EBUSY {
                    busy_help(&cfg.device_path);
                }
            }
            eprintln!("Ошибка определения размера устройства: {e}");
            std::process::exit(1);
        }
    };

    let bs: BlockSizes = get_block_sizes(&cfg.device_path).unwrap_or_else(|_| BlockSizes {
        logical: 512,
        physical: 4096,
    });
    let buf_size: usize = choose_buffer_size(bs, cfg.buf_size);
    let sector: usize = bs.sector() as usize;
    let use_direct: bool = cfg.mode.is_direct();

    println!(
        "Размер устройства: {} байт ({:.2} GB)",
        device_size,
        device_size as f64 / (1024.0 * 1024.0 * 1024.0)
    );
    println!(
        "Выполняется {} проходов очистки (последний — нулями)...",
        cfg.passes
    );
    println!("Режим: {}", cfg.mode.label());
    println!(
        "Блоки: logical = {}B, physical = {}B; выбран буфер = {}B",
        bs.logical, bs.physical, buf_size
    );
    println!("ВНИМАНИЕ: все данные на устройстве будут уничтожены!");
    println!("Для отмены нажмите Ctrl+C в течение 5 секунд...");
    sleep(Duration::from_secs(5));

    let mut progress: ProgressTracker = ProgressTracker::new(cfg.passes, device_size, cfg.quiet);
    let mut buffers = Buffers::new(buf_size, use_direct, sector).unwrap_or_else(|e| {
        eprintln!("Не удалось подготовить буфер записи: {e}");
        std::process::exit(1);
    });
    let mut main_handle: File = open_device(&cfg, cfg.mode);
    #[cfg(feature = "direct")]
    let mut tail_handle: Option<File> = if use_direct {
        Some(open_device(&cfg, SyncMode::Fast))
    } else {
        None
    };

    for pass_idx in 0..cfg.passes.saturating_sub(1) {
        println!(
            "\nПроход {}/{} (случайные данные)...",
            pass_idx + 1,
            cfg.passes
        );
        progress.start_pass(pass_idx + 1);
        if let Err(e) = main_handle.seek(SeekFrom::Start(0)) {
            eprintln!("Не удалось вернуть устройство в начало: {e}");
            std::process::exit(1);
        }
        let durable_mode = cfg.mode.is_durable();
        let tail_ref: Option<&mut File> = {
            #[cfg(feature = "direct")]
            {
                tail_handle.as_mut()
            }
            #[cfg(not(feature = "direct"))]
            {
                None
            }
        };
        if let Err(e) = pass_random(
            &mut main_handle,
            device_size,
            durable_mode,
            sector,
            &cfg.device_path,
            &mut progress,
            &mut buffers,
            tail_ref,
        ) {
            eprintln!("Ошибка записи случайных данных: {e}");
            std::process::exit(1);
        }
    }

    println!("\nФинальный проход {}/{} (нули)...", cfg.passes, cfg.passes);
    progress.start_pass(cfg.passes);
    if let Err(e) = main_handle.seek(SeekFrom::Start(0)) {
        eprintln!("Не удалось вернуть устройство в начало: {e}");
        std::process::exit(1);
    }
    let durable_mode = cfg.mode.is_durable();
    let tail_ref: Option<&mut File> = {
        #[cfg(feature = "direct")]
        {
            tail_handle.as_mut()
        }
        #[cfg(not(feature = "direct"))]
        {
            None
        }
    };
    if let Err(e) = pass_zeros(
        &mut main_handle,
        device_size,
        durable_mode,
        sector,
        &cfg.device_path,
        &mut progress,
        &mut buffers,
        tail_ref,
    ) {
        eprintln!("Ошибка записи нулей: {e}");
        std::process::exit(1);
    }

    println!("\nУстройство {} успешно очищено", cfg.device_path);
}

fn open_device(cfg: &Config, mode: SyncMode) -> File {
    match open_device_writable(&cfg.device_path, mode) {
        Ok(file) => file,
        Err(e) => {
            if let Some(code) = e.raw_os_error() {
                if code == libc::EBUSY {
                    busy_help(&cfg.device_path);
                }
            }
            eprintln!("Ошибка открытия устройства: {e}");
            std::process::exit(1);
        }
    }
}

fn busy_help(device_path: &str) {
    eprintln!(
        "Устройство {} занято (возможно, примонтировано).",
        device_path
    );
    eprintln!("macOS:  diskutil unmountDisk {}", device_path);
    eprintln!("Linux:  sudo umount <точка_монтирования_или_/dev/..>");
    eprintln!("        lsblk -f | grep $(basename {})", device_path);
    eprintln!("        sudo lsof {}  | head", device_path);
    eprintln!("        sudo fuser -mv {}", device_path);
    eprintln!("        sudo swapoff -a");
    eprintln!("        sudo dmsetup ls");
}
