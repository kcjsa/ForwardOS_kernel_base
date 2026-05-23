// src/boot/async_utils.rs

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};
use core::arch::asm;
use crate::boot::timer;

/// CPUを即座に手放すFuture
pub struct YieldFuture;
impl Future for YieldFuture {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<()> {
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

/// キーボード入力を待つFuture（割り込みバッファから）
pub struct KeyboardFuture;
impl Future for KeyboardFuture {
    type Output = u8;
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<u8> {
        cx.waker().wake_by_ref();
        
        unsafe {
            if let Some(sc) = crate::interrupts::get_key_from_buffer() {
                return Poll::Ready(sc);
            }
        }
        
        let status: u8;
        unsafe { asm!("in al, dx", out("al") status, in("edx") 0x64u16); }
        if (status & 0x01) != 0 {
            let sc: u8;
            unsafe { asm!("in al, dx", out("al") sc, in("edx") 0x60u16); }
            return Poll::Ready(sc);
        }
        
        Poll::Pending
    }
}

/// 指定ティック数待つFuture（ビジーループなし）
pub struct DelayFuture {
    remaining: u64,
}
impl DelayFuture {
    pub fn new(ticks: u64) -> Self {
        Self { remaining: ticks }
    }
}
impl Future for DelayFuture {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<()> {
        if self.remaining == 0 {
            Poll::Ready(())
        } else {
            self.remaining -= 1;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

/// TSCタイマーを使った正確なsleep（async版）
pub struct SleepFuture {
    target_tsc: u64,
}

impl SleepFuture {
    pub fn new(ms: u64) -> Self {
        unsafe {
            let freq = timer::get_timer().frequency();
            let now = timer::get_timer().now();
            let ticks = (freq * ms) / 1000;
            Self { target_tsc: now + ticks }
        }
    }
    
    pub fn new_us(us: u64) -> Self {
        unsafe {
            let freq = timer::get_timer().frequency();
            let now = timer::get_timer().now();
            let ticks = (freq * us) / 1_000_000;
            Self { target_tsc: now + ticks }
        }
    }
}

impl Future for SleepFuture {
    type Output = ();
    
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        unsafe {
            if timer::get_timer().now() >= self.target_tsc {
                Poll::Ready(())
            } else {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }
}

/// ミリ秒単位で待機するFuture（便利関数）
pub fn sleep_ms(ms: u64) -> SleepFuture {
    SleepFuture::new(ms)
}

/// マイクロ秒単位で待機するFuture（便利関数）
pub fn sleep_us(us: u64) -> SleepFuture {
    SleepFuture::new_us(us)
}

/// 何もしないFuture（即座にReady）
pub struct NopFuture;
impl Future for NopFuture {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _cx: &mut Context) -> Poll<()> {
        Poll::Ready(())
    }
}