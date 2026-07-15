use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnvelopePhase {
    Off,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AdsrEnvelope {
    level: i32,
    counter: u32,
    phase: EnvelopePhase,
}

impl AdsrEnvelope {
    pub fn new() -> Self {
        Self {
            level: 0,
            counter: 0,
            phase: EnvelopePhase::Off,
        }
    }

    pub fn key_on(&mut self) {
        self.level = 0;
        self.counter = 0;
        self.phase = EnvelopePhase::Attack;
    }

    pub fn key_off(&mut self) {
        if self.phase != EnvelopePhase::Off {
            self.counter = 0;
            self.phase = EnvelopePhase::Release;
        }
    }

    pub fn force_off(&mut self) {
        self.level = 0;
        self.counter = 0;
        self.phase = EnvelopePhase::Off;
    }

    pub fn level(&self) -> i16 {
        self.level as i16
    }

    pub fn set_level(&mut self, value: u16) {
        self.level = value as i16 as i32;
    }

    pub fn phase(&self) -> EnvelopePhase {
        self.phase
    }

    pub fn tick(&mut self, adsr_low: u16, adsr_high: u16) {
        match self.phase {
            EnvelopePhase::Off => {}
            EnvelopePhase::Attack => {
                let shift = ((adsr_low >> 10) & 0x1f) as u8;
                let step = ((adsr_low >> 8) & 3) as u8;
                step_envelope(
                    &mut self.level,
                    &mut self.counter,
                    Rate {
                        shift,
                        step,
                        exponential: (adsr_low & 0x8000) != 0,
                        decreasing: false,
                        phase_negative: false,
                        frozen: shift == 0x1f && step == 3,
                    },
                );
                if self.level >= 0x7fff {
                    self.level = 0x7fff;
                    self.counter = 0;
                    self.phase = EnvelopePhase::Decay;
                }
            }
            EnvelopePhase::Decay => {
                let shift = ((adsr_low >> 4) & 0x0f) as u8;
                step_envelope(
                    &mut self.level,
                    &mut self.counter,
                    Rate {
                        shift,
                        step: 0,
                        exponential: true,
                        decreasing: true,
                        phase_negative: false,
                        frozen: false,
                    },
                );
                let target = (((adsr_low & 0x0f) as i32 + 1) * 0x800).min(0x7fff);
                if self.level <= target {
                    self.level = target;
                    self.counter = 0;
                    self.phase = EnvelopePhase::Sustain;
                }
            }
            EnvelopePhase::Sustain => {
                let shift = ((adsr_high >> 8) & 0x1f) as u8;
                let step = ((adsr_high >> 6) & 3) as u8;
                step_envelope(
                    &mut self.level,
                    &mut self.counter,
                    Rate {
                        shift,
                        step,
                        exponential: (adsr_high & 0x8000) != 0,
                        decreasing: (adsr_high & 0x4000) != 0,
                        phase_negative: false,
                        frozen: shift == 0x1f && step == 3,
                    },
                );
            }
            EnvelopePhase::Release => {
                let shift = (adsr_high & 0x1f) as u8;
                step_envelope(
                    &mut self.level,
                    &mut self.counter,
                    Rate {
                        shift,
                        step: 0,
                        exponential: (adsr_high & 0x20) != 0,
                        decreasing: true,
                        phase_negative: false,
                        frozen: shift == 0x1f,
                    },
                );
                if self.level <= 0 {
                    self.force_off();
                }
            }
        }
    }
}

impl Default for AdsrEnvelope {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct VolumeEnvelope {
    register: u16,
    level: i32,
    counter: u32,
}

impl VolumeEnvelope {
    pub fn new() -> Self {
        Self {
            register: 0,
            level: 0,
            counter: 0,
        }
    }

    pub fn write(&mut self, value: u16) {
        self.register = value;
        self.counter = 0;
        if (value & 0x8000) == 0 {
            self.level = ((value << 1) as i16) as i32;
        }
    }

    pub fn register(&self) -> u16 {
        self.register
    }

    pub fn current(&self) -> i16 {
        self.level as i16
    }

    pub fn tick(&mut self) {
        if (self.register & 0x8000) == 0 {
            self.level = ((self.register << 1) as i16) as i32;
            return;
        }

        let shift = ((self.register >> 2) & 0x1f) as u8;
        let step = (self.register & 3) as u8;
        step_envelope(
            &mut self.level,
            &mut self.counter,
            Rate {
                shift,
                step,
                exponential: (self.register & 0x4000) != 0,
                decreasing: (self.register & 0x2000) != 0,
                phase_negative: (self.register & 0x1000) != 0,
                frozen: shift == 0x1f && step == 3,
            },
        );
    }
}

impl Default for VolumeEnvelope {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy)]
struct Rate {
    shift: u8,
    step: u8,
    exponential: bool,
    decreasing: bool,
    phase_negative: bool,
    frozen: bool,
}

fn step_envelope(level: &mut i32, counter: &mut u32, rate: Rate) {
    if rate.frozen {
        return;
    }

    let mut step = 7i32 - rate.step as i32;
    if rate.decreasing ^ rate.phase_negative {
        step = !step;
    }
    step <<= 11u32.saturating_sub(rate.shift as u32);

    let mut increment = 0x8000u32 >> (rate.shift as u32).saturating_sub(11);
    increment = increment.max(1);

    if rate.exponential && !rate.decreasing && *level > 0x6000 {
        if rate.shift < 10 {
            step >>= 2;
        } else if rate.shift >= 11 {
            increment = (increment >> 2).max(1);
        } else {
            step >>= 1;
            increment = (increment >> 1).max(1);
        }
    } else if rate.exponential && rate.decreasing {
        step = step.saturating_mul(*level) >> 15;
    }

    *counter = counter.saturating_add(increment);
    if *counter < 0x8000 {
        return;
    }
    *counter -= 0x8000;
    *level = level.saturating_add(step);

    if !rate.decreasing {
        *level = (*level).clamp(-0x8000, 0x7fff);
    } else if rate.phase_negative {
        *level = (*level).clamp(-0x8000, 0);
    } else {
        *level = (*level).max(0);
    }
}

#[cfg(test)]
mod tests {
    use super::{AdsrEnvelope, EnvelopePhase, VolumeEnvelope};

    #[test]
    fn fast_adsr_reaches_sustain_and_release_returns_to_zero() {
        let mut envelope = AdsrEnvelope::new();
        envelope.key_on();
        let low = 0x0008;
        let high = 0x1fc0;

        for _ in 0..32 {
            envelope.tick(low, high);
        }
        assert_eq!(envelope.phase(), EnvelopePhase::Sustain);
        assert_eq!(envelope.level(), 0x4800);

        envelope.key_off();
        for _ in 0..16 {
            envelope.tick(low, high);
        }
        assert_eq!(envelope.phase(), EnvelopePhase::Off);
        assert_eq!(envelope.level(), 0);
    }

    #[test]
    fn fixed_volume_sign_extends_and_doubles_the_15_bit_value() {
        let mut volume = VolumeEnvelope::new();
        volume.write(0x2000);
        assert_eq!(volume.current(), 0x4000);
        volume.write(0x6000);
        assert_eq!(volume.current(), -0x4000);
    }
}
