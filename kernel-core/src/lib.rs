#![no_std]

mod arch;
mod console;
mod memory;
mod paging;
mod scheduler;
mod serial;

use arch::{Architecture, X86_64};
use core::panic::PanicInfo;
use serial::{Com1, SerialOutput};

pub use arch::timer_ticks;
pub use console::{BootInfo, BootStatus, Display, Framebuffer, boot};
pub use memory::{
    MemoryRegion, MemoryRegionKind, PAGE_SIZE, PhysicalFrameAllocator, PhysicalMemoryRange,
    PhysicalMemoryStats, allocate_physical_frame, free_physical_frame, initialize_physical_memory,
    physical_memory_stats,
};
pub use paging::{
    DEVICE_WINDOW_BASE, FUTURE_USER_SPACE_BASE, KERNEL_STACK_GUARD_PAGE, PageFlags, PagingConfig,
    PagingError, initialize_virtual_memory, map_user_page,
};
pub use scheduler::{TaskEntry, spawn as spawn_task, start as start_scheduler, yield_now};

pub fn halt() -> ! {
    X86_64::halt()
}

pub fn panic(_: &PanicInfo) -> ! {
    Com1.write(b"open-kernel: panic\r\n");
    halt()
}
