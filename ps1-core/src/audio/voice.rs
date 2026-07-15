use super::envelope::{AdsrEnvelope, EnvelopePhase, VolumeEnvelope};
use crate::SPU_RAM_SIZE;
use serde::{Deserialize, Serialize};

const BLOCK_BYTES: u32 = 16;
const SAMPLES_PER_BLOCK: u32 = 28;
const SAMPLE_FRACTION_BITS: u32 = 12;
const BLOCK_POSITION_END: u32 = SAMPLES_PER_BLOCK << SAMPLE_FRACTION_BITS;

#[derive(Debug, Clone, Copy, Default)]
pub struct VoiceTick {
    pub output: i16,
    pub end_reached: bool,
    pub irq: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Voice {
    volume_left: VolumeEnvelope,
    volume_right: VolumeEnvelope,
    pitch: u16,
    start_address: u16,
    adsr_low: u16,
    adsr_high: u16,
    repeat_address: u16,
    envelope: AdsrEnvelope,
    current_address: u32,
    block_position: u32,
    decoded: [i16; 28],
    block_flags: u8,
    block_loaded: bool,
    previous_samples: [i16; 2],
    last_output: i16,
}

impl Voice {
    pub fn new() -> Self {
        Self {
            volume_left: VolumeEnvelope::new(),
            volume_right: VolumeEnvelope::new(),
            pitch: 0,
            start_address: 0,
            adsr_low: 0,
            adsr_high: 0,
            repeat_address: 0,
            envelope: AdsrEnvelope::new(),
            current_address: 0,
            block_position: 0,
            decoded: [0; 28],
            block_flags: 0,
            block_loaded: false,
            previous_samples: [0; 2],
            last_output: 0,
        }
    }

    pub fn read_register(&self, register: u32) -> u16 {
        match register & 0x0f {
            0x0 => self.volume_left.register(),
            0x2 => self.volume_right.register(),
            0x4 => self.pitch,
            0x6 => self.start_address,
            0x8 => self.adsr_low,
            0xa => self.adsr_high,
            0xc => self.envelope.level() as u16,
            0xe => self.repeat_address,
            _ => 0,
        }
    }

    pub fn write_register(&mut self, register: u32, value: u16) {
        match register & 0x0f {
            0x0 => self.volume_left.write(value),
            0x2 => self.volume_right.write(value),
            0x4 => self.pitch = value,
            0x6 => self.start_address = value,
            0x8 => self.adsr_low = value,
            0xa => self.adsr_high = value,
            0xc => self.envelope.set_level(value),
            0xe => self.repeat_address = value,
            _ => {}
        }
    }

    pub fn key_on(&mut self) {
        self.envelope.key_on();
        self.current_address = (self.start_address as u32) << 3;
        self.block_position = 0;
        self.block_loaded = false;
        self.previous_samples = [0; 2];
        self.last_output = 0;
    }

    pub fn key_off(&mut self) {
        self.envelope.key_off();
    }

    pub fn tick(
        &mut self,
        ram: &[u8],
        irq_address: u32,
        pitch_modulation: Option<i16>,
        noise: Option<i16>,
    ) -> VoiceTick {
        self.volume_left.tick();
        self.volume_right.tick();
        self.envelope.tick(self.adsr_low, self.adsr_high);

        if self.envelope.phase() == EnvelopePhase::Off {
            self.last_output = 0;
            return VoiceTick::default();
        }

        let mut irq = false;
        if !self.block_loaded {
            irq |= self.decode_block(ram, irq_address);
        }

        let raw = if let Some(noise) = noise {
            noise
        } else {
            let index = (self.block_position >> SAMPLE_FRACTION_BITS).min(27) as usize;
            let fraction = (self.block_position & 0x0fff) as i32;
            let current = self.decoded[index] as i32;
            let next = self.decoded[(index + 1).min(27)] as i32;
            (current + (((next - current) * fraction) >> SAMPLE_FRACTION_BITS)) as i16
        };

        let output = ((raw as i32 * self.envelope.level() as i32) >> 15)
            .clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        self.last_output = output;

        let mut end_reached = false;
        if noise.is_none() {
            self.block_position = self
                .block_position
                .saturating_add(pitch_step(self.pitch, pitch_modulation));
            if self.block_position >= BLOCK_POSITION_END {
                self.block_position -= BLOCK_POSITION_END;
                end_reached = (self.block_flags & 1) != 0;
                self.finish_block();
            }
        }

        VoiceTick {
            output,
            end_reached,
            irq,
        }
    }

    pub fn volume_left(&self) -> i16 {
        self.volume_left.current()
    }

    pub fn volume_right(&self) -> i16 {
        self.volume_right.current()
    }

    #[cfg(test)]
    pub fn current_address(&self) -> u32 {
        self.current_address
    }

    fn decode_block(&mut self, ram: &[u8], irq_address: u32) -> bool {
        let address = self.current_address & (SPU_RAM_SIZE as u32 - 1);
        let header = ram[address as usize];
        self.block_flags = ram[((address + 1) as usize) & (SPU_RAM_SIZE - 1)];
        self.decoded = decode_adpcm_block(ram, address, header, &mut self.previous_samples);
        self.block_loaded = true;

        if (self.block_flags & 4) != 0 {
            self.repeat_address = (address >> 3) as u16;
        }

        range_contains_wrapped(address, BLOCK_BYTES, irq_address)
    }

    fn finish_block(&mut self) {
        self.block_loaded = false;
        if (self.block_flags & 1) == 0 {
            self.current_address =
                self.current_address.wrapping_add(BLOCK_BYTES) & (SPU_RAM_SIZE as u32 - 1);
            return;
        }

        self.current_address = ((self.repeat_address as u32) << 3) & (SPU_RAM_SIZE as u32 - 1);
        if (self.block_flags & 2) == 0 {
            self.envelope.force_off();
            self.last_output = 0;
        }
    }
}

impl Default for Voice {
    fn default() -> Self {
        Self::new()
    }
}

fn pitch_step(pitch: u16, modulation: Option<i16>) -> u32 {
    let mut step = pitch as u32;
    if let Some(modulation) = modulation {
        let factor = modulation as i32 + 0x8000;
        let signed_pitch = pitch as i16 as i32;
        step = (((signed_pitch as i64 * factor as i64) >> 15) as u32) & 0xffff;
    }
    step.min(0x4000)
}

pub(crate) fn decode_adpcm_block(
    ram: &[u8],
    address: u32,
    header: u8,
    history: &mut [i16; 2],
) -> [i16; 28] {
    const POSITIVE: [i32; 5] = [0, 60, 115, 98, 122];
    const NEGATIVE: [i32; 5] = [0, 0, -52, -55, -60];

    let shift = (header & 0x0f).min(12) as u32;
    let filter = ((header >> 4) & 0x0f).min(4) as usize;
    let mut decoded = [0i16; 28];
    let mut old = history[0] as i32;
    let mut older = history[1] as i32;

    for (sample_index, sample) in decoded.iter_mut().enumerate() {
        let byte_address = (address + 2 + (sample_index / 2) as u32) as usize & (SPU_RAM_SIZE - 1);
        let byte = ram[byte_address];
        let nibble = if sample_index & 1 == 0 {
            byte & 0x0f
        } else {
            byte >> 4
        };
        let signed = ((nibble as i8) << 4) >> 4;
        let source = ((signed as i32) << 12) >> shift;
        let prediction = (old * POSITIVE[filter] + older * NEGATIVE[filter] + 32) >> 6;
        let value = source
            .saturating_add(prediction)
            .clamp(i16::MIN as i32, i16::MAX as i32);
        *sample = value as i16;
        older = old;
        old = value;
    }

    history[0] = old as i16;
    history[1] = older as i16;
    decoded
}

fn range_contains_wrapped(start: u32, len: u32, target: u32) -> bool {
    let mask = SPU_RAM_SIZE as u32 - 1;
    (0..len).any(|offset| start.wrapping_add(offset) & mask == target & mask)
}

#[cfg(test)]
mod tests {
    use super::{decode_adpcm_block, pitch_step, Voice};
    use crate::SPU_RAM_SIZE;

    #[test]
    fn filter_zero_decodes_low_nibble_before_high_nibble() {
        let mut ram = vec![0; SPU_RAM_SIZE];
        ram[0] = 0x0c;
        ram[2] = 0x1f;
        let mut history = [0; 2];

        let decoded = decode_adpcm_block(&ram, 0, ram[0], &mut history);

        assert_eq!(decoded[0], -1);
        assert_eq!(decoded[1], 1);
    }

    #[test]
    fn pitch_modulation_uses_previous_voice_amplitude_and_clamps() {
        assert_eq!(pitch_step(0x1000, Some(0)), 0x1000);
        assert_eq!(pitch_step(0x1000, Some(0x4000)), 0x1800);
        assert_eq!(pitch_step(0x7000, None), 0x4000);
    }

    #[test]
    fn loop_start_updates_repeat_address_and_loop_end_jumps_back() {
        let mut ram = vec![0; SPU_RAM_SIZE];
        ram[0x100] = 0x0c;
        ram[0x101] = 0x07;
        let mut voice = Voice::new();
        voice.write_register(0x4, 0x4000);
        voice.write_register(0x6, 0x20);
        voice.write_register(0x8, 0);
        voice.write_register(0xa, 0);
        voice.key_on();

        let mut tick = Default::default();
        for _ in 0..7 {
            tick = voice.tick(&ram, 0x7ffff, None, None);
        }

        assert!(tick.end_reached);
        assert_eq!(voice.current_address(), 0x100);
        assert_eq!(voice.read_register(0xe), 0x20);
    }
}
