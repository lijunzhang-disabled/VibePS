use super::envelope::VolumeEnvelope;
use super::reverb::Reverb;
use super::voice::Voice;
use crate::{CPU_CLOCK_HZ, SPU_RAM_SIZE};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub const SPU_OUTPUT_HZ: u32 = 44_100;
pub const SPU_CYCLES_PER_SAMPLE: u32 = CPU_CLOCK_HZ / SPU_OUTPUT_HZ;

const VOICE_COUNT: usize = 24;
const OUTPUT_CAPACITY: usize = SPU_OUTPUT_HZ as usize * 2;
const RAM_MASK: u32 = SPU_RAM_SIZE as u32 - 1;

const REG_MAIN_VOLUME_LEFT: u32 = 0x180;
const REG_MAIN_VOLUME_RIGHT: u32 = 0x182;
const REG_REVERB_VOLUME_LEFT: u32 = 0x184;
const REG_REVERB_VOLUME_RIGHT: u32 = 0x186;
const REG_KEY_ON_LOW: u32 = 0x188;
const REG_KEY_ON_HIGH: u32 = 0x18a;
const REG_KEY_OFF_LOW: u32 = 0x18c;
const REG_KEY_OFF_HIGH: u32 = 0x18e;
const REG_PITCH_MOD_LOW: u32 = 0x190;
const REG_NOISE_LOW: u32 = 0x194;
const REG_REVERB_ON_LOW: u32 = 0x198;
const REG_ENDX_LOW: u32 = 0x19c;
const REG_ENDX_HIGH: u32 = 0x19e;
const REG_REVERB_BASE: u32 = 0x1a2;
const REG_IRQ_ADDRESS: u32 = 0x1a4;
const REG_TRANSFER_ADDRESS: u32 = 0x1a6;
const REG_TRANSFER_FIFO: u32 = 0x1a8;
const REG_CONTROL: u32 = 0x1aa;
const REG_TRANSFER_CONTROL: u32 = 0x1ac;
const REG_STATUS: u32 = 0x1ae;
const REG_CD_VOLUME_LEFT: u32 = 0x1b0;
const REG_CD_VOLUME_RIGHT: u32 = 0x1b2;
const REG_CURRENT_MAIN_LEFT: u32 = 0x1b8;
const REG_CURRENT_MAIN_RIGHT: u32 = 0x1ba;
const REG_REVERB_CONFIG_START: u32 = 0x1c0;
const REG_INTERNAL_VOICE_START: u32 = 0x200;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Spu {
    registers: Vec<u16>,
    ram: Vec<u8>,
    voices: [Voice; VOICE_COUNT],
    main_volume_left: VolumeEnvelope,
    main_volume_right: VolumeEnvelope,
    reverb: Reverb,
    transfer_address: u32,
    transfer_fifo: VecDeque<u16>,
    control: u16,
    applied_control: u16,
    end_flags: u32,
    cycle_accumulator: u32,
    output: VecDeque<i16>,
    cd_input: VecDeque<i16>,
    capture_offset: u16,
    noise_level: u16,
    noise_timer: i32,
    irq_flag: bool,
    irq_edge_pending: bool,
}

impl Spu {
    pub fn new() -> Self {
        let mut registers = vec![0; 0x200];
        registers[REG_TRANSFER_CONTROL as usize / 2] = 4;
        Self {
            registers,
            ram: vec![0; SPU_RAM_SIZE],
            voices: std::array::from_fn(|_| Voice::new()),
            main_volume_left: VolumeEnvelope::new(),
            main_volume_right: VolumeEnvelope::new(),
            reverb: Reverb::new(),
            transfer_address: 0,
            transfer_fifo: VecDeque::with_capacity(32),
            control: 0,
            applied_control: 0,
            end_flags: 0,
            cycle_accumulator: 0,
            output: VecDeque::with_capacity(8192),
            cd_input: VecDeque::with_capacity(4096),
            capture_offset: 0,
            noise_level: 1,
            noise_timer: 0,
            irq_flag: false,
            irq_edge_pending: false,
        }
    }

    pub fn read16(&self, offset: u32) -> u16 {
        let offset = offset & 0x3fe;
        if offset < REG_MAIN_VOLUME_LEFT {
            let voice = (offset / 0x10) as usize;
            return self.voices[voice].read_register(offset & 0x0f);
        }
        if (REG_INTERNAL_VOICE_START..REG_INTERNAL_VOICE_START + 0x60).contains(&offset) {
            let voice = ((offset - REG_INTERNAL_VOICE_START) / 4) as usize;
            return if (offset & 2) == 0 {
                self.voices[voice].volume_left() as u16
            } else {
                self.voices[voice].volume_right() as u16
            };
        }

        match offset {
            REG_ENDX_LOW => self.end_flags as u16,
            REG_ENDX_HIGH => (self.end_flags >> 16) as u16,
            REG_TRANSFER_ADDRESS => self.registers[offset as usize / 2],
            REG_TRANSFER_FIFO => 0,
            REG_CONTROL => self.control,
            REG_STATUS => self.status(),
            REG_CURRENT_MAIN_LEFT => self.main_volume_left.current() as u16,
            REG_CURRENT_MAIN_RIGHT => self.main_volume_right.current() as u16,
            _ => self.registers[offset as usize / 2],
        }
    }

    pub fn write16(&mut self, offset: u32, value: u16) {
        let offset = offset & 0x3fe;
        if offset < REG_MAIN_VOLUME_LEFT {
            let voice = (offset / 0x10) as usize;
            self.voices[voice].write_register(offset & 0x0f, value);
            return;
        }

        self.registers[offset as usize / 2] = value;
        match offset {
            REG_MAIN_VOLUME_LEFT => self.main_volume_left.write(value),
            REG_MAIN_VOLUME_RIGHT => self.main_volume_right.write(value),
            REG_KEY_ON_LOW | REG_KEY_ON_HIGH => self.key_on(write_mask(offset, value)),
            REG_KEY_OFF_LOW | REG_KEY_OFF_HIGH => self.key_off(write_mask(offset, value)),
            REG_ENDX_LOW | REG_ENDX_HIGH => {}
            REG_REVERB_BASE => self.reverb.set_base(value),
            REG_TRANSFER_ADDRESS => self.transfer_address = (value as u32) << 3,
            REG_TRANSFER_FIFO => self.write_transfer_fifo(value),
            REG_CONTROL => self.write_control(value),
            REG_STATUS => {}
            _ => {}
        }
    }

    pub fn dma_write32(&mut self, value: u32) {
        self.write_ram_halfword(value as u16);
        self.write_ram_halfword((value >> 16) as u16);
    }

    pub fn dma_read32(&mut self) -> u32 {
        let low = self.read_ram_halfword() as u32;
        let high = self.read_ram_halfword() as u32;
        low | (high << 16)
    }

    pub fn tick(&mut self, cycles: u32) -> bool {
        self.cycle_accumulator = self.cycle_accumulator.saturating_add(cycles);
        while self.cycle_accumulator >= SPU_CYCLES_PER_SAMPLE {
            self.cycle_accumulator -= SPU_CYCLES_PER_SAMPLE;
            self.produce_sample();
        }
        let edge = self.irq_edge_pending;
        self.irq_edge_pending = false;
        edge
    }

    pub fn drain_audio(&mut self, out: &mut [i16]) -> usize {
        let count = out.len().min(self.output.len());
        for sample in &mut out[..count] {
            *sample = self.output.pop_front().unwrap_or(0);
        }
        out[count..].fill(0);
        count
    }

    pub fn queued_samples(&self) -> usize {
        self.output.len()
    }

    pub fn push_cd_sample(&mut self, left: i16, right: i16) {
        self.cd_input.push_back(left);
        self.cd_input.push_back(right);
        while self.cd_input.len() > OUTPUT_CAPACITY {
            self.cd_input.pop_front();
            self.cd_input.pop_front();
        }
    }

    pub fn ram(&self) -> &[u8] {
        &self.ram
    }

    pub fn irq_pending(&self) -> bool {
        self.irq_flag
    }

    fn status(&self) -> u16 {
        let mut status = self.applied_control & 0x3f;
        let transfer_mode = (self.applied_control >> 4) & 3;
        if transfer_mode == 2 {
            status |= (1 << 8) | (1 << 7);
        } else if transfer_mode == 3 {
            status |= (1 << 9) | (1 << 7);
        }
        if self.irq_flag {
            status |= 1 << 6;
        }
        if (self.capture_offset & 0x200) != 0 {
            status |= 1 << 11;
        }
        status
    }

    fn write_control(&mut self, value: u16) {
        self.control = value;
        if (value & ((1 << 15) | (1 << 6))) != ((1 << 15) | (1 << 6)) {
            self.irq_flag = false;
            self.irq_edge_pending = false;
        }
    }

    fn key_on(&mut self, mask: u32) {
        let mask = mask & 0x00ff_ffff;
        self.end_flags &= !mask;
        for voice in 0..VOICE_COUNT {
            if (mask & (1 << voice)) != 0 {
                self.voices[voice].key_on();
            }
        }
    }

    fn key_off(&mut self, mask: u32) {
        for voice in 0..VOICE_COUNT {
            if (mask & (1 << voice)) != 0 {
                self.voices[voice].key_off();
            }
        }
    }

    fn write_transfer_fifo(&mut self, value: u16) {
        if self.transfer_fifo.len() == 32 {
            self.transfer_fifo.pop_front();
        }
        self.transfer_fifo.push_back(value);
    }

    fn flush_transfer_fifo(&mut self) {
        if self.transfer_fifo.is_empty() {
            return;
        }
        let input: Vec<u16> = self.transfer_fifo.drain(..).collect();
        let transfer_type = (self.registers[REG_TRANSFER_CONTROL as usize / 2] >> 1) & 7;
        let output = match transfer_type {
            2 => input,
            3 => repeat_groups(&input, 2, 0),
            4 => repeat_groups(&input, 4, 0),
            5 => repeat_groups(&input, 8, 7),
            _ => vec![*input.last().unwrap_or(&0); input.len()],
        };
        for halfword in output {
            self.write_ram_halfword(halfword);
        }
    }

    fn write_ram_halfword(&mut self, value: u16) {
        let address = self.transfer_address & RAM_MASK & !1;
        let bytes = value.to_le_bytes();
        self.ram[address as usize] = bytes[0];
        self.ram[(address as usize + 1) & (SPU_RAM_SIZE - 1)] = bytes[1];
        self.touch_irq(address);
        self.transfer_address = self.transfer_address.wrapping_add(2) & RAM_MASK;
    }

    fn read_ram_halfword(&mut self) -> u16 {
        let address = self.transfer_address & RAM_MASK & !1;
        let value = u16::from_le_bytes([
            self.ram[address as usize],
            self.ram[(address as usize + 1) & (SPU_RAM_SIZE - 1)],
        ]);
        self.touch_irq(address);
        self.transfer_address = self.transfer_address.wrapping_add(2) & RAM_MASK;
        value
    }

    fn produce_sample(&mut self) {
        self.applied_control = self.control;
        if ((self.applied_control >> 4) & 3) == 1 {
            self.flush_transfer_fifo();
        }

        self.main_volume_left.tick();
        self.main_volume_right.tick();
        self.update_noise();

        let pitch_modulation = self.flag_register(REG_PITCH_MOD_LOW);
        let noise_mode = self.flag_register(REG_NOISE_LOW);
        let reverb_mode = self.flag_register(REG_REVERB_ON_LOW);
        let irq_address = self.irq_address();
        let noise = self.noise_level as i16;
        let mut previous_output = 0i16;
        let mut voice_mix = [0i64; 2];
        let mut cd_mix = [0i64; 2];
        let mut reverb_input = [0i32; 2];
        let mut captures = [0i16; 2];
        let mut voice_irq = false;

        {
            let (voices, ram) = (&mut self.voices, &self.ram);
            for (index, voice) in voices.iter_mut().enumerate() {
                let modulator = if index != 0 && (pitch_modulation & (1 << index)) != 0 {
                    Some(previous_output)
                } else {
                    None
                };
                let noise_sample = if (noise_mode & (1 << index)) != 0 {
                    Some(noise)
                } else {
                    None
                };
                let tick = voice.tick(ram, irq_address, modulator, noise_sample);
                previous_output = tick.output;
                voice_irq |= tick.irq;
                if tick.end_reached {
                    self.end_flags |= 1 << index;
                }
                if index == 1 {
                    captures[0] = tick.output;
                } else if index == 3 {
                    captures[1] = tick.output;
                }

                let left = multiply(tick.output as i32, voice.volume_left() as i32);
                let right = multiply(tick.output as i32, voice.volume_right() as i32);
                voice_mix[0] += left as i64;
                voice_mix[1] += right as i64;
                if (reverb_mode & (1 << index)) != 0 {
                    reverb_input[0] = reverb_input[0].saturating_add(left);
                    reverb_input[1] = reverb_input[1].saturating_add(right);
                }
            }
        }
        if voice_irq {
            self.raise_irq();
        }

        let cd = [
            self.cd_input.pop_front().unwrap_or(0),
            self.cd_input.pop_front().unwrap_or(0),
        ];
        let capture_irq = self.write_capture(0x000, cd[0])
            | self.write_capture(0x400, cd[1])
            | self.write_capture(0x800, captures[0])
            | self.write_capture(0xc00, captures[1]);
        if capture_irq {
            self.raise_irq();
        }

        if (self.applied_control & 1) != 0 {
            let cd_left = multiply(
                cd[0] as i32,
                self.registers[REG_CD_VOLUME_LEFT as usize / 2] as i16 as i32,
            );
            let cd_right = multiply(
                cd[1] as i32,
                self.registers[REG_CD_VOLUME_RIGHT as usize / 2] as i16 as i32,
            );
            cd_mix[0] += cd_left as i64;
            cd_mix[1] += cd_right as i64;
            if (self.applied_control & (1 << 2)) != 0 {
                reverb_input[0] = reverb_input[0].saturating_add(cd_left);
                reverb_input[1] = reverb_input[1].saturating_add(cd_right);
            }
        }

        let mut reverb_registers = [0u16; 32];
        let start = REG_REVERB_CONFIG_START as usize / 2;
        reverb_registers.copy_from_slice(&self.registers[start..start + 32]);
        let (wet, reverb_irq) = self.reverb.process(
            &mut self.ram,
            &reverb_registers,
            reverb_input,
            (self.applied_control & (1 << 7)) != 0,
            irq_address,
        );
        if reverb_irq {
            self.raise_irq();
        }
        voice_mix[0] += multiply(
            wet[0] as i32,
            self.registers[REG_REVERB_VOLUME_LEFT as usize / 2] as i16 as i32,
        ) as i64;
        voice_mix[1] += multiply(
            wet[1] as i32,
            self.registers[REG_REVERB_VOLUME_RIGHT as usize / 2] as i16 as i32,
        ) as i64;

        let voice_output_enabled = (self.applied_control & 0xc000) == 0xc000;
        if voice_output_enabled {
            cd_mix[0] += voice_mix[0];
            cd_mix[1] += voice_mix[1];
        }
        let left = multiply_wide(cd_mix[0], self.main_volume_left.current() as i32);
        let right = multiply_wide(cd_mix[1], self.main_volume_right.current() as i32);
        self.push_output(clamp_i16(left), clamp_i16(right));
        self.capture_offset = self.capture_offset.wrapping_add(2) & 0x03ff;
    }

    fn update_noise(&mut self) {
        let step = ((self.applied_control >> 8) & 3) as i32 + 4;
        let shift = ((self.applied_control >> 10) & 0x0f) as u32;
        self.noise_timer -= step;
        let parity = ((self.noise_level >> 15)
            ^ (self.noise_level >> 12)
            ^ (self.noise_level >> 11)
            ^ (self.noise_level >> 10)
            ^ 1)
            & 1;
        if self.noise_timer < 0 {
            self.noise_level = (self.noise_level << 1) | parity;
            let reload = (0x20_000u32 >> shift) as i32;
            self.noise_timer += reload;
            if self.noise_timer < 0 {
                self.noise_timer += reload;
            }
        }
    }

    fn write_capture(&mut self, base: u32, value: i16) -> bool {
        let address = base + self.capture_offset as u32;
        let bytes = value.to_le_bytes();
        self.ram[address as usize] = bytes[0];
        self.ram[address as usize + 1] = bytes[1];
        (address & !7) == (self.irq_address() & !7)
            && (self.registers[REG_TRANSFER_CONTROL as usize / 2] & 0x0c) != 0
    }

    fn flag_register(&self, low_offset: u32) -> u32 {
        self.registers[low_offset as usize / 2] as u32
            | ((self.registers[(low_offset as usize / 2) + 1] as u32) << 16)
    }

    fn irq_address(&self) -> u32 {
        (self.registers[REG_IRQ_ADDRESS as usize / 2] as u32) << 3
    }

    fn touch_irq(&mut self, address: u32) {
        if (address & !7) == (self.irq_address() & !7) {
            self.raise_irq();
        }
    }

    fn raise_irq(&mut self) {
        if (self.control & ((1 << 15) | (1 << 6))) != ((1 << 15) | (1 << 6)) {
            return;
        }
        if !self.irq_flag {
            self.irq_flag = true;
            self.irq_edge_pending = true;
        }
    }

    fn push_output(&mut self, left: i16, right: i16) {
        if self.output.len() + 2 > OUTPUT_CAPACITY {
            self.output.pop_front();
            self.output.pop_front();
        }
        self.output.push_back(left);
        self.output.push_back(right);
    }
}

impl Default for Spu {
    fn default() -> Self {
        Self::new()
    }
}

fn write_mask(offset: u32, value: u16) -> u32 {
    if (offset & 2) == 0 {
        value as u32
    } else {
        (value as u32) << 16
    }
}

fn repeat_groups(input: &[u16], group: usize, source: usize) -> Vec<u16> {
    let mut output = Vec::with_capacity(input.len());
    for chunk in input.chunks(group) {
        let value = chunk[source.min(chunk.len() - 1)];
        output.extend(std::iter::repeat_n(value, chunk.len()));
    }
    output
}

fn multiply(sample: i32, volume: i32) -> i32 {
    ((sample as i64 * volume as i64) >> 15).clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

fn multiply_wide(sample: i64, volume: i32) -> i64 {
    sample.saturating_mul(volume as i64) >> 15
}

fn clamp_i16(value: i64) -> i16 {
    value.clamp(i16::MIN as i64, i16::MAX as i64) as i16
}

#[cfg(test)]
mod tests {
    use super::{Spu, SPU_CYCLES_PER_SAMPLE};

    #[test]
    fn transfer_address_register_stays_fixed_while_dma_address_advances() {
        let mut spu = Spu::new();
        spu.write16(0x1a6, 0x20);
        spu.dma_write32(0x4433_2211);

        assert_eq!(spu.read16(0x1a6), 0x20);
        assert_eq!(&spu.ram()[0x100..0x104], &[0x11, 0x22, 0x33, 0x44]);

        spu.write16(0x1a6, 0x20);
        assert_eq!(spu.dma_read32(), 0x4433_2211);
    }

    #[test]
    fn manual_fifo_commits_on_the_next_spu_tick() {
        let mut spu = Spu::new();
        spu.write16(0x1a6, 2);
        spu.write16(0x1a8, 0x1234);
        spu.write16(0x1a8, 0xabcd);
        spu.write16(0x1aa, 0x8010);

        assert_eq!(&spu.ram()[16..20], &[0, 0, 0, 0]);
        spu.tick(SPU_CYCLES_PER_SAMPLE);

        assert_eq!(&spu.ram()[16..20], &[0x34, 0x12, 0xcd, 0xab]);
        assert_eq!(spu.read16(0x1ae) & 0x3f, 0x10);
    }

    #[test]
    fn spu_produces_stereo_samples_at_44100_hz() {
        let mut spu = Spu::new();

        spu.tick(SPU_CYCLES_PER_SAMPLE * 3 - 1);
        assert_eq!(spu.queued_samples(), 4);
        spu.tick(1);
        assert_eq!(spu.queued_samples(), 6);
    }

    #[test]
    fn adpcm_voice_reaches_the_stereo_mixer() {
        let mut spu = Spu::new();
        spu.write16(0x1a6, 0x20);
        let block = [0x7777_0700, 0x7777_7777, 0x7777_7777, 0x7777_7777];
        for word in block {
            spu.dma_write32(word);
        }
        spu.write16(0x000, 0x3fff);
        spu.write16(0x002, 0x3fff);
        spu.write16(0x004, 0x1000);
        spu.write16(0x006, 0x20);
        spu.write16(0x008, 0x0000);
        spu.write16(0x00a, 0x0000);
        spu.write16(0x180, 0x3fff);
        spu.write16(0x182, 0x3fff);
        spu.write16(0x188, 1);
        spu.write16(0x1aa, 0xc000);

        spu.tick(SPU_CYCLES_PER_SAMPLE * 8);
        let mut output = [0i16; 16];
        assert_eq!(spu.drain_audio(&mut output), 16);
        assert!(output.iter().any(|sample| *sample != 0));
        assert!(output.chunks_exact(2).all(|pair| pair[0] == pair[1]));
    }

    #[test]
    fn transfer_to_irq_address_sets_status_and_emits_edge() {
        let mut spu = Spu::new();
        spu.write16(0x1a4, 0x20);
        spu.write16(0x1a6, 0x20);
        spu.write16(0x1aa, 0x8040);

        spu.dma_write32(0x1234_5678);

        assert!(spu.irq_pending());
        assert_ne!(spu.read16(0x1ae) & (1 << 6), 0);
        assert!(spu.tick(1));
        assert!(!spu.tick(1));
        spu.write16(0x1aa, 0x8000);
        assert!(!spu.irq_pending());
    }

    #[test]
    fn disabling_irq_cancels_an_unconsumed_edge() {
        let mut spu = Spu::new();
        spu.write16(0x1a4, 0x20);
        spu.write16(0x1a6, 0x20);
        spu.write16(0x1aa, 0x8040);
        spu.dma_write32(0);

        spu.write16(0x1aa, 0x8000);

        assert!(!spu.tick(1));
        assert!(!spu.irq_pending());
    }

    #[test]
    fn cd_input_is_mixed_and_written_to_capture_ram() {
        let mut spu = Spu::new();
        spu.write16(0x180, 0x3fff);
        spu.write16(0x182, 0x3fff);
        spu.write16(0x1b0, 0x7fff);
        spu.write16(0x1b2, 0x7fff);
        spu.write16(0x1aa, 0xc001);
        spu.push_cd_sample(10_000, -10_000);

        spu.tick(SPU_CYCLES_PER_SAMPLE);
        let mut output = [0i16; 2];
        spu.drain_audio(&mut output);

        assert!(output[0] > 9_000);
        assert!(output[1] < -9_000);
        assert_eq!(i16::from_le_bytes([spu.ram()[0], spu.ram()[1]]), 10_000);
        assert_eq!(
            i16::from_le_bytes([spu.ram()[0x400], spu.ram()[0x401]]),
            -10_000
        );

        spu.write16(0x1aa, 0x0001);
        spu.push_cd_sample(8_000, 4_000);
        spu.tick(SPU_CYCLES_PER_SAMPLE);
        spu.drain_audio(&mut output);
        assert!(output[0] > 7_000);
        assert!(output[1] > 3_000);
    }
}
