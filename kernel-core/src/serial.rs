use core::arch::asm;

pub trait SerialOutput {
    fn write(&self, text: &[u8]);
    fn write_usize(&self, value: usize);
}

pub struct Com1;

impl SerialOutput for Com1 {
    fn write(&self, text: &[u8]) {
        for byte in text {
            unsafe {
                asm!("out dx, al", in("dx") 0x3F8_u16, in("al") *byte, options(nomem, nostack));
            }
        }
    }

    fn write_usize(&self, mut value: usize) {
        let mut digits = [0_u8; 20];
        let mut length = 0;
        if value == 0 {
            self.write(b"0");
            return;
        }
        while value != 0 {
            digits[length] = b'0' + (value % 10) as u8;
            length += 1;
            value /= 10;
        }
        while length != 0 {
            length -= 1;
            self.write(&digits[length..=length]);
        }
    }
}
