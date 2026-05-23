// src/boot/fd.rs
pub struct FileDescriptor {
    pub cluster: u32,
    pub position: u64,
    pub is_dir: bool,
    pub file_size: u32,
}

impl FileDescriptor {
    pub fn seek(&mut self, pos: u64) {
        self.position = pos;
    }
}

#[repr(C)]
pub struct FileStat {
    pub file_size: u32,
    pub cluster: u32,
    pub position: u64,
    pub is_dir: bool,
}

static mut FD_TABLE: [Option<FileDescriptor>; 256] = [const { None }; 256];

pub fn allocate_fd(cluster: u32, is_dir: bool, file_size: u32) -> i32 {
    unsafe {
        for i in 3..256 {
            if FD_TABLE[i].is_none() {
                FD_TABLE[i] = Some(FileDescriptor {
                    cluster,
                    position: 0,
                    is_dir,
                    file_size,
                });
                return i as i32;
            }
        }
        -1
    }
}

pub fn get_fd(fd: u32) -> Option<&'static FileDescriptor> {
    unsafe {
        if (fd as usize) < 256 {
            FD_TABLE[fd as usize].as_ref()
        } else {
            None
        }
    }
}

pub fn get_fd_mut(fd: u32) -> Option<&'static mut FileDescriptor> {
    unsafe {
        if (fd as usize) < 256 {
            FD_TABLE[fd as usize].as_mut()
        } else {
            None
        }
    }
}

pub fn close_fd(fd: u32) -> bool {
    unsafe {
        if fd >= 3 && (fd as usize) < 256 {
            FD_TABLE[fd as usize] = None;
            true
        } else {
            false
        }
    }
}

fn next_cluster(fs: &crate::file::SimpleFileSystem, cluster: u32) -> Option<u32> {
    let abar = unsafe { crate::AHCI_ABAR };
    let active_port = unsafe { crate::AHCI_PORT };
    let ct_base = unsafe { crate::AHCI_CT_BASE };
    let data_ptr = unsafe { crate::AHCI_DATA_PTR };

    if active_port == -1 { return None; }

    let fat_lba = fs.get_fat_lba();
    let fat_offset = cluster as u64 * 4;
    let fat_sector = fat_lba + fat_offset / 512;

    unsafe {
        crate::ahci::read_sector(abar, active_port as usize, fat_sector, ct_base, data_ptr, fs.cl_base);
        let next = *((data_ptr + (fat_offset % 512)) as *const u32) & 0x0FFFFFFF;
        if next < 0x0FFFFFF8 && next != 0 {
            Some(next)
        } else {
            None
        }
    }
}

fn read_cluster(fs: &crate::file::SimpleFileSystem, cluster: u32, buf: &mut [u8], offset: u64) -> usize {
    let abar = unsafe { crate::AHCI_ABAR };
    let active_port = unsafe { crate::AHCI_PORT };
    let ct_base = unsafe { crate::AHCI_CT_BASE };
    let data_ptr = unsafe { crate::AHCI_DATA_PTR };

    if active_port == -1 { return 0; }

    let lba = fs.get_lba_of_cluster(cluster);
    let cluster_size = fs.sectors_per_clus as u64 * 512;
    let cluster_offset = (offset % cluster_size) as usize;
    let read_size = buf.len().min(cluster_size as usize - cluster_offset);

    let mut bytes_read = 0;

    for i in 0..fs.sectors_per_clus as u64 {
        unsafe {
            crate::ahci::read_sector(abar as u64, active_port as usize, lba + i, ct_base, data_ptr, fs.cl_base);
        }
        let sector_start = (i * 512) as usize;
        let sector_end = sector_start + 512;
        
        if sector_end > cluster_offset && sector_start < cluster_offset + read_size {
            let copy_start = if sector_start > cluster_offset { sector_start } else { cluster_offset };
            let copy_end = if sector_end < cluster_offset + read_size { sector_end } else { cluster_offset + read_size };
            let copy_len = copy_end - copy_start;
            
            let src = unsafe { (data_ptr as *const u8).add(copy_start - sector_start) };
            let dst_start = copy_start - cluster_offset;
            let dst = &mut buf[dst_start..dst_start + copy_len];
            dst.copy_from_slice(unsafe { core::slice::from_raw_parts(src, copy_len) });
            bytes_read += copy_len;
        }
    }
    
    bytes_read
}

pub fn sys_read_file(fd: u32, buf_ptr: u64, buf_len: u64) -> isize {
    if buf_ptr == 0 || buf_len == 0 {
        crate::boot::debug_print("read_file: null buf_ptr or buf_len");
        return -1;
    }

    let fd_entry = match get_fd(fd) {
        Some(f) => f,
        None => {
            crate::boot::debug_print(&alloc::format!("read_file: fd {} not found", fd));
            return -1;
        }
    };

    if fd_entry.is_dir {
        crate::boot::debug_print("read_file: is directory");
        return -1;
    }
    
    let mut total_read: u64 = 0;
    let mut offset = fd_entry.position;
    let mut cluster = fd_entry.cluster;
    
    let max_read = buf_len.min((fd_entry.file_size as u64).saturating_sub(offset));

    if max_read == 0 {
        return 0;
    }

    let fs = unsafe {
        match crate::file::SimpleFileSystem::init(
            crate::AHCI_ABAR,
            crate::AHCI_PORT as usize,
            crate::AHCI_CT_BASE,
            crate::AHCI_DATA_PTR,
            crate::AHCI_CT_BASE,
        ) {
            Some(fs) => fs,
            None => return -1,
        }
    };

    let cluster_size = fs.sectors_per_clus as u64 * 512;
    
    let mut current_cluster = cluster;
    let mut cluster_start_offset = 0u64;
    while cluster_start_offset + cluster_size <= offset {
        match next_cluster(&fs, current_cluster) {
            Some(next) => {
                current_cluster = next;
                cluster_start_offset += cluster_size;
            }
            None => break,
        }
    }

    let mut buf_slice = unsafe { core::slice::from_raw_parts_mut(buf_ptr as *mut u8, buf_len as usize) };
    let mut remaining = max_read as usize;
    let mut buf_pos = 0;
    let mut cur_cluster = current_cluster;
    let mut read_offset = offset - cluster_start_offset;

    while remaining > 0 {
        let chunk_size = remaining.min(cluster_size as usize - read_offset as usize);
        let chunk_buf = &mut buf_slice[buf_pos..buf_pos + chunk_size];
        
        let n = read_cluster(&fs, cur_cluster, chunk_buf, read_offset);
        if n == 0 { break; }
        
        buf_pos += n;
        remaining -= n;
        total_read += n as u64;
        read_offset = 0;
        
        if remaining > 0 {
            match next_cluster(&fs, cur_cluster) {
                Some(next) => cur_cluster = next,
                None => break,
            }
        }
    }

    if let Some(fd_entry_mut) = get_fd_mut(fd) {
        fd_entry_mut.position += total_read;
    }

    total_read as isize
}

pub fn syscall_open(path: &str) -> i32 {
    let abar = unsafe { crate::AHCI_ABAR };
    let active_port = unsafe { crate::AHCI_PORT };
    let ct_base = unsafe { crate::AHCI_CT_BASE };
    let data_ptr = unsafe { crate::AHCI_DATA_PTR };

    if active_port == -1 { return -1; }

    let fs = unsafe {
        match crate::file::SimpleFileSystem::init(abar, active_port as usize, ct_base, data_ptr, crate::AHCI_CT_BASE) {
            Some(fs) => fs,
            None => return -1,
        }
    };

    let cluster = unsafe {
        crate::file::find_entry(
            &fs, fs.root_cluster, path,
            abar, active_port as i32, ct_base, data_ptr,
        )
    };

    match cluster {
        Some((c, size)) => {
            allocate_fd(c, false, size)
        },
        None => -1,
    }
}

pub fn sys_lseek(fd: u32, offset: i64, whence: u32) -> i64 {
    let fd_entry = match get_fd_mut(fd) {
        Some(f) => f,
        None => {
            crate::boot::debug_print("lseek: fd not found!");
            return -1;
        }
    };

    let new_pos = match whence {
        0 => offset,
        1 => fd_entry.position as i64 + offset,
        2 => fd_entry.file_size as i64 + offset,
        _ => return -1,
    };

    if new_pos < 0 {
        return -1;
    }

    fd_entry.position = new_pos as u64;
    fd_entry.position as i64
}

pub fn sys_fstat(fd: u32, buf_ptr: u64) -> bool {
    let fd_entry = match get_fd(fd) {
        Some(f) => f,
        None => {
            return false;
        }
    };
    
    let stat = FileStat {
        file_size: fd_entry.file_size,
        cluster: fd_entry.cluster,
        position: fd_entry.position,
        is_dir: fd_entry.is_dir,
    };
    
    unsafe {
        let dst = buf_ptr as *mut FileStat;
        core::ptr::write_volatile(dst, stat);
    }
    
    true
}

pub fn sys_seek(fd: u32, pos: u64) -> i64 {
    match get_fd_mut(fd) {
        Some(fd_entry) => {
            fd_entry.seek(pos);
            fd_entry.position as i64
        }
        None => -1,
    }
}