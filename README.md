# destroyer (Rust)

> ⚠️ ОПАСНО: Утилита безвозвратно перезаписывает указанный блок-девайс. Убедитесь, что путь `/dev/...` верный и устройство размонтировано.

## Возможности
- Криптографически стойкие случайные данные (`/dev/urandom`).
- На **каждом проходе** генерируется новый случайный буфер.
- Последний проход — нулями.
- Режимы работы:
  - `fast` (по умолчанию) — максимально быстро;
  - `durable` — повышенная надёжность:  
    - Linux: открытие с `O_SYNC` (каждый `write()` ждёт устойчивой записи).  
    - macOS: отключение кэширования (`F_NOCACHE`) и «жёсткий» flush `F_FULLFSYNC` по окончании прохода.
- Подсказки, если устройство занято (`EBUSY`): команды для Linux и macOS.

## Сборка

```bash
cargo build --release
```

## Запуск

```bash
# Синтаксис:
sudo target/release/destroyer <устройство> [проходы] [--mode fast|durable] [--buf BYTES]

# Примеры:
sudo target/release/destroyer /dev/sdX 
sudo target/release/destroyer /dev/sdX 5 --mode durable --buf 65536
sudo target/release/destroyer /dev/diskN 3 --mode fast
```
## Значение параметров

```
destroyer <устройство> [проходы] [--mode fast|durable] [--buf BYTES]
```

- <устройство> — путь к блочному устройству (Linux: /dev/sdX, /dev/nvme0n1; macOS: /dev/diskN).
- [число_проходов] — количество проходов, по умолчанию 8 (последний — нулями).
- --mode — fast или durable (см. выше), по умолчанию fast.
- --buf BYTES — размер буфера записи, по умолчанию 4096. Значение больше (например, 65536) может ускорить запись на некоторых носителях.

## Размонтирование / освобождение устройства

- **macOS**: `diskutil unmountDisk /dev/diskN`
- **Linux**:
  - `sudo umount <точка_монтирования_или_/dev/...>`
  - Найти привязки: `lsblk -f | grep $(basename /dev/sdX)`
  - Кто держит: `sudo lsof /dev/sdX | head`, `sudo fuser -mv /dev/sdX`
  - Если это swap: `sudo swapoff -a`
  - Для LVM/dm-crypt: `sudo dmsetup ls`
