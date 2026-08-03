use crate::{
    FUTURE_USER_SPACE_BASE, PAGE_SIZE, PagingError, allocate_physical_frame, map_user_code_page,
    map_user_page, write_physical_frame,
};

const USER_CODE_ADDRESS: u64 = FUTURE_USER_SPACE_BASE;
const USER_DATA_ADDRESS: u64 = USER_CODE_ADDRESS + PAGE_SIZE;
const USER_STACK_ADDRESS: u64 = USER_CODE_ADDRESS + PAGE_SIZE * 3;

// spawn; sleep(10); write(USER_DATA_ADDRESS, 24); exit
const USER_PROGRAM: [u8; 60] = [
    0x48, 0xC7, 0xC0, 4, 0, 0, 0, 0x0F, 0x05, 0x48, 0xC7, 0xC0, 5, 0, 0, 0, 0x48, 0xC7, 0xC7,
    10, 0, 0, 0, 0x0F, 0x05, 0x48, 0xC7, 0xC0, 1, 0, 0, 0, 0x48, 0xBF, 0x00, 0x10, 0x00, 0x00,
    0x00, 0x40, 0x00, 0x00, 0x48, 0xC7, 0xC6, 24, 0, 0, 0, 0x0F, 0x05, 0x48, 0xC7, 0xC0, 3,
    0, 0, 0, 0x0F, 0x05,
];
const USER_MESSAGE: &[u8] = b"open-kernel: user mode\r\n";

pub fn run_demo() -> Result<(), PagingError> {
    let code_frame = allocate_physical_frame().ok_or(PagingError::FrameAllocationFailed)?;
    map_user_code_page(USER_CODE_ADDRESS, code_frame)?;
    write_physical_frame(code_frame, &USER_PROGRAM);

    let data_frame = allocate_physical_frame().ok_or(PagingError::FrameAllocationFailed)?;
    map_user_page(USER_DATA_ADDRESS, data_frame)?;
    write_physical_frame(data_frame, USER_MESSAGE);

    let stack_frame = allocate_physical_frame().ok_or(PagingError::FrameAllocationFailed)?;
    map_user_page(USER_STACK_ADDRESS - PAGE_SIZE, stack_frame)?;

    crate::scheduler::spawn(user_entry).ok_or(PagingError::FrameAllocationFailed)?;
    crate::scheduler::start()
}

pub(crate) extern "C" fn user_entry() -> ! {
    unsafe {
        crate::arch::enter_user_mode(USER_CODE_ADDRESS, USER_STACK_ADDRESS);
    }
}
