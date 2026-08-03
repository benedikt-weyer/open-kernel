#![no_std]

mod arch;
mod console;
mod drivers;
mod elf;
mod keyboard;
mod memory;
mod mouse;
mod paging;
mod scheduler;
mod serial;
mod storage;
mod user;
mod xhci;

use arch::{Architecture, X86_64};
use core::panic::PanicInfo;
use serial::{Com1, SerialOutput};

pub use arch::timer_ticks;
pub use arch::shutdown;
pub use console::{BootInfo, BootStatus, Display, Framebuffer, boot};
pub use drivers::{Driver, DriverError, LoopbackNetworkDriver, NetworkDriver};
pub use elf::{ElfError, LoadedImage, load_user_elf};
pub use keyboard::Ps2KeyboardDriver;
pub use memory::{
    MemoryRegion, MemoryRegionKind, PAGE_SIZE, PhysicalFrameAllocator, PhysicalMemoryRange,
    PhysicalMemoryStats, allocate_physical_frame, free_physical_frame, initialize_physical_memory,
    physical_memory_stats,
};
pub use mouse::{initialize as initialize_mouse, poll as poll_mouse, position as mouse_position};
pub use paging::{
    DEVICE_WINDOW_BASE, FUTURE_USER_SPACE_BASE, KERNEL_STACK_GUARD_PAGE, PageFlags, PagingConfig,
    PagingError, initialize_virtual_memory, map_user_code_page, map_user_page,
    map_user_page_with_flags, write_physical_frame, zero_physical_frame,
};
pub use scheduler::{TaskEntry, spawn as spawn_task, start as start_scheduler, yield_now};
pub use storage::{File, FileSystem, InitRamFs, file_count, open as open_file, register_boot_file};
pub use xhci::XhciController;

pub fn halt() -> ! {
    X86_64::halt()
}

pub fn panic(_: &PanicInfo) -> ! {
    Com1.write(b"open-kernel: panic\r\n");
    halt()
}
