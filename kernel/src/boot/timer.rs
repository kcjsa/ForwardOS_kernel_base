// src/boot/timer.rs

use core::arch::asm;
use crate::boot::debug_print;
use alloc::format;

pub struct TscTimer {
    frequency: u64,
}

impl TscTimer {
    pub fn new() -> Self {
        let frequency = Self::measure_frequency();
        debug_print(&format!("[TSC] Frequency: {} MHz", frequency / 1_000_000));
        Self { frequency }
    }
    
    fn measure_frequency() -> u64 {
        unsafe {
            let target_ms = 100;
            let pit_freq = 1193182;
            
            asm!("out dx, al", in("dx") 0x43u16, in("al") 0xB0u8);
            asm!("out dx, al", in("dx") 0x43u16, in("al") 0xB0u8);
            let low: u8;
            asm!("in al, dx", out("al") low, in("dx") 0x42u16);
            let high: u8;
            asm!("in al, dx", out("al") high, in("dx") 0x42u16);
            let pit_start = ((high as u16) << 8) | low as u16;
            
            let tsc_start = Self::read_tsc();
            
            let target_count = pit_start.saturating_sub((pit_freq * target_ms / 1000) as u16);
            loop {
                asm!("out dx, al", in("dx") 0x43u16, in("al") 0xB0u8);
                let low: u8;
                asm!("in al, dx", out("al") low, in("dx") 0x42u16);
                let high: u8;
                asm!("in al, dx", out("al") high, in("dx") 0x42u16);
                let current = ((high as u16) << 8) | low as u16;
                if current <= target_count {
                    break;
                }
                asm!("pause");
            }
            
            let tsc_end = Self::read_tsc();
            let tsc_diff = tsc_end - tsc_start;
            (tsc_diff * 1000) / target_ms
        }
    }
    
    #[inline]
    unsafe fn read_tsc() -> u64 {
        let low: u32;
        let high: u32;
        asm!("rdtsc", out("eax") low, out("edx") high);
        ((high as u64) << 32) | low as u64
    }
    
    #[inline]
    pub unsafe fn now(&self) -> u64 {
        Self::read_tsc()
    }
    
    pub fn frequency(&self) -> u64 {
        self.frequency
    }
    
    pub unsafe fn wait_ms(&self, ms: u64) {
        let ticks = (self.frequency * ms) / 1000;
        let start = self.now();
        while self.now() - start < ticks {
            asm!("pause");
        }
    }
}

static mut GLOBAL_TIMER: Option<TscTimer> = None;

pub unsafe fn init_timer() {
    GLOBAL_TIMER = Some(TscTimer::new());
}

pub unsafe fn get_timer() -> &'static TscTimer {
    GLOBAL_TIMER.as_ref().unwrap()
}

pub unsafe fn sleep_ms(ms: u64) {
    get_timer().wait_ms(ms);
}
