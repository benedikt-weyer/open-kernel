#![no_std]
#![no_main]

use core::{arch::asm, panic::PanicInfo};
use kernel_core::{
    BootInfo, BootStatus, Display, Framebuffer, MemoryRegion, MemoryRegionKind, PhysicalMemoryRange,
};
use limine::{
    BaseRevision,
    memory_map::EntryType,
    request::{
        EntryPointRequest, ExecutableAddressRequest, FramebufferRequest, MemoryMapRequest,
        RequestsEndMarker, RequestsStartMarker,
    },
};

unsafe extern "C" {
    static kernel_start: u8;
    static kernel_end: u8;
}

#[used]
#[unsafe(link_section = ".limine_requests_start")]
static REQUESTS_START: RequestsStartMarker = RequestsStartMarker::new();

#[used]
#[unsafe(link_section = ".limine_requests")]
static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
#[unsafe(link_section = ".limine_requests")]
static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

#[used]
#[unsafe(link_section = ".limine_requests")]
static MEMORY_MAP_REQUEST: MemoryMapRequest = MemoryMapRequest::new();

#[used]
#[unsafe(link_section = ".limine_requests")]
static EXECUTABLE_ADDRESS_REQUEST: ExecutableAddressRequest = ExecutableAddressRequest::new();

#[used]
#[unsafe(link_section = ".limine_requests")]
static ENTRY_POINT_REQUEST: EntryPointRequest =
    EntryPointRequest::new().with_entry_point(limine_entry);

#[used]
#[unsafe(link_section = ".limine_requests_end")]
static REQUESTS_END: RequestsEndMarker = RequestsEndMarker::new();

#[unsafe(no_mangle)]
pub extern "C" fn limine_entry() -> ! {
    enable_sse();

    let status = if BASE_REVISION.is_supported() {
        BootStatus::Ready
    } else {
        BootStatus::InvalidBootInfo
    };
    let display = FRAMEBUFFER_REQUEST
        .get_response()
        .and_then(|response| response.framebuffers().next())
        .map(|framebuffer| {
            Display::Framebuffer(Framebuffer::new(
                framebuffer.addr(),
                framebuffer.width() as usize,
                framebuffer.height() as usize,
                framebuffer.pitch() as usize,
                framebuffer.bpp(),
            ))
        })
        .unwrap_or(Display::None);

    let kernel_start_address = &raw const kernel_start as u64;
    let kernel_end_address = &raw const kernel_end as u64;
    let kernel_range = EXECUTABLE_ADDRESS_REQUEST
        .get_response()
        .map(|response| {
            PhysicalMemoryRange::new(
                response.physical_base() + (kernel_start_address - response.virtual_base()),
                kernel_end_address - kernel_start_address,
            )
        })
        .unwrap_or(PhysicalMemoryRange::new(0, 0));
    kernel_core::initialize_physical_memory(
        MEMORY_MAP_REQUEST
            .get_response()
            .into_iter()
            .flat_map(|response| response.entries().iter())
            .map(|entry| {
                MemoryRegion::new(
                    entry.base,
                    entry.length,
                    if entry.entry_type == EntryType::USABLE {
                        MemoryRegionKind::Usable
                    } else {
                        MemoryRegionKind::Reserved
                    },
                )
            }),
        [kernel_range],
    );

    kernel_core::boot(BootInfo::new(display, "Limine", status));
}

fn enable_sse() {
    let mut cr0: u64;
    let mut cr4: u64;

    unsafe {
        asm!("mov {}, cr0", out(reg) cr0, options(nomem, nostack));
        asm!("mov {}, cr4", out(reg) cr4, options(nomem, nostack));
    }

    cr0 &= !(1 << 2);
    cr0 |= 1 << 1;
    cr4 |= (1 << 9) | (1 << 10);

    unsafe {
        asm!("mov cr0, {}", in(reg) cr0, options(nomem, nostack));
        asm!("mov cr4, {}", in(reg) cr4, options(nomem, nostack));
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    kernel_core::panic(info)
}
