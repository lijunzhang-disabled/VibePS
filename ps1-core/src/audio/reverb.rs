use crate::SPU_RAM_SIZE;
use serde::{Deserialize, Serialize};

const RAM_MASK: u32 = SPU_RAM_SIZE as u32 - 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reverb {
    base_address: u32,
    current_address: u32,
    second_half_tick: bool,
    last_output: [i16; 2],
}

impl Reverb {
    pub fn new() -> Self {
        Self {
            base_address: 0,
            current_address: 0,
            second_half_tick: false,
            last_output: [0; 2],
        }
    }

    pub fn set_base(&mut self, value: u16) {
        self.base_address = ((value as u32) << 3) & RAM_MASK;
        self.current_address = self.base_address;
    }

    pub fn process(
        &mut self,
        ram: &mut [u8],
        registers: &[u16],
        input: [i32; 2],
        writes_enabled: bool,
        irq_address: u32,
    ) -> ([i16; 2], bool) {
        self.second_half_tick = !self.second_half_tick;
        if !self.second_half_tick {
            return (self.last_output, false);
        }

        let mut irq = false;
        let lin = multiply(input[0], signed(registers[30]));
        let rin = multiply(input[1], signed(registers[31]));
        let viir = signed(registers[2]);
        let vwall = signed(registers[7]);

        let mlsame = self.address(registers[10]);
        let mrsame = self.address(registers[11]);
        let mldiff = self.address(registers[18]);
        let mrdiff = self.address(registers[19]);

        let lsame_old = read_sample(ram, self.displaced(mlsame, -2), irq_address, &mut irq);
        let rsame_old = read_sample(ram, self.displaced(mrsame, -2), irq_address, &mut irq);
        let ldiff_old = read_sample(ram, self.displaced(mldiff, -2), irq_address, &mut irq);
        let rdiff_old = read_sample(ram, self.displaced(mrdiff, -2), irq_address, &mut irq);

        let lsame = multiply(
            lin + multiply(
                read_sample(ram, self.address(registers[16]), irq_address, &mut irq),
                vwall,
            ) - lsame_old,
            viir,
        ) + lsame_old;
        let rsame = multiply(
            rin + multiply(
                read_sample(ram, self.address(registers[17]), irq_address, &mut irq),
                vwall,
            ) - rsame_old,
            viir,
        ) + rsame_old;
        let ldiff = multiply(
            lin + multiply(
                read_sample(ram, self.address(registers[25]), irq_address, &mut irq),
                vwall,
            ) - ldiff_old,
            viir,
        ) + ldiff_old;
        let rdiff = multiply(
            rin + multiply(
                read_sample(ram, self.address(registers[24]), irq_address, &mut irq),
                vwall,
            ) - rdiff_old,
            viir,
        ) + rdiff_old;

        if writes_enabled {
            write_sample(ram, mlsame, lsame, irq_address, &mut irq);
            write_sample(ram, mrsame, rsame, irq_address, &mut irq);
            write_sample(ram, mldiff, ldiff, irq_address, &mut irq);
            write_sample(ram, mrdiff, rdiff, irq_address, &mut irq);
        }

        let mut left = self.comb(ram, registers, [12, 14, 20, 22], irq_address, &mut irq);
        let mut right = self.comb(ram, registers, [13, 15, 21, 23], irq_address, &mut irq);
        left = self.all_pass(
            ram,
            left,
            registers[26],
            registers[0],
            signed(registers[8]),
            writes_enabled,
            irq_address,
            &mut irq,
        );
        right = self.all_pass(
            ram,
            right,
            registers[27],
            registers[0],
            signed(registers[8]),
            writes_enabled,
            irq_address,
            &mut irq,
        );
        left = self.all_pass(
            ram,
            left,
            registers[28],
            registers[1],
            signed(registers[9]),
            writes_enabled,
            irq_address,
            &mut irq,
        );
        right = self.all_pass(
            ram,
            right,
            registers[29],
            registers[1],
            signed(registers[9]),
            writes_enabled,
            irq_address,
            &mut irq,
        );

        self.last_output = [clamp_i16(left), clamp_i16(right)];
        self.current_address = self.displaced(self.current_address, 2);
        (self.last_output, irq)
    }

    fn comb(
        &self,
        ram: &[u8],
        registers: &[u16],
        addresses: [usize; 4],
        irq_address: u32,
        irq: &mut bool,
    ) -> i32 {
        let volumes = [3usize, 4, 5, 6];
        addresses
            .into_iter()
            .zip(volumes)
            .map(|(address, volume)| {
                multiply(
                    read_sample(ram, self.address(registers[address]), irq_address, irq),
                    signed(registers[volume]),
                )
            })
            .sum()
    }

    #[allow(clippy::too_many_arguments)]
    fn all_pass(
        &self,
        ram: &mut [u8],
        input: i32,
        address_register: u16,
        displacement_register: u16,
        volume: i32,
        writes_enabled: bool,
        irq_address: u32,
        irq: &mut bool,
    ) -> i32 {
        let address = self.address(address_register);
        let displaced = self.displaced(address, -((displacement_register as i32) << 3));
        let delayed = read_sample(ram, displaced, irq_address, irq);
        let value = input - multiply(delayed, volume);
        if writes_enabled {
            write_sample(ram, address, value, irq_address, irq);
        }
        multiply(value, volume) + delayed
    }

    fn address(&self, register: u16) -> u32 {
        self.displaced(self.current_address, (register as i32) << 3)
    }

    fn displaced(&self, address: u32, displacement: i32) -> u32 {
        let base = self.base_address;
        let size = SPU_RAM_SIZE as u32 - base;
        if size == 0 {
            return RAM_MASK & !1;
        }
        let relative = address.wrapping_sub(base) % size;
        let displaced = (relative as i64 + displacement as i64).rem_euclid(size as i64) as u32;
        (base + displaced) & RAM_MASK & !1
    }
}

impl Default for Reverb {
    fn default() -> Self {
        Self::new()
    }
}

fn signed(value: u16) -> i32 {
    value as i16 as i32
}

fn multiply(value: i32, volume: i32) -> i32 {
    ((value as i64 * volume as i64) >> 15).clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

fn clamp_i16(value: i32) -> i16 {
    value.clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

fn read_sample(ram: &[u8], address: u32, irq_address: u32, irq: &mut bool) -> i32 {
    *irq |= (address & !7) == (irq_address & !7);
    let index = address as usize & (SPU_RAM_SIZE - 1);
    i16::from_le_bytes([ram[index], ram[(index + 1) & (SPU_RAM_SIZE - 1)]]) as i32
}

fn write_sample(ram: &mut [u8], address: u32, value: i32, irq_address: u32, irq: &mut bool) {
    *irq |= (address & !7) == (irq_address & !7);
    let index = address as usize & (SPU_RAM_SIZE - 1);
    let bytes = clamp_i16(value).to_le_bytes();
    ram[index] = bytes[0];
    ram[(index + 1) & (SPU_RAM_SIZE - 1)] = bytes[1];
}

#[cfg(test)]
mod tests {
    use super::Reverb;
    use crate::SPU_RAM_SIZE;

    #[test]
    fn disabled_reverb_keeps_ram_unchanged() {
        let mut reverb = Reverb::new();
        reverb.set_base(0x1000);
        let mut ram = vec![0; SPU_RAM_SIZE];
        let mut registers = [0u16; 32];
        registers[2] = 0x7fff;
        registers[30] = 0x7fff;
        registers[31] = 0x7fff;

        reverb.process(&mut ram, &registers, [10_000, -10_000], false, 0);

        assert!(ram.iter().all(|byte| *byte == 0));
    }
}
