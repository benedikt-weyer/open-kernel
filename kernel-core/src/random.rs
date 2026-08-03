#![allow(unsafe_op_in_unsafe_fn)]

use core::arch::asm;

use crate::{
    allocate_physical_frame, free_physical_frame, physical_to_virtual,
    pci::{enable_memory_and_bus_master, find_virtio_rng},
};

const VIRTIO_STATUS: u16 = 0x12;
const VIRTIO_QUEUE_ADDRESS: u16 = 0x08;
const VIRTIO_QUEUE_SIZE: u16 = 0x0C;
const VIRTIO_QUEUE_SELECT: u16 = 0x0E;
const VIRTIO_QUEUE_NOTIFY: u16 = 0x10;
const VIRTIO_ACKNOWLEDGE: u8 = 1;
const VIRTIO_DRIVER: u8 = 2;
const VIRTIO_DRIVER_OK: u8 = 4;
const VIRTIO_QUEUE: u16 = 0;
const PAGE_SIZE: u64 = 4096;

#[derive(Clone, Copy)]
pub enum RandomError {
    Unavailable,
    Timeout,
}

static mut VIRTIO_PORT: u16 = 0;
static mut VIRTIO_QUEUE_FRAME: u64 = 0;
static mut VIRTIO_BUFFER_FRAME: u64 = 0;
static mut VIRTIO_RING_SIZE: usize = 0;
static mut VIRTIO_AVAILABLE_INDEX: u16 = 0;
static mut VIRTIO_USED_INDEX: u16 = 0;

pub fn initialize() {
    let Some(device) = find_virtio_rng() else {
        return;
    };
    enable_memory_and_bus_master(device.device);
    unsafe {
        outb(device.io_base + VIRTIO_STATUS, 0);
        outb(device.io_base + VIRTIO_STATUS, VIRTIO_ACKNOWLEDGE | VIRTIO_DRIVER);
        outw(device.io_base + VIRTIO_QUEUE_SELECT, VIRTIO_QUEUE);
        let queue_size = inw(device.io_base + VIRTIO_QUEUE_SIZE) as usize;
        if queue_size == 0 || queue_size > 128 {
            return;
        }
        let Some(queue) = allocate_physical_frame() else { return; };
        let Some(queue_next) = allocate_physical_frame() else {
            free_physical_frame(queue);
            return;
        };
        if queue_next != queue + PAGE_SIZE {
            free_physical_frame(queue);
            free_physical_frame(queue_next);
            return;
        }
        let Some(buffer) = allocate_physical_frame() else {
            free_physical_frame(queue);
            free_physical_frame(queue_next);
            return;
        };
        zero_page(queue);
        zero_page(queue_next);
        outl(device.io_base + VIRTIO_QUEUE_ADDRESS, (queue / PAGE_SIZE) as u32);
        outb(device.io_base + VIRTIO_STATUS, VIRTIO_ACKNOWLEDGE | VIRTIO_DRIVER | VIRTIO_DRIVER_OK);
        VIRTIO_PORT = device.io_base;
        VIRTIO_QUEUE_FRAME = queue;
        VIRTIO_BUFFER_FRAME = buffer;
        VIRTIO_RING_SIZE = queue_size;
        VIRTIO_AVAILABLE_INDEX = 0;
        VIRTIO_USED_INDEX = 0;
    }
}

pub fn fill(output: &mut [u8]) -> Result<(), RandomError> {
    if output.is_empty() {
        return Ok(());
    }
    if unsafe { VIRTIO_PORT } != 0 && output.len() <= PAGE_SIZE as usize {
        if fill_virtio(output).is_ok() {
            return Ok(());
        }
    }
    fill_hardware(output)
}

fn fill_virtio(output: &mut [u8]) -> Result<(), RandomError> {
    unsafe {
        let port = VIRTIO_PORT;
        let queue = physical_to_virtual(VIRTIO_QUEUE_FRAME);
        let queue_size = VIRTIO_RING_SIZE;
        let available_index = VIRTIO_AVAILABLE_INDEX;
        let used_index = VIRTIO_USED_INDEX;
        let available = queue.add(queue_size * 16);
        // The used ring starts at the second page; queue setup rejects sizes
        // that would require a larger contiguous legacy ring.
        write_u64(queue, 0, VIRTIO_BUFFER_FRAME);
        write_u32(queue, 8, output.len() as u32);
        write_u16(queue, 12, 2); // device writes into the buffer
        write_u16(queue, 14, 0);
        write_u16(available, 0, 0);
        write_u16(available, 4 + (available_index as usize % queue_size) * 2, 0);
        write_u16(available, 2, available_index.wrapping_add(1));
        VIRTIO_AVAILABLE_INDEX = available_index.wrapping_add(1);
        outw(port + VIRTIO_QUEUE_NOTIFY, VIRTIO_QUEUE);
        let used = queue.add(PAGE_SIZE as usize);
        for _ in 0..1_000_000 {
            if read_u16(used, 2) != used_index {
                let length = read_u32(used, 8 + (used_index as usize % queue_size) * 8)
                    .min(output.len() as u32) as usize;
                VIRTIO_USED_INDEX = used_index.wrapping_add(1);
                let source = physical_to_virtual(VIRTIO_BUFFER_FRAME);
                for index in 0..length {
                    output[index] = core::ptr::read_volatile(source.add(index));
                }
                return if length == output.len() { Ok(()) } else { Err(RandomError::Unavailable) };
            }
            core::hint::spin_loop();
        }
    }
    Err(RandomError::Timeout)
}

fn fill_hardware(output: &mut [u8]) -> Result<(), RandomError> {
    let mut offset = 0;
    while offset < output.len() {
        let Some(value) = hardware_word() else { return Err(RandomError::Unavailable); };
        for byte in value.to_ne_bytes() {
            if offset == output.len() { break; }
            output[offset] = byte;
            offset += 1;
        }
    }
    Ok(())
}

fn hardware_word() -> Option<u64> {
    let leaf1 = core::arch::x86_64::__cpuid(1);
    let leaf7 = core::arch::x86_64::__cpuid_count(7, 0);
    for _ in 0..10 {
        let value: u64;
        let success: u8;
        unsafe {
            if leaf7.ebx & (1 << 18) != 0 {
                asm!("rdseed {value}", "setc {success}", value = out(reg) value, success = out(reg_byte) success, options(nomem, nostack));
            } else if leaf1.ecx & (1 << 30) != 0 {
                asm!("rdrand {value}", "setc {success}", value = out(reg) value, success = out(reg_byte) success, options(nomem, nostack));
            } else {
                return None;
            }
        }
        if success != 0 { return Some(value); }
    }
    None
}

unsafe fn zero_page(frame: u64) {
    let page = physical_to_virtual(frame);
    for index in 0..PAGE_SIZE as usize { core::ptr::write_volatile(page.add(index), 0); }
}
unsafe fn write_u16(base: *mut u8, offset: usize, value: u16) { core::ptr::write_volatile(base.add(offset).cast(), value); }
unsafe fn write_u32(base: *mut u8, offset: usize, value: u32) { core::ptr::write_volatile(base.add(offset).cast(), value); }
unsafe fn write_u64(base: *mut u8, offset: usize, value: u64) { core::ptr::write_volatile(base.add(offset).cast(), value); }
unsafe fn read_u16(base: *mut u8, offset: usize) -> u16 { core::ptr::read_volatile(base.add(offset).cast()) }
unsafe fn read_u32(base: *mut u8, offset: usize) -> u32 { core::ptr::read_volatile(base.add(offset).cast()) }
unsafe fn inw(port: u16) -> u16 { let value; asm!("in ax, dx", in("dx") port, out("ax") value, options(nomem, nostack)); value }
unsafe fn outb(port: u16, value: u8) { asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack)); }
unsafe fn outw(port: u16, value: u16) { asm!("out dx, ax", in("dx") port, in("ax") value, options(nomem, nostack)); }
unsafe fn outl(port: u16, value: u32) { asm!("out dx, eax", in("dx") port, in("eax") value, options(nomem, nostack)); }
