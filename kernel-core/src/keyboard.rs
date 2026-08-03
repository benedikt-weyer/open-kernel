use crate::{
    arch::take_keyboard_scancode,
    drivers::{Driver, DriverError},
};
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
        take_keyboard_scancode()
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
