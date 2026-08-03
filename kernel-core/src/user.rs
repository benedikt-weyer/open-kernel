use crate::{ElfError, LoadedImage, PagingError, load_user_elf, open_file};

static mut USER_IMAGE: Option<LoadedImage> = None;

pub fn run_demo() -> Result<(), ElfError> {
    let image = open_file("init").ok_or(ElfError::InvalidHeader)?;
    let loaded = load_user_elf(image)?;
    unsafe {
        USER_IMAGE = Some(loaded);
    }
    crate::scheduler::initialize_user_process();
    crate::scheduler::spawn_user(loaded.entry, 0, Some(loaded.stack_pointer))
        .ok_or(PagingError::FrameAllocationFailed)?;
    crate::scheduler::start()
}
