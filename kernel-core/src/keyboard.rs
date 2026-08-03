use crate::{
    arch::take_keyboard_scancode,
    drivers::{Driver, DriverError},
};
use core::arch::asm;
pub struct Ps2KeyboardDriver {
    initialized: bool,
}
impl Ps2KeyboardDriver {
    pub const fn new() -> Self {
        Self { initialized: false }
    }
    pub fn read_scancode(&self) -> Option<u8> {
        if !self.initialized {
            return None;
        }
        take_keyboard_scancode().or_else(|| {
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
            Some(scancode)
        })
    }
}
impl Driver for Ps2KeyboardDriver {
    fn name(&self) -> &'static str {
        "ps2-keyboard"
    }
    fn initialize(&mut self) -> Result<(), DriverError> {
        self.initialized = true;
        Ok(())
    }
}
