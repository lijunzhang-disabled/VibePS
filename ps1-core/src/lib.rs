//! PlayStation 1 emulator core. See `../PLAN.md` and `../ARCHITECTURE.md`.

pub mod audio;
pub mod bus;
pub mod cdrom;
pub mod cpu;
pub mod dma;
pub mod gpu;
pub mod interrupt;
pub mod mdec;
pub mod scheduler;
pub mod timer;

use bus::Bus;
use cpu::Cpu;
use scheduler::{Event, EventKind, Scheduler};
use serde::{Deserialize, Serialize};

pub const CPU_CLOCK_HZ: u32 = 33_868_800;
pub const MAIN_RAM_SIZE: usize = 2 * 1024 * 1024;
pub const SCRATCHPAD_SIZE: usize = 1024;
pub const BIOS_SIZE: usize = 512 * 1024;
pub const GPU_VRAM_WIDTH: usize = 1024;
pub const GPU_VRAM_HEIGHT: usize = 512;
pub const GPU_VRAM_PIXELS: usize = GPU_VRAM_WIDTH * GPU_VRAM_HEIGHT;
pub const SPU_RAM_SIZE: usize = 512 * 1024;

/// A conservative NTSC frame approximation used until the GPU timing model is
/// precise enough to drive HBlank/VBlank directly.
pub const APPROX_CYCLES_PER_FRAME: u64 = CPU_CLOCK_HZ as u64 / 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ps1 {
    pub cpu: Cpu,
    pub bus: Bus,
    pub scheduler: Scheduler,
}

impl Ps1 {
    pub fn new(bios: Option<Vec<u8>>) -> Self {
        let mut ps1 = Self {
            cpu: Cpu::new(),
            bus: Bus::new(bios),
            scheduler: Scheduler::new(),
        };
        ps1.schedule_initial_events();
        ps1
    }

    fn schedule_initial_events(&mut self) {
        self.scheduler.schedule(Event {
            fire_time: APPROX_CYCLES_PER_FRAME,
            kind: EventKind::VBlank,
        });
    }

    pub fn reset(&mut self) {
        self.cpu.reset();
        self.scheduler = Scheduler::new();
        self.schedule_initial_events();
    }

    pub fn step_one(&mut self) -> u32 {
        let cycles = self.cpu.step(&mut self.bus);
        self.bus.tick(cycles);
        self.scheduler.add_cycles(cycles as u64);
        while let Some(event) = self.scheduler.pop_if_ready() {
            self.dispatch_event(event);
        }
        cycles
    }

    pub fn run_cycles(&mut self, cycles: u64) {
        let target = self.scheduler.timestamp().saturating_add(cycles);
        while self.scheduler.timestamp() < target {
            self.step_one();
        }
    }

    pub fn run_frame(&mut self) {
        self.run_cycles(APPROX_CYCLES_PER_FRAME);
    }

    pub fn load_psx_exe(&mut self, exe: &[u8]) -> Result<(), ExeLoadError> {
        if exe.len() < 0x800 || &exe[0..8] != b"PS-X EXE" {
            return Err(ExeLoadError::InvalidHeader);
        }

        let initial_pc = read_le_u32(exe, 0x10)?;
        let initial_gp = read_le_u32(exe, 0x14)?;
        let load_addr = read_le_u32(exe, 0x18)?;
        let file_size = read_le_u32(exe, 0x1c)? as usize;
        let sp_base = read_le_u32(exe, 0x30).unwrap_or(0);
        let sp_offset = read_le_u32(exe, 0x34).unwrap_or(0);
        let payload_end = 0x800usize
            .checked_add(file_size)
            .ok_or(ExeLoadError::PayloadTooLarge)?;
        if payload_end > exe.len() {
            return Err(ExeLoadError::TruncatedPayload);
        }

        self.bus
            .copy_to_ram(load_addr, &exe[0x800..payload_end])
            .map_err(|_| ExeLoadError::LoadAddressOutOfRange)?;
        self.cpu.set_pc(initial_pc);
        self.cpu.regs[28] = initial_gp;
        if sp_base != 0 {
            self.cpu.regs[29] = sp_base.wrapping_add(sp_offset);
        }
        Ok(())
    }
}

fn read_le_u32(bytes: &[u8], offset: usize) -> Result<u32, ExeLoadError> {
    let end = offset.checked_add(4).ok_or(ExeLoadError::InvalidHeader)?;
    let chunk = bytes.get(offset..end).ok_or(ExeLoadError::InvalidHeader)?;
    Ok(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
}

impl Ps1 {
    fn dispatch_event(&mut self, event: Event) {
        match event.kind {
            EventKind::VBlank => {
                self.bus.irq.request(interrupt::IRQ_VBLANK);
                self.scheduler.schedule(Event {
                    fire_time: self
                        .scheduler
                        .timestamp()
                        .saturating_add(APPROX_CYCLES_PER_FRAME),
                    kind: EventKind::VBlank,
                });
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExeLoadError {
    InvalidHeader,
    PayloadTooLarge,
    TruncatedPayload,
    LoadAddressOutOfRange,
}
