// src/interrupts.rs
use core::arch::asm;

#[derive(Copy, Clone)]
#[repr(C, packed)]
pub struct IDTEntry {
    offset_low: u16, selector: u16, ist: u8, flags: u8,
    offset_mid: u16, offset_high: u32, _reserved: u32,
}

#[repr(C, packed)]
pub struct IDTR { pub limit: u16, pub base: u64 }

#[repr(C)]
pub struct InterruptStackFrame { pub rip: u64, pub cs: u64, pub rflags: u64, pub rsp: u64, pub ss: u64 }

static mut IDT: [IDTEntry; 256] = [const { IDTEntry::new() }; 256];
static mut KEY_BUFFER: [u8; 16] = [0; 16];
static mut KEY_BUF_HEAD: usize = 0;
static mut KEY_BUF_TAIL: usize = 0;
pub static mut SYSCALL_RET: u64 = 0;
static mut EOI_CALLBACK: Option<unsafe fn()> = None;

impl IDTEntry {
    const fn new() -> Self { Self { offset_low: 0, selector: 0, ist: 0, flags: 0, offset_mid: 0, offset_high: 0, _reserved: 0 } }
    fn set_handler(&mut self, handler: u64) {
        self.offset_low = handler as u16;
        self.selector = 0x38;
        self.ist = 0;
        self.flags = 0x8E;
        self.offset_mid = (handler >> 16) as u16;
        self.offset_high = (handler >> 32) as u32;
        self._reserved = 0;
    }
}

pub unsafe fn init_idt() {
    IDT[14].set_handler(page_fault_handler as u64);
    IDT[0x21].set_handler(keyboard_handler as u64);
    IDT[0x80].set_handler(syscall_int80_handler as u64);
    let idtr = IDTR { limit: (core::mem::size_of::<[IDTEntry; 256]>() - 1) as u16, base: &IDT as *const _ as u64 };
    asm!("lidt [{}]", in(reg) &idtr);
}

pub fn get_key_from_buffer() -> Option<u8> {
    unsafe {
        if KEY_BUF_HEAD != KEY_BUF_TAIL {
            let sc = KEY_BUFFER[KEY_BUF_TAIL];
            KEY_BUF_TAIL = (KEY_BUF_TAIL + 1) % 16;
            Some(sc)
        } else { None }
    }
}

pub fn set_eoi_callback(cb: unsafe fn()) { unsafe { EOI_CALLBACK = Some(cb); } }

extern "x86-interrupt" fn page_fault_handler(_: InterruptStackFrame, _: u64) { loop { unsafe { asm!("hlt") } } }

extern "x86-interrupt" fn keyboard_handler(_: InterruptStackFrame) {
    unsafe {
        let sc: u8; asm!("in al, dx", out("al") sc, in("edx") 0x60u16);
        let next = (KEY_BUF_HEAD + 1) % 16;
        if next != KEY_BUF_TAIL { KEY_BUFFER[KEY_BUF_HEAD] = sc; KEY_BUF_HEAD = next; }
        crate::apic::wake_keyboard();
        crate::executor::wake();
        if let Some(cb) = EOI_CALLBACK { cb(); }
    }
}

extern "x86-interrupt" fn syscall_int80_handler(_: InterruptStackFrame) {
    unsafe {
        let syscall_num: u64; asm!("mov {}, rax", out(reg) syscall_num);
        match syscall_num {
            0 => {
                let ptr: u64; let len: u64;
                asm!("mov {}, rdi", out(reg) ptr); asm!("mov {}, rsi", out(reg) len);
                let msg = core::slice::from_raw_parts(ptr as *const u8, len as usize);
                crate::boot::draw_string(10, 400, core::str::from_utf8_unchecked(msg), 0x00FFFFFF);
                crate::boot::flip(); asm!("mov rax, 0");
            }
            1 => {
                let buf_ptr: u64; let buf_len: u64;
                asm!("mov {}, rdi", out(reg) buf_ptr); asm!("mov {}, rsi", out(reg) buf_len);
                let mut sc: u8 = 0;
                loop { let s: u8; asm!("in al, dx", out("al") s, in("edx") 0x64u16); if (s & 0x01) != 0 { asm!("in al, dx", out("al") sc, in("edx") 0x60u16); if sc < 0x80 && sc != 0 { break; } } core::hint::spin_loop(); }
                if buf_len > 0 && buf_ptr != 0 { (buf_ptr as *mut u8).write_volatile(sc); asm!("mov rax, 1"); } else { asm!("mov rax, -1"); }
            }
// interrupts.rs

//2は動きません、taskを終了できないです。　使用しないでください　(CのABIを使ってください)
2 => { 
    crate::boot::draw_string(10, 600, "Program exited.", 0x00FF0000); 
    crate::boot::flip();
    SYSCALL_RET = 0;
    return;
}
            3 => {
                let path_ptr: u64; asm!("mov {}, rdi", out(reg) path_ptr);
                let mut buf = [0u8; 128];
                for i in 0..128 { let b = (path_ptr as *const u8).add(i).read_volatile(); buf[i] = b; if b == 0 { break; } }
                let len = buf.iter().position(|&b| b == 0).unwrap_or(128);
                SYSCALL_RET = crate::boot::fd::syscall_open(core::str::from_utf8_unchecked(&buf[..len])) as u64;
            }
            4 => { let fd: u64; asm!("mov {}, rdi", out(reg) fd); SYSCALL_RET = if crate::boot::fd::close_fd(fd as u32) { 0 } else { -1i64 as u64 }; }
            5 => { let fd: u64; let off: i64; let w: u64; asm!("mov {}, rdi", out(reg) fd); asm!("mov {}, rsi", out(reg) off); asm!("mov {}, rdx", out(reg) w); SYSCALL_RET = crate::boot::fd::sys_lseek(fd as u32, off, w as u32) as u64; }
            6 => {
                let fd: u64; let buf: u64; asm!("mov {}, rdi", out(reg) fd); asm!("mov {}, rsi", out(reg) buf);
                SYSCALL_RET = if buf != 0 && crate::boot::fd::sys_fstat(fd as u32, buf) { 0 } else { -1i64 as u64 };
            }
            8 => { crate::executor::wake(); SYSCALL_RET = 0; }
            9 => {
                let fd: u64; let buf: u64; let len: u64; asm!("mov {}, rdi", out(reg) fd); asm!("mov {}, rsi", out(reg) buf); asm!("mov {}, rdx", out(reg) len);
                SYSCALL_RET = if buf != 0 && len > 0 { crate::boot::fd::sys_read_file(fd as u32, buf, len) as u64 } else { -1isize as u64 };
            }
            10 => {
                let fd: u64; let buf: u64; let len: u64; asm!("mov {}, rdi", out(reg) fd); asm!("mov {}, rsi", out(reg) buf); asm!("mov {}, rdx", out(reg) len);
                SYSCALL_RET = crate::boot::sys_write_file::sys_write_file(fd as u32, buf, len) as u64;
            }
            11 => { let fd: u64; let pos: u64; asm!("mov {}, rdi", out(reg) fd); asm!("mov {}, rsi", out(reg) pos); SYSCALL_RET = crate::boot::fd::sys_seek(fd as u32, pos) as u64; }
            13 => {
                let pages: u64; let align: u64; let res: u64; asm!("mov {}, rdi", out(reg) pages); asm!("mov {}, rsi", out(reg) align); asm!("mov {}, rdx", out(reg) res);
                core::ptr::write_volatile(res as *mut u64, crate::ram::allocate_sectors_direct(pages as usize)); SYSCALL_RET = 0;
            }
            14 => {
                let pages: u64; let align: u64; let res: u64; asm!("mov {}, rdi", out(reg) pages); asm!("mov {}, rsi", out(reg) align); asm!("mov {}, rdx", out(reg) res);
                crate::boot::sys_mget(pages, align, res); SYSCALL_RET = 0;
            }
15 => { // SYS_WRITE_AT
    let x: u64;
    let y: u64;
    let ptr: u64;
    let len: u64;
    unsafe {
        asm!("mov {}, rdi", out(reg) x);
        asm!("mov {}, rsi", out(reg) y);
        asm!("mov {}, rdx", out(reg) ptr);
        asm!("mov {}, rcx", out(reg) len);
    }
    
    // デバッグ出力
    crate::boot::debug_print(&alloc::format!(
        "[SYS_WRITE_AT] x={}, y={}, ptr=0x{:X}, len={}",
        x, y, ptr, len
    ));
    
    if ptr == 0 || len == 0 {
        crate::boot::debug_print("[SYS_WRITE_AT] invalid ptr or len");
        unsafe { asm!("mov rax, -1"); }
        return;
    }
    
    let msg = unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) };
    let len_usize = len as usize;
    
    // 16進ダンプ
    let mut hex = alloc::string::String::new();
    for i in 0..len_usize.min(16) {
        hex.push_str(&alloc::format!("{:02X} ", msg[i]));
    }
    crate::boot::debug_print(&alloc::format!("[SYS_WRITE_AT] data: {}", hex));
    
    // UTF-8として表示を試みる
    match core::str::from_utf8(msg) {
        Ok(msg_str) => {
            crate::boot::debug_print(&alloc::format!("[SYS_WRITE_AT] as utf8: {}", msg_str));
            crate::boot::draw_string(x as u32, y as u32, msg_str, 0x00FFFFFF);
            crate::boot::flip();
        }
        Err(e) => {
            crate::boot::debug_print(&alloc::format!("[SYS_WRITE_AT] invalid utf8: {}", e));
            unsafe { asm!("mov rax, -1"); }
            return;
        }
    }
    
    unsafe { asm!("mov rax, 0"); }
}
33 => {  // SYS_MUNMAP
    let addr: u64; let pages: u64;
    asm!("mov {}, rdi", out(reg) addr);
    asm!("mov {}, rsi", out(reg) pages);
    
    // メモリ解放
    let start_sector = (addr / 4096) as usize;
    for i in 0..pages as usize {
        crate::ram::RAM_MGR.as_ref().unwrap().mark_free(start_sector + i);
    }
    
    SYSCALL_RET = 0;
}

            _ => { asm!("mov rax, -1"); }
        }
    }
}