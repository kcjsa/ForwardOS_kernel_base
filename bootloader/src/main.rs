#![no_std]
#![no_main]

use uefi::prelude::*;
use uefi::proto::media::file::{File, FileAttribute, FileMode, FileType, FileInfo};
use uefi::proto::media::fs::SimpleFileSystem;
use uefi::proto::console::gop::GraphicsOutput;
use uefi::table::boot::{MemoryType, AllocateType, MemoryDescriptor};
use uefi::{cstr16, entry, Handle, Status};
use xmas_elf::program::Type;
use xmas_elf::ElfFile;
use core::arch::asm;

extern crate alloc;
use alloc::vec;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FrameBufferConfig {
    pub frame_buffer: *mut u32,
    pub pixels_per_scan_line: u32,
    pub horizontal_resolution: u32,
    pub vertical_resolution: u32,
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        unsafe { asm!("hlt"); }
    }
}

#[entry]
fn main(_handle: Handle, mut system_table: SystemTable<Boot>) -> Status {
    uefi::helpers::init(&mut system_table).unwrap();
    
    // --- 1. カーネルファイルの読み込み ---
    let kernel_data;
    let entry_point_addr;
    {
        let bt = system_table.boot_services();
        let mut fs = bt.open_protocol_exclusive::<SimpleFileSystem>(
            bt.get_handle_for_protocol::<SimpleFileSystem>().unwrap()
        ).unwrap();
        let mut root = fs.open_volume().unwrap();
        let handle = root.open(cstr16!("kernel.elf"), FileMode::Read, FileAttribute::empty()).unwrap();
        let mut file = match handle.into_type().unwrap() {
            FileType::Regular(f) => f,
            _ => panic!("Kernel not found"),
        };
        
        let mut info_buf = [0u8; 128];
        let info = file.get_info::<FileInfo>(&mut info_buf).unwrap();
        let mut data = vec![0u8; info.file_size() as usize];
        file.read(&mut data).unwrap();
        
        let elf = ElfFile::new(&data).expect("Failed to parse ELF");
        entry_point_addr = elf.header.pt2.entry_point();
        kernel_data = data;
    }

    // --- 2. GOP取得 ---
    let config = {
        let bt = system_table.boot_services();
        let mut gop = bt.open_protocol_exclusive::<GraphicsOutput>(
            bt.get_handle_for_protocol::<GraphicsOutput>().unwrap()
        ).unwrap();
        let mode = gop.current_mode_info();
        FrameBufferConfig {
            frame_buffer: gop.frame_buffer().as_mut_ptr() as *mut u32,
            pixels_per_scan_line: mode.stride() as u32,
            horizontal_resolution: mode.resolution().0 as u32,
            vertical_resolution: mode.resolution().1 as u32,
        }
    };

    // --- 3. ELF展開 (修正ポイント) ---
    {
        let elf = ElfFile::new(&kernel_data).unwrap();
        for ph in elf.program_iter() {
            if let Ok(Type::Load) = ph.get_type() {
                let start_addr = ph.virtual_addr();
                let mem_size = ph.mem_size();
                let file_size = ph.file_size();
                
                // ページ境界でアライメント調整して確保
                let aligned_start = start_addr & !0xfff;
                let aligned_end = (start_addr + mem_size + 0xfff) & !0xfff;
                let pages = (aligned_end - aligned_start) / 0x1000;
                
                let _ = system_table.boot_services().allocate_pages(
                    AllocateType::Address(aligned_start),
                    MemoryType::LOADER_DATA,
                    pages as usize,
                );

                unsafe {
                    // 実際にデータをコピーする先は virtual_addr そのもの
                    let dest = start_addr as *mut u8;
                    let src = kernel_data.as_ptr().add(ph.offset() as usize);
                    core::ptr::copy_nonoverlapping(src, dest, file_size as usize);
                    
                    // .bss 領域などのゼロクリア
                    if mem_size > file_size {
                        core::ptr::write_bytes(
                            dest.add(file_size as usize),
                            0,
                            (mem_size - file_size) as usize,
                        );
                    }
                }
            }
        }
    }

    // --- 4. スタック確保 ---
    let stack_size: usize = 0x100000;
    let stack_base = system_table.boot_services().allocate_pages(
        AllocateType::AnyPages,
        MemoryType::LOADER_DATA,
        stack_size / 0x1000
    ).expect("Failed to allocate stack");
    let stack_top = stack_base + stack_size as u64;

    // --- 5. CPU初期化 ---
    unsafe {
        let (mut cr0, mut cr4): (u64, u64);
        asm!("mov {0}, cr0", out(reg) cr0);
        cr0 &= !(1 << 2); cr0 |= 1 << 1;
        asm!("mov cr0, {0}", in(reg) cr0);
        asm!("mov {0}, cr4", out(reg) cr4);
        cr4 |= 1 << 9; cr4 |= 1 << 10;
        asm!("mov cr4, {0}", in(reg) cr4);
        asm!("finit");
    }

    // --- 6. 情報を退避 ---
    let entry_point = entry_point_addr;
    let fb_ptr = config.frame_buffer;
    let fb_stride = config.pixels_per_scan_line;
    let fb_h = config.horizontal_resolution;
    let fb_v = config.vertical_resolution;

// --- 7. UEFI終了 ---
    let (_rt, mmap_res) = system_table.exit_boot_services(MemoryType::LOADER_DATA);

    // --- 8. メモリ情報を構造体にまとめる ---
    #[repr(C)]
    struct MemoryMapConfig {
        ptr: u64,
        len: u64,
        desc_size: u64,
    }

    let mmap_ptr = mmap_res.entries().next()
        .map(|d| d as *const uefi::table::boot::MemoryDescriptor as u64)
        .unwrap_or(0);

    // デスクリプタサイズを、イテレータの差分から動的に取得（これが最重要）
    let real_desc_size = {
        let mut it = mmap_res.entries();
        let e1 = it.next().map(|d| d as *const _ as usize).unwrap_or(0);
        let e2 = it.next().map(|d| d as *const _ as usize).unwrap_or(0);
        if e2 > e1 { (e2 - e1) as u64 } else { core::mem::size_of::<uefi::table::boot::MemoryDescriptor>() as u64 }
    };

    let mmap_config = MemoryMapConfig {
        ptr: mmap_ptr,
        len: (mmap_res.entries().count() as u64) * real_desc_size,
        desc_size: real_desc_size,
    };

    // --- 9. カーネルへジャンプ ---
    unsafe {
        asm!(
            "cli",
            "mov rsp, {stack}",
            "and rsp, -16",
            "push 0", // アライメント調整
            "jmp {entry}",
            stack = in(reg) stack_top,
            entry = in(reg) entry_point,
            in("rdi") fb_ptr,    // 第1引数
            in("rsi") fb_stride, // 第2引数
            in("rdx") fb_h,      // 第3引数
            in("rcx") fb_v,      // 第4引数
            in("r8") &mmap_config, // 第5引数：構造体へのポインタを渡す！
            in("rax") &mmap_res, // 寿命維持用
            options(noreturn)
        );
    }
}