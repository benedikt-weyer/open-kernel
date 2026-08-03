#![no_std]

mod arch;
mod console;
mod memory;
mod serial;

use arch::{Architecture, X86_64};
use core::panic::PanicInfo;
use serial::{Com1, SerialOutput};

pub use console::{BootInfo, BootStatus, Display, Framebuffer, boot};
pub use memory::{
    MemoryRegion, MemoryRegionKind, PAGE_SIZE, PhysicalFrameAllocator, PhysicalMemoryRange,
    PhysicalMemoryStats, allocate_physical_frame, free_physical_frame, initialize_physical_memory,
    physical_memory_stats,
};

pub fn halt() -> ! {
    X86_64::halt()
}

pub fn panic(_: &PanicInfo) -> ! {
    Com1.write(b"open-kernel: panic\r\n");
    halt()
}
