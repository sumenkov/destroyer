//! Десктопный GUI поверх той же библиотеки, что и CLI (`destroyer`).
//! Сейчас реализован и поддерживается только для Linux — см.
//! `src/bin/destroyer_gui.rs`, который на других ОС просто выводит
//! сообщение и завершает работу. Библиотечная логика очистки
//! (`crate::wipe`, `crate::dev`) переиспользуется без изменений.

pub mod app;
pub mod devices;
pub mod worker;

pub use app::run;
