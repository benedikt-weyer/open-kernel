#![no_main]

static mut TLS_TCB: [u8; 128] = [0; 128];

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
    unsafe {
        let base = (&raw mut TLS_TCB).cast::<u8>().add(32);
        (base as *mut u64).write(base as u64);
        core::arch::asm!(
            "syscall",
            inlateout("rax") 30_u64 => _,
            in("rdi") base as u64,
            clobber_abi("sysv64"),
        );
    }
    let mut values = Vec::new();
    values.extend_from_slice(&[3_u64, 5, 8, 13]);

    println!("openkernel std smoke test");
    println!("vec_sum={}", values.iter().sum::<u64>());

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
