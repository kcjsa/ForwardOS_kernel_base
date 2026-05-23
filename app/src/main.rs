#![no_std]
#![no_main]

mod fscc;  // ← 同じディレクトリの fscc.rs を読み込む

use fscc::{draw_text, exit};
use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
pub extern "C" fn _start() {
    draw_text(100, 200, "Hello at (100,200)!");
}