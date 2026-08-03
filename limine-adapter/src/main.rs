#![no_std]
#![no_main]

use core::panic::PanicInfo;
use kernel_core::{BootInfo, BootStatus, Display, Framebuffer};
use limine::{
    BaseRevision,
    request::{EntryPointRequest, FramebufferRequest, RequestsEndMarker, RequestsStartMarker},
};

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
static ENTRY_POINT_REQUEST: EntryPointRequest = EntryPointRequest::new().with_entry_point(limine_entry);

#[used]
#[unsafe(link_section = ".limine_requests_end")]
static REQUESTS_END: RequestsEndMarker = RequestsEndMarker::new();

#[unsafe(no_mangle)]
pub extern "C" fn limine_entry() -> ! {
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

    kernel_core::boot(BootInfo::new(display, "Limine", status));
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    kernel_core::halt()
}
