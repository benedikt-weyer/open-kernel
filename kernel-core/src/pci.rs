use core::arch::asm;

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

#[derive(Clone, Copy)]
pub struct PciDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class: u8,
    pub subclass: u8,
    pub programming_interface: u8,
}

#[derive(Clone, Copy)]
pub struct AhciController {
    pub device: PciDevice,
    pub abar: u64,
}

pub fn enumerate(mut visit: impl FnMut(PciDevice)) {
    for bus in 0..=u8::MAX {
        for device in 0..32 {
            let Some(function_zero) = read_device(bus, device, 0) else {
                continue;
            };
            visit(function_zero);
            if !is_multifunction(bus, device) {
                continue;
            }
            for function in 1..8 {
                if let Some(device) = read_device(bus, device, function) {
                    visit(device);
                }
            }
        }
    }
}

pub fn device_count() -> usize {
    let mut count = 0;
    enumerate(|_| count += 1);
    count
}

pub fn find_ahci_controller() -> Option<AhciController> {
    let mut controller = None;
    enumerate(|device| {
        if controller.is_none() && (device.class, device.subclass) == (0x01, 0x06) {
            let abar = read_config(device.bus, device.device, device.function, 0x24) & !0xF;
            if abar != 0 {
                controller = Some(AhciController {
                    device,
                    abar: abar as u64,
                });
            }
        }
    });
    controller
}

pub fn enable_memory_and_bus_master(device: PciDevice) {
    let command = read_config(device.bus, device.device, device.function, 0x04);
    write_config(
        device.bus,
        device.device,
        device.function,
        0x04,
        command | (1 << 1) | (1 << 2),
    );
}

fn read_device(bus: u8, device: u8, function: u8) -> Option<PciDevice> {
    let identification = read_config(bus, device, function, 0);
    let vendor_id = identification as u16;
    if vendor_id == u16::MAX {
        return None;
    }
    let class = read_config(bus, device, function, 8);
    Some(PciDevice {
        bus,
        device,
        function,
        vendor_id,
        device_id: (identification >> 16) as u16,
        class: (class >> 24) as u8,
        subclass: (class >> 16) as u8,
        programming_interface: (class >> 8) as u8,
    })
}

fn is_multifunction(bus: u8, device: u8) -> bool {
    read_config(bus, device, 0, 0x0C) & (1 << 23) != 0
}

fn read_config(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    let address = config_address(bus, device, function, offset);
    unsafe {
        asm!("out dx, eax", in("dx") CONFIG_ADDRESS, in("eax") address, options(nostack));
        let value: u32;
        asm!("in eax, dx", in("dx") CONFIG_DATA, out("eax") value, options(nostack));
        value
    }
}

fn write_config(bus: u8, device: u8, function: u8, offset: u8, value: u32) {
    let address = config_address(bus, device, function, offset);
    unsafe {
        asm!("out dx, eax", in("dx") CONFIG_ADDRESS, in("eax") address, options(nostack));
        asm!("out dx, eax", in("dx") CONFIG_DATA, in("eax") value, options(nostack));
    }
}

fn config_address(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    0x8000_0000
        | (u32::from(bus) << 16)
        | (u32::from(device) << 11)
        | (u32::from(function) << 8)
        | u32::from(offset & 0xFC)
}
