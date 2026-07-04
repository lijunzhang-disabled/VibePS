use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gte {
    regs: Vec<u32>,
}

impl Gte {
    pub fn new() -> Self {
        Self { regs: vec![0; 64] }
    }

    pub fn reset(&mut self) {
        self.regs.fill(0);
    }

    pub fn read_data(&self, index: usize) -> u32 {
        self.regs.get(index & 31).copied().unwrap_or(0)
    }

    pub fn write_data(&mut self, index: usize, value: u32) {
        self.regs[index & 31] = value;
    }

    pub fn read_control(&self, index: usize) -> u32 {
        self.regs.get(32 + (index & 31)).copied().unwrap_or(0)
    }

    pub fn write_control(&mut self, index: usize, value: u32) {
        self.regs[32 + (index & 31)] = value;
    }

    pub fn execute_command(&mut self, _opcode: u32) {
        // GTE math is a major subsystem. Register transfer support is useful
        // during CPU bring-up; command execution is implemented in a later phase.
    }
}

impl Default for Gte {
    fn default() -> Self {
        Self::new()
    }
}
