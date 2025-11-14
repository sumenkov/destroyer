use crate::dev::SyncMode;
use std::process::exit;

/// Конфигурация запуска.
pub struct Config {
    pub device_path: String,
    pub passes: usize,
    /// Пользовательский размер буфера, если задан через --buf.
    /// Если None — будет выбран автоматически по размеру блока устройства.
    pub buf_size: Option<usize>,
    pub mode: SyncMode,
}

impl Config {
    /// Примитивный парсер аргументов без внешних крейтов.
    ///
    /// Форматы:
    ///   destroyer <device> [passes]
    ///   destroyer <device> [passes] --mode fast|durable|direct [--buf BYTES]
    pub fn parse(mut args: Vec<String>) -> Self {
        if args.len() < 2 {
            eprintln!(
                "{}",
                Self::usage(&args.get(0).map(String::as_str).unwrap_or("destroyer"))
            );
            exit(1);
        }

        let prog: String = args.remove(0);
        let mut device_path: Option<String> = None;
        let mut passes: Option<usize> = None;
        let mut buf_size: Option<usize> = None;
        let mut mode: SyncMode = SyncMode::Fast;

        let mut i: usize = 0usize;
        while i < args.len() {
            match args[i].as_str() {
                "--help" | "-h" => {
                    eprintln!("{}", Self::usage(&prog));
                    exit(0);
                }
                "--mode" => {
                    if i + 1 >= args.len() {
                        eprintln!("--mode требует аргумент: fast|durable|direct");
                        exit(1);
                    }
                    let val: &str = args[i + 1].as_str();
                    mode = match val {
                        "fast" => SyncMode::Fast,
                        "durable" => SyncMode::Durable,
                        "direct" => {
                            // На Linux разрешаем, на macOS выдадим понятную ошибку в main при попытке открыть.
                            #[cfg(target_os = "linux")]
                            {
                                SyncMode::Direct
                            }
                            #[cfg(not(target_os = "linux"))]
                            {
                                eprintln!(
                                    "Режим 'direct' поддерживается только на Linux (O_DIRECT)."
                                );
                                exit(1);
                            }
                        }
                        _ => {
                            eprintln!(
                                "Неизвестное значение --mode: {val}. Ожидается fast|durable|direct"
                            );
                            exit(1);
                        }
                    };
                    i += 2;
                }
                "--buf" => {
                    if i + 1 >= args.len() {
                        eprintln!("--buf требует число байт");
                        exit(1);
                    }
                    let parsed: usize = args[i + 1].parse::<usize>().unwrap_or_else(|_| {
                        eprintln!("Некорректное значение для --buf");
                        exit(1);
                    });
                    if parsed == 0 {
                        eprintln!("--buf должен быть > 0");
                        exit(1);
                    }
                    buf_size = Some(parsed);
                    i += 2;
                }
                s if s.starts_with("--") => {
                    eprintln!("Неизвестный флаг: {s}");
                    exit(1);
                }
                // позиционные:
                other => {
                    if device_path.is_none() {
                        device_path = Some(other.to_string());
                        i += 1;
                    } else if passes.is_none() {
                        let p: usize = other.parse::<usize>().unwrap_or_else(|_| {
                            eprintln!("Число проходов должно быть положительным целым");
                            exit(1);
                        });
                        if p == 0 {
                            eprintln!("Число проходов должно быть >= 1");
                            exit(1);
                        }
                        passes = Some(p);
                        i += 1;
                    } else {
                        eprintln!("Лишний позиционный аргумент: {other}");
                        exit(1);
                    }
                }
            }
        }

        let device_path: String = device_path.unwrap_or_else(|| {
            eprintln!("{}", Self::usage(&prog));
            exit(1);
        });

        let passes: usize = passes.unwrap_or(8);

        Self {
            device_path,
            passes,
            buf_size,
            mode,
        }
    }

    pub fn usage(prog: &str) -> String {
        format!(
"Использование:
  {prog} <устройство> [проходы] [--mode fast|durable|direct] [--buf BYTES]

Примеры:
  sudo {prog} /dev/sdX 8
  sudo {prog} /dev/sdX 8 --mode durable --buf 65536
  sudo {prog} /dev/sdX 8 --mode direct
  sudo {prog} /dev/diskN 3 --mode fast

Пояснения:
  <устройство>     Путь к блочному девайсу (Linux: /dev/sdX|nvme0n1; macOS: /dev/diskN)
  [проходы]        Количество проходов (последний — нулями). По умолчанию 8
  --mode           fast (быстро) | durable (максимум надёжности) | direct (Linux, O_DIRECT — без page cache)
  --buf BYTES      Размер буфера. Если не указан — выбирается автоматически
                   по размеру блока устройства (кратно сектору, целимся ~64 KiB)"
        )
    }
}
