use crate::{cpu::cop0::Exception, ExeLoadError, Ps1};

const DEFAULT_MAX_STEPS: u64 = 1_000_000;
const EXCODE_MASK: u32 = 0x7c;

#[derive(Debug, Clone)]
pub struct PsxExeTestConfig {
    pub max_steps: u64,
    pub stop_conditions: Vec<PsxExeStopCondition>,
    pub pass_conditions: Vec<PsxExePassCondition>,
    pub exit_code_source: Option<PsxExeExitCodeSource>,
}

impl PsxExeTestConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_max_steps(mut self, max_steps: u64) -> Self {
        self.max_steps = max_steps;
        self
    }

    pub fn stop_when(mut self, condition: PsxExeStopCondition) -> Self {
        self.stop_conditions.push(condition);
        self
    }

    pub fn pass_when(mut self, condition: PsxExePassCondition) -> Self {
        self.pass_conditions.push(condition);
        self
    }

    pub fn exit_code_from(mut self, source: PsxExeExitCodeSource) -> Self {
        self.exit_code_source = Some(source);
        self
    }

    pub fn with_mailbox32(mut self, addr: u32, pass_value: u32) -> Self {
        self.stop_conditions
            .push(PsxExeStopCondition::Memory32NonZero { addr });
        self.pass_conditions
            .push(PsxExePassCondition::Memory32Equals {
                addr,
                value: pass_value,
            });
        self.exit_code_source = Some(PsxExeExitCodeSource::Memory32(addr));
        self
    }
}

impl Default for PsxExeTestConfig {
    fn default() -> Self {
        Self {
            max_steps: DEFAULT_MAX_STEPS,
            stop_conditions: vec![PsxExeStopCondition::Exception(Exception::Break)],
            pass_conditions: Vec::new(),
            exit_code_source: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PsxExeTestRunner {
    ps1: Ps1,
    config: PsxExeTestConfig,
}

impl PsxExeTestRunner {
    pub fn new(
        bios: Option<Vec<u8>>,
        exe: &[u8],
        config: PsxExeTestConfig,
    ) -> Result<Self, ExeLoadError> {
        let mut ps1 = Ps1::new(bios);
        ps1.load_psx_exe(exe)?;
        Ok(Self { ps1, config })
    }

    pub fn ps1(&self) -> &Ps1 {
        &self.ps1
    }

    pub fn ps1_mut(&mut self) -> &mut Ps1 {
        &mut self.ps1
    }

    pub fn run(&mut self) -> PsxExeRunReport {
        self.ps1.run_loaded_psx_exe_test(&self.config)
    }
}

impl Ps1 {
    pub fn run_loaded_psx_exe_test(&mut self, config: &PsxExeTestConfig) -> PsxExeRunReport {
        let max_steps = config.max_steps.max(1);
        let mut steps = 0u64;
        let mut cycles = 0u64;

        if let Some(reason) = find_stop_reason(self, config) {
            return finish_report(self, config, steps, cycles, reason);
        }

        while steps < max_steps {
            cycles = cycles.saturating_add(self.step_one() as u64);
            steps += 1;

            if let Some(reason) = find_stop_reason(self, config) {
                return finish_report(self, config, steps, cycles, reason);
            }
        }

        PsxExeRunReport {
            status: PsxExeRunStatus::TimedOut,
            steps,
            cycles,
            stop_reason: None,
            exit_code: read_exit_code(self, config.exit_code_source),
            pc: self.cpu.pc,
            next_pc: self.cpu.next_pc,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PsxExeStopCondition {
    Pc(u32),
    RegisterEquals { reg: u8, value: u32 },
    Memory32Equals { addr: u32, value: u32 },
    Memory32NonZero { addr: u32 },
    Exception(Exception),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PsxExePassCondition {
    RegisterEquals { reg: u8, value: u32 },
    Memory32Equals { addr: u32, value: u32 },
    Exception(Exception),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PsxExeExitCodeSource {
    Register(u8),
    Memory32(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PsxExeRunStatus {
    Passed,
    Failed,
    Stopped,
    TimedOut,
}

impl PsxExeRunStatus {
    pub fn is_success(self) -> bool {
        matches!(self, Self::Passed | Self::Stopped)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PsxExeStopReason {
    Pc(u32),
    RegisterEquals { reg: u8, value: u32 },
    Memory32Equals { addr: u32, value: u32 },
    Memory32NonZero { addr: u32, value: u32 },
    Exception(Exception),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PsxExeRunReport {
    pub status: PsxExeRunStatus,
    pub steps: u64,
    pub cycles: u64,
    pub stop_reason: Option<PsxExeStopReason>,
    pub exit_code: Option<u32>,
    pub pc: u32,
    pub next_pc: u32,
}

fn finish_report(
    ps1: &Ps1,
    config: &PsxExeTestConfig,
    steps: u64,
    cycles: u64,
    reason: PsxExeStopReason,
) -> PsxExeRunReport {
    let status = if config.pass_conditions.is_empty() {
        PsxExeRunStatus::Stopped
    } else if config
        .pass_conditions
        .iter()
        .all(|condition| pass_condition_matches(ps1, *condition))
    {
        PsxExeRunStatus::Passed
    } else {
        PsxExeRunStatus::Failed
    };
    PsxExeRunReport {
        status,
        steps,
        cycles,
        stop_reason: Some(reason),
        exit_code: read_exit_code(ps1, config.exit_code_source),
        pc: ps1.cpu.pc,
        next_pc: ps1.cpu.next_pc,
    }
}

fn find_stop_reason(ps1: &Ps1, config: &PsxExeTestConfig) -> Option<PsxExeStopReason> {
    config
        .stop_conditions
        .iter()
        .find_map(|condition| stop_condition_matches(ps1, *condition))
}

fn stop_condition_matches(ps1: &Ps1, condition: PsxExeStopCondition) -> Option<PsxExeStopReason> {
    match condition {
        PsxExeStopCondition::Pc(pc) if ps1.cpu.pc == pc => Some(PsxExeStopReason::Pc(pc)),
        PsxExeStopCondition::Pc(_) => None,
        PsxExeStopCondition::RegisterEquals { reg, value } => {
            let actual = read_reg(ps1, reg)?;
            (actual == value).then_some(PsxExeStopReason::RegisterEquals { reg, value })
        }
        PsxExeStopCondition::Memory32Equals { addr, value } => {
            let actual = ps1.bus.peek32(addr);
            (actual == value).then_some(PsxExeStopReason::Memory32Equals { addr, value })
        }
        PsxExeStopCondition::Memory32NonZero { addr } => {
            let value = ps1.bus.peek32(addr);
            (value != 0).then_some(PsxExeStopReason::Memory32NonZero { addr, value })
        }
        PsxExeStopCondition::Exception(exception) => {
            exception_matches(ps1, exception).then_some(PsxExeStopReason::Exception(exception))
        }
    }
}

fn pass_condition_matches(ps1: &Ps1, condition: PsxExePassCondition) -> bool {
    match condition {
        PsxExePassCondition::RegisterEquals { reg, value } => {
            read_reg(ps1, reg).is_some_and(|actual| actual == value)
        }
        PsxExePassCondition::Memory32Equals { addr, value } => ps1.bus.peek32(addr) == value,
        PsxExePassCondition::Exception(exception) => exception_matches(ps1, exception),
    }
}

fn read_exit_code(ps1: &Ps1, source: Option<PsxExeExitCodeSource>) -> Option<u32> {
    match source? {
        PsxExeExitCodeSource::Register(reg) => read_reg(ps1, reg),
        PsxExeExitCodeSource::Memory32(addr) => Some(ps1.bus.peek32(addr)),
    }
}

fn read_reg(ps1: &Ps1, reg: u8) -> Option<u32> {
    ps1.cpu.regs.get(reg as usize).copied()
}

fn exception_matches(ps1: &Ps1, exception: Exception) -> bool {
    ((ps1.cpu.cop0.cause() & EXCODE_MASK) >> 2) == exception as u32
}

#[cfg(test)]
mod tests {
    use super::{
        Exception, PsxExeExitCodeSource, PsxExePassCondition, PsxExeRunStatus, PsxExeStopCondition,
        PsxExeStopReason, PsxExeTestConfig, PsxExeTestRunner,
    };
    use crate::cdrom::CdromSectorSize;

    const LOAD_ADDR: u32 = 0x8001_0000;
    const MAILBOX: u32 = 0x8001_0100;
    const CDROM_MAILBOX: u32 = 0x8001_4000;
    const CDROM_DMA_BUFFER: u32 = 0x8001_4400;

    #[test]
    fn mailbox_runner_passes_when_guest_writes_expected_code() {
        let exe = psx_exe(&mailbox_program(1));
        let mut runner = PsxExeTestRunner::new(
            None,
            &exe,
            PsxExeTestConfig::new()
                .with_max_steps(16)
                .with_mailbox32(MAILBOX, 1),
        )
        .unwrap();

        let report = runner.run();

        assert_eq!(report.status, PsxExeRunStatus::Passed);
        assert_eq!(report.exit_code, Some(1));
        assert_eq!(runner.ps1().bus.peek32(MAILBOX), 1);
    }

    #[test]
    fn mailbox_runner_fails_when_guest_writes_unexpected_code() {
        let exe = psx_exe(&mailbox_program(2));
        let mut runner = PsxExeTestRunner::new(
            None,
            &exe,
            PsxExeTestConfig::new()
                .with_max_steps(16)
                .with_mailbox32(MAILBOX, 1),
        )
        .unwrap();

        let report = runner.run();

        assert_eq!(report.status, PsxExeRunStatus::Failed);
        assert_eq!(report.exit_code, Some(2));
    }

    #[test]
    fn runner_times_out_when_no_stop_condition_matches() {
        let exe = psx_exe(&self_loop_program());
        let mut runner = PsxExeTestRunner::new(
            None,
            &exe,
            PsxExeTestConfig::new()
                .with_max_steps(4)
                .with_mailbox32(MAILBOX, 1),
        )
        .unwrap();

        let report = runner.run();

        assert_eq!(report.status, PsxExeRunStatus::TimedOut);
        assert_eq!(report.steps, 4);
        assert_eq!(report.exit_code, Some(0));
    }

    #[test]
    fn runner_can_stop_on_pc_and_check_registers() {
        let stop_pc = LOAD_ADDR + 12;
        let exe = psx_exe(&[addiu(2, 0, 7), j(stop_pc), 0, j(stop_pc), 0]);
        let mut runner = PsxExeTestRunner::new(
            None,
            &exe,
            PsxExeTestConfig::new()
                .with_max_steps(8)
                .stop_when(PsxExeStopCondition::Pc(stop_pc))
                .pass_when(PsxExePassCondition::RegisterEquals { reg: 2, value: 7 })
                .exit_code_from(PsxExeExitCodeSource::Register(2)),
        )
        .unwrap();

        let report = runner.run();

        assert_eq!(report.status, PsxExeRunStatus::Passed);
        assert_eq!(report.exit_code, Some(7));
    }

    #[test]
    fn runner_stops_on_break_exception_by_default() {
        let exe = psx_exe(&[break_()]);
        let mut runner =
            PsxExeTestRunner::new(None, &exe, PsxExeTestConfig::new().with_max_steps(4)).unwrap();

        let report = runner.run();

        assert_eq!(report.status, PsxExeRunStatus::Stopped);
        assert_eq!(
            report.stop_reason,
            Some(PsxExeStopReason::Exception(Exception::Break))
        );
    }

    #[test]
    fn psx_exe_runner_executes_cdrom_command_and_dma_smoke_test() {
        let exe = psx_exe(&cdrom_guest_smoke_program());
        let mut runner = PsxExeTestRunner::new(
            None,
            &exe,
            PsxExeTestConfig::new()
                .with_max_steps(512)
                .with_mailbox32(CDROM_MAILBOX, 1),
        )
        .unwrap();
        runner
            .ps1_mut()
            .bus
            .cdrom
            .load_disc_image(cooked_disc(2), CdromSectorSize::Cooked2048)
            .unwrap();

        let report = runner.run();

        assert_eq!(report.status, PsxExeRunStatus::Passed, "{report:?}");
        assert_eq!(report.exit_code, Some(1));
        assert_eq!(runner.ps1().bus.peek32(CDROM_DMA_BUFFER), 0x0302_0100);
        assert_eq!(runner.ps1().bus.peek32(CDROM_DMA_BUFFER + 4), 0x0706_0504);
    }

    fn mailbox_program(value: u32) -> Vec<u32> {
        vec![
            lui(8, (MAILBOX >> 16) as u16),
            ori(8, 8, MAILBOX as u16),
            addiu(9, 0, value as i16),
            sw(9, 8, 0),
            j(LOAD_ADDR + 16),
            0,
        ]
    }

    fn self_loop_program() -> Vec<u32> {
        vec![j(LOAD_ADDR), 0]
    }

    fn cdrom_guest_smoke_program() -> Vec<u32> {
        let mut words = Vec::new();
        push_li(&mut words, 8, 0x1f80_1800); // CD-ROM register base
        push_li(&mut words, 9, CDROM_MAILBOX);
        push_li(&mut words, 10, CDROM_DMA_BUFFER);
        push_li(&mut words, 13, 0x1f80_1000); // DMA register base

        cdrom_command(&mut words, 0x13, &[]); // GetTN
        cdrom_expect_flags(&mut words, 0x03, 0x101);
        cdrom_expect_response(&mut words, 0x02, 0x102);
        cdrom_expect_response(&mut words, 0x01, 0x103);
        cdrom_expect_response(&mut words, 0x01, 0x104);
        cdrom_ack(&mut words);

        cdrom_command(&mut words, 0x14, &[0x01]); // GetTD track 1
        cdrom_expect_flags(&mut words, 0x03, 0x111);
        cdrom_expect_response(&mut words, 0x02, 0x112);
        cdrom_expect_response(&mut words, 0x00, 0x113);
        cdrom_expect_response(&mut words, 0x02, 0x114);
        cdrom_expect_response(&mut words, 0x00, 0x115);
        cdrom_ack(&mut words);

        cdrom_command(&mut words, 0x0e, &[0x80]); // Setmode
        cdrom_expect_flags(&mut words, 0x03, 0x121);
        cdrom_expect_response(&mut words, 0x02, 0x122);
        cdrom_ack(&mut words);

        cdrom_command(&mut words, 0x0f, &[]); // Getparam
        cdrom_expect_flags(&mut words, 0x03, 0x123);
        cdrom_expect_response(&mut words, 0x02, 0x124);
        cdrom_expect_response(&mut words, 0x80, 0x125);
        cdrom_expect_response(&mut words, 0x00, 0x126);
        cdrom_expect_response(&mut words, 0x00, 0x127);
        cdrom_expect_response(&mut words, 0x00, 0x128);
        cdrom_ack(&mut words);

        cdrom_command(&mut words, 0x02, &[0x00, 0x02, 0x00]); // Setloc LBA 0
        cdrom_expect_flags(&mut words, 0x03, 0x131);
        cdrom_expect_response(&mut words, 0x02, 0x132);
        cdrom_ack(&mut words);

        cdrom_command(&mut words, 0x06, &[]); // ReadN
        cdrom_expect_flags(&mut words, 0x03, 0x133);
        cdrom_expect_response(&mut words, 0x22, 0x134);
        cdrom_ack(&mut words);
        cdrom_expect_flags(&mut words, 0x01, 0x135);
        cdrom_expect_response(&mut words, 0x22, 0x136);

        push_li(&mut words, 11, 1 << 15);
        words.push(sw(11, 13, 0x00f0)); // DPCR: enable DMA3
        push_li(&mut words, 11, CDROM_DMA_BUFFER & 0x001f_fffc);
        words.push(sw(11, 13, 0x00b0)); // D3_MADR
        push_li(&mut words, 11, (1 << 16) | 2);
        words.push(sw(11, 13, 0x00b4)); // D3_BCR: two words
        push_li(&mut words, 11, 0x0100_0200);
        words.push(sw(11, 13, 0x00b8)); // D3_CHCR: from CD-ROM to RAM

        words.push(lw(11, 10, 0));
        words.push(nop());
        assert_reg_eq(&mut words, 11, 0x0302_0100, 0x141);
        words.push(lw(11, 10, 4));
        words.push(nop());
        assert_reg_eq(&mut words, 11, 0x0706_0504, 0x142);

        push_li(&mut words, 11, 1);
        words.push(sw(11, 9, 0));
        words
    }

    fn cdrom_command(words: &mut Vec<u32>, command: u8, params: &[u8]) {
        words.push(sb(0, 8, 0));
        for param in params {
            push_li(words, 11, *param as u32);
            words.push(sb(11, 8, 2));
        }
        push_li(words, 11, command as u32);
        words.push(sb(11, 8, 1));
    }

    fn cdrom_expect_flags(words: &mut Vec<u32>, expected: u32, failure: u32) {
        push_li(words, 11, 1);
        words.push(sb(11, 8, 0));
        words.push(lbu(11, 8, 3));
        words.push(nop());
        assert_reg_eq(words, 11, expected, failure);
    }

    fn cdrom_expect_response(words: &mut Vec<u32>, expected: u32, failure: u32) {
        words.push(lbu(11, 8, 1));
        words.push(nop());
        assert_reg_eq(words, 11, expected, failure);
    }

    fn cdrom_ack(words: &mut Vec<u32>) {
        push_li(words, 11, 1);
        words.push(sb(11, 8, 0));
        push_li(words, 11, 0x1f);
        words.push(sb(11, 8, 3));
        words.push(sb(0, 8, 0));
    }

    fn assert_reg_eq(words: &mut Vec<u32>, reg: u32, expected: u32, failure: u32) {
        push_li(words, 12, expected);
        words.push(beq(reg, 12, 3));
        words.push(nop());
        push_li(words, 12, failure);
        words.push(sw(12, 9, 0));
    }

    fn push_li(words: &mut Vec<u32>, reg: u32, value: u32) {
        if value <= 0xffff {
            words.push(ori(reg, 0, value as u16));
            return;
        }
        words.push(lui(reg, (value >> 16) as u16));
        let low = value as u16;
        if low != 0 {
            words.push(ori(reg, reg, low));
        }
    }

    fn cooked_disc(sectors: usize) -> Vec<u8> {
        let mut disc = vec![0; sectors * 2048];
        for sector in 0..sectors {
            for offset in 0..2048 {
                disc[sector * 2048 + offset] = offset as u8;
            }
        }
        disc
    }

    fn psx_exe(words: &[u32]) -> Vec<u8> {
        let mut payload = Vec::with_capacity(words.len() * 4);
        for word in words {
            payload.extend(word.to_le_bytes());
        }
        let mut exe = vec![0; 0x800 + payload.len()];
        exe[0..8].copy_from_slice(b"PS-X EXE");
        write_le_u32(&mut exe, 0x10, LOAD_ADDR);
        write_le_u32(&mut exe, 0x14, 0x8000_4000);
        write_le_u32(&mut exe, 0x18, LOAD_ADDR);
        write_le_u32(&mut exe, 0x1c, payload.len() as u32);
        write_le_u32(&mut exe, 0x30, 0x801f_0000);
        write_le_u32(&mut exe, 0x34, 0x100);
        exe[0x800..].copy_from_slice(&payload);
        exe
    }

    fn write_le_u32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn lui(rt: u32, imm: u16) -> u32 {
        (0x0f << 26) | (rt << 16) | imm as u32
    }

    fn ori(rt: u32, rs: u32, imm: u16) -> u32 {
        (0x0d << 26) | (rs << 21) | (rt << 16) | imm as u32
    }

    fn addiu(rt: u32, rs: u32, imm: i16) -> u32 {
        (0x09 << 26) | (rs << 21) | (rt << 16) | imm as u16 as u32
    }

    fn sw(rt: u32, rs: u32, imm: i16) -> u32 {
        (0x2b << 26) | (rs << 21) | (rt << 16) | imm as u16 as u32
    }

    fn lw(rt: u32, rs: u32, imm: i16) -> u32 {
        (0x23 << 26) | (rs << 21) | (rt << 16) | imm as u16 as u32
    }

    fn lbu(rt: u32, rs: u32, imm: i16) -> u32 {
        (0x24 << 26) | (rs << 21) | (rt << 16) | imm as u16 as u32
    }

    fn sb(rt: u32, rs: u32, imm: i16) -> u32 {
        (0x28 << 26) | (rs << 21) | (rt << 16) | imm as u16 as u32
    }

    fn beq(rs: u32, rt: u32, imm: i16) -> u32 {
        (0x04 << 26) | (rs << 21) | (rt << 16) | imm as u16 as u32
    }

    fn j(target: u32) -> u32 {
        (0x02 << 26) | ((target >> 2) & 0x03ff_ffff)
    }

    fn nop() -> u32 {
        0
    }

    fn break_() -> u32 {
        0x0000_000d
    }
}
