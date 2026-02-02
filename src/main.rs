#[cfg(target_os = "linux")]
fn main() {
    destroyer::platform::linux::run();
}

#[cfg(target_os = "macos")]
fn main() {
    destroyer::platform::macos::run();
}

#[cfg(target_os = "windows")]
fn main() {
    destroyer::platform::windows::run();
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn main() {
    eprintln!("destroyer поддерживает только Linux, macOS и Windows.");
    std::process::exit(1);
}
