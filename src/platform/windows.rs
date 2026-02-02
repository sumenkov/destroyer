use crate::app::{self, Platform};

pub fn run() {
    println!("== destroyer/Windows ==");
    app::run(Platform::Windows);
}
