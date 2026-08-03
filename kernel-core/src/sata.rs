use core::ptr::{read_volatile, write_volatile};

use crate::{
    DEVICE_WINDOW_BASE, PAGE_SIZE, allocate_physical_frame, map_device_page, physical_to_virtual,
    zero_physical_frame,
};

const HBA_GHC: usize = 0x04;
const HBA_PI: usize = 0x0C;
const HBA_GHC_AE: u32 = 1 << 31;
const HBA_GHC_HR: u32 = 1;
const PORT_BASE: usize = 0x100;
const PORT_SIZE: usize = 0x80;
const PORT_CLB: usize = 0x00;
const PORT_FB: usize = 0x08;
const PORT_CMD: usize = 0x18;
const PORT_TFD: usize = 0x20;
const PORT_SIG: usize = 0x24;
const PORT_SSTS: usize = 0x28;
const PORT_IS: usize = 0x10;
const PORT_CI: usize = 0x38;
const PORT_CMD_ST: u32 = 1;
const PORT_CMD_FRE: u32 = 1 << 4;
const PORT_CMD_FR: u32 = 1 << 14;
const PORT_CMD_CR: u32 = 1 << 15;
const SATA_SIGNATURE: u32 = 0x0000_0101;
const ATA_IDENTIFY: u8 = 0xEC;
const ATA_READ_DMA_EXT: u8 = 0x25;
const AHCI_MMIO_BASE: u64 = DEVICE_WINDOW_BASE + PAGE_SIZE * 16;

#[derive(Clone, Copy)]
pub enum SataError {
    NotFound,
    NoDevice,
    Timeout,
    DeviceError,
    AllocationFailed,
}

#[derive(Clone, Copy)]
struct Controller {
    mmio: *mut u8,
    port: usize,
    command_list: u64,
    _received_fis: u64,
    command_table: u64,
    data_buffer: u64,
}

static mut CONTROLLER: Option<Controller> = None;

pub fn initialize() -> Result<(), SataError> {
    let Some(ahci) = crate::pci::find_ahci_controller() else {
        return Err(SataError::NotFound);
    };
    let bar = ahci.abar;
    map_device_page(AHCI_MMIO_BASE, bar).map_err(|_| SataError::AllocationFailed)?;
    map_device_page(AHCI_MMIO_BASE + PAGE_SIZE, bar + PAGE_SIZE)
        .map_err(|_| SataError::AllocationFailed)?;
    let mmio = AHCI_MMIO_BASE as *mut u8;
    let controller = initialize_controller(mmio)?;
    unsafe {
        CONTROLLER = Some(controller);
    }
    Ok(())
}

pub fn is_available() -> bool {
    unsafe { core::ptr::read_volatile(&raw const CONTROLLER).is_some() }
}

pub fn identify() -> Result<[u8; 40], SataError> {
    let controller = unsafe { CONTROLLER.ok_or(SataError::NotFound)? };
    unsafe {
        issue_command(controller, ATA_IDENTIFY, 0)?;
        let source = physical_to_virtual(controller.data_buffer);
        let mut model = [b' '; 40];
        for index in 0..20 {
            model[index * 2] = read_volatile(source.add(54 + index * 2 + 1));
            model[index * 2 + 1] = read_volatile(source.add(54 + index * 2));
        }
        Ok(model)
    }
}

pub fn read_first_sector() -> Result<[u8; 16], SataError> {
    let controller = unsafe { CONTROLLER.ok_or(SataError::NotFound)? };
    unsafe {
        issue_command(controller, ATA_READ_DMA_EXT, 0)?;
        let source = physical_to_virtual(controller.data_buffer);
        let mut preview = [0; 16];
        for (index, byte) in preview.iter_mut().enumerate() {
            *byte = read_volatile(source.add(index));
        }
        Ok(preview)
    }
}

fn initialize_controller(mmio: *mut u8) -> Result<Controller, SataError> {
    write_register(mmio, HBA_GHC, read_register(mmio, HBA_GHC) | HBA_GHC_AE);
    write_register(mmio, HBA_GHC, read_register(mmio, HBA_GHC) | HBA_GHC_HR);
    if !wait_for(|| read_register(mmio, HBA_GHC) & HBA_GHC_HR == 0) {
        return Err(SataError::Timeout);
    }
    write_register(mmio, HBA_GHC, read_register(mmio, HBA_GHC) | HBA_GHC_AE);
    let implemented = read_register(mmio, HBA_PI);
    for port in 0..32 {
        if implemented & (1 << port) == 0 {
            continue;
        }
        let base = PORT_BASE + port * PORT_SIZE;
        let status = read_register(mmio, base + PORT_SSTS);
        if status & 0xF != 3 || (status >> 8) & 0xF == 0 {
            continue;
        }
        if read_register(mmio, base + PORT_SIG) != SATA_SIGNATURE {
            continue;
        }
        let command_list = allocate_physical_frame().ok_or(SataError::AllocationFailed)?;
        let received_fis = allocate_physical_frame().ok_or(SataError::AllocationFailed)?;
        let command_table = allocate_physical_frame().ok_or(SataError::AllocationFailed)?;
        let data_buffer = allocate_physical_frame().ok_or(SataError::AllocationFailed)?;
        zero_physical_frame(command_list);
        zero_physical_frame(received_fis);
        zero_physical_frame(command_table);
        zero_physical_frame(data_buffer);
        stop_port(mmio, base)?;
        write_u64(mmio, base + PORT_CLB, command_list);
        write_u64(mmio, base + PORT_FB, received_fis);
        write_register(mmio, base + PORT_IS, u32::MAX);
        write_register(
            mmio,
            base + PORT_CMD,
            read_register(mmio, base + PORT_CMD) | PORT_CMD_FRE | PORT_CMD_ST,
        );
        return Ok(Controller {
            mmio,
            port,
            command_list,
            _received_fis: received_fis,
            command_table,
            data_buffer,
        });
    }
    Err(SataError::NoDevice)
}

#[allow(unsafe_op_in_unsafe_fn)]
unsafe fn issue_command(controller: Controller, command: u8, lba: u64) -> Result<(), SataError> {
    let base = PORT_BASE + controller.port * PORT_SIZE;
    if !wait_for(|| read_register(controller.mmio, base + PORT_TFD) & 0x88 == 0) {
        return Err(SataError::Timeout);
    }
    zero_physical_frame(controller.command_table);
    zero_physical_frame(controller.data_buffer);
    let header = physical_to_virtual(controller.command_list);
    write_volatile(header.cast::<u16>(), 5);
    write_volatile(header.add(2).cast::<u16>(), 1);
    write_volatile(header.add(8).cast::<u32>(), controller.command_table as u32);
    write_volatile(header.add(12).cast::<u32>(), (controller.command_table >> 32) as u32);

    let table = physical_to_virtual(controller.command_table);
    write_volatile(table, 0x27);
    write_volatile(table.add(1), 0x80);
    write_volatile(table.add(2), command);
    write_volatile(table.add(4), lba as u8);
    write_volatile(table.add(5), (lba >> 8) as u8);
    write_volatile(table.add(6), (lba >> 16) as u8);
    write_volatile(table.add(7), 1 << 6);
    write_volatile(table.add(8), (lba >> 24) as u8);
    write_volatile(table.add(9), (lba >> 32) as u8);
    write_volatile(table.add(10), (lba >> 40) as u8);
    write_volatile(table.add(12), 1);
    write_volatile(table.add(128).cast::<u32>(), controller.data_buffer as u32);
    write_volatile(table.add(132).cast::<u32>(), (controller.data_buffer >> 32) as u32);
    write_volatile(table.add(140).cast::<u32>(), 511 | (1 << 31));

    write_register(controller.mmio, base + PORT_IS, u32::MAX);
    write_register(controller.mmio, base + PORT_CI, 1);
    if !wait_for(|| read_register(controller.mmio, base + PORT_CI) & 1 == 0) {
        return Err(SataError::Timeout);
    }
    if read_register(controller.mmio, base + PORT_TFD) & 1 != 0 {
        return Err(SataError::DeviceError);
    }
    Ok(())
}

fn stop_port(mmio: *mut u8, base: usize) -> Result<(), SataError> {
    write_register(mmio, base + PORT_CMD, read_register(mmio, base + PORT_CMD) & !(PORT_CMD_ST | PORT_CMD_FRE));
    if !wait_for(|| read_register(mmio, base + PORT_CMD) & (PORT_CMD_CR | PORT_CMD_FR) == 0) {
        return Err(SataError::Timeout);
    }
    Ok(())
}

fn read_register(mmio: *mut u8, offset: usize) -> u32 {
    unsafe { read_volatile(mmio.add(offset).cast()) }
}
fn write_register(mmio: *mut u8, offset: usize, value: u32) {
    unsafe { write_volatile(mmio.add(offset).cast(), value) }
}
fn write_u64(mmio: *mut u8, offset: usize, value: u64) {
    write_register(mmio, offset, value as u32);
    write_register(mmio, offset + 4, (value >> 32) as u32);
}
fn wait_for(mut ready: impl FnMut() -> bool) -> bool {
    for _ in 0..1_000_000 {
        if ready() {
            return true;
        }
    }
    false
}
