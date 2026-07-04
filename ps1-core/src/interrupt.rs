use serde::{Deserialize, Serialize};

pub const IRQ_VBLANK: u16 = 1 << 0;
pub const IRQ_GPU: u16 = 1 << 1;
pub const IRQ_CDROM: u16 = 1 << 2;
pub const IRQ_DMA: u16 = 1 << 3;
pub const IRQ_TIMER0: u16 = 1 << 4;
pub const IRQ_TIMER1: u16 = 1 << 5;
pub const IRQ_TIMER2: u16 = 1 << 6;
pub const IRQ_JOY: u16 = 1 << 7;
pub const IRQ_SIO: u16 = 1 << 8;
pub const IRQ_SPU: u16 = 1 << 9;
pub const IRQ_LIGHTPEN: u16 = 1 << 10;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterruptController {
    status: u16,
    mask: u16,
}

impl InterruptController {
    pub fn new() -> Self {
        Self { status: 0, mask: 0 }
    }

    pub fn status(&self) -> u16 {
        self.status
    }

    pub fn mask(&self) -> u16 {
        self.mask
    }

    pub fn request(&mut self, bit: u16) {
        self.status |= bit & 0x07ff;
    }

    pub fn acknowledge(&mut self, value: u16) {
        self.status &= value & 0x07ff;
    }

    pub fn set_mask(&mut self, value: u16) {
        self.mask = value & 0x07ff;
    }

    pub fn pending(&self) -> bool {
        (self.status & self.mask) != 0
    }
}

impl Default for InterruptController {
    fn default() -> Self {
        Self::new()
    }
}
