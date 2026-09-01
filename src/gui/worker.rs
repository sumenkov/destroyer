//! Запуск процесса очистки в фоновом потоке, чтобы не блокировать GUI,
//! плюс кооперативная отмена и передача прогресса через mpsc-канал.

use crate::dev::{
    BlockSizes, SyncMode, choose_buffer_size, get_block_sizes, get_device_size_bytes,
    open_device_writable,
};
use crate::wipe::{Buffers, ProgressSnapshot, ProgressTracker, pass_random, pass_zeros};
use std::fs::File;
use std::io::{self, Seek, SeekFrom};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PassKind {
    Random,
    Zeros,
}

pub enum WorkerMsg {
    Started {
        device_size: u64,
        buf_size: usize,
        sector: usize,
    },
    PassStarted {
        pass: usize,
        total: usize,
        kind: PassKind,
    },
    Progress(ProgressSnapshot),
    Finished {
        elapsed: Duration,
    },
    Cancelled,
    Error(String),
}

/// Параметры одного запуска очистки, задаваемые из GUI.
pub struct WipeRequest {
    pub device_path: String,
    pub passes: usize,
    pub buf_size: Option<usize>,
    pub mode: SyncMode,
}

/// Хэндл на запущенный в фоне процесс очистки.
pub struct WorkerHandle {
    pub receiver: Receiver<WorkerMsg>,
    cancel_flag: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl WorkerHandle {
    /// Запросить остановку. Поток завершится между блоками записи
    /// (обычно в течение времени записи одного буфера, доли секунды).
    pub fn request_cancel(&self) {
        self.cancel_flag.store(true, Ordering::Relaxed);
    }

    pub fn is_finished(&self) -> bool {
        self.join
            .as_ref()
            .map(|j| j.is_finished())
            .unwrap_or(true)
    }
}

impl Drop for WorkerHandle {
    fn drop(&mut self) {
        // Если окно закрывают посреди записи — просим поток остановиться
        // и не блокируем UI-поток ожиданием join().
        self.cancel_flag.store(true, Ordering::Relaxed);
    }
}

pub fn spawn_wipe(req: WipeRequest) -> WorkerHandle {
    let (tx, rx) = mpsc::channel();
    let cancel_flag = Arc::new(AtomicBool::new(false));
    let cancel_for_thread = cancel_flag.clone();

    let join = thread::spawn(move || {
        let started = Instant::now();
        match run_wipe(&req, &tx, &cancel_for_thread) {
            Ok(true) => {
                let _ = tx.send(WorkerMsg::Finished {
                    elapsed: started.elapsed(),
                });
            }
            Ok(false) => {
                // Отмена уже сообщена изнутри run_wipe.
            }
            Err(msg) => {
                let _ = tx.send(WorkerMsg::Error(msg));
            }
        }
    });

    WorkerHandle {
        receiver: rx,
        cancel_flag,
        join: Some(join),
    }
}

/// Возвращает `Ok(true)` при успешном завершении всех проходов,
/// `Ok(false)` если процесс был отменён пользователем (сообщение уже отправлено).
fn run_wipe(req: &WipeRequest, tx: &Sender<WorkerMsg>, cancel: &Arc<AtomicBool>) -> Result<bool, String> {
    let device_size = get_device_size_bytes(&req.device_path)
        .map_err(|e| describe_size_err(&req.device_path, &e))?;

    let bs: BlockSizes = get_block_sizes(&req.device_path).unwrap_or(BlockSizes {
        logical: 512,
        physical: 4096,
    });
    let buf_size = choose_buffer_size(bs, req.buf_size);
    let sector = bs.sector() as usize;
    let use_direct = req.mode.is_direct();

    let _ = tx.send(WorkerMsg::Started {
        device_size,
        buf_size,
        sector,
    });

    let mut progress = ProgressTracker::new(req.passes, device_size, true);
    {
        let tx_progress = tx.clone();
        progress.set_on_update(Box::new(move |snap: &ProgressSnapshot| {
            let _ = tx_progress.send(WorkerMsg::Progress(snap.clone()));
        }));
    }

    let mut buffers = Buffers::new(buf_size, use_direct, sector)
        .map_err(|e| format!("Не удалось подготовить буфер записи: {e}"))?;

    let mut main_handle: File =
        open_device_writable(&req.device_path, req.mode).map_err(|e| describe_open_err(&req.device_path, &e))?;

    #[cfg(feature = "direct")]
    let mut tail_handle: Option<File> = if use_direct {
        Some(
            open_device_writable(&req.device_path, SyncMode::Fast)
                .map_err(|e| describe_open_err(&req.device_path, &e))?,
        )
    } else {
        None
    };

    let durable = req.mode.is_durable();

    for pass_idx in 0..req.passes.saturating_sub(1) {
        if cancel.load(Ordering::Relaxed) {
            let _ = tx.send(WorkerMsg::Cancelled);
            return Ok(false);
        }
        let _ = tx.send(WorkerMsg::PassStarted {
            pass: pass_idx + 1,
            total: req.passes,
            kind: PassKind::Random,
        });
        progress.start_pass(pass_idx + 1);
        main_handle
            .seek(SeekFrom::Start(0))
            .map_err(|e| format!("Не удалось вернуть устройство в начало: {e}"))?;

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

        let result = pass_random(
            &mut main_handle,
            device_size,
            durable,
            sector,
            &req.device_path,
            &mut progress,
            &mut buffers,
            tail_ref,
            Some(cancel.as_ref()),
        );
        if was_cancelled(result, tx)? {
            return Ok(false);
        }
    }

    if cancel.load(Ordering::Relaxed) {
        let _ = tx.send(WorkerMsg::Cancelled);
        return Ok(false);
    }
    let _ = tx.send(WorkerMsg::PassStarted {
        pass: req.passes,
        total: req.passes,
        kind: PassKind::Zeros,
    });
    progress.start_pass(req.passes);
    main_handle
        .seek(SeekFrom::Start(0))
        .map_err(|e| format!("Не удалось вернуть устройство в начало: {e}"))?;

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

    let result = pass_zeros(
        &mut main_handle,
        device_size,
        durable,
        sector,
        &req.device_path,
        &mut progress,
        &mut buffers,
        tail_ref,
        Some(cancel.as_ref()),
    );
    if was_cancelled(result, tx)? {
        return Ok(false);
    }

    Ok(true)
}

/// Разобрать результат прохода: `Interrupted` = штатная отмена (сообщаем и
/// возвращаем `true`, чтобы вызывающий код прекратил работу), любая другая
/// ошибка пробрасывается как есть, успех — `Ok(false)` (продолжаем).
fn was_cancelled(result: io::Result<()>, tx: &Sender<WorkerMsg>) -> Result<bool, String> {
    match result {
        Ok(()) => Ok(false),
        Err(e) if e.kind() == io::ErrorKind::Interrupted => {
            let _ = tx.send(WorkerMsg::Cancelled);
            Ok(true)
        }
        Err(e) => Err(format!("Ошибка записи: {e}")),
    }
}

fn describe_size_err(device_path: &str, e: &io::Error) -> String {
    if e.raw_os_error() == Some(libc::EBUSY) {
        format!("{}\n\n{}", e, busy_help(device_path))
    } else {
        format!("Ошибка определения размера устройства: {e}")
    }
}

fn describe_open_err(device_path: &str, e: &io::Error) -> String {
    if e.raw_os_error() == Some(libc::EBUSY) {
        format!("Ошибка открытия устройства: {e}\n\n{}", busy_help(device_path))
    } else {
        format!("Ошибка открытия устройства: {e}")
    }
}

fn busy_help(device_path: &str) -> String {
    format!(
        "Устройство {device_path} занято (возможно, примонтировано).\n\
         Linux:  sudo umount <точка_монтирования_или_/dev/..>\n\
         \x20       lsblk -f | grep $(basename {device_path})\n\
         \x20       sudo lsof {device_path} | head\n\
         \x20       sudo fuser -mv {device_path}\n\
         \x20       sudo swapoff -a\n\
         \x20       sudo dmsetup ls"
    )
}
