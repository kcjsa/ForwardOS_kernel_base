// src/apic.rs
use core::arch::asm;
use core::task::Waker;

static mut APIC_BASE: u64 = 0;
static mut KEYBOARD_WAKER: Option<Waker> = None;

pub unsafe fn init() {
    let low: u32; let high: u32;
    asm!("rdmsr", in("ecx") 0x1Bu32, out("eax") low, out("edx") high);
    APIC_BASE = ((high as u64) << 32 | (low as u64)) & 0xFFFF_F000;
    let sivr = APIC_BASE + 0xF0;
    let val = core::ptr::read_volatile(sivr as *const u32);
    core::ptr::write_volatile(sivr as *mut u32, val | 0x1FF);
    crate::interrupts::set_eoi_callback(apic_eoi);
    let lvt_timer = APIC_BASE + 0x320;
    core::ptr::write_volatile(lvt_timer as *mut u32, 0x10000);
}

unsafe fn apic_eoi() {
    core::ptr::write_volatile((APIC_BASE + 0xB0) as *mut u32, 0);
}

pub fn get_base() -> u64 { unsafe { APIC_BASE } }

pub unsafe fn init_io_apic() {
    let io_apic_base = 0xFEC00000;
    write_io_apic(io_apic_base, 0x12, 0x21 | (1 << 16));
    write_io_apic(io_apic_base, 0x13, 0);
}

unsafe fn write_io_apic(base: u64, reg: u8, value: u32) {
    core::ptr::write_volatile(base as *mut u32, reg as u32);
    core::ptr::write_volatile((base + 0x10) as *mut u32, value);
}

pub fn register_keyboard_waker(waker: Waker) { unsafe { KEYBOARD_WAKER = Some(waker); } }
pub fn wake_keyboard() { unsafe { if let Some(ref waker) = KEYBOARD_WAKER { waker.wake_by_ref(); } } }