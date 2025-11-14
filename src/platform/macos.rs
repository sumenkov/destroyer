use crate::app::{self, Platform};

pub fn run() {
    println!("== destroyer/macOS ==");
    app::run(Platform::MacOs);
}
