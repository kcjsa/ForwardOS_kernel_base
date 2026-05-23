// src/boot/container.rs
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
    pub entry: u64,
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

pub fn create_container(entry: u64, name: &'static str, pages: u64) -> u32 {
    unsafe {
        let id = NEXT_CONTAINER_ID;
        NEXT_CONTAINER_ID += 1;
        let container = Container {
            id,
            state: ContainerState::Running,
            memory_region: (0, pages * 4096),
            entry,
            name,
        };
        if let Some(ref mut containers) = CONTAINERS {
            containers.push(container);
        }
        id
    }
}