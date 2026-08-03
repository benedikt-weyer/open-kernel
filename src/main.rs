#![no_std]
#![no_main]

use core::panic::PanicInfo;
use core::ptr::write_volatile;

const VGA_TEXT_BUFFER: *mut u16 = 0xB8000 as *mut u16;
const VGA_COLOR: u16 = 0x0F00;

#[unsafe(no_mangle)]
pub extern "C" fn kernel_main() -> ! {
    for index in 0..(80 * 25) {
        unsafe {
            write_volatile(VGA_TEXT_BUFFER.add(index), VGA_COLOR | b' ' as u16);
        }
    }

    write_text(b"open-kernel is running", 0);

    loop {
        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack));
        }
    }
}

fn write_text(text: &[u8], row: usize) {
    for (column, character) in text.iter().enumerate() {
        unsafe {
            write_volatile(
                VGA_TEXT_BUFFER.add(row * 80 + column),
                VGA_COLOR | *character as u16,
            );
        }
    }
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        unsafe {
            core::arch::asm!("cli; hlt", options(nomem, nostack));
        }
    }
}
