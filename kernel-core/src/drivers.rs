pub trait Driver {
    fn name(&self) -> &'static str;
    fn initialize(&mut self) -> Result<(), DriverError>;
}

#[derive(Clone, Copy)]
pub enum DriverError {
    Unsupported,
    NotReady,
    BufferTooSmall,
}

pub trait NetworkDriver: Driver {
    fn mac_address(&self) -> [u8; 6];
    fn transmit(&mut self, frame: &[u8]) -> Result<(), DriverError>;
    fn receive(&mut self, buffer: &mut [u8]) -> Result<Option<usize>, DriverError>;
}

pub struct LoopbackNetworkDriver {
    frame: [u8; 1536],
    length: usize,
    initialized: bool,
}
impl LoopbackNetworkDriver {
    pub const fn new() -> Self {
        Self {
            frame: [0; 1536],
            length: 0,
            initialized: false,
        }
    }
}
impl Driver for LoopbackNetworkDriver {
    fn name(&self) -> &'static str {
        "loopback"
    }
    fn initialize(&mut self) -> Result<(), DriverError> {
        self.initialized = true;
        Ok(())
    }
}
impl NetworkDriver for LoopbackNetworkDriver {
    fn mac_address(&self) -> [u8; 6] {
        [0x02, 0, 0, 0, 0, 1]
    }
    fn transmit(&mut self, frame: &[u8]) -> Result<(), DriverError> {
        if !self.initialized {
            return Err(DriverError::NotReady);
        }
        if frame.len() > self.frame.len() {
            return Err(DriverError::BufferTooSmall);
        }
        self.frame[..frame.len()].copy_from_slice(frame);
        self.length = frame.len();
        Ok(())
    }
    fn receive(&mut self, buffer: &mut [u8]) -> Result<Option<usize>, DriverError> {
        if !self.initialized {
            return Err(DriverError::NotReady);
        }
        if self.length == 0 {
            return Ok(None);
        }
        if buffer.len() < self.length {
            return Err(DriverError::BufferTooSmall);
        }
        buffer[..self.length].copy_from_slice(&self.frame[..self.length]);
        let length = self.length;
        self.length = 0;
        Ok(Some(length))
    }
}
