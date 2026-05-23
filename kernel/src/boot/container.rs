// src/boot/container.rs
use alloc::boxed::Box;
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};
use alloc::vec::Vec;

#[derive(PartialEq, Debug, Clone, Copy)]
pub enum ContainerState {
    Running,
    Stopped,
}

pub struct Container {
    pub id: u32,
    pub state: ContainerState,
    pub memory_region: (u64, u64),
    pub name: &'static str,
}

static mut CONTAINERS: Option<Vec<Container>> = None;
static mut NEXT_CONTAINER_ID: u32 = 1;

pub fn init() {
    unsafe {
        CONTAINERS = Some(Vec::new());
        NEXT_CONTAINER_ID = 1;
    }
}

pub fn create_container(mem_start: u64, mem_pages: u64, name: &'static str) -> u32 {
    unsafe {
        let id = NEXT_CONTAINER_ID;
        NEXT_CONTAINER_ID += 1;
        
        let container = Container {
            id,
            state: ContainerState::Running,
            memory_region: (mem_start, mem_pages * 4096),
            name,
        };
        
        if let Some(ref mut containers) = CONTAINERS {
            containers.push(container);
        }
        
        id
    }
}

pub fn stop_container(id: u32) {
    unsafe {
        if let Some(ref mut containers) = CONTAINERS {
            if let Some(container) = containers.iter_mut().find(|c| c.id == id) {
                container.state = ContainerState::Stopped;
            }
        }
    }
}

pub fn get_container_state(id: u32) -> Option<ContainerState> {
    unsafe {
        CONTAINERS.as_ref()?.iter()
            .find(|c| c.id == id)
            .map(|c| c.state)
    }
}

pub fn get_container_mem(id: u32) -> Option<u64> {
    unsafe {
        CONTAINERS.as_ref()?.iter()
            .find(|c| c.id == id)
            .map(|c| c.memory_region.0)
    }
}
// container.rs に追加
pub fn is_in_container_memory(addr: u64) -> bool {
    unsafe {
        if let Some(ref containers) = CONTAINERS {
            for container in containers {
                let start = container.memory_region.0;
                let end = start + container.memory_region.1;
                if addr >= start && addr < end {
                    return true;
                }
            }
        }
    }
    false
}

pub fn find_container_by_addr(addr: u64) -> Option<u32> {
    unsafe {
        if let Some(ref containers) = CONTAINERS {
            for container in containers {
                let start = container.memory_region.0;
                let end = start + container.memory_region.1;
                if addr >= start && addr < end {
                    return Some(container.id);
                }
            }
        }
    }
    None
}

// コンテナ内で実行するFuture
pub struct ContainerFuture<F: Future<Output = ()>> {
    future: Pin<Box<F>>,
    container_id: u32,
}

impl<F: Future<Output = ()>> Future for ContainerFuture<F> {
    type Output = ();
    
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        // コンテナが停止していないかチェック
        if get_container_state(self.container_id) == Some(ContainerState::Stopped) {
            return Poll::Ready(());
        }
        self.future.as_mut().poll(cx)
    }
}

pub fn run_in_container<F: Future<Output = ()> + 'static>(container_id: u32, future: F) -> ContainerFuture<F> {
    ContainerFuture {
        future: Box::pin(future),
        container_id,
    }
}