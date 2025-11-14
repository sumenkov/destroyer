#[cfg(target_os = "linux")]
fn main() {
    destroyer::platform::linux::run();
}

#[cfg(target_os = "macos")]
fn main() {
    destroyer::platform::macos::run();
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn main() {
    eprintln!("destroyer поддерживает только Linux и macOS.");
    std::process::exit(1);
}
