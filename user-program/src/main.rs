#![no_std]
#![no_main]

use core::arch::asm;
use core::panic::PanicInfo;

static BANNER: [u8; 37] = *b"OPEN KERNEL USER CONSOLE\r\nTYPE HELP\r\n";
static PROMPT: [u8; 2] = *b"> ";
static HELP: [u8; 36] = *b"COMMANDS: HELP CLEAR EXIT SHUTDOWN\r\n";
static UNKNOWN: [u8; 17] = *b"UNKNOWN COMMAND\r\n";
static EXITING: [u8; 9] = *b"GOODBYE\r\n";

#[unsafe(no_mangle)]
extern "C" fn _start() -> ! {
    syscall0(7);
    write(&BANNER);
    write(&PROMPT);
    let mut input = core::mem::MaybeUninit::<[u8; 64]>::uninit();
    let mut length = 0;

    loop {
        let character = syscall0(6) as u8;
        if character == 0 {
            syscall1(5, 1);
            continue;
        }
        if character == b'\n' || character == b'\r' {
            write(b"\r\n");
            let input = input.as_ptr().cast::<u8>();
            if equals(input, length, b"help") {
                write(&HELP);
            } else if equals(input, length, b"clear") {
                syscall0(7);
            } else if equals(input, length, b"exit") {
                write(&EXITING);
                syscall0(3);
            } else if equals(input, length, b"shutdown") {
                write(&EXITING);
                syscall0(9);
            } else if length != 0 {
                write(&UNKNOWN);
            }
            length = 0;
            write(&PROMPT);
            continue;
        }
        if character == 8 {
            if length != 0 {
                length -= 1;
                syscall0(8);
            }
            continue;
        }
        if length < 64 {
            unsafe {
                input.as_mut_ptr().cast::<u8>().add(length).write(character);
            }
            length += 1;
            write(core::slice::from_ref(&character));
        }
    }
}

fn equals(input: *const u8, length: usize, expected: &[u8]) -> bool {
    if length != expected.len() {
        return false;
    }
    for index in 0..length {
        if unsafe { input.add(index).read() } != expected[index] {
            return false;
        }
    }
    true
}

fn write(bytes: &[u8]) {
    unsafe {
        asm!(
            "syscall",
            inlateout("rax") 1_u64 => _,
            in("rdi") bytes.as_ptr() as u64,
            in("rsi") bytes.len() as u64,
            clobber_abi("sysv64"),
        );
    }
}

fn syscall0(number: u64) -> u64 {
    let result: u64;
    unsafe {
        asm!(
            "syscall",
            inlateout("rax") number => result,
            clobber_abi("sysv64"),
        );
    }
    result
}

fn syscall1(number: u64, argument: u64) -> u64 {
    let result: u64;
    unsafe {
        asm!(
            "syscall",
            inlateout("rax") number => result,
            in("rdi") argument,
            clobber_abi("sysv64"),
        );
    }
    result
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
