#![no_std]
#![no_main]

use core::arch::asm;
use core::panic::PanicInfo;
use core::ptr::write_volatile;
use multiboot2::{BootInformation, BootInformationHeader, MAGIC};

// Keep the bootstrap object, including its Multiboot2 entry point, in the ELF.
#[used]
static BOOTSTRAP_LINKAGE: &u8 = &bootstrap::BOOTSTRAP_LINK;

const VGA_TEXT_BUFFER: *mut u16 = 0xB8000 as *mut u16;
const VGA_COLOR: u16 = 0x0F00;

#[unsafe(no_mangle)]
pub extern "C" fn kernel_main(magic: u32, boot_info_address: usize) -> ! {
    for index in 0..(80 * 25) {
        unsafe {
            write_volatile(VGA_TEXT_BUFFER.add(index), VGA_COLOR | b' ' as u16);
        }
    }

    if magic != MAGIC {
        write_text(b"Invalid Multiboot2 handoff", 0);
    } else {
        let boot_info = unsafe {
            BootInformation::load(boot_info_address as *const BootInformationHeader)
        };

        match boot_info {
            Ok(boot_info) => {
                write_text(b"open-kernel is running", 0);

                if let Some(tag) = boot_info.boot_loader_name_tag()
                    && let Ok(name) = tag.name()
                {
                    write_text(name.as_bytes(), 1);
                }
            }
            Err(_) => write_text(b"Invalid Multiboot2 information", 0),
        }
    }

    loop {
        unsafe {
            asm!("hlt", options(nomem, nostack));
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
            asm!("cli; hlt", options(nomem, nostack));
        }
    }
}
