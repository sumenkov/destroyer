//! Перечисление блочных устройств для GUI. Реализовано только для Linux —
//! так как сам GUI сейчас предназначен только для Linux (см. README).

/// Информация об одном блочном устройстве, достаточная, чтобы показать её
/// пользователю в списке выбора и предупредить о рискованных дисках.
#[derive(Clone, Debug)]
pub struct DeviceInfo {
    /// Путь вида `/dev/sdX`, `/dev/nvme0n1`.
    pub path: String,
    /// Короткое имя (`sdX`, `nvme0n1`) — как в /sys/class/block.
    pub name: String,
    pub size_bytes: u64,
    pub removable: bool,
    pub model: String,
    /// `Some(true)` — вращающийся диск (HDD), `Some(false)` — SSD/флеш, `None` — неизвестно.
    pub rotational: Option<bool>,
    /// Хотя бы один раздел/сам диск сейчас примонтирован (по данным /proc/mounts).
    pub mounted: bool,
}

impl DeviceInfo {
    pub fn size_human(&self) -> String {
        human_bytes(self.size_bytes)
    }

    pub fn kind_label(&self) -> &'static str {
        match self.rotational {
            Some(true) => "HDD",
            Some(false) => "SSD/флеш",
            None => "?",
        }
    }
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit_idx = 0usize;
    while value >= 1024.0 && unit_idx < UNITS.len() - 1 {
        value /= 1024.0;
        unit_idx += 1;
    }
    if unit_idx == 0 {
        format!("{bytes} {}", UNITS[unit_idx])
    } else {
        format!("{value:.2} {}", UNITS[unit_idx])
    }
}

#[cfg(target_os = "linux")]
pub fn list_block_devices() -> Vec<DeviceInfo> {
    use std::fs;

    let mut result = Vec::new();
    let entries = match fs::read_dir("/sys/class/block") {
        Ok(e) => e,
        Err(_) => return result,
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();

        // Пропускаем виртуальные/служебные устройства, которые почти
        // никогда не являются целью для физического уничтожения данных.
        if name.starts_with("loop") || name.starts_with("ram") || name.starts_with("zram") {
            continue;
        }

        let sys_path = entry.path();

        // У разделов есть файл "partition" — нам нужны только целые диски.
        if sys_path.join("partition").exists() {
            continue;
        }

        let size_sectors: u64 = fs::read_to_string(sys_path.join("size"))
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);
        let size_bytes = size_sectors.saturating_mul(512);
        if size_bytes == 0 {
            // Отсутствующий картовод, пустой NVMe-слот и т.п.
            continue;
        }

        let removable = fs::read_to_string(sys_path.join("removable"))
            .ok()
            .map(|s| s.trim() == "1")
            .unwrap_or(false);

        let model = fs::read_to_string(sys_path.join("device/model"))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "—".to_string());

        let rotational = fs::read_to_string(sys_path.join("queue/rotational"))
            .ok()
            .map(|s| s.trim() == "1");

        let path = format!("/dev/{name}");
        let mounted = is_mounted(&path);

        result.push(DeviceInfo {
            path,
            name,
            size_bytes,
            removable,
            model,
            rotational,
            mounted,
        });
    }

    result.sort_by(|a, b| a.name.cmp(&b.name));
    result
}

#[cfg(target_os = "linux")]
fn is_mounted(dev_path: &str) -> bool {
    // Упрощённая проверка: ищем в /proc/mounts точки монтирования, чей
    // источник начинается с пути устройства (покрывает и /dev/sdX1 при
    // проверке /dev/sdX). Не заменяет `lsblk`/`fuser`, но достаточно для
    // предупреждения в GUI перед подтверждением.
    let contents = match std::fs::read_to_string("/proc/mounts") {
        Ok(c) => c,
        Err(_) => return false,
    };
    contents.lines().any(|line| {
        line.split_whitespace()
            .next()
            .map(|src| src.starts_with(dev_path))
            .unwrap_or(false)
    })
}

#[cfg(not(target_os = "linux"))]
pub fn list_block_devices() -> Vec<DeviceInfo> {
    // GUI пока не предназначен для запуска вне Linux (см. bin/destroyer_gui.rs),
    // но модуль всё равно должен компилироваться на других таргетах.
    Vec::new()
}
