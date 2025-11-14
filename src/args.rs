use crate::dev::SyncMode;
use std::ffi::OsString;
use std::iter::Peekable;
use std::process::exit;

/// Конфигурация запуска.
pub struct Config {
    pub device_path: String,
    pub passes: usize,
    /// Пользовательский размер буфера, если задан через --buf.
    /// Если None — будет выбран автоматически по размеру блока устройства.
    pub buf_size: Option<usize>,
    pub mode: SyncMode,
    pub quiet: bool,
}

impl Config {
    /// Примитивный парсер аргументов без внешних крейтов.
    ///
    /// Форматы:
    ///   destroyer <device> [passes]
    ///   destroyer <device> [passes] --mode fast|durable|direct [--buf BYTES]
    pub fn parse<I>(args: I) -> Self
    where
        I: IntoIterator<Item = OsString>,
    {
        let mut iter: Peekable<_> = args.into_iter().peekable();
        let prog = iter
            .next()
            .and_then(|s| s.into_string().ok())
            .unwrap_or_else(|| {
                eprintln!("{}", Self::usage("destroyer"));
                exit(1);
            });

        let mut device_path: Option<String> = None;
        let mut passes: Option<usize> = None;
        let mut buf_size: Option<usize> = None;
        let mut mode: SyncMode = SyncMode::Fast;
        let mut quiet: bool = false;

        while let Some(arg) = iter.next() {
            match arg.to_str() {
                Some("--help") | Some("-h") => {
                    eprintln!("{}", Self::usage(&prog));
                    exit(0);
                }
                Some("--mode") => {
                    let val = iter.next().unwrap_or_else(|| {
                        eprintln!("--mode требует аргумент: fast|durable|direct");
                        exit(1);
                    });
                    let val_str = val.to_str().unwrap_or_else(|| {
                        eprintln!("--mode принимает только UTF-8 значения");
                        exit(1);
                    });
                    mode = match val_str {
                        "fast" => SyncMode::Fast,
                        "durable" => {
                            #[cfg(feature = "durable")]
                            {
                                SyncMode::Durable
                            }
                            #[cfg(not(feature = "durable"))]
                            {
                                eprintln!("Режим 'durable' недоступен в текущей сборке.");
                                exit(1);
                            }
                        }
                        "direct" => {
                            #[cfg(all(feature = "direct", target_os = "linux"))]
                            {
                                SyncMode::Direct
                            }
                            #[cfg(not(all(feature = "direct", target_os = "linux")))]
                            {
                                eprintln!(
                                    "Режим 'direct' поддерживается только на Linux и при включённом флаге сборки."
                                );
                                exit(1);
                            }
                        }
                        other => {
                            eprintln!(
                                "Неизвестное значение --mode: {other}. Ожидается fast|durable|direct"
                            );
                            exit(1);
                        }
                    };
                }
                Some("--buf") => {
                    let val = iter.next().unwrap_or_else(|| {
                        eprintln!("--buf требует число байт");
                        exit(1);
                    });
                    let val_str = val.to_str().unwrap_or_else(|| {
                        eprintln!("--buf принимает только UTF-8 значения");
                        exit(1);
                    });
                    let parsed: usize = val_str.parse::<usize>().unwrap_or_else(|_| {
                        eprintln!("Некорректное значение для --buf");
                        exit(1);
                    });
                    if parsed == 0 {
                        eprintln!("--buf должен быть > 0");
                        exit(1);
                    }
                    buf_size = Some(parsed);
                }
                Some("--quiet") => {
                    quiet = true;
                }
                Some(s) if s.starts_with("--") => {
                    eprintln!("Неизвестный флаг: {s}");
                    exit(1);
                }
                Some(other) => {
                    if device_path.is_none() {
                        device_path = Some(other.to_string());
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
                    } else {
                        eprintln!("Лишний позиционный аргумент: {other}");
                        exit(1);
                    }
                }
                None => {
                    eprintln!("Аргументы должны быть валидным UTF-8");
                    exit(1);
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
            quiet,
        }
    }

    pub fn usage(prog: &str) -> String {
        format!(
"Использование:
  {prog} <устройство> [проходы] [--mode fast|durable|direct] [--buf BYTES] [--quiet]

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
                   по размеру блока устройства (кратно сектору, целимся ~64 KiB)
  --quiet          Не выводить строку прогресса (ускоряет работу)."
        )
    }
}
