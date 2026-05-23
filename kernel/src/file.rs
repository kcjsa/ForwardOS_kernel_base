// src/file.rs
use crate::ahci;

#[repr(C, packed)]
pub struct Fat32Bpb {
    pub jmp: [u8; 3],
    pub oem: [u8; 8],
    pub bytes_per_sec: u16,
    pub sectors_per_clus: u8,
    pub reserved_secs: u16,
    pub num_fats: u8,
    pub root_ent_cnt: u16,
    pub tot_sec16: u16,
    pub media: u8,
    pub fat_sz16: u16,
    pub sec_per_trk: u16,
    pub num_heads: u16,
    pub hidd_sec: u32,
    pub tot_sec32: u32,
    pub fat_sz32: u32,
    pub ext_flags: u16,
    pub fs_ver: u16,
    pub root_cluster: u32,
}

#[repr(C, packed)]
pub struct DirectoryEntry {
    pub name: [u8; 11],
    pub attr: u8,
    pub nt_res: u8,
    pub crt_time_te: u8,
    pub crt_time: u16,
    pub crt_date: u16,
    pub lst_acc_date: u16,
    pub fst_clus_hi: u16,
    pub lst_wrt_time: u16,
    pub lst_wrt_date: u16,
    pub fst_clus_lo: u16,
    pub file_size: u32,
}

pub struct SimpleFileSystem {
    pub abar: u64,
    pub port: usize,
    pub cl_base: u64,
    pub cluster_begin_lba: u64,
    pub sectors_per_clus: u8,
    pub root_cluster: u32,
    pub reserved_secs: u16,
    pub fat_sz32: u32,
    pub num_fats: u8,
    pub part_lba: u64,
}

impl SimpleFileSystem {
    pub unsafe fn init(abar: u64, port: usize, ct_ptr: u64, data_ptr: u64, cl_ptr: u64) -> Option<Self> {
        ahci::read_sector(abar, port, 0, ct_ptr, data_ptr, cl_ptr);
        let mbr = data_ptr as *const u8;
        
        let part_offset = 446 + 8;
        let part_lba = 
            (*mbr.add(part_offset) as u64) |
            ((*mbr.add(part_offset + 1) as u64) << 8) |
            ((*mbr.add(part_offset + 2) as u64) << 16) |
            ((*mbr.add(part_offset + 3) as u64) << 24);
        
        ahci::read_sector(abar, port, part_lba, ct_ptr, data_ptr, cl_ptr);
        
        let vbr = &*(data_ptr as *const Fat32Bpb);
        if vbr.bytes_per_sec != 512 { return None; }

        let cluster_begin = part_lba + vbr.reserved_secs as u64 + (vbr.num_fats as u64 * vbr.fat_sz32 as u64);

        Some(Self {
            abar,
            port,
            cl_base: cl_ptr,
            cluster_begin_lba: cluster_begin,
            sectors_per_clus: vbr.sectors_per_clus,
            root_cluster: vbr.root_cluster,
            reserved_secs: vbr.reserved_secs,
            fat_sz32: vbr.fat_sz32,
            num_fats: vbr.num_fats,
            part_lba,
        })
    }

    pub fn get_lba_of_cluster(&self, cluster: u32) -> u64 {
        self.cluster_begin_lba + (cluster as u64 - 2) * self.sectors_per_clus as u64
    }

    pub fn get_fat_lba(&self) -> u64 {
        self.part_lba + self.reserved_secs as u64
    }
}

pub unsafe fn find_directory(fs: &SimpleFileSystem, cluster: u32, name: &str, abar: u64, active_port: i32, ct_base: u64, data_ptr: u64) -> Option<u32> {
    let mut current = cluster;
    let fat_start_lba = fs.get_fat_lba();
    
    while current < 0x0FFFFFF8 && current != 0 {
        let lba = fs.get_lba_of_cluster(current);
        
        for i in 0..(fs.sectors_per_clus as u64) {
            ahci::read_sector(abar, active_port as usize, lba + i, ct_base, data_ptr, fs.cl_base);
            
            let entries = data_ptr as *const DirectoryEntry;
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

pub unsafe fn find_entry(fs: &SimpleFileSystem, cluster: u32, name: &str, abar: u64, active_port: i32, ct_base: u64, data_ptr: u64) -> Option<(u32, u32)> {
    let mut current = cluster;
    let fat_start_lba = fs.get_fat_lba();
    
    while current < 0x0FFFFFF8 && current != 0 {
        let lba = fs.get_lba_of_cluster(current);
        
        for i in 0..(fs.sectors_per_clus as u64) {
            ahci::read_sector(abar, active_port as usize, lba + i, ct_base, data_ptr, fs.cl_base);
            
            let entries = data_ptr as *const DirectoryEntry;
            for j in 0..16 {
                let entry = &*entries.add(j);
                if entry.name[0] == 0x00 { return None; }
                if entry.name[0] == 0xE5 { continue; }
                if entry.attr == 0x0F { continue; }
                
                let mut name_buf = [0u8; 13];
                let mut p = 0;
                for k in 0..8 { if entry.name[k] != b' ' { name_buf[p] = entry.name[k]; p += 1; } }
                if entry.name[8] != b' ' { 
                    name_buf[p] = b'.'; p += 1; 
                    for k in 8..11 { if entry.name[k] != b' ' { name_buf[p] = entry.name[k]; p += 1; } } 
                }
                let display_name = core::str::from_utf8_unchecked(&name_buf[..p]);
                
                if display_name.eq_ignore_ascii_case(name) {
                    let cluster = (entry.fst_clus_hi as u32) << 16 | entry.fst_clus_lo as u32;
                    let size = entry.file_size;
                    return Some((cluster, size));
                }
            }
        }
        
        let fat_offset = current as u64 * 4;
        let fat_sector = fat_start_lba + fat_offset / 512;
        ahci::read_sector(abar, active_port as usize, fat_sector, ct_base, data_ptr, fs.cl_base);
        current = *((data_ptr + (fat_offset % 512)) as *const u32) & 0x0FFFFFFF;
    }
    None
}