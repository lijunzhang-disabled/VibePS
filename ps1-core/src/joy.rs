use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

pub const MEMORY_CARD_SIZE: usize = 128 * 1024;
const MEMORY_CARD_SECTOR_SIZE: usize = 128;
const MEMORY_CARD_SECTORS: usize = MEMORY_CARD_SIZE / MEMORY_CARD_SECTOR_SIZE;

const CTRL_TX_ENABLE: u16 = 1 << 0;
const CTRL_DTR: u16 = 1 << 1;
const CTRL_FORCE_RX: u16 = 1 << 2;
const CTRL_ACK: u16 = 1 << 4;
const CTRL_RESET: u16 = 1 << 6;
const CTRL_TX_IRQ_ENABLE: u16 = 1 << 10;
const CTRL_RX_IRQ_ENABLE: u16 = 1 << 11;
const CTRL_DSR_IRQ_ENABLE: u16 = 1 << 12;
const CTRL_PORT_SELECT: u16 = 1 << 13;
const CTRL_WRITE_MASK: u16 = 0x3faf;

const STAT_TX_READY: u32 = 1 << 0;
const STAT_RX_READY: u32 = 1 << 1;
const STAT_TX_IDLE: u32 = 1 << 2;
const STAT_DSR: u32 = 1 << 7;
const STAT_IRQ: u32 = 1 << 9;

const ACK_DELAY_CYCLES: u32 = 400;
const ACK_PULSE_CYCLES: u32 = 100;
const MEMORY_CARD_READ_DELAY_CYCLES: u32 = 31_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryCardImageError {
    InvalidSize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControllerType {
    Digital,
    DualShock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Button {
    Select = 0,
    L3 = 1,
    R3 = 2,
    Start = 3,
    Up = 4,
    Right = 5,
    Down = 6,
    Left = 7,
    L2 = 8,
    R2 = 9,
    L1 = 10,
    R1 = 11,
    Triangle = 12,
    Circle = 13,
    Cross = 14,
    Square = 15,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Controller {
    kind: ControllerType,
    pressed: u16,
    analog_mode: bool,
    axes: [u8; 4],
    command: u8,
    step: usize,
}

impl Controller {
    pub fn digital() -> Self {
        Self {
            kind: ControllerType::Digital,
            pressed: 0,
            analog_mode: false,
            axes: [0x80; 4],
            command: 0,
            step: 0,
        }
    }

    pub fn dualshock() -> Self {
        Self {
            kind: ControllerType::DualShock,
            analog_mode: true,
            ..Self::digital()
        }
    }

    pub fn kind(&self) -> ControllerType {
        self.kind
    }

    pub fn set_button(&mut self, button: Button, pressed: bool) {
        let mask = 1u16 << button as u8;
        if pressed {
            self.pressed |= mask;
        } else {
            self.pressed &= !mask;
        }
    }

    pub fn set_pressed_bits(&mut self, pressed: u16) {
        self.pressed = pressed;
    }

    pub fn pressed_bits(&self) -> u16 {
        self.pressed
    }

    pub fn set_analog_mode(&mut self, enabled: bool) {
        self.analog_mode = self.kind == ControllerType::DualShock && enabled;
    }

    pub fn analog_mode(&self) -> bool {
        self.analog_mode
    }

    pub fn set_analog_axes(&mut self, right_x: u8, right_y: u8, left_x: u8, left_y: u8) {
        self.axes = [right_x, right_y, left_x, left_y];
    }

    fn reset_transaction(&mut self) {
        self.command = 0;
        self.step = 0;
    }

    fn exchange(&mut self, value: u8) -> TransferResponse {
        if self.command == 0 {
            self.command = value;
            self.step = 0;
            if value != 0x42 {
                return TransferResponse::last(0xff);
            }
            return TransferResponse::more(self.id());
        }

        if self.command != 0x42 {
            return TransferResponse::last(0xff);
        }

        let switches = !self.pressed;
        let response = match self.step {
            0 => TransferResponse::more(0x5a),
            1 => TransferResponse::more(switches as u8),
            2 if self.analog_mode => TransferResponse::more((switches >> 8) as u8),
            2 => TransferResponse::last((switches >> 8) as u8),
            3 => TransferResponse::more(self.axes[0]),
            4 => TransferResponse::more(self.axes[1]),
            5 => TransferResponse::more(self.axes[2]),
            6 => TransferResponse::last(self.axes[3]),
            _ => TransferResponse::last(0xff),
        };
        self.step += 1;
        response
    }

    fn id(&self) -> u8 {
        if self.kind == ControllerType::DualShock && self.analog_mode {
            0x73
        } else {
            0x41
        }
    }
}

impl Default for Controller {
    fn default() -> Self {
        Self::digital()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryCard {
    data: Vec<u8>,
    flag: u8,
    dirty: bool,
    command: u8,
    step: usize,
    previous: u8,
    address: u16,
    write_buffer: Vec<u8>,
    write_result: u8,
}

impl MemoryCard {
    pub fn formatted() -> Self {
        let mut data = vec![0xff; MEMORY_CARD_SIZE];
        format_memory_card(&mut data);
        Self::from_valid_data(data)
    }

    pub fn from_bytes(data: Vec<u8>) -> Result<Self, MemoryCardImageError> {
        if data.len() != MEMORY_CARD_SIZE {
            return Err(MemoryCardImageError::InvalidSize);
        }
        Ok(Self::from_valid_data(data))
    }

    fn from_valid_data(data: Vec<u8>) -> Self {
        Self {
            data,
            flag: 0x08,
            dirty: false,
            command: 0,
            step: 0,
            previous: 0,
            address: 0,
            write_buffer: Vec::with_capacity(MEMORY_CARD_SECTOR_SIZE),
            write_result: 0x47,
        }
    }

    pub fn image(&self) -> &[u8] {
        &self.data
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    pub fn read_sector(&self, sector: usize) -> Option<&[u8]> {
        let start = sector.checked_mul(MEMORY_CARD_SECTOR_SIZE)?;
        self.data.get(start..start + MEMORY_CARD_SECTOR_SIZE)
    }

    fn reset_transaction(&mut self) {
        self.command = 0;
        self.step = 0;
        self.previous = 0;
        self.address = 0;
        self.write_buffer.clear();
        self.write_result = 0x47;
    }

    fn exchange(&mut self, value: u8) -> TransferResponse {
        if self.command == 0 {
            self.command = value;
            self.step = 0;
            self.previous = value;
            if !matches!(value, 0x52 | 0x53 | 0x57) {
                return TransferResponse::last(self.flag);
            }
            return TransferResponse::more(self.flag);
        }

        let response = match self.command {
            0x52 => self.exchange_read(value),
            0x53 => self.exchange_id(),
            0x57 => self.exchange_write(value),
            _ => TransferResponse::last(0xff),
        };
        self.previous = value;
        self.step += 1;
        response
    }

    fn exchange_read(&mut self, value: u8) -> TransferResponse {
        match self.step {
            0 => TransferResponse::more(0x5a),
            1 => TransferResponse::more(0x5d),
            2 => {
                self.address = (value as u16) << 8;
                TransferResponse::more(0x00)
            }
            3 => {
                self.address |= value as u16;
                TransferResponse::more(self.previous)
            }
            4 => TransferResponse::more_with_delay(0x5c, MEMORY_CARD_READ_DELAY_CYCLES),
            5 => TransferResponse::more(0x5d),
            6 => TransferResponse::more(if self.valid_address() {
                (self.address >> 8) as u8
            } else {
                0xff
            }),
            7 if !self.valid_address() => TransferResponse::last(0xff),
            7 => TransferResponse::more(self.address as u8),
            8..=135 => {
                let offset = self.address as usize * MEMORY_CARD_SECTOR_SIZE + self.step - 8;
                TransferResponse::more(self.data[offset])
            }
            136 => TransferResponse::more(self.read_checksum()),
            137 => TransferResponse::last(0x47),
            _ => TransferResponse::last(0xff),
        }
    }

    fn exchange_write(&mut self, value: u8) -> TransferResponse {
        match self.step {
            0 => TransferResponse::more(0x5a),
            1 => TransferResponse::more(0x5d),
            2 => {
                self.address = (value as u16) << 8;
                TransferResponse::more(0x00)
            }
            3 => {
                self.address |= value as u16;
                TransferResponse::more(self.previous)
            }
            4..=131 => {
                self.write_buffer.push(value);
                TransferResponse::more(self.previous)
            }
            132 => {
                self.finish_write(value);
                TransferResponse::more(self.previous)
            }
            133 => TransferResponse::more(0x5c),
            134 => TransferResponse::more(0x5d),
            135 => TransferResponse::last(self.write_result),
            _ => TransferResponse::last(0xff),
        }
    }

    fn exchange_id(&mut self) -> TransferResponse {
        const RESPONSE: [u8; 8] = [0x5a, 0x5d, 0x5c, 0x5d, 0x04, 0x00, 0x00, 0x80];
        RESPONSE
            .get(self.step)
            .copied()
            .map(|value| {
                if self.step + 1 == RESPONSE.len() {
                    TransferResponse::last(value)
                } else {
                    TransferResponse::more(value)
                }
            })
            .unwrap_or_else(|| TransferResponse::last(0xff))
    }

    fn valid_address(&self) -> bool {
        (self.address as usize) < MEMORY_CARD_SECTORS
    }

    fn read_checksum(&self) -> u8 {
        let mut checksum = (self.address >> 8) as u8 ^ self.address as u8;
        if let Some(sector) = self.read_sector(self.address as usize) {
            for byte in sector {
                checksum ^= *byte;
            }
        }
        checksum
    }

    fn finish_write(&mut self, checksum: u8) {
        if !self.valid_address() {
            self.write_result = 0xff;
            return;
        }

        let expected = self.write_buffer.iter().fold(
            (self.address >> 8) as u8 ^ self.address as u8,
            |checksum, byte| checksum ^ byte,
        );
        if self.write_buffer.len() != MEMORY_CARD_SECTOR_SIZE || checksum != expected {
            self.write_result = 0x4e;
            return;
        }

        let start = self.address as usize * MEMORY_CARD_SECTOR_SIZE;
        self.data[start..start + MEMORY_CARD_SECTOR_SIZE].copy_from_slice(&self.write_buffer);
        self.flag &= !0x08;
        self.dirty = true;
        self.write_result = 0x47;
    }
}

impl Default for MemoryCard {
    fn default() -> Self {
        Self::formatted()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum SelectedDevice {
    None,
    Controller,
    MemoryCard,
}

#[derive(Debug, Clone, Copy)]
struct TransferResponse {
    value: u8,
    acknowledge: bool,
    extra_ack_delay: u32,
}

impl TransferResponse {
    fn more(value: u8) -> Self {
        Self {
            value,
            acknowledge: true,
            extra_ack_delay: 0,
        }
    }

    fn more_with_delay(value: u8, extra_ack_delay: u32) -> Self {
        Self {
            value,
            acknowledge: true,
            extra_ack_delay,
        }
    }

    fn last(value: u8) -> Self {
        Self {
            value,
            acknowledge: false,
            extra_ack_delay: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoySerial {
    mode: u16,
    control: u16,
    baud: u16,
    rx_fifo: VecDeque<u8>,
    last_rx: u8,
    pending_tx: Option<u8>,
    transfer_cycles: u32,
    ack_delay_cycles: u32,
    ack_pulse_cycles: u32,
    irq: bool,
    selected: SelectedDevice,
    controllers: [Option<Controller>; 2],
    memory_cards: [Option<MemoryCard>; 2],
}

impl JoySerial {
    pub fn new() -> Self {
        Self {
            mode: 0,
            control: 0,
            baud: 0,
            rx_fifo: VecDeque::with_capacity(8),
            last_rx: 0,
            pending_tx: None,
            transfer_cycles: 0,
            ack_delay_cycles: 0,
            ack_pulse_cycles: 0,
            irq: false,
            selected: SelectedDevice::None,
            controllers: [Some(Controller::digital()), None],
            memory_cards: [None, None],
        }
    }

    pub fn reset(&mut self) {
        self.mode = 0;
        self.control = 0;
        self.baud = 0;
        self.rx_fifo.clear();
        self.last_rx = 0;
        self.pending_tx = None;
        self.transfer_cycles = 0;
        self.ack_delay_cycles = 0;
        self.ack_pulse_cycles = 0;
        self.irq = false;
        self.reset_transaction();
    }

    pub fn status(&self) -> u32 {
        let mut status = 0;
        if self.pending_tx.is_none() {
            status |= STAT_TX_READY | STAT_TX_IDLE;
        }
        if !self.rx_fifo.is_empty() {
            status |= STAT_RX_READY;
        }
        if self.ack_pulse_cycles != 0 {
            status |= STAT_DSR;
        }
        if self.irq {
            status |= STAT_IRQ;
        }
        status
    }

    pub fn mode(&self) -> u16 {
        self.mode
    }

    pub fn control(&self) -> u16 {
        self.control
    }

    pub fn baud(&self) -> u16 {
        self.baud
    }

    pub fn write_mode(&mut self, value: u16) {
        self.mode = value & 0x013f;
    }

    pub fn write_control(&mut self, value: u16) {
        if (value & CTRL_RESET) != 0 {
            self.reset();
            return;
        }

        let old_control = self.control;
        self.control = value & CTRL_WRITE_MASK & !(CTRL_ACK | CTRL_RESET);
        if (value & CTRL_ACK) != 0
            && !((self.control & CTRL_DSR_IRQ_ENABLE) != 0 && self.ack_pulse_cycles != 0)
        {
            self.irq = false;
        }
        let old_port = (old_control & CTRL_PORT_SELECT) != 0;
        let new_port = (self.control & CTRL_PORT_SELECT) != 0;
        if (old_control & CTRL_DTR) != 0 && ((self.control & CTRL_DTR) == 0 || old_port != new_port)
        {
            self.reset_transaction();
        }
    }

    pub fn write_baud(&mut self, value: u16) {
        self.baud = value;
    }

    pub fn write_data8(&mut self, value: u8) {
        self.pending_tx = Some(value);
        self.transfer_cycles = self.byte_cycles();
    }

    pub fn read_data8(&mut self) -> u8 {
        if let Some(value) = self.rx_fifo.pop_front() {
            self.last_rx = value;
        }
        self.last_rx
    }

    pub fn read_data16(&mut self) -> u16 {
        let lo = self.rx_fifo.front().copied().unwrap_or(self.last_rx);
        let hi = self.rx_fifo.get(1).copied().unwrap_or(lo);
        self.read_data8();
        lo as u16 | ((hi as u16) << 8)
    }

    pub fn read_data32(&mut self) -> u32 {
        let mut result = 0;
        for index in 0..4 {
            let value = self.rx_fifo.pop_front().unwrap_or(self.last_rx);
            self.last_rx = value;
            result |= (value as u32) << (index * 8);
        }
        result
    }

    pub fn tick(&mut self, cycles: u32) -> bool {
        let mut irq_edge = false;

        if self.ack_pulse_cycles != 0 {
            self.ack_pulse_cycles = self.ack_pulse_cycles.saturating_sub(cycles);
        }

        if self.ack_delay_cycles != 0 {
            let previous = self.ack_delay_cycles;
            self.ack_delay_cycles = self.ack_delay_cycles.saturating_sub(cycles);
            if previous != 0 && self.ack_delay_cycles == 0 {
                self.ack_pulse_cycles = ACK_PULSE_CYCLES;
                if (self.control & CTRL_DSR_IRQ_ENABLE) != 0 {
                    irq_edge |= self.raise_irq();
                }
            }
        }

        if self.pending_tx.is_some() && (self.control & CTRL_TX_ENABLE) != 0 {
            let previous = self.transfer_cycles;
            self.transfer_cycles = self.transfer_cycles.saturating_sub(cycles);
            if previous != 0 && self.transfer_cycles == 0 {
                irq_edge |= self.finish_transfer();
            }
        }

        irq_edge
    }

    pub fn set_controller(&mut self, port: usize, controller: Option<Controller>) {
        if let Some(slot) = self.controllers.get_mut(port) {
            *slot = controller;
            self.reset_transaction();
        }
    }

    pub fn controller(&self, port: usize) -> Option<&Controller> {
        self.controllers.get(port).and_then(Option::as_ref)
    }

    pub fn controller_mut(&mut self, port: usize) -> Option<&mut Controller> {
        self.controllers.get_mut(port).and_then(Option::as_mut)
    }

    pub fn insert_memory_card(&mut self, port: usize, card: MemoryCard) {
        if let Some(slot) = self.memory_cards.get_mut(port) {
            *slot = Some(card);
            self.reset_transaction();
        }
    }

    pub fn remove_memory_card(&mut self, port: usize) -> Option<MemoryCard> {
        let card = self.memory_cards.get_mut(port)?.take();
        self.reset_transaction();
        card
    }

    pub fn memory_card(&self, port: usize) -> Option<&MemoryCard> {
        self.memory_cards.get(port).and_then(Option::as_ref)
    }

    pub fn memory_card_mut(&mut self, port: usize) -> Option<&mut MemoryCard> {
        self.memory_cards.get_mut(port).and_then(Option::as_mut)
    }

    fn byte_cycles(&self) -> u32 {
        let factor = match self.mode & 3 {
            2 => 16,
            3 => 64,
            _ => 1,
        };
        let period = ((self.baud as u32 * factor) & !1).max(1);
        period.saturating_mul(8)
    }

    fn finish_transfer(&mut self) -> bool {
        let value = self.pending_tx.take().unwrap_or(0);
        let receive_enabled = (self.control & (CTRL_DTR | CTRL_FORCE_RX)) != 0;
        let response = if (self.control & CTRL_DTR) != 0 {
            self.exchange(value)
        } else {
            TransferResponse::last(0xff)
        };

        if receive_enabled {
            if self.rx_fifo.len() == 8 {
                self.rx_fifo.pop_back();
            }
            self.rx_fifo.push_back(response.value);
        }
        if (self.control & CTRL_FORCE_RX) != 0 {
            self.control &= !CTRL_FORCE_RX;
        }
        if response.acknowledge {
            self.ack_delay_cycles = ACK_DELAY_CYCLES + response.extra_ack_delay;
        }

        let rx_threshold = 1usize << ((self.control >> 8) & 3);
        let interrupt = ((self.control & CTRL_TX_IRQ_ENABLE) != 0)
            || ((self.control & CTRL_RX_IRQ_ENABLE) != 0 && self.rx_fifo.len() >= rx_threshold);
        interrupt && self.raise_irq()
    }

    fn exchange(&mut self, value: u8) -> TransferResponse {
        let port = usize::from((self.control & CTRL_PORT_SELECT) != 0);
        match self.selected {
            SelectedDevice::None => match value {
                0x01 if self.controllers[port].is_some() => {
                    if let Some(controller) = self.controllers[port].as_mut() {
                        controller.reset_transaction();
                    }
                    self.selected = SelectedDevice::Controller;
                    TransferResponse::more(0xff)
                }
                0x81 if self.memory_cards[port].is_some() => {
                    if let Some(card) = self.memory_cards[port].as_mut() {
                        card.reset_transaction();
                    }
                    self.selected = SelectedDevice::MemoryCard;
                    TransferResponse::more(0xff)
                }
                _ => TransferResponse::last(0xff),
            },
            SelectedDevice::Controller => self.controllers[port]
                .as_mut()
                .map(|controller| controller.exchange(value))
                .unwrap_or_else(|| TransferResponse::last(0xff)),
            SelectedDevice::MemoryCard => self.memory_cards[port]
                .as_mut()
                .map(|card| card.exchange(value))
                .unwrap_or_else(|| TransferResponse::last(0xff)),
        }
    }

    fn raise_irq(&mut self) -> bool {
        let edge = !self.irq;
        self.irq = true;
        edge
    }

    fn reset_transaction(&mut self) {
        self.selected = SelectedDevice::None;
        for controller in self.controllers.iter_mut().flatten() {
            controller.reset_transaction();
        }
        for card in self.memory_cards.iter_mut().flatten() {
            card.reset_transaction();
        }
    }
}

impl Default for JoySerial {
    fn default() -> Self {
        Self::new()
    }
}

fn format_memory_card(data: &mut [u8]) {
    data.fill(0xff);

    let header = &mut data[..MEMORY_CARD_SECTOR_SIZE];
    header.fill(0);
    header[0] = b'M';
    header[1] = b'C';
    header[127] = frame_checksum(header);

    for frame in 1..=15 {
        let start = frame * MEMORY_CARD_SECTOR_SIZE;
        let sector = &mut data[start..start + MEMORY_CARD_SECTOR_SIZE];
        sector.fill(0);
        sector[0] = 0xa0;
        sector[8] = 0xff;
        sector[9] = 0xff;
        sector[127] = frame_checksum(sector);
    }

    for frame in 16..=35 {
        let start = frame * MEMORY_CARD_SECTOR_SIZE;
        let sector = &mut data[start..start + MEMORY_CARD_SECTOR_SIZE];
        sector.fill(0);
        sector[..4].fill(0xff);
        sector[127] = frame_checksum(sector);
    }

    let write_test = 63 * MEMORY_CARD_SECTOR_SIZE;
    let (before, after) = data.split_at_mut(write_test);
    after[..MEMORY_CARD_SECTOR_SIZE].copy_from_slice(&before[..MEMORY_CARD_SECTOR_SIZE]);
}

fn frame_checksum(frame: &[u8]) -> u8 {
    frame[..127]
        .iter()
        .copied()
        .fold(0, |checksum, byte| checksum ^ byte)
}

#[cfg(test)]
mod tests {
    use super::{
        Button, Controller, JoySerial, MemoryCard, ACK_DELAY_CYCLES, CTRL_DSR_IRQ_ENABLE, CTRL_DTR,
        CTRL_TX_ENABLE, MEMORY_CARD_SECTOR_SIZE, STAT_IRQ,
    };

    fn begin(joy: &mut JoySerial) {
        joy.write_baud(1);
        joy.write_control(CTRL_TX_ENABLE | CTRL_DTR);
    }

    fn transfer(joy: &mut JoySerial, value: u8) -> u8 {
        joy.write_data8(value);
        joy.tick(8);
        joy.read_data8()
    }

    #[test]
    fn digital_controller_poll_returns_active_low_buttons() {
        let mut joy = JoySerial::new();
        joy.controller_mut(0)
            .unwrap()
            .set_button(Button::Cross, true);
        begin(&mut joy);

        let replies = [0x01, 0x42, 0, 0, 0].map(|value| transfer(&mut joy, value));

        assert_eq!(replies, [0xff, 0x41, 0x5a, 0xff, 0xbf]);
    }

    #[test]
    fn dualshock_poll_returns_four_analog_axes() {
        let mut joy = JoySerial::new();
        let mut controller = Controller::dualshock();
        controller.set_analog_axes(1, 2, 3, 4);
        joy.set_controller(0, Some(controller));
        begin(&mut joy);

        let replies = [0x01, 0x42, 0, 0, 0, 0, 0, 0, 0].map(|value| transfer(&mut joy, value));

        assert_eq!(replies, [0xff, 0x73, 0x5a, 0xff, 0xff, 1, 2, 3, 4]);
    }

    #[test]
    fn port_select_routes_transfer_to_second_controller() {
        let mut joy = JoySerial::new();
        joy.set_controller(0, None);
        joy.set_controller(1, Some(Controller::digital()));
        joy.write_baud(1);
        joy.write_control(CTRL_TX_ENABLE | CTRL_DTR | super::CTRL_PORT_SELECT);

        let replies = [0x01, 0x42, 0, 0, 0].map(|value| transfer(&mut joy, value));

        assert_eq!(replies, [0xff, 0x41, 0x5a, 0xff, 0xff]);
    }

    #[test]
    fn controller_ack_irq_is_delayed_and_acknowledgeable() {
        let mut joy = JoySerial::new();
        joy.write_baud(1);
        joy.write_control(CTRL_TX_ENABLE | CTRL_DTR | CTRL_DSR_IRQ_ENABLE);
        joy.write_data8(0x01);

        assert!(!joy.tick(8));
        assert_eq!(joy.status() & STAT_IRQ, 0);
        assert!(!joy.tick(ACK_DELAY_CYCLES - 1));
        assert!(joy.tick(1));
        assert_ne!(joy.status() & STAT_IRQ, 0);
        assert_ne!(joy.status() & (1 << 7), 0);

        joy.write_control(CTRL_TX_ENABLE | CTRL_DTR | CTRL_DSR_IRQ_ENABLE | (1 << 4));
        assert_ne!(joy.status() & STAT_IRQ, 0);
        joy.tick(super::ACK_PULSE_CYCLES);
        joy.write_control(CTRL_TX_ENABLE | CTRL_DTR | CTRL_DSR_IRQ_ENABLE | (1 << 4));
        assert_eq!(joy.status() & STAT_IRQ, 0);
    }

    #[test]
    fn formatted_card_has_valid_header_and_directory_checksums() {
        let card = MemoryCard::formatted();
        assert_eq!(&card.image()[..2], b"MC");
        for frame in 0..16 {
            let start = frame * MEMORY_CARD_SECTOR_SIZE;
            let sector = &card.image()[start..start + MEMORY_CARD_SECTOR_SIZE];
            assert_eq!(super::frame_checksum(sector), sector[127]);
        }
    }

    #[test]
    fn memory_card_write_then_read_round_trips_sector() {
        let mut joy = JoySerial::new();
        joy.insert_memory_card(0, MemoryCard::formatted());
        begin(&mut joy);
        let data: Vec<u8> = (0..128).map(|value| value as u8).collect();
        let address = 7u16;
        let checksum = data
            .iter()
            .fold((address >> 8) as u8 ^ address as u8, |checksum, byte| {
                checksum ^ byte
            });

        let mut request = vec![0x81, 0x57, 0, 0, (address >> 8) as u8, address as u8];
        request.extend_from_slice(&data);
        request.extend_from_slice(&[checksum, 0, 0, 0]);
        let replies: Vec<u8> = request
            .into_iter()
            .map(|value| transfer(&mut joy, value))
            .collect();
        assert_eq!(&replies[replies.len() - 3..], &[0x5c, 0x5d, 0x47]);
        assert!(joy.memory_card(0).unwrap().is_dirty());

        joy.write_control(CTRL_TX_ENABLE);
        joy.write_control(CTRL_TX_ENABLE | CTRL_DTR);
        let mut read = vec![0x81, 0x52, 0, 0, 0, address as u8, 0, 0, 0, 0];
        read.extend(std::iter::repeat_n(0, 130));
        let replies: Vec<u8> = read
            .into_iter()
            .map(|value| transfer(&mut joy, value))
            .collect();

        assert_eq!(&replies[10..138], data.as_slice());
        assert_eq!(replies[138], checksum);
        assert_eq!(replies[139], 0x47);
    }

    #[test]
    fn memory_card_rejects_bad_write_checksum() {
        let mut joy = JoySerial::new();
        joy.insert_memory_card(0, MemoryCard::formatted());
        let before = joy.memory_card(0).unwrap().read_sector(2).unwrap().to_vec();
        begin(&mut joy);

        let mut request = vec![0x81, 0x57, 0, 0, 0, 2];
        request.extend(std::iter::repeat_n(0x55, 128));
        request.extend_from_slice(&[0xff, 0, 0, 0]);
        let replies: Vec<u8> = request
            .into_iter()
            .map(|value| transfer(&mut joy, value))
            .collect();

        assert_eq!(replies.last(), Some(&0x4e));
        assert_eq!(joy.memory_card(0).unwrap().read_sector(2).unwrap(), before);
        assert!(!joy.memory_card(0).unwrap().is_dirty());
    }
}
