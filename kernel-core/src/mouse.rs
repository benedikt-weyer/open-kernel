use core::arch::asm;

static mut X: i32 = 64;
static mut Y: i32 = 64;
static mut PACKET: [u8; 3] = [0; 3];
static mut PACKET_INDEX: usize = 0;

pub fn initialize() {
    unsafe {
        if !wait_for_input_buffer_empty() {
            return;
        }
        outb(0x64, 0xA8);
        if !mouse_command(0xF6) || !mouse_command(0xF4) {
            return;
        }
    }
}

pub fn poll() -> bool {
    let mut moved = false;
    for _ in 0..16 {
        let status = unsafe { inb(0x64) };
        if status & 0x21 != 0x21 {
            break;
        }
        moved |= handle_byte(unsafe { inb(0x60) });
    }
    moved
}

pub fn position() -> (usize, usize) {
    unsafe { (X as usize, Y as usize) }
}

fn handle_byte(byte: u8) -> bool {
    unsafe {
        if PACKET_INDEX == 0 && byte & 0x08 == 0 {
            return false;
        }
        PACKET[PACKET_INDEX] = byte;
        PACKET_INDEX += 1;
        if PACKET_INDEX != 3 {
            return false;
        }
        PACKET_INDEX = 0;

        if PACKET[0] & 0xC0 != 0 {
            return false;
        }

        let delta_x = PACKET[1] as i8 as i32;
        let delta_y = PACKET[2] as i8 as i32;
        X = X.saturating_add(delta_x).max(0);
        Y = Y.saturating_sub(delta_y).max(0);
        delta_x != 0 || delta_y != 0
    }
}

unsafe fn mouse_command(command: u8) -> bool {
    unsafe {
        if !wait_for_input_buffer_empty() {
            return false;
        }
        outb(0x64, 0xD4);
        if !wait_for_input_buffer_empty() {
            return false;
        }
        outb(0x60, command);
        matches!(read_data(), Some(0xFA))
    }
}

unsafe fn wait_for_input_buffer_empty() -> bool {
    for _ in 0..100_000 {
        if unsafe { inb(0x64) } & 0x02 == 0 {
            return true;
        }
    }
    false
}

unsafe fn read_data() -> Option<u8> {
    for _ in 0..100_000 {
        if unsafe { inb(0x64) } & 0x01 != 0 {
            return Some(unsafe { inb(0x60) });
        }
    }
    None
}

unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    unsafe {
        asm!("in al, dx", in("dx") port, out("al") value, options(nomem, nostack));
    }
    value
}
unsafe fn outb(port: u16, value: u8) {
    unsafe {
        asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack));
    }
}
