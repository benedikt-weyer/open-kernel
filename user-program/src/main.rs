#![no_std]
#![no_main]

use core::arch::asm;
use core::panic::PanicInfo;

static MESSAGE: [u8; 31] = *b"open-kernel: ELF user program\r\n";

#[unsafe(no_mangle)]
extern "C" fn _start() -> ! {
    unsafe {
        asm!(
            "mov rax, 1",
            "lea rdi, [rip + {message}]",
            "mov rsi, {message_length}",
            "syscall",
            "mov rax, 3",
            "syscall",
            message = sym MESSAGE,
            message_length = const MESSAGE.len(),
            options(noreturn),
        );
    }
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
