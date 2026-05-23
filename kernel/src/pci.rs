use core::arch::asm;

#[derive(Debug, Clone, Copy)]
pub struct PciDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub class_code: u8,
    pub subclass: u8,
    pub bar0: u32,
    pub bar1: u32,
    pub bar5: u32,
}

pub unsafe fn config_read_u32(bus: u8, device: u8, func: u8, offset: u8) -> u32 {
    let address = ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((func as u32) << 8)
        | ((offset as u32) & 0xfc)
        | 0x8000_0000;
    asm!("out dx, eax", in("edx") 0x0CF8u16, in("eax") address);
    let mut res: u32;
    asm!("in eax, dx", out("eax") res, in("edx") 0x0CFCu16);
    res
}

pub unsafe fn config_write_u32(bus: u8, device: u8, func: u8, offset: u8, value: u32) {
    let address = ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((func as u32) << 8)
        | ((offset as u32) & 0xfc)
        | 0x8000_0000;
    asm!("out dx, eax", in("edx") 0x0CF8u16, in("eax") address);
    asm!("out dx, eax", in("edx") 0x0CFCu16, in("eax") value);
    // 書き込み完了を保証するための読み戻し
    let _ = config_read_u32(bus, device, func, offset);
}

pub fn find_ahci_device() -> Option<PciDevice> {
    for dev in 0..32 {
        let reg0 = unsafe { config_read_u32(0, dev, 0, 0x00) };
        if (reg0 & 0xffff) == 0xffff { continue; }
        let reg8 = unsafe { config_read_u32(0, dev, 0, 0x08) };
        if (reg8 >> 24) as u8 == 0x01 && (reg8 >> 16) as u8 == 0x06 {
            return Some(PciDevice {
                bus: 0, device: dev, function: 0,
                vendor_id: (reg0 & 0xffff) as u16,
                class_code: 0x01, subclass: 0x06,
                bar0: 0, bar1: 0,
                bar5: unsafe { config_read_u32(0, dev, 0, 0x24) },
            });
        }
    }
    None
}

// pci.rs に追加
pub fn find_e1000() -> Option<PciDevice> {
    for dev in 0..32 {
        let reg0 = unsafe { config_read_u32(0, dev, 0, 0x00) };
        if (reg0 & 0xffff) == 0xffff { continue; }
        
        let vendor = (reg0 & 0xffff) as u16;
        let device_id = (reg0 >> 16) as u16;
        
        // Intel E1000: vendor=0x8086, device=0x100E (82540EM)
        if vendor == 0x8086 && device_id == 0x100E {
            let reg8 = unsafe { config_read_u32(0, dev, 0, 0x08) };
            let class = (reg8 >> 24) as u8;
            let subclass = (reg8 >> 16) as u8;
            
            if class == 0x02 && subclass == 0x00 {
                return Some(PciDevice {
                    bus: 0, device: dev, function: 0,
                    vendor_id: vendor,
                    class_code: class, subclass,
                    bar0: unsafe { config_read_u32(0, dev, 0, 0x10) },
                    bar1: unsafe { config_read_u32(0, dev, 0, 0x14) },
                    bar5: 0,
                });
            }
        }
    }
    None
}

impl PciDevice {
pub unsafe fn enable_bus_master(&self) {
    let cmd_reg = 0x04;
    let mut cmd = config_read_u32(self.bus, self.device, self.function, cmd_reg);
    cmd |= 0x06; // Memory Space (Bit 1) + Bus Master (Bit 2)
    config_write_u32(self.bus, self.device, self.function, cmd_reg, cmd);
        core::arch::asm!("mfence");
    }

    pub fn get_mmio_base(&self) -> usize {
        // BAR0の下位4bitはフラグ（64bitフラグ等）なので、しっかりマスクする
        let low = (self.bar0 & 0xFFFF_FFF0) as u64;
        let high = (self.bar1 as u64) << 32;
        (high | low) as usize
    }
    // pci.rs の PciDevice に追加、あるいは初期化時に呼ぶ
}