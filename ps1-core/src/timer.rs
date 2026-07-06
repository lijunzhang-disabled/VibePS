use crate::interrupt::{InterruptController, IRQ_TIMER0, IRQ_TIMER1, IRQ_TIMER2};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Timer {
    counter: u16,
    mode: u16,
    target: u16,
    irq_fired: bool,
    divider_accum: u32,
    sync_free_run: bool,
}

impl Timer {
    pub fn new() -> Self {
        Self {
            counter: 0,
            mode: 0,
            target: 0xffff,
            irq_fired: false,
            divider_accum: 0,
            sync_free_run: false,
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
        self.sync_free_run = false;
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
    hblank: bool,
    vblank: bool,
}

impl Timers {
    pub fn new() -> Self {
        Self {
            timers: [Timer::new(), Timer::new(), Timer::new()],
            hblank: false,
            vblank: false,
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
            let Some(divisor) = timer_divisor(index, self.timers[index].mode) else {
                continue;
            };
            self.timers[index].divider_accum =
                self.timers[index].divider_accum.saturating_add(cpu_cycles);
            while self.timers[index].divider_accum >= divisor {
                self.timers[index].divider_accum -= divisor;
                self.clock_timer(index, irq);
            }
        }
    }

    pub fn set_hblank(&mut self, active: bool, irq: &mut InterruptController) {
        let rising = !self.hblank && active;
        self.hblank = active;
        if rising {
            self.handle_sync_edge(0);
            if timer1_uses_hblank_clock(self.timers[1].mode) {
                self.clock_timer(1, irq);
            }
        }
    }

    pub fn set_vblank(&mut self, active: bool) {
        let rising = !self.vblank && active;
        self.vblank = active;
        if rising {
            self.handle_sync_edge(1);
        }
    }

    fn handle_sync_edge(&mut self, index: usize) {
        let timer = &mut self.timers[index];
        if (timer.mode & MODE_SYNC_ENABLE) == 0 {
            return;
        }
        match sync_mode(timer.mode) {
            1 | 2 => timer.counter = 0,
            3 => timer.sync_free_run = true,
            _ => {}
        }
    }

    fn clock_timer(&mut self, index: usize, irq: &mut InterruptController) {
        if !self.counter_enabled(index) {
            return;
        }
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
                fire_timer_irq(timer, index, irq);
            }
        }
    }

    fn counter_enabled(&self, index: usize) -> bool {
        let timer = self.timers[index];
        if (timer.mode & MODE_SYNC_ENABLE) == 0 {
            return true;
        }

        match index {
            0 => match sync_mode(timer.mode) {
                0 => !self.hblank,
                1 => true,
                2 => self.hblank,
                _ => timer.sync_free_run,
            },
            1 => match sync_mode(timer.mode) {
                0 => !self.vblank,
                1 => true,
                2 => self.vblank,
                _ => timer.sync_free_run,
            },
            _ => matches!(sync_mode(timer.mode), 1 | 2),
        }
    }
}

impl Default for Timers {
    fn default() -> Self {
        Self::new()
    }
}

const MODE_SYNC_ENABLE: u16 = 1 << 0;
const MODE_IRQ_REQUEST: u16 = 1 << 10;

fn timer_divisor(index: usize, mode: u16) -> Option<u32> {
    let clock_source = (mode >> 8) & 0x3;
    match index {
        0 if clock_source == 1 || clock_source == 3 => Some(5),
        1 if clock_source == 1 || clock_source == 3 => None,
        2 if clock_source >= 2 => Some(8),
        _ => Some(1),
    }
}

fn timer1_uses_hblank_clock(mode: u16) -> bool {
    matches!((mode >> 8) & 0x3, 1 | 3)
}

fn sync_mode(mode: u16) -> u16 {
    (mode >> 1) & 0x3
}

fn fire_timer_irq(timer: &mut Timer, index: usize, irq: &mut InterruptController) {
    let toggle = (timer.mode & (1 << 7)) != 0;
    let request = if toggle {
        let was_high = (timer.mode & MODE_IRQ_REQUEST) != 0;
        if was_high {
            timer.mode &= !MODE_IRQ_REQUEST;
            true
        } else {
            timer.mode |= MODE_IRQ_REQUEST;
            false
        }
    } else {
        timer.mode &= !MODE_IRQ_REQUEST;
        true
    };

    if request {
        irq.request(match index {
            0 => IRQ_TIMER0,
            1 => IRQ_TIMER1,
            _ => IRQ_TIMER2,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::Timers;
    use crate::interrupt::{InterruptController, IRQ_TIMER0, IRQ_TIMER1, IRQ_TIMER2};

    fn clear_irq(irq: &mut InterruptController, bit: u16) {
        irq.acknowledge(!bit);
    }

    #[test]
    fn one_shot_target_irq_suppresses_later_target_events() {
        let mut timers = Timers::new();
        let mut irq = InterruptController::new();
        timers.write16(0x8, 1);
        timers.write16(0x4, (1 << 3) | (1 << 4));

        timers.tick(1, &mut irq);
        assert_ne!(irq.status() & IRQ_TIMER0, 0);
        clear_irq(&mut irq, IRQ_TIMER0);

        timers.tick(1, &mut irq);
        assert_eq!(irq.status() & IRQ_TIMER0, 0);
    }

    #[test]
    fn repeat_pulse_target_irq_requests_every_target_event() {
        let mut timers = Timers::new();
        let mut irq = InterruptController::new();
        timers.write16(0x8, 1);
        timers.write16(0x4, (1 << 3) | (1 << 4) | (1 << 6));

        timers.tick(1, &mut irq);
        assert_ne!(irq.status() & IRQ_TIMER0, 0);
        clear_irq(&mut irq, IRQ_TIMER0);
        timers.tick(1, &mut irq);

        assert_ne!(irq.status() & IRQ_TIMER0, 0);
    }

    #[test]
    fn repeat_toggle_irq_requests_on_every_other_event() {
        let mut timers = Timers::new();
        let mut irq = InterruptController::new();
        timers.write16(0x8, 1);
        timers.write16(0x4, (1 << 3) | (1 << 4) | (1 << 6) | (1 << 7));

        timers.tick(1, &mut irq);
        assert_ne!(irq.status() & IRQ_TIMER0, 0);
        clear_irq(&mut irq, IRQ_TIMER0);

        timers.tick(1, &mut irq);
        assert_eq!(irq.status() & IRQ_TIMER0, 0);

        timers.tick(1, &mut irq);
        assert_ne!(irq.status() & IRQ_TIMER0, 0);
    }

    #[test]
    fn timer2_clock_source_can_divide_system_clock_by_eight() {
        let mut timers = Timers::new();
        let mut irq = InterruptController::new();
        timers.write16(0x24, 2 << 8);

        timers.tick(7, &mut irq);
        assert_eq!(timers.read16(0x20), 0);

        timers.tick(1, &mut irq);
        assert_eq!(timers.read16(0x20), 1);
        assert_eq!(irq.status() & IRQ_TIMER2, 0);
    }

    #[test]
    fn timer1_hblank_clock_counts_hblank_edges() {
        let mut timers = Timers::new();
        let mut irq = InterruptController::new();
        timers.write16(0x14, 1 << 8);

        timers.tick(100, &mut irq);
        assert_eq!(timers.read16(0x10), 0);

        timers.set_hblank(true, &mut irq);
        timers.set_hblank(false, &mut irq);
        timers.set_hblank(true, &mut irq);

        assert_eq!(timers.read16(0x10), 2);
        assert_eq!(irq.status() & IRQ_TIMER1, 0);
    }

    #[test]
    fn sync_pause_mode_stops_timer0_during_hblank() {
        let mut timers = Timers::new();
        let mut irq = InterruptController::new();
        timers.write16(0x4, 1);

        timers.tick(1, &mut irq);
        assert_eq!(timers.read16(0x0), 1);

        timers.set_hblank(true, &mut irq);
        timers.tick(10, &mut irq);

        assert_eq!(timers.read16(0x0), 1);
    }
}
