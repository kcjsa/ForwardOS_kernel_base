// src/boot/exec.rs
use crate::boot::{debug_print, sys_mmap, sys_open, sys_read_file, sys_close, sys_seek};
use alloc::format;
use alloc::string::String;

#[repr(C)]
pub struct Elf64Header {
    pub e_ident: [u8; 16],
    pub e_type: u16,
    pub e_machine: u16,
    pub e_version: u32,
    pub e_entry: u64,
    pub e_phoff: u64,
    pub e_shoff: u64,
    pub e_flags: u32,
    pub e_ehsize: u16,
    pub e_phentsize: u16,
    pub e_phnum: u16,
    pub e_shentsize: u16,
    pub e_shnum: u16,
    pub e_shstrndx: u16,
}

#[repr(C)]
pub struct Elf64Phdr {
    pub p_type: u32,
    pub p_flags: u32,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_paddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}

pub struct LoadedElf {
    pub entry: u64,
    pub base_addr: u64,
    pub size: u64,
    pub path: String,
}

pub fn load_elf(path: &str) -> Option<LoadedElf> {
    debug_print(&format!("[ELF] Loading: {}", path));
    let fd = sys_open(path);
    if fd < 0 { return None; }

    let mut header: Elf64Header = unsafe { core::mem::zeroed() };
    let header_size = core::mem::size_of::<Elf64Header>() as u64;
    sys_seek(fd as u32, 0);
    let header_slice = unsafe { core::slice::from_raw_parts_mut(&mut header as *mut _ as *mut u8, header_size as usize) };
    if sys_read_file(fd as u32, header_slice) as u64 != header_size { sys_close(fd as u32); return None; }
    if &header.e_ident[0..4] != b"\x7FELF" || (header.e_type != 2 && header.e_type != 3) { sys_close(fd as u32); return None; }

    let phdr_count = header.e_phnum as u64;
    let phdr_size = header.e_phentsize as u64;
    let total_phdr_size = phdr_count * phdr_size;
    let mut phdr_mem: u64 = 0;
    sys_mmap((total_phdr_size + 4095) / 4096, 0, &mut phdr_mem as *mut u64 as u64);
    sys_seek(fd as u32, header.e_phoff);
    let phdr_slice = unsafe { core::slice::from_raw_parts_mut(phdr_mem as *mut u8, total_phdr_size as usize) };
    sys_read_file(fd as u32, phdr_slice);

    let mut min_vaddr = u64::MAX;
    let mut max_vaddr = 0u64;
    for i in 0..phdr_count {
        let phdr = unsafe { &*((phdr_mem + i * phdr_size) as *const Elf64Phdr) };
        if phdr.p_type == 1 {
            if phdr.p_vaddr < min_vaddr { min_vaddr = phdr.p_vaddr; }
            let end = phdr.p_vaddr + phdr.p_memsz;
            if end > max_vaddr { max_vaddr = end; }
        }
    }
    if min_vaddr == u64::MAX { sys_close(fd as u32); return None; }

    let total_size = max_vaddr - min_vaddr;
    let total_pages = (total_size + 4095) / 4096;
    let mut base_addr: u64 = 0;
    sys_mmap(total_pages, 0, &mut base_addr as *mut u64 as u64);
    unsafe { core::ptr::write_bytes(base_addr as *mut u8, 0, total_size as usize); }
    let base_offset = base_addr as i64 - min_vaddr as i64;

    for i in 0..phdr_count {
        let phdr = unsafe { &*((phdr_mem + i * phdr_size) as *const Elf64Phdr) };
        if phdr.p_type == 1 {
            let dest = (phdr.p_vaddr as i64 + base_offset) as u64;
            if phdr.p_filesz > 0 {
                sys_seek(fd as u32, phdr.p_offset);
                let data = unsafe { core::slice::from_raw_parts_mut(dest as *mut u8, phdr.p_filesz as usize) };
                sys_read_file(fd as u32, data);
            }
        }
    }
    sys_close(fd as u32);

    let actual_entry = (header.e_entry as i64 + base_offset) as u64;
    debug_print(&format!("[ELF] Loaded: {} entry=0x{:X}", path, actual_entry));
    Some(LoadedElf { entry: actual_entry, base_addr, size: total_size, path: String::from(path) })
}

pub fn load_and_run(path: &str) -> isize {
    if let Some(elf) = load_elf(path) {
        let app_main: extern "C" fn() -> ! = unsafe { core::mem::transmute(elf.entry) };
        app_main();
    }
    -1
}

pub fn load_and_run_container(path: &str) -> isize {
    if let Some(elf) = load_elf(path) {
        debug_print(&format!("[ELF] Container: calling entry 0x{:X}", elf.entry));
        let entry: extern "C" fn() = unsafe { core::mem::transmute(elf.entry) };
        entry();
        debug_print(&format!("[ELF] Container: returned from {}", path));
        
        crate::boot::debug_print("[ELF] After entry return, about to return 0");
        
        0
    } else {
        -1
    }
}
