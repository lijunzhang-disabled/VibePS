use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cdrom {
    index: u8,
    status: u8,
    interrupt_enable: u8,
    interrupt_flags: u8,
    params: VecDeque<u8>,
    responses: VecDeque<u8>,
    data: VecDeque<u8>,
}

impl Cdrom {
    pub fn new() -> Self {
        Self {
            index: 0,
            status: 0x18,
            interrupt_enable: 0,
            interrupt_flags: 0,
            params: VecDeque::new(),
            responses: VecDeque::new(),
            data: VecDeque::new(),
        }
    }

    pub fn read8(&mut self, offset: u32) -> u8 {
        match offset & 0x3 {
            0 => self.status_byte(),
            1 => self.responses.pop_front().unwrap_or(0),
            2 => self.data.pop_front().unwrap_or(0),
            3 => match self.index & 0x3 {
                0 | 2 => self.interrupt_enable,
                _ => self.interrupt_flags,
            },
            _ => 0,
        }
    }

    pub fn write8(&mut self, offset: u32, value: u8) {
        match offset & 0x3 {
            0 => self.index = value & 0x3,
            1 => {
                if self.index == 0 {
                    self.command(value);
                }
            }
            2 => match self.index & 0x3 {
                0 => self.params.push_back(value),
                1 => self.interrupt_enable = value & 0x1f,
                _ => {}
            },
            3 => match self.index & 0x3 {
                0 => {}
                1 | 3 => self.interrupt_flags &= !value,
                _ => {}
            },
            _ => {}
        }
    }

    pub fn dma_read32(&mut self) -> u32 {
        let b0 = self.data.pop_front().unwrap_or(0) as u32;
        let b1 = self.data.pop_front().unwrap_or(0) as u32;
        let b2 = self.data.pop_front().unwrap_or(0) as u32;
        let b3 = self.data.pop_front().unwrap_or(0) as u32;
        b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
    }

    pub fn interrupt_pending(&self) -> bool {
        (self.interrupt_flags & self.interrupt_enable & 0x1f) != 0
    }

    fn status_byte(&self) -> u8 {
        let mut status = self.index & 0x3;
        if !self.responses.is_empty() {
            status |= 1 << 5;
        }
        if !self.params.is_empty() {
            status |= 1 << 4;
        }
        status | 0x18
    }

    fn command(&mut self, command: u8) {
        self.params.clear();
        match command {
            0x01 => self.finish_command(&[self.status]), // Nop
            0x0a => self.finish_command(&[self.status]), // Init, first response only for now
            0x0e => self.finish_command(&[self.status]), // Setmode
            0x0f => self.finish_command(&[self.status, 0, 0, 0, 0]), // Getparam
            0x19 => self.finish_command(&[self.status, 0, 0, 0, 0]), // Test placeholder
            _ => self.finish_command(&[self.status | 0x01]),
        }
    }

    fn finish_command(&mut self, response: &[u8]) {
        self.responses.extend(response.iter().copied());
        self.interrupt_flags = (self.interrupt_flags & !0x7) | 0x3;
    }
}

impl Default for Cdrom {
    fn default() -> Self {
        Self::new()
    }
}
