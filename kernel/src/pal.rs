//! Peripheral Abstraction Layer (PAL) — bus instance table (CLAUDE.md spec)

pub const MAX_BUS_INSTANCES: usize = 8;

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum BusType {
    Gpio = 0,
    I2c = 1,
    Spi = 2,
    Uart = 3,
    OneWire = 4,
}

#[derive(Copy, Clone)]
pub struct BusInstance {
    pub in_use: bool,
    pub bus_type: BusType,
    pub pins: [u8; 4],
    pub config: u32,
}

const EMPTY_BUS: BusInstance = BusInstance {
    in_use: false,
    bus_type: BusType::Gpio,
    pins: [0; 4],
    config: 0,
};

pub static mut PAL_BUS_TABLE: [BusInstance; MAX_BUS_INSTANCES] = [EMPTY_BUS; MAX_BUS_INSTANCES];

pub fn bus_create(bus_type: BusType, pins: [u8; 4], config: u32) -> Result<usize, &'static str> {
    unsafe {
        for (i, bus) in PAL_BUS_TABLE.iter_mut().enumerate() {
            if !bus.in_use {
                bus.in_use = true;
                bus.bus_type = bus_type;
                bus.pins = pins;
                bus.config = config;
                return Ok(i);
            }
        }
        Err("ERR_NO_BUS_SLOTS")
    }
}

pub fn bus_delete(handle: usize) -> Result<(), &'static str> {
    unsafe {
        if handle >= MAX_BUS_INSTANCES {
            return Err("ERR_INVALID_HANDLE");
        }
        if !PAL_BUS_TABLE[handle].in_use {
            return Err("ERR_NOT_FOUND");
        }
        PAL_BUS_TABLE[handle].in_use = false;
        Ok(())
    }
}

pub fn pal_query(driver_slot: usize, out_buf: &mut [u8]) -> usize {
    unsafe {
        if driver_slot >= MAX_BUS_INSTANCES || !PAL_BUS_TABLE[driver_slot].in_use {
            return 0;
        }
        let bus = &PAL_BUS_TABLE[driver_slot];
        if out_buf.len() >= 8 {
            out_buf[0] = bus.bus_type as u8;
            out_buf[1] = bus.pins[0];
            out_buf[2] = bus.pins[1];
            out_buf[3] = bus.pins[2];
            out_buf[4] = bus.pins[3];
            out_buf[5..9].copy_from_slice(&bus.config.to_le_bytes());
            9
        } else {
            0
        }
    }
}

pub fn format_proc_pal(out: &mut [u8]) -> usize {
    let mut written = 0;
    unsafe {
        for (i, bus) in PAL_BUS_TABLE.iter().enumerate() {
            if bus.in_use {
                let header = b"BUS=";
                for &b in header { if written < out.len() { out[written] = b; written += 1; } }
                if written < out.len() { out[written] = b'0' + (i as u8); written += 1; }
                let type_str: &[u8] = match bus.bus_type {
                    BusType::Gpio => b" gpio\n",
                    BusType::I2c => b" i2c\n",
                    BusType::Spi => b" spi\n",
                    BusType::Uart => b" uart\n",
                    BusType::OneWire => b" onewire\n",
                };
                for &b in type_str { if written < out.len() { out[written] = b; written += 1; } }
            }
        }
    }
    written
}
