mod args;
mod dev;
mod wipe;

use args::Config;
use dev::{get_device_size_bytes, open_device_writable, SyncMode};
use wipe::{pass_random, pass_zeros};
use std::thread::sleep;
use std::time::Duration;

fn main() {
    let cfg = Config::parse(std::env::args().collect());

    // Размер устройства
    let device_size = match get_device_size_bytes(&cfg.device_path) {
        Ok(s) => s,
        Err(e) => {
            if let Some(code) = e.raw_os_error() {
                if code == libc::EBUSY {
                    eprintln!("Устройство {} занято (возможно, примонтировано).", cfg.device_path);
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

    println!(
        "Размер устройства: {} байт ({:.2} GB)",
        device_size,
        device_size as f64 / (1024.0 * 1024.0 * 1024.0)
    );

    println!(
        "Выполняется {} проходов очистки (последний — нулями)...",
        cfg.passes
    );
    println!("Режим: {:?}", match cfg.mode { SyncMode::Fast => "fast", SyncMode::Durable => "durable" });
    println!("Буфер: {} bytes", cfg.buf_size);
    println!("ВНИМАНИЕ: все данные на устройстве будут уничтожены!");
    println!("Для отмены нажмите Ctrl+C в течение 5 секунд...");
    sleep(Duration::from_secs(5));

    // Проходы
    for pass_idx in 0..cfg.passes.saturating_sub(1) {
        println!("\nПроход {}/{} (случайные данные)...", pass_idx + 1, cfg.passes);

        let mut f = match open_device_writable(&cfg.device_path, cfg.mode) {
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
        };

        if let Err(e) = pass_random(&mut f, cfg.buf_size, device_size, matches!(cfg.mode, SyncMode::Durable)) {
            eprintln!("Ошибка записи случайных данных: {e}");
            std::process::exit(1);
        }
    }

    // Финальный проход нулями
    println!("\nФинальный проход {}/{} (нули)...", cfg.passes, cfg.passes);

    let mut f = match open_device_writable(&cfg.device_path, cfg.mode) {
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
    };

    if let Err(e) = pass_zeros(&mut f, cfg.buf_size, device_size, matches!(cfg.mode, SyncMode::Durable)) {
        eprintln!("Ошибка записи нулей: {e}");
        std::process::exit(1);
    }

    println!("\nУстройство {} успешно очищено", cfg.device_path);
}
