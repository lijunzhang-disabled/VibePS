use serde::{Deserialize, Serialize};

const FLAG_ERROR: u32 = 1 << 31;
const FLAG_MAC1_POS: u32 = 1 << 30;
const FLAG_MAC2_POS: u32 = 1 << 29;
const FLAG_MAC3_POS: u32 = 1 << 28;
const FLAG_MAC1_NEG: u32 = 1 << 27;
const FLAG_MAC2_NEG: u32 = 1 << 26;
const FLAG_MAC3_NEG: u32 = 1 << 25;
const FLAG_IR1_SAT: u32 = 1 << 24;
const FLAG_IR2_SAT: u32 = 1 << 23;
const FLAG_IR3_SAT: u32 = 1 << 22;
const FLAG_OTZ_SAT: u32 = 1 << 18;
const FLAG_DIV_OVERFLOW: u32 = 1 << 17;
const FLAG_MAC0_POS: u32 = 1 << 16;
const FLAG_MAC0_NEG: u32 = 1 << 15;
const FLAG_SX2_SAT: u32 = 1 << 14;
const FLAG_SY2_SAT: u32 = 1 << 13;
const FLAG_IR0_SAT: u32 = 1 << 12;
const FLAG_WRITE_MASK: u32 = 0x7fff_f000;
const FLAG_SUMMARY_MASK: u32 = 0x7f87_e000;

const FN_RTPS: u32 = 0x01;
const FN_NCLIP: u32 = 0x06;
const FN_MVMVA: u32 = 0x12;
const FN_AVSZ3: u32 = 0x2d;
const FN_AVSZ4: u32 = 0x2e;
const FN_RTPT: u32 = 0x30;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gte {
    data: [u32; 32],
    control: [u32; 32],
}

impl Gte {
    pub fn new() -> Self {
        Self {
            data: [0; 32],
            control: [0; 32],
        }
    }

    pub fn reset(&mut self) {
        self.data = [0; 32];
        self.control = [0; 32];
    }

    pub fn read_data(&self, index: usize) -> u32 {
        match index & 31 {
            1 | 3 | 5 | 8..=11 => sign_extend16(self.data[index & 31]),
            7 | 16..=19 => self.data[index & 31] & 0xffff,
            15 => self.data[14],
            28 | 29 => self.pack_orgb(),
            _ => self.data[index & 31],
        }
    }

    pub fn write_data(&mut self, index: usize, value: u32) {
        match index & 31 {
            1 | 3 | 5 | 8..=11 => self.data[index & 31] = sign_extend16(value),
            7 | 16..=19 => self.data[index & 31] = value & 0xffff,
            15 => self.push_sxy_raw(value),
            28 => self.expand_irgb(value),
            30 => {
                self.data[30] = value;
                self.data[31] = leading_sign_bits(value);
            }
            31 => {}
            _ => self.data[index & 31] = value,
        }
    }

    pub fn read_control(&self, index: usize) -> u32 {
        self.control[index & 31]
    }

    pub fn write_control(&mut self, index: usize, value: u32) {
        match index & 31 {
            4 | 12 | 20 | 26 | 27 | 29 | 30 => self.control[index & 31] = sign_extend16(value),
            31 => self.control[31] = normalize_flag(value & FLAG_WRITE_MASK),
            _ => self.control[index & 31] = value,
        }
    }

    pub fn execute_command(&mut self, opcode: u32) {
        self.control[31] = 0;
        match opcode & 0x3f {
            FN_RTPS => self.rtps(opcode),
            FN_NCLIP => self.nclip(),
            FN_MVMVA => self.mvmva(opcode),
            FN_AVSZ3 => self.avsz3(),
            FN_AVSZ4 => self.avsz4(),
            FN_RTPT => self.rtpt(opcode),
            _ => {}
        }
    }

    fn rtps(&mut self, opcode: u32) {
        let sf = shift_fraction(opcode);
        let lm = limit_mode(opcode);
        self.transform_vertex(0, sf, lm);
    }

    fn rtpt(&mut self, opcode: u32) {
        let sf = shift_fraction(opcode);
        let lm = limit_mode(opcode);
        for index in 0..3 {
            self.transform_vertex(index, sf, lm);
        }
    }

    fn transform_vertex(&mut self, index: usize, sf: bool, lm: bool) {
        let vector = self.vector(index);
        let matrix = self.matrix(0);
        let translation = self.control_vector(0);
        let mut mac = [0i64; 3];
        for row in 0..3 {
            let acc = (translation[row] << 12)
                + matrix[row][0] * vector[0]
                + matrix[row][1] * vector[1]
                + matrix[row][2] * vector[2];
            mac[row] = if sf { acc >> 12 } else { acc };
            self.set_mac(row + 1, mac[row]);
            self.limit_ir(row + 1, mac[row], lm, sf);
        }

        let depth = if sf { mac[2] } else { mac[2] >> 12 };
        let sz3 = self.limit_depth(depth);
        self.push_sz(sz3);

        let quotient = self.divide(self.h(), sz3);
        let sx = self.project_screen(self.control_as_i64(24), self.ir(1), quotient, true);
        let sy = self.project_screen(self.control_as_i64(25), self.ir(2), quotient, false);
        self.push_sxy_raw(pack_i16_pair(sx, sy));

        let mac0 = self.control_as_i64(28) + self.control_as_i64(27) * quotient as i64;
        self.set_mac0(mac0);
        self.limit_ir0(mac0 >> 12);
    }

    fn nclip(&mut self) {
        let [sx0, sy0] = self.sxy(12);
        let [sx1, sy1] = self.sxy(13);
        let [sx2, sy2] = self.sxy(14);
        let value = sx0 * (sy1 - sy2) + sx1 * (sy2 - sy0) + sx2 * (sy0 - sy1);
        self.set_mac0(value);
    }

    fn avsz3(&mut self) {
        let sum = self.data[17] as i64 + self.data[18] as i64 + self.data[19] as i64;
        self.average_z(sum, self.control_as_i64(29));
    }

    fn avsz4(&mut self) {
        let sum = self.data[16] as i64
            + self.data[17] as i64
            + self.data[18] as i64
            + self.data[19] as i64;
        self.average_z(sum, self.control_as_i64(30));
    }

    fn average_z(&mut self, sum: i64, factor: i64) {
        let mac0 = sum * factor;
        self.set_mac0(mac0);
        let otz = self.limit_unsigned16(mac0 >> 12);
        self.data[7] = otz;
    }

    fn mvmva(&mut self, opcode: u32) {
        let sf = shift_fraction(opcode);
        let lm = limit_mode(opcode);
        let mx = ((opcode >> 17) & 0x3) as usize;
        let vector_select = ((opcode >> 15) & 0x3) as usize;
        let control_select = ((opcode >> 13) & 0x3) as usize;
        let matrix = self.matrix(mx);
        let vector = self.vector(vector_select);
        let control = self.control_vector(control_select);
        for row in 0..3 {
            let acc = (control[row] << 12)
                + matrix[row][0] * vector[0]
                + matrix[row][1] * vector[1]
                + matrix[row][2] * vector[2];
            let value = if sf { acc >> 12 } else { acc };
            self.set_mac(row + 1, value);
            self.limit_ir(row + 1, value, lm, sf);
        }
    }

    fn matrix(&self, select: usize) -> [[i64; 3]; 3] {
        let base = match select {
            0 => 0,
            1 => 8,
            2 => 16,
            _ => return [[0; 3]; 3],
        };
        [
            [
                self.control_packed_low(base),
                self.control_packed_high(base),
                self.control_packed_low(base + 1),
            ],
            [
                self.control_packed_high(base + 1),
                self.control_packed_low(base + 2),
                self.control_packed_high(base + 2),
            ],
            [
                self.control_packed_low(base + 3),
                self.control_packed_high(base + 3),
                self.control_as_i64(base + 4),
            ],
        ]
    }

    fn vector(&self, select: usize) -> [i64; 3] {
        match select {
            0 => [self.vx(0), self.vy(0), self.vz(0)],
            1 => [self.vx(1), self.vy(1), self.vz(1)],
            2 => [self.vx(2), self.vy(2), self.vz(2)],
            _ => [self.ir(1), self.ir(2), self.ir(3)],
        }
    }

    fn control_vector(&self, select: usize) -> [i64; 3] {
        match select {
            0 => [
                self.control_as_i64(5),
                self.control_as_i64(6),
                self.control_as_i64(7),
            ],
            1 => [
                self.control_as_i64(13),
                self.control_as_i64(14),
                self.control_as_i64(15),
            ],
            2 => [
                self.control_as_i64(21),
                self.control_as_i64(22),
                self.control_as_i64(23),
            ],
            _ => [0, 0, 0],
        }
    }

    fn push_sxy_raw(&mut self, value: u32) {
        self.data[12] = self.data[13];
        self.data[13] = self.data[14];
        self.data[14] = value;
    }

    fn push_sz(&mut self, value: u32) {
        self.data[16] = self.data[17];
        self.data[17] = self.data[18];
        self.data[18] = self.data[19];
        self.data[19] = value & 0xffff;
    }

    fn set_mac0(&mut self, value: i64) {
        if value > i32::MAX as i64 {
            self.set_flag(FLAG_MAC0_POS);
        } else if value < i32::MIN as i64 {
            self.set_flag(FLAG_MAC0_NEG);
        }
        self.data[24] = value as i32 as u32;
    }

    fn set_mac(&mut self, index: usize, value: i64) {
        const MAX_MAC: i64 = 0x7ff_ffff_ffff;
        const MIN_MAC: i64 = -0x800_0000_0000;
        if value > MAX_MAC {
            self.set_flag([FLAG_MAC1_POS, FLAG_MAC2_POS, FLAG_MAC3_POS][index - 1]);
        } else if value < MIN_MAC {
            self.set_flag([FLAG_MAC1_NEG, FLAG_MAC2_NEG, FLAG_MAC3_NEG][index - 1]);
        }
        self.data[24 + index] = value as i32 as u32;
    }

    fn limit_ir(&mut self, index: usize, value: i64, lm: bool, sf: bool) {
        let flag_check = if sf { value } else { value >> 12 };
        let min = if lm { 0 } else { -0x8000 };
        let clamped = value.clamp(min, 0x7fff);
        if flag_check < min || flag_check > 0x7fff {
            self.set_flag([FLAG_IR1_SAT, FLAG_IR2_SAT, FLAG_IR3_SAT][index - 1]);
        }
        self.data[8 + index] = clamped as i16 as i32 as u32;
    }

    fn limit_ir0(&mut self, value: i64) {
        let clamped = value.clamp(0, 0x1000);
        if clamped != value {
            self.set_flag(FLAG_IR0_SAT);
        }
        self.data[8] = clamped as u32;
    }

    fn limit_depth(&mut self, value: i64) -> u32 {
        self.limit_unsigned16(value)
    }

    fn limit_unsigned16(&mut self, value: i64) -> u32 {
        let clamped = value.clamp(0, 0xffff);
        if clamped != value {
            self.set_flag(FLAG_OTZ_SAT);
        }
        clamped as u32
    }

    fn divide(&mut self, h: i64, sz3: u32) -> u32 {
        if sz3 == 0 || h >= (sz3 as i64 * 2) {
            self.set_flag(FLAG_DIV_OVERFLOW);
            0x1ffff
        } else {
            ((h << 16) / sz3 as i64).clamp(0, 0x1ffff) as u32
        }
    }

    fn project_screen(&mut self, offset: i64, ir: i64, quotient: u32, is_x: bool) -> i16 {
        let projected = (offset + ir * quotient as i64) >> 16;
        let clamped = projected.clamp(-0x400, 0x3ff);
        if clamped != projected {
            self.set_flag(if is_x { FLAG_SX2_SAT } else { FLAG_SY2_SAT });
        }
        clamped as i16
    }

    fn expand_irgb(&mut self, value: u32) {
        self.data[9] = (((value & 0x1f) << 7) as i16 as i32) as u32;
        self.data[10] = ((((value >> 5) & 0x1f) << 7) as i16 as i32) as u32;
        self.data[11] = ((((value >> 10) & 0x1f) << 7) as i16 as i32) as u32;
    }

    fn pack_orgb(&self) -> u32 {
        let r = ((self.ir(1) >> 7).clamp(0, 0x1f) as u32) & 0x1f;
        let g = ((self.ir(2) >> 7).clamp(0, 0x1f) as u32) & 0x1f;
        let b = ((self.ir(3) >> 7).clamp(0, 0x1f) as u32) & 0x1f;
        r | (g << 5) | (b << 10)
    }

    fn set_flag(&mut self, flag: u32) {
        self.control[31] = normalize_flag((self.control[31] | flag) & FLAG_WRITE_MASK);
    }

    fn vx(&self, index: usize) -> i64 {
        (self.data[index * 2] as u16 as i16) as i64
    }

    fn vy(&self, index: usize) -> i64 {
        ((self.data[index * 2] >> 16) as u16 as i16) as i64
    }

    fn vz(&self, index: usize) -> i64 {
        (self.data[index * 2 + 1] as u16 as i16) as i64
    }

    fn ir(&self, index: usize) -> i64 {
        self.data[8 + index] as i32 as i64
    }

    fn h(&self) -> i64 {
        self.control[26] as i32 as i64
    }

    fn sxy(&self, index: usize) -> [i64; 2] {
        [
            (self.data[index] as u16 as i16) as i64,
            ((self.data[index] >> 16) as u16 as i16) as i64,
        ]
    }

    fn control_as_i64(&self, index: usize) -> i64 {
        self.control[index] as i32 as i64
    }

    fn control_packed_low(&self, index: usize) -> i64 {
        (self.control[index] as u16 as i16) as i64
    }

    fn control_packed_high(&self, index: usize) -> i64 {
        ((self.control[index] >> 16) as u16 as i16) as i64
    }
}

impl Default for Gte {
    fn default() -> Self {
        Self::new()
    }
}

fn normalize_flag(value: u32) -> u32 {
    let mut flag = value & FLAG_WRITE_MASK;
    if (flag & FLAG_SUMMARY_MASK) != 0 {
        flag |= FLAG_ERROR;
    }
    flag
}

fn sign_extend16(value: u32) -> u32 {
    (value as u16 as i16 as i32) as u32
}

fn leading_sign_bits(value: u32) -> u32 {
    if (value & 0x8000_0000) == 0 {
        value.leading_zeros()
    } else {
        (!value).leading_zeros()
    }
}

fn shift_fraction(opcode: u32) -> bool {
    ((opcode >> 19) & 1) != 0
}

fn limit_mode(opcode: u32) -> bool {
    ((opcode >> 10) & 1) != 0
}

fn pack_i16_pair(x: i16, y: i16) -> u32 {
    (x as u16 as u32) | ((y as u16 as u32) << 16)
}

#[cfg(test)]
mod tests {
    use super::{Gte, FLAG_ERROR};

    const FN_RTPS: u32 = 0x01;
    const FN_NCLIP: u32 = 0x06;
    const FN_MVMVA: u32 = 0x12;
    const FN_AVSZ3: u32 = 0x2d;
    const FN_AVSZ4: u32 = 0x2e;
    const FN_RTPT: u32 = 0x30;

    #[test]
    fn register_io_applies_gte_special_cases() {
        let mut gte = Gte::new();

        gte.write_data(8, 0x0000_ffff);
        gte.write_data(1, 0x0000_ff00);
        gte.write_data(7, 0xffff_ffff);
        gte.write_data(16, 0xdead_beef);
        gte.write_data(15, 0x0001_0002);
        gte.write_data(15, 0x0003_0004);
        gte.write_data(15, 0x0005_0006);

        assert_eq!(gte.read_data(8), 0xffff_ffff);
        assert_eq!(gte.read_data(1), 0xffff_ff00);
        assert_eq!(gte.read_data(7), 0x0000_ffff);
        assert_eq!(gte.read_data(16), 0x0000_beef);
        assert_eq!(gte.read_data(12), 0x0001_0002);
        assert_eq!(gte.read_data(13), 0x0003_0004);
        assert_eq!(gte.read_data(14), 0x0005_0006);
        assert_eq!(gte.read_data(15), 0x0005_0006);
    }

    #[test]
    fn irgb_orgb_lzcr_and_flag_registers_match_hardware_shape() {
        let mut gte = Gte::new();

        gte.write_data(28, 0x7fff);
        assert_eq!(gte.read_data(9), 0x0000_0f80);
        assert_eq!(gte.read_data(10), 0x0000_0f80);
        assert_eq!(gte.read_data(11), 0x0000_0f80);
        assert_eq!(gte.read_data(29), 0x7fff);

        gte.write_data(9, 0xffff_8000);
        gte.write_data(10, 0x0000_2000);
        gte.write_data(11, 0x0000_0380);
        assert_eq!(gte.read_data(29), 0x1f << 5 | 0x07 << 10);

        gte.write_data(30, 0);
        assert_eq!(gte.read_data(31), 32);
        gte.write_data(30, 0xfffe_0000);
        assert_eq!(gte.read_data(31), 15);

        gte.write_control(31, 1 << 12);
        assert_eq!(gte.read_control(31), 1 << 12);
        gte.write_control(31, 1 << 13);
        assert_eq!(gte.read_control(31), FLAG_ERROR | (1 << 13));
        gte.write_control(31, 1 << 22);
        assert_eq!(gte.read_control(31), 1 << 22);
        gte.write_control(31, 1 << 23);
        assert_eq!(gte.read_control(31), FLAG_ERROR | (1 << 23));
    }

    #[test]
    fn control_single_halfword_registers_sign_extend() {
        let mut gte = Gte::new();
        for index in [4, 12, 20, 26, 27, 29, 30] {
            gte.write_control(index, 0x8000);
            assert_eq!(gte.read_control(index), 0xffff_8000);
        }
    }

    #[test]
    fn nclip_computes_screen_space_area_and_mac0_overflow_flags() {
        let mut gte = Gte::new();

        gte.write_data(12, 0x0000_0000);
        gte.write_data(13, 0x0000_0064);
        gte.write_data(14, 0x0064_0000);
        gte.execute_command(gte_op(0, 0, 0, 0, 0, FN_NCLIP));
        assert_eq!(gte.read_data(24), 10_000);
        assert_eq!(gte.read_control(31), 0);

        gte.write_data(12, (0x7fff << 16) | 0x7fff);
        gte.write_data(13, (0x8000 << 16) | 0x8000);
        gte.write_data(14, (0x7fff << 16) | 0x8000);
        gte.execute_command(gte_op(0, 0, 0, 0, 0, FN_NCLIP));
        assert_eq!(gte.read_data(24), 131_071);
        assert_ne!(gte.read_control(31) & (1 << 15), 0);
    }

    #[test]
    fn avsz3_and_avsz4_average_depth_and_saturate_otz() {
        let mut gte = Gte::new();
        gte.write_data(17, 100);
        gte.write_data(18, 200);
        gte.write_data(19, 300);
        gte.write_control(29, 0x0555);

        gte.execute_command(gte_op(1, 0, 0, 0, 0, FN_AVSZ3));

        assert_eq!(gte.read_data(24), 819_000);
        assert_eq!(gte.read_data(7), 199);

        gte.write_data(16, 100);
        gte.write_data(17, 200);
        gte.write_data(18, 300);
        gte.write_data(19, 400);
        gte.write_control(30, 0x0400);

        gte.execute_command(gte_op(1, 0, 0, 0, 0, FN_AVSZ4));

        assert_eq!(gte.read_data(24), 1_024_000);
        assert_eq!(gte.read_data(7), 250);

        gte.write_data(17, 0xffff);
        gte.write_data(18, 0xffff);
        gte.write_data(19, 0xffff);
        gte.write_control(29, 0x1000);

        gte.execute_command(gte_op(1, 0, 0, 0, 0, FN_AVSZ3));

        assert_eq!(gte.read_data(7), 0xffff);
        assert_ne!(gte.read_control(31) & (1 << 18), 0);
    }

    #[test]
    fn mvmva_handles_basic_matrix_vector_and_limit_modes() {
        let mut gte = Gte::new();
        set_z_rotation_90(&mut gte);
        set_translation(&mut gte, 10, 20, 30);
        gte.write_data(0, (200 << 16) | 100);
        gte.write_data(1, 300);

        gte.execute_command(gte_op(1, 0, 0, 0, 0, FN_MVMVA));

        assert_eq!(gte.read_data(25), (-190i32) as u32);
        assert_eq!(gte.read_data(26), 120);
        assert_eq!(gte.read_data(27), 330);

        set_identity_rotation(&mut gte);
        set_translation(&mut gte, -500, -600, -700);
        gte.write_data(0, (100 << 16) | 100);
        gte.write_data(1, 100);

        gte.execute_command(gte_op(1, 0, 0, 0, 1, FN_MVMVA));

        assert_eq!(gte.read_data(25), (-400i32) as u32);
        assert_eq!(gte.read_data(9), 0);

        set_identity_rotation(&mut gte);
        set_translation(&mut gte, 0, 0, 0);
        gte.write_data(0, (10 << 16) | 10);
        gte.write_data(1, 10);

        gte.execute_command(gte_op(0, 0, 0, 3, 0, FN_MVMVA));

        assert_eq!(gte.read_data(25), 40_960);
        assert_eq!(gte.read_data(26), 40_960);
        assert_eq!(gte.read_data(27), 40_960);
    }

    #[test]
    fn mvmva_supports_light_color_matrix_with_background() {
        let mut gte = Gte::new();
        gte.write_control(16, 0x0000_1000);
        gte.write_control(17, 0x0000_0000);
        gte.write_control(18, 0x0000_1000);
        gte.write_control(19, 0x0000_0000);
        gte.write_control(20, 0x1000);
        gte.write_control(13, 100);
        gte.write_control(14, 200);
        gte.write_control(15, 300);
        gte.write_data(9, 0x1000);
        gte.write_data(10, 0x1000);
        gte.write_data(11, 0x1000);

        gte.execute_command(gte_op(1, 2, 3, 1, 0, FN_MVMVA));

        assert_eq!(gte.read_data(25), 4196);
        assert_eq!(gte.read_data(26), 4296);
        assert_eq!(gte.read_data(27), 4396);
    }

    #[test]
    fn rtps_projects_single_vertex_and_sets_depth_cue() {
        let mut gte = Gte::new();
        set_identity_rotation(&mut gte);
        set_translation(&mut gte, 0, 0, 0);
        set_screen(&mut gte, 160 << 16, 120 << 16, 200);
        gte.write_data(0, (50 << 16) | 100);
        gte.write_data(1, 500);

        gte.execute_command(gte_op(1, 0, 0, 0, 0, FN_RTPS));

        assert_eq!(gte.read_data(19), 500);
        assert_eq!(gte.read_data(14), (139 << 16) | 199);
        assert_eq!(gte.read_data(25), 100);
        assert_eq!(gte.read_data(26), 50);
        assert_eq!(gte.read_data(27), 500);

        set_screen(&mut gte, 0, 0, 200);
        gte.write_control(27, 0xffff_f880);
        gte.write_control(28, 0x0100_0000);
        gte.write_data(0, 0);
        gte.write_data(1, 1000);

        gte.execute_command(gte_op(1, 0, 0, 0, 0, FN_RTPS));

        assert_eq!(gte.read_data(24), (-8_388_224i32) as u32);
        assert_eq!(gte.read_data(8), 0);
    }

    #[test]
    fn rtps_saturates_screen_and_reports_division_overflow() {
        let mut gte = Gte::new();
        set_identity_rotation(&mut gte);
        set_translation(&mut gte, 0, 0, 0);
        set_screen(&mut gte, 0, 0, 200);
        gte.write_data(0, 0x0000_7fff);
        gte.write_data(1, 100);

        gte.execute_command(gte_op(1, 0, 0, 0, 0, FN_RTPS));

        assert_eq!(gte.read_data(14) as u16, 0x03ff);
        assert_ne!(gte.read_control(31) & (1 << 17), 0);
        assert_ne!(gte.read_control(31) & (1 << 14), 0);
    }

    #[test]
    fn rtpt_projects_three_vertices_and_pushes_sz_fifo() {
        let mut gte = Gte::new();
        set_identity_rotation(&mut gte);
        set_translation(&mut gte, 0, 0, 0);
        set_screen(&mut gte, 160 << 16, 120 << 16, 200);
        gte.write_data(0, 0);
        gte.write_data(1, 100);
        gte.write_data(2, 0);
        gte.write_data(3, 200);
        gte.write_data(4, 0);
        gte.write_data(5, 300);

        gte.execute_command(gte_op(1, 0, 0, 0, 0, FN_RTPT));

        assert_eq!(gte.read_data(17), 100);
        assert_eq!(gte.read_data(18), 200);
        assert_eq!(gte.read_data(19), 300);
        assert_eq!(gte.read_data(12), (120 << 16) | 160);
        assert_eq!(gte.read_data(13), (120 << 16) | 160);
        assert_eq!(gte.read_data(14), (120 << 16) | 160);
    }

    fn set_identity_rotation(gte: &mut Gte) {
        gte.write_control(0, 0x0000_1000);
        gte.write_control(1, 0x0000_0000);
        gte.write_control(2, 0x0000_1000);
        gte.write_control(3, 0x0000_0000);
        gte.write_control(4, 0x1000);
    }

    fn set_z_rotation_90(gte: &mut Gte) {
        gte.write_control(0, 0xf000_0000);
        gte.write_control(1, 0x1000_0000);
        gte.write_control(2, 0);
        gte.write_control(3, 0);
        gte.write_control(4, 0x1000);
    }

    fn set_translation(gte: &mut Gte, x: i32, y: i32, z: i32) {
        gte.write_control(5, x as u32);
        gte.write_control(6, y as u32);
        gte.write_control(7, z as u32);
    }

    fn set_screen(gte: &mut Gte, ofx: i32, ofy: i32, h: u16) {
        gte.write_control(24, ofx as u32);
        gte.write_control(25, ofy as u32);
        gte.write_control(26, h as u32);
        gte.write_control(27, 0);
        gte.write_control(28, 0);
    }

    fn gte_op(sf: u32, mx: u32, v: u32, cv: u32, lm: u32, function: u32) -> u32 {
        (sf << 19) | (mx << 17) | (v << 15) | (cv << 13) | (lm << 10) | function
    }
}
