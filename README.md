# destroyer (Rust)

> ⚠️ **ОПАСНО**: Утилита безвозвратно перезаписывает указанный блок‑девайс. Проверьте путь `/dev/...` или `\\.\PhysicalDriveN` и убедитесь, что устройство **размонтировано**.  
> **Поддерживаемые ОС:** Linux, macOS и Windows 10/11.

[English](README.en.md) · [中文](./README.zh-CN.md)

---

## Содержание
- [Что это](#что-это)
- [Важные предупреждения](#важные-предупреждения)
- [Поддерживаемые платформы](#поддерживаемые-платформы)
- [Установка Rust и Cargo](#установка-rust-и-cargo)
- [Сборка](#сборка)
- [Запуск](#запуск)
- [Режимы](#режимы)
- [Прямой ввод-вывод (Linux/Windows)](#прямой-ввод-вывод-linuxwindows)
- [Примеры](#примеры)
- [Размонтирование / освобождение устройства](#размонтирование--освобождение-устройства)
- [Тюнинг и производительность](#тюнинг-и-производительность)
- [Диагностика](#диагностика)
- [Архитектура](#архитектура)
- [Разработка](#разработка)
- [Фичи](#фичи)
- [Лицензия](#лицензия)

## Что это
`destroyer` — безопасная утилита для многопроходного стирания **блочных устройств**. Несколько проходов случайными данными из системного CSPRNG (Linux/macOS: `/dev/urandom`, Windows: `BCryptGenRandom`), затем финальный проход нулями.

- На **каждом проходе** генерируется новый случайный буфер.
- Финальный проход — **нули**.
- Корректное определение размера устройства на Linux/macOS/Windows.

## Важные предупреждения
- Запуск по неверному устройству **навсегда уничтожит** данные.
- Всегда сначала размонтируйте устройство (Windows: переведите диск в offline).
- Предпочтительно работать по **всему диску** (`/dev/sdX`, `/dev/nvme0n1`, `/dev/diskN`, `\\.\PhysicalDriveN`), а не по смонтированному разделу.
- Нужны права суперпользователя (`sudo`) или запуск от администратора (Windows).

## Поддерживаемые платформы
- **Linux** — поддерживается.
- **macOS** — поддерживается.
- **Windows 10/11** — поддерживаются.
- **Прочие ОС** — не поддерживаются.

## Установка Rust и Cargo
**Рекомендуется (rustup):**
```bash
# Linux / macOS:
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# затем перезагрузите shell или:
source $HOME/.cargo/env
rustc --version && cargo --version
```

**macOS через Homebrew (опционально):**
```bash
brew install rustup-init
rustup-init -y
source $HOME/.cargo/env
```

**Windows (PowerShell):**
```powershell
# 1) Скачайте rustup-init.exe с https://rustup.rs и запустите:
.\rustup-init.exe
# 2) Перезапустите терминал и проверьте:
rustc --version
cargo --version
```

## Сборка
```bash
cargo build --release
```

**Продакшен (nightly, panic-abort std)**
```bash
# Требуется: rustup component add rust-src --toolchain nightly-<трёхбуквенный_код>
cargo +nightly build --release -Zbuild-std=std,panic_abort
```

## Запуск
```bash
# Linux / macOS:
sudo target/release/destroyer <устройство> [проходы] [--mode fast|durable|direct] [--buf BYTES]

# Windows (PowerShell):
target\\release\\destroyer.exe \\\\.\\PhysicalDriveN [проходы] [--mode fast|durable|direct] [--buf BYTES]
```

### Параметры
- `<устройство>` — путь к блочному устройству (Linux: `/dev/sdX`, `/dev/nvme0n1`; macOS: `/dev/diskN`; Windows: `\\.\PhysicalDriveN`).
- `[проходы]` — количество проходов, по умолчанию **8** (последний — нулями).
- `--mode` — `fast` (по умолчанию) | `durable` (если включена фича `durable`, см. ниже) | `direct` (Linux/Windows).
- `--buf BYTES` — размер буфера записи. Если не указан — выбирается **автоматически**
  по размеру блока устройства (кратно сектору; целимся ~64 KiB в диапазоне 16 KiB..1 MiB).
- `--quiet` — не выводить прогресс (немного быстрее и тише в логах).
- `--mode direct` — доступно на Linux/Windows при включённой фиче `direct` (Linux: O_DIRECT, Windows: NO_BUFFERING).

## Режимы
- `fast` — приоритет скорость.
- `durable` — повышенная надёжность (работает, если фича `durable` включена при сборке):
  - **Linux**: открываем с `O_SYNC` (каждый `write()` ждёт устойчивой записи).
  - **macOS**: отключаем кеш (`F_NOCACHE`) и делаем «жёсткий» flush `F_FULLFSYNC` в конце каждого прохода.
  - **Windows**: `WRITE_THROUGH` + `FlushFileBuffers` в конце прохода.

## Прямой ввод-вывод (Linux/Windows)
`--mode direct` использует **O_DIRECT** на Linux или **NO_BUFFERING** на Windows и обходит page cache — так мы не «засоряем» кэш при длинной последовательной записи.

Ограничения:
- Адрес и длина **буфера** должны быть выровнены по сектору (обычно 4096 байт).
- Смещения записей должны быть кратны сектору.
- Если остаётся «хвост», не кратный сектору, он дописывается вторым дескриптором **без** O_DIRECT — устройство всё равно перезаписывается полностью.
- На **Windows** используется `FILE_FLAG_NO_BUFFERING` с теми же требованиями выравнивания.
- На **macOS** режим `--mode direct` **недоступен** — будет понятная ошибка.

Совет: не указывайте `--buf`, если в этом нет нужды — утилита сама подберёт кратный сектору размер (~64 КиБ).

## Автовыбор буфера
На Linux читаем `/sys/class/block/<dev>/queue/{logical_block_size,physical_block_size}`.
На macOS используем `DKIOCGETBLOCKSIZE`.
На Windows используем `IOCTL_STORAGE_QUERY_PROPERTY` (alignment), fallback — `IOCTL_DISK_GET_DRIVE_GEOMETRY`.
Буфер выбирается кратным `max(logical, physical)` с целевым значением **~64 KiB** (ограничения **16 KiB..1 MiB**).
Если задан `--buf`, значение нормализуется до кратности сектору и также ограничивается диапазоном.

## Примеры
```bash
sudo target/release/destroyer /dev/sdX
sudo target/release/destroyer /dev/sdX 5 --mode durable --buf 65536
sudo target/release/destroyer /dev/diskN 3 --mode fast
target\\release\\destroyer.exe \\\\.\\PhysicalDrive2 3 --mode direct
```

## Размонтирование / освобождение устройства
**macOS**
```bash
diskutil unmountDisk /dev/diskN
```

**Linux**
```bash
sudo umount <точка_монтирования_или_/dev/...>
# Найти привязки:
lsblk -f | grep $(basename /dev/sdX)
# Кто держит:
sudo lsof /dev/sdX | head
sudo fuser -mv /dev/sdX
# Если это swap:
sudo swapoff -a
# LVM / dm-crypt:
sudo dmsetup ls
```

**Windows**
```powershell
diskpart
list disk
select disk N
offline disk
exit
```
Альтернатива: «Управление дисками» → выбрать диск → «Отключить (Offline)».

## Тюнинг и производительность
- Увеличьте `--buf` до 64KiB или 1MiB, если носитель лучше пишет крупными блоками.
- Режим `durable` будет **заметно медленнее** (из-за барьеров/flush).
- На macOS предпочтительнее узлы всего диска (`/dev/diskN`).

## Диагностика
- **`EBUSY` (занято):** устройство смонтировано или занято процессом — см. раздел о размонтировании.
- **`Inappropriate ioctl for device` при синхронизации:** часть сырых устройств не поддерживает `fsync` — в коде есть безопасные обходы.
- **Permission denied:** запускайте через `sudo` или от администратора (Windows).
- **Windows `Access denied` / `Sharing violation`:** диск не отключён (offline) или не хватает прав.

## Архитектура
- Базовая логика (парсинг аргументов, помощники блочных устройств, проходы перезаписи) вынесена в библиотечный крейт `destroyer` (`src/args.rs`, `src/dev.rs`, `src/wipe.rs`, `src/app.rs`).
- Платформенные раннеры находятся в `src/platform/`: для Linux используется `platform::linux::run`, для macOS — `platform::macos::run`, для Windows — `platform::windows::run`. Здесь удобно добавлять специфичные флаги/отладку перед вызовом общего `app::run`.
- Бинарь `src/main.rs` выбирает нужный раннер с помощью `#[cfg(target_os = "...")]`, поэтому изменение поведения для одной ОС не затрагивает другую, пока вы не правите общие модули.

## Разработка
- `cargo test --features test-support` — интеграционные тесты (включают вспомогательные функции вне релизной сборки).
- `cargo clippy --release -- -W clippy::perf` — поиск потенциальных деградаций до релиза.
- `cargo bench --features test-support` — Criterion-бенчмарки буферов.
- `cargo bloat --release -n 20` — контроль роста бинарника.
- `cargo asm --release destroyer::wipe::pass_random` — просмотр критичного ассемблера.

## Фичи
| Фича           | По умолчанию | Назначение                               |
|----------------|--------------|-------------------------------------------|
| `durable`      | ✅            | Режим с барьерами O_SYNC/F_FULLFSYNC.     |
| `direct`       | ✅            | Linux O_DIRECT / Windows NO_BUFFERING.    |
| `test-support` | ❌            | Вспомогательные утилиты для тестов/bench. |

## Лицензия
MIT. Переводы для удобства:
- [LICENSE (англ.)](./LICENSE)
- [LICENSE.ru (рус., неофиц.)](./LICENSE.ru)
- [LICENSE.zh-CN (кит., неофиц.)](./LICENSE.zh-CN)
