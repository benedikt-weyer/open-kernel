use core::arch::{asm, global_asm};

use crate::serial::{Com1, SerialOutput};

const KERNEL_CODE: u64 = 0x08;
const KERNEL_DATA: u16 = 0x10;
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
        self.selector = KERNEL_CODE as u16;
        self.ist = 0;
        self.attributes = 0x8E;
        self.offset_middle = (handler >> 16) as u16;
        self.offset_high = (handler >> 32) as u32;
        self.reserved = 0;
    }
}
static mut IDT: [IdtEntry; 256] = [IdtEntry::MISSING; 256];
static mut TIMER_TICKS: u64 = 0;
static mut KEYBOARD_SCANCODE: u8 = 0;

pub trait Architecture {
    fn initialize();
    fn halt() -> !;
}
pub struct X86_64;

unsafe extern "C" {
    fn x86_exception_stub();
    fn x86_timer_irq_stub();
    fn x86_keyboard_irq_stub();
    fn x86_serial_irq_stub();
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
.macro irq_stub name, vector
.global \name
.type \name, @function
\name:
    push %rax
    push %rcx
    push %rdx
    push %rsi
    push %rdi
    push %r8
    push %r9
    push %r10
    push %r11
    mov $\vector, %edi
    call irq_dispatch
    pop %r11
    pop %r10
    pop %r9
    pop %r8
    pop %rdi
    pop %rsi
    pop %rdx
    pop %rcx
    pop %rax
    iretq
.endm
irq_stub x86_timer_irq_stub, 32
irq_stub x86_keyboard_irq_stub, 33
irq_stub x86_serial_irq_stub, 36
"#,
    options(att_syntax)
);

impl Architecture for X86_64 {
    fn initialize() {
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
            (*idt)[32].set_handler(x86_timer_irq_stub as *const () as usize);
            (*idt)[33].set_handler(x86_keyboard_irq_stub as *const () as usize);
            (*idt)[36].set_handler(x86_serial_irq_stub as *const () as usize);
        }
        let idt_pointer = DescriptorTablePointer {
            limit: (core::mem::size_of::<[IdtEntry; 256]>() - 1) as u16,
            base: idt as *const IdtEntry as u64,
        };
        unsafe {
            asm!("lidt [{}]", in(reg) &idt_pointer, options(readonly, nostack));
        }
        initialize_irq_controller();
        unsafe {
            asm!("sti", options(nomem, nostack));
        }
        Com1.write(b"open-kernel: x86_64 interrupts initialized\r\n");
    }
    fn halt() -> ! {
        loop {
            unsafe {
                asm!("hlt", options(nomem, nostack));
            }
        }
    }
}

pub fn timer_ticks() -> u64 {
    unsafe { core::ptr::read_volatile(&raw const TIMER_TICKS) }
}

pub fn take_keyboard_scancode() -> Option<u8> {
    let scancode = unsafe { core::ptr::read_volatile(&raw const KEYBOARD_SCANCODE) };
    if scancode == 0 {
        return None;
    }
    unsafe {
        core::ptr::write_volatile(&raw mut KEYBOARD_SCANCODE, 0);
    }
    Some(scancode)
}

#[unsafe(no_mangle)]
extern "C" fn irq_dispatch(vector: u64) {
    unsafe {
        match vector {
            32 => {
                let ticks = core::ptr::read_volatile(&raw const TIMER_TICKS);
                core::ptr::write_volatile(&raw mut TIMER_TICKS, ticks.wrapping_add(1));
            }
            33 => {
                core::ptr::write_volatile(&raw mut KEYBOARD_SCANCODE, inb(0x60));
            }
            36 => {
                if inb(0x3FD) & 1 != 0 {
                    let _ = inb(0x3F8);
                }
            }
            _ => {}
        }
        if vector >= 40 {
            outb(0xA0, 0x20);
        }
        outb(0x20, 0x20);
    }
}

#[unsafe(no_mangle)]
extern "C" fn exception_halt() -> ! {
    Com1.write(b"open-kernel: CPU exception\r\n");
    X86_64::halt()
}

fn load_gdt(gdt: &DescriptorTablePointer) {
    unsafe {
        asm!(
        "lgdt [{gdt}]", "push {code}", "lea {target}, [rip + 2f]", "push {target}", "retfq", "2:",
        "mov ax, cx", "mov ds, ax", "mov es, ax", "mov ss, ax", "mov fs, ax", "mov gs, ax",
        gdt = in(reg) gdt, code = const KERNEL_CODE, in("cx") KERNEL_DATA, target = out(reg) _, out("ax") _,
            );
        crate::scheduler::request_preemption();
    }
}
fn initialize_irq_controller() {
    unsafe {
        for (port, value) in [
            (0x20_u16, 0x11_u8),
            (0xA0, 0x11),
            (0x21, 0x20),
            (0xA1, 0x28),
            (0x21, 0x04),
            (0xA1, 0x02),
            (0x21, 0x01),
            (0xA1, 0x01),
            (0x21, 0xEC),
            (0xA1, 0xFF),
        ] {
            asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack));
        }
    }
    let divisor = (1_193_182_u32 / 100) as u16;
    unsafe {
        outb(0x43, 0x36);
        outb(0x40, divisor as u8);
        outb(0x40, (divisor >> 8) as u8);
        outb(0x3F9, 0x01);
    }
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
