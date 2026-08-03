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
        Display::Framebuffer(framebuffer) => {
            serial_write(b"open-kernel: framebuffer ");
            serial_write_usize(framebuffer.width);
            serial_write(b"x");
            serial_write_usize(framebuffer.height);
            serial_write(b"\r\n");
            paint_framebuffer(framebuffer, info.bootloader.as_bytes(), info.status)
        }
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

fn paint_framebuffer(framebuffer: Framebuffer, bootloader: &[u8], status: BootStatus) {
    if framebuffer.bits_per_pixel != 32 {
        return;
    }

    let background = match status {
        BootStatus::Ready => 0x0016_2D3D,
        BootStatus::InvalidBootInfo => 0x0040_1818,
    };

    for row in 0..framebuffer.height {
        let row_start = unsafe { framebuffer.address.add(row * framebuffer.pitch).cast::<u32>() };

        for column in 0..framebuffer.width {
            unsafe {
                write_volatile(row_start.add(column), background);
            }
        }
    }

    draw_framebuffer_text(&framebuffer, b"OPEN KERNEL", 32, 32, 3, 0x00FF_FFFF);
    draw_framebuffer_text(&framebuffer, bootloader, 32, 64, 2, 0x00B8_E8FF);

    let status_text = match status {
        BootStatus::Ready => b"READY".as_slice(),
        BootStatus::InvalidBootInfo => b"INVALID BOOT INFO".as_slice(),
    };
    draw_framebuffer_text(&framebuffer, status_text, 32, 96, 2, 0x00FF_FFFF);
}

fn draw_framebuffer_text(
    framebuffer: &Framebuffer,
    text: &[u8],
    x: usize,
    y: usize,
    scale: usize,
    color: u32,
) {
    let mut cursor = x;

    for byte in text {
        let glyph = framebuffer_glyph(byte.to_ascii_uppercase());
        for (glyph_row, bits) in glyph.iter().enumerate() {
            for glyph_column in 0..5 {
                if bits & (1 << (4 - glyph_column)) != 0 {
                    for pixel_y in 0..scale {
                        for pixel_x in 0..scale {
                            put_framebuffer_pixel(
                                framebuffer,
                                cursor + glyph_column * scale + pixel_x,
                                y + glyph_row * scale + pixel_y,
                                color,
                            );
                        }
                    }
                }
            }
        }
        cursor += 6 * scale;
    }
}

fn put_framebuffer_pixel(framebuffer: &Framebuffer, x: usize, y: usize, color: u32) {
    if x >= framebuffer.width || y >= framebuffer.height {
        return;
    }

    let pixel = unsafe {
        framebuffer
            .address
            .add(y * framebuffer.pitch + x * core::mem::size_of::<u32>())
            .cast::<u32>()
    };
    unsafe {
        write_volatile(pixel, color);
    }
}

fn framebuffer_glyph(character: u8) -> [u8; 7] {
    match character {
        b'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        b'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        b'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        b'D' => [0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E],
        b'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        b'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        b'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F],
        b'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        b'I' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x1F],
        b'J' => [0x01, 0x01, 0x01, 0x01, 0x11, 0x11, 0x0E],
        b'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        b'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        b'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        b'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        b'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        b'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        b'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        b'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        b'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        b'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        b'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        b'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        b'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A],
        b'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        b'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        b'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        b' ' => [0; 7],
        _ => [0x1F, 0x01, 0x02, 0x04, 0x04, 0x00, 0x04],
    }
}

fn serial_write(text: &[u8]) {
    for byte in text {
        unsafe {
            asm!("out dx, al", in("dx") 0x3F8_u16, in("al") *byte, options(nomem, nostack));
        }
    }
}

fn serial_write_usize(mut value: usize) {
    let mut digits = [0_u8; 20];
    let mut length = 0;

    if value == 0 {
        serial_write(b"0");
        return;
    }

    while value != 0 {
        digits[length] = b'0' + (value % 10) as u8;
        length += 1;
        value /= 10;
    }

    while length != 0 {
        length -= 1;
        serial_write(&digits[length..=length]);
    }
}
