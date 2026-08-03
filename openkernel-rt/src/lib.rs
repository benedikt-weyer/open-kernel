#![no_std]

use core::ffi::c_void;

unsafe extern "C" {
    fn main();
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    unsafe {
        main();
        core::arch::asm!(
            "syscall",
            in("rax") 16_u64,
            in("rdi") 0_u64,
            clobber_abi("sysv64"),
            options(noreturn),
        );
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn memcpy(dst: *mut c_void, src: *const c_void, len: usize) -> *mut c_void {
    for index in 0..len {
        unsafe { (dst as *mut u8).add(index).write((src as *const u8).add(index).read()) };
    }
    dst
}

#[unsafe(no_mangle)]
unsafe extern "C" fn memmove(dst: *mut c_void, src: *const c_void, len: usize) -> *mut c_void {
    if dst.addr() <= src.addr() {
        unsafe { memcpy(dst, src, len) }
    } else {
        for index in (0..len).rev() {
            unsafe { (dst as *mut u8).add(index).write((src as *const u8).add(index).read()) };
        }
        dst
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn memset(dst: *mut c_void, value: i32, len: usize) -> *mut c_void {
    for index in 0..len {
        unsafe { (dst as *mut u8).add(index).write(value as u8) };
    }
    dst
}

#[unsafe(no_mangle)]
unsafe extern "C" fn memcmp(left: *const c_void, right: *const c_void, len: usize) -> i32 {
    for index in 0..len {
        let (left, right) = unsafe {
            ((left as *const u8).add(index).read(), (right as *const u8).add(index).read())
        };
        if left != right { return i32::from(left) - i32::from(right); }
    }
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn strlen(value: *const i8) -> usize {
    let mut len = 0;
    while unsafe { value.add(len).read() } != 0 { len += 1; }
    len
}
