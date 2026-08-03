use core::{arch::asm, ptr::write_volatile};

use crate::{
    arch::{Architecture, X86_64, take_keyboard_scancode},
    memory::{BitmapFrameAllocator, PhysicalFrameAllocator},
    serial::{Com1, SerialOutput},
};

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

#[derive(Clone, Copy)]
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
    X86_64::initialize();

    Com1.write(b"open-kernel: ");
    Com1.write(info.bootloader.as_bytes());
    Com1.write(b" entry reached\r\n");
    let memory = BitmapFrameAllocator::stats();
    Com1.write(b"open-kernel: physical frames free ");
    Com1.write_usize(memory.free_frames);
    Com1.write(b" of ");
    Com1.write_usize(memory.tracked_frames);
    Com1.write(b"\r\n");

    match info.display {
        Display::None => {}
        Display::VgaText => paint_vga(info.bootloader.as_bytes(), info.status),
        Display::Framebuffer(framebuffer) => {
            Com1.write(b"open-kernel: framebuffer ");
            Com1.write_usize(framebuffer.width);
            Com1.write(b"x");
            Com1.write_usize(framebuffer.height);
            Com1.write(b"\r\n");
            run_framebuffer_console(framebuffer, info.bootloader.as_bytes(), info.status)
        }
    }

    X86_64::halt()
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

fn run_framebuffer_console(framebuffer: Framebuffer, bootloader: &[u8], status: BootStatus) -> ! {
    let mut input = [0_u8; 64];
    let mut input_length = 0;
    let mut response = [0_u8; 80];
    let mut response_length = copy_text(&mut response, b"TYPE HELP");

    render_framebuffer_console(
        &framebuffer,
        bootloader,
        status,
        &input[..input_length],
        &response[..response_length],
    );

    loop {
        let mut redraw = false;

        match read_key() {
            Some(b'\n') => {
                response_length = run_console_command(
                    &input[..input_length],
                    &mut response,
                    bootloader,
                    framebuffer.width,
                    framebuffer.height,
                );
                input_length = 0;
                redraw = true;
            }
            Some(8) if input_length > 0 => {
                input_length -= 1;
                redraw = true;
            }
            Some(character) if input_length < input.len() => {
                input[input_length] = character;
                input_length += 1;
                redraw = true;
            }
            _ => {}
        }

        if redraw {
            render_framebuffer_console(
                &framebuffer,
                bootloader,
                status,
                &input[..input_length],
                &response[..response_length],
            );
        }
    }
}

fn render_framebuffer_console(
    framebuffer: &Framebuffer,
    bootloader: &[u8],
    status: BootStatus,
    input: &[u8],
    response: &[u8],
) {
    if framebuffer.bits_per_pixel != 32 {
        return;
    }

    let background = match status {
        BootStatus::Ready => 0x0016_2D3D,
        BootStatus::InvalidBootInfo => 0x0040_1818,
    };

    for row in 0..framebuffer.height {
        let row_start = unsafe {
            framebuffer
                .address
                .add(row * framebuffer.pitch)
                .cast::<u32>()
        };

        for column in 0..framebuffer.width {
            unsafe {
                write_volatile(row_start.add(column), background);
            }
        }
    }

    draw_framebuffer_text(framebuffer, b"OPEN KERNEL", 32, 32, 3, 0x00FF_FFFF);
    draw_framebuffer_text(framebuffer, bootloader, 32, 64, 2, 0x00B8_E8FF);

    let status_text = match status {
        BootStatus::Ready => b"READY".as_slice(),
        BootStatus::InvalidBootInfo => b"INVALID BOOT INFO".as_slice(),
    };
    draw_framebuffer_text(framebuffer, status_text, 32, 96, 2, 0x00FF_FFFF);
    draw_framebuffer_text(framebuffer, response, 32, 144, 2, 0x00FF_FFFF);
    draw_framebuffer_text(framebuffer, b"> ", 32, 176, 2, 0x00B8_E8FF);
    draw_framebuffer_text(framebuffer, input, 56, 176, 2, 0x00FF_FFFF);
}

fn run_console_command(
    input: &[u8],
    response: &mut [u8; 80],
    bootloader: &[u8],
    width: usize,
    height: usize,
) -> usize {
    match input {
        b"" => 0,
        b"help" => copy_text(response, b"HELP CLEAR RESOLUTION BOOTLOADER HALT"),
        b"clear" => 0,
        b"resolution" => {
            let mut length = copy_text(response, b"RESOLUTION ");
            length = append_usize(response, length, width);
            response[length] = b'X';
            length += 1;
            append_usize(response, length, height)
        }
        b"bootloader" => {
            let mut length = copy_text(response, b"BOOTLOADER ");
            for byte in bootloader.iter().copied().take(response.len() - length) {
                response[length] = byte;
                length += 1;
            }
            length
        }
        b"halt" => X86_64::halt(),
        _ => copy_text(response, b"UNKNOWN COMMAND"),
    }
}

fn copy_text(target: &mut [u8], source: &[u8]) -> usize {
    let length = source.len().min(target.len());
    target[..length].copy_from_slice(&source[..length]);
    length
}

fn append_usize(target: &mut [u8], start: usize, mut value: usize) -> usize {
    let mut digits = [0_u8; 20];
    let mut length = 0;

    if value == 0 {
        if start < target.len() {
            target[start] = b'0';
            return start + 1;
        }
        return start;
    }

    while value != 0 && length < digits.len() {
        digits[length] = b'0' + (value % 10) as u8;
        length += 1;
        value /= 10;
    }

    let mut output = start;
    while length != 0 && output < target.len() {
        length -= 1;
        target[output] = digits[length];
        output += 1;
    }
    output
}

fn read_key() -> Option<u8> {
    let scancode = match take_keyboard_scancode() {
        Some(scancode) => scancode,
        None => {
            let status: u8;
            unsafe {
                asm!("in al, dx", in("dx") 0x64_u16, out("al") status, options(nomem, nostack));
            }
            if status & 1 == 0 {
                return None;
            }
            let scancode: u8;
            unsafe {
                asm!("in al, dx", in("dx") 0x60_u16, out("al") scancode, options(nomem, nostack));
            }
            scancode
        }
    };
    if scancode & 0x80 != 0 {
        return None;
    }

    match scancode {
        0x02..=0x0B => Some(b'1' + scancode - 0x02),
        0x0E => Some(8),
        0x10 => Some(b'q'),
        0x11 => Some(b'w'),
        0x12 => Some(b'e'),
        0x13 => Some(b'r'),
        0x14 => Some(b't'),
        0x15 => Some(b'y'),
        0x16 => Some(b'u'),
        0x17 => Some(b'i'),
        0x18 => Some(b'o'),
        0x19 => Some(b'p'),
        0x1C => Some(b'\n'),
        0x1E => Some(b'a'),
        0x1F => Some(b's'),
        0x20 => Some(b'd'),
        0x21 => Some(b'f'),
        0x22 => Some(b'g'),
        0x23 => Some(b'h'),
        0x24 => Some(b'j'),
        0x25 => Some(b'k'),
        0x26 => Some(b'l'),
        0x2C => Some(b'z'),
        0x2D => Some(b'x'),
        0x2E => Some(b'c'),
        0x2F => Some(b'v'),
        0x30 => Some(b'b'),
        0x31 => Some(b'n'),
        0x32 => Some(b'm'),
        0x39 => Some(b' '),
        _ => None,
    }
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
        b'0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        b'1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        b'2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        b'3' => [0x1E, 0x01, 0x01, 0x0E, 0x01, 0x01, 0x1E],
        b'4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        b'5' => [0x1F, 0x10, 0x10, 0x1E, 0x01, 0x01, 0x1E],
        b'6' => [0x0E, 0x10, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        b'7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        b'8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        b'9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x01, 0x0E],
        b'>' => [0x10, 0x08, 0x04, 0x02, 0x04, 0x08, 0x10],
        b' ' => [0; 7],
        _ => [0x1F, 0x01, 0x02, 0x04, 0x04, 0x00, 0x04],
    }
}
