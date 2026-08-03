#![no_std]
#![no_main]

use core::panic::PanicInfo;
use kernel_core::{
    BootInfo, BootStatus, Display, Framebuffer, MemoryRegion, MemoryRegionKind, PhysicalMemoryRange,
};
use multiboot2::{BootInformation, BootInformationHeader, MAGIC, MemoryAreaType};

unsafe extern "C" {
    static kernel_start: u8;
    static kernel_end: u8;
}

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

    let (display, status) =
        match unsafe { BootInformation::load(boot_info_address as *const BootInformationHeader) } {
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
                let kernel_start_address = &raw const kernel_start as u64;
                let kernel_end_address = &raw const kernel_end as u64;
                kernel_core::initialize_physical_memory(
                    boot_info
                        .memory_map_tag()
                        .into_iter()
                        .flat_map(|tag| tag.memory_areas().iter())
                        .map(|area| {
                            MemoryRegion::new(
                                area.start_address(),
                                area.size(),
                                if area.typ() == MemoryAreaType::Available {
                                    MemoryRegionKind::Usable
                                } else {
                                    MemoryRegionKind::Reserved
                                },
                            )
                        }),
                    [PhysicalMemoryRange::new(
                        kernel_start_address,
                        kernel_end_address - kernel_start_address,
                    )],
                );
                (display, BootStatus::Ready)
            }
            Err(_) => {
                kernel_core::initialize_physical_memory(core::iter::empty(), core::iter::empty());
                (Display::VgaText, BootStatus::InvalidBootInfo)
            }
        };

    kernel_core::boot(BootInfo::new(display, "GRUB Multiboot2", status));
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    kernel_core::panic(info)
}
