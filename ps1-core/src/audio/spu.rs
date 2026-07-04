use crate::SPU_RAM_SIZE;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Spu {
    regs: Vec<u16>,
    ram: Vec<u8>,
    transfer_addr: u32,
}

impl Spu {
    pub fn new() -> Self {
        Self {
            regs: vec![0; 0x200],
            ram: vec![0; SPU_RAM_SIZE],
            transfer_addr: 0,
        }
    }

    pub fn read16(&self, offset: u32) -> u16 {
        match offset {
            0x1a6 => (self.transfer_addr >> 3) as u16,
            0x1a8 => self.read_transfer_fifo(),
            0x1ae => self.regs[0x1ae / 2] | 0x0400,
            _ => self.regs[((offset as usize) / 2) & 0x1ff],
        }
    }

    pub fn write16(&mut self, offset: u32, value: u16) {
        match offset {
            0x1a6 => self.transfer_addr = (value as u32) << 3,
            0x1a8 => self.write_transfer_fifo(value),
            _ => {
                let index = ((offset as usize) / 2) & 0x1ff;
                self.regs[index] = value;
            }
        }
    }

    pub fn dma_write32(&mut self, value: u32) {
        self.write_transfer_fifo(value as u16);
        self.write_transfer_fifo((value >> 16) as u16);
    }

    pub fn dma_read32(&mut self) -> u32 {
        let lo = self.read_transfer_fifo() as u32;
        let hi = self.read_transfer_fifo() as u32;
        lo | (hi << 16)
    }

    fn write_transfer_fifo(&mut self, value: u16) {
        let index = self.transfer_addr as usize & (SPU_RAM_SIZE - 1);
        self.ram[index] = value as u8;
        self.ram[(index + 1) & (SPU_RAM_SIZE - 1)] = (value >> 8) as u8;
        self.transfer_addr = self.transfer_addr.wrapping_add(2) & ((SPU_RAM_SIZE - 1) as u32);
    }

    fn read_transfer_fifo(&self) -> u16 {
        let index = self.transfer_addr as usize & (SPU_RAM_SIZE - 1);
        u16::from_le_bytes([self.ram[index], self.ram[(index + 1) & (SPU_RAM_SIZE - 1)]])
    }
}

impl Default for Spu {
    fn default() -> Self {
        Self::new()
    }
}
