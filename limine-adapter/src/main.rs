#![no_std]
#![no_main]

use core::{arch::asm, panic::PanicInfo};
use kernel_core::{
    BootInfo, BootStatus, Display, Framebuffer, MemoryRegion, MemoryRegionKind, PagingConfig,
    PhysicalMemoryRange,
};
use limine::{
    BaseRevision,
    memory_map::EntryType,
    request::{
        EntryPointRequest, ExecutableAddressRequest, FramebufferRequest, HhdmRequest,
        MemoryMapRequest, ModuleRequest, RequestsEndMarker, RequestsStartMarker,
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
static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

#[used]
#[unsafe(link_section = ".limine_requests")]
static MODULE_REQUEST: ModuleRequest = ModuleRequest::new();

#[used]
#[unsafe(link_section = ".limine_requests")]
static ENTRY_POINT_REQUEST: EntryPointRequest =
    EntryPointRequest::new().with_entry_point(limine_entry);

#[used]
#[unsafe(link_section = ".limine_requests_end")]
static REQUESTS_END: RequestsEndMarker = RequestsEndMarker::new();

#[unsafe(no_mangle)]
unsafe extern "C" fn strlen(value: *const u8) -> usize {
    let mut length = 0;
    while unsafe { value.add(length).read() } != 0 {
        length += 1;
    }
    length
}

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
    let physical_offset = HHDM_REQUEST
        .get_response()
        .map(|response| response.offset())
        .unwrap_or(0);
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
        [kernel_range].into_iter().chain(
            MODULE_REQUEST
                .get_response()
                .into_iter()
                .flat_map(|response| response.modules().iter())
                .map(|module| {
                    let address = module.addr() as u64;
                    PhysicalMemoryRange::new(
                        if address >= physical_offset {
                            address - physical_offset
                        } else {
                            address
                        },
                        module.size(),
                    )
                }),
        ),
    );
    let _ = kernel_core::initialize_virtual_memory(PagingConfig::new(
        physical_offset,
        kernel_start_address,
        kernel_range.base,
        kernel_range.length,
    ));
    if let Some(response) = MODULE_REQUEST.get_response() {
        for module in response.modules() {
            let data =
                unsafe { core::slice::from_raw_parts(module.addr() as *const u8, module.size() as usize) };
            let name = match module.string().to_bytes() {
                b"std-smoke" => "std-smoke",
                b"console" => "console",
                _ => "init",
            };
            let _ = kernel_core::register_boot_file(name, data);
        }
    }

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
