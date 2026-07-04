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
            self.write_reg_now(reg, value);
        }
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
            let target = pc.wrapping_add(4).wrapping_add(offset);
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
        let value = bus.read8(ea(self, opcode));
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
        let value = bus.read16(addr);
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
        let value = bus.read32(addr);
        self.set_load(rt(opcode), value);
    }

    fn lwl(&mut self, opcode: u32, bus: &mut Bus) {
        let addr = ea(self, opcode);
        let aligned = addr & !3;
        let word = bus.read32(aligned);
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
        let word = bus.read32(aligned);
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
        bus.write8(ea(self, opcode), self.reg(rt(opcode)) as u8);
    }

    fn store16(&mut self, opcode: u32, bus: &mut Bus, pc: u32) {
        let addr = ea(self, opcode);
        if (addr & 1) != 0 {
            self.raise_exception(Exception::AddressStore, pc, Some(addr), None);
            return;
        }
        bus.write16(addr, self.reg(rt(opcode)) as u16);
    }

    fn store32(&mut self, opcode: u32, bus: &mut Bus, pc: u32) {
        let addr = ea(self, opcode);
        if (addr & 3) != 0 {
            self.raise_exception(Exception::AddressStore, pc, Some(addr), None);
            return;
        }
        bus.write32(addr, self.reg(rt(opcode)));
    }

    fn swl(&mut self, opcode: u32, bus: &mut Bus) {
        let addr = ea(self, opcode);
        let value = self.reg(rt(opcode));
        match addr & 3 {
            0 => bus.write8(addr, (value >> 24) as u8),
            1 => {
                bus.write8(addr - 1, (value >> 16) as u8);
                bus.write8(addr, (value >> 24) as u8);
            }
            2 => {
                bus.write8(addr - 2, (value >> 8) as u8);
                bus.write8(addr - 1, (value >> 16) as u8);
                bus.write8(addr, (value >> 24) as u8);
            }
            _ => bus.write32(addr & !3, value),
        }
    }

    fn swr(&mut self, opcode: u32, bus: &mut Bus) {
        let addr = ea(self, opcode);
        let value = self.reg(rt(opcode));
        match addr & 3 {
            0 => bus.write32(addr, value),
            1 => {
                bus.write8(addr, value as u8);
                bus.write8(addr + 1, (value >> 8) as u8);
                bus.write8(addr + 2, (value >> 16) as u8);
            }
            2 => {
                bus.write8(addr, value as u8);
                bus.write8(addr + 1, (value >> 8) as u8);
            }
            _ => bus.write8(addr, value as u8),
        }
    }

    fn lwc2(&mut self, opcode: u32, bus: &mut Bus, pc: u32) {
        let addr = ea(self, opcode);
        if (addr & 3) != 0 {
            self.raise_exception(Exception::AddressLoad, pc, Some(addr), None);
            return;
        }
        let value = bus.read32(addr);
        self.gte.write_data(rt(opcode), value);
    }

    fn swc2(&mut self, opcode: u32, bus: &mut Bus, pc: u32) {
        let addr = ea(self, opcode);
        if (addr & 3) != 0 {
            self.raise_exception(Exception::AddressStore, pc, Some(addr), None);
            return;
        }
        let value = self.gte.read_data(rt(opcode));
        bus.write32(addr, value);
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
}
