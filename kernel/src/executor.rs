// src/executor.rs
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
use core::sync::atomic::{AtomicBool, Ordering};
use core::arch::asm;

static WOKEN: AtomicBool = AtomicBool::new(false);

pub fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = future;
    let mut future = unsafe { Pin::new_unchecked(&mut future) };
    let raw_waker = RawWaker::new(core::ptr::null(), &WAKER_VTABLE);
    let waker = unsafe { Waker::from_raw(raw_waker) };
    let mut ctx = Context::from_waker(&waker);
    loop {
        match future.as_mut().poll(&mut ctx) {
            Poll::Ready(output) => return output,
            Poll::Pending => {
                WOKEN.store(false, Ordering::SeqCst);
                match future.as_mut().poll(&mut ctx) {
                    Poll::Ready(output) => return output,
                    Poll::Pending => {
                        if !WOKEN.load(Ordering::SeqCst) {
                            unsafe { asm!("pause"); }
                        }
                    }
                }
            }
        }
    }
}

pub fn wake() {
    WOKEN.store(true, Ordering::SeqCst);
}


const WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
    |data| RawWaker::new(data, &WAKER_VTABLE),
    |_| { wake(); },
    |_| { wake(); },
    |_| {},
);