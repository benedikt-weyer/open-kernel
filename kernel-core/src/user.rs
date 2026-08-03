use crate::{
    ElfError, LoadedImage, PAGE_SIZE, PageFlags, PagingError, allocate_physical_frame,
    load_user_elf, map_user_page_with_flags, vfs_open, zero_physical_frame,
};

static mut USER_IMAGE: Option<LoadedImage> = None;
const USER_HEAP_BASE: u64 = 0x0000_5000_0000_0000;
const USER_HEAP_LIMIT: u64 = 0x0000_7000_0000_0000;
static mut USER_HEAP_BREAK: u64 = USER_HEAP_BASE;
static mut USER_HEAP_MAPPED_END: u64 = USER_HEAP_BASE;

pub fn run_demo() -> Result<(), ElfError> {
    let image = vfs_open("/init").ok_or(ElfError::InvalidHeader)?;
    let loaded = load_user_elf(image)?;
    unsafe {
        USER_IMAGE = Some(loaded);
        USER_HEAP_BREAK = USER_HEAP_BASE;
        USER_HEAP_MAPPED_END = USER_HEAP_BASE;
    }
    crate::scheduler::initialize_user_process();
    crate::scheduler::spawn_user(loaded.entry, 0, Some(loaded.stack_pointer))
        .ok_or(PagingError::FrameAllocationFailed)?;
    crate::scheduler::start()
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

