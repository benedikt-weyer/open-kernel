#![no_main]

use std::collections::HashMap;

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
