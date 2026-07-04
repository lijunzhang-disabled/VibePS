use crate::audio::Spu;
use crate::cdrom::Cdrom;
use crate::dma::DmaController;
use crate::gpu::Gpu;
use crate::interrupt::{InterruptController, IRQ_CDROM, IRQ_DMA};
use crate::mdec::Mdec;
use crate::timer::Timers;
use crate::{BIOS_SIZE, MAIN_RAM_SIZE, SCRATCHPAD_SIZE};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyToRamError {
    OutOfRange,
}

const CACHE_CONTROL_ADDR: u32 = 0xfffe_0130;
const DEFAULT_CACHE_CONTROL: u32 = 0x0001_e988;
const ICACHE_WORDS: usize = 1024;
const ICACHE_LINES: usize = 256;
const BCC_TAG: u32 = 1 << 2;
const BCC_IS1: u32 = 1 << 11;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bus {
    ram: Vec<u8>,
    scratchpad: Vec<u8>,
    bios: Vec<u8>,
    io: Vec<u8>,
    cache_control: u32,
    icache_words: Vec<u32>,
    icache_tags: Vec<u32>,
    pub irq: InterruptController,
    pub dma: DmaController,
    pub timers: Timers,
    pub gpu: Gpu,
    pub spu: Spu,
    pub cdrom: Cdrom,
    pub mdec: Mdec,
    open_bus: u32,
}

impl Bus {
    pub fn new(bios: Option<Vec<u8>>) -> Self {
        let mut bios_rom = vec![0xff; BIOS_SIZE];
        if let Some(bytes) = bios {
            let len = bytes.len().min(BIOS_SIZE);
            bios_rom[..len].copy_from_slice(&bytes[..len]);
        }

        Self {
            ram: vec![0; MAIN_RAM_SIZE],
            scratchpad: vec![0; SCRATCHPAD_SIZE],
            bios: bios_rom,
            io: vec![0; 0x2000],
            cache_control: DEFAULT_CACHE_CONTROL,
            icache_words: vec![0; ICACHE_WORDS],
            icache_tags: vec![0; ICACHE_LINES],
            irq: InterruptController::new(),
            dma: DmaController::new(),
            timers: Timers::new(),
            gpu: Gpu::new(),
            spu: Spu::new(),
            cdrom: Cdrom::new(),
            mdec: Mdec::new(),
            open_bus: 0,
        }
    }

    pub fn physical_addr(addr: u32) -> u32 {
        match addr {
            0x0000_0000..=0x1fff_ffff => addr,
            0x8000_0000..=0x9fff_ffff => addr - 0x8000_0000,
            0xa000_0000..=0xbfff_ffff => addr - 0xa000_0000,
            _ => addr,
        }
    }

    pub fn copy_to_ram(&mut self, addr: u32, bytes: &[u8]) -> Result<(), CopyToRamError> {
        let phys = Self::physical_addr(addr);
        let start = (phys as usize) & (MAIN_RAM_SIZE - 1);
        let end = start
            .checked_add(bytes.len())
            .ok_or(CopyToRamError::OutOfRange)?;
        if end > MAIN_RAM_SIZE {
            return Err(CopyToRamError::OutOfRange);
        }
        self.ram[start..end].copy_from_slice(bytes);
        Ok(())
    }

    pub fn read8(&mut self, addr: u32) -> u8 {
        let phys = Self::physical_addr(addr);
        let value = match phys {
            0x0000_0000..=0x007f_ffff => self.ram[(phys as usize) & (MAIN_RAM_SIZE - 1)],
            0x1f80_0000..=0x1f80_03ff => {
                self.scratchpad[(phys as usize - 0x1f80_0000) & (SCRATCHPAD_SIZE - 1)]
            }
            0x1f80_1000..=0x1f80_1fff => self.read_io8(phys),
            0x1f80_2000..=0x1f80_3fff => 0xff,
            0x1fc0_0000..=0x1fc7_ffff => self.bios[(phys as usize - 0x1fc0_0000) & (BIOS_SIZE - 1)],
            CACHE_CONTROL_ADDR..=0xfffe_0133 => {
                ((self.cache_control >> ((phys - CACHE_CONTROL_ADDR) * 8)) & 0xff) as u8
            }
            _ => self.open_bus as u8,
        };
        self.open_bus = (self.open_bus & 0xffff_ff00) | value as u32;
        value
    }

    pub fn read16(&mut self, addr: u32) -> u16 {
        let phys = Self::physical_addr(addr);
        let value = if (0x1f80_1000..=0x1f80_1fff).contains(&phys) {
            self.read_io16(phys)
        } else {
            let lo = self.read8(addr) as u16;
            let hi = self.read8(addr.wrapping_add(1)) as u16;
            lo | (hi << 8)
        };
        self.open_bus = (self.open_bus & 0xffff_0000) | value as u32;
        value
    }

    pub fn read32(&mut self, addr: u32) -> u32 {
        let phys = Self::physical_addr(addr);
        let value = if phys == CACHE_CONTROL_ADDR {
            self.cache_control
        } else if (0x1f80_1000..=0x1f80_1fff).contains(&phys) {
            self.read_io32(phys)
        } else {
            let b0 = self.read8(addr) as u32;
            let b1 = self.read8(addr.wrapping_add(1)) as u32;
            let b2 = self.read8(addr.wrapping_add(2)) as u32;
            let b3 = self.read8(addr.wrapping_add(3)) as u32;
            b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
        };
        self.open_bus = value;
        value
    }

    pub fn peek32(&self, addr: u32) -> u32 {
        let b0 = self.peek8(addr) as u32;
        let b1 = self.peek8(addr.wrapping_add(1)) as u32;
        let b2 = self.peek8(addr.wrapping_add(2)) as u32;
        let b3 = self.peek8(addr.wrapping_add(3)) as u32;
        b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
    }

    pub fn write8(&mut self, addr: u32, value: u8) {
        let phys = Self::physical_addr(addr);
        match phys {
            0x0000_0000..=0x007f_ffff => {
                self.ram[(phys as usize) & (MAIN_RAM_SIZE - 1)] = value;
            }
            0x1f80_0000..=0x1f80_03ff => {
                let index = (phys as usize - 0x1f80_0000) & (SCRATCHPAD_SIZE - 1);
                self.scratchpad[index] = value;
            }
            0x1f80_1000..=0x1f80_1fff => self.write_io8(phys, value),
            CACHE_CONTROL_ADDR..=0xfffe_0133 => {
                let shift = (phys - CACHE_CONTROL_ADDR) * 8;
                self.cache_control =
                    (self.cache_control & !(0xff << shift)) | ((value as u32) << shift);
            }
            _ => {}
        }
        self.open_bus = (self.open_bus & 0xffff_ff00) | value as u32;
    }

    pub fn write16(&mut self, addr: u32, value: u16) {
        let phys = Self::physical_addr(addr);
        if (CACHE_CONTROL_ADDR..=0xfffe_0132).contains(&phys) {
            self.write8(addr, value as u8);
            self.write8(addr.wrapping_add(1), (value >> 8) as u8);
        } else if (0x1f80_1000..=0x1f80_1fff).contains(&phys) {
            self.write_io16(phys, value);
        } else {
            self.write8(addr, value as u8);
            self.write8(addr.wrapping_add(1), (value >> 8) as u8);
        }
        self.open_bus = (self.open_bus & 0xffff_0000) | value as u32;
    }

    pub fn write32(&mut self, addr: u32, value: u32) {
        let phys = Self::physical_addr(addr);
        if phys == CACHE_CONTROL_ADDR {
            self.cache_control = value;
        } else if (0x1f80_1000..=0x1f80_1fff).contains(&phys) {
            self.write_io32(phys, value);
        } else {
            self.write8(addr, value as u8);
            self.write8(addr.wrapping_add(1), (value >> 8) as u8);
            self.write8(addr.wrapping_add(2), (value >> 16) as u8);
            self.write8(addr.wrapping_add(3), (value >> 24) as u8);
        }
        self.open_bus = value;
    }

    pub fn tick(&mut self, cycles: u32) {
        self.timers.tick(cycles, &mut self.irq);
        if self.cdrom.interrupt_pending() {
            self.irq.request(IRQ_CDROM);
        }
    }

    pub fn cache_control(&self) -> u32 {
        self.cache_control
    }

    pub fn isolated_cache_read8(&self, addr: u32) -> u8 {
        ((self.isolated_cache_read32(addr & !3) >> ((addr & 3) * 8)) & 0xff) as u8
    }

    pub fn isolated_cache_read16(&self, addr: u32) -> u16 {
        let lo = self.isolated_cache_read8(addr) as u16;
        let hi = self.isolated_cache_read8(addr.wrapping_add(1)) as u16;
        lo | (hi << 8)
    }

    pub fn isolated_cache_read32(&self, addr: u32) -> u32 {
        if (self.cache_control & BCC_IS1) == 0 {
            return 0;
        }

        let word_index = icache_word_index(addr);
        if (self.cache_control & BCC_TAG) != 0 {
            let tag = self.icache_tags[icache_line_index(addr)];
            let match_bit = if (tag & 0xffff_f000) == (addr & 0xffff_f000) {
                0x10
            } else {
                0
            };
            (self.icache_words[word_index] & !0x1f) | (tag & 0x0f) | match_bit
        } else {
            self.icache_words[word_index]
        }
    }

    pub fn isolated_cache_write8(&mut self, addr: u32, value: u8) {
        if (self.cache_control & BCC_TAG) != 0 {
            self.isolated_cache_write32(addr, value as u32);
            return;
        }

        let aligned = addr & !3;
        let shift = (addr & 3) * 8;
        let word =
            (self.isolated_cache_read32(aligned) & !(0xff << shift)) | ((value as u32) << shift);
        self.isolated_cache_write32(aligned, word);
    }

    pub fn isolated_cache_write16(&mut self, addr: u32, value: u16) {
        if (self.cache_control & BCC_TAG) != 0 {
            self.isolated_cache_write32(addr, value as u32);
            return;
        }

        self.isolated_cache_write8(addr, value as u8);
        self.isolated_cache_write8(addr.wrapping_add(1), (value >> 8) as u8);
    }

    pub fn isolated_cache_write32(&mut self, addr: u32, value: u32) {
        if (self.cache_control & BCC_IS1) == 0 {
            return;
        }

        if (self.cache_control & BCC_TAG) != 0 {
            self.icache_tags[icache_line_index(addr)] = (value & 0x0f) | (addr & 0xffff_f000);
        } else {
            self.icache_words[icache_word_index(addr)] = value;
        }
    }

    fn peek8(&self, addr: u32) -> u8 {
        let phys = Self::physical_addr(addr);
        match phys {
            0x0000_0000..=0x007f_ffff => self.ram[(phys as usize) & (MAIN_RAM_SIZE - 1)],
            0x1f80_0000..=0x1f80_03ff => {
                self.scratchpad[(phys as usize - 0x1f80_0000) & (SCRATCHPAD_SIZE - 1)]
            }
            0x1f80_1000..=0x1f80_1fff => self.io[(phys as usize - 0x1f80_1000) & 0x1fff],
            0x1fc0_0000..=0x1fc7_ffff => self.bios[(phys as usize - 0x1fc0_0000) & (BIOS_SIZE - 1)],
            CACHE_CONTROL_ADDR..=0xfffe_0133 => {
                ((self.cache_control >> ((phys - CACHE_CONTROL_ADDR) * 8)) & 0xff) as u8
            }
            _ => ((self.open_bus >> ((addr & 3) * 8)) & 0xff) as u8,
        }
    }

    fn read_io8(&mut self, phys: u32) -> u8 {
        if (0x1f80_1800..=0x1f80_1803).contains(&phys) {
            return self.cdrom.read8(phys - 0x1f80_1800);
        }
        let word = self.read_io32(phys & !3);
        ((word >> ((phys & 3) * 8)) & 0xff) as u8
    }

    fn read_io16(&mut self, phys: u32) -> u16 {
        match phys {
            0x1f80_1070 => self.irq.status(),
            0x1f80_1074 => self.irq.mask(),
            0x1f80_1100..=0x1f80_112f => self.timers.read16(phys - 0x1f80_1100),
            0x1f80_1c00..=0x1f80_1fff => self.spu.read16(phys - 0x1f80_1c00),
            _ => {
                let lo = self.io[(phys as usize - 0x1f80_1000) & 0x1fff] as u16;
                let hi = self.io[(phys as usize + 1 - 0x1f80_1000) & 0x1fff] as u16;
                lo | (hi << 8)
            }
        }
    }

    fn read_io32(&mut self, phys: u32) -> u32 {
        match phys {
            0x1f80_1070 => self.irq.status() as u32,
            0x1f80_1074 => self.irq.mask() as u32,
            0x1f80_1080..=0x1f80_10ff => self.dma.read32(phys - 0x1f80_1080),
            0x1f80_1100..=0x1f80_112f => {
                let lo = self.timers.read16(phys - 0x1f80_1100) as u32;
                let hi = self.timers.read16((phys + 2) - 0x1f80_1100) as u32;
                lo | (hi << 16)
            }
            0x1f80_1810 => self.gpu.read_gp0(),
            0x1f80_1814 => self.gpu.read_gp1(),
            0x1f80_1820 => self.mdec.read_data(),
            0x1f80_1824 => self.mdec.read_status(),
            _ => {
                let offset = (phys as usize - 0x1f80_1000) & 0x1fff;
                u32::from_le_bytes([
                    self.io[offset],
                    self.io[(offset + 1) & 0x1fff],
                    self.io[(offset + 2) & 0x1fff],
                    self.io[(offset + 3) & 0x1fff],
                ])
            }
        }
    }

    fn write_io8(&mut self, phys: u32, value: u8) {
        if (0x1f80_1800..=0x1f80_1803).contains(&phys) {
            self.cdrom.write8(phys - 0x1f80_1800, value);
            return;
        }
        let offset = (phys as usize - 0x1f80_1000) & 0x1fff;
        self.io[offset] = value;
    }

    fn write_io16(&mut self, phys: u32, value: u16) {
        match phys {
            0x1f80_1070 => self.irq.acknowledge(value),
            0x1f80_1074 => self.irq.set_mask(value),
            0x1f80_1100..=0x1f80_112f => self.timers.write16(phys - 0x1f80_1100, value),
            0x1f80_1c00..=0x1f80_1fff => self.spu.write16(phys - 0x1f80_1c00, value),
            _ => {
                let offset = (phys as usize - 0x1f80_1000) & 0x1fff;
                self.io[offset] = value as u8;
                self.io[(offset + 1) & 0x1fff] = (value >> 8) as u8;
            }
        }
    }

    fn write_io32(&mut self, phys: u32, value: u32) {
        match phys {
            0x1f80_1070 => self.irq.acknowledge(value as u16),
            0x1f80_1074 => self.irq.set_mask(value as u16),
            0x1f80_1080..=0x1f80_10ff => {
                if let Some(channel) = self.dma.write32(phys - 0x1f80_1080, value) {
                    self.run_dma(channel);
                }
            }
            0x1f80_1100..=0x1f80_112f => self.timers.write16(phys - 0x1f80_1100, value as u16),
            0x1f80_1810 => self.gpu.write_gp0(value),
            0x1f80_1814 => self.gpu.write_gp1(value),
            0x1f80_1820 => self.mdec.write_data(value),
            0x1f80_1824 => self.mdec.write_control(value),
            _ => {
                let offset = (phys as usize - 0x1f80_1000) & 0x1fff;
                let bytes = value.to_le_bytes();
                for (i, byte) in bytes.into_iter().enumerate() {
                    self.io[(offset + i) & 0x1fff] = byte;
                }
            }
        }
    }

    fn run_dma(&mut self, channel: usize) {
        if !self.dma.master_enabled(channel) {
            return;
        }
        let ch = self.dma.channel(channel);
        match channel {
            2 if ch.sync_mode() == 2 && ch.from_ram() => self.run_gpu_linked_list_dma(ch.madr),
            2 => self.run_gpu_dma(ch),
            3 => self.run_cdrom_dma(ch),
            4 => self.run_spu_dma(ch),
            6 => self.run_otc_dma(ch),
            _ => {}
        }
        if self.dma.complete_channel(channel) {
            self.irq.request(IRQ_DMA);
        }
    }

    fn run_gpu_linked_list_dma(&mut self, start: u32) {
        let mut addr = start & 0x001f_fffc;
        let mut guard = 0usize;
        loop {
            let header = self.read_ram32(addr);
            let words = (header >> 24) as usize;
            for i in 0..words {
                let word = self.read_ram32(addr.wrapping_add(4 + (i as u32 * 4)));
                self.gpu.write_gp0(word);
            }
            if (header & 0x0080_0000) != 0 {
                break;
            }
            addr = header & 0x001f_fffc;
            guard += 1;
            if guard > 0x20_000 {
                break;
            }
        }
    }

    fn run_gpu_dma(&mut self, ch: crate::dma::DmaChannel) {
        let mut addr = ch.madr & 0x001f_fffc;
        let count = dma_word_count(ch.bcr, ch.sync_mode());
        let step = if ch.step_backwards() { -4i32 } else { 4i32 };
        for _ in 0..count {
            if ch.from_ram() {
                let word = self.read_ram32(addr);
                self.gpu.write_gp0(word);
            } else {
                let word = self.gpu.read_gp0();
                self.write_ram32(addr, word);
            }
            addr = addr.wrapping_add(step as u32) & 0x001f_fffc;
        }
    }

    fn run_cdrom_dma(&mut self, ch: crate::dma::DmaChannel) {
        if ch.from_ram() {
            return;
        }
        let mut addr = ch.madr & 0x001f_fffc;
        for _ in 0..dma_word_count(ch.bcr, ch.sync_mode()) {
            let word = self.cdrom.dma_read32();
            self.write_ram32(addr, word);
            addr = addr.wrapping_add(4) & 0x001f_fffc;
        }
    }

    fn run_spu_dma(&mut self, ch: crate::dma::DmaChannel) {
        let mut addr = ch.madr & 0x001f_fffc;
        for _ in 0..dma_word_count(ch.bcr, ch.sync_mode()) {
            if ch.from_ram() {
                let word = self.read_ram32(addr);
                self.spu.dma_write32(word);
            } else {
                let word = self.spu.dma_read32();
                self.write_ram32(addr, word);
            }
            addr = addr.wrapping_add(4) & 0x001f_fffc;
        }
    }

    fn run_otc_dma(&mut self, ch: crate::dma::DmaChannel) {
        let count = (ch.bcr & 0xffff).max(1);
        let mut addr = ch.madr & 0x001f_fffc;
        for i in 0..count {
            let value = if i == count - 1 {
                0x00ff_ffff
            } else {
                addr.wrapping_sub(4) & 0x001f_ffff
            };
            self.write_ram32(addr, value);
            addr = addr.wrapping_sub(4) & 0x001f_fffc;
        }
    }

    fn read_ram32(&self, addr: u32) -> u32 {
        let index = (addr as usize) & (MAIN_RAM_SIZE - 1);
        u32::from_le_bytes([
            self.ram[index],
            self.ram[(index + 1) & (MAIN_RAM_SIZE - 1)],
            self.ram[(index + 2) & (MAIN_RAM_SIZE - 1)],
            self.ram[(index + 3) & (MAIN_RAM_SIZE - 1)],
        ])
    }

    fn write_ram32(&mut self, addr: u32, value: u32) {
        let index = (addr as usize) & (MAIN_RAM_SIZE - 1);
        let bytes = value.to_le_bytes();
        for (i, byte) in bytes.into_iter().enumerate() {
            self.ram[(index + i) & (MAIN_RAM_SIZE - 1)] = byte;
        }
    }
}

impl Default for Bus {
    fn default() -> Self {
        Self::new(None)
    }
}

fn dma_word_count(bcr: u32, sync_mode: u32) -> u32 {
    match sync_mode {
        0 => {
            let words = bcr & 0xffff;
            if words == 0 {
                0x1_0000
            } else {
                words
            }
        }
        1 => {
            let block_size = bcr & 0xffff;
            let block_count = (bcr >> 16) & 0xffff;
            block_size.max(1) * block_count.max(1)
        }
        _ => 0,
    }
}

fn icache_word_index(addr: u32) -> usize {
    ((addr as usize) & 0x0fff) >> 2
}

fn icache_line_index(addr: u32) -> usize {
    ((addr as usize) & 0x0fff) >> 4
}

#[cfg(test)]
mod tests {
    use super::{Bus, BCC_IS1, BCC_TAG, DEFAULT_CACHE_CONTROL};

    #[test]
    fn maps_ram_through_kseg0_and_kseg1() {
        let mut bus = Bus::new(None);
        bus.write32(0x8000_0100, 0x1234_5678);
        assert_eq!(bus.read32(0x0000_0100), 0x1234_5678);
        assert_eq!(bus.read32(0xa000_0100), 0x1234_5678);
    }

    #[test]
    fn maps_scratchpad() {
        let mut bus = Bus::new(None);
        bus.write32(0x1f80_0000, 0xfeed_beef);
        assert_eq!(bus.read32(0x9f80_0000), 0xfeed_beef);
    }

    #[test]
    fn reads_bios_from_boot_mirror() {
        let mut bios = vec![0xff; crate::BIOS_SIZE];
        bios[0] = 0x12;
        bios[1] = 0x34;
        let mut bus = Bus::new(Some(bios));
        assert_eq!(bus.read16(0xbfc0_0000), 0x3412);
    }

    #[test]
    fn peek32_reads_memory_without_changing_open_bus() {
        let mut bus = Bus::new(None);
        bus.write32(0x8000_0100, 0x1234_5678);
        bus.write32(0x8000_0200, 0xaabb_ccdd);
        let open_bus = bus.open_bus;

        assert_eq!(bus.peek32(0x8000_0100), 0x1234_5678);
        assert_eq!(bus.open_bus, open_bus);
    }

    #[test]
    fn cache_control_register_round_trips() {
        let mut bus = Bus::new(None);

        assert_eq!(bus.read32(0xfffe_0130), DEFAULT_CACHE_CONTROL);
        bus.write32(0xfffe_0130, 0x0000_0804);
        assert_eq!(bus.cache_control(), 0x0000_0804);
        assert_eq!(bus.read32(0xfffe_0130), 0x0000_0804);
    }

    #[test]
    fn isolated_cache_tag_and_code_modes_do_not_touch_ram() {
        let mut bus = Bus::new(None);

        bus.write32(0xfffe_0130, BCC_IS1 | BCC_TAG);
        bus.isolated_cache_write32(0x0000_0000, 0);
        assert_eq!(bus.isolated_cache_read32(0x0000_0000) & 0x1f, 0x10);
        bus.isolated_cache_write32(0x0000_0004, 0x0f);
        assert_eq!(bus.isolated_cache_read32(0x0000_0000) & 0x1f, 0x1f);

        bus.write32(0xfffe_0130, BCC_IS1);
        bus.isolated_cache_write32(0x0000_0004, 0x1234_5678);

        assert_eq!(bus.isolated_cache_read32(0x0000_0004), 0x1234_5678);
        assert_eq!(bus.read32(0x0000_0004), 0);
    }
}
