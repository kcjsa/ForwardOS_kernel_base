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
pub mod exec;// src/boot/mod.rs の先頭付近
pub mod timer;  // 追加
pub const SYS_WRITE: u64 = 0; //ok
pub const SYS_READ: u64 = 1; //ok 
pub const SYS_EXIT: u64 = 2; //ok 
pub const SYS_OPEN: u64 = 3; //ok 
pub const SYS_CLOSE: u64 = 4; //ok 
pub const SYS_LSEEK_NUM: u64 = 5; //ok 
pub const SYS_FSTAT_NUM: u64 = 6; //ok 
pub const SYS_EXEC: u64 = 7; //ok 
pub const SYS_YIELD: u64 = 8; //ok  
pub const SYS_READ_FILE: u64 = 9; //ok 
pub const SYS_WRITE_FILE_NUM: u64 = 10; //ok 
pub const SYS_SEEK: u64 = 11; //ok 
pub const SYS_MGET: u64 = 13; //ok 
pub const SYS_MMAP: u64 = 14; //ok 
pub const SYS_WRITE_AT: u64 = 15; //ok 
pub const SYS_FORK: u64 = 16;
pub const SYS_WAITPID: u64 = 17;
pub const SYS_GETPID: u64 = 18;
pub const SYS_EXECVE: u64 = 19;  // 今のSYS_EXECは引数なし
pub const SYS_SOCKET: u64 = 20;
pub const SYS_BIND: u64 = 21;
pub const SYS_LISTEN: u64 = 22;
pub const SYS_ACCEPT: u64 = 23;
pub const SYS_CONNECT: u64 = 24;
pub const SYS_SEND: u64 = 25;
pub const SYS_RECV: u64 = 26;
pub const SYS_MKDIR: u64 = 27;
pub const SYS_RMDIR: u64 = 28;
pub const SYS_UNLINK: u64 = 29;
pub const SYS_STAT: u64 = 30;
pub const SYS_CHDIR: u64 = 31;
pub const SYS_GETCWD: u64 = 32;
pub const SYS_MUNMAP: u64 = 33;  //ok  メモリ解放
pub const SYS_MPROTECT: u64 = 34;  // 保護設定

const FONT_BINARY: &[u8] = include_bytes!("../font16.psf");

static mut FB_PTR: *mut u32 = core::ptr::null_mut();
static mut FB_STRIDE: u32 = 0;
static mut FB_WIDTH: u32 = 0;
static mut FB_HEIGHT: u32 = 0;
static mut BACK_BUFFER: [u32; 1920 * 1080] = [0; 1920 * 1080];
static mut PATH_BUF: [u8; 128] = [0; 128];

static mut DEBUG_Y: u32 = 20;
static mut DEBUG_X: u32 = 400;
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

#[inline(never)]
pub fn sys_munmap(addr: u64, pages: u64) -> i64 {
    unsafe {
        asm!(
            "int 0x80",
            in("rdi") addr,
            in("rsi") pages,
            in("rax") SYS_MUNMAP,
            lateout("rax") _,
        );
        crate::interrupts::SYSCALL_RET as i64
    }
}


pub fn standard_boot() -> ! {
    fill_screen(0x00336699);
    draw_string(200, 200, "Hello, Forward OS!", 0x00FFFFFF);
    flip();
    
    let mut result: u64 = 0;
    sys_mget(1, 0, &mut result as *mut u64 as u64);
    draw_string(200, 440, &format!("MGET test: 0x{:X}", result), 0x00FFFF00);
    flip();

    // ファイルテスト
    let fd = crate::boot::fd::syscall_open("TXT.TXT");
    draw_string(200, 220, &alloc::format!("fd={}", fd), 0x00FF00);
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
        
        let pos = sys_seek(fd as u32, 0);
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

    // ネットワーク初期化
    if let Some(nic_dev) = pci::find_e1000() {
        unsafe {
            let mut e1000 = crate::net::e1000::E1000::new(nic_dev.bar0);
            let our_mac = e1000.mac;
            
            draw_string(200, 520, &format!("NIC MAC: {:02X?}", our_mac), 0x00FF00);
            draw_string(200, 540, "E1000: RX/TX ready", 0x00FFFF00);
            flip();
            
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
            
            draw_string(200, 580, "Listening...", 0x00FFFF00);
            flip();
            
            loop {
                if let Some(pkt) = e1000.poll_receive() {
                    let len = pkt.len();
                    draw_string(200, 600, &format!("RX: {} bytes", len), 0x00FF00);
                    
                    if len >= 42 && pkt[12] == 0x08 && pkt[13] == 0x06 && pkt[20] == 0x00 && pkt[21] == 0x02 {
                        host_mac = [pkt[22], pkt[23], pkt[24], pkt[25], pkt[26], pkt[27]];
                        draw_string(200, 620, &format!("Host MAC: {:02X?}", host_mac), 0x00FFFF00);
                        flip();
                        break;
                    }
                    flip();
                }
            }
        }
    }

    // ============================================
    // SYS_MUNMAP テスト
    // ============================================
    
    draw_string(200, 700, "=== SYS_MUNMAP Test ===", 0x00FFFF00);
    flip();
    
    // メモリ確保
    let mut test_addr: u64 = 0;
    sys_mmap(1, 4096, &mut test_addr as *mut u64 as u64);
    draw_string(200, 720, &alloc::format!("Allocated: 0x{:X}", test_addr), 0x00FF00);
    flip();
    
    // 書き込みテスト
    if test_addr != 0 && test_addr != 0xFFFFFFFFFFFFFFFF {
        unsafe {
            let ptr = test_addr as *mut u64;
            ptr.write(0x12345678);
            let val = ptr.read();
            draw_string(200, 740, &alloc::format!("Write/Read: 0x{:X}", val), 0x00FF00);
            flip();
        }
    }
    
    // メモリ解放
    let munmap_ret = sys_munmap(test_addr, 1);
    draw_string(200, 760, &alloc::format!("Munmap returned: {}", munmap_ret), 0x00FFFF);
    flip();
    
    // 再度同じサイズ確保（別のアドレスかもしれない）
    let mut test_addr2: u64 = 0;
    sys_mmap(1, 4096, &mut test_addr2 as *mut u64 as u64);
    draw_string(200, 780, &alloc::format!("Re-allocated: 0x{:X}", test_addr2), 0x00FF00);
    flip();
    
    // ============================================
    // コンテナ（新設計）
    // ============================================
    
    // コンテナ初期化
    container::init();

    // コンテナ専用メモリを確保（.ram_mgr領域から）
    let mut container_mem: u64 = 0;
    sys_mmap(64, 4096, &mut container_mem as *mut u64 as u64);
    draw_string(200, 660, &alloc::format!("Container mem: 0x{:X}", container_mem), 0x00FFFF00);
    flip();

    // コンテナを作成
    let container_id = container::create_container(container_mem, 64, "test_container");
    draw_string(200, 640, &alloc::format!("Container ID: {}", container_id), 0x00FFFF00);
    flip();

    unsafe {
        timer::init_timer();
        executor::block_on(async {
            let start = timer::get_timer().now();
            async_utils::sleep_ms(1000).await;
            let end = timer::get_timer().now();
            
            let elapsed_ms = (end - start) * 1000 / timer::get_timer().frequency();
            draw_string(200, 500, &alloc::format!("Sleep 1s -> actual {} ms", elapsed_ms), 0x00FFFF00);
            flip();
        })
    }

    // Shiftキー待ち
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
    
    fill_screen(0x00336699);
    
    // コンテナ内で実行
    let container_future = container::run_in_container(container_id, async {
        draw_string(200, 680, "[CONTAINER] Started!", 0x00FF00);
        flip();
        
        let ret = crate::boot::exec::load_and_run_container("APP.ELF");
        draw_string(150, 700, &alloc::format!("[CONTAINER] ELF returned: {}", ret), 0x00FFFF00);
        flip();
        
        draw_string(150, 720, "[CONTAINER] Work done!", 0x00FF00);
        flip();
    });

    executor::block_on(container_future);

    draw_string(150, 740, "[CONTAINER] Finished!", 0x00FF00);
    flip();

    loop { unsafe { asm!("hlt"); } }
}

pub fn backbuffer_write(x: u32, y: u32, color: u32) {
    put_pixel(x, y, color);
}