// src/ahci.rs
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};

#[repr(C)]
pub struct HbaPort {
    pub clb: u32, pub clbu: u32, pub fb: u32, pub fbu: u32,
    pub is: u32, pub ie: u32, pub cmd: u32, pub rsv0: u32,
    pub tfd: u32, pub sig: u32, pub ssts: u32, pub sctl: u32,
    pub serr: u32, pub sact: u32, pub ci: u32, pub sntf: u32,
    pub fbs: u32, pub rsv1: [u32; 11], pub vendor: [u32; 4],
}

#[repr(C)]
pub struct HbaMem {
    pub cap: u32, pub ghc: u32, pub is: u32, pub pi: u32, pub vs: u32,
    pub ccc_ctl: u32, pub ccc_pts: u32, pub em_loc: u32, pub em_ctl: u32,
    pub cap2: u32, pub bohc: u32, pub rsv: [u8; 116], pub vendor: [u8; 96],
    pub ports: [HbaPort; 32],
}

#[repr(C, align(1024))]
pub struct CommandList { pub headers: [CommandHeader; 32], }

#[repr(C)]
pub struct CommandHeader {
    pub flags: u16, pub prdtl: u16, pub prdbc: u32,
    pub ctba: u32, pub ctbau: u32, pub rsv: [u32; 4],
}

// ahci.rs - CommandTableにパディング追加
#[repr(C, align(128))]
pub struct CommandTable {
    pub cfis: [u8; 64],
    pub acmd: [u8; 16],
    pub rsv: [u8; 48],
    pub prdt_entry: [PrdtEntry; 1],
    pub _padding: [u8; 112],  // 256バイト保証
}

#[repr(C)]
pub struct PrdtEntry { pub dba: u32, pub dbau: u32, pub rsv0: u32, pub dw3: u32, }

const PxCMD_ST: u32   = 0x0001;
const PxCMD_FRE: u32  = 0x0010;
const PxCMD_FR: u32   = 0x4000;
const PxCMD_CR: u32   = 0x8000;
const PxCMD_ATAPI: u32 = 0x0020;
const PxIS_TFES: u32  = 1 << 30;

unsafe fn wait_cycles(cycles: u32) {
    for _ in 0..cycles {
        core::hint::spin_loop();
    }
}

unsafe fn wait_ms(ms: u32) {
    for _ in 0..(ms * 10000) {
        core::hint::spin_loop();
    }
}

pub unsafe fn get_implemented_ports(abar: u64) -> u32 {
    read_volatile(&(*(abar as *const HbaMem)).pi)
}

pub unsafe fn get_port_status(abar: u64, port_idx: usize) -> (u32, u32) {
    let port = &(*(abar as *const HbaMem)).ports[port_idx];
    (read_volatile(&port.ssts), read_volatile(&port.sig))
}

pub unsafe fn reset_hba(abar: u64) -> bool {
    let hba = &mut *(abar as *mut HbaMem);
    let ghc = &mut hba.ghc;
    
    crate::boot::debug_print(&alloc::format!("[AHCI] HBA at 0x{:X}, CAP=0x{:08X}", abar, hba.cap));
    
    let mut val = read_volatile(ghc);
    if (val & 0x80000000) == 0 {
        crate::boot::debug_print("[AHCI] Setting AE bit");
        write_volatile(ghc, val | 0x80000000);
        wait_ms(10);
        val = read_volatile(ghc);
    }
    
    if (val & 0x00000001) == 0 {
        crate::boot::debug_print("[AHCI] Setting HR bit");
        write_volatile(ghc, val | 0x00000001);
        
        let mut timeout = 1_000_000;
        while (read_volatile(ghc) & 0x00000001) != 0 && timeout > 0 {
            wait_cycles(10);
            timeout -= 1;
        }
        
        if timeout == 0 {
            crate::boot::debug_print("[AHCI] ERROR: HBA reset timeout");
            return false;
        }
        
        wait_ms(10);
        val = read_volatile(ghc);
        if (val & 0x80000000) == 0 {
            write_volatile(ghc, val | 0x80000000);
        }
    }
    
    crate::boot::debug_print("[AHCI] HBA reset completed");
    true
}

pub unsafe fn init_port(abar: u64, port_idx: usize, cl_phys: u64, fb_phys: u64, ct_phys: u64) -> bool {
    let hba = &mut *(abar as *mut HbaMem);
    let port = &mut hba.ports[port_idx];
    
    crate::boot::debug_print(&alloc::format!("[AHCI] Initializing port {}", port_idx));
    
    let mut cmd = read_volatile(&port.cmd);
    if (cmd & PxCMD_ST) != 0 {
        cmd &= !PxCMD_ST;
        write_volatile(&mut port.cmd, cmd);
        wait_cycles(1000);
    }
    
    let mut sctl = read_volatile(&port.sctl);
    sctl = (sctl & !0x0F) | 0x01;
    write_volatile(&mut port.sctl, sctl);
    wait_ms(10);
    
    sctl = (sctl & !0x0F) | 0x00;
    write_volatile(&mut port.sctl, sctl);
    
    let mut timeout = 5_000_000;
    while (read_volatile(&port.ssts) & 0x0F) != 0x03 && timeout > 0 {
        wait_cycles(10);
        timeout -= 1;
    }
    
    if timeout == 0 {
        crate::boot::debug_print(&alloc::format!("[AHCI] Port {}: No device after COMRESET", port_idx));
        return false;
    }
    
    let ssts = read_volatile(&port.ssts);
    let sig = read_volatile(&port.sig);
    crate::boot::debug_print(&alloc::format!("[AHCI] Port {}: SSTS=0x{:X}, SIG=0x{:08X}", port_idx, ssts, sig));
    
    if sig != 0x00000101 {
        if sig == 0xEB140101 {
            crate::boot::debug_print("[AHCI] ATAPI device not supported");
            return false;
        }
    }
    
    core::ptr::write_bytes(cl_phys as *mut u8, 0, 1024);
    core::ptr::write_bytes(fb_phys as *mut u8, 0, 256);
    core::ptr::write_bytes(ct_phys as *mut u8, 0, 256);
    
    write_volatile(&mut port.is, 0xFFFFFFFF);
    write_volatile(&mut port.serr, 0xFFFFFFFF);
    write_volatile(&mut port.ie, 0);
    
    cmd = read_volatile(&port.cmd);
    cmd &= !(PxCMD_ST | PxCMD_FRE);
    cmd &= !PxCMD_ATAPI;
    write_volatile(&mut port.cmd, cmd);
    
    timeout = 1_000_000;
    while (read_volatile(&port.cmd) & PxCMD_CR) != 0 && timeout > 0 {
        wait_cycles(10);
        timeout -= 1;
    }
    if timeout == 0 {
        crate::boot::debug_print(&alloc::format!("[AHCI] Port {}: CR clear timeout", port_idx));
        return false;
    }
    
    timeout = 1_000_000;
    while (read_volatile(&port.cmd) & PxCMD_FR) != 0 && timeout > 0 {
        wait_cycles(10);
        timeout -= 1;
    }
    if timeout == 0 {
        crate::boot::debug_print(&alloc::format!("[AHCI] Port {}: FR clear timeout", port_idx));
        return false;
    }
    
    write_volatile(&mut port.clb, cl_phys as u32);
    write_volatile(&mut port.clbu, (cl_phys >> 32) as u32);
    write_volatile(&mut port.fb, fb_phys as u32);
    write_volatile(&mut port.fbu, (fb_phys >> 32) as u32);
    
    let cmd_list = &mut *(cl_phys as *mut CommandList);
    let header = &mut cmd_list.headers[0];
    write_volatile(&mut header.ctba, ct_phys as u32);
    write_volatile(&mut header.ctbau, (ct_phys >> 32) as u32);
    write_volatile(&mut header.prdtl, 1);
    write_volatile(&mut header.prdbc, 0);
    write_volatile(&mut header.flags, 0x05);
    
    cmd = read_volatile(&port.cmd) | PxCMD_FRE;
    write_volatile(&mut port.cmd, cmd);
    
    timeout = 1_000_000;
    while (read_volatile(&port.cmd) & PxCMD_FR) == 0 && timeout > 0 {
        wait_cycles(10);
        timeout -= 1;
    }
    if timeout == 0 {
        crate::boot::debug_print(&alloc::format!("[AHCI] Port {}: FR set timeout", port_idx));
        return false;
    }
    
    timeout = 10_000_000;
    while (read_volatile(&port.tfd) & 0x88) != 0 && timeout > 0 {
        wait_cycles(10);
        timeout -= 1;
    }
    if timeout == 0 {
        let tfd = read_volatile(&port.tfd);
        crate::boot::debug_print(&alloc::format!("[AHCI] Port {}: BSY/DRQ timeout TFD=0x{:X}", port_idx, tfd));
    }
    
    cmd = read_volatile(&port.cmd) | PxCMD_ST;
    write_volatile(&mut port.cmd, cmd);
    
    timeout = 1_000_000;
    while (read_volatile(&port.cmd) & PxCMD_CR) == 0 && timeout > 0 {
        wait_cycles(10);
        timeout -= 1;
    }
    if timeout == 0 {
        crate::boot::debug_print(&alloc::format!("[AHCI] Port {}: CR set timeout", port_idx));
        return false;
    }
    
    crate::boot::debug_print(&alloc::format!("[AHCI] Port {}: Initialization successful", port_idx));
    true
}

pub unsafe fn read_sector(abar: u64, port_idx: usize, lba: u64, ct_phys: u64, dest_phys: u64, cl_phys: u64) -> bool {
    let hba = &mut *(abar as *mut HbaMem);
    let port = &mut hba.ports[port_idx];
    let ct = ct_phys as *mut CommandTable;
    
    crate::boot::debug_print(&alloc::format!("[AHCI] Reading LBA {}", lba));
    
    let mut timeout = 1_000_000;
    while (read_volatile(&port.tfd) & 0x88) != 0 && timeout > 0 {
        wait_cycles(10);
        timeout -= 1;
    }
    if timeout == 0 {
        let tfd = read_volatile(&port.tfd);
        crate::boot::debug_print(&alloc::format!("[AHCI] read_sector: BSY/DRQ timeout TFD=0x{:X}", tfd));
        return false;
    }
    
    write_volatile(&mut port.is, 0xFFFFFFFF);
    write_volatile(&mut port.serr, 0xFFFFFFFF);
    
    core::ptr::write_bytes(ct as *mut u8, 0, 256);
    
    write_volatile(&mut (*ct).cfis[0], 0x27);
    write_volatile(&mut (*ct).cfis[1], 0x80);
    write_volatile(&mut (*ct).cfis[2], 0x25);
    write_volatile(&mut (*ct).cfis[3], 0x00);
    write_volatile(&mut (*ct).cfis[4], (lba & 0xFF) as u8);
    write_volatile(&mut (*ct).cfis[5], ((lba >> 8) & 0xFF) as u8);
    write_volatile(&mut (*ct).cfis[6], ((lba >> 16) & 0xFF) as u8);
    write_volatile(&mut (*ct).cfis[7], 0x40);
    write_volatile(&mut (*ct).cfis[8], ((lba >> 24) & 0xFF) as u8);
    write_volatile(&mut (*ct).cfis[9], ((lba >> 32) & 0xFF) as u8);
    write_volatile(&mut (*ct).cfis[10], ((lba >> 40) & 0xFF) as u8);
    write_volatile(&mut (*ct).cfis[11], 0x00);
    write_volatile(&mut (*ct).cfis[12], 0x01);
    write_volatile(&mut (*ct).cfis[13], 0x00);
    
    let prdt = &mut (*ct).prdt_entry[0];
    write_volatile(&mut prdt.dba, dest_phys as u32);
    write_volatile(&mut prdt.dbau, (dest_phys >> 32) as u32);
    write_volatile(&mut prdt.dw3, 511 | (1 << 31));
    
    fence(Ordering::SeqCst);
    write_volatile(&mut port.ci, 1);
    
    timeout = 10_000_000;
    let mut tfes_detected = false;
    
    while (read_volatile(&port.ci) & 1) != 0 && timeout > 0 {
        let is = read_volatile(&port.is);
        if (is & PxIS_TFES) != 0 {
            tfes_detected = true;
            write_volatile(&mut port.is, is);
            crate::boot::debug_print(&alloc::format!("[AHCI] read_sector: TFES error IS=0x{:X}", is));
            break;
        }
        wait_cycles(50);
        timeout -= 1;
    }
    
    fence(Ordering::SeqCst);
    
    if timeout == 0 {
        crate::boot::debug_print("[AHCI] read_sector: CI timeout");
        return false;
    }
    
    if tfes_detected {
        let tfd = read_volatile(&port.tfd);
        crate::boot::debug_print(&alloc::format!("[AHCI] read_sector: TFES, TFD=0x{:X}", tfd));
        return false;
    }
    
    let cmd_list = &*(cl_phys as *const CommandList);
    let prdbc = read_volatile(&cmd_list.headers[0].prdbc);
    if prdbc != 512 {
        crate::boot::debug_print(&alloc::format!("[AHCI] read_sector: prdbc={} (expected 512)", prdbc));
    }
    
    crate::boot::debug_print(&alloc::format!("[AHCI] Read completed: LBA {}", lba));
    true
}