use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct DmaChannel {
    pub madr: u32,
    pub bcr: u32,
    pub chcr: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DmaWriteResult {
    pub irq_edge: bool,
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

    pub fn chopping_enabled(&self) -> bool {
        (self.chcr & (1 << 8)) != 0
    }

    pub fn ready_to_start(&self) -> bool {
        self.active() && (self.sync_mode() != 0 || (self.chcr & (1 << 28)) != 0)
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

    pub fn channel_priority(&self, index: usize) -> u32 {
        (self.dpcr >> (index * 4)) & 0x7
    }

    pub fn master_enabled(&self, index: usize) -> bool {
        (self.dpcr & (1 << (index * 4 + 3))) != 0
    }

    pub fn next_pending_channel(&self) -> Option<usize> {
        (0..self.channels.len())
            .filter(|&index| self.channels[index].ready_to_start() && self.master_enabled(index))
            .min_by_key(|&index| (self.channel_priority(index), 6usize.saturating_sub(index)))
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
                0x8 | 0xc => readable_chcr(channel, self.channels[channel].chcr),
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

    pub fn write32(&mut self, offset: u32, value: u32) -> DmaWriteResult {
        let master_before = self.master_irq_pending();
        if offset < 0x70 {
            let channel = ((offset >> 4) & 0x7) as usize;
            let reg = offset & 0x0f;
            if channel >= self.channels.len() {
                return DmaWriteResult::default();
            }
            match reg {
                0x0 => self.channels[channel].madr = value & 0x00ff_ffff,
                0x4 => self.channels[channel].bcr = value,
                0x8 | 0xc => {
                    self.channels[channel].chcr = writable_chcr(channel, value);
                }
                _ => {}
            }
        } else {
            match offset {
                0x70 => self.dpcr = value,
                0x74 => {
                    let ack = value & 0x7f00_0000;
                    self.dicr &= !ack;
                    self.dicr = (self.dicr & 0x7f00_0000) | (value & 0x00ff_807f);
                }
                _ => {}
            }
        }
        DmaWriteResult {
            irq_edge: !master_before && self.master_irq_pending(),
        }
    }

    pub fn complete_channel(
        &mut self,
        index: usize,
        final_madr: Option<u32>,
        final_bcr: Option<u32>,
        bus_error: bool,
    ) -> bool {
        let master_before = self.master_irq_pending();
        if let Some(madr) = final_madr {
            self.channels[index].madr = madr & 0x00ff_fffc;
        }
        if let Some(bcr) = final_bcr {
            self.channels[index].bcr = bcr;
        }
        self.channels[index].chcr &= !(1 << 24);
        self.channels[index].chcr &= !(1 << 28);
        if index == 6 {
            self.channels[index].chcr = readable_chcr(index, self.channels[index].chcr);
        }
        if bus_error {
            self.dicr |= 1 << 15;
        }
        if (self.dicr & (1 << (16 + index))) != 0 {
            self.dicr |= 1 << (24 + index);
        }
        !master_before && self.master_irq_pending()
    }

    pub fn irq_pending(&self) -> bool {
        self.master_irq_pending()
    }

    fn master_irq_bit(&self) -> u32 {
        if self.master_irq_pending() {
            1 << 31
        } else {
            0
        }
    }

    fn master_irq_pending(&self) -> bool {
        (self.dicr & (1 << 15)) != 0
            || ((self.dicr & (1 << 23)) != 0 && (self.dicr & 0x7f00_0000) != 0)
    }
}

impl Default for DmaController {
    fn default() -> Self {
        Self::new()
    }
}

fn writable_chcr(channel: usize, value: u32) -> u32 {
    if channel == 6 {
        (value & ((1 << 24) | (1 << 28) | (1 << 30))) | (1 << 1)
    } else {
        value & 0x7177_0703
    }
}

fn readable_chcr(channel: usize, value: u32) -> u32 {
    if channel == 6 {
        (value & ((1 << 24) | (1 << 28) | (1 << 30))) | (1 << 1)
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::DmaController;

    #[test]
    fn dma6_chcr_only_exposes_otc_control_bits() {
        let mut dma = DmaController::new();

        dma.write32(0x68, 0xffff_ffff);

        assert_eq!(dma.read32(0x68), 0x5100_0002);
    }

    #[test]
    fn completion_sets_dicr_flag_only_when_channel_is_enabled() {
        let mut dma = DmaController::new();

        assert!(!dma.complete_channel(2, None, None, false));
        assert_eq!(dma.read32(0x74) & ((1 << 31) | (1 << 26)), 0);

        dma.write32(0x74, (1 << 23) | (1 << 18));
        assert!(dma.complete_channel(2, None, None, false));

        assert_ne!(dma.read32(0x74) & (1 << 26), 0);
        assert_ne!(dma.read32(0x74) & (1 << 31), 0);

        dma.write32(0x74, 1 << 26);
        assert_eq!(dma.read32(0x74) & ((1 << 31) | (1 << 26)), 0);
    }

    #[test]
    fn dicr_bus_error_forces_master_irq_flag() {
        let mut dma = DmaController::new();

        let result = dma.write32(0x74, 1 << 15);

        assert!(result.irq_edge);
        assert_ne!(dma.read32(0x74) & (1 << 15), 0);
        assert_ne!(dma.read32(0x74) & (1 << 31), 0);
    }

    #[test]
    fn pending_channel_selection_uses_priority_then_high_channel_number() {
        let mut dma = DmaController::new();
        dma.write32(0x70, (1 << 11) | (1 << 27));
        dma.write32(0x28, 0x0100_0201);
        dma.write32(0x68, 0x1100_0002);

        assert_eq!(dma.next_pending_channel(), Some(6));
    }

    #[test]
    fn sync0_channel_waits_for_force_start_bit() {
        let mut dma = DmaController::new();
        dma.write32(0x70, 1 << 15);
        dma.write32(0x38, 1 << 24);

        assert_eq!(dma.next_pending_channel(), None);

        dma.write32(0x38, (1 << 24) | (1 << 28));

        assert_eq!(dma.next_pending_channel(), Some(3));
    }
}
