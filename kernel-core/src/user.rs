use crate::{
    ElfError, LoadedImage, PAGE_SIZE, PageFlags, PagingError, allocate_physical_frame,
    load_user_elf, map_user_page_with_flags, vfs_open, zero_physical_frame,
};

static mut USER_IMAGE: Option<LoadedImage> = None;
const USER_HEAP_BASE: u64 = 0x0000_5000_0000_0000;
const USER_HEAP_LIMIT: u64 = 0x0000_7000_0000_0000;
static mut USER_HEAP_BREAK: u64 = USER_HEAP_BASE;
static mut USER_HEAP_MAPPED_END: u64 = USER_HEAP_BASE;
const EXECUTABLE_PATH: &[u8] = b"/init";
const CURRENT_DIRECTORY_MAX: usize = 5;
static mut CURRENT_DIRECTORY: [u8; CURRENT_DIRECTORY_MAX] = [0; CURRENT_DIRECTORY_MAX];
static mut CURRENT_DIRECTORY_LENGTH: usize = 1;
static mut EXECUTABLE_ENTRY: u64 = 0;
static mut RESOLVED_PATH: [u8; FD_PATH_LENGTH] = [0; FD_PATH_LENGTH];
const MAX_FDS: usize = 16;
const FD_PATH_LENGTH: usize = 64;
pub const OPEN_WRITE: u64 = 1;
pub const OPEN_CREATE: u64 = 2;
pub const OPEN_DIRECTORY: u64 = 4;

#[derive(Clone, Copy)]
pub struct ExecutableMetadata {
    pub entry: u64,
    pub path: &'static str,
}

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
    let mut loaded = load_user_elf(image)?;
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
        CURRENT_DIRECTORY[0] = b'/';
        CURRENT_DIRECTORY_LENGTH = 1;
        EXECUTABLE_ENTRY = loaded.entry;
    }
    loaded.stack_pointer = initialize_process_stack(loaded.stack_pointer);
    crate::scheduler::initialize_user_process();
    crate::scheduler::spawn_user(loaded.entry, 0, 0, Some(loaded.stack_pointer))
        .ok_or(PagingError::FrameAllocationFailed)?;
    crate::scheduler::start()
}

pub fn executable_metadata() -> ExecutableMetadata {
    ExecutableMetadata {
        entry: unsafe { EXECUTABLE_ENTRY },
        path: "/init",
    }
}

pub(crate) fn open(path: &[u8], flags: u64) -> u64 {
    let Some(path) = resolve_path(path) else {
        return u64::MAX;
    };
    let directory = flags & OPEN_DIRECTORY != 0;
    let writable = flags & OPEN_WRITE != 0;
    let create = flags & OPEN_CREATE != 0;
    if (directory && !crate::vfs_is_directory(path)) || (!directory && crate::vfs_open_file(path, writable, create).is_err()) {
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

pub(crate) fn spawn(path: &[u8]) -> u64 {
    let Some(path) = resolve_path(path) else {
        return u64::MAX;
    };
    let Some(image) = crate::vfs_open(path) else {
        return u64::MAX;
    };
    let Ok(address_space) = crate::create_user_address_space() else {
        return u64::MAX;
    };
    let Some(process) = crate::create_process(address_space) else {
        return u64::MAX;
    };
    let Ok(loaded) = crate::load_user_elf_into(image, address_space, process) else {
        return u64::MAX;
    };
    let previous_address_space = crate::active_address_space();
    unsafe { crate::switch_address_space(address_space) };
    let stack_pointer = initialize_process_stack(loaded.stack_pointer);
    unsafe { crate::switch_address_space(previous_address_space) };
    let Some(thread) = crate::spawn_user_for_process(process, loaded.entry, 0, 0, Some(stack_pointer)) else {
        return u64::MAX;
    };
    crate::set_process_main_thread(process, thread);
    process as u64
}

pub(crate) fn chdir(path: &[u8]) -> u64 {
    let Some(path) = resolve_path(path) else {
        return u64::MAX;
    };
    if !crate::vfs_is_directory(path) {
        return u64::MAX;
    }
    unsafe {
        CURRENT_DIRECTORY_LENGTH = path.len();
        for (index, byte) in path.as_bytes().iter().enumerate() {
            CURRENT_DIRECTORY[index] = *byte;
        }
    }
    0
}

pub(crate) fn getcwd(output: &mut [u8]) -> u64 {
    unsafe {
        if output.len() < CURRENT_DIRECTORY_LENGTH + 1 {
            return u64::MAX;
        }
        for index in 0..CURRENT_DIRECTORY_LENGTH {
            output[index] = CURRENT_DIRECTORY[index];
        }
        output[CURRENT_DIRECTORY_LENGTH] = 0;
        CURRENT_DIRECTORY_LENGTH as u64
    }
}

pub(crate) fn executable_info(output: &mut [u8]) -> u64 {
    if output.len() < EXECUTABLE_PATH.len() + 1 {
        return u64::MAX;
    }
    for (index, byte) in EXECUTABLE_PATH.iter().enumerate() {
        output[index] = *byte;
    }
    output[EXECUTABLE_PATH.len()] = 0;
    unsafe { EXECUTABLE_ENTRY }
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

pub(crate) fn stat(fd: u64, output: &mut [u8]) -> u64 {
    if output.len() < 16 {
        return u64::MAX;
    }
    let Some(index) = fd.checked_sub(3).map(|value| value as usize).filter(|index| *index < MAX_FDS) else {
        return u64::MAX;
    };
    unsafe {
        let descriptor = &(*(&raw const FILE_DESCRIPTORS))[index];
        let Ok(path) = core::str::from_utf8(&descriptor.path[..descriptor.path_length]) else {
            return u64::MAX;
        };
        let kind = match descriptor.kind {
            DescriptorKind::File => 1,
            DescriptorKind::Directory => 2,
            DescriptorKind::Empty => return u64::MAX,
        };
        output[0] = kind;
        output[1] = u8::from(!descriptor.writable);
        let size = if descriptor.kind == DescriptorKind::File {
            crate::vfs_open(path).map_or(0, |file| file.len()) as u64
        } else {
            0
        };
        for byte in 0..8 {
            output[8 + byte] = (size >> (byte * 8)) as u8;
        }
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

fn resolve_path(path: &[u8]) -> Option<&'static str> {
    unsafe {
        let mut length = 0;
        if path.first() != Some(&b'/') {
            if CURRENT_DIRECTORY_LENGTH + 1 + path.len() >= FD_PATH_LENGTH {
                return None;
            }
            for index in 0..CURRENT_DIRECTORY_LENGTH {
                RESOLVED_PATH[length] = CURRENT_DIRECTORY[index];
                length += 1;
            }
            if length != 1 {
                RESOLVED_PATH[length] = b'/';
                length += 1;
            }
        }
        if length + path.len() >= FD_PATH_LENGTH {
            return None;
        }
        for (index, byte) in path.iter().enumerate() {
            RESOLVED_PATH[length + index] = *byte;
        }
        length += path.len();
        core::str::from_utf8(&RESOLVED_PATH[..length]).ok()
    }
}

fn initialize_process_stack(stack_pointer: u64) -> u64 {
    let program_name = b"init\0";
    let path = b"PATH=/bin\0";
    let pwd = b"PWD=/\0";
    let mut cursor = stack_pointer + 8;
    cursor -= program_name.len() as u64;
    let program_name_pointer = cursor;
    write_user_bytes(cursor, program_name);
    cursor -= path.len() as u64;
    let path_pointer = cursor;
    write_user_bytes(cursor, path);
    cursor -= pwd.len() as u64;
    let pwd_pointer = cursor;
    write_user_bytes(cursor, pwd);
    cursor &= !0xF;

    // A padding word preserves the 8-byte System V function-entry alignment
    // used by the hand-written Rust `_start` in this image.
    for value in [0, 0, 0, 0, pwd_pointer, path_pointer, 0, program_name_pointer, 1] {
        cursor -= 8;
        unsafe { core::ptr::write_volatile(cursor as *mut u64, value) };
    }
    cursor
}

fn write_user_bytes(address: u64, bytes: &[u8]) {
    for (index, byte) in bytes.iter().enumerate() {
        unsafe { core::ptr::write_volatile((address as *mut u8).add(index), *byte) };
    }
}

