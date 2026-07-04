use crate::interrupt::{InterruptController, IRQ_TIMER0, IRQ_TIMER1, IRQ_TIMER2};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Timer {
    counter: u16,
    mode: u16,
    target: u16,
    irq_fired: bool,
    divider_accum: u32,
}

impl Timer {
    pub fn new() -> Self {
        Self {
            counter: 0,
            mode: 0,
            target: 0xffff,
            irq_fired: false,
            divider_accum: 0,
        }
    }

    pub fn counter(&self) -> u16 {
        self.counter
    }

    pub fn mode(&self) -> u16 {
        self.mode
    }

    pub fn target(&self) -> u16 {
        self.target
    }

    pub fn set_counter(&mut self, value: u16) {
        self.counter = value;
        self.divider_accum = 0;
    }

    pub fn set_mode(&mut self, value: u16) {
        self.mode = (value & 0x03ff) | (1 << 10);
        self.counter = 0;
        self.irq_fired = false;
        self.divider_accum = 0;
    }

    pub fn set_target(&mut self, value: u16) {
        self.target = value;
    }
}

impl Default for Timer {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Timers {
    timers: [Timer; 3],
}

impl Timers {
    pub fn new() -> Self {
        Self {
            timers: [Timer::new(), Timer::new(), Timer::new()],
        }
    }

    pub fn read16(&mut self, offset: u32) -> u16 {
        let index = ((offset >> 4) & 0x3) as usize;
        let reg = offset & 0x0f;
        if index >= self.timers.len() {
            return 0xffff;
        }
        match reg {
            0x0 => self.timers[index].counter(),
            0x4 => {
                let mode = self.timers[index].mode();
                self.timers[index].mode &= !((1 << 11) | (1 << 12));
                mode
            }
            0x8 => self.timers[index].target(),
            _ => 0xffff,
        }
    }

    pub fn write16(&mut self, offset: u32, value: u16) {
        let index = ((offset >> 4) & 0x3) as usize;
        let reg = offset & 0x0f;
        if index >= self.timers.len() {
            return;
        }
        match reg {
            0x0 => self.timers[index].set_counter(value),
            0x4 => self.timers[index].set_mode(value),
            0x8 => self.timers[index].set_target(value),
            _ => {}
        }
    }

    pub fn tick(&mut self, cpu_cycles: u32, irq: &mut InterruptController) {
        for index in 0..self.timers.len() {
            let divisor = timer_divisor(index, self.timers[index].mode);
            self.timers[index].divider_accum =
                self.timers[index].divider_accum.saturating_add(cpu_cycles);
            while self.timers[index].divider_accum >= divisor {
                self.timers[index].divider_accum -= divisor;
                self.increment(index, irq);
            }
        }
    }

    fn increment(&mut self, index: usize, irq: &mut InterruptController) {
        let timer = &mut self.timers[index];
        let old = timer.counter;
        timer.counter = timer.counter.wrapping_add(1);

        let reached_target = timer.counter == timer.target;
        let reached_overflow = old == 0xffff;
        if reached_target {
            timer.mode |= 1 << 11;
        }
        if reached_overflow {
            timer.mode |= 1 << 12;
        }

        let reset_on_target = (timer.mode & (1 << 3)) != 0;
        if reset_on_target && reached_target {
            timer.counter = 0;
        }

        let irq_on_target = (timer.mode & (1 << 4)) != 0 && reached_target;
        let irq_on_overflow = (timer.mode & (1 << 5)) != 0 && reached_overflow;
        if irq_on_target || irq_on_overflow {
            let repeat = (timer.mode & (1 << 6)) != 0;
            if repeat || !timer.irq_fired {
                timer.irq_fired = true;
                timer.mode &= !(1 << 10);
                irq.request(match index {
                    0 => IRQ_TIMER0,
                    1 => IRQ_TIMER1,
                    _ => IRQ_TIMER2,
                });
            }
        }
    }
}

impl Default for Timers {
    fn default() -> Self {
        Self::new()
    }
}

fn timer_divisor(index: usize, mode: u16) -> u32 {
    let clock_source = (mode >> 8) & 0x3;
    if index == 2 && clock_source >= 2 {
        8
    } else {
        1
    }
}
