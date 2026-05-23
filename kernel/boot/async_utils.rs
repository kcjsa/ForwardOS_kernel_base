// src/boot/async_utils.rs

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};
use core::arch::asm;

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
// async_utils.rs
impl Future for KeyboardFuture {
    type Output = u8;
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<u8> {
        cx.waker().wake_by_ref(); // ★ 常に再ポーリングを要求
        
        // 割り込みバッファをチェック
        unsafe {
            if let Some(sc) = crate::interrupts::get_key_from_buffer() {
                return Poll::Ready(sc);
            }
        }
        
        // 直接ポーリングもする
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

/// 指定ティック数待つFuture
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

/// 何もしないFuture（即座にReady）
pub struct NopFuture;
impl Future for NopFuture {
    type Output = ();
    fn poll(self: Pin<&mut Self>, _cx: &mut Context) -> Poll<()> {
        Poll::Ready(())
    }
}