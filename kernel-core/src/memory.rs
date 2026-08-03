use core::ptr::write_volatile;

pub const PAGE_SIZE: u64 = 4096;
const MAX_PHYSICAL_ADDRESS: u64 = 4 * 1024 * 1024 * 1024;
const MAX_PHYSICAL_FRAMES: usize = (MAX_PHYSICAL_ADDRESS / PAGE_SIZE) as usize;
const FRAME_BITMAP_BYTES: usize = MAX_PHYSICAL_FRAMES / 8;

#[derive(Clone, Copy)]
pub enum MemoryRegionKind {
    Usable,
    Reserved,
}

#[derive(Clone, Copy)]
pub struct MemoryRegion {
    pub base: u64,
    pub length: u64,
    pub kind: MemoryRegionKind,
}
impl MemoryRegion {
    pub const fn new(base: u64, length: u64, kind: MemoryRegionKind) -> Self {
        Self { base, length, kind }
    }
}

#[derive(Clone, Copy)]
pub struct PhysicalMemoryRange {
    pub base: u64,
    pub length: u64,
}
impl PhysicalMemoryRange {
    pub const fn new(base: u64, length: u64) -> Self {
        Self { base, length }
    }
}

#[derive(Clone, Copy)]
pub struct PhysicalMemoryStats {
    pub free_frames: usize,
    pub tracked_frames: usize,
}
impl PhysicalMemoryStats {
    const EMPTY: Self = Self {
        free_frames: 0,
        tracked_frames: MAX_PHYSICAL_FRAMES,
    };
}

pub trait PhysicalFrameAllocator {
    fn initialize(
        regions: impl IntoIterator<Item = MemoryRegion>,
        reserved_ranges: impl IntoIterator<Item = PhysicalMemoryRange>,
    ) -> PhysicalMemoryStats;
    fn allocate() -> Option<u64>;
    fn free(address: u64);
    fn stats() -> PhysicalMemoryStats;
}

pub struct BitmapFrameAllocator;
static mut FRAME_BITMAP: [u8; FRAME_BITMAP_BYTES] = [0; FRAME_BITMAP_BYTES];
static mut ALLOCATED_BITMAP: [u8; FRAME_BITMAP_BYTES] = [0; FRAME_BITMAP_BYTES];
static mut MEMORY_STATS: PhysicalMemoryStats = PhysicalMemoryStats::EMPTY;

impl PhysicalFrameAllocator for BitmapFrameAllocator {
    fn initialize(
        regions: impl IntoIterator<Item = MemoryRegion>,
        reserved_ranges: impl IntoIterator<Item = PhysicalMemoryRange>,
    ) -> PhysicalMemoryStats {
        unsafe {
            for byte in &mut *(&raw mut FRAME_BITMAP) {
                write_volatile(byte, u8::MAX);
            }
            for byte in &mut *(&raw mut ALLOCATED_BITMAP) {
                write_volatile(byte, 0);
            }
        }
        for region in regions {
            if matches!(region.kind, MemoryRegionKind::Usable) {
                mark_range(region.base, region.length, false);
            }
        }
        mark_range(0, 1024 * 1024, true);
        for range in reserved_ranges {
            mark_range(range.base, range.length, true);
        }
        let mut free_frames = 0;
        for frame in 0..MAX_PHYSICAL_FRAMES {
            if !reserved(frame) {
                free_frames += 1;
            }
        }
        let stats = PhysicalMemoryStats {
            free_frames,
            tracked_frames: MAX_PHYSICAL_FRAMES,
        };
        unsafe {
            core::ptr::write_volatile(&raw mut MEMORY_STATS, stats);
        }
        stats
    }
    fn allocate() -> Option<u64> {
        for frame in 0..MAX_PHYSICAL_FRAMES {
            if !reserved(frame) {
                set(&raw mut FRAME_BITMAP, frame, true);
                set(&raw mut ALLOCATED_BITMAP, frame, true);
                unsafe {
                    (*(&raw mut MEMORY_STATS)).free_frames -= 1;
                }
                return Some(frame as u64 * PAGE_SIZE);
            }
        }
        None
    }
    fn free(address: u64) {
        if address % PAGE_SIZE != 0 || address >= MAX_PHYSICAL_ADDRESS {
            return;
        }
        let frame = (address / PAGE_SIZE) as usize;
        if bit(&raw const ALLOCATED_BITMAP, frame) {
            set(&raw mut FRAME_BITMAP, frame, false);
            set(&raw mut ALLOCATED_BITMAP, frame, false);
            unsafe {
                (*(&raw mut MEMORY_STATS)).free_frames += 1;
            }
        }
    }
    fn stats() -> PhysicalMemoryStats {
        unsafe { core::ptr::read_volatile(&raw const MEMORY_STATS) }
    }
}

fn mark_range(base: u64, length: u64, is_reserved: bool) {
    let start = base.saturating_add(PAGE_SIZE - 1) / PAGE_SIZE;
    let end = base.saturating_add(length).min(MAX_PHYSICAL_ADDRESS) / PAGE_SIZE;
    for frame in start.min(MAX_PHYSICAL_FRAMES as u64)..end {
        set(&raw mut FRAME_BITMAP, frame as usize, is_reserved);
    }
}
fn reserved(frame: usize) -> bool {
    bit(&raw const FRAME_BITMAP, frame)
}
fn bit(bitmap: *const [u8; FRAME_BITMAP_BYTES], frame: usize) -> bool {
    let byte = unsafe { core::ptr::read_volatile((*bitmap).as_ptr().add(frame / 8)) };
    byte & (1 << (frame % 8)) != 0
}
fn set(bitmap: *mut [u8; FRAME_BITMAP_BYTES], frame: usize, value: bool) {
    let byte = unsafe { (*bitmap).as_mut_ptr().add(frame / 8) };
    let mask = 1 << (frame % 8);
    let current = unsafe { core::ptr::read_volatile(byte) };
    unsafe {
        write_volatile(
            byte,
            if value {
                current | mask
            } else {
                current & !mask
            },
        );
    }
}

pub fn initialize_physical_memory(
    regions: impl IntoIterator<Item = MemoryRegion>,
    reserved: impl IntoIterator<Item = PhysicalMemoryRange>,
) -> PhysicalMemoryStats {
    BitmapFrameAllocator::initialize(regions, reserved)
}
pub fn allocate_physical_frame() -> Option<u64> {
    BitmapFrameAllocator::allocate()
}
pub fn free_physical_frame(address: u64) {
    BitmapFrameAllocator::free(address)
}
pub fn physical_memory_stats() -> PhysicalMemoryStats {
    BitmapFrameAllocator::stats()
}
