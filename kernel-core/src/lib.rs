#![no_std]

mod arch;
mod console;
mod drivers;
mod elf;
mod keyboard;
mod memory;
mod mouse;
mod pci;
mod paging;
#[path = "threads.rs"]
mod scheduler;
mod sata;
mod serial;
mod storage;
mod user;
mod vfs;
mod xhci;

use arch::{Architecture, X86_64};
use core::panic::PanicInfo;
use serial::{Com1, SerialOutput};

pub use arch::{monotonic_milliseconds, timer_ticks};
pub use arch::shutdown;
pub use console::{BootInfo, BootStatus, Display, Framebuffer, boot};
pub use drivers::{
    BlockDevice, BlockDeviceError, Driver, DriverError, LoopbackNetworkDriver, NetworkDriver,
};
pub use elf::{ElfError, LoadedImage, load_user_elf};
pub use keyboard::Ps2KeyboardDriver;
pub use memory::{
    MemoryRegion, MemoryRegionKind, PAGE_SIZE, PhysicalFrameAllocator, PhysicalMemoryRange,
    PhysicalMemoryStats, allocate_physical_frame, free_physical_frame, initialize_physical_memory,
    physical_memory_stats,
};
pub use mouse::{initialize as initialize_mouse, poll as poll_mouse, position as mouse_position};
pub use pci::{AhciController, PciDevice, device_count as pci_device_count, enumerate as enumerate_pci, find_ahci_controller};
pub use paging::{
    DEVICE_WINDOW_BASE, FUTURE_USER_SPACE_BASE, KERNEL_STACK_GUARD_PAGE, PageFlags, PagingConfig,
    PagingError, allocate_kernel_stack, allocate_user_stack, initialize_virtual_memory, map_user_code_page, map_user_page,
    is_user_executable, map_device_page, map_user_page_with_flags, physical_to_virtual, write_physical_frame,
    zero_physical_frame,
};
pub use scheduler::{
    Process, ProcessId, TaskEntry, ThreadId, ThreadState, UserContext, block_current as block_current_thread,
    exit_current as exit_current_thread, spawn as spawn_task, start as start_scheduler,
    current_id as current_thread_id, process_id as thread_process_id, state as thread_state,
    wake as wake_thread, yield_now,
};
pub use sata::{
    SataBlockDevice, SataError, identify as sata_identify,
    identify_model_byte as sata_identify_model_byte, initialize as initialize_sata,
    is_available as sata_available, read_first_sector as sata_read_first_sector,
    read_sector as sata_read_sector, sector_count as sata_sector_count,
};
pub use storage::{
    File, FileSystem, InitRamFs, RamFs, RamFsError, create as create_ram_file,
    delete as delete_ram_file, file_count, open as open_file, ram_file_count,
    read as read_ram_file, register_boot_file, write as write_ram_file,
};
pub use xhci::XhciController;
pub use user::{ExecutableMetadata, executable_metadata};
pub use vfs::{
    VfsError, directory_entry as vfs_directory_entry, is_directory as vfs_is_directory, mount_count as vfs_mount_count,
    mount_initramfs, mounted as vfs_mounted, open as vfs_open, open_file as vfs_open_file,
    write as vfs_write, write_at as vfs_write_at,
};

pub fn halt() -> ! {
    X86_64::halt()
}

pub fn panic(_: &PanicInfo) -> ! {
    Com1.write(b"open-kernel: panic\r\n");
    halt()
}
