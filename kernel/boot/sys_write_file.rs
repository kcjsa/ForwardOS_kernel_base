// src/boot/sys_write_file.rs

use crate::ahci;
use crate::file::SimpleFileSystem;
use crate::boot::fd;
use core::arch::asm;


fn next_cluster(fs: &SimpleFileSystem, cluster: u32) -> Option<u32> {
    let abar = unsafe { crate::AHCI_ABAR };
    let active_port = unsafe { crate::AHCI_PORT };
    let ct_base = unsafe { crate::AHCI_CT_BASE };
    let data_ptr = unsafe { crate::AHCI_DATA_PTR };

    if active_port == -1 { return None; }

    let fat_lba = fs.get_fat_lba();
    let fat_offset = cluster as u64 * 4;
    let fat_sector = fat_lba + fat_offset / 512;

    unsafe {
        ahci::read_sector(abar, active_port as usize, fat_sector, ct_base, data_ptr);
        let next = *((data_ptr + (fat_offset % 512)) as *const u32) & 0x0FFFFFFF;
        if next < 0x0FFFFFF8 && next != 0 {
            Some(next)
        } else {
            None
        }
    }
}


unsafe fn write_sector(lba: u64) {
    let abar = crate::AHCI_ABAR;
    let port = crate::AHCI_PORT as usize;
    let ct_ptr = crate::AHCI_CT_BASE;
    let data_ptr = crate::AHCI_DATA_PTR;

    if crate::AHCI_PORT == -1 { return; }

    let port_ptr = (abar as u64 + 0x100 + (port as u64 * 0x80)) as *mut u32;
    let ct = ct_ptr as *mut crate::ahci::CommandTable;


    for i in 0..64 { core::ptr::write_volatile(&mut (*ct).cfis[i], 0); }
    core::ptr::write_volatile(&mut (*ct).cfis[0], 0x27); // Register H2D
    core::ptr::write_volatile(&mut (*ct).cfis[1], 0x80); // Command bit
    core::ptr::write_volatile(&mut (*ct).cfis[2], 0x35); // WRITE DMA EXT

    core::ptr::write_volatile(&mut (*ct).cfis[4], lba as u8);
    core::ptr::write_volatile(&mut (*ct).cfis[5], (lba >> 8) as u8);
    core::ptr::write_volatile(&mut (*ct).cfis[6], (lba >> 16) as u8);
    core::ptr::write_volatile(&mut (*ct).cfis[7], 0x40); // LBA mode
    core::ptr::write_volatile(&mut (*ct).cfis[8], (lba >> 24) as u8);
    core::ptr::write_volatile(&mut (*ct).cfis[9], (lba >> 32) as u8);
    core::ptr::write_volatile(&mut (*ct).cfis[10], (lba >> 40) as u8);
    core::ptr::write_volatile(&mut (*ct).cfis[12], 1); // 1 sector


    let prdt = &mut (*ct).prdt_entry[0];
    core::ptr::write_volatile(&mut prdt.dba, data_ptr as u32);
    core::ptr::write_volatile(&mut prdt.dbau, (data_ptr >> 32) as u32);
    core::ptr::write_volatile(&mut prdt.dw3, 511 | (1 << 31));

    core::arch::asm!("mfence", "sfence");


    core::ptr::write_volatile(port_ptr.add(0x38 / 4), 1);


    while (core::ptr::read_volatile(&(*port_ptr)) & 1) != 0 {
        if (core::ptr::read_volatile(port_ptr.add(0x10 / 4)) & 0xFD800000) != 0 { break; }
        core::hint::spin_loop();
    }
    core::arch::asm!("mfence", "lfence");
    for _ in 0..10000 { core::hint::spin_loop(); } // DMA完了のための十分な遅延
}


fn alloc_cluster(fs: &SimpleFileSystem) -> Option<u32> {
    let abar = unsafe { crate::AHCI_ABAR };
    let active_port = unsafe { crate::AHCI_PORT };
    let ct_base = unsafe { crate::AHCI_CT_BASE };
    let data_ptr = unsafe { crate::AHCI_DATA_PTR };

    if active_port == -1 { return None; }

    let fat_lba = fs.get_fat_lba();
    let fat_sectors = fs.fat_sz32 as u64;

    for sector_off in 0..fat_sectors {
        unsafe {
            ahci::read_sector(abar, active_port as usize, fat_lba + sector_off, ct_base, data_ptr);
        }
        let entries = unsafe { core::slice::from_raw_parts(data_ptr as *const u32, 128) };
        for (i, &entry) in entries.iter().enumerate() {
            if (entry & 0x0FFFFFFF) == 0 {
                let cluster = (sector_off * 128 + i as u64) as u32;
                if cluster >= 2 { 
                    return Some(cluster);
                }
            }
        }
    }
    None
}

unsafe fn set_fat_entry(fs: &SimpleFileSystem, cluster: u32, value: u32) {
    let abar = crate::AHCI_ABAR;
    let active_port = crate::AHCI_PORT;
    let ct_base = crate::AHCI_CT_BASE;
    let data_ptr = crate::AHCI_DATA_PTR;

    if active_port == -1 { return; }

    let fat_lba = fs.get_fat_lba();
    let fat_offset = cluster as u64 * 4;
    let fat_sector = fat_lba + fat_offset / 512;

    ahci::read_sector(abar, active_port as usize, fat_sector, ct_base, data_ptr);
    let entry_ptr = (data_ptr + (fat_offset % 512)) as *mut u32;
    let current = core::ptr::read_volatile(entry_ptr);
    let new_val = (current & 0xF0000000) | (value & 0x0FFFFFFF);
    core::ptr::write_volatile(entry_ptr, new_val);
    write_sector(fat_sector);
}

unsafe fn write_cluster(fs: &SimpleFileSystem, cluster: u32, data: &[u8], offset: u64) -> usize {
    let abar = crate::AHCI_ABAR;
    let active_port = crate::AHCI_PORT;
    let ct_base = crate::AHCI_CT_BASE;
    let data_ptr = crate::AHCI_DATA_PTR;

    if active_port == -1 { return 0; }

    let lba = fs.get_lba_of_cluster(cluster);
    crate::boot::debug_print(&alloc::format!("write_clus: clus={} lba={}", cluster, lba));
    let cluster_size = fs.sectors_per_clus as u64 * 512;
    let cluster_offset = (offset % cluster_size) as usize;
    let write_size = data.len().min(cluster_size as usize - cluster_offset);

    for i in 0..fs.sectors_per_clus as u64 {
        let sector_lba = lba + i;
        ahci::read_sector(abar, active_port as usize, sector_lba, ct_base, data_ptr);
        
        let sector_start = (i * 512) as usize;
        let sector_end = sector_start + 512;
        
        if sector_end > cluster_offset && sector_start < cluster_offset + write_size {
            let copy_start = if sector_start > cluster_offset { sector_start } else { cluster_offset };
            let copy_end = if sector_end < cluster_offset + write_size { sector_end } else { cluster_offset + write_size };
            let copy_len = copy_end - copy_start;
            
            let dst = (data_ptr as *mut u8).add(copy_start - sector_start);
            let src = &data[copy_start - cluster_offset..copy_start - cluster_offset + copy_len];
            core::ptr::copy_nonoverlapping(src.as_ptr(), dst, copy_len);
        }
        write_sector(sector_lba);
    }
    
    write_size
}

pub fn sys_write_file(fd: u32, buf_ptr: u64, buf_len: u64) -> isize {
     unsafe { asm!("cli"); }  // ★
   if buf_ptr == 0 || buf_len == 0 {
        crate::boot::debug_print("write_file: null buf");  // ★
         unsafe { asm!("sti"); }  // ★
        return -1;
    }

    let fd_entry = match fd::get_fd(fd) {
        Some(f) => f,
        None => {
            crate::boot::debug_print("write_file: fd not found");  // ★
             unsafe { asm!("sti"); }  // ★
            return -1;
        }
    };

    if fd_entry.is_dir {
        crate::boot::debug_print("write_file: is dir");  // ★
         unsafe { asm!("sti"); }  // ★
        return -1;
    }



let fs = match unsafe {
    SimpleFileSystem::init(
        crate::AHCI_ABAR,
        crate::AHCI_PORT as usize,
        crate::AHCI_CT_BASE,
        crate::AHCI_DATA_PTR,
    )
} {
    Some(fs) => fs,
    None => {
        crate::boot::debug_print("write_file: fs init failed");
         unsafe { asm!("sti"); }  // ★
        return -1;
    }
};

    let cluster_size = fs.sectors_per_clus as u64 * 512;
    let offset = fd_entry.position;
    let data = unsafe { core::slice::from_raw_parts(buf_ptr as *const u8, buf_len as usize) };
    
    let mut cur_cluster = fd_entry.cluster;
    let mut cluster_start = 0u64;
    while cluster_start + cluster_size <= offset {
        match next_cluster(&fs, cur_cluster) {
            Some(next) => {
                cur_cluster = next;
                cluster_start += cluster_size;
            }
            None => {
                if let Some(new_cluster) = alloc_cluster(&fs) {
                unsafe { set_fat_entry(&fs, cur_cluster, new_cluster); }
                unsafe { set_fat_entry(&fs, new_cluster, 0x0FFFFFFF); }
                cur_cluster = new_cluster;
                cluster_start += cluster_size;
                } else {
                     unsafe { asm!("sti"); }  // ★
                    return -1; 
                }
            }
        }
    }

    let mut remaining = data.len();
    let mut written = 0usize;
    let mut write_offset = offset - cluster_start;

    while remaining > 0 {
        let chunk = &data[written..];
        let n = unsafe { write_cluster(&fs, cur_cluster, chunk, write_offset) };
        if n == 0 { break; }
        
        written += n;
        remaining -= n;
        write_offset = 0;
        
        if remaining > 0 {
            match next_cluster(&fs, cur_cluster) {
                Some(next) => cur_cluster = next,
                None => {
                if let Some(new_cluster) = alloc_cluster(&fs) {
                    unsafe { set_fat_entry(&fs, cur_cluster, new_cluster); }
                    unsafe { set_fat_entry(&fs, new_cluster, 0x0FFFFFFF); }
                    cur_cluster = new_cluster;
                } else {
                        break;
                    }
                }
            }
        }
    }



    if let Some(fd_mut) = fd::get_fd_mut(fd) {
        fd_mut.position += written as u64;
        if fd_mut.position > fd_mut.file_size as u64 {
            fd_mut.file_size = fd_mut.position as u32;
            unsafe { update_dir_entry(&fs, fd_mut.cluster, fd_mut.file_size); }
            for _ in 0..500000 { core::hint::spin_loop(); }
        }
    }

    crate::boot::debug_print(&alloc::format!("write_file: written={}", written));
    
    unsafe {
        let abar = crate::AHCI_ABAR;
        let port = crate::AHCI_PORT as usize;
        if port != -1isize as usize {
            crate::ahci::read_sector(abar, port, 2048, crate::AHCI_CT_BASE, crate::AHCI_DATA_PTR);
        }
    }

    crate::boot::debug_print("write_file: done");
    written as isize
}

unsafe fn update_dir_entry(fs: &SimpleFileSystem, target_cluster: u32, new_size: u32) {
    let abar = crate::AHCI_ABAR;
    let active_port = crate::AHCI_PORT;
    let ct_base = crate::AHCI_CT_BASE;
    let data_ptr = crate::AHCI_DATA_PTR;
    
    if active_port == -1 { return; }

    let mut current = fs.root_cluster;
    let fat_start_lba = fs.get_fat_lba();
    
    while current < 0x0FFFFFF8 && current != 0 {
        let lba = fs.get_lba_of_cluster(current);
        
        for i in 0..fs.sectors_per_clus as u64 {
            ahci::read_sector(abar, active_port as usize, lba + i, ct_base, data_ptr);
            
            let entries = data_ptr as *mut crate::file::DirectoryEntry;
            for j in 0..16 {
                let entry = &mut *entries.add(j);
                if entry.name[0] == 0x00 { return; }
                if entry.name[0] == 0xE5 { continue; }
                if entry.attr == 0x0F { continue; }
                
                let entry_cluster = (entry.fst_clus_hi as u32) << 16 | entry.fst_clus_lo as u32;
                if entry_cluster == target_cluster {
                let old_size = core::ptr::addr_of!(entry.file_size).read_unaligned();
                core::ptr::addr_of_mut!(entry.file_size).write_unaligned(new_size);
                let check = core::ptr::addr_of!(entry.file_size).read_unaligned();
                write_sector(lba + i);
                return;
}
            }
        }
        
        let fat_offset = current as u64 * 4;
        let fat_sector = fat_start_lba + fat_offset / 512;
        ahci::read_sector(abar, active_port as usize, fat_sector, ct_base, data_ptr);
        current = *((data_ptr + (fat_offset % 512)) as *const u32) & 0x0FFFFFFF;
    }
}