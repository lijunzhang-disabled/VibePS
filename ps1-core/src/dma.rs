use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct DmaChannel {
    pub madr: u32,
    pub bcr: u32,
    pub chcr: u32,
}

impl DmaChannel {
    pub fn new() -> Self {
        Self {
            madr: 0,
            bcr: 0,
            chcr: 0,
        }
    }

    pub fn active(&self) -> bool {
        (self.chcr & (1 << 24)) != 0
    }

    pub fn sync_mode(&self) -> u32 {
        (self.chcr >> 9) & 0x3
    }

    pub fn from_ram(&self) -> bool {
        (self.chcr & 1) != 0
    }

    pub fn step_backwards(&self) -> bool {
        (self.chcr & (1 << 1)) != 0
    }
}

impl Default for DmaChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DmaController {
    channels: [DmaChannel; 7],
    dpcr: u32,
    dicr: u32,
}

impl DmaController {
    pub fn new() -> Self {
        Self {
            channels: [DmaChannel::new(); 7],
            dpcr: 0x0765_4321,
            dicr: 0,
        }
    }

    pub fn channel(&self, index: usize) -> DmaChannel {
        self.channels[index]
    }

    pub fn channel_mut(&mut self, index: usize) -> &mut DmaChannel {
        &mut self.channels[index]
    }

    pub fn master_enabled(&self, index: usize) -> bool {
        (self.dpcr & (1 << (index * 4 + 3))) != 0
    }

    pub fn read32(&self, offset: u32) -> u32 {
        if offset < 0x70 {
            let channel = ((offset >> 4) & 0x7) as usize;
            let reg = offset & 0x0f;
            if channel >= self.channels.len() {
                return 0xffff_ffff;
            }
            match reg {
                0x0 => self.channels[channel].madr,
                0x4 => self.channels[channel].bcr,
                0x8 | 0xc => self.channels[channel].chcr,
                _ => 0xffff_ffff,
            }
        } else {
            match offset {
                0x70 => self.dpcr,
                0x74 => self.dicr | self.master_irq_bit(),
                0x78 => 0x7ffa_c68b,
                0x7c => 0x00ff_fff7,
                _ => 0xffff_ffff,
            }
        }
    }

    pub fn write32(&mut self, offset: u32, value: u32) -> Option<usize> {
        let mut started = None;
        if offset < 0x70 {
            let channel = ((offset >> 4) & 0x7) as usize;
            let reg = offset & 0x0f;
            if channel >= self.channels.len() {
                return None;
            }
            match reg {
                0x0 => self.channels[channel].madr = value & 0x00ff_ffff,
                0x4 => self.channels[channel].bcr = value,
                0x8 | 0xc => {
                    self.channels[channel].chcr = value;
                    if self.channels[channel].active() {
                        started = Some(channel);
                    }
                }
                _ => {}
            }
        } else {
            match offset {
                0x70 => self.dpcr = value,
                0x74 => {
                    let ack = value & 0x7f00_0000;
                    self.dicr &= !ack;
                    self.dicr = (self.dicr & 0x7fff_0000) | (value & 0x00ff_ffff);
                }
                _ => {}
            }
        }
        started
    }

    pub fn complete_channel(&mut self, index: usize) -> bool {
        self.channels[index].chcr &= !(1 << 24);
        self.channels[index].chcr &= !(1 << 28);
        self.dicr |= 1 << (24 + index);
        self.irq_pending()
    }

    pub fn irq_pending(&self) -> bool {
        let channel_flags = (self.dicr >> 24) & 0x7f;
        let channel_mask = (self.dicr >> 16) & 0x7f;
        let master = (self.dicr & (1 << 23)) != 0;
        master && (channel_flags & channel_mask) != 0
    }

    fn master_irq_bit(&self) -> u32 {
        if self.irq_pending() {
            1 << 31
        } else {
            0
        }
    }
}

impl Default for DmaController {
    fn default() -> Self {
        Self::new()
    }
}
