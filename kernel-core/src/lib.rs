#![no_std]

use core::{
    arch::{asm, global_asm},
    panic::PanicInfo,
    ptr::write_volatile,
};

const VGA_TEXT_BUFFER: *mut u16 = 0xB8000 as *mut u16;
const VGA_COLOR: u16 = 0x0F00;
const GDT_KERNEL_CODE: u64 = 0x08;
const GDT_KERNEL_DATA: u16 = 0x10;
pub const PAGE_SIZE: u64 = 4096;
const MAX_PHYSICAL_ADDRESS: u64 = 4 * 1024 * 1024 * 1024;
const MAX_PHYSICAL_FRAMES: usize = (MAX_PHYSICAL_ADDRESS / PAGE_SIZE) as usize;
const FRAME_BITMAP_BYTES: usize = MAX_PHYSICAL_FRAMES / 8;

static GDT: [u64; 3] = [0, 0x00AF_9B00_0000_FFFF, 0x00AF_9300_0000_FFFF];

#[repr(C, packed)]
struct DescriptorTablePointer {
    limit: u16,
    base: u64,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    attributes: u8,
    offset_middle: u16,
    offset_high: u32,
    reserved: u32,
}

impl IdtEntry {
    const MISSING: Self = Self {
        offset_low: 0,
        selector: 0,
        ist: 0,
        attributes: 0,
        offset_middle: 0,
        offset_high: 0,
        reserved: 0,
    };

    fn set_handler(&mut self, handler: usize) {
        self.offset_low = handler as u16;
        self.selector = GDT_KERNEL_CODE as u16;
        self.ist = 0;
        self.attributes = 0x8E;
        self.offset_middle = (handler >> 16) as u16;
        self.offset_high = (handler >> 32) as u32;
        self.reserved = 0;
    }
}

static mut IDT: [IdtEntry; 256] = [IdtEntry::MISSING; 256];
static mut FRAME_BITMAP: [u8; FRAME_BITMAP_BYTES] = [0; FRAME_BITMAP_BYTES];
static mut MEMORY_STATS: PhysicalMemoryStats = PhysicalMemoryStats::EMPTY;

#[derive(Clone, Copy)]
pub enum MemoryRegionKind {
    Usable,
    Reserved,
}

#[derive(Clone, Copy)]
pub struct MemoryRegion {
    pub base: u64,
    pub length: u64,
    pub kind: MemoryRegionKind,
}

impl MemoryRegion {
    pub const fn new(base: u64, length: u64, kind: MemoryRegionKind) -> Self {
        Self { base, length, kind }
    }
}

#[derive(Clone, Copy)]
pub struct PhysicalMemoryRange {
    pub base: u64,
    pub length: u64,
}

impl PhysicalMemoryRange {
    pub const fn new(base: u64, length: u64) -> Self {
        Self { base, length }
    }
}

#[derive(Clone, Copy)]
pub struct PhysicalMemoryStats {
    pub free_frames: usize,
    pub tracked_frames: usize,
}

impl PhysicalMemoryStats {
    const EMPTY: Self = Self {
        free_frames: 0,
        tracked_frames: MAX_PHYSICAL_FRAMES,
    };
}

unsafe extern "C" {
    fn x86_exception_stub();
}

global_asm!(
    r#"
.section .text
.global x86_exception_stub
.type x86_exception_stub, @function
x86_exception_stub:
    cli
    call exception_halt
1:
    hlt
    jmp 1b
"#,
    options(att_syntax)
);

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
    initialize_architecture();

    serial_write(b"open-kernel: ");
    serial_write(info.bootloader.as_bytes());
    serial_write(b" entry reached\r\n");
    let memory = physical_memory_stats();
    serial_write(b"open-kernel: physical frames free ");
    serial_write_usize(memory.free_frames);
    serial_write(b" of ");
    serial_write_usize(memory.tracked_frames);
    serial_write(b"\r\n");

    match info.display {
        Display::None => {}
        Display::VgaText => paint_vga(info.bootloader.as_bytes(), info.status),
        Display::Framebuffer(framebuffer) => {
            serial_write(b"open-kernel: framebuffer ");
            serial_write_usize(framebuffer.width);
            serial_write(b"x");
            serial_write_usize(framebuffer.height);
            serial_write(b"\r\n");
            run_framebuffer_console(framebuffer, info.bootloader.as_bytes(), info.status)
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

pub fn panic(_: &PanicInfo) -> ! {
    serial_write(b"open-kernel: panic\r\n");
    halt()
}

pub fn initialize_physical_memory(
    regions: impl IntoIterator<Item = MemoryRegion>,
    reserved_ranges: impl IntoIterator<Item = PhysicalMemoryRange>,
) -> PhysicalMemoryStats {
    let bitmap = &raw mut FRAME_BITMAP;
    unsafe {
        for byte in &mut *bitmap {
            write_volatile(byte, u8::MAX);
        }
    }

    for region in regions {
        if matches!(region.kind, MemoryRegionKind::Usable) {
            mark_range(region.base, region.length, false);
        }
    }

    mark_range(0, 1024 * 1024, true);
    for range in reserved_ranges {
        mark_range(range.base, range.length, true);
    }

    let mut free_frames = 0;
    for frame in 0..MAX_PHYSICAL_FRAMES {
        if !frame_is_reserved(frame) {
            free_frames += 1;
        }
    }

    let stats = PhysicalMemoryStats {
        free_frames,
        tracked_frames: MAX_PHYSICAL_FRAMES,
    };
    unsafe {
        core::ptr::write_volatile(&raw mut MEMORY_STATS, stats);
    }
    stats
}

pub fn physical_memory_stats() -> PhysicalMemoryStats {
    unsafe { core::ptr::read_volatile(&raw const MEMORY_STATS) }
}

pub fn allocate_physical_frame() -> Option<u64> {
    for frame in 0..MAX_PHYSICAL_FRAMES {
        if !frame_is_reserved(frame) {
            set_frame_reserved(frame, true);
            unsafe {
                let stats = &raw mut MEMORY_STATS;
                (*stats).free_frames -= 1;
            }
            return Some(frame as u64 * PAGE_SIZE);
        }
    }
    None
}

pub fn free_physical_frame(address: u64) {
    if address % PAGE_SIZE != 0 || address >= MAX_PHYSICAL_ADDRESS {
        return;
    }

    let frame = (address / PAGE_SIZE) as usize;
    if frame_is_reserved(frame) {
        set_frame_reserved(frame, false);
        unsafe {
            let stats = &raw mut MEMORY_STATS;
            (*stats).free_frames += 1;
        }
    }
}

fn mark_range(base: u64, length: u64, reserved: bool) {
    let start = base.saturating_add(PAGE_SIZE - 1) / PAGE_SIZE;
    let end = base.saturating_add(length).min(MAX_PHYSICAL_ADDRESS) / PAGE_SIZE;

    for frame in start.min(MAX_PHYSICAL_FRAMES as u64)..end {
        set_frame_reserved(frame as usize, reserved);
    }
}

fn frame_is_reserved(frame: usize) -> bool {
    let bitmap = &raw const FRAME_BITMAP;
    let byte = unsafe { core::ptr::read_volatile((*bitmap).as_ptr().add(frame / 8)) };
    byte & (1 << (frame % 8)) != 0
}

fn set_frame_reserved(frame: usize, reserved: bool) {
    let bitmap = &raw mut FRAME_BITMAP;
    let byte = unsafe { (*bitmap).as_mut_ptr().add(frame / 8) };
    let mask = 1 << (frame % 8);
    let value = unsafe { core::ptr::read_volatile(byte) };
    unsafe {
        write_volatile(
            byte,
            if reserved {
                value | mask
            } else {
                value & !mask
            },
        );
    }
}

#[unsafe(no_mangle)]
extern "C" fn exception_halt() -> ! {
    serial_write(b"open-kernel: CPU exception\r\n");
    halt()
}

fn initialize_architecture() {
    unsafe {
        asm!("cli", options(nomem, nostack));
    }

    let gdt = DescriptorTablePointer {
        limit: (core::mem::size_of_val(&GDT) - 1) as u16,
        base: GDT.as_ptr() as u64,
    };
    load_gdt(&gdt);

    let idt = &raw mut IDT;
    unsafe {
        for entry in (&mut *idt).iter_mut().take(32) {
            entry.set_handler(x86_exception_stub as *const () as usize);
        }
    }
    let idt = DescriptorTablePointer {
        limit: (core::mem::size_of::<[IdtEntry; 256]>() - 1) as u16,
        base: idt as *const IdtEntry as u64,
    };
    unsafe {
        asm!("lidt [{}]", in(reg) &idt, options(readonly, nostack));
    }

    remap_and_mask_pic();
    serial_write(b"open-kernel: x86_64 interrupts initialized\r\n");
}

fn load_gdt(gdt: &DescriptorTablePointer) {
    unsafe {
        asm!(
            "lgdt [{gdt}]",
            "push {code}",
            "lea {target}, [rip + 2f]",
            "push {target}",
            "retfq",
            "2:",
            "mov ax, cx",
            "mov ds, ax",
            "mov es, ax",
            "mov ss, ax",
            "mov fs, ax",
            "mov gs, ax",
            gdt = in(reg) gdt,
            code = const GDT_KERNEL_CODE,
            in("cx") GDT_KERNEL_DATA,
            target = out(reg) _,
            out("ax") _,
        );
    }
}

fn remap_and_mask_pic() {
    unsafe {
        outb(0x20, 0x11);
        outb(0xA0, 0x11);
        outb(0x21, 0x20);
        outb(0xA1, 0x28);
        outb(0x21, 0x04);
        outb(0xA1, 0x02);
        outb(0x21, 0x01);
        outb(0xA1, 0x01);
        outb(0x21, 0xFF);
        outb(0xA1, 0xFF);
    }
}

unsafe fn outb(port: u16, value: u8) {
    unsafe {
        asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack));
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
        b"halt" => halt(),
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
