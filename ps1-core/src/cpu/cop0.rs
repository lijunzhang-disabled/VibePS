use serde::{Deserialize, Serialize};

pub const STATUS_IE: u32 = 1 << 0;
pub const STATUS_ISC: u32 = 1 << 16;
pub const STATUS_BEV: u32 = 1 << 22;
pub const CAUSE_IP2: u32 = 1 << 10;
pub const CAUSE_BT: u32 = 1 << 30;
pub const CAUSE_BD: u32 = 1 << 31;
const CAUSE_EXCODE_MASK: u32 = 0x7c;
const CAUSE_CE_MASK: u32 = 0x3000_0000;
const INTERRUPT_MASK: u32 = 0xff00;
const CAUSE_SOFTWARE_INTERRUPT_MASK: u32 = 0x0300;
const CAUSE_BT_MASK: u32 = CAUSE_BT;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exception {
    Interrupt = 0,
    AddressLoad = 4,
    AddressStore = 5,
    Syscall = 8,
    Break = 9,
    ReservedInstruction = 10,
    CoprocessorUnusable = 11,
    Overflow = 12,
}

pub(super) struct ExceptionContext {
    pub exception: Exception,
    pub epc: u32,
    pub in_delay_slot: bool,
    pub delay_slot_branch_taken: bool,
    pub delay_slot_branch_target: u32,
    pub bad_vaddr: Option<u32>,
    pub coprocessor: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cop0 {
    regs: [u32; 16],
}

impl Cop0 {
    pub fn new() -> Self {
        let mut regs = [0; 16];
        regs[12] = STATUS_BEV;
        regs[15] = 0x0000_0002;
        Self { regs }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn read(&self, index: usize) -> u32 {
        self.regs.get(index).copied().unwrap_or(0)
    }

    pub fn read_checked(&self, index: usize) -> Result<u32, Exception> {
        match index {
            0..=2 | 4 | 10 => Err(Exception::ReservedInstruction),
            16..=31 => Ok(0x0000_0020),
            _ => Ok(self.read(index)),
        }
    }

    pub fn write(&mut self, index: usize, value: u32) {
        match index {
            // Cause is read-only except software interrupt bits 8-9.
            13 => {
                self.regs[13] = (self.regs[13] & !CAUSE_SOFTWARE_INTERRUPT_MASK)
                    | (value & CAUSE_SOFTWARE_INTERRUPT_MASK);
            }
            // PRId and BadVaddr are read-only.
            8 | 15 => {}
            _ => {
                if let Some(reg) = self.regs.get_mut(index) {
                    *reg = value;
                }
            }
        }
    }

    pub fn status(&self) -> u32 {
        self.regs[12]
    }

    pub fn cause(&self) -> u32 {
        self.regs[13]
    }

    pub fn epc(&self) -> u32 {
        self.regs[14]
    }

    pub fn bad_vaddr(&self) -> u32 {
        self.regs[8]
    }

    pub fn set_interrupt_pending(&mut self, pending: bool) {
        if pending {
            self.regs[13] |= CAUSE_IP2;
        } else {
            self.regs[13] &= !CAUSE_IP2;
        }
    }

    pub fn interrupts_enabled(&self) -> bool {
        (self.regs[12] & STATUS_IE) != 0 && (self.regs[12] & self.regs[13] & INTERRUPT_MASK) != 0
    }

    pub fn cache_isolated(&self) -> bool {
        (self.regs[12] & STATUS_ISC) != 0
    }

    pub(super) fn enter_exception(&mut self, context: ExceptionContext) -> u32 {
        let ExceptionContext {
            exception,
            epc,
            in_delay_slot,
            delay_slot_branch_taken,
            delay_slot_branch_target,
            bad_vaddr,
            coprocessor,
        } = context;
        if let Some(addr) = bad_vaddr {
            self.regs[8] = addr;
        }
        self.regs[14] = if in_delay_slot {
            epc.wrapping_sub(4)
        } else {
            epc
        };
        let mut cause = self.regs[13] & !(CAUSE_EXCODE_MASK | CAUSE_CE_MASK | CAUSE_BT_MASK);
        cause |= (exception as u32) << 2;
        if let Some(cop) = coprocessor {
            cause |= ((cop as u32) & 0x3) << 28;
        }
        if in_delay_slot {
            cause |= CAUSE_BD;
            if delay_slot_branch_taken {
                cause |= CAUSE_BT;
                self.regs[6] = delay_slot_branch_target;
            }
        } else {
            cause &= !CAUSE_BD;
        }
        self.regs[13] = cause;
        self.regs[12] = (self.regs[12] & !0x3f) | ((self.regs[12] << 2) & 0x3f);
        if (self.regs[12] & STATUS_BEV) != 0 {
            0xbfc0_0180
        } else {
            0x8000_0080
        }
    }

    pub fn rfe(&mut self) {
        self.regs[12] = (self.regs[12] & !0x0f) | ((self.regs[12] >> 2) & 0x0f);
    }
}

impl Default for Cop0 {
    fn default() -> Self {
        Self::new()
    }
}
