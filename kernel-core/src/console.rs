use core::ptr::write_volatile;

use crate::{
    arch::{Architecture, X86_64},
    memory::{BitmapFrameAllocator, PhysicalFrameAllocator},
    serial::{Com1, SerialOutput},
};

const VGA_TEXT_BUFFER: *mut u16 = 0xB8000 as *mut u16;
const VGA_COLOR: u16 = 0x0F00;
const USER_CONSOLE_BACKGROUND: u32 = 0x0016_2D3D;
const USER_CONSOLE_FOREGROUND: u32 = 0x00FF_FFFF;
const USER_CONSOLE_MARGIN: usize = 32;
const USER_CONSOLE_SCALE: usize = 2;
const USER_CONSOLE_LINE_HEIGHT: usize = 16;
const USER_CONSOLE_CHARACTER_WIDTH: usize = 12;

/// Number of switchable virtual terminals. Each keeps its own off-screen
/// framebuffer, cursor, and pending keypress; only the active one is ever
/// blitted onto the real hardware framebuffer.
pub const TTY_COUNT: usize = 3;
const MAX_TTY_WIDTH: usize = 1280;
const MAX_TTY_HEIGHT: usize = 800;
const TTY_BUFFER_BYTES: usize = MAX_TTY_WIDTH * MAX_TTY_HEIGHT * 4;

/// Scancodes for the modifier and function keys used by the Ctrl+Alt+Fn
/// switch shortcut.
const SCANCODE_ALT_MAKE: u8 = 0x38;
const SCANCODE_ALT_BREAK: u8 = 0xB8;
const SCANCODE_F1: u8 = 0x3B;

struct TtyState {
    cursor_x: usize,
    cursor_y: usize,
    cursor_visible: bool,
    blink_ticks: u8,
    pending_key: u8,
}
impl TtyState {
    const EMPTY: Self = Self {
        cursor_x: USER_CONSOLE_MARGIN,
        cursor_y: USER_CONSOLE_MARGIN,
        cursor_visible: false,
        blink_ticks: 0,
        pending_key: 0,
    };
}

struct TtyStorage([[u8; TTY_BUFFER_BYTES]; TTY_COUNT]);

static mut TTY_BUFFERS: TtyStorage = TtyStorage([const { [0; TTY_BUFFER_BYTES] }; TTY_COUNT]);
static mut TTY_STATE: [TtyState; TTY_COUNT] = [TtyState::EMPTY; TTY_COUNT];
static mut ACTIVE_TTY: usize = 0;
static mut ALT_HELD: bool = false;
static mut PRIMARY_FRAMEBUFFER: Option<Framebuffer> = None;
/// Off-screen per-tty buffers only fit resolutions up to
/// `MAX_TTY_WIDTH`x`MAX_TTY_HEIGHT`; above that we fall back to tty 0
/// drawing straight onto the real framebuffer, and other ttys are unusable.
static mut MULTI_TTY_ENABLED: bool = false;

pub enum Display {
    None,
    VgaText,
    Framebuffer(Framebuffer),
}

#[derive(Clone, Copy)]
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
    match crate::mount_initramfs() {
        Ok(()) => Com1.write(b"open-kernel: initramfs mounted at /\r\n"),
        Err(crate::VfsError::AlreadyMounted) => {}
        Err(_) => Com1.write(b"open-kernel: initramfs mount failed\r\n"),
    }
    X86_64::initialize();
    match crate::initialize_sata() {
        Ok(()) => Com1.write(b"open-kernel: SATA initialized\r\n"),
        Err(crate::SataError::NotFound) => Com1.write(b"open-kernel: AHCI not found\r\n"),
        Err(crate::SataError::NoDevice) => Com1.write(b"open-kernel: SATA port not found\r\n"),
        Err(crate::SataError::Timeout) => Com1.write(b"open-kernel: SATA reset timed out\r\n"),
        Err(_) => Com1.write(b"open-kernel: SATA unavailable\r\n"),
    }
    crate::initialize_random();
    // Nothing drains PS/2 aux data, so the mouse is never initialized here:
    // enabling it without a poller would wedge the shared PS/2 output
    // register with an unread aux byte on first motion, blocking all
    // subsequent keyboard scancodes from reaching IRQ1.

    Com1.write(b"open-kernel: ");
    Com1.write(info.bootloader.as_bytes());
    Com1.write(b" entry reached\r\n");
    let memory = BitmapFrameAllocator::stats();
    Com1.write(b"open-kernel: physical frames free ");
    Com1.write_usize(memory.free_frames);
    Com1.write(b" of ");
    Com1.write_usize(memory.tracked_frames);
    Com1.write(b"\r\n");

    if let Display::Framebuffer(framebuffer) = info.display {
        Com1.write(b"open-kernel: framebuffer ");
        Com1.write_usize(framebuffer.width);
        Com1.write(b"x");
        Com1.write_usize(framebuffer.height);
        Com1.write(b"\r\n");
        enable_user_console(framebuffer);
    } else if matches!(info.display, Display::VgaText) {
        paint_vga(info.bootloader.as_bytes(), info.status);
    }
    if matches!(info.status, BootStatus::Ready) {
        if let Err(error) = crate::user::run_demo() {
            report_startup_failure(error);
        }
    }

    X86_64::halt()
}

pub(crate) fn report_startup_failure(error: crate::ElfError) {
    Com1.write(b"open-kernel: could not start user process: ");
    Com1.write(elf_error_message(error));
    Com1.write(b"\r\n");
}

fn elf_error_message(error: crate::ElfError) -> &'static [u8] {
    match error {
        crate::ElfError::InvalidHeader => b"missing /init or invalid ELF header",
        crate::ElfError::UnsupportedExecutable => b"unsupported executable format",
        crate::ElfError::InvalidProgramHeader => b"invalid ELF program header",
        crate::ElfError::InvalidSegment => b"invalid ELF load segment",
        crate::ElfError::NoExecutableSegment => b"no executable ELF segment",
        crate::ElfError::Paging(error) => match error {
            crate::PagingError::FrameAllocationFailed => b"out of physical frames",
            crate::PagingError::HugePageConflict => b"page-table huge-page conflict",
            crate::PagingError::AlreadyMapped => b"user page already mapped",
            crate::PagingError::InvalidUserAddress => b"invalid user virtual address",
        },
    }
}

/// Renders `text` to `tty`'s virtual terminal, blitting to the real screen
/// only if `tty` is currently the active one.
pub fn user_console_write(tty: usize, text: &[u8]) {
    let Some(target) = tty_target(tty) else {
        return;
    };
    hide_cursor(tty, &target);
    for byte in text {
        match *byte {
            b'\r' => {}
            b'\n' => newline(tty, &target),
            byte => {
                if tty_state(tty).cursor_x + USER_CONSOLE_CHARACTER_WIDTH > target.width {
                    newline(tty, &target);
                }
                let state = tty_state(tty);
                let (x, y) = (state.cursor_x, state.cursor_y);
                draw_framebuffer_text(
                    &target,
                    core::slice::from_ref(&byte),
                    x,
                    y,
                    USER_CONSOLE_SCALE,
                    USER_CONSOLE_FOREGROUND,
                );
                tty_state(tty).cursor_x = x + USER_CONSOLE_CHARACTER_WIDTH;
            }
        }
    }
    show_cursor(tty, &target);
    blit_if_active(tty, &target);
}

pub fn user_console_clear(tty: usize) {
    clear_tty(tty);
}

pub fn user_console_tick() {
    for tty in 0..TTY_COUNT {
        let Some(target) = tty_target(tty) else {
            continue;
        };
        let state = tty_state(tty);
        state.blink_ticks = state.blink_ticks.wrapping_add(1);
        if state.blink_ticks < 50 {
            continue;
        }
        state.blink_ticks = 0;
        if state.cursor_visible {
            hide_cursor(tty, &target);
        } else {
            show_cursor(tty, &target);
        }
        blit_if_active(tty, &target);
    }
}

pub fn user_console_backspace(tty: usize) {
    let Some(target) = tty_target(tty) else {
        return;
    };
    hide_cursor(tty, &target);
    let x = tty_state(tty).cursor_x;
    if x <= USER_CONSOLE_MARGIN {
        show_cursor(tty, &target);
        return;
    }
    let previous_x = x - USER_CONSOLE_CHARACTER_WIDTH;
    let y = tty_state(tty).cursor_y;
    for pixel_y in 0..USER_CONSOLE_LINE_HEIGHT {
        for pixel_x in 0..USER_CONSOLE_CHARACTER_WIDTH {
            put_framebuffer_pixel(&target, previous_x + pixel_x, y + pixel_y, USER_CONSOLE_BACKGROUND);
        }
    }
    tty_state(tty).cursor_x = previous_x;
    show_cursor(tty, &target);
    blit_if_active(tty, &target);
}

/// Reads one pending keypress for `tty`, if any. Only the active tty ever
/// receives new keys from the keyboard IRQ, so background ttys read as
/// empty until switched to.
pub fn poll_user_key(tty: usize) -> Option<u8> {
    if tty >= TTY_COUNT {
        return None;
    }
    let byte = {
        let state = tty_state(tty);
        let value = state.pending_key;
        state.pending_key = 0;
        value
    };
    if byte == 0 {
        return None;
    }
    decode_scancode(byte)
}

/// Called from the keyboard IRQ handler for every scancode byte. Tracks
/// Alt modifier state, switches the active tty on Alt+Fn (Ctrl+Alt+Fn also
/// works, since Ctrl makes no difference here), and otherwise queues the
/// byte for whichever tty is currently active.
pub fn handle_scancode(byte: u8) {
    match byte {
        SCANCODE_ALT_MAKE => {
            unsafe { ALT_HELD = true };
            return;
        }
        SCANCODE_ALT_BREAK => {
            unsafe { ALT_HELD = false };
            return;
        }
        _ => {}
    }
    if unsafe { ALT_HELD } && (SCANCODE_F1..SCANCODE_F1 + TTY_COUNT as u8).contains(&byte) {
        switch_tty((byte - SCANCODE_F1) as usize);
        return;
    }
    // Only queue make codes; break codes carry no character and would just
    // needlessly clobber a key the target tty hasn't consumed yet.
    if byte & 0x80 == 0 {
        let active = unsafe { ACTIVE_TTY };
        tty_state(active).pending_key = byte;
    }
}

fn switch_tty(tty: usize) {
    if tty >= TTY_COUNT {
        return;
    }
    unsafe { ACTIVE_TTY = tty };
    if let Some(target) = tty_target(tty) {
        blit_to_primary(&target);
    }
}

fn enable_user_console(framebuffer: Framebuffer) {
    unsafe {
        write_volatile(&raw mut PRIMARY_FRAMEBUFFER, Some(framebuffer));
        MULTI_TTY_ENABLED = framebuffer.bits_per_pixel == 32
            && framebuffer.width <= MAX_TTY_WIDTH
            && framebuffer.height <= MAX_TTY_HEIGHT;
    }
    for tty in 0..TTY_COUNT {
        clear_tty(tty);
    }
}

fn clear_tty(tty: usize) {
    let Some(target) = tty_target(tty) else {
        return;
    };
    for row in 0..target.height {
        for column in 0..target.width {
            put_framebuffer_pixel(&target, column, row, USER_CONSOLE_BACKGROUND);
        }
    }
    let state = tty_state(tty);
    state.cursor_x = USER_CONSOLE_MARGIN;
    state.cursor_y = USER_CONSOLE_MARGIN;
    state.cursor_visible = false;
    show_cursor(tty, &target);
    blit_if_active(tty, &target);
}

fn newline(tty: usize, target: &Framebuffer) {
    let next_y = tty_state(tty).cursor_y + USER_CONSOLE_LINE_HEIGHT;
    if next_y + USER_CONSOLE_LINE_HEIGHT > target.height {
        clear_tty(tty);
        return;
    }
    let state = tty_state(tty);
    state.cursor_x = USER_CONSOLE_MARGIN;
    state.cursor_y = next_y;
}

fn show_cursor(tty: usize, target: &Framebuffer) {
    let state = tty_state(tty);
    let (x, y) = (state.cursor_x, state.cursor_y);
    for pixel_y in USER_CONSOLE_LINE_HEIGHT - 2..USER_CONSOLE_LINE_HEIGHT {
        for pixel_x in 0..USER_CONSOLE_CHARACTER_WIDTH {
            put_framebuffer_pixel(target, x + pixel_x, y + pixel_y, USER_CONSOLE_FOREGROUND);
        }
    }
    tty_state(tty).cursor_visible = true;
}

fn hide_cursor(tty: usize, target: &Framebuffer) {
    let state = tty_state(tty);
    if !state.cursor_visible {
        return;
    }
    let (x, y) = (state.cursor_x, state.cursor_y);
    for pixel_y in USER_CONSOLE_LINE_HEIGHT - 2..USER_CONSOLE_LINE_HEIGHT {
        for pixel_x in 0..USER_CONSOLE_CHARACTER_WIDTH {
            put_framebuffer_pixel(target, x + pixel_x, y + pixel_y, USER_CONSOLE_BACKGROUND);
        }
    }
    tty_state(tty).cursor_visible = false;
}

/// The framebuffer a tty should draw into: its own off-screen buffer when
/// multi-tty is enabled, or (for tty 0 only) the real framebuffer directly
/// when the resolution was too large to back every tty with its own copy.
fn tty_target(tty: usize) -> Option<Framebuffer> {
    if tty >= TTY_COUNT {
        return None;
    }
    let primary = unsafe { core::ptr::read_volatile(&raw const PRIMARY_FRAMEBUFFER) }?;
    if unsafe { MULTI_TTY_ENABLED } {
        let pitch = primary.width * 4;
        Some(Framebuffer::new(tty_buffer_ptr(tty), primary.width, primary.height, pitch, primary.bits_per_pixel))
    } else if tty == 0 {
        Some(primary)
    } else {
        None
    }
}

fn tty_buffer_ptr(tty: usize) -> *mut u8 {
    unsafe { (&raw mut (*(&raw mut TTY_BUFFERS)).0[tty]).cast::<u8>() }
}

#[allow(clippy::mut_from_ref)]
fn tty_state(tty: usize) -> &'static mut TtyState {
    unsafe { &mut (*(&raw mut TTY_STATE))[tty] }
}

fn blit_if_active(tty: usize, target: &Framebuffer) {
    if !unsafe { MULTI_TTY_ENABLED } || tty != unsafe { ACTIVE_TTY } {
        return;
    }
    blit_to_primary(target);
}

fn blit_to_primary(source: &Framebuffer) {
    let Some(primary) = (unsafe { core::ptr::read_volatile(&raw const PRIMARY_FRAMEBUFFER) }) else {
        return;
    };
    // A manual pixel loop rather than `core::ptr::copy_nonoverlapping`,
    // since this freestanding, `-nostdlib` binary has no `memcpy` to lower
    // a runtime-sized copy into.
    for row in 0..source.height {
        let source_row = unsafe { source.address.add(row * source.pitch).cast::<u32>() };
        let primary_row = unsafe { primary.address.add(row * primary.pitch).cast::<u32>() };
        for column in 0..source.width {
            unsafe {
                write_volatile(primary_row.add(column), core::ptr::read_volatile(source_row.add(column)));
            }
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

fn decode_scancode(scancode: u8) -> Option<u8> {
    if scancode & 0x80 != 0 {
        return None;
    }

    match scancode {
        0x02..=0x0B => Some(b'1' + scancode - 0x02),
        0x0E => Some(8),
        0x0F => Some(b'\t'),
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
    if let Some(pixel) = framebuffer_pixel(framebuffer, x, y) {
        unsafe {
            write_volatile(pixel, color);
        }
    }
}

fn framebuffer_pixel(framebuffer: &Framebuffer, x: usize, y: usize) -> Option<*mut u32> {
    if x >= framebuffer.width || y >= framebuffer.height {
        return None;
    }
    Some(unsafe {
        framebuffer
            .address
            .add(y * framebuffer.pitch + x * core::mem::size_of::<u32>())
            .cast::<u32>()
    })
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
