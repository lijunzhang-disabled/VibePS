use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

const INT_DATA_READY: u8 = 0x01;
const INT_COMPLETE: u8 = 0x02;
const INT_ACK: u8 = 0x03;
const INT_ERROR: u8 = 0x05;
const DRIVE_STATUS_ERROR: u8 = 0x01;
const DRIVE_STATUS_MOTOR_ON: u8 = 0x02;
const DRIVE_STATUS_READ: u8 = 0x20;
const SECTOR_PREGAP: i32 = 150;
const COOKED_SECTOR_SIZE: usize = 2048;
const RAW_SECTOR_SIZE: usize = 2352;
const RAW_MODE2_PAYLOAD_OFFSET: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CdromSectorSize {
    Cooked2048,
    Raw2352,
}

impl CdromSectorSize {
    pub fn bytes(self) -> usize {
        match self {
            Self::Cooked2048 => COOKED_SECTOR_SIZE,
            Self::Raw2352 => RAW_SECTOR_SIZE,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CdromDiscError {
    Empty,
    MisalignedLength,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdromDisc {
    bytes: Vec<u8>,
    sector_size: CdromSectorSize,
}

impl CdromDisc {
    pub fn new(bytes: Vec<u8>, sector_size: CdromSectorSize) -> Result<Self, CdromDiscError> {
        let sector_bytes = sector_size.bytes();
        if bytes.is_empty() {
            return Err(CdromDiscError::Empty);
        }
        if bytes.len() % sector_bytes != 0 {
            return Err(CdromDiscError::MisalignedLength);
        }
        Ok(Self { bytes, sector_size })
    }

    pub fn sector_count(&self) -> usize {
        self.bytes.len() / self.sector_size.bytes()
    }

    fn sector_payload(&self, lba: u32) -> Option<&[u8]> {
        let sector_start = lba as usize * self.sector_size.bytes();
        match self.sector_size {
            CdromSectorSize::Cooked2048 => self.bytes.get(sector_start..sector_start + 2048),
            CdromSectorSize::Raw2352 => {
                let payload_start = sector_start + RAW_MODE2_PAYLOAD_OFFSET;
                self.bytes.get(payload_start..payload_start + 2048)
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cdrom {
    index: u8,
    mode: u8,
    interrupt_enable: u8,
    interrupt_flags: u8,
    params: VecDeque<u8>,
    responses: VecDeque<u8>,
    data: VecDeque<u8>,
    queued_interrupts: VecDeque<QueuedInterrupt>,
    disc: Option<CdromDisc>,
    loc_lba: u32,
    read_active: bool,
    last_sector_lba: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct QueuedInterrupt {
    code: u8,
    response: Vec<u8>,
}

impl Cdrom {
    pub fn new() -> Self {
        Self {
            index: 0,
            mode: 0,
            interrupt_enable: 0,
            interrupt_flags: 0,
            params: VecDeque::new(),
            responses: VecDeque::new(),
            data: VecDeque::new(),
            queued_interrupts: VecDeque::new(),
            disc: None,
            loc_lba: 0,
            read_active: false,
            last_sector_lba: None,
        }
    }

    pub fn load_disc_image(
        &mut self,
        bytes: Vec<u8>,
        sector_size: CdromSectorSize,
    ) -> Result<(), CdromDiscError> {
        self.disc = Some(CdromDisc::new(bytes, sector_size)?);
        self.loc_lba = 0;
        self.read_active = false;
        self.last_sector_lba = None;
        self.data.clear();
        Ok(())
    }

    pub fn eject_disc(&mut self) {
        self.disc = None;
        self.read_active = false;
        self.data.clear();
        self.last_sector_lba = None;
    }

    pub fn disc_sector_count(&self) -> Option<usize> {
        self.disc.as_ref().map(CdromDisc::sector_count)
    }

    pub fn read8(&mut self, offset: u32) -> u8 {
        match offset & 0x3 {
            0 => self.status_register(),
            1 => self.responses.pop_front().unwrap_or(0),
            2 => self.pop_data_byte(),
            3 => match self.index & 0x3 {
                0 | 2 => self.interrupt_enable,
                _ => self.interrupt_flags,
            },
            _ => 0,
        }
    }

    pub fn write8(&mut self, offset: u32, value: u8) {
        match offset & 0x3 {
            0 => self.index = value & 0x3,
            1 => {
                if self.index == 0 {
                    self.command(value);
                }
            }
            2 => match self.index & 0x3 {
                0 => self.params.push_back(value),
                1 => self.interrupt_enable = value & 0x1f,
                _ => {}
            },
            3 => match self.index & 0x3 {
                1 | 3 => self.ack_interrupt(value & 0x1f),
                _ => {}
            },
            _ => {}
        }
    }

    pub fn dma_read32(&mut self) -> u32 {
        let b0 = self.pop_data_byte() as u32;
        let b1 = self.pop_data_byte() as u32;
        let b2 = self.pop_data_byte() as u32;
        let b3 = self.pop_data_byte() as u32;
        b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
    }

    pub fn interrupt_pending(&self) -> bool {
        (self.interrupt_flags & self.interrupt_enable & 0x1f) != 0
    }

    fn status_register(&self) -> u8 {
        let mut status = self.index & 0x3;
        if self.params.is_empty() {
            status |= 1 << 3;
        }
        status |= 1 << 4;
        if !self.responses.is_empty() {
            status |= 1 << 5;
        }
        if !self.data.is_empty() {
            status |= 1 << 6;
        }
        status
    }

    fn drive_status(&self) -> u8 {
        let mut status = DRIVE_STATUS_MOTOR_ON;
        if self.read_active {
            status |= DRIVE_STATUS_READ;
        }
        status
    }

    fn command(&mut self, command: u8) {
        let params: Vec<u8> = self.params.drain(..).collect();
        match command {
            0x01 => self.queue_interrupt(INT_ACK, &[self.drive_status()]), // Nop
            0x02 => self.setloc(&params),
            0x06 => self.start_read(), // ReadN
            0x09 => self.pause(),
            0x0a => self.init(),
            0x0e => self.setmode(&params),
            0x0f => self.getparam(),
            0x10 => self.getloc_l(),
            0x19 => self.queue_interrupt(INT_ACK, &[self.drive_status(), 0, 0, 0, 0]), // Test
            0x1b => self.start_read(),                                                 // ReadS
            _ => self.error_response(),
        }
    }

    fn setloc(&mut self, params: &[u8]) {
        if params.len() < 3 {
            self.error_response();
            return;
        }
        let minute = bcd_to_u8(params[0]) as i32;
        let second = bcd_to_u8(params[1]) as i32;
        let frame = bcd_to_u8(params[2]) as i32;
        let absolute = ((minute * 60) + second) * 75 + frame;
        self.loc_lba = absolute.saturating_sub(SECTOR_PREGAP) as u32;
        self.queue_interrupt(INT_ACK, &[self.drive_status()]);
    }

    fn start_read(&mut self) {
        if self.disc.is_none() {
            self.error_response();
            return;
        }
        self.read_active = true;
        self.queue_interrupt(INT_ACK, &[self.drive_status()]);
        self.queue_current_sector();
    }

    fn pause(&mut self) {
        self.read_active = false;
        self.queue_interrupt(INT_ACK, &[self.drive_status()]);
        self.queue_interrupt(INT_COMPLETE, &[self.drive_status()]);
    }

    fn init(&mut self) {
        self.mode = 0;
        self.read_active = false;
        self.data.clear();
        self.queue_interrupt(INT_ACK, &[self.drive_status()]);
    }

    fn setmode(&mut self, params: &[u8]) {
        if params.is_empty() {
            self.error_response();
            return;
        }
        self.mode = params[0];
        self.queue_interrupt(INT_ACK, &[self.drive_status()]);
    }

    fn getparam(&mut self) {
        self.queue_interrupt(INT_ACK, &[self.drive_status(), self.mode, 0, 0, 0]);
    }

    fn getloc_l(&mut self) {
        let lba = self.last_sector_lba.unwrap_or(self.loc_lba);
        let [minute, second, frame] = lba_to_bcd_msf(lba);
        self.queue_interrupt(INT_ACK, &[minute, second, frame, 0x02, 0, 0, 0x20, 0]);
    }

    fn error_response(&mut self) {
        self.read_active = false;
        self.queue_interrupt(INT_ERROR, &[self.drive_status() | DRIVE_STATUS_ERROR, 0x80]);
    }

    fn queue_current_sector(&mut self) {
        let Some(disc) = self.disc.as_ref() else {
            self.error_response();
            return;
        };
        let lba = self.loc_lba;
        let Some(payload) = disc.sector_payload(lba) else {
            self.error_response();
            return;
        };
        self.data.clear();
        self.data.extend(payload.iter().copied());
        self.last_sector_lba = Some(lba);
        self.loc_lba = self.loc_lba.saturating_add(1);
        self.queue_interrupt(INT_DATA_READY, &[self.drive_status()]);
    }

    fn queue_next_sector_if_ready(&mut self) {
        if self.read_active
            && self.data.is_empty()
            && (self.interrupt_flags & 0x7) == 0
            && self.queued_interrupts.is_empty()
        {
            self.queue_current_sector();
        }
    }

    fn queue_interrupt(&mut self, code: u8, response: &[u8]) {
        let queued = QueuedInterrupt {
            code,
            response: response.to_vec(),
        };
        if (self.interrupt_flags & 0x7) == 0 {
            self.activate_interrupt(queued);
        } else {
            self.queued_interrupts.push_back(queued);
        }
    }

    fn activate_interrupt(&mut self, interrupt: QueuedInterrupt) {
        self.responses.clear();
        self.responses.extend(interrupt.response);
        self.interrupt_flags = (self.interrupt_flags & !0x7) | (interrupt.code & 0x7);
    }

    fn ack_interrupt(&mut self, mask: u8) {
        self.interrupt_flags &= !mask;
        if (self.interrupt_flags & 0x7) == 0 {
            if let Some(interrupt) = self.queued_interrupts.pop_front() {
                self.activate_interrupt(interrupt);
            } else {
                self.queue_next_sector_if_ready();
            }
        }
    }

    fn pop_data_byte(&mut self) -> u8 {
        let value = self.data.pop_front().unwrap_or(0);
        self.queue_next_sector_if_ready();
        value
    }
}

impl Default for Cdrom {
    fn default() -> Self {
        Self::new()
    }
}

fn bcd_to_u8(value: u8) -> u8 {
    ((value >> 4) & 0x0f) * 10 + (value & 0x0f)
}

fn u8_to_bcd(value: u8) -> u8 {
    ((value / 10) << 4) | (value % 10)
}

fn lba_to_bcd_msf(lba: u32) -> [u8; 3] {
    let absolute = lba + SECTOR_PREGAP as u32;
    let minute = (absolute / (60 * 75)) as u8;
    let second = ((absolute / 75) % 60) as u8;
    let frame = (absolute % 75) as u8;
    [u8_to_bcd(minute), u8_to_bcd(second), u8_to_bcd(frame)]
}

#[cfg(test)]
mod tests {
    use super::{Cdrom, CdromDiscError, CdromSectorSize};

    #[test]
    fn setmode_and_getparam_round_trip_mode_byte() {
        let mut cdrom = Cdrom::new();
        command_with_params(&mut cdrom, 0x0e, &[0x80]);

        assert_eq!(interrupt_flags(&mut cdrom) & 0x7, 0x3);
        assert_eq!(cdrom.read8(1), 0x02);
        ack(&mut cdrom);

        command_with_params(&mut cdrom, 0x0f, &[]);

        assert_eq!(cdrom.read8(1), 0x02);
        assert_eq!(cdrom.read8(1), 0x80);
        assert_eq!(cdrom.read8(1), 0);
        assert_eq!(cdrom.read8(1), 0);
        assert_eq!(cdrom.read8(1), 0);
    }

    #[test]
    fn setloc_and_readn_queue_cooked_sector_data_after_ack() {
        let mut cdrom = Cdrom::new();
        cdrom
            .load_disc_image(cooked_disc(3), CdromSectorSize::Cooked2048)
            .unwrap();
        command_with_params(&mut cdrom, 0x02, &[0x00, 0x02, 0x01]);
        ack(&mut cdrom);

        command_with_params(&mut cdrom, 0x06, &[]);

        assert_eq!(interrupt_flags(&mut cdrom) & 0x7, 0x3);
        assert_eq!(cdrom.read8(1), 0x22);
        ack(&mut cdrom);
        assert_eq!(interrupt_flags(&mut cdrom) & 0x7, 0x1);
        assert_eq!(cdrom.read8(1), 0x22);
        assert_eq!(cdrom.read8(2), 1);
        assert_eq!(cdrom.read8(2), 2);
        assert_eq!(cdrom.read8(2), 3);
        assert_eq!(cdrom.read8(2), 4);
    }

    #[test]
    fn readn_without_disc_reports_error_interrupt() {
        let mut cdrom = Cdrom::new();

        command_with_params(&mut cdrom, 0x06, &[]);

        assert_eq!(interrupt_flags(&mut cdrom) & 0x7, 0x5);
        assert_eq!(cdrom.read8(1), 0x03);
        assert_eq!(cdrom.read8(1), 0x80);
    }

    #[test]
    fn raw_disc_reads_mode2_payload_area() {
        let mut raw = vec![0xee; 2352];
        raw[24] = 0x11;
        raw[25] = 0x22;
        let mut cdrom = Cdrom::new();
        cdrom
            .load_disc_image(raw, CdromSectorSize::Raw2352)
            .unwrap();

        command_with_params(&mut cdrom, 0x06, &[]);
        ack(&mut cdrom);

        assert_eq!(cdrom.read8(2), 0x11);
        assert_eq!(cdrom.read8(2), 0x22);
    }

    #[test]
    fn rejects_misaligned_disc_images() {
        let mut cdrom = Cdrom::new();

        let err = cdrom
            .load_disc_image(vec![0; 2047], CdromSectorSize::Cooked2048)
            .unwrap_err();

        assert_eq!(err, CdromDiscError::MisalignedLength);
    }

    fn command_with_params(cdrom: &mut Cdrom, command: u8, params: &[u8]) {
        cdrom.write8(0, 0);
        for param in params {
            cdrom.write8(2, *param);
        }
        cdrom.write8(1, command);
    }

    fn interrupt_flags(cdrom: &mut Cdrom) -> u8 {
        cdrom.write8(0, 1);
        cdrom.read8(3)
    }

    fn ack(cdrom: &mut Cdrom) {
        cdrom.write8(0, 1);
        cdrom.write8(3, 0x1f);
        cdrom.write8(0, 0);
    }

    fn cooked_disc(sectors: usize) -> Vec<u8> {
        let mut disc = vec![0; sectors * 2048];
        for sector in 0..sectors {
            for offset in 0..2048 {
                disc[sector * 2048 + offset] = (sector as u8).wrapping_add(offset as u8);
            }
        }
        disc
    }
}
