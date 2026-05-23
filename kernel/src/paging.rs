// src/paging.rs
use crate::ram::RamManager;

pub const PAGE_PRESENT: u64  = 1 << 0;
pub const PAGE_WRITABLE: u64 = 1 << 1;
pub const PAGE_USER: u64     = 1 << 2;
pub const PAGE_HUGE: u64     = 1 << 7;

#[repr(C, align(4096))]
pub struct PageTable {
    pub entries: [u64; 512],
}

impl PageTable {
    pub fn clear(&mut self) {
        self.entries.fill(0);
    }
}

pub unsafe fn init_os_paging(mgr: &RamManager, _total_ram: u64, _fb_ptr: u64) -> u64 {
    let pml4_phys = mgr.allocate_sectors(1).expect("PML4 fail");
    let pdpt_phys = mgr.allocate_sectors(1).expect("PDPT fail");
    
    let pml4 = pml4_phys as *mut PageTable;
    let pdpt = pdpt_phys as *mut PageTable;
    (*pml4).clear();
    (*pdpt).clear();

    let mask = 0x000F_FFFF_FFFF_F000;

    // PML4[0] -> PDPT
    (*pml4).entries[0] = (pdpt_phys & mask) | PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER;

    // 64GB分を巨大ページ(2MB)でマップ
    for j in 0..64 {
        let pd_phys = mgr.allocate_sectors(1).expect("PD fail");
        let pd = pd_phys as *mut PageTable;
        (*pd).clear();

        (*pdpt).entries[j] = (pd_phys & mask) | PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER;

        for i in 0..512 {
            let addr = (j as u64 * 1024 * 1024 * 1024) + (i as u64 * 2 * 1024 * 1024);
            let mut flags = PAGE_PRESENT | PAGE_WRITABLE | PAGE_HUGE | PAGE_USER;

            // すべての物理メモリを非キャッシュ設定にする
            // Bit 3: PWT (Write-through)
            // Bit 4: PCD (Cache Disable)
            flags |= (1 << 3) | (1 << 4); 

            (*pd).entries[i] = (addr & mask) | flags;
        }
    }

    core::arch::asm!("mfence");
    pml4_phys & mask
}