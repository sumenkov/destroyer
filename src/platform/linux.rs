use crate::app::{self, Platform};

pub fn run() {
    println!("== destroyer/Linux ==");
    app::run(Platform::Linux);
}
