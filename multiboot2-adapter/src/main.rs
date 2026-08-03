#![no_std]
#![no_main]

use core::panic::PanicInfo;
use kernel_core::{BootInfo, BootStatus, Display};
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

    let status = match unsafe { BootInformation::load(boot_info_address as *const BootInformationHeader) } {
        Ok(_) => BootStatus::Ready,
        Err(_) => BootStatus::InvalidBootInfo,
    };

    kernel_core::boot(BootInfo::new(Display::VgaText, "GRUB Multiboot2", status));
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    kernel_core::halt()
}
