use core::arch::asm;

use crate::allocate_physical_frame;

const ENTRY_ADDRESS_MASK: u64 = 0x000F_FFFF_FFFF_F000;
const PRESENT: u64 = 1 << 0;
const WRITABLE: u64 = 1 << 1;
const USER: u64 = 1 << 2;
const HUGE_PAGE: u64 = 1 << 7;
const NO_EXECUTE: u64 = 1 << 63;
pub const PAGE_SIZE: u64 = 4096;
pub const DEVICE_WINDOW_BASE: u64 = 0xFFFF_FF00_0000_0000;
pub const KERNEL_STACK_GUARD_PAGE: u64 = 0xFFFF_FF10_0000_0000;
pub const FUTURE_USER_SPACE_BASE: u64 = 0x0000_4000_0000_0000;

#[derive(Clone, Copy)]
pub struct PagingConfig {
    pub physical_memory_offset: u64,
    pub kernel_virtual_base: u64,
    pub kernel_physical_base: u64,
    pub kernel_size: u64,
}
impl PagingConfig {
    pub const fn new(offset: u64, virtual_base: u64, physical_base: u64, size: u64) -> Self {
        Self {
            physical_memory_offset: offset,
            kernel_virtual_base: virtual_base,
            kernel_physical_base: physical_base,
            kernel_size: size,
        }
    }
}

#[derive(Clone, Copy)]
pub enum PageFlags {
    KernelReadWrite,
    Device,
    UserReadOnly,
    UserReadWrite,
    UserReadExecute,
    UserReadWriteExecute,
}
impl PageFlags {
    const fn bits(self) -> u64 {
        match self {
            Self::KernelReadWrite => PRESENT | WRITABLE | NO_EXECUTE,
            Self::Device => PRESENT | WRITABLE | NO_EXECUTE,
            Self::UserReadOnly => PRESENT | USER | NO_EXECUTE,
            Self::UserReadWrite => PRESENT | WRITABLE | USER | NO_EXECUTE,
            Self::UserReadExecute => PRESENT | USER,
            Self::UserReadWriteExecute => PRESENT | WRITABLE | USER,
        }
    }
}
#[derive(Clone, Copy)]
pub enum PagingError {
    FrameAllocationFailed,
    HugePageConflict,
    AlreadyMapped,
    InvalidUserAddress,
}

#[repr(align(4096))]
struct PageTable([u64; 512]);
static mut PHYSICAL_MEMORY_OFFSET: u64 = 0;

pub fn initialize_virtual_memory(config: PagingConfig) -> Result<(), PagingError> {
    unsafe {
        PHYSICAL_MEMORY_OFFSET = config.physical_memory_offset;
    }
    // Kernel mappings are supplied by GRUB/Limine and deliberately preserved.
    let _kernel_range = (
        config.kernel_virtual_base,
        config.kernel_physical_base,
        config.kernel_size,
    );
    map_page(DEVICE_WINDOW_BASE, 0xB8000, PageFlags::Device)?;
    for offset in 1..=4 {
        map_page(
            KERNEL_STACK_GUARD_PAGE + offset * PAGE_SIZE,
            allocate_physical_frame().ok_or(PagingError::FrameAllocationFailed)?,
            PageFlags::KernelReadWrite,
        )?;
    }
    Ok(())
}

pub fn map_user_page(virtual_address: u64, physical_address: u64) -> Result<(), PagingError> {
    if virtual_address < FUTURE_USER_SPACE_BASE || virtual_address >= 0x0000_8000_0000_0000 {
        return Err(PagingError::InvalidUserAddress);
    }
    map_page(virtual_address, physical_address, PageFlags::UserReadWrite)
}

pub fn map_user_code_page(
    virtual_address: u64,
    physical_address: u64,
) -> Result<(), PagingError> {
    if virtual_address < FUTURE_USER_SPACE_BASE || virtual_address >= 0x0000_8000_0000_0000 {
        return Err(PagingError::InvalidUserAddress);
    }
    map_page(virtual_address, physical_address, PageFlags::UserReadExecute)
}

pub fn map_user_page_with_flags(
    virtual_address: u64,
    physical_address: u64,
    flags: PageFlags,
) -> Result<(), PagingError> {
    if virtual_address < FUTURE_USER_SPACE_BASE || virtual_address >= 0x0000_8000_0000_0000 {
        return Err(PagingError::InvalidUserAddress);
    }
    map_page(virtual_address, physical_address, flags)
}

pub fn write_physical_frame(physical_address: u64, source: &[u8]) {
    let destination = (physical_address + unsafe { PHYSICAL_MEMORY_OFFSET }) as *mut u8;
    for (offset, byte) in source.iter().enumerate() {
        unsafe {
            core::ptr::write_volatile(destination.add(offset), *byte);
        }
    }
}

pub fn zero_physical_frame(physical_address: u64) {
    let destination = (physical_address + unsafe { PHYSICAL_MEMORY_OFFSET }) as *mut u8;
    for offset in 0..PAGE_SIZE as usize {
        unsafe {
            core::ptr::write_volatile(destination.add(offset), 0);
        }
    }
}

fn map_page(
    virtual_address: u64,
    physical_address: u64,
    flags: PageFlags,
) -> Result<(), PagingError> {
    let indices = [
        (virtual_address >> 39) & 0x1FF,
        (virtual_address >> 30) & 0x1FF,
        (virtual_address >> 21) & 0x1FF,
        (virtual_address >> 12) & 0x1FF,
    ];
    let mut table = unsafe { table_mut(read_cr3() & ENTRY_ADDRESS_MASK) };
    for index in indices[..3].iter().copied() {
        let entry = &mut table.0[index as usize];
        if *entry & PRESENT == 0 {
            let frame = allocate_physical_frame().ok_or(PagingError::FrameAllocationFailed)?;
            *entry = frame | PRESENT | WRITABLE | if flags.bits() & USER != 0 { USER } else { 0 };
            zero_table(frame);
        } else if *entry & HUGE_PAGE != 0 {
            return Err(PagingError::HugePageConflict);
        }
        table = unsafe { table_mut(*entry & ENTRY_ADDRESS_MASK) };
    }
    let leaf = &mut table.0[indices[3] as usize];
    if *leaf & PRESENT != 0 {
        return Err(PagingError::AlreadyMapped);
    }
    *leaf = (physical_address & ENTRY_ADDRESS_MASK) | flags.bits();
    unsafe {
        asm!("invlpg [{}]", in(reg) virtual_address, options(nostack));
    }
    Ok(())
}

unsafe fn table_mut(physical_address: u64) -> &'static mut PageTable {
    unsafe { &mut *((physical_address + PHYSICAL_MEMORY_OFFSET) as *mut PageTable) }
}
fn zero_table(physical_address: u64) {
    let table = unsafe { table_mut(physical_address) };
    for entry in &mut table.0 {
        unsafe {
            core::ptr::write_volatile(entry, 0);
        }
    }
}
fn read_cr3() -> u64 {
    let value: u64;
    unsafe {
        asm!("mov {}, cr3", out(reg) value, options(nomem, nostack));
    }
    value
}
