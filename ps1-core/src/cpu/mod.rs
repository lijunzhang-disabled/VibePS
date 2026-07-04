pub mod cop0;
pub mod gte;

use crate::bus::Bus;
use cop0::{Cop0, Exception, ExceptionContext};
use gte::Gte;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cpu {
    pub regs: [u32; 32],
    pub hi: u32,
    pub lo: u32,
    pub pc: u32,
    pub next_pc: u32,
    pub cop0: Cop0,
    pub gte: Gte,
    load_delay: Option<(usize, u32)>,
    load_delay_merge: Option<(usize, u32)>,
    reg_write_mask: u32,
    in_delay_slot: bool,
    in_delay_slot_branch_taken: bool,
    in_delay_slot_branch_target: u32,
    next_is_delay_slot: bool,
    next_delay_slot_branch_taken: bool,
    next_delay_slot_branch_target: u32,
    pub halted: bool,
}

impl Cpu {
    pub fn new() -> Self {
        Self {
            regs: [0; 32],
            hi: 0,
            lo: 0,
            pc: 0xbfc0_0000,
            next_pc: 0xbfc0_0004,
            cop0: Cop0::new(),
            gte: Gte::new(),
            load_delay: None,
            load_delay_merge: None,
            reg_write_mask: 0,
            in_delay_slot: false,
            in_delay_slot_branch_taken: false,
            in_delay_slot_branch_target: 0,
            next_is_delay_slot: false,
            next_delay_slot_branch_taken: false,
            next_delay_slot_branch_target: 0,
            halted: false,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn set_pc(&mut self, pc: u32) {
        self.pc = pc;
        self.next_pc = pc.wrapping_add(4);
        self.in_delay_slot = false;
        self.in_delay_slot_branch_taken = false;
        self.in_delay_slot_branch_target = 0;
        self.next_is_delay_slot = false;
        self.next_delay_slot_branch_taken = false;
        self.next_delay_slot_branch_target = 0;
        self.load_delay = None;
        self.load_delay_merge = None;
        self.reg_write_mask = 0;
    }

    pub fn step(&mut self, bus: &mut Bus) -> u32 {
        self.cop0.set_interrupt_pending(bus.irq.pending());
        if self.cop0.interrupts_enabled() {
            self.in_delay_slot = self.next_is_delay_slot;
            self.in_delay_slot_branch_taken = self.next_delay_slot_branch_taken;
            self.in_delay_slot_branch_target = self.next_delay_slot_branch_target;
            self.raise_exception(Exception::Interrupt, self.pc, None, None);
            return 2;
        }

        if (self.pc & 3) != 0 {
            let pc = self.pc;
            self.in_delay_slot = self.next_is_delay_slot;
            self.in_delay_slot_branch_taken = self.next_delay_slot_branch_taken;
            self.in_delay_slot_branch_target = self.next_delay_slot_branch_target;
            self.raise_exception(Exception::AddressLoad, pc, Some(pc), None);
            return 2;
        }

        let pc = self.pc;
        let opcode = bus.read32(pc);
        let delayed_load = self.load_delay.take();
        self.load_delay_merge = delayed_load;
        self.reg_write_mask = 0;
        let was_delay_slot = self.next_is_delay_slot;
        let was_delay_slot_branch_taken = self.next_delay_slot_branch_taken;
        let was_delay_slot_branch_target = self.next_delay_slot_branch_target;

        self.pc = self.next_pc;
        self.next_pc = self.next_pc.wrapping_add(4);
        self.in_delay_slot = was_delay_slot;
        self.in_delay_slot_branch_taken = was_delay_slot_branch_taken;
        self.in_delay_slot_branch_target = was_delay_slot_branch_target;
        self.next_is_delay_slot = false;
        self.next_delay_slot_branch_taken = false;
        self.next_delay_slot_branch_target = 0;

        self.execute(opcode, bus, pc);

        self.load_delay_merge = None;
        if let Some((reg, value)) = delayed_load {
            let next_load_to_same_reg =
                matches!(self.load_delay, Some((next_reg, _)) if next_reg == reg);
            if !next_load_to_same_reg && (self.reg_write_mask & (1 << reg)) == 0 {
                self.write_reg_now(reg, value);
            }
        }
        self.reg_write_mask = 0;
        self.regs[0] = 0;
        self.in_delay_slot = false;
        self.in_delay_slot_branch_taken = false;
        self.in_delay_slot_branch_target = 0;
        2
    }

    fn execute(&mut self, opcode: u32, bus: &mut Bus, pc: u32) {
        match opcode >> 26 {
            0x00 => self.execute_special(opcode, pc),
            0x01 => self.execute_bcondz(opcode, pc),
            0x02 => self.jump(opcode, pc, false),
            0x03 => self.jump(opcode, pc, true),
            0x04 => self.branch(self.reg(rs(opcode)) == self.reg(rt(opcode)), opcode, pc),
            0x05 => self.branch(self.reg(rs(opcode)) != self.reg(rt(opcode)), opcode, pc),
            0x06 => self.branch((self.reg(rs(opcode)) as i32) <= 0, opcode, pc),
            0x07 => self.branch((self.reg(rs(opcode)) as i32) > 0, opcode, pc),
            0x08 => self.addi(opcode, pc, true),
            0x09 => self.addi(opcode, pc, false),
            0x0a => self.set_reg(rt(opcode), slt_signed(self.reg(rs(opcode)), imm_se(opcode))),
            0x0b => self.set_reg(rt(opcode), (self.reg(rs(opcode)) < imm_se(opcode)) as u32),
            0x0c => self.set_reg(rt(opcode), self.reg(rs(opcode)) & imm_z(opcode)),
            0x0d => self.set_reg(rt(opcode), self.reg(rs(opcode)) | imm_z(opcode)),
            0x0e => self.set_reg(rt(opcode), self.reg(rs(opcode)) ^ imm_z(opcode)),
            0x0f => self.set_reg(rt(opcode), opcode << 16),
            0x10 => self.execute_cop0(opcode, pc),
            0x11 => self.raise_exception(Exception::CoprocessorUnusable, pc, None, Some(1)),
            0x12 => self.execute_cop2(opcode, pc),
            0x13 => self.raise_exception(Exception::CoprocessorUnusable, pc, None, Some(3)),
            0x20 => self.load8(opcode, bus, true),
            0x21 => self.load16(opcode, bus, pc, true),
            0x22 => self.lwl(opcode, bus),
            0x23 => self.load32(opcode, bus, pc),
            0x24 => self.load8(opcode, bus, false),
            0x25 => self.load16(opcode, bus, pc, false),
            0x26 => self.lwr(opcode, bus),
            0x28 => self.store8(opcode, bus),
            0x29 => self.store16(opcode, bus, pc),
            0x2a => self.swl(opcode, bus),
            0x2b => self.store32(opcode, bus, pc),
            0x2e => self.swr(opcode, bus),
            0x30 => self.raise_exception(Exception::CoprocessorUnusable, pc, None, Some(0)),
            0x31 => self.raise_exception(Exception::CoprocessorUnusable, pc, None, Some(1)),
            0x32 => self.lwc2(opcode, bus, pc),
            0x33 => self.raise_exception(Exception::CoprocessorUnusable, pc, None, Some(3)),
            0x38 => self.raise_exception(Exception::CoprocessorUnusable, pc, None, Some(0)),
            0x39 => self.raise_exception(Exception::CoprocessorUnusable, pc, None, Some(1)),
            0x3a => self.swc2(opcode, bus, pc),
            0x3b => self.raise_exception(Exception::CoprocessorUnusable, pc, None, Some(3)),
            _ => self.raise_exception(Exception::ReservedInstruction, pc, None, None),
        }
    }

    fn execute_special(&mut self, opcode: u32, pc: u32) {
        let shamt = (opcode >> 6) & 0x1f;
        match opcode & 0x3f {
            0x00 => self.set_reg(rd(opcode), self.reg(rt(opcode)) << shamt),
            0x02 => self.set_reg(rd(opcode), self.reg(rt(opcode)) >> shamt),
            0x03 => self.set_reg(rd(opcode), ((self.reg(rt(opcode)) as i32) >> shamt) as u32),
            0x04 => self.set_reg(
                rd(opcode),
                self.reg(rt(opcode)) << (self.reg(rs(opcode)) & 0x1f),
            ),
            0x06 => self.set_reg(
                rd(opcode),
                self.reg(rt(opcode)) >> (self.reg(rs(opcode)) & 0x1f),
            ),
            0x07 => self.set_reg(
                rd(opcode),
                ((self.reg(rt(opcode)) as i32) >> (self.reg(rs(opcode)) & 0x1f)) as u32,
            ),
            0x08 => self.branch_to(self.reg(rs(opcode))),
            0x09 => {
                let link = self.pc.wrapping_add(4);
                let dest = rd(opcode);
                self.branch_to(self.reg(rs(opcode)));
                self.set_reg(if dest == 0 { 31 } else { dest }, link);
            }
            0x0c => self.raise_exception(Exception::Syscall, pc, None, None),
            0x0d => self.raise_exception(Exception::Break, pc, None, None),
            0x10 => self.set_reg(rd(opcode), self.hi),
            0x11 => self.hi = self.reg(rs(opcode)),
            0x12 => self.set_reg(rd(opcode), self.lo),
            0x13 => self.lo = self.reg(rs(opcode)),
            0x18 => self.mult(opcode, true),
            0x19 => self.mult(opcode, false),
            0x1a => self.div(opcode, true),
            0x1b => self.div(opcode, false),
            0x20 => self.add_sub(opcode, pc, true, true),
            0x21 => self.add_sub(opcode, pc, true, false),
            0x22 => self.add_sub(opcode, pc, false, true),
            0x23 => self.add_sub(opcode, pc, false, false),
            0x24 => self.set_reg(rd(opcode), self.reg(rs(opcode)) & self.reg(rt(opcode))),
            0x25 => self.set_reg(rd(opcode), self.reg(rs(opcode)) | self.reg(rt(opcode))),
            0x26 => self.set_reg(rd(opcode), self.reg(rs(opcode)) ^ self.reg(rt(opcode))),
            0x27 => self.set_reg(rd(opcode), !(self.reg(rs(opcode)) | self.reg(rt(opcode)))),
            0x2a => self.set_reg(
                rd(opcode),
                slt_signed(self.reg(rs(opcode)), self.reg(rt(opcode))),
            ),
            0x2b => self.set_reg(
                rd(opcode),
                (self.reg(rs(opcode)) < self.reg(rt(opcode))) as u32,
            ),
            _ => self.raise_exception(Exception::ReservedInstruction, pc, None, None),
        }
    }

    fn execute_bcondz(&mut self, opcode: u32, pc: u32) {
        let value = self.reg(rs(opcode)) as i32;
        match rt(opcode) {
            0x00 => self.branch(value < 0, opcode, pc),
            0x01 => self.branch(value >= 0, opcode, pc),
            0x10 => {
                self.set_reg(31, self.pc.wrapping_add(4));
                self.branch(value < 0, opcode, pc);
            }
            0x11 => {
                self.set_reg(31, self.pc.wrapping_add(4));
                self.branch(value >= 0, opcode, pc);
            }
            _ => self.raise_exception(Exception::ReservedInstruction, pc, None, None),
        }
    }

    fn execute_cop0(&mut self, opcode: u32, pc: u32) {
        let rs_field = rs(opcode);
        match rs_field {
            0x00 => match self.cop0.read_checked(rd(opcode)) {
                Ok(value) => self.set_load(rt(opcode), value),
                Err(exception) => self.raise_exception(exception, pc, None, None),
            },
            0x02 => self.raise_exception(Exception::ReservedInstruction, pc, None, None),
            0x04 => self.cop0.write(rd(opcode), self.reg(rt(opcode))),
            0x06 => self.raise_exception(Exception::ReservedInstruction, pc, None, None),
            0x10 if (opcode & 0x3f) == 0x10 => self.cop0.rfe(),
            0x10 => self.raise_exception(Exception::ReservedInstruction, pc, None, None),
            _ => self.raise_exception(Exception::CoprocessorUnusable, pc, None, Some(0)),
        }
    }

    fn execute_cop2(&mut self, opcode: u32, pc: u32) {
        match rs(opcode) {
            0x00 => self.set_load(rt(opcode), self.gte.read_data(rd(opcode))),
            0x02 => self.set_load(rt(opcode), self.gte.read_control(rd(opcode))),
            0x04 => self.gte.write_data(rd(opcode), self.reg(rt(opcode))),
            0x06 => self.gte.write_control(rd(opcode), self.reg(rt(opcode))),
            0x10..=0x1f => self.gte.execute_command(opcode),
            _ => self.raise_exception(Exception::CoprocessorUnusable, pc, None, Some(2)),
        }
    }

    fn addi(&mut self, opcode: u32, pc: u32, trap_on_overflow: bool) {
        let a = self.reg(rs(opcode));
        let b = imm_se(opcode);
        let result = a.wrapping_add(b);
        if trap_on_overflow && add_overflow(a, b, result) {
            self.raise_exception(Exception::Overflow, pc, None, None);
        } else {
            self.set_reg(rt(opcode), result);
        }
    }

    fn add_sub(&mut self, opcode: u32, pc: u32, add: bool, trap_on_overflow: bool) {
        let a = self.reg(rs(opcode));
        let b = self.reg(rt(opcode));
        let (operand, result) = if add {
            (b, a.wrapping_add(b))
        } else {
            ((!b).wrapping_add(1), a.wrapping_sub(b))
        };
        if trap_on_overflow && add_overflow(a, operand, result) {
            self.raise_exception(Exception::Overflow, pc, None, None);
        } else {
            self.set_reg(rd(opcode), result);
        }
    }

    fn mult(&mut self, opcode: u32, signed: bool) {
        let result = if signed {
            (self.reg(rs(opcode)) as i32 as i64).wrapping_mul(self.reg(rt(opcode)) as i32 as i64)
                as u64
        } else {
            (self.reg(rs(opcode)) as u64).wrapping_mul(self.reg(rt(opcode)) as u64)
        };
        self.lo = result as u32;
        self.hi = (result >> 32) as u32;
    }

    fn div(&mut self, opcode: u32, signed: bool) {
        let n = self.reg(rs(opcode));
        let d = self.reg(rt(opcode));
        if d == 0 {
            self.lo = if signed && (n as i32) < 0 {
                1
            } else {
                0xffff_ffff
            };
            self.hi = n;
            return;
        }
        if signed {
            self.lo = (n as i32).wrapping_div(d as i32) as u32;
            self.hi = (n as i32).wrapping_rem(d as i32) as u32;
        } else {
            self.lo = n / d;
            self.hi = n % d;
        }
    }

    fn jump(&mut self, opcode: u32, _pc: u32, link: bool) {
        if link {
            self.set_reg(31, self.pc.wrapping_add(4));
        }
        let target = (self.pc & 0xf000_0000) | ((opcode & 0x03ff_ffff) << 2);
        self.branch_to(target);
    }

    fn branch(&mut self, condition: bool, opcode: u32, pc: u32) {
        self.next_is_delay_slot = true;
        if condition {
            let offset = ((opcode as i16 as i32) << 2) as u32;
            let target = if self.in_delay_slot {
                self.pc.wrapping_add(offset)
            } else {
                pc.wrapping_add(4).wrapping_add(offset)
            };
            self.next_pc = target;
            self.next_delay_slot_branch_taken = true;
            self.next_delay_slot_branch_target = target;
        } else {
            self.next_delay_slot_branch_taken = false;
            self.next_delay_slot_branch_target = 0;
        }
    }

    fn branch_to(&mut self, target: u32) {
        self.next_pc = target;
        self.next_is_delay_slot = true;
        self.next_delay_slot_branch_taken = true;
        self.next_delay_slot_branch_target = target;
    }

    fn load8(&mut self, opcode: u32, bus: &mut Bus, signed: bool) {
        let value = self.data_read8(bus, ea(self, opcode));
        let extended = if signed {
            value as i8 as i32 as u32
        } else {
            value as u32
        };
        self.set_load(rt(opcode), extended);
    }

    fn load16(&mut self, opcode: u32, bus: &mut Bus, pc: u32, signed: bool) {
        let addr = ea(self, opcode);
        if (addr & 1) != 0 {
            self.raise_exception(Exception::AddressLoad, pc, Some(addr), None);
            return;
        }
        let value = self.data_read16(bus, addr);
        let extended = if signed {
            value as i16 as i32 as u32
        } else {
            value as u32
        };
        self.set_load(rt(opcode), extended);
    }

    fn load32(&mut self, opcode: u32, bus: &mut Bus, pc: u32) {
        let addr = ea(self, opcode);
        if (addr & 3) != 0 {
            self.raise_exception(Exception::AddressLoad, pc, Some(addr), None);
            return;
        }
        let value = self.data_read32(bus, addr);
        self.set_load(rt(opcode), value);
    }

    fn lwl(&mut self, opcode: u32, bus: &mut Bus) {
        let addr = ea(self, opcode);
        let aligned = addr & !3;
        let word = self.data_read32(bus, aligned);
        let old = self.load_merge_base(rt(opcode));
        let value = match addr & 3 {
            0 => (old & 0x00ff_ffff) | (word << 24),
            1 => (old & 0x0000_ffff) | (word << 16),
            2 => (old & 0x0000_00ff) | (word << 8),
            _ => word,
        };
        self.set_load(rt(opcode), value);
    }

    fn lwr(&mut self, opcode: u32, bus: &mut Bus) {
        let addr = ea(self, opcode);
        let aligned = addr & !3;
        let word = self.data_read32(bus, aligned);
        let old = self.load_merge_base(rt(opcode));
        let value = match addr & 3 {
            0 => word,
            1 => (old & 0xff00_0000) | (word >> 8),
            2 => (old & 0xffff_0000) | (word >> 16),
            _ => (old & 0xffff_ff00) | (word >> 24),
        };
        self.set_load(rt(opcode), value);
    }

    fn store8(&mut self, opcode: u32, bus: &mut Bus) {
        self.data_write8(bus, ea(self, opcode), self.reg(rt(opcode)) as u8);
    }

    fn store16(&mut self, opcode: u32, bus: &mut Bus, pc: u32) {
        let addr = ea(self, opcode);
        if (addr & 1) != 0 {
            self.raise_exception(Exception::AddressStore, pc, Some(addr), None);
            return;
        }
        self.data_write16(bus, addr, self.reg(rt(opcode)) as u16);
    }

    fn store32(&mut self, opcode: u32, bus: &mut Bus, pc: u32) {
        let addr = ea(self, opcode);
        if (addr & 3) != 0 {
            self.raise_exception(Exception::AddressStore, pc, Some(addr), None);
            return;
        }
        self.data_write32(bus, addr, self.reg(rt(opcode)));
    }

    fn swl(&mut self, opcode: u32, bus: &mut Bus) {
        let addr = ea(self, opcode);
        let value = self.reg(rt(opcode));
        match addr & 3 {
            0 => self.data_write8(bus, addr, (value >> 24) as u8),
            1 => {
                self.data_write8(bus, addr - 1, (value >> 16) as u8);
                self.data_write8(bus, addr, (value >> 24) as u8);
            }
            2 => {
                self.data_write8(bus, addr - 2, (value >> 8) as u8);
                self.data_write8(bus, addr - 1, (value >> 16) as u8);
                self.data_write8(bus, addr, (value >> 24) as u8);
            }
            _ => self.data_write32(bus, addr & !3, value),
        }
    }

    fn swr(&mut self, opcode: u32, bus: &mut Bus) {
        let addr = ea(self, opcode);
        let value = self.reg(rt(opcode));
        match addr & 3 {
            0 => self.data_write32(bus, addr, value),
            1 => {
                self.data_write8(bus, addr, value as u8);
                self.data_write8(bus, addr + 1, (value >> 8) as u8);
                self.data_write8(bus, addr + 2, (value >> 16) as u8);
            }
            2 => {
                self.data_write8(bus, addr, value as u8);
                self.data_write8(bus, addr + 1, (value >> 8) as u8);
            }
            _ => self.data_write8(bus, addr, value as u8),
        }
    }

    fn lwc2(&mut self, opcode: u32, bus: &mut Bus, pc: u32) {
        let addr = ea(self, opcode);
        if (addr & 3) != 0 {
            self.raise_exception(Exception::AddressLoad, pc, Some(addr), None);
            return;
        }
        let value = self.data_read32(bus, addr);
        self.gte.write_data(rt(opcode), value);
    }

    fn swc2(&mut self, opcode: u32, bus: &mut Bus, pc: u32) {
        let addr = ea(self, opcode);
        if (addr & 3) != 0 {
            self.raise_exception(Exception::AddressStore, pc, Some(addr), None);
            return;
        }
        let value = self.gte.read_data(rt(opcode));
        self.data_write32(bus, addr, value);
    }

    fn data_read8(&self, bus: &mut Bus, addr: u32) -> u8 {
        if self.cop0.cache_isolated() {
            bus.isolated_cache_read8(addr)
        } else {
            bus.read8(addr)
        }
    }

    fn data_read16(&self, bus: &mut Bus, addr: u32) -> u16 {
        if self.cop0.cache_isolated() {
            bus.isolated_cache_read16(addr)
        } else {
            bus.read16(addr)
        }
    }

    fn data_read32(&self, bus: &mut Bus, addr: u32) -> u32 {
        if self.cop0.cache_isolated() {
            bus.isolated_cache_read32(addr)
        } else {
            bus.read32(addr)
        }
    }

    fn data_write8(&self, bus: &mut Bus, addr: u32, value: u8) {
        if self.cop0.cache_isolated() {
            bus.isolated_cache_write8(addr, value);
        } else {
            bus.write8(addr, value);
        }
    }

    fn data_write16(&self, bus: &mut Bus, addr: u32, value: u16) {
        if self.cop0.cache_isolated() {
            bus.isolated_cache_write16(addr, value);
        } else {
            bus.write16(addr, value);
        }
    }

    fn data_write32(&self, bus: &mut Bus, addr: u32, value: u32) {
        if self.cop0.cache_isolated() {
            bus.isolated_cache_write32(addr, value);
        } else {
            bus.write32(addr, value);
        }
    }

    fn reg(&self, index: usize) -> u32 {
        if index == 0 {
            0
        } else {
            self.regs[index]
        }
    }

    fn load_merge_base(&self, index: usize) -> u32 {
        match self.load_delay {
            Some((pending, value)) if pending == index => value,
            _ => match self.load_delay_merge {
                Some((pending, value)) if pending == index => value,
                _ => self.reg(index),
            },
        }
    }

    fn set_reg(&mut self, index: usize, value: u32) {
        if index != 0 {
            self.reg_write_mask |= 1 << index;
            self.regs[index] = value;
        }
    }

    fn write_reg_now(&mut self, index: usize, value: u32) {
        if index != 0 {
            self.regs[index] = value;
        }
    }

    fn set_load(&mut self, index: usize, value: u32) {
        if index != 0 {
            self.load_delay = Some((index, value));
        }
    }

    fn raise_exception(
        &mut self,
        exception: Exception,
        pc: u32,
        bad_vaddr: Option<u32>,
        coprocessor: Option<u8>,
    ) {
        let vector = self.cop0.enter_exception(ExceptionContext {
            exception,
            epc: pc,
            in_delay_slot: self.in_delay_slot,
            delay_slot_branch_taken: self.in_delay_slot_branch_taken,
            delay_slot_branch_target: self.in_delay_slot_branch_target,
            bad_vaddr,
            coprocessor,
        });
        self.pc = vector;
        self.next_pc = vector.wrapping_add(4);
        self.in_delay_slot = false;
        self.in_delay_slot_branch_taken = false;
        self.in_delay_slot_branch_target = 0;
        self.next_is_delay_slot = false;
        self.next_delay_slot_branch_taken = false;
        self.next_delay_slot_branch_target = 0;
        self.load_delay = None;
        self.load_delay_merge = None;
        self.reg_write_mask = 0;
    }
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

fn rs(opcode: u32) -> usize {
    ((opcode >> 21) & 0x1f) as usize
}

fn rt(opcode: u32) -> usize {
    ((opcode >> 16) & 0x1f) as usize
}

fn rd(opcode: u32) -> usize {
    ((opcode >> 11) & 0x1f) as usize
}

fn imm_se(opcode: u32) -> u32 {
    (opcode as u16 as i16 as i32) as u32
}

fn imm_z(opcode: u32) -> u32 {
    opcode & 0xffff
}

fn ea(cpu: &Cpu, opcode: u32) -> u32 {
    cpu.reg(rs(opcode)).wrapping_add(imm_se(opcode))
}

fn slt_signed(a: u32, b: u32) -> u32 {
    ((a as i32) < (b as i32)) as u32
}

fn add_overflow(a: u32, b: u32, result: u32) -> bool {
    ((a ^ result) & (b ^ result) & 0x8000_0000) != 0
}

#[cfg(test)]
mod tests {
    use super::{
        cop0::{CAUSE_BD, CAUSE_BT},
        Cpu,
    };
    use crate::bus::Bus;

    fn i(op: u32, rs: u32, rt: u32, imm: i16) -> u32 {
        (op << 26) | (rs << 21) | (rt << 16) | (imm as u16 as u32)
    }

    fn r(rs: u32, rt: u32, rd: u32, shamt: u32, funct: u32) -> u32 {
        (rs << 21) | (rt << 16) | (rd << 11) | (shamt << 6) | funct
    }

    fn j(op: u32, target: u32) -> u32 {
        (op << 26) | ((target >> 2) & 0x03ff_ffff)
    }

    fn jr(reg: u32) -> u32 {
        r(reg, 0, 0, 0, 0x08)
    }

    fn cop(op: u32, rs: u32, rt: u32, rd: u32, funct: u32) -> u32 {
        (op << 26) | (rs << 21) | (rt << 16) | (rd << 11) | funct
    }

    fn write_program(bus: &mut Bus, base: u32, words: &[u32]) {
        for (i, word) in words.iter().enumerate() {
            bus.write32(base + (i as u32 * 4), *word);
        }
    }

    #[test]
    fn executes_basic_integer_and_memory_ops() {
        let mut cpu = Cpu::new();
        let mut bus = Bus::new(None);
        cpu.set_pc(0x8000_0000);
        write_program(
            &mut bus,
            0x0000_0000,
            &[
                i(0x09, 0, 1, 0x1234), // addiu r1,r0,0x1234
                i(0x0d, 1, 2, 0x00ff), // ori r2,r1,0xff
                i(0x2b, 0, 2, 0x100),  // sw r2,0x100(r0)
                i(0x23, 0, 3, 0x100),  // lw r3,0x100(r0)
                r(3, 2, 4, 0, 0x21),   // addu r4,r3,r2 (load delay sees old r3)
                r(3, 2, 5, 0, 0x21),   // addu r5,r3,r2
            ],
        );

        for _ in 0..6 {
            cpu.step(&mut bus);
        }

        assert_eq!(cpu.regs[1], 0x1234);
        assert_eq!(cpu.regs[2], 0x12ff);
        assert_eq!(cpu.regs[3], 0x12ff);
        assert_eq!(cpu.regs[4], 0x12ff);
        assert_eq!(cpu.regs[5], 0x25fe);
    }

    #[test]
    fn register_write_cancels_pending_load_to_same_register() {
        let mut cpu = Cpu::new();
        let mut bus = Bus::new(None);
        cpu.set_pc(0x8000_0000);
        write_program(
            &mut bus,
            0x0000_0000,
            &[
                i(0x09, 0, 1, 0x100),  // addiu r1,r0,0x100
                i(0x09, 0, 2, 0x1234), // addiu r2,r0,0x1234
                i(0x2b, 1, 2, 0),      // sw r2,0(r1)
                i(0x23, 1, 3, 0),      // lw r3,0(r1)
                i(0x09, 0, 3, 0x55aa), // addiu r3,r0,0x55aa cancels the load
                0x0000_0000,
            ],
        );

        for _ in 0..6 {
            cpu.step(&mut bus);
        }

        assert_eq!(cpu.regs[3], 0x55aa);
    }

    #[test]
    fn jal_link_cancels_pending_load_to_ra() {
        let mut cpu = Cpu::new();
        let mut bus = Bus::new(None);
        cpu.set_pc(0x8000_0000);
        write_program(
            &mut bus,
            0x0000_0000,
            &[
                i(0x09, 0, 1, 0x100), // addiu r1,r0,0x100; RAM at 0x100 is zero
                i(0x23, 1, 31, 0),    // lw ra,0(r1)
                j(0x03, 0x8000_0014), // jal target; link should survive old pending load
                0x0000_0000,          // delay slot
                i(0x09, 0, 2, 1),     // skipped
                r(31, 0, 3, 0, 0x21), // addu r3,ra,r0
            ],
        );

        for _ in 0..6 {
            cpu.step(&mut bus);
        }

        assert_eq!(cpu.regs[2], 0);
        assert_eq!(cpu.regs[3], 0x8000_0010);
    }

    #[test]
    fn consecutive_loads_to_same_register_keep_first_load_invisible() {
        let mut cpu = Cpu::new();
        let mut bus = Bus::new(None);
        cpu.set_pc(0x8000_0000);
        cpu.regs[5] = 4;
        bus.write32(0x0000_0100, 1);
        bus.write32(0x0000_0104, 2);
        write_program(
            &mut bus,
            0x0000_0000,
            &[
                i(0x09, 0, 4, 0x100), // addiu a0,r0,0x100
                i(0x23, 4, 5, 0),     // lw a1,0(a0)
                i(0x23, 4, 5, 4),     // lw a1,4(a0)
                r(5, 0, 2, 0, 0x21),  // move v0,a1; sees original
                r(5, 0, 3, 0, 0x21),  // move v1,a1; sees second load
            ],
        );

        for _ in 0..5 {
            cpu.step(&mut bus);
        }

        assert_eq!(cpu.regs[2], 4);
        assert_eq!(cpu.regs[3], 2);
    }

    #[test]
    fn pcsx_lwl_lwr_no_delay_reads_original_register() {
        let mut cpu = Cpu::new();
        let mut bus = Bus::new(None);
        cpu.set_pc(0x8000_0000);
        cpu.regs[5] = 0xaabb_ccdd;
        bus.write32(0x0000_0100, 0x1122_3344);
        bus.write32(0x0000_0104, 0x5566_7788);
        write_program(
            &mut bus,
            0x0000_0000,
            &[
                i(0x09, 0, 4, 0x100), // addiu a0,r0,0x100
                i(0x22, 4, 5, 4),     // lwl a1,4(a0)
                i(0x26, 4, 5, 1),     // lwr a1,1(a0)
                r(5, 0, 2, 0, 0x21),  // move v0,a1; no delay, sees original
                r(5, 0, 3, 0, 0x21),  // move v1,a1; now sees merged load
            ],
        );

        for _ in 0..5 {
            cpu.step(&mut bus);
        }

        assert_eq!(cpu.regs[2], 0xaabb_ccdd);
        assert_eq!(cpu.regs[3], 0x8811_2233);
    }

    #[test]
    fn pcsx_lwl_lwr_delayed_cases_merge_pending_loads() {
        let mut cpu = Cpu::new();
        let mut bus = Bus::new(None);
        cpu.set_pc(0x8000_0000);
        cpu.regs[5] = 0xaabb_ccdd;
        bus.write32(0x0000_0100, 0x1122_3344);
        bus.write32(0x0000_0104, 0x5566_7788);
        write_program(
            &mut bus,
            0x0000_0000,
            &[
                i(0x09, 0, 4, 0x100), // addiu a0,r0,0x100
                i(0x22, 4, 5, 4),     // lwl a1,4(a0)
                0x0000_0000,          // wait one instruction
                r(5, 0, 2, 0, 0x21),  // move v0,a1
                i(0x22, 4, 5, 4),     // lwl a1,4(a0)
                i(0x26, 4, 5, 1),     // lwr a1,1(a0)
                0x0000_0000,          // wait one instruction
                r(5, 0, 3, 0, 0x21),  // move v1,a1
            ],
        );

        for _ in 0..8 {
            cpu.step(&mut bus);
        }

        assert_eq!(cpu.regs[2], 0x88bb_ccdd);
        assert_eq!(cpu.regs[3], 0x8811_2233);
    }

    #[test]
    fn pcsx_unaligned_load_pairs_merge_different_words_and_lw_base() {
        let mut cpu = Cpu::new();
        let mut bus = Bus::new(None);
        cpu.set_pc(0x8000_0000);
        cpu.regs[5] = 0xeeff_effe;
        bus.write32(0x0000_0100, 0x1122_3344);
        bus.write32(0x0000_0104, 0x5566_7788);
        bus.write32(0x0000_0108, 0xaabb_ccdd);
        write_program(
            &mut bus,
            0x0000_0000,
            &[
                i(0x09, 0, 4, 0x100), // addiu a0,r0,0x100
                i(0x22, 4, 5, 4),     // lwl a1,4(a0)
                i(0x26, 4, 5, 5),     // lwr a1,5(a0)
                0x0000_0000,          // wait one instruction
                r(5, 0, 2, 0, 0x21),  // move v0,a1
                i(0x23, 4, 5, 8),     // lw a1,8(a0)
                i(0x26, 4, 5, 1),     // lwr a1,1(a0)
                0x0000_0000,          // wait one instruction
                r(5, 0, 3, 0, 0x21),  // move v1,a1
            ],
        );

        for _ in 0..9 {
            cpu.step(&mut bus);
        }

        assert_eq!(cpu.regs[2], 0x8855_6677);
        assert_eq!(cpu.regs[3], 0xaa11_2233);
    }

    #[test]
    fn pcsx_divide_by_zero_results_match_r3000a() {
        let mut cpu = Cpu::new();
        let mut bus = Bus::new(None);
        cpu.set_pc(0x8000_0000);
        write_program(
            &mut bus,
            0x0000_0000,
            &[
                i(0x09, 0, 1, 42),   // addiu r1,r0,42
                r(1, 0, 0, 0, 0x1a), // div r1,r0
                r(0, 0, 2, 0, 0x10), // mfhi r2
                r(0, 0, 3, 0, 0x12), // mflo r3
                i(0x09, 0, 4, -42),  // addiu r4,r0,-42
                r(4, 0, 0, 0, 0x1a), // div r4,r0
                r(0, 0, 5, 0, 0x10), // mfhi r5
                r(0, 0, 6, 0, 0x12), // mflo r6
                r(1, 0, 0, 0, 0x1b), // divu r1,r0
                r(0, 0, 7, 0, 0x10), // mfhi r7
                r(0, 0, 8, 0, 0x12), // mflo r8
            ],
        );

        for _ in 0..11 {
            cpu.step(&mut bus);
        }

        assert_eq!(cpu.regs[2], 42);
        assert_eq!(cpu.regs[3], 0xffff_ffff);
        assert_eq!(cpu.regs[5], (-42i32) as u32);
        assert_eq!(cpu.regs[6], 1);
        assert_eq!(cpu.regs[7], 42);
        assert_eq!(cpu.regs[8], 0xffff_ffff);
    }

    #[test]
    fn pcsx_bltzal_not_taken_still_writes_link_register() {
        let mut cpu = Cpu::new();
        let mut bus = Bus::new(None);
        cpu.set_pc(0x8000_0000);
        write_program(
            &mut bus,
            0x0000_0000,
            &[
                i(0x01, 0, 0x10, 1),  // bltzal r0,+1; not taken, but links
                0x0000_0000,          // delay slot
                r(31, 0, 2, 0, 0x21), // move v0,ra
                i(0x09, 0, 3, 1),     // addiu r3,r0,1; confirms fallthrough
            ],
        );

        for _ in 0..4 {
            cpu.step(&mut bus);
        }

        assert_eq!(cpu.regs[2], 0x8000_0008);
        assert_eq!(cpu.regs[3], 1);
    }

    #[test]
    fn pcsx_branch_in_branch_delay_slot_uses_current_pc_base() {
        let mut cpu = Cpu::new();
        let mut bus = Bus::new(None);
        cpu.set_pc(0x8000_0000);
        cpu.regs[31] = 0x8000_003c;
        write_program(
            &mut bus,
            0x0000_0000,
            &[
                i(0x09, 0, 2, 1),   // li v0,1
                i(0x04, 0, 0, 4),   // b t1branch1
                i(0x04, 0, 0, 6),   // b t1branch2; delay slot of first branch
                i(0x0d, 2, 2, 2),   // no
                jr(31),             // no
                i(0x0d, 2, 2, 4),   // no
                i(0x0d, 2, 2, 8),   // t1branch1: yes
                jr(31),             // no
                i(0x0d, 2, 2, 16),  // no
                i(0x0d, 2, 2, 32),  // t1branch2: no
                jr(31),             // no
                i(0x0d, 2, 2, 64),  // no
                i(0x0d, 2, 2, 128), // yes
                jr(31),             // return
                i(0x0d, 2, 2, 256), // delay slot: yes
                0x0000_0000,        // return target
            ],
        );

        for _ in 0..7 {
            cpu.step(&mut bus);
        }

        assert_eq!(cpu.regs[2], 0x189);
    }

    #[test]
    fn pcsx_branch_in_branch_delay_slot_execution_order() {
        let mut cpu = Cpu::new();
        let mut bus = Bus::new(None);
        cpu.set_pc(0x8000_0000);
        cpu.regs[31] = 0x8000_003c;
        write_program(
            &mut bus,
            0x0000_0000,
            &[
                i(0x09, 0, 2, 1),    // li v0,1
                i(0x04, 0, 0, 4),    // b t2branch1
                i(0x04, 0, 0, 6),    // b t2branch2; delay slot of first branch
                i(0x09, 2, 2, 3),    // no
                jr(31),              // no
                r(0, 0, 2, 0, 0x21), // no
                i(0x09, 2, 2, 1),    // t2branch1: v0=2
                jr(31),              // no
                r(0, 0, 2, 0, 0x21), // no
                r(0, 2, 2, 3, 0x00), // t2branch2: no
                jr(31),              // no
                i(0x09, 2, 2, 5),    // no
                r(0, 2, 2, 2, 0x00), // v0=8
                jr(31),              // return
                i(0x09, 2, 2, 1),    // delay slot: v0=9
                0x0000_0000,
            ],
        );

        for _ in 0..7 {
            cpu.step(&mut bus);
        }

        assert_eq!(cpu.regs[2], 9);
    }

    #[test]
    fn pcsx_jump_in_jump_delay_slot_uses_absolute_second_target() {
        let mut cpu = Cpu::new();
        let mut bus = Bus::new(None);
        cpu.set_pc(0x8000_0000);
        cpu.regs[31] = 0x8000_003c;
        write_program(
            &mut bus,
            0x0000_0000,
            &[
                i(0x09, 0, 2, 1),     // li v0,1
                j(0x02, 0x8000_0018), // j t1jump1
                j(0x02, 0x8000_0024), // j t1jump2; delay slot
                i(0x0d, 2, 2, 2),     // no
                jr(31),               // no
                i(0x0d, 2, 2, 4),     // no
                i(0x0d, 2, 2, 8),     // t1jump1: yes
                jr(31),               // no
                i(0x0d, 2, 2, 16),    // no
                i(0x0d, 2, 2, 32),    // t1jump2: yes
                jr(31),               // return
                i(0x0d, 2, 2, 64),    // delay slot: yes
                i(0x0d, 2, 2, 128),   // no
                jr(31),               // no
                i(0x0d, 2, 2, 256),   // no
                0x0000_0000,
            ],
        );

        for _ in 0..7 {
            cpu.step(&mut bus);
        }

        assert_eq!(cpu.regs[2], 0x69);
    }

    #[test]
    fn pcsx_jump_in_jump_delay_slot_execution_order() {
        let mut cpu = Cpu::new();
        let mut bus = Bus::new(None);
        cpu.set_pc(0x8000_0000);
        cpu.regs[31] = 0x8000_003c;
        write_program(
            &mut bus,
            0x0000_0000,
            &[
                i(0x09, 0, 2, 1),     // li v0,1
                j(0x02, 0x8000_0018), // j t2jump1
                j(0x02, 0x8000_0024), // j t2jump2; delay slot
                i(0x09, 2, 2, 3),     // no
                jr(31),               // no
                r(0, 0, 2, 0, 0x21),  // no
                i(0x09, 2, 2, 1),     // t2jump1: v0=2
                jr(31),               // no
                r(0, 0, 2, 0, 0x21),  // no
                r(0, 2, 2, 3, 0x00),  // t2jump2: v0=16
                jr(31),               // return
                i(0x09, 2, 2, 5),     // delay slot: v0=21
                r(0, 2, 2, 2, 0x00),  // no
                jr(31),               // no
                i(0x09, 2, 2, 1),     // no
                0x0000_0000,
            ],
        );

        for _ in 0..7 {
            cpu.step(&mut bus);
        }

        assert_eq!(cpu.regs[2], 21);
    }

    #[test]
    fn executes_branch_delay_slot() {
        let mut cpu = Cpu::new();
        let mut bus = Bus::new(None);
        cpu.set_pc(0x8000_0000);
        write_program(
            &mut bus,
            0x0000_0000,
            &[
                i(0x09, 0, 1, 1), // addiu r1,r0,1
                i(0x04, 1, 1, 2), // beq r1,r1,+2, target 0x10
                i(0x09, 0, 2, 2), // delay slot
                i(0x09, 0, 2, 3), // skipped
                i(0x09, 0, 3, 4), // target
            ],
        );

        for _ in 0..5 {
            cpu.step(&mut bus);
        }

        assert_eq!(cpu.regs[2], 2);
        assert_eq!(cpu.regs[3], 4);
    }

    #[test]
    fn raises_syscall_exception_to_boot_vector() {
        let mut cpu = Cpu::new();
        let mut bus = Bus::new(None);
        cpu.set_pc(0x8000_0000);
        write_program(&mut bus, 0x0000_0000, &[0x0000_000c]);

        cpu.step(&mut bus);

        assert_eq!(cpu.pc, 0xbfc0_0180);
        assert_eq!(cpu.cop0.epc(), 0x8000_0000);
    }

    #[test]
    fn exception_in_taken_branch_delay_slot_sets_bd_and_branch_epc() {
        let mut cpu = Cpu::new();
        let mut bus = Bus::new(None);
        cpu.set_pc(0x8000_0000);
        write_program(
            &mut bus,
            0x0000_0000,
            &[
                i(0x04, 0, 0, 1), // beq r0,r0,+1
                i(0x23, 0, 1, 1), // delay slot: unaligned lw
                i(0x09, 0, 2, 2),
            ],
        );

        cpu.step(&mut bus);
        cpu.step(&mut bus);

        assert_eq!(cpu.pc, 0xbfc0_0180);
        assert_eq!(cpu.cop0.epc(), 0x8000_0000);
        assert_eq!(cpu.cop0.bad_vaddr(), 0x0000_0001);
        assert_ne!(cpu.cop0.cause() & CAUSE_BD, 0);
        assert_ne!(cpu.cop0.cause() & CAUSE_BT, 0);
        assert_eq!(cpu.cop0.read(6), 0x8000_0008);
        assert_eq!((cpu.cop0.cause() >> 2) & 0x1f, 4);
    }

    #[test]
    fn exception_in_not_taken_branch_delay_slot_still_sets_bd() {
        let mut cpu = Cpu::new();
        let mut bus = Bus::new(None);
        cpu.set_pc(0x8000_0000);
        write_program(
            &mut bus,
            0x0000_0000,
            &[
                i(0x09, 0, 1, 1), // addiu r1,r0,1
                i(0x04, 0, 1, 1), // beq r0,r1,+1 (not taken)
                i(0x23, 0, 2, 1), // delay slot: unaligned lw
                i(0x09, 0, 3, 3),
            ],
        );

        cpu.step(&mut bus);
        cpu.step(&mut bus);
        cpu.step(&mut bus);

        assert_eq!(cpu.pc, 0xbfc0_0180);
        assert_eq!(cpu.cop0.epc(), 0x8000_0004);
        assert_eq!(cpu.cop0.bad_vaddr(), 0x0000_0001);
        assert_ne!(cpu.cop0.cause() & CAUSE_BD, 0);
        assert_eq!(cpu.cop0.cause() & CAUSE_BT, 0);
        assert_eq!(cpu.cop0.read(6), 0);
        assert_eq!((cpu.cop0.cause() >> 2) & 0x1f, 4);
    }

    #[test]
    fn invalid_cop0_register_read_raises_reserved_instruction() {
        let mut cpu = Cpu::new();
        let mut bus = Bus::new(None);
        cpu.set_pc(0x8000_0000);
        write_program(
            &mut bus,
            0x0000_0000,
            &[
                cop(0x10, 0x00, 1, 0, 0), // mfc0 r1,BPC; invalid on PS1
            ],
        );

        cpu.step(&mut bus);

        assert_eq!(cpu.pc, 0xbfc0_0180);
        assert_eq!(cpu.cop0.epc(), 0x8000_0000);
        assert_eq!((cpu.cop0.cause() >> 2) & 0x1f, 10);
    }

    #[test]
    fn unusable_coprocessor_exception_sets_cause_ce() {
        let mut cpu = Cpu::new();
        let mut bus = Bus::new(None);
        cpu.set_pc(0x8000_0000);
        write_program(
            &mut bus,
            0x0000_0000,
            &[
                cop(0x11, 0x10, 0, 0, 0), // COP1 command
            ],
        );

        cpu.step(&mut bus);

        assert_eq!(cpu.pc, 0xbfc0_0180);
        assert_eq!((cpu.cop0.cause() >> 2) & 0x1f, 11);
        assert_eq!((cpu.cop0.cause() >> 28) & 0x3, 1);
    }

    #[test]
    fn unusable_lwc_swc_cop0_paths_set_cause_ce_zero() {
        let mut cpu = Cpu::new();
        let mut bus = Bus::new(None);
        cpu.set_pc(0x8000_0000);
        write_program(
            &mut bus,
            0x0000_0000,
            &[
                i(0x30, 0, 1, 0), // lwc0 r1,0(r0)
            ],
        );

        cpu.step(&mut bus);

        assert_eq!(cpu.pc, 0xbfc0_0180);
        assert_eq!((cpu.cop0.cause() >> 2) & 0x1f, 11);
        assert_eq!((cpu.cop0.cause() >> 28) & 0x3, 0);

        cpu.set_pc(0x8000_0004);
        write_program(
            &mut bus,
            0x0000_0004,
            &[
                i(0x38, 0, 1, 0), // swc0 r1,0(r0)
            ],
        );

        cpu.step(&mut bus);

        assert_eq!(cpu.pc, 0xbfc0_0180);
        assert_eq!((cpu.cop0.cause() >> 2) & 0x1f, 11);
        assert_eq!((cpu.cop0.cause() >> 28) & 0x3, 0);
    }

    #[test]
    fn software_interrupt_uses_status_mask_and_cause_pending_bits() {
        let mut cpu = Cpu::new();
        let mut bus = Bus::new(None);
        cpu.set_pc(0x8000_0000);
        write_program(
            &mut bus,
            0x0000_0000,
            &[
                i(0x09, 0, 1, 0x0101),     // addiu r1,r0,IE|IM0
                cop(0x10, 0x04, 1, 12, 0), // mtc0 r1,SR
                i(0x09, 0, 2, 0x0100),     // addiu r2,r0,software interrupt 0
                cop(0x10, 0x04, 2, 13, 0), // mtc0 r2,Cause
                0x0000_0000,               // interrupted before this executes
            ],
        );

        for _ in 0..5 {
            cpu.step(&mut bus);
        }

        assert_eq!(cpu.pc, 0x8000_0080);
        assert_eq!(cpu.cop0.epc(), 0x8000_0010);
        assert_eq!((cpu.cop0.cause() >> 2) & 0x1f, 0);
        assert_ne!(cpu.cop0.cause() & 0x0100, 0);
    }

    #[test]
    fn misaligned_instruction_fetch_sets_bad_vaddr() {
        let mut cpu = Cpu::new();
        let mut bus = Bus::new(None);
        cpu.set_pc(0x8000_0002);

        cpu.step(&mut bus);

        assert_eq!(cpu.pc, 0xbfc0_0180);
        assert_eq!(cpu.cop0.epc(), 0x8000_0002);
        assert_eq!(cpu.cop0.bad_vaddr(), 0x8000_0002);
        assert_eq!(cpu.cop0.cause() & CAUSE_BD, 0);
        assert_eq!((cpu.cop0.cause() >> 2) & 0x1f, 4);
    }

    #[test]
    fn unaligned_store_and_load_pairs_use_little_endian_byte_order() {
        let mut cpu = Cpu::new();
        let mut bus = Bus::new(None);
        cpu.set_pc(0x8000_0000);
        write_program(
            &mut bus,
            0x0000_0000,
            &[
                i(0x09, 0, 1, 0x100),  // addiu r1,r0,0x100
                i(0x0f, 0, 2, 0x1122), // lui r2,0x1122
                i(0x0d, 2, 2, 0x3344), // ori r2,r2,0x3344
                i(0x2a, 1, 2, 4),      // swl r2,4(r1)
                i(0x2e, 1, 2, 1),      // swr r2,1(r1)
                i(0x22, 1, 3, 4),      // lwl r3,4(r1)
                i(0x26, 1, 3, 1),      // lwr r3,1(r1)
                0x0000_0000,           // nop; wait for load delay
            ],
        );

        for _ in 0..8 {
            cpu.step(&mut bus);
        }

        assert_eq!(bus.read8(0x101), 0x44);
        assert_eq!(bus.read8(0x102), 0x33);
        assert_eq!(bus.read8(0x103), 0x22);
        assert_eq!(bus.read8(0x104), 0x11);
        assert_eq!(cpu.regs[3], 0x1122_3344);
    }

    #[test]
    fn isolated_cache_mode_redirects_data_stores_away_from_ram() {
        let mut cpu = Cpu::new();
        let mut bus = Bus::new(None);
        cpu.set_pc(0x8000_0000);
        write_program(
            &mut bus,
            0x0000_0000,
            &[
                i(0x0f, 0, 1, 0x0001),     // lui r1,0x0001 ; COP0 SR.IsC
                cop(0x10, 0x04, 1, 12, 0), // mtc0 r1,SR
                i(0x09, 0, 2, 0x1234),     // addiu r2,r0,0x1234
                i(0x2b, 0, 2, 0x100),      // sw r2,0x100(r0) ; isolated
                cop(0x10, 0x04, 0, 12, 0), // mtc0 r0,SR
                i(0x23, 0, 3, 0x100),      // lw r3,0x100(r0) ; normal RAM
                0x0000_0000,               // nop; wait for load delay
            ],
        );

        for _ in 0..7 {
            cpu.step(&mut bus);
        }

        assert_eq!(bus.read32(0x0000_0100), 0);
        assert_eq!(bus.isolated_cache_read32(0x0000_0100), 0x0000_1234);
        assert_eq!(cpu.regs[3], 0);
    }
}
