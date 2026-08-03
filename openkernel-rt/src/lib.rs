#![no_std]

use core::arch::global_asm;
use core::ffi::c_void;

// `std`'s generated `main` symbol expects the SysV `main(argc, argv)`
// convention: argc at `[rsp]` and argv (a pointer to the argv array) at
// `[rsp + 8]`, exactly where the kernel lays them out on a fresh process's
// initial stack. A normal `extern "C" fn _start()` can't read those
// reliably: it takes no parameters, so nothing loads them into rdi/rsi
// before the call to `main`, and any argc/argv the kernel wrote onto the
// stack is silently dropped (every process would observe `std::env::args()`
// as empty, no matter what it was actually spawned with). This reads them
// off `rsp` before anything else touches it and passes them on explicitly.
global_asm!(
    ".global _start",
    "_start:",
    "mov rdi, [rsp]",
    "lea rsi, [rsp + 8]",
    "and rsp, -16",
    "call {trampoline}",
    trampoline = sym start_trampoline,
);

unsafe extern "C" {
    fn main(argc: isize, argv: *const *const u8) -> isize;
}

extern "C" fn start_trampoline(argc: isize, argv: *const *const u8) -> ! {
    unsafe {
        main(argc, argv);
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
