#![no_main]

use std::collections::HashMap;

#[unsafe(no_mangle)]
unsafe extern "C" fn memcpy(destination: *mut u8, source: *const u8, length: usize) -> *mut u8 {
    for index in 0..length {
        unsafe { destination.add(index).write(source.add(index).read()) };
    }
    destination
}

#[unsafe(no_mangle)]
unsafe extern "C" fn memmove(destination: *mut u8, source: *const u8, length: usize) -> *mut u8 {
    if destination.addr() <= source.addr() {
        unsafe { memcpy(destination, source, length) }
    } else {
        for index in (0..length).rev() {
            unsafe { destination.add(index).write(source.add(index).read()) };
        }
        destination
    }
}

#[unsafe(no_mangle)]
unsafe extern "C" fn memset(destination: *mut u8, value: i32, length: usize) -> *mut u8 {
    for index in 0..length {
        unsafe { destination.add(index).write(value as u8) };
    }
    destination
}

#[unsafe(no_mangle)]
unsafe extern "C" fn memcmp(left: *const u8, right: *const u8, length: usize) -> i32 {
    for index in 0..length {
        let (left, right) = unsafe { (left.add(index).read(), right.add(index).read()) };
        if left != right { return i32::from(left) - i32::from(right); }
    }
    0
}

#[unsafe(no_mangle)]
unsafe extern "C" fn strlen(value: *const u8) -> usize {
    let mut length = 0;
    while unsafe { value.add(length).read() } != 0 { length += 1; }
    length
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let args: Vec<_> = std::env::args().collect();
    let mut values = Vec::new();
    values.extend_from_slice(&[3_u64, 5, 8, 13]);

    // HashMap construction requests randomized keys from the OpenKernel std PAL.
    let mut map = HashMap::new();
    map.insert("kernel", "open");

    println!("openkernel std smoke test");
    println!("args={} vec_sum={} map={}", args.len(), values.iter().sum::<u64>(), map["kernel"]);

    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") 16_u64,
            in("rdi") 0_u64,
            clobber_abi("sysv64"),
            options(noreturn),
        );
    }
}
