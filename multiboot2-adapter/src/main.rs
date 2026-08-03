#![no_std]
#![no_main]

use core::panic::PanicInfo;
use kernel_core::{BootInfo, BootStatus, Display, Framebuffer};
use multiboot2::{BootInformation, BootInformationHeader, MAGIC};

// Keep the bootstrap object, including its Multiboot2 entry point, in the ELF.
#[used]
static BOOTSTRAP_LINKAGE: &u8 = &multiboot2_adapter_bootstrap::BOOTSTRAP_LINK;

#[unsafe(no_mangle)]
pub extern "C" fn kernel_main(magic: u32, boot_info_address: usize) -> ! {
    if magic != MAGIC {
        kernel_core::boot(BootInfo::new(
            Display::VgaText,
            "GRUB Multiboot2",
            BootStatus::InvalidBootInfo,
        ));
    }

    let (display, status) = match unsafe {
        BootInformation::load(boot_info_address as *const BootInformationHeader)
    } {
        Ok(boot_info) => {
            let display = boot_info
                .framebuffer_tag()
                .and_then(Result::ok)
                .filter(|framebuffer| framebuffer.bpp() == 32)
                .map(|framebuffer| {
                    Display::Framebuffer(Framebuffer::new(
                        framebuffer.address() as *mut u8,
                        framebuffer.width() as usize,
                        framebuffer.height() as usize,
                        framebuffer.pitch() as usize,
                        framebuffer.bpp() as u16,
                    ))
                })
                .unwrap_or(Display::VgaText);
            (display, BootStatus::Ready)
        }
        Err(_) => (Display::VgaText, BootStatus::InvalidBootInfo),
    };

    kernel_core::boot(BootInfo::new(display, "GRUB Multiboot2", status));
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    kernel_core::halt()
}
