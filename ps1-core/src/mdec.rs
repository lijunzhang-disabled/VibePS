use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mdec {
    command: u32,
    status: u32,
}

impl Mdec {
    pub fn new() -> Self {
        Self {
            command: 0,
            status: 0x8000_0000,
        }
    }

    pub fn read_data(&self) -> u32 {
        0
    }

    pub fn write_data(&mut self, value: u32) {
        self.command = value;
        self.status &= !0x8000_0000;
    }

    pub fn read_status(&self) -> u32 {
        self.status
    }

    pub fn write_control(&mut self, value: u32) {
        if (value & 0x8000_0000) != 0 {
            self.command = 0;
            self.status = 0x8000_0000;
        }
    }
}

impl Default for Mdec {
    fn default() -> Self {
        Self::new()
    }
}
