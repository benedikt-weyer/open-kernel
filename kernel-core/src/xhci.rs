use core::ptr::{read_volatile, write_volatile};

use crate::drivers::{Driver, DriverError};

const USBCMD: usize = 0x00;
const USBSTS: usize = 0x04;
const USBCMD_RUN: u32 = 1;
const USBSTS_HALTED: u32 = 1;
const USBCMD_RESET: u32 = 1 << 1;

pub struct XhciController {
    mmio: *mut u8,
    operational_offset: usize,
    initialized: bool,
}

impl XhciController {
    pub const unsafe fn new(mmio_base: *mut u8) -> Self {
        Self {
            mmio: mmio_base,
            operational_offset: 0,
            initialized: false,
        }
    }
    fn register(&self, offset: usize) -> *mut u32 {
        unsafe { self.mmio.add(self.operational_offset + offset).cast() }
    }
    fn read(&self, offset: usize) -> u32 {
        unsafe { read_volatile(self.register(offset)) }
    }
    fn write(&self, offset: usize, value: u32) {
        unsafe {
            write_volatile(self.register(offset), value);
        }
    }
    fn wait_for(&self, offset: usize, mask: u32, set: bool) -> bool {
        for _ in 0..1_000_000 {
            if (self.read(offset) & mask != 0) == set {
                return true;
            }
        }
        false
    }
}

impl Driver for XhciController {
    fn name(&self) -> &'static str {
        "xhci"
    }
    fn initialize(&mut self) -> Result<(), DriverError> {
        if self.mmio.is_null() {
            return Err(DriverError::Unsupported);
        }
        let capability_length = unsafe { read_volatile(self.mmio) } as usize;
        if capability_length < 0x20 {
            return Err(DriverError::Unsupported);
        }
        self.operational_offset = capability_length;
        self.write(USBCMD, self.read(USBCMD) & !USBCMD_RUN);
        if !self.wait_for(USBSTS, USBSTS_HALTED, true) {
            return Err(DriverError::NotReady);
        }
        self.write(USBCMD, self.read(USBCMD) | USBCMD_RESET);
        if !self.wait_for(USBCMD, USBCMD_RESET, false) {
            return Err(DriverError::NotReady);
        }
        self.write(USBCMD, self.read(USBCMD) | USBCMD_RUN);
        self.initialized = true;
        Ok(())
    }
}

impl XhciController {
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }
}
