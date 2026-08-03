use core::arch::asm;

use crate::{allocate_physical_frame, free_physical_frame};

const ENTRY_ADDRESS_MASK: u64 = 0x000F_FFFF_FFFF_F000;
const PRESENT: u64 = 1 << 0;
const WRITABLE: u64 = 1 << 1;
const USER: u64 = 1 << 2;
const HUGE_PAGE: u64 = 1 << 7;
const NO_EXECUTE: u64 = 1 << 63;
pub const PAGE_SIZE: u64 = 4096;
pub const DEVICE_WINDOW_BASE: u64 = 0xFFFF_FF00_0000_0000;
pub const KERNEL_STACK_GUARD_PAGE: u64 = 0xFFFF_FF10_0000_0000;
const KERNEL_STACK_PAGES: u64 = 4;
const USER_STACK_PAGES: u64 = 4;
const USER_SPACE_END: u64 = 0x0000_8000_0000_0000;
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

/// Creates an empty user address space while retaining every kernel mapping
/// from the currently active page table.
pub fn create_user_address_space() -> Result<u64, PagingError> {
    let frame = allocate_physical_frame().ok_or(PagingError::FrameAllocationFailed)?;
    zero_table(frame);
    let source = unsafe { table_mut(read_cr3() & ENTRY_ADDRESS_MASK) };
    let destination = unsafe { table_mut(frame) };
    // Canonical lower-half entries are user mappings; the upper half belongs
    // to the kernel and is shared by all processes.
    for index in 256..512 {
        destination.0[index] = source.0[index];
    }
    Ok(frame)
}

pub fn active_address_space() -> u64 {
    read_cr3() & ENTRY_ADDRESS_MASK
}

pub unsafe fn switch_address_space(address_space: u64) {
    unsafe {
        asm!("mov cr3, {}", in(reg) address_space, options(nostack));
    }
}

/// Releases all lower-half user mappings and page-table frames owned by an
/// address space. Kernel-half entries are shared and deliberately untouched.
pub fn destroy_user_address_space(address_space: u64) {
    unsafe {
        let root = table_mut(address_space & ENTRY_ADDRESS_MASK);
        for index in 0..256 {
            let entry = root.0[index];
            if entry & PRESENT != 0 {
                release_user_table(entry & ENTRY_ADDRESS_MASK, 3);
                root.0[index] = 0;
            }
        }
    }
    free_physical_frame(address_space & ENTRY_ADDRESS_MASK);
}

pub fn map_user_page_in(
    address_space: u64,
    virtual_address: u64,
    physical_address: u64,
    flags: PageFlags,
) -> Result<(), PagingError> {
    if virtual_address < FUTURE_USER_SPACE_BASE || virtual_address >= USER_SPACE_END {
        return Err(PagingError::InvalidUserAddress);
    }
    map_page_in(address_space, virtual_address, physical_address, flags)
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

pub fn physical_to_virtual(physical_address: u64) -> *mut u8 {
    (physical_address + unsafe { PHYSICAL_MEMORY_OFFSET }) as *mut u8
}

pub fn map_device_page(virtual_address: u64, physical_address: u64) -> Result<(), PagingError> {
    if virtual_address % PAGE_SIZE != 0 || physical_address % PAGE_SIZE != 0 {
        return Err(PagingError::InvalidUserAddress);
    }
    map_page(virtual_address, physical_address, PageFlags::Device)
}

pub fn allocate_kernel_stack(slot: usize) -> Result<u64, PagingError> {
    let stack_base = KERNEL_STACK_GUARD_PAGE
        + (slot as u64) * (KERNEL_STACK_PAGES + 1) * PAGE_SIZE
        + PAGE_SIZE;
    for page in 0..KERNEL_STACK_PAGES {
        map_page(
            stack_base + page * PAGE_SIZE,
            allocate_physical_frame().ok_or(PagingError::FrameAllocationFailed)?,
            PageFlags::KernelReadWrite,
        )?;
    }
    Ok(stack_base + KERNEL_STACK_PAGES * PAGE_SIZE)
}

pub fn release_kernel_stack(slot: usize) {
    let stack_base = KERNEL_STACK_GUARD_PAGE
        + (slot as u64) * (KERNEL_STACK_PAGES + 1) * PAGE_SIZE
        + PAGE_SIZE;
    for page in 0..KERNEL_STACK_PAGES {
        if let Some(frame) = unmap_page(stack_base + page * PAGE_SIZE) {
            free_physical_frame(frame);
        }
    }
}

pub fn allocate_user_stack(slot: usize) -> Result<u64, PagingError> {
    allocate_user_stack_in(active_address_space(), slot)
}

pub fn allocate_user_stack_in(address_space: u64, slot: usize) -> Result<u64, PagingError> {
    // Leave one unmapped page below each downward-growing stack as a guard.
    let stack_top = USER_SPACE_END - PAGE_SIZE - (slot as u64) * (USER_STACK_PAGES + 1) * PAGE_SIZE;
    for page in 1..=USER_STACK_PAGES {
        let address = stack_top - page * PAGE_SIZE;
        let frame = allocate_physical_frame().ok_or(PagingError::FrameAllocationFailed)?;
        zero_physical_frame(frame);
        map_user_page_in(address_space, address, frame, PageFlags::UserReadWrite)?;
    }
    Ok(stack_top)
}

pub fn release_user_stack(slot: usize) {
    let stack_top = USER_SPACE_END - PAGE_SIZE - (slot as u64) * (USER_STACK_PAGES + 1) * PAGE_SIZE;
    for page in 1..=USER_STACK_PAGES {
        if let Some(frame) = unmap_page(stack_top - page * PAGE_SIZE) {
            free_physical_frame(frame);
        }
    }
}

pub fn is_user_executable(virtual_address: u64) -> bool {
    if virtual_address < FUTURE_USER_SPACE_BASE || virtual_address >= USER_SPACE_END {
        return false;
    }
    let indices = [
        (virtual_address >> 39) & 0x1FF,
        (virtual_address >> 30) & 0x1FF,
        (virtual_address >> 21) & 0x1FF,
        (virtual_address >> 12) & 0x1FF,
    ];
    let mut table = unsafe { table_mut(read_cr3() & ENTRY_ADDRESS_MASK) };
    for index in indices[..3].iter().copied() {
        let entry = table.0[index as usize];
        if entry & (PRESENT | USER | HUGE_PAGE) != (PRESENT | USER) {
            return false;
        }
        table = unsafe { table_mut(entry & ENTRY_ADDRESS_MASK) };
    }
    let leaf = table.0[indices[3] as usize];
    leaf & (PRESENT | USER) == (PRESENT | USER) && leaf & NO_EXECUTE == 0
}

pub fn is_user_mapped(virtual_address: u64) -> bool {
    if virtual_address < FUTURE_USER_SPACE_BASE || virtual_address >= USER_SPACE_END {
        return false;
    }
    let indices = [
        (virtual_address >> 39) & 0x1FF,
        (virtual_address >> 30) & 0x1FF,
        (virtual_address >> 21) & 0x1FF,
        (virtual_address >> 12) & 0x1FF,
    ];
    let mut table = unsafe { table_mut(read_cr3() & ENTRY_ADDRESS_MASK) };
    for index in indices[..3].iter().copied() {
        let entry = table.0[index as usize];
        if entry & (PRESENT | USER | HUGE_PAGE) != (PRESENT | USER) {
            return false;
        }
        table = unsafe { table_mut(entry & ENTRY_ADDRESS_MASK) };
    }
    let leaf = table.0[indices[3] as usize];
    leaf & (PRESENT | USER) == (PRESENT | USER)
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

fn map_page_in(
    address_space: u64,
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
    let mut table = unsafe { table_mut(address_space & ENTRY_ADDRESS_MASK) };
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
    Ok(())
}

fn unmap_page(virtual_address: u64) -> Option<u64> {
    let indices = [
        (virtual_address >> 39) & 0x1FF,
        (virtual_address >> 30) & 0x1FF,
        (virtual_address >> 21) & 0x1FF,
        (virtual_address >> 12) & 0x1FF,
    ];
    let mut table = unsafe { table_mut(read_cr3() & ENTRY_ADDRESS_MASK) };
    for index in indices[..3].iter().copied() {
        let entry = table.0[index as usize];
        if entry & PRESENT == 0 || entry & HUGE_PAGE != 0 {
            return None;
        }
        table = unsafe { table_mut(entry & ENTRY_ADDRESS_MASK) };
    }
    let leaf = &mut table.0[indices[3] as usize];
    if *leaf & PRESENT == 0 {
        return None;
    }
    let frame = *leaf & ENTRY_ADDRESS_MASK;
    *leaf = 0;
    unsafe {
        asm!("invlpg [{}]", in(reg) virtual_address, options(nostack));
    }
    Some(frame)
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

unsafe fn release_user_table(frame: u64, level: usize) {
    let table = unsafe { table_mut(frame) };
    for index in 0..512 {
        let entry = table.0[index];
        if entry & PRESENT == 0 {
            continue;
        }
        let child = entry & ENTRY_ADDRESS_MASK;
        if level == 1 || entry & HUGE_PAGE != 0 {
            free_physical_frame(child);
        } else {
            unsafe { release_user_table(child, level - 1) };
        }
        table.0[index] = 0;
    }
    free_physical_frame(frame);
}
fn read_cr3() -> u64 {
    let value: u64;
    unsafe {
        asm!("mov {}, cr3", out(reg) value, options(nomem, nostack));
    }
    value
}
