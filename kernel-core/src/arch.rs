use core::arch::{asm, global_asm};

use crate::serial::{Com1, SerialOutput};

const KERNEL_CODE: u64 = 0x08;
const KERNEL_DATA: u16 = 0x10;
const USER_DATA: u64 = 0x1B;
const USER_CODE: u64 = 0x23;
const TSS_SELECTOR: u16 = 0x28;
const IA32_EFER: u32 = 0xC000_0080;
const IA32_STAR: u32 = 0xC000_0081;
const IA32_LSTAR: u32 = 0xC000_0082;
const IA32_FMASK: u32 = 0xC000_0084;
const SYSCALL_STACK_SIZE: usize = 16 * 1024;

static mut GDT: [u64; 7] = [
    0,
    0x00AF_9B00_0000_FFFF,
    0x00CF_9300_0000_FFFF,
    0x00CF_F300_0000_FFFF,
    0x00AF_FB00_0000_FFFF,
    0,
    0,
];

#[repr(C, packed)]
struct TaskStateSegment {
    reserved_0: u32,
    rsp: [u64; 3],
    reserved_1: u64,
    ist: [u64; 7],
    reserved_2: u64,
    reserved_3: u16,
    io_map_base: u16,
}
static mut TSS: TaskStateSegment = TaskStateSegment {
    reserved_0: 0,
    rsp: [0; 3],
    reserved_1: 0,
    ist: [0; 7],
    reserved_2: 0,
    reserved_3: 0,
    io_map_base: core::mem::size_of::<TaskStateSegment>() as u16,
};
#[repr(align(16))]
#[allow(dead_code)]
struct SyscallStack([u8; SYSCALL_STACK_SIZE]);
#[unsafe(no_mangle)]
static mut SYSCALL_STACK: SyscallStack = SyscallStack([0; SYSCALL_STACK_SIZE]);
#[unsafe(no_mangle)]
static mut USER_SYSCALL_STACK_POINTER: u64 = 0;
#[unsafe(no_mangle)]
static mut USER_SYSCALL_RIP: u64 = 0;
#[unsafe(no_mangle)]
static mut USER_SYSCALL_RFLAGS: u64 = 0;
#[unsafe(no_mangle)]
static mut USER_SYSCALL_NUMBER: u64 = 0;
#[unsafe(no_mangle)]
static mut USER_SYSCALL_POINTER: u64 = 0;
#[unsafe(no_mangle)]
static mut USER_SYSCALL_LENGTH: u64 = 0;
#[unsafe(no_mangle)]
static mut USER_SYSCALL_ARGUMENT: u64 = 0;
#[unsafe(no_mangle)]
static mut USER_SYSCALL_CONTEXT: crate::scheduler::UserContext =
    crate::scheduler::UserContext::EMPTY;

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
    fn x86_syscall_stub();
}
global_asm!(
    r#"
.section .text
.global x86_exception_stub
.type x86_exception_stub, @function
x86_exception_stub:
    cli
    mov %rsp, %rdi
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
.global x86_syscall_stub
.type x86_syscall_stub, @function
x86_syscall_stub:
    mov %rsp, USER_SYSCALL_CONTEXT+8(%rip)
    mov %rcx, USER_SYSCALL_CONTEXT(%rip)
    mov %r11, USER_SYSCALL_CONTEXT+16(%rip)
    mov %rax, USER_SYSCALL_CONTEXT+24(%rip)
    mov %rbx, USER_SYSCALL_CONTEXT+32(%rip)
    mov %rcx, USER_SYSCALL_CONTEXT+40(%rip)
    mov %rdx, USER_SYSCALL_CONTEXT+48(%rip)
    mov %rsi, USER_SYSCALL_CONTEXT+56(%rip)
    mov %rdi, USER_SYSCALL_CONTEXT+64(%rip)
    mov %rbp, USER_SYSCALL_CONTEXT+72(%rip)
    mov %r8, USER_SYSCALL_CONTEXT+80(%rip)
    mov %r9, USER_SYSCALL_CONTEXT+88(%rip)
    mov %r10, USER_SYSCALL_CONTEXT+96(%rip)
    mov %r11, USER_SYSCALL_CONTEXT+104(%rip)
    mov %r12, USER_SYSCALL_CONTEXT+112(%rip)
    mov %r13, USER_SYSCALL_CONTEXT+120(%rip)
    mov %r14, USER_SYSCALL_CONTEXT+128(%rip)
    mov %r15, USER_SYSCALL_CONTEXT+136(%rip)
    mov %rsp, USER_SYSCALL_STACK_POINTER(%rip)
    mov %rcx, USER_SYSCALL_RIP(%rip)
    mov %r11, USER_SYSCALL_RFLAGS(%rip)
    mov %rax, USER_SYSCALL_NUMBER(%rip)
    mov %rdi, USER_SYSCALL_POINTER(%rip)
    mov %rsi, USER_SYSCALL_LENGTH(%rip)
    mov %rdx, USER_SYSCALL_ARGUMENT(%rip)
    call scheduler_syscall_stack_top
    mov %rax, %rsp
    and $-16, %rsp
    call scheduler_save_syscall_state
    mov USER_SYSCALL_LENGTH(%rip), %rdx
    mov USER_SYSCALL_POINTER(%rip), %rsi
    mov USER_SYSCALL_NUMBER(%rip), %rdi
    mov USER_SYSCALL_ARGUMENT(%rip), %rcx
    call syscall_dispatch
    pushq %rax
    call scheduler_current_syscall_state
    mov %rax, %rdx
    pushq %rdx
    call scheduler_set_tss_stack
    popq %rdx
    popq %rax
    pushq $0x1B
    pushq (%rdx)
    mov 16(%rdx), %r11
    and $0xED7, %r11
    or $0x202, %r11
    pushq %r11
    pushq $0x23
    pushq 8(%rdx)
    iretq
"#,
    options(att_syntax)
);

impl Architecture for X86_64 {
    fn initialize() {
        unsafe {
            asm!("cli", options(nomem, nostack));
        }
        initialize_tss();
        let gdt = DescriptorTablePointer {
            limit: (core::mem::size_of::<[u64; 7]>() - 1) as u16,
            base: (&raw const GDT).cast::<u64>() as u64,
        };
        load_gdt(&gdt);
        unsafe {
            asm!("ltr {selector:x}", selector = in(reg) TSS_SELECTOR, options(nostack));
        }
        initialize_syscalls();
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

fn initialize_tss() {
    unsafe {
        (*(&raw mut TSS)).rsp[0] = (&raw const SYSCALL_STACK).cast::<u8>() as u64
            + SYSCALL_STACK_SIZE as u64;
        let base = (&raw const TSS).cast::<u8>() as u64;
        let limit = (core::mem::size_of::<TaskStateSegment>() - 1) as u64;
        let descriptor = limit & 0xFFFF
            | (base & 0x00FF_FFFF) << 16
            | 0x89 << 40
            | ((limit >> 16) & 0xF) << 48
            | ((base >> 24) & 0xFF) << 56;
        (*(&raw mut GDT))[5] = descriptor;
        (*(&raw mut GDT))[6] = base >> 32;
    }
}

fn initialize_syscalls() {
    let efer = read_msr(IA32_EFER) | 1 | (1 << 11);
    unsafe {
        write_msr(IA32_EFER, efer);
        write_msr(IA32_STAR, (KERNEL_CODE << 32) | ((KERNEL_DATA as u64) << 48));
        write_msr(IA32_LSTAR, x86_syscall_stub as *const () as u64);
        write_msr(IA32_FMASK, (1 << 8) | (1 << 9) | (1 << 10) | (1 << 14) | (1 << 18));
    }
}

pub fn set_user_kernel_stack(stack_top: u64) {
    unsafe {
        (*(&raw mut TSS)).rsp[0] = stack_top;
    }
}

pub unsafe fn resume_user_context(context: *const crate::scheduler::UserContext) -> ! {
    set_user_kernel_stack(crate::scheduler::current_kernel_stack_top());
    unsafe {
        asm!(
            "push {user_data}",
            "push qword ptr [rdi + 8]",
            "push qword ptr [rdi + 16]",
            "push {user_code}",
            "push qword ptr [rdi]",
            "mov rax, [rdi + 24]",
            "mov rbx, [rdi + 32]",
            "mov rcx, [rdi + 40]",
            "mov rdx, [rdi + 48]",
            "mov rsi, [rdi + 56]",
            "mov rbp, [rdi + 72]",
            "mov r8, [rdi + 80]",
            "mov r9, [rdi + 88]",
            "mov r10, [rdi + 96]",
            "mov r11, [rdi + 104]",
            "mov r12, [rdi + 112]",
            "mov r13, [rdi + 120]",
            "mov r14, [rdi + 128]",
            "mov r15, [rdi + 136]",
            "mov rdi, [rdi + 64]",
            "iretq",
            user_data = in(reg) USER_DATA,
            user_code = in(reg) USER_CODE,
            in("rdi") context,
            options(noreturn),
        );
    }
}

#[unsafe(no_mangle)]
extern "C" fn scheduler_syscall_stack_top() -> u64 {
    crate::scheduler::syscall_stack_top()
}

#[unsafe(no_mangle)]
extern "C" fn scheduler_save_syscall_state() {
    crate::scheduler::save_syscall_state(crate::scheduler::SyscallState {
        stack_pointer: unsafe { core::ptr::read_volatile(&raw const USER_SYSCALL_STACK_POINTER) },
        instruction_pointer: unsafe { core::ptr::read_volatile(&raw const USER_SYSCALL_RIP) },
        flags: unsafe { core::ptr::read_volatile(&raw const USER_SYSCALL_RFLAGS) },
    });
    unsafe {
        crate::scheduler::save_user_context(core::ptr::read_volatile(
            &raw const USER_SYSCALL_CONTEXT,
        ));
    }
}

#[unsafe(no_mangle)]
extern "C" fn scheduler_current_syscall_state() -> *const crate::scheduler::SyscallState {
    crate::scheduler::current_syscall_state()
}

#[unsafe(no_mangle)]
extern "C" fn scheduler_set_tss_stack() {
    set_user_kernel_stack(crate::scheduler::current_kernel_stack_top());
}

#[unsafe(no_mangle)]
extern "C" fn syscall_dispatch(number: u64, pointer: u64, length: u64, argument: u64) -> u64 {
    match number {
        1 => syscall_write(pointer, length),
        2 => {
            crate::scheduler::yield_now();
            0
        }
        3 => {
            Com1.write(b"open-kernel: user process exited\r\n");
            crate::scheduler::exit_current()
        }
        4 => syscall_spawn(),
        5 => syscall_sleep(pointer),
        6 => crate::console::poll_user_key().map(u64::from).unwrap_or(0),
        7 => {
            crate::console::user_console_clear();
            0
        }
        8 => {
            crate::console::user_console_backspace();
            0
        }
        9 => crate::shutdown(),
        10 => syscall_sata_status(),
        11 => syscall_sata_identify(),
        12 => syscall_sata_read(),
        13 => syscall_pci_status(),
        14 => syscall_lsblk(),
        15 => syscall_thread_create(pointer, length),
        16 => crate::scheduler::exit_current_with_status(pointer),
        17 => crate::scheduler::join(pointer as usize).unwrap_or(u64::MAX),
        18 => crate::user::brk(pointer),
        19 => syscall_vfs_open(pointer, length, argument),
        20 => syscall_vfs_read(pointer, length, argument),
        21 => syscall_vfs_write(pointer, length, argument),
        22 => crate::user::close(pointer),
        23 => crate::user::seek(pointer, length as i64, argument),
        24 => syscall_chdir(pointer, length),
        25 => syscall_getcwd(pointer, length),
        26 => syscall_executable_info(pointer, length),
        _ => u64::MAX,
    }
}

fn syscall_spawn() -> u64 {
    u64::MAX
}

fn syscall_thread_create(entry: u64, argument: u64) -> u64 {
    crate::scheduler::spawn_user(entry, argument, None)
        .map(|thread| thread as u64)
        .unwrap_or(u64::MAX)
}

fn syscall_sleep(ticks: u64) -> u64 {
    let start = timer_ticks();
    while timer_ticks().wrapping_sub(start) < ticks {
        unsafe {
            asm!("sti", "hlt", "cli", options(nomem, nostack));
        }
    }
    0
}

fn syscall_vfs_open(path: u64, length: u64, flags: u64) -> u64 {
    let Some(bytes) = user_bytes(path, length, 64) else {
        return u64::MAX;
    };
    crate::user::open(bytes, flags)
}

fn syscall_vfs_read(fd: u64, buffer: u64, length: u64) -> u64 {
    let Some(output) = user_bytes_mut(buffer, length, 4096) else {
        return u64::MAX;
    };
    crate::user::read(fd, output)
}

fn syscall_vfs_write(fd: u64, buffer: u64, length: u64) -> u64 {
    let Some(input) = user_bytes(buffer, length, 4096) else {
        return u64::MAX;
    };
    crate::user::write(fd, input)
}

fn syscall_chdir(path: u64, length: u64) -> u64 {
    let Some(path) = user_bytes(path, length, 64) else {
        return u64::MAX;
    };
    crate::user::chdir(path)
}

fn syscall_getcwd(buffer: u64, length: u64) -> u64 {
    let Some(output) = user_bytes_mut(buffer, length, 64) else {
        return u64::MAX;
    };
    crate::user::getcwd(output)
}

fn syscall_executable_info(buffer: u64, length: u64) -> u64 {
    let Some(output) = user_bytes_mut(buffer, length, 64) else {
        return u64::MAX;
    };
    crate::user::executable_info(output)
}

fn syscall_write(pointer: u64, length: u64) -> u64 {
    let Some(bytes) = user_bytes(pointer, length, 256) else {
        return u64::MAX;
    };
    Com1.write(bytes);
    crate::console::user_console_write(bytes);
    length
}

fn user_bytes(pointer: u64, length: u64, maximum: u64) -> Option<&'static [u8]> {
    const USER_SPACE_END: u64 = 0x0000_8000_0000_0000;
    let end = pointer.checked_add(length)?;
    if pointer < crate::FUTURE_USER_SPACE_BASE || end > USER_SPACE_END || length > maximum {
        return None;
    }
    Some(unsafe { core::slice::from_raw_parts(pointer as *const u8, length as usize) })
}

fn user_bytes_mut(pointer: u64, length: u64, maximum: u64) -> Option<&'static mut [u8]> {
    let bytes = user_bytes(pointer, length, maximum)?;
    Some(unsafe { core::slice::from_raw_parts_mut(bytes.as_ptr() as *mut u8, bytes.len()) })
}

fn syscall_sata_status() -> u64 {
    if crate::sata_available() {
        console_output(b"SATA: AHCI DEVICE READY\r\n");
        0
    } else {
        console_output(b"SATA: NO AHCI DEVICE\r\n");
        u64::MAX
    }
}

fn syscall_pci_status() -> u64 {
    let count = crate::pci_device_count();
    console_output(b"PCI DEVICES: ");
    let mut digits = [0_u8; 20];
    let mut value = count;
    let mut length = 0;
    if value == 0 {
        digits[0] = b'0';
        length = 1;
    } else {
        while value != 0 {
            digits[length] = b'0' + (value % 10) as u8;
            value /= 10;
            length += 1;
        }
        digits[..length].reverse();
    }
    console_output(&digits[..length]);
    console_output(b"\r\n");
    if crate::find_ahci_controller().is_some() {
        console_output(b"PCI AHCI CONTROLLER FOUND\r\n");
    } else {
        console_output(b"PCI AHCI CONTROLLER NOT FOUND\r\n");
    }
    count as u64
}

fn syscall_lsblk() -> u64 {
    if crate::sata_identify().is_err() {
        console_output(b"NAME   TYPE  SECTORS\r\n(no block devices)\r\n");
        return u64::MAX;
    }
    console_output(b"NAME   TYPE  SECTORS\r\nsata0  disk  ");
    console_output_decimal(crate::sata_sector_count());
    console_output(b"\r\n");
    0
}

fn syscall_sata_identify() -> u64 {
    match crate::sata_identify() {
        Ok(()) => {
            console_output(b"SATA MODEL: ");
            for index in 0..40 {
                console_output(&[crate::sata_identify_model_byte(index)]);
            }
            console_output(b"\r\n");
            0
        }
        Err(_) => {
            console_output(b"SATA IDENTIFY FAILED\r\n");
            u64::MAX
        }
    }
}

fn syscall_sata_read() -> u64 {
    match crate::sata_read_first_sector() {
        Ok(bytes) => {
            let mut output = [0_u8; 64];
            let mut length = copy_bytes(&mut output, b"LBA 0: ");
            for byte in bytes {
                output[length] = hex_digit(byte >> 4);
                output[length + 1] = hex_digit(byte & 0xF);
                length += 2;
                if length < output.len() - 2 {
                    output[length] = b' ';
                    length += 1;
                }
            }
            output[length] = b'\r';
            output[length + 1] = b'\n';
            console_output(&output[..length + 2]);
            0
        }
        Err(_) => {
            console_output(b"SATA READ FAILED\r\n");
            u64::MAX
        }
    }
}

fn console_output(bytes: &[u8]) {
    Com1.write(bytes);
    crate::console::user_console_write(bytes);
}

fn console_output_decimal(mut value: u64) {
    let mut digits = [0_u8; 20];
    let mut length = 0;
    if value == 0 {
        console_output(b"0");
        return;
    }
    while value != 0 {
        digits[length] = b'0' + (value % 10) as u8;
        value /= 10;
        length += 1;
    }
    while length != 0 {
        length -= 1;
        console_output(&digits[length..=length]);
    }
}

fn copy_bytes(target: &mut [u8], source: &[u8]) -> usize {
    let length = target.len().min(source.len());
    for index in 0..length {
        target[index] = source[index];
    }
    length
}

fn hex_digit(value: u8) -> u8 {
    match value {
        0..=9 => b'0' + value,
        _ => b'A' + value - 10,
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

pub fn shutdown() -> ! {
    unsafe { outw(0x604, 0x2000); }
    X86_64::halt()
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
extern "C" fn exception_halt(frame: *const u64) -> ! {
    let instruction_pointer = unsafe { core::ptr::read_volatile(frame.add(1)) };
    Com1.write(b"open-kernel: CPU exception at 0x");
    write_hex(instruction_pointer);
    Com1.write(b"\r\n");
    X86_64::halt()
}

fn write_hex(value: u64) {
    for shift in (0..16).rev() {
        let digit = ((value >> (shift * 4)) & 0xF) as u8;
        Com1.write(&[hex_digit(digit)]);
    }
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

unsafe fn outw(port: u16, value: u16) {
    unsafe {
        asm!("out dx, ax", in("dx") port, in("ax") value, options(nomem, nostack));
    }
}

fn read_msr(msr: u32) -> u64 {
    let low: u32;
    let high: u32;
    unsafe {
        asm!(
            "rdmsr",
            in("ecx") msr,
            out("eax") low,
            out("edx") high,
            options(nomem, nostack),
        );
    }
    (u64::from(high) << 32) | u64::from(low)
}

unsafe fn write_msr(msr: u32, value: u64) {
    unsafe {
        asm!(
            "wrmsr",
            in("ecx") msr,
            in("eax") value as u32,
            in("edx") (value >> 32) as u32,
            options(nomem, nostack),
        );
    }
}
