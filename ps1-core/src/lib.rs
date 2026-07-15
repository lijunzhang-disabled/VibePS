//! PlayStation 1 emulator core. See `../PLAN.md` and `../ARCHITECTURE.md`.

pub mod audio;
pub mod bus;
pub mod cdrom;
pub mod cpu;
pub mod dma;
pub mod gpu;
pub mod interrupt;
pub mod joy;
pub mod mdec;
pub mod scheduler;
pub mod test_runner;
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
                self.bus.timers.set_vblank(true);
                self.bus.timers.set_vblank(false);
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

#[cfg(test)]
mod tests {
    use super::{ExeLoadError, Ps1, BIOS_SIZE};
    use crate::cpu::cop0::STATUS_BEV;

    fn i(op: u32, rs: u32, rt: u32, imm: i16) -> u32 {
        (op << 26) | (rs << 21) | (rt << 16) | (imm as u16 as u32)
    }

    fn write_le_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn psx_exe(payload: &[u8], load_addr: u32, pc: u32) -> Vec<u8> {
        let mut exe = vec![0; 0x800 + payload.len()];
        exe[0..8].copy_from_slice(b"PS-X EXE");
        write_le_u32(&mut exe, 0x10, pc);
        write_le_u32(&mut exe, 0x14, 0x8000_4000);
        write_le_u32(&mut exe, 0x18, load_addr);
        write_le_u32(&mut exe, 0x1c, payload.len() as u32);
        write_le_u32(&mut exe, 0x30, 0x801f_0000);
        write_le_u32(&mut exe, 0x34, 0x100);
        exe[0x800..].copy_from_slice(payload);
        exe
    }

    #[test]
    fn boots_from_supplied_bios_image() {
        let mut bios = vec![0; BIOS_SIZE];
        write_le_u32(&mut bios, 0, i(0x09, 0, 2, 0x1234)); // addiu r2,r0,0x1234
        write_le_u32(&mut bios, 4, i(0x2b, 0, 2, 0x100)); // sw r2,0x100(r0)
        let mut ps1 = Ps1::new(Some(bios));

        ps1.step_one();
        ps1.step_one();

        assert_eq!(ps1.cpu.regs[2], 0x1234);
        assert_eq!(ps1.bus.read32(0x0000_0100), 0x1234);
    }

    #[test]
    fn reset_returns_cpu_to_bios_boot_vector() {
        let mut ps1 = Ps1::new(None);
        ps1.cpu.set_pc(0x8000_0000);
        ps1.cpu.regs[1] = 0x1234;

        ps1.reset();

        assert_eq!(ps1.cpu.pc, 0xbfc0_0000);
        assert_eq!(ps1.cpu.next_pc, 0xbfc0_0004);
        assert_eq!(ps1.cpu.regs[1], 0);
        assert_ne!(ps1.cpu.cop0.status() & STATUS_BEV, 0);
    }

    #[test]
    fn loads_psx_exe_payload_and_initial_registers() {
        let exe = psx_exe(&0x1234_5678u32.to_le_bytes(), 0x8001_0000, 0x8001_0000);
        let mut ps1 = Ps1::new(None);

        ps1.load_psx_exe(&exe).unwrap();

        assert_eq!(ps1.cpu.pc, 0x8001_0000);
        assert_eq!(ps1.cpu.next_pc, 0x8001_0004);
        assert_eq!(ps1.cpu.regs[28], 0x8000_4000);
        assert_eq!(ps1.cpu.regs[29], 0x801f_0100);
        assert_eq!(ps1.bus.read32(0x8001_0000), 0x1234_5678);
    }

    #[test]
    fn rejects_invalid_psx_exe_header() {
        let mut ps1 = Ps1::new(None);

        let err = ps1.load_psx_exe(&vec![0; 0x800]).unwrap_err();

        assert_eq!(err, ExeLoadError::InvalidHeader);
    }

    #[test]
    fn rejects_truncated_psx_exe_payload() {
        let mut exe = psx_exe(&[1, 2, 3], 0x8001_0000, 0x8001_0000);
        write_le_u32(&mut exe, 0x1c, 4);
        let mut ps1 = Ps1::new(None);

        let err = ps1.load_psx_exe(&exe).unwrap_err();

        assert_eq!(err, ExeLoadError::TruncatedPayload);
    }

    #[test]
    fn rejects_psx_exe_payload_that_crosses_ram_end() {
        let exe = psx_exe(&[1, 2, 3, 4], 0x801f_fffe, 0x8001_0000);
        let mut ps1 = Ps1::new(None);

        let err = ps1.load_psx_exe(&exe).unwrap_err();

        assert_eq!(err, ExeLoadError::LoadAddressOutOfRange);
    }
}
