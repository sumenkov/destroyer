use crate::dev::SyncMode;
use crate::gui::devices::{DeviceInfo, list_block_devices};
use crate::gui::worker::{PassKind, WipeRequest, WorkerHandle, WorkerMsg, spawn_wipe};
use crate::wipe::ProgressSnapshot;
use eframe::egui;
use std::time::Duration;

pub fn run() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([760.0, 600.0])
            .with_min_inner_size([600.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native(
        "destroyer — безопасное уничтожение диска",
        options,
        Box::new(|_cc| Ok(Box::new(DestroyerApp::new()))),
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ModeChoice {
    Fast,
    Durable,
    Direct,
}

impl ModeChoice {
    fn to_sync_mode(self) -> SyncMode {
        match self {
            ModeChoice::Fast => SyncMode::Fast,
            #[cfg(feature = "durable")]
            ModeChoice::Durable => SyncMode::Durable,
            #[cfg(not(feature = "durable"))]
            ModeChoice::Durable => SyncMode::Fast,
            #[cfg(all(feature = "direct", target_os = "linux"))]
            ModeChoice::Direct => SyncMode::Direct,
            #[cfg(not(all(feature = "direct", target_os = "linux")))]
            ModeChoice::Direct => SyncMode::Fast,
        }
    }
}

/// Состояние экрана. `Setup` используется также как дешёвый "заполнитель"
/// при временном изъятии состояния через `std::mem::replace` в `draw_screen`
/// (см. комментарий там про то, зачем это нужно).
enum Screen {
    Setup,
    Confirm {
        device_path: String,
        typed: String,
        ack: bool,
    },
    Running {
        handle: WorkerHandle,
        device_path: String,
        device_size: u64,
        total_passes: usize,
        current_pass: usize,
        current_kind: Option<PassKind>,
        last_progress: Option<ProgressSnapshot>,
        log: Vec<String>,
    },
    Done {
        device_path: String,
        message: String,
        kind: DoneKind,
    },
}

#[derive(PartialEq)]
enum DoneKind {
    Success,
    Cancelled,
    Error,
}

enum ConfirmAction {
    None,
    Back,
    Start,
}

struct DestroyerApp {
    devices: Vec<DeviceInfo>,
    selected: Option<usize>,
    manual_path: String,
    use_manual: bool,
    passes_str: String,
    mode: ModeChoice,
    buf_str: String,
    screen: Screen,
}

impl DestroyerApp {
    fn new() -> Self {
        Self {
            devices: list_block_devices(),
            selected: None,
            manual_path: String::new(),
            use_manual: false,
            passes_str: "8".to_string(),
            mode: ModeChoice::Fast,
            buf_str: String::new(),
            screen: Screen::Setup,
        }
    }

    fn selected_device_path(&self) -> Option<String> {
        if self.use_manual {
            let p = self.manual_path.trim();
            if p.is_empty() { None } else { Some(p.to_string()) }
        } else {
            self.selected
                .and_then(|i| self.devices.get(i))
                .map(|d| d.path.clone())
        }
    }

    fn passes(&self) -> usize {
        self.passes_str.trim().parse::<usize>().unwrap_or(0).max(1)
    }

    fn buf_size(&self) -> Option<usize> {
        let s = self.buf_str.trim();
        if s.is_empty() {
            None
        } else {
            s.parse::<usize>().ok().filter(|v| *v > 0)
        }
    }

    /// Забрать сообщения от фонового потока очистки, если он сейчас запущен.
    /// Отдельный метод, вызывается до отрисовки, чтобы UI показывал самые
    /// свежие данные за этот кадр.
    fn poll_worker(&mut self, ctx: &egui::Context) {
        let mut pending_finish: Option<(String, String, DoneKind)> = None;

        if let Screen::Running {
            handle,
            device_path,
            device_size,
            current_pass,
            current_kind,
            last_progress,
            log,
            ..
        } = &mut self.screen
        {
            while let Ok(msg) = handle.receiver.try_recv() {
                match msg {
                    WorkerMsg::Started {
                        device_size: ds,
                        buf_size,
                        sector,
                    } => {
                        *device_size = ds;
                        log.push(format!(
                            "Устройство: {ds} байт, буфер {buf_size} байт, сектор {sector} байт"
                        ));
                    }
                    WorkerMsg::PassStarted { pass, total, kind } => {
                        *current_pass = pass;
                        *current_kind = Some(kind);
                        let kind_label = match kind {
                            PassKind::Random => "случайные данные",
                            PassKind::Zeros => "нули",
                        };
                        log.push(format!("Проход {pass}/{total} ({kind_label})"));
                    }
                    WorkerMsg::Progress(snap) => {
                        *last_progress = Some(snap);
                    }
                    WorkerMsg::Finished { elapsed } => {
                        pending_finish = Some((
                            device_path.clone(),
                            format!("Устройство успешно очищено за {elapsed:.1?}."),
                            DoneKind::Success,
                        ));
                    }
                    WorkerMsg::Cancelled => {
                        pending_finish = Some((
                            device_path.clone(),
                            "Очистка отменена пользователем. Часть данных уже перезаписана — \
                             считайте их разрушенными, но полная очистка не гарантирована."
                                .to_string(),
                            DoneKind::Cancelled,
                        ));
                    }
                    WorkerMsg::Error(err) => {
                        pending_finish = Some((device_path.clone(), err, DoneKind::Error));
                    }
                }
            }
            // Пока идёт запись, просим перерисовываться сами по себе, чтобы
            // прогресс-бар обновлялся, даже если пользователь не двигает мышь.
            ctx.request_repaint_after(Duration::from_millis(120));
        }

        if let Some((device_path, message, kind)) = pending_finish {
            self.screen = Screen::Done {
                device_path,
                message,
                kind,
            };
        }
    }

    /// Экран выбора устройства и параметров. Возвращает `Some(путь)`, когда
    /// пользователь нажал "Продолжить" — вызывающая сторона решает, что
    /// делать с переходом состояния.
    fn draw_setup_fields(&mut self, ui: &mut egui::Ui) -> Option<String> {
        ui.heading("destroyer — безопасное многопроходное уничтожение диска");
        ui.colored_label(
            egui::Color32::from_rgb(220, 60, 60),
            "⚠ Эта программа необратимо стирает данные на выбранном устройстве целиком.",
        );
        ui.separator();

        ui.horizontal(|ui| {
            ui.label("1. Устройство");
            if ui.button("🔄 Обновить список").clicked() {
                self.devices = list_block_devices();
                self.selected = None;
            }
        });

        if self.devices.is_empty() {
            ui.label("Диски не найдены (или недостаточно прав — запустите через sudo).");
        }

        egui::ScrollArea::vertical()
            .max_height(220.0)
            .show(ui, |ui| {
                for (idx, dev) in self.devices.iter().enumerate() {
                    let selected = !self.use_manual && self.selected == Some(idx);
                    let label = format!(
                        "{}   {}   {}   {}{}",
                        dev.path,
                        dev.size_human(),
                        dev.kind_label(),
                        dev.model,
                        if dev.mounted { "   [ПРИМОНТИРОВАН]" } else { "" },
                    );
                    let mut rich = egui::RichText::new(label);
                    if dev.mounted {
                        rich = rich.color(egui::Color32::from_rgb(220, 140, 40));
                    }
                    if ui.selectable_label(selected, rich).clicked() {
                        self.use_manual = false;
                        self.selected = Some(idx);
                    }
                }
            });

        ui.horizontal(|ui| {
            ui.checkbox(&mut self.use_manual, "Указать путь вручную:");
            let edit = ui.add_enabled(
                self.use_manual,
                egui::TextEdit::singleline(&mut self.manual_path).hint_text("/dev/sdX"),
            );
            if edit.gained_focus() {
                self.use_manual = true;
            }
        });

        ui.add_space(8.0);
        ui.label("2. Параметры");
        egui::Grid::new("options_grid")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                ui.label("Число проходов (последний — нули):");
                ui.add(egui::TextEdit::singleline(&mut self.passes_str).desired_width(60.0));
                ui.end_row();

                ui.label("Режим:");
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.mode, ModeChoice::Fast, "fast");
                    ui.add_enabled_ui(cfg!(feature = "durable"), |ui| {
                        ui.selectable_value(&mut self.mode, ModeChoice::Durable, "durable");
                    });
                    ui.add_enabled_ui(cfg!(all(feature = "direct", target_os = "linux")), |ui| {
                        ui.selectable_value(&mut self.mode, ModeChoice::Direct, "direct (O_DIRECT)");
                    });
                });
                ui.end_row();

                ui.label("Буфер записи (байт, пусто = авто):");
                ui.add(
                    egui::TextEdit::singleline(&mut self.buf_str)
                        .hint_text("авто ~64 KiB")
                        .desired_width(120.0),
                );
                ui.end_row();
            });

        ui.add_space(12.0);
        let device_path = self.selected_device_path();
        let can_continue = device_path.is_some() && self.passes() >= 1;

        let mut go = false;
        ui.add_enabled_ui(can_continue, |ui| {
            if ui
                .add_sized(
                    [220.0, 36.0],
                    egui::Button::new("Продолжить →").fill(egui::Color32::from_rgb(180, 40, 40)),
                )
                .clicked()
            {
                go = true;
            }
        });
        if !can_continue {
            ui.label("Выберите устройство и задайте число проходов, чтобы продолжить.");
        }

        if go { device_path } else { None }
    }

    fn draw_confirm(
        ui: &mut egui::Ui,
        device_path: &str,
        typed: &mut String,
        ack: &mut bool,
    ) -> ConfirmAction {
        let mut action = ConfirmAction::None;
        ui.heading("Последняя проверка перед уничтожением данных");
        ui.add_space(6.0);
        ui.colored_label(
            egui::Color32::from_rgb(220, 60, 60),
            format!("Все данные на «{device_path}» будут стёрты БЕЗВОЗВРАТНО."),
        );
        ui.label(
            "Если это внешний диск с важными данными — сейчас последний шанс отменить \
             и отключить его.",
        );
        ui.add_space(10.0);

        ui.label(format!(
            "Чтобы подтвердить, наберите путь устройства ровно так: {device_path}"
        ));
        ui.add(egui::TextEdit::singleline(typed).hint_text(device_path));
        ui.checkbox(
            ack,
            "Я понимаю, что это действие необратимо и данные будет невозможно восстановить.",
        );

        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui.button("← Назад").clicked() {
                action = ConfirmAction::Back;
            }
            let matches = typed.trim() == device_path;
            ui.add_enabled_ui(matches && *ack, |ui| {
                if ui
                    .add_sized(
                        [220.0, 36.0],
                        egui::Button::new("Стереть безвозвратно")
                            .fill(egui::Color32::from_rgb(150, 20, 20)),
                    )
                    .clicked()
                {
                    action = ConfirmAction::Start;
                }
            });
            if !matches {
                ui.label("Путь не совпадает.");
            }
        });
        action
    }

    fn draw_running_fields(
        ui: &mut egui::Ui,
        handle: &mut WorkerHandle,
        device_path: &str,
        total_passes: usize,
        current_pass: usize,
        current_kind: Option<PassKind>,
        last_progress: &Option<ProgressSnapshot>,
        log: &[String],
    ) {
        ui.heading(format!("Идёт очистка: {device_path}"));
        let kind_label = match current_kind {
            Some(PassKind::Random) => "случайные данные",
            Some(PassKind::Zeros) => "нули",
            None => "подготовка…",
        };
        ui.label(format!("Проход {current_pass}/{total_passes} — {kind_label}"));

        if let Some(snap) = last_progress {
            ui.add(
                egui::ProgressBar::new((snap.pass_percent / 100.0) as f32)
                    .text(format!("Проход: {:.0}%", snap.pass_percent)),
            );
            ui.add(
                egui::ProgressBar::new((snap.total_percent / 100.0) as f32)
                    .text(format!("Всего: {:.0}%", snap.total_percent)),
            );
            ui.horizontal(|ui| {
                ui.label(format!("Осталось (проход): {}", fmt_eta(snap.pass_eta)));
                ui.separator();
                ui.label(format!("Осталось (всего): {}", fmt_eta(snap.total_eta)));
            });
        } else {
            ui.add(egui::ProgressBar::new(0.0).text("Подготовка устройства…"));
        }

        ui.add_space(10.0);
        if ui.button("Отменить").clicked() {
            handle.request_cancel();
        }

        ui.add_space(10.0);
        ui.collapsing("Журнал", |ui| {
            egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                for line in log.iter() {
                    ui.label(line);
                }
            });
        });
    }

    /// Основная отрисовка + переходы между экранами.
    ///
    /// Состояние `self.screen` временно изымается через `std::mem::replace`
    /// (с дешёвым `Screen::Setup` как заглушкой). Пока мы работаем с
    /// изъятым локальным значением `screen`, поле `self.screen` не
    /// заимствовано вообще, поэтому внутри веток можно свободно вызывать
    /// методы `self` (например, обновлять список устройств) — это устраняет
    /// целый класс конфликтов заимствования, которые иначе возникли бы при
    /// `match &mut self.screen { ... self.some_method() ... }`.
    fn draw_screen(&mut self, ui: &mut egui::Ui, passes: usize, buf_size: Option<usize>, mode: SyncMode) {
        let mut screen = std::mem::replace(&mut self.screen, Screen::Setup);
        let mut next_screen: Option<Screen> = None;

        match &mut screen {
            Screen::Setup => {
                if let Some(device_path) = self.draw_setup_fields(ui) {
                    next_screen = Some(Screen::Confirm {
                        device_path,
                        typed: String::new(),
                        ack: false,
                    });
                }
            }
            Screen::Confirm { device_path, typed, ack } => {
                match Self::draw_confirm(ui, device_path, typed, ack) {
                    ConfirmAction::None => {}
                    ConfirmAction::Back => {
                        next_screen = Some(Screen::Setup);
                    }
                    ConfirmAction::Start => {
                        let req = WipeRequest {
                            device_path: device_path.clone(),
                            passes,
                            buf_size,
                            mode,
                        };
                        let handle = spawn_wipe(req);
                        next_screen = Some(Screen::Running {
                            handle,
                            device_path: device_path.clone(),
                            device_size: 0,
                            total_passes: passes,
                            current_pass: 0,
                            current_kind: None,
                            last_progress: None,
                            log: Vec::new(),
                        });
                    }
                }
            }
            Screen::Running {
                handle,
                device_path,
                total_passes,
                current_pass,
                current_kind,
                last_progress,
                log,
                ..
            } => {
                Self::draw_running_fields(
                    ui,
                    handle,
                    device_path,
                    *total_passes,
                    *current_pass,
                    *current_kind,
                    last_progress,
                    log,
                );
            }
            Screen::Done { device_path, message, kind } => {
                match kind {
                    DoneKind::Success => {
                        ui.heading("✅ Готово");
                        ui.label(format!("{device_path}: {message}"));
                    }
                    DoneKind::Cancelled => {
                        ui.heading("⏹ Отменено");
                        ui.colored_label(egui::Color32::from_rgb(220, 140, 40), message.as_str());
                    }
                    DoneKind::Error => {
                        ui.heading("❌ Ошибка");
                        ui.colored_label(egui::Color32::from_rgb(220, 60, 60), message.as_str());
                    }
                }
                ui.add_space(12.0);
                if ui.button("← К списку устройств").clicked() {
                    self.devices = list_block_devices();
                    self.selected = None;
                    next_screen = Some(Screen::Setup);
                }
            }
        }

        self.screen = next_screen.unwrap_or(screen);
    }
}

impl eframe::App for DestroyerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_worker(ctx);

        // Считаны до входа в отрисовку, чтобы не требовать заимствования
        // self ещё раз внутри draw_screen (см. комментарий там).
        let passes = self.passes();
        let buf_size = self.buf_size();
        let mode = self.mode.to_sync_mode();

        egui::CentralPanel::default().show(ctx, |ui| {
            self.draw_screen(ui, passes, buf_size, mode);
        });
    }
}

fn fmt_eta(eta: Option<Duration>) -> String {
    match eta {
        Some(d) => {
            let secs = d.as_secs();
            format!(
                "{:02}:{:02}:{:02}",
                secs / 3600,
                (secs % 3600) / 60,
                secs % 60
            )
        }
        None => "--:--:--".to_string(),
    }
}
