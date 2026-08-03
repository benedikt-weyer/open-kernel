#![no_std]

use core::arch::asm;
use core::ptr::write_volatile;

const VGA_TEXT_BUFFER: *mut u16 = 0xB8000 as *mut u16;
const VGA_COLOR: u16 = 0x0F00;

pub enum Display {
    None,
    VgaText,
    Framebuffer(Framebuffer),
}

pub struct Framebuffer {
    address: *mut u8,
    width: usize,
    height: usize,
    pitch: usize,
    bits_per_pixel: u16,
}

impl Framebuffer {
    pub const fn new(
        address: *mut u8,
        width: usize,
        height: usize,
        pitch: usize,
        bits_per_pixel: u16,
    ) -> Self {
        Self {
            address,
            width,
            height,
            pitch,
            bits_per_pixel,
        }
    }
}

pub enum BootStatus {
    Ready,
    InvalidBootInfo,
}

pub struct BootInfo {
    pub display: Display,
    pub bootloader: &'static str,
    pub status: BootStatus,
}

impl BootInfo {
    pub const fn new(display: Display, bootloader: &'static str, status: BootStatus) -> Self {
        Self {
            display,
            bootloader,
            status,
        }
    }
}

pub fn boot(info: BootInfo) -> ! {
    serial_write(b"open-kernel: ");
    serial_write(info.bootloader.as_bytes());
    serial_write(b" entry reached\r\n");

    match info.display {
        Display::None => {}
        Display::VgaText => paint_vga(info.bootloader.as_bytes(), info.status),
        Display::Framebuffer(framebuffer) => paint_framebuffer(framebuffer, info.status),
    }

    halt()
}

pub fn halt() -> ! {
    loop {
        unsafe {
            asm!("hlt", options(nomem, nostack));
        }
    }
}

fn paint_vga(bootloader: &[u8], status: BootStatus) {
    for index in 0..(80 * 25) {
        unsafe {
            write_volatile(VGA_TEXT_BUFFER.add(index), VGA_COLOR | b' ' as u16);
        }
    }

    write_vga_text(b"open-kernel is running", 0);
    write_vga_text(bootloader, 1);

    if matches!(status, BootStatus::InvalidBootInfo) {
        write_vga_text(b"Invalid boot information", 2);
    }
}

fn write_vga_text(text: &[u8], row: usize) {
    for (column, character) in text.iter().take(80).enumerate() {
        unsafe {
            write_volatile(
                VGA_TEXT_BUFFER.add(row * 80 + column),
                VGA_COLOR | *character as u16,
            );
        }
    }
}

fn paint_framebuffer(framebuffer: Framebuffer, status: BootStatus) {
    if framebuffer.bits_per_pixel != 32 {
        return;
    }

    let color = match status {
        BootStatus::Ready => 0x0016_A1E8,
        BootStatus::InvalidBootInfo => 0x00C0_3C3C,
    };
    let width = framebuffer.width.min(320);
    let height = framebuffer.height.min(80);

    for row in 0..height {
        let row_start = unsafe { framebuffer.address.add(row * framebuffer.pitch).cast::<u32>() };

        for column in 0..width {
            unsafe {
                write_volatile(row_start.add(column), color);
            }
        }
    }
}

fn serial_write(text: &[u8]) {
    for byte in text {
        unsafe {
            asm!("out dx, al", in("dx") 0x3F8_u16, in("al") *byte, options(nomem, nostack));
        }
    }
}
