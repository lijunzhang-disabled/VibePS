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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchError {
    BusError,
}

const CACHE_CONTROL_ADDR: u32 = 0xfffe_0130;
const DEFAULT_CACHE_CONTROL: u32 = 0x0001_e988;
const ICACHE_WORDS: usize = 1024;
const ICACHE_LINES: usize = 256;
const BCC_TAG: u32 = 1 << 2;
const BCC_IS1: u32 = 1 << 11;
const BCC_IBLKSZ_MASK: u32 = 0x0300;

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

    pub fn fetch32(&mut self, addr: u32) -> Result<u32, FetchError> {
        if self.instruction_bus_error(addr) {
            return Err(FetchError::BusError);
        }

        if !self.instruction_cache_enabled(addr) {
            return Ok(self.read32(addr));
        }

        let phys = Self::physical_addr(addr);
        let line = icache_line_index(phys);
        let word = ((phys >> 2) & 3) as usize;
        let tag = self.icache_tags[line];
        let tag_addr = phys & 0xffff_f000;
        let valid_bit = 1u32 << word;
        if (tag & 0xffff_f000) != tag_addr || (tag & valid_bit) == 0 {
            self.fill_icache_line(addr);
        }

        Ok(self.icache_words[icache_word_index(phys)])
    }

    pub fn data_bus_error(&self, addr: u32) -> bool {
        !is_data_accessible_phys(Self::physical_addr(addr))
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

    fn instruction_cache_enabled(&self, addr: u32) -> bool {
        (self.cache_control & BCC_IS1) != 0
            && matches!(addr, 0x0000_0000..=0x1fff_ffff | 0x8000_0000..=0x9fff_ffff)
    }

    fn instruction_bus_error(&self, addr: u32) -> bool {
        matches!(addr, 0xc000_0000..=0xffff_ffff)
            || !is_instruction_accessible_phys(Self::physical_addr(addr))
    }

    fn fill_icache_line(&mut self, addr: u32) {
        let phys = Self::physical_addr(addr);
        let line = icache_line_index(phys);
        let word = ((phys >> 2) & 3) as usize;
        let tag_addr = phys & 0xffff_f000;
        let old_valid = if (self.icache_tags[line] & 0xffff_f000) == tag_addr {
            self.icache_tags[line] & 0x0f
        } else {
            0
        };
        let last_word = if word == 0 && (self.cache_control & BCC_IBLKSZ_MASK) == 0 {
            1
        } else {
            3
        };
        let line_base_addr = addr & !0x0f;
        let mut valid = old_valid;
        for slot in word..=last_word {
            let fetch_addr = line_base_addr.wrapping_add((slot as u32) * 4);
            self.icache_words[icache_word_index(fetch_addr)] = self.read32(fetch_addr);
            valid |= 1 << slot;
        }
        self.icache_tags[line] = tag_addr | valid;
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
                let result = self.dma.write32(phys - 0x1f80_1080, value);
                if result.irq_edge {
                    self.irq.request(IRQ_DMA);
                }
                self.run_pending_dma();
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

    fn run_pending_dma(&mut self) {
        let mut guard = 0usize;
        while let Some(channel) = self.dma.next_pending_channel() {
            self.run_dma(channel);
            guard += 1;
            if guard > 32 {
                break;
            }
        }
    }

    fn run_dma(&mut self, channel: usize) {
        let ch = self.dma.channel(channel);
        let result = match channel {
            0 => self.run_mdec_in_dma(ch),
            1 => self.run_mdec_out_dma(ch),
            2 if ch.sync_mode() == 2 && ch.from_ram() => self.run_gpu_linked_list_dma(ch.madr),
            2 => self.run_gpu_dma(ch),
            3 => self.run_cdrom_dma(ch),
            4 => self.run_spu_dma(ch),
            6 => self.run_otc_dma(ch),
            _ => DmaTransferResult::default(),
        };
        if self.dma.complete_channel(
            channel,
            result.final_madr,
            result.final_bcr,
            result.bus_error,
        ) {
            self.irq.request(IRQ_DMA);
        }
    }

    fn run_mdec_in_dma(&mut self, ch: crate::dma::DmaChannel) -> DmaTransferResult {
        if !ch.from_ram() {
            return DmaTransferResult::default();
        }
        let mut addr = ch.madr & 0x001f_fffc;
        for _ in 0..dma_word_count(ch.bcr, ch.sync_mode()) {
            let word = self.read_ram32(addr);
            self.mdec.write_data(word);
            addr = addr.wrapping_add(4) & 0x001f_fffc;
        }
        dma_transfer_result(ch, addr)
    }

    fn run_mdec_out_dma(&mut self, ch: crate::dma::DmaChannel) -> DmaTransferResult {
        if ch.from_ram() {
            return DmaTransferResult::default();
        }
        let mut addr = ch.madr & 0x001f_fffc;
        for _ in 0..dma_word_count(ch.bcr, ch.sync_mode()) {
            let word = self.mdec.read_data();
            self.write_ram32(addr, word);
            addr = addr.wrapping_add(4) & 0x001f_fffc;
        }
        dma_transfer_result(ch, addr)
    }

    fn run_gpu_linked_list_dma(&mut self, start: u32) -> DmaTransferResult {
        let mut addr = start & 0x001f_fffc;
        let mut guard = 0usize;
        let final_madr = loop {
            let header = self.read_ram32(addr);
            let words = (header >> 24) as usize;
            for i in 0..words {
                let word = self.read_ram32(addr.wrapping_add(4 + (i as u32 * 4)));
                self.gpu.write_gp0(word);
            }
            if (header & 0x0080_0000) != 0 {
                break header & 0x00ff_fffc;
            }
            addr = header & 0x001f_fffc;
            guard += 1;
            if guard > 0x20_000 {
                break addr;
            }
        };
        DmaTransferResult {
            final_madr: Some(final_madr),
            ..DmaTransferResult::default()
        }
    }

    fn run_gpu_dma(&mut self, ch: crate::dma::DmaChannel) -> DmaTransferResult {
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
        dma_transfer_result(ch, addr)
    }

    fn run_cdrom_dma(&mut self, ch: crate::dma::DmaChannel) -> DmaTransferResult {
        if ch.from_ram() {
            return DmaTransferResult::default();
        }
        let mut addr = ch.madr & 0x001f_fffc;
        for _ in 0..dma_word_count(ch.bcr, ch.sync_mode()) {
            let word = self.cdrom.dma_read32();
            self.write_ram32(addr, word);
            addr = addr.wrapping_add(4) & 0x001f_fffc;
        }
        dma_transfer_result(ch, addr)
    }

    fn run_spu_dma(&mut self, ch: crate::dma::DmaChannel) -> DmaTransferResult {
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
        dma_transfer_result(ch, addr)
    }

    fn run_otc_dma(&mut self, ch: crate::dma::DmaChannel) -> DmaTransferResult {
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
        DmaTransferResult::default()
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
            dma_count_part(words)
        }
        1 => {
            let block_size = dma_count_part(bcr & 0xffff);
            let block_count = dma_count_part((bcr >> 16) & 0xffff);
            block_size.saturating_mul(block_count)
        }
        _ => 0,
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct DmaTransferResult {
    final_madr: Option<u32>,
    final_bcr: Option<u32>,
    bus_error: bool,
}

fn dma_transfer_result(ch: crate::dma::DmaChannel, final_addr: u32) -> DmaTransferResult {
    match ch.sync_mode() {
        1 => DmaTransferResult {
            final_madr: Some(final_addr),
            final_bcr: Some(ch.bcr & 0x0000_ffff),
            ..DmaTransferResult::default()
        },
        _ => DmaTransferResult::default(),
    }
}

fn dma_count_part(value: u32) -> u32 {
    if value == 0 {
        0x1_0000
    } else {
        value
    }
}

fn is_instruction_accessible_phys(phys: u32) -> bool {
    matches!(
        phys,
        0x0000_0000..=0x007f_ffff
            | 0x1f00_0000..=0x1f7f_ffff
            | 0x1fa0_0000..=0x1fbf_ffff
            | 0x1fc0_0000..=0x1fc7_ffff
    )
}

fn is_data_accessible_phys(phys: u32) -> bool {
    matches!(
        phys,
        0x0000_0000..=0x007f_ffff
            | 0x1f00_0000..=0x1f7f_ffff
            | 0x1f80_0000..=0x1f80_03ff
            | 0x1f80_1000..=0x1f80_3fff
            | 0x1fa0_0000..=0x1fbf_ffff
            | 0x1fc0_0000..=0x1fc7_ffff
            | CACHE_CONTROL_ADDR..=0xfffe_0133
    )
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
    use crate::interrupt::IRQ_DMA;

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

    #[test]
    fn dma2_linked_list_sends_gp0_packets_and_updates_madr() {
        let mut bus = Bus::new(None);
        bus.write32(0x0000_0100, 0x03ff_ffff);
        bus.write32(0x0000_0104, 0x0200_00ff); // fill rect, red
        bus.write32(0x0000_0108, 0x0000_0000); // xy
        bus.write32(0x0000_010c, 0x0001_0001); // 16x1 after fill alignment
        bus.write32(0x1f80_10f0, 1 << 11);
        bus.write32(0x1f80_10a0, 0x0000_0100);

        bus.write32(0x1f80_10a8, 0x0100_0401);

        assert_eq!(bus.gpu.vram()[0], 0x001f);
        assert_eq!(bus.gpu.vram()[15], 0x001f);
        assert_eq!(bus.gpu.vram()[16], 0x0000);
        assert_eq!(bus.read32(0x1f80_10a0), 0x00ff_fffc);
        assert_eq!(bus.read32(0x1f80_10a8) & (1 << 24), 0);
    }

    #[test]
    fn dma2_sync1_vram_upload_updates_madr_and_block_count() {
        let mut bus = Bus::new(None);
        bus.write32(0x0000_0100, 0xa000_0000); // cpu-to-vram
        bus.write32(0x0000_0104, 0x0000_0000); // xy
        bus.write32(0x0000_0108, 0x0001_0002); // width=2,height=1
        bus.write32(0x0000_010c, 0x2222_1111);
        bus.write32(0x1f80_10f0, 1 << 11);
        bus.write32(0x1f80_10a0, 0x0000_0100);
        bus.write32(0x1f80_10a4, (1 << 16) | 4);

        bus.write32(0x1f80_10a8, 0x0100_0201);

        assert_eq!(bus.gpu.vram()[0], 0x1111);
        assert_eq!(bus.gpu.vram()[1], 0x2222);
        assert_eq!(bus.read32(0x1f80_10a0), 0x0000_0110);
        assert_eq!(bus.read32(0x1f80_10a4), 0x0000_0004);
    }

    #[test]
    fn dma6_otc_clears_ordering_table_and_requests_irq_when_enabled() {
        let mut bus = Bus::new(None);
        bus.write32(0x1f80_10f0, 1 << 27);
        bus.write32(0x1f80_10f4, (1 << 23) | (1 << 22));
        bus.write32(0x1f80_10e0, 0x0000_010c);
        bus.write32(0x1f80_10e4, 4);

        bus.write32(0x1f80_10e8, 0x1100_0002);

        assert_eq!(bus.read32(0x0000_010c), 0x0000_0108);
        assert_eq!(bus.read32(0x0000_0108), 0x0000_0104);
        assert_eq!(bus.read32(0x0000_0104), 0x0000_0100);
        assert_eq!(bus.read32(0x0000_0100), 0x00ff_ffff);
        assert_ne!(bus.read32(0x1f80_10f4) & ((1 << 31) | (1 << 30)), 0);
        assert_ne!(bus.irq.status() & IRQ_DMA, 0);
    }
}
