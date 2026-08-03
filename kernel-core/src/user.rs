use crate::{
    ElfError, LoadedImage, PAGE_SIZE, PageFlags, PagingError, allocate_physical_frame,
    load_user_elf, map_user_page_with_flags, vfs_open, zero_physical_frame,
};

static mut USER_IMAGE: Option<LoadedImage> = None;
const USER_HEAP_BASE: u64 = 0x0000_5000_0000_0000;
const USER_HEAP_LIMIT: u64 = 0x0000_7000_0000_0000;
static mut USER_HEAP_BREAK: u64 = USER_HEAP_BASE;
static mut USER_HEAP_MAPPED_END: u64 = USER_HEAP_BASE;
const MAX_FDS: usize = 16;
const FD_PATH_LENGTH: usize = 64;
pub const OPEN_WRITE: u64 = 1;
pub const OPEN_CREATE: u64 = 2;
pub const OPEN_DIRECTORY: u64 = 4;

#[derive(Clone, Copy, PartialEq, Eq)]
enum DescriptorKind {
    Empty,
    File,
    Directory,
}
#[derive(Clone, Copy)]
struct Descriptor {
    kind: DescriptorKind,
    path: [u8; FD_PATH_LENGTH],
    path_length: usize,
    position: usize,
    writable: bool,
}
impl Descriptor {
    const EMPTY: Self = Self {
        kind: DescriptorKind::Empty,
        path: [0; FD_PATH_LENGTH],
        path_length: 0,
        position: 0,
        writable: false,
    };
}
static mut FILE_DESCRIPTORS: [Descriptor; MAX_FDS] = [Descriptor::EMPTY; MAX_FDS];

pub fn run_demo() -> Result<(), ElfError> {
    let image = vfs_open("/init").ok_or(ElfError::InvalidHeader)?;
    let loaded = load_user_elf(image)?;
    unsafe {
        USER_IMAGE = Some(loaded);
        USER_HEAP_BREAK = USER_HEAP_BASE;
        USER_HEAP_MAPPED_END = USER_HEAP_BASE;
        for index in 0..MAX_FDS {
            core::ptr::write_volatile(
                (&raw mut FILE_DESCRIPTORS).cast::<Descriptor>().add(index),
                Descriptor::EMPTY,
            );
        }
    }
    crate::scheduler::initialize_user_process();
    crate::scheduler::spawn_user(loaded.entry, 0, Some(loaded.stack_pointer))
        .ok_or(PagingError::FrameAllocationFailed)?;
    crate::scheduler::start()
}

pub(crate) fn open(path: &[u8], flags: u64) -> u64 {
    let Ok(path) = core::str::from_utf8(path) else {
        return u64::MAX;
    };
    let directory = flags & OPEN_DIRECTORY != 0;
    let writable = flags & OPEN_WRITE != 0;
    let create = flags & OPEN_CREATE != 0;
    if (directory && (path != "/" && path != "/tmp")) || (!directory && crate::vfs_open_file(path, writable, create).is_err()) {
        return u64::MAX;
    }
    unsafe {
        for index in 0..MAX_FDS {
            let descriptor = &mut (*(&raw mut FILE_DESCRIPTORS))[index];
            if descriptor.kind == DescriptorKind::Empty {
                if path.len() >= FD_PATH_LENGTH {
                    return u64::MAX;
                }
                descriptor.kind = if directory { DescriptorKind::Directory } else { DescriptorKind::File };
                for (offset, byte) in path.as_bytes().iter().enumerate() {
                    descriptor.path[offset] = *byte;
                }
                descriptor.path_length = path.len();
                descriptor.position = 0;
                descriptor.writable = writable;
                return (index + 3) as u64;
            }
        }
    }
    u64::MAX
}

pub(crate) fn read(fd: u64, output: &mut [u8]) -> u64 {
    let Some(index) = fd.checked_sub(3).map(|value| value as usize).filter(|index| *index < MAX_FDS) else {
        return u64::MAX;
    };
    unsafe {
        let descriptor = &mut (*(&raw mut FILE_DESCRIPTORS))[index];
        let Ok(path) = core::str::from_utf8(&descriptor.path[..descriptor.path_length]) else {
            return u64::MAX;
        };
        match descriptor.kind {
            DescriptorKind::File => {
                let Some(data) = crate::vfs_open(path) else { return u64::MAX; };
                let remaining = data.get(descriptor.position..).unwrap_or(&[]);
                let count = remaining.len().min(output.len());
                for offset in 0..count {
                    output[offset] = remaining[offset];
                }
                descriptor.position += count;
                count as u64
            }
            DescriptorKind::Directory => {
                if output.len() < 2 {
                    return u64::MAX;
                }
                let capacity = output.len() - 1;
                let Some(length) = crate::vfs_directory_entry(path, descriptor.position, &mut output[..capacity]) else {
                    return 0;
                };
                output[length] = 0;
                descriptor.position += 1;
                (length + 1) as u64
            }
            DescriptorKind::Empty => u64::MAX,
        }
    }
}

pub(crate) fn write(fd: u64, input: &[u8]) -> u64 {
    let Some(index) = fd.checked_sub(3).map(|value| value as usize).filter(|index| *index < MAX_FDS) else {
        return u64::MAX;
    };
    unsafe {
        let descriptor = &mut (*(&raw mut FILE_DESCRIPTORS))[index];
        if descriptor.kind != DescriptorKind::File || !descriptor.writable {
            return u64::MAX;
        }
        let Ok(path) = core::str::from_utf8(&descriptor.path[..descriptor.path_length]) else {
            return u64::MAX;
        };
        if crate::vfs_write_at(path, descriptor.position, input).is_err() {
            return u64::MAX;
        }
        descriptor.position += input.len();
        input.len() as u64
    }
}

pub(crate) fn close(fd: u64) -> u64 {
    let Some(index) = fd.checked_sub(3).map(|value| value as usize).filter(|index| *index < MAX_FDS) else {
        return u64::MAX;
    };
    unsafe {
        let descriptor = &mut (*(&raw mut FILE_DESCRIPTORS))[index];
        if descriptor.kind == DescriptorKind::Empty {
            return u64::MAX;
        }
        core::ptr::write_volatile(descriptor, Descriptor::EMPTY);
    }
    0
}

pub(crate) fn seek(fd: u64, offset: i64, whence: u64) -> u64 {
    let Some(index) = fd.checked_sub(3).map(|value| value as usize).filter(|index| *index < MAX_FDS) else {
        return u64::MAX;
    };
    unsafe {
        let descriptor = &mut (*(&raw mut FILE_DESCRIPTORS))[index];
        if descriptor.kind == DescriptorKind::Empty {
            return u64::MAX;
        }
        let base = match whence {
            0 => 0_i64,
            1 => descriptor.position as i64,
            2 if descriptor.kind == DescriptorKind::File => {
                let Ok(path) = core::str::from_utf8(&descriptor.path[..descriptor.path_length]) else {
                    return u64::MAX;
                };
                crate::vfs_open(path).map_or(0, |data| data.len()) as i64
            }
            _ => return u64::MAX,
        };
        let Some(position) = base.checked_add(offset).filter(|position| *position >= 0) else {
            return u64::MAX;
        };
        descriptor.position = position as usize;
        descriptor.position as u64
    }
}

pub(crate) fn brk(requested_break: u64) -> u64 {
    unsafe {
        let current_break = USER_HEAP_BREAK;
        if requested_break == 0 {
            return current_break;
        }
        if requested_break < USER_HEAP_BASE || requested_break > USER_HEAP_LIMIT {
            return u64::MAX;
        }
        if requested_break <= current_break {
            // Keep existing mappings; reducing the logical break is safe and lets the
            // allocator reuse the range without needing page-table reclamation yet.
            USER_HEAP_BREAK = requested_break;
            return requested_break;
        }

        let mapped_end = USER_HEAP_MAPPED_END;
        let required_end = match requested_break.checked_add(PAGE_SIZE - 1) {
            Some(value) => value & !(PAGE_SIZE - 1),
            None => return u64::MAX,
        };
        let mut page = mapped_end;
        while page < required_end {
            let Some(frame) = allocate_physical_frame() else {
                return u64::MAX;
            };
            zero_physical_frame(frame);
            if map_user_page_with_flags(page, frame, PageFlags::UserReadWrite).is_err() {
                return u64::MAX;
            }
            page += PAGE_SIZE;
        }
        USER_HEAP_BREAK = requested_break;
        USER_HEAP_MAPPED_END = required_end;
        requested_break
    }
}

