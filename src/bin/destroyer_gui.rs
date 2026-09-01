#[cfg(target_os = "linux")]
fn main() -> eframe::Result<()> {
    destroyer::gui::run()
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("destroyer-gui пока поддерживает только Linux.");
    eprintln!("Используйте консольную версию: destroyer <устройство> [опции]");
    std::process::exit(1);
}
