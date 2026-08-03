use crate::{ElfError, LoadedImage, PagingError, load_user_elf, open_file};

static mut USER_IMAGE: Option<LoadedImage> = None;

pub fn run_demo() -> Result<(), ElfError> {
    let image = open_file("init").ok_or(ElfError::InvalidHeader)?;
    let loaded = load_user_elf(image)?;
    unsafe {
        USER_IMAGE = Some(loaded);
    }
    crate::scheduler::spawn(user_entry).ok_or(PagingError::FrameAllocationFailed)?;
    crate::scheduler::start()
}

pub(crate) extern "C" fn user_entry() -> ! {
    let image = unsafe { USER_IMAGE.expect("user image missing") };
    unsafe {
        crate::arch::enter_user_mode(image.entry, image.stack_pointer);
    }
}
