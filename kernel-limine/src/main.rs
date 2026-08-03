#![no_std]
#![no_main]

use core::arch::asm;
use core::panic::PanicInfo;
use core::ptr::write_volatile;
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
    serial_write(b"open-kernel: Limine entry reached\r\n");

    if BASE_REVISION.is_supported() {
        paint_framebuffer();
    }

    halt();
}

fn serial_write(text: &[u8]) {
    for byte in text {
        unsafe {
            asm!("out dx, al", in("dx") 0x3F8_u16, in("al") *byte, options(nomem, nostack));
        }
    }
}

fn paint_framebuffer() {
    let Some(response) = FRAMEBUFFER_REQUEST.get_response() else {
        return;
    };
    let Some(framebuffer) = response.framebuffers().next() else {
        return;
    };

    if framebuffer.bpp() != 32 {
        return;
    }

    let width = framebuffer.width().min(320) as usize;
    let height = framebuffer.height().min(80) as usize;
    let pitch = framebuffer.pitch() as usize;

    for row in 0..height {
        let row_start = unsafe { framebuffer.addr().add(row * pitch).cast::<u32>() };

        for column in 0..width {
            unsafe {
                write_volatile(row_start.add(column), 0x0016_A1E8);
            }
        }
    }
}

fn halt() -> ! {
    loop {
        unsafe {
            asm!("hlt", options(nomem, nostack));
        }
    }
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    halt()
}
