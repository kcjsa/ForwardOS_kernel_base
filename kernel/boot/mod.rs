// src/boot/mod.rs
use core::ptr::write_volatile;
use core::arch::asm;
extern crate alloc;
use alloc::boxed::Box;
use alloc::vec::Vec;
use alloc::format;
use crate::boot::async_utils::{YieldFuture, KeyboardFuture};
use crate::executor;
use crate::pci;
use crate::net::e1000::E1000;
pub mod fd;
pub mod sys_write_file;
pub mod async_utils;  // 追加
pub mod container;
pub mod exec;
pub const SYS_WRITE: u64 = 0;
pub const SYS_READ: u64 = 1;
pub const SYS_EXIT: u64 = 2;
pub const SYS_OPEN: u64 = 3;
pub const SYS_CLOSE: u64 = 4;
pub const SYS_LSEEK_NUM: u64 = 5;
pub const SYS_FSTAT_NUM: u64 = 6;
pub const SYS_EXEC: u64 = 7;
pub const SYS_YIELD: u64 = 8;
pub const SYS_READ_FILE: u64 = 9;
pub const SYS_WRITE_FILE_NUM: u64 = 10;
pub const SYS_SEEK: u64 = 11;
pub const SYS_MGET: u64 = 13;
pub const SYS_MMAP: u64 = 14;
pub const SYS_WRITE_AT: u64 = 15;

const FONT_BINARY: &[u8] = include_bytes!("../font16.psf");

static mut FB_PTR: *mut u32 = core::ptr::null_mut();
static mut FB_STRIDE: u32 = 0;
static mut FB_WIDTH: u32 = 0;
static mut FB_HEIGHT: u32 = 0;
static mut BACK_BUFFER: [u32; 1920 * 1080] = [0; 1920 * 1080];
static mut PATH_BUF: [u8; 128] = [0; 128];

static mut DEBUG_Y: u32 = 20;
static mut DEBUG_X: u32 = 700;
static mut DEBUG_LINE_COUNT: u32 = 0;
const MAX_DEBUG_LINES: u32 = 15; // 一度に表示する最大行数

pub fn debug_print(s: &str) {
    unsafe {
        // 画面が溢れたら、デバッグ領域だけをクリアして先頭に戻る
        if DEBUG_LINE_COUNT >= MAX_DEBUG_LINES {
            fill_rect(0, 550, FB_WIDTH, 200, 0x00336699); // 下部を黒でクリア
            DEBUG_Y = 560; // Y座標をリセット
            DEBUG_LINE_COUNT = 0;
        }
        
        draw_string(DEBUG_X, DEBUG_Y, s, 0x00FFFF00);
        DEBUG_Y += 12;
        DEBUG_LINE_COUNT += 1;
    }
    flip();
}

pub fn debug_clear() {
    unsafe {
        DEBUG_Y = 600;
        DEBUG_X = 10;
    }
}

pub fn set_framebuffer(ptr: *mut u32, stride: u32, width: u32, height: u32) {
    unsafe { FB_PTR = ptr; FB_STRIDE = stride; FB_WIDTH = width; FB_HEIGHT = height; }
}

pub fn put_pixel(x: u32, y: u32, color: u32) {
    unsafe {
        if x < FB_WIDTH && y < FB_HEIGHT {
            let idx = (y * FB_STRIDE + x) as usize;
            if idx < BACK_BUFFER.len() { BACK_BUFFER[idx] = color; }
        }
    }
}

pub fn fill_screen(color: u32) {
    unsafe {
        let size = (FB_STRIDE * FB_HEIGHT) as usize;
        for i in 0..size.min(BACK_BUFFER.len()) { BACK_BUFFER[i] = color; }
    }
}

pub fn flip() {
    unsafe {
        if !FB_PTR.is_null() {
            let size = (FB_STRIDE * FB_HEIGHT) as usize;
            for i in 0..size.min(BACK_BUFFER.len()) { write_volatile(FB_PTR.add(i), BACK_BUFFER[i]); }
        }
    }
}

pub fn fill_rect(x: u32, y: u32, w: u32, h: u32, color: u32) {
    for dy in 0..h { for dx in 0..w { put_pixel(x + dx, y + dy, color); } }
}

fn get_font_info() -> Option<(usize, usize, usize)> {
    if FONT_BINARY.len() < 4 { return None; }
    if FONT_BINARY[0..4] == [0x72, 0xb5, 0x4a, 0x86] {
        let h_size = u32::from_le_bytes([FONT_BINARY[12], FONT_BINARY[13], FONT_BINARY[14], FONT_BINARY[15]]) as usize;
        let g_size = u32::from_le_bytes([FONT_BINARY[24], FONT_BINARY[25], FONT_BINARY[26], FONT_BINARY[27]]) as usize;
        let h = u32::from_le_bytes([FONT_BINARY[20], FONT_BINARY[21], FONT_BINARY[22], FONT_BINARY[23]]) as usize;
        Some((h_size, g_size, h))
    } else if FONT_BINARY[0..2] == [0x36, 0x04] {
        let h = FONT_BINARY[3] as usize; Some((4, h, h))
    } else { None }
}

pub fn draw_char(x: u32, y: u32, c: u8, color: u32) {
    let (header_size, glyph_size, height) = match get_font_info() { Some(i) => i, _ => return };
    let offset = header_size + (c as usize) * glyph_size;
    if offset + height > FONT_BINARY.len() { return; }
    let glyph = &FONT_BINARY[offset..offset + height];
    for dy in 0..height {
        let row = glyph[dy];
        for dx in 0..8 { if (row << dx) & 0x80 != 0 { put_pixel(x + dx as u32, y + dy as u32, color); } }
    }
}

pub fn draw_string(x: u32, y: u32, s: &str, color: u32) {
    let mut cx = x;
    for c in s.bytes() {
        if c == b'\n' { continue; }  
        draw_char(cx, y, c, color);
        cx += 8;
    }
}

pub fn sys_write(msg: &str) {
    unsafe {
        asm!(
            "mov rax, {}",
            "mov rdi, {}",
            "mov rsi, {}",
            "int 0x80",
            in(reg) SYS_WRITE,
            in(reg) msg.as_ptr() as u64,
            in(reg) msg.len() as u64,
        );
    }
    flip();
}

pub fn sys_read(buf: &mut [u8]) -> usize {
    let ret: u64;
    unsafe {
        asm!(
            "mov rdi, {0}",
            "mov rsi, {1}",
            "mov rax, 1",
            "int 0x80",
            "mov {2}, rax",
            in(reg) buf.as_ptr() as u64,
            in(reg) buf.len() as u64,
            out(reg) ret,
        );
    }
    ret as usize
}

pub fn sys_exit(code: i32) -> ! {
    unsafe {
        asm!(
            "mov rax, {}",
            "mov rdi, {}",
            "int 0x80",
            in(reg) SYS_EXIT,
            in(reg) code as u64,
        );
    }
    loop {}
}

pub fn sys_open(path: &str) -> i32 {
    let len = path.len().min(127);
    unsafe {
        PATH_BUF[..len].copy_from_slice(&path.as_bytes()[..len]);
        PATH_BUF[len] = 0;
        
        asm!(
            "mov rdi, {0}",
            "mov rax, 3",
            "int 0x80",
            in(reg) PATH_BUF.as_ptr() as u64,
        );
        crate::interrupts::SYSCALL_RET as i32
    }
}
// src/boot/mod.rs
pub fn sys_read_file(fd: u32, buf: &mut [u8]) -> isize {
    unsafe {
        asm!(
            "mov rdi, {0}",
            "mov rsi, {1}",
            "mov rdx, {2}",
            "mov rax, 9",
            "int 0x80",
            in(reg) fd as u64,
            in(reg) buf.as_ptr() as u64,
            in(reg) buf.len() as u64,
        );
        crate::interrupts::SYSCALL_RET as isize
    }
}
pub fn sys_close(fd: u32) -> i32 {
    unsafe {
        asm!(
            "mov rdi, {0}",
            "mov rax, 4",
            "int 0x80",
            in(reg) fd as u64,
        );
        crate::interrupts::SYSCALL_RET as i32
    }
}

pub fn SYS_LSEEK(fd: u32, offset: i64, whence: u32) -> i64 {
    unsafe {
        asm!(
            "int 0x80",
            in("rdi") fd as u64,
            in("rsi") offset,
            in("rdx") whence as u64,
            in("rax") 5u64,
            lateout("rax") _,
        );
        crate::interrupts::SYSCALL_RET as i64
    }
}

pub fn SYS_FSTAT(fd: u32, stat_ptr: u64) -> i32 {
    unsafe {
        asm!(
            "int 0x80",
            in("rdi") fd as u64,
            in("rsi") stat_ptr,
            in("rax") 6u64,
            lateout("rax") _,
        );
        crate::interrupts::SYSCALL_RET as i32
    }
}

pub fn SYS_WRITE_FILE(fd: u32, buf_ptr: u64, buf_len: u64) -> isize {
    if buf_ptr == 0 || buf_len == 0 {
        return -1;
    }
    
    unsafe {
        asm!(
            "int 0x80",
            in("rdi") fd as u64,
            in("rsi") buf_ptr,
            in("rdx") buf_len,
            in("rax") 10u64,
            lateout("rax") _,
        );
        crate::interrupts::SYSCALL_RET as isize
    }
}

pub fn sys_seek(fd: u32, pos: u64) -> i64 {
    // デバッグ用の安全ネット: このログがあるだけで、コンパイラがこの関数を不当に最適化するのを防ぐ
    // crate::boot::debug_print("sys_seek called");
    unsafe {
        asm!(
            "int 0x80",
            in("rdi") fd as u64,
            in("rsi") pos,
            in("rax") SYS_SEEK, // SYS_SEEK = 11
            lateout("rax") _,
        );
        crate::interrupts::SYSCALL_RET as i64
    }
}


#[inline(never)]
pub fn sys_mget(pages: u64, alignment: u64, result_ptr: u64) {
    unsafe {
        asm!(
            "int 0x80",
            in("rdi") pages,
            in("rsi") alignment,
            in("rdx") result_ptr,
            in("rax") SYS_MGET,
            lateout("rax") _,
        );
    }
}


#[inline(never)]
pub fn sys_mmap(pages: u64, alignment: u64, result_ptr: u64) {
    unsafe {
        asm!(
            "int 0x80",
            in("rdi") pages,
            in("rsi") alignment,
            in("rdx") result_ptr,
            in("rax") SYS_MMAP,
            lateout("rax") _,
        );
    }
}

// src/boot/mod.rs
#[inline(never)]
pub fn sys_yield() {
    unsafe {
        asm!(
            "int 0x80",
            in("rax") SYS_YIELD,
            lateout("rax") _,
        );
    }
}





pub fn standard_boot() -> ! {
    fill_screen(0x00336699);
    draw_string(200, 200, "Hello, Forward OS!", 0x00FFFFFF);
    flip();
    // standard_boot 内
    let mut mmap_result: u64 = 0;
    sys_mmap(2, 0, &mut mmap_result as *mut u64 as u64);
    draw_string(200, 460, &format!("MMAP 2 pages: 0x{:X}", mmap_result), 0x00FFFF00);
    flip();

    let mut mmap_result2: u64 = 0;
    sys_mmap(1, 4096, &mut mmap_result2 as *mut u64 as u64);
    draw_string(200, 480, &format!("MMAP 1 page aligned: 0x{:X}", mmap_result2), 0x00FFFF00);
    flip();

let mut result: u64 = 0;
sys_mget(1, 0, &mut result as *mut u64 as u64);
draw_string(200, 440, &format!("MGET test: 0x{:X}", result), 0x00FFFF00);
flip();


// 直接 fd::syscall_open を呼ぶ（syscallなし）
let fd = crate::boot::fd::syscall_open("TXT.TXT");
draw_string(200, 220, &alloc::format!("fd={}", fd), 0x00FF00); flip();
flip();
    if fd >= 0 {
        let mut buf = [0u8; 512];
        let n = sys_read_file(fd as u32, &mut buf);
        draw_string(200, 240, &format!("read: {} bytes", n), 0x00FFFF00);
        flip();
        
        if n > 0 {
            let content = unsafe { core::str::from_utf8_unchecked(&buf[..n as usize]) };
            draw_string(200, 260, content, 0x00FFFFFF);
            flip();
        }
        
        let test_data = "Hello from Forward OS!";
        crate::boot::draw_string(10, 500, &alloc::format!("test_data ptr=0x{:X}", test_data.as_ptr() as u64), 0x00FFFF00);
        crate::boot::flip();
        let write_ret = SYS_WRITE_FILE(fd as u32, test_data.as_ptr() as u64, test_data.len() as u64);
        draw_string(200, 280, &format!("write: {}", write_ret), 0x00FFFF00);
        flip();
        
        let seek_ret = SYS_LSEEK(fd as u32, 0, 0);
        draw_string(200, 290, &format!("seek: {}", seek_ret), 0x00FFFF00);
        flip();
        let pos = sys_seek(fd as u32, 0); // 先頭にシーク
        draw_string(200, 430, &format!("Seek pos: {}", pos), 0x00FFFF00);
        flip();
        let mut buf2 = [0u8; 512];
        let n2 = sys_read_file(fd as u32, &mut buf2);
        draw_string(200, 300, &format!("after write: {} bytes", n2), 0x00FFFF00);
        flip();
        
        if n2 > 0 {
            let content2 = unsafe { core::str::from_utf8_unchecked(&buf2[..n2 as usize]) };
            draw_string(200, 320, content2, 0x00FFFFFF);
            flip();
        }
        
        let ret = sys_close(fd as u32);
        draw_string(200, 340, &format!("close: {}", ret), 0x00FFFF00);
        flip();
    }
    
        draw_string(200, 400, "Test Complete!", 0x00FF0000);
    draw_string(200, 420, "Press SHIFT for debug mode", 0x00FFFF00);
    flip();


if let Some(nic_dev) = pci::find_e1000() {
    unsafe {
        let mut e1000 = crate::net::e1000::E1000::new(nic_dev.bar0);
        let our_mac = e1000.mac;
        
        draw_string(200, 520, &format!("NIC MAC: {:02X?}", our_mac), 0x00FF00);
        draw_string(200, 540, "E1000: RX/TX ready", 0x00FFFF00);
        flip();
        
        // ARPパケットを送信
        let arp_pkt: [u8; 42] = [
            0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
            our_mac[0], our_mac[1], our_mac[2], our_mac[3], our_mac[4], our_mac[5],
            0x08, 0x06,
            0x00, 0x01, 0x08, 0x00,
            0x06, 0x04, 0x00, 0x01,
            our_mac[0], our_mac[1], our_mac[2], our_mac[3], our_mac[4], our_mac[5],
            0x0a, 0x00, 0x02, 0x0f,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x0a, 0x00, 0x02, 0x02,
        ];
        
        e1000.send(&arp_pkt);
        draw_string(200, 560, "ARP sent", 0x00FF00);
        flip();
        
        let mut host_mac: [u8; 6] = [0; 6];
        let mut arp_done = false;
        
        draw_string(200, 580, "Listening...", 0x00FFFF00);
        flip();
        
        loop {
            if let Some(pkt) = e1000.poll_receive() {
                let len = pkt.len();
                draw_string(200, 600, &format!("RX: {} bytes", len), 0x00FF00);
                
                // ARP Reply判定
                if len >= 42 && pkt[12] == 0x08 && pkt[13] == 0x06 && pkt[20] == 0x00 && pkt[21] == 0x02 {
                    host_mac = [pkt[22], pkt[23], pkt[24], pkt[25], pkt[26], pkt[27]];
                    arp_done = true;
                    draw_string(200, 620, &format!("Host MAC: {:02X?}", host_mac), 0x00FFFF00);
                    flip();
                    break;
                }
                
                flip();
            }
        }
    }
}

container::init();

let test_container = async {
    fill_screen(0x00336699);
    // コンテナからファイルを開く
    crate::boot::exec::load_and_run("APP.ELF");
    loop {
        crate::boot::sys_yield();
    }
};


    loop {
        let status: u8;
        unsafe { asm!("in al, dx", out("al") status, in("edx") 0x64u16); }
        if (status & 0x01) != 0 {
            let sc: u8;
            unsafe { asm!("in al, dx", out("al") sc, in("edx") 0x60u16); }
            if sc == 0x2A || sc == 0x36 { 
                break;
            }
        }
        unsafe { asm!("pause"); }
    }
    executor::block_on(test_container);
    loop { unsafe { asm!("hlt"); } }
}

pub fn backbuffer_write(x: u32, y: u32, color: u32) {
    put_pixel(x, y, color);
}