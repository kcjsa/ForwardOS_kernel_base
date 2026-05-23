// src/ram.rs
pub const PAGE_SIZE: u64 = 4096;

pub struct RamManager {
    bitmap: *mut u8,
    total_sectors: usize,
}

pub static mut RAM_MGR: Option<RamManager> = None;


impl RamManager {

    #[inline(never)]
    pub fn new(bitmap_addr: u64, total_ram: u64) -> Self {
        let total_sectors = (total_ram / PAGE_SIZE) as usize;
        let bitmap_ptr = bitmap_addr as *mut u8;
        let bitmap_byte_size = (total_sectors + 7) / 8;

        for i in 0..bitmap_byte_size {
            unsafe {
                bitmap_ptr.add(i).write_volatile(0xFF);
            }
        }

        Self {
            bitmap: bitmap_ptr,
            total_sectors,
        }
    }
    #[inline(never)]
    pub fn mark_used(&self, sector_idx: usize) {
        if sector_idx >= self.total_sectors { return; }
        let byte_idx = (sector_idx / 8) as isize;
        let bit_idx = sector_idx % 8;
        unsafe {
            let p = self.bitmap.offset(byte_idx);
            let val = p.read_volatile();
            p.write_volatile(val | (1 << bit_idx));
        }
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
    }
     #[inline(never)]
    pub fn mark_free(&self, sector_idx: usize) {
        if sector_idx >= self.total_sectors { return; }
        let byte_idx = (sector_idx / 8) as isize;
        let bit_idx = sector_idx % 8;
        unsafe {
            let p = self.bitmap.offset(byte_idx);
            let val = p.read_volatile();
            p.write_volatile(val & !(1 << bit_idx));
        }
    }
    #[inline(never)]
    pub fn is_used(&self, sector_idx: usize) -> bool {
        if sector_idx >= self.total_sectors { return true; }
        let byte_idx = (sector_idx / 8) as isize;
        let bit_idx = sector_idx % 8;
        let result = unsafe { (self.bitmap.offset(byte_idx).read_volatile() & (1 << bit_idx)) != 0 };
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
        result
    }

        #[inline(never)]
    pub fn allocate_sectors(&self, n: usize) -> Option<u64> {
        // ★ デバッグ: この関数が呼ばれたことを確認
        crate::boot::debug_print("ram: allocate_sectors called");
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
        let mut consecutive = 0;
        let mut start_idx = 0;
        for i in 0..self.total_sectors {
            if !self.is_used(i) {
                if consecutive == 0 { start_idx = i; }
                consecutive += 1;
                if consecutive == n {
                    let addr = (start_idx as u64) * PAGE_SIZE;
                    for j in 0..n { self.mark_used(start_idx + j); }
                    return Some(addr);
                }
            } else { consecutive = 0; }
        }
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::SeqCst);
        None
    }
        #[inline(never)]
    pub fn allocate_aligned(&self, n: usize, alignment: u64) -> Option<u64> {
        let mut consecutive = 0;
        let mut start_idx = 0;
        for i in 0..self.total_sectors {
            let addr = (i as u64) * PAGE_SIZE;
            
            if !self.is_used(i) && (addr % alignment == 0) {
                let mut fit = true;
                for j in 0..n {
                    if self.is_used(i + j) {
                        fit = false;
                        break;
                    }
                }
                
                if fit {
                    for j in 0..n { self.mark_used(i + j); }
                    return Some(addr);
                }
            }
        }
        None
    }


}

    pub fn allocate_sectors_direct(n: usize) -> u64 {
    let mgr = unsafe { RAM_MGR.as_ref().unwrap() };
    match mgr.allocate_sectors(n) {
        Some(addr) => addr,
        None => 0xFFFFFFFFFFFFFFFF, // 失敗時は最大値
    }
}