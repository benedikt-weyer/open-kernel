use core::arch::asm;

static mut X: i32 = 64;
static mut Y: i32 = 64;
static mut PACKET: [u8; 3] = [0; 3];
static mut PACKET_INDEX: usize = 0;

pub fn initialize() {
    unsafe {
        outb(0x64, 0xA8);
        mouse_command(0xF6);
        mouse_command(0xF4);
    }
}
pub fn poll() -> bool {
    let status = unsafe { inb(0x64) };
    if status & 0x21 != 0x21 {
        return false;
    }
    let byte = unsafe { inb(0x60) };
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
        X = X.saturating_add(PACKET[1] as i8 as i32).max(0);
        Y = Y.saturating_sub(PACKET[2] as i8 as i32).max(0);
    }
    true
}
pub fn position() -> (usize, usize) {
    unsafe { (X as usize, Y as usize) }
}
unsafe fn mouse_command(command: u8) {
    unsafe { outb(0x64, 0xD4); outb(0x60, command); }
}
unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    unsafe { asm!("in al, dx", in("dx") port, out("al") value, options(nomem, nostack)); }
    value
}
unsafe fn outb(port: u16, value: u8) {
    unsafe { asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack)); }
}
