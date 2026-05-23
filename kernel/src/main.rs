// src/main.rs
#![no_std]
#![no_main]
#![feature(alloc_error_handler)]
#![feature(abi_x86_interrupt)]

mod draw; mod ram; mod paging; mod interrupts; mod pci; mod keyboard; mod ahci; mod file; mod apic; mod executor; mod boot; mod net;

extern crate alloc;
use core::panic::PanicInfo;
use linked_list_allocator::LockedHeap;
use alloc::format;
use core::arch::asm;
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();
static mut IS_SHIFT: bool = false;
static mut CURRENT_DIR_CLUSTER: u32 = 0;
static mut AHCI_ABAR: u64 = 0;
static mut AHCI_PORT: i32 = -1;
static mut AHCI_CT_BASE: u64 = 0;
static mut AHCI_DATA_PTR: u64 = 0;

pub struct KeyboardFuture;

impl Future for KeyboardFuture {
    type Output = u8;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<u8> {
        apic::register_keyboard_waker(cx.waker().clone());
        unsafe {
            if let Some(sc) = interrupts::get_key_from_buffer() {
                if sc == 0x2A || sc == 0x36 { IS_SHIFT = true; }
                if sc == 0xAA || sc == 0xB6 { IS_SHIFT = false; }
                Poll::Ready(sc)
            } else {
                let status: u8;
                asm!("in al, dx", out("al") status, in("edx") 0x64u16);
                if (status & 0x01) != 0 {
                    let scancode: u8;
                    asm!("in al, dx", out("al") scancode, in("edx") 0x60u16);
                    if scancode == 0x2A || scancode == 0x36 { IS_SHIFT = true; }
                    if scancode == 0xAA || scancode == 0xB6 { IS_SHIFT = false; }
                    return Poll::Ready(scancode);
                }
                Poll::Pending
            }
        }
    }
}

#[alloc_error_handler]
fn alloc_error_handler(layout: core::alloc::Layout) -> ! { panic!("alloc error: {:?}", layout); }

#[repr(C)]
pub struct MemoryMapConfig { pub ptr: u64, pub len: u64, pub desc_size: u64 }
#[repr(C)]
pub struct FrameBufferConfig { pub frame_buffer: *mut u32, pub pixels_per_scan_line: u32, pub horizontal_resolution: u32, pub vertical_resolution: u32 }

pub struct Console { pub current_y: u32, line_height: u32 }
impl Console {
    fn new() -> Self { Self { current_y: 220, line_height: 20 } }
    pub fn write_line(&mut self, db: &mut draw::DoubleBuffer, text: &str, color: u32) {
        draw::draw_string_db(db, 10, self.current_y, text, color);
        self.current_y += self.line_height;
        if self.current_y > 440 { self.current_y = 220; draw::fill_rect_db(db, 0, 220, 800, 260, 0x000000); }
    }
}

fn wait_ms(ms: u64) { for _ in 0..(ms * 10_000) { unsafe { asm!("pause"); } } }

unsafe fn reset_system() -> ! {
    asm!("cli"); asm!("out 0x64, al", in("al") 0xfe_u8);
    let port: u16 = 0xcf9;
    asm!("out dx, al", in("dx") port, in("al") 0x06_u8); wait_ms(5);
    asm!("out dx, al", in("dx") port, in("al") 0x0e_u8);
    let lidt_ptr: [u16; 3] = [0, 0, 0];
    asm!("lidt [{}]", in(reg) &lidt_ptr); asm!("int 3");
    loop { asm!("hlt") }
}

unsafe fn list_directory(fs: &file::SimpleFileSystem, cluster: u32, abar: u64, active_port: i32, ct_base: u64, data_ptr: u64, db: &mut draw::DoubleBuffer, console: &mut Console) {
    let mut current = cluster;
    let fat_start_lba = fs.get_fat_lba();
    'outer: while current < 0x0FFFFFF8 && current != 0 {
        let lba = fs.get_lba_of_cluster(current);
        for i in 0..(fs.sectors_per_clus as u64) {
            ahci::read_sector(abar, active_port as usize, lba + i, ct_base, data_ptr, fs.cl_base);
            let entries = data_ptr as *const file::DirectoryEntry;
            for j in 0..16 {
                let entry = &*entries.add(j);
                if entry.name[0] == 0x00 { break 'outer; }
                if entry.name[0] == 0xE5 { continue; }
                if entry.attr == 0x0F { continue; }
                let mut name_buf = [0u8; 13]; let mut p = 0;
                for k in 0..8 { if entry.name[k] != b' ' { name_buf[p] = entry.name[k]; p += 1; } }
                if entry.name[8] != b' ' { name_buf[p] = b'.'; p += 1; for k in 8..11 { if entry.name[k] != b' ' { name_buf[p] = entry.name[k]; p += 1; } } }
                if (entry.attr & 0x10) != 0 { name_buf[p] = b'/'; p += 1; }
                console.write_line(db, core::str::from_utf8_unchecked(&name_buf[..p]), 0xFFFFFF);
            }
        }
        let fat_offset = current as u64 * 4;
        ahci::read_sector(abar, active_port as usize, fat_start_lba + fat_offset / 512, ct_base, data_ptr, fs.cl_base);
        current = *((data_ptr + (fat_offset % 512)) as *const u32) & 0x0FFFFFFF;
    }
}

unsafe fn find_directory(fs: &file::SimpleFileSystem, cluster: u32, name: &str, abar: u64, active_port: i32, ct_base: u64, data_ptr: u64) -> Option<u32> {
    let mut current = cluster;
    let fat_start_lba = fs.get_fat_lba();
    while current < 0x0FFFFFF8 && current != 0 {
        let lba = fs.get_lba_of_cluster(current);
        for i in 0..(fs.sectors_per_clus as u64) {
            ahci::read_sector(abar, active_port as usize, lba + i, ct_base, data_ptr, fs.cl_base);
            let entries = data_ptr as *const file::DirectoryEntry;
            for j in 0..16 {
                let entry = &*entries.add(j);
                if entry.name[0] == 0x00 { return None; }
                if entry.name[0] == 0xE5 { continue; }
                if entry.attr == 0x0F { continue; }
                if (entry.attr & 0x10) != 0 && core::str::from_utf8_unchecked(&entry.name).starts_with(name) {
                    return Some((entry.fst_clus_hi as u32) << 16 | entry.fst_clus_lo as u32);
                }
            }
        }
        let fat_offset = current as u64 * 4;
        ahci::read_sector(abar, active_port as usize, fat_start_lba + fat_offset / 512, ct_base, data_ptr, fs.cl_base);
        current = *((data_ptr + (fat_offset % 512)) as *const u32) & 0x0FFFFFFF;
    }
    None
}

#[no_mangle]
#[unsafe(naked)]
pub unsafe extern "sysv64" fn _start() -> ! {
    core::arch::naked_asm!("mov rsp, 0x1FFFFF0","mov rbp, rsp","and rsp, -16","sub rsp, 8","call kmain","hlt")
}

#[no_mangle]
pub unsafe extern "sysv64" fn kmain(fb_ptr: *mut u32, stride: u32, h_res: u32, v_res: u32, mmap_config_ptr: *const MemoryMapConfig) -> ! {
    asm!("cli", "fninit", "mov rax, cr0", "and ax, 0xFFFB", "or ax, 0x22", "mov cr0, rax", "mov rax, cr4", "or ax, 0x600", "mov cr4, rax", out("rax") _);
    
    let fb = FrameBufferConfig { frame_buffer: fb_ptr, pixels_per_scan_line: stride, horizontal_resolution: h_res, vertical_resolution: v_res };
    let mmap = unsafe { &*mmap_config_ptr };
    let mmap_base = mmap.ptr as *const u8;
    let mut total_ram = 0;
    for i in 0..(mmap.len / mmap.desc_size) {
        let desc = unsafe { mmap_base.add(i as usize * mmap.desc_size as usize) };
        if unsafe { *(desc as *const u32) } == 7 { total_ram += unsafe { *(desc.add(24) as *const u64) } * 4096; }
    }

    let ram_mgr = ram::RamManager::new(0x2000000, total_ram);
    for i in 0..(mmap.len / mmap.desc_size) {
        let desc = unsafe { mmap_base.add(i as usize * mmap.desc_size as usize) };
        if unsafe { *(desc as *const u32) } == 7 {
            let start = unsafe { *(desc.add(8) as *const u64) };
            let n = unsafe { *(desc.add(24) as *const u64) };
            for j in 0..n {
                let addr = start + (j * 4096);
                if addr >= 0x4000000 { ram_mgr.mark_free((addr / 4096) as usize); }
            }
        }
    }
    crate::ram::RAM_MGR = Some(ram_mgr);
    let m = crate::ram::RAM_MGR.as_ref().unwrap();
    let ahci_mem_common = m.allocate_sectors(4).expect("AHCI mem fail") as u64;
    let heap = m.allocate_sectors(16384).expect("Heap fail");
    ALLOCATOR.lock().init(heap as *mut u8, 16384 * 4096);

    asm!("out 0x21, al", in("al") 0xFDu8);
    apic::init();
    apic::init_io_apic();
    interrupts::init_idt();

    let pml4_phys = paging::init_os_paging(m, total_ram, fb_ptr as u64); 
    asm!("mov cr3, {0}", in(reg) pml4_phys);

    let mut db = draw::DoubleBuffer::new(&fb);
    let mut console = Console::new();
    draw::draw_string_db(&mut db, 150, 80, "====================================", 0x00FF00);
    draw::draw_string_db(&mut db, 150, 100, "       FORWARD OS BOOT MENU         ", 0xFFFFFF);
    draw::draw_string_db(&mut db, 150, 120, "====================================", 0x00FF00);
    draw::draw_string_db(&mut db, 170, 160, "[F1] Standard Boot (Auto sys64)", 0x888888);
    draw::draw_string_db(&mut db, 170, 190, "[F2] Debug Shell (Manual AHCI/xHCI)", 0x00FFFF);
    draw::draw_string_db(&mut db, 200, 450, "PRESS ANY KEY TO START...", 0xFFFFFF);
    db.flip(&fb);

    let mut boot_mode = 0;
    while boot_mode == 0 {
        let status: u8;
        asm!("in al, dx", out("al") status, in("edx") 0x64u16);
        if (status & 0x01) != 0 {
            let sc: u8;
            asm!("in al, dx", out("al") sc, in("edx") 0x60u16);
            if sc != 0 && (sc & 0x80) == 0 {
                match sc {
                    0x3B => boot_mode = 1,
                    0x3C => boot_mode = 2,
                    _    => boot_mode = 2,
                }
            }
        }
        asm!("pause");
    }

    if boot_mode == 1 {
        draw::clear_screen_db(&mut db, 0x000000);
        draw::draw_string_db(&mut db, 200, 200, "Booting Forward OS...", 0x00FF00);
        db.flip(&fb);
        let cl_base = ahci_mem_common;
        let fb_base = ahci_mem_common + 1024;
        let ct_base = ahci_mem_common + 2048;
        let data_ptr: u64 = 0x10000000;
        let mut active_port: i32 = -1;
        let mut abar: u64 = 0;

        if let Some(dev) = pci::find_ahci_device() { 
            abar = dev.bar5 as u64;
            dev.enable_bus_master();
            ahci::reset_hba(abar);
            wait_ms(100);
            core::ptr::write_volatile((abar as *mut u32).add(1), 0x80000000); 
        }

        if abar != 0 {
            wait_ms(500);
            ahci::reset_hba(abar);
            wait_ms(100);
            
            let pi = ahci::get_implemented_ports(abar);
            crate::boot::debug_print(&alloc::format!("[AHCI] Implemented ports: 0x{:X}", pi));
            
            for i in 0..32 {
                if (pi & (1 << i)) != 0 {
                    crate::boot::debug_print(&alloc::format!("[AHCI] Trying port {}", i));
                    if ahci::init_port(abar, i, cl_base, fb_base, ct_base) {
                        active_port = i as i32;
                        crate::boot::debug_print(&alloc::format!("[AHCI] Port {} selected", i));
                        break;
                    }
                }
            }
        }

        unsafe {
            AHCI_ABAR = abar;
            AHCI_PORT = active_port;
            AHCI_CT_BASE = ct_base;
            AHCI_DATA_PTR = data_ptr;
        }

        boot::set_framebuffer(fb_ptr, stride, h_res, v_res);
        boot::standard_boot();
    }

    draw::clear_screen_db(&mut db, 0x000000);
    console.write_line(&mut db, "Debug Shell Mode Started.", 0x00FFFF);
    db.flip(&fb);

    let cl_base = ahci_mem_common;
    let fb_base = ahci_mem_common + 1024;
    let ct_base = ahci_mem_common + 2048;
    let data_ptr: u64 = 0x10000000;
    let mut active_port: i32 = -1;
    let mut abar: u64 = 0;

    if let Some(dev) = pci::find_ahci_device() { 
        abar = dev.bar5 as u64; 
        dev.enable_bus_master();
        ahci::reset_hba(abar);
        wait_ms(100);
        core::ptr::write_volatile((abar as *mut u32).add(1), 0x80000000); 
    }

    if abar != 0 {
        wait_ms(500);
        ahci::reset_hba(abar);
        wait_ms(100);
        
        let pi = ahci::get_implemented_ports(abar);
        crate::boot::debug_print(&alloc::format!("[AHCI] Implemented ports: 0x{:X}", pi));
        
        for i in 0..32 {
            if (pi & (1 << i)) != 0 {
                crate::boot::debug_print(&alloc::format!("[AHCI] Trying port {}", i));
                if ahci::init_port(abar, i, cl_base, fb_base, ct_base) {
                    active_port = i as i32;
                    crate::boot::debug_print(&alloc::format!("[AHCI] Port {} selected", i));
                    break;
                }
            }
        }
    }

    unsafe {
        AHCI_ABAR = abar;
        AHCI_PORT = active_port;
        AHCI_CT_BASE = ct_base;
        AHCI_DATA_PTR = data_ptr;
    }

    executor::block_on(async {
        let mut input_buf = [0u8; 64];
        let mut input_len = 0;
        let mut debug_lba: u64 = 0;
        
        loop {
            draw::fill_rect_db(&mut db, 0, 0, fb.horizontal_resolution, 130, 0x111111);
            draw::draw_string_db(&mut db, 10, 10, &format!("LBA: {} (0x{:X})", debug_lba, debug_lba), 0xFFFF00);
            
            if active_port != -1 {
                unsafe {
                    ahci::read_sector(abar, active_port as usize, debug_lba, ct_base, data_ptr, cl_base);
                    let ci_ptr = (abar as u64 + 0x100 + (active_port as u64 * 0x80) + 0x38) as *const u32;
                    while (core::ptr::read_volatile(ci_ptr) & 1) != 0 { asm!("pause"); }
                    draw::draw_hex_dump(&mut db, 10, 30, data_ptr as *const u8, 128, 0x00FFFF);
                }
            }

            draw::fill_rect_db(&mut db, 0, console.current_y, fb.horizontal_resolution, 20, 0x000000);
            draw::draw_string_db(&mut db, 10, console.current_y, "forward@os:~$ ", 0x00FF00);
            draw::draw_string_db(&mut db, 130, console.current_y, core::str::from_utf8_unchecked(&input_buf[..input_len]), 0xFFFFFF);
            db.flip(&fb);

            let scancode = KeyboardFuture.await;
            
            if (scancode & 0x80) == 0 { 
                match scancode {
                    0x4B => { if debug_lba > 0 { debug_lba -= 1; } },
                    0x4D => { if debug_lba < 0xFFFFFFFF { debug_lba += 1; } },
                    0x1C => {
                        let cmd_line = core::str::from_utf8_unchecked(&input_buf[..input_len]);
                        console.current_y += 20;
                        let mut args = cmd_line.split_whitespace();
                        let cmd = args.next().unwrap_or("");
                        match cmd {
                            "reboot" => unsafe { reset_system() },
                            "clear" => { draw::clear_screen_db(&mut db, 0x000000); console.current_y = 130; },
                            "cd" => {
                                let target = args.next().unwrap_or("/");
                                if target == "/" || target == ".." { unsafe { CURRENT_DIR_CLUSTER = 0; } }
                                else if let Some(fs) = unsafe { file::SimpleFileSystem::init(abar, active_port as usize, ct_base, data_ptr, cl_base) } {
                                    let start = unsafe { if CURRENT_DIR_CLUSTER == 0 { fs.root_cluster } else { CURRENT_DIR_CLUSTER } };
                                    if let Some(dir) = unsafe { find_directory(&fs, start, target, abar, active_port, ct_base, data_ptr) } {
                                        unsafe { CURRENT_DIR_CLUSTER = dir; }
                                        console.write_line(&mut db, &format!("cd {}", target), 0x00FF00);
                                    } else { console.write_line(&mut db, "Not found", 0xFF0000); }
                                }
                            },
                            "ls" => {
                                let target_dir = args.next().unwrap_or("");
                                if let Some(fs) = unsafe { file::SimpleFileSystem::init(abar, active_port as usize, ct_base, data_ptr, cl_base) } {
                                    let mut cluster = unsafe { if CURRENT_DIR_CLUSTER == 0 { fs.root_cluster } else { CURRENT_DIR_CLUSTER } };
                                    if !target_dir.is_empty() {
                                        if let Some(dir) = unsafe { find_directory(&fs, cluster, target_dir, abar, active_port, ct_base, data_ptr) } { cluster = dir; }
                                    }
                                    unsafe { list_directory(&fs, cluster, abar, active_port, ct_base, data_ptr, &mut db, &mut console); }
                                }
                            },
                            "set" => {
                                let target = args.next().unwrap_or("");
                                match target {
                                    "lba" => {
                                        if let Some(val_str) = args.next() {
                                            let val = if val_str.starts_with("0x") || val_str.starts_with("0X") {
                                                u64::from_str_radix(&val_str[2..], 16).unwrap_or(0)
                                            } else { val_str.parse::<u64>().unwrap_or(0) };
                                            debug_lba = val;
                                            console.write_line(&mut db, &format!("LBA set to {} (0x{:X})", val, val), 0x00FF00);
                                        } else { console.write_line(&mut db, &format!("Current LBA: {} (0x{:X})", debug_lba, debug_lba), 0x00FF00); }
                                    }
                                    _ => { console.write_line(&mut db, "Usage: set lba <number>", 0xFFFF00); }
                                }
                            },
                            _ => { if !cmd.is_empty() { console.write_line(&mut db, &format!("Unknown: {}", cmd), 0xFF0000); } }
                        }
                        input_len = 0;
                    },
                    0x0E => { if input_len > 0 { input_len -= 1; } },
                    _ => {
                        let sc = scancode as usize;
                        if sc < 0x80 {
                            let c = if unsafe { IS_SHIFT } { keyboard::SHIFT_KEYMAP[sc] } else { keyboard::KEYMAP[sc] };
                            if c != '\0' && c != '\x08' && c != '\n' && input_len < 63 { input_buf[input_len] = c as u8; input_len += 1; }
                        }
                    }
                }
            }
        }
    });
    
    loop { unsafe { asm!("hlt"); } }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! { loop { unsafe { asm!("hlt"); } } }