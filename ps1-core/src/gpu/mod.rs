use crate::{GPU_VRAM_HEIGHT, GPU_VRAM_PIXELS, GPU_VRAM_WIDTH};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gpu {
    vram: Vec<u16>,
    status: u32,
    read_response: u32,
    packet: Gp0Packet,
    display_x: u16,
    display_y: u16,
    display_width: usize,
    display_height: usize,
    dma_direction: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum Gp0Packet {
    Idle,
    Fill {
        color: u16,
        params: Vec<u32>,
    },
    CpuToVram {
        params: Vec<u32>,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        words_remaining: usize,
        pixel_index: usize,
    },
}

impl Gpu {
    pub fn new() -> Self {
        Self {
            vram: vec![0; GPU_VRAM_PIXELS],
            status: 0x1c00_0000,
            read_response: 0,
            packet: Gp0Packet::Idle,
            display_x: 0,
            display_y: 0,
            display_width: 320,
            display_height: 240,
            dma_direction: 0,
        }
    }

    pub fn read_gp0(&self) -> u32 {
        self.read_response
    }

    pub fn read_gp1(&self) -> u32 {
        let dma_bits = (self.dma_direction as u32) << 29;
        self.status | dma_bits | (1 << 26) | (1 << 27) | (1 << 28)
    }

    pub fn write_gp0(&mut self, word: u32) {
        let packet = std::mem::replace(&mut self.packet, Gp0Packet::Idle);
        match packet {
            Gp0Packet::Idle => self.start_gp0(word),
            Gp0Packet::Fill { color, mut params } => {
                params.push(word);
                if params.len() == 2 {
                    self.fill_rect(color, params[0], params[1]);
                } else {
                    self.packet = Gp0Packet::Fill { color, params };
                }
            }
            Gp0Packet::CpuToVram {
                mut params,
                mut x,
                mut y,
                mut width,
                mut height,
                mut words_remaining,
                mut pixel_index,
            } => {
                if params.len() < 2 {
                    params.push(word);
                    if params.len() == 2 {
                        x = (params[0] & 0x3ff) as u16;
                        y = ((params[0] >> 16) & 0x1ff) as u16;
                        width = (params[1] & 0xffff) as u16;
                        height = ((params[1] >> 16) & 0xffff) as u16;
                        let pixels = width.max(1) as usize * height.max(1) as usize;
                        words_remaining = pixels.div_ceil(2);
                    }
                    self.packet = Gp0Packet::CpuToVram {
                        params,
                        x,
                        y,
                        width,
                        height,
                        words_remaining,
                        pixel_index,
                    };
                } else {
                    self.write_vram_upload_word(x, y, width, height, pixel_index, word);
                    pixel_index += 2;
                    words_remaining = words_remaining.saturating_sub(1);
                    if words_remaining != 0 {
                        self.packet = Gp0Packet::CpuToVram {
                            params,
                            x,
                            y,
                            width,
                            height,
                            words_remaining,
                            pixel_index,
                        };
                    }
                }
            }
        }
    }

    pub fn write_gp1(&mut self, word: u32) {
        let command = (word >> 24) as u8;
        match command {
            0x00 => self.reset(),
            0x01 => self.packet = Gp0Packet::Idle,
            0x03 => {
                if (word & 1) != 0 {
                    self.status |= 1 << 23;
                } else {
                    self.status &= !(1 << 23);
                }
            }
            0x04 => self.dma_direction = (word & 0x3) as u8,
            0x05 => {
                self.display_x = (word & 0x3ff) as u16;
                self.display_y = ((word >> 10) & 0x1ff) as u16;
            }
            0x08 => self.set_display_mode(word),
            0x10 => self.read_response = self.gp1_info(word & 0x0f),
            _ => {}
        }
    }

    pub fn display_frame(&self) -> Vec<u16> {
        let mut out = vec![0; self.display_width * self.display_height];
        for y in 0..self.display_height {
            for x in 0..self.display_width {
                let sx = (self.display_x as usize + x) & (GPU_VRAM_WIDTH - 1);
                let sy = (self.display_y as usize + y) % GPU_VRAM_HEIGHT;
                out[y * self.display_width + x] = self.vram[sy * GPU_VRAM_WIDTH + sx];
            }
        }
        out
    }

    pub fn display_size(&self) -> (usize, usize) {
        (self.display_width, self.display_height)
    }

    pub fn vram(&self) -> &[u16] {
        &self.vram
    }

    fn start_gp0(&mut self, word: u32) {
        match (word >> 24) as u8 {
            0x00 => {}
            0x01 => {}
            0x02 => {
                self.packet = Gp0Packet::Fill {
                    color: rgb24_to_bgr555(word & 0x00ff_ffff),
                    params: Vec::with_capacity(2),
                };
            }
            0xa0 => {
                self.packet = Gp0Packet::CpuToVram {
                    params: Vec::with_capacity(2),
                    x: 0,
                    y: 0,
                    width: 0,
                    height: 0,
                    words_remaining: 0,
                    pixel_index: 0,
                };
            }
            _ => {}
        }
    }

    fn fill_rect(&mut self, color: u16, xy: u32, wh: u32) {
        let x = (xy & 0x3f0) as usize;
        let y = ((xy >> 16) & 0x1ff) as usize;
        let width = (((wh & 0xffff) + 0x0f) & !0x0f).max(1) as usize;
        let height = (((wh >> 16) & 0xffff).max(1)) as usize;
        for dy in 0..height {
            let py = (y + dy) % GPU_VRAM_HEIGHT;
            for dx in 0..width {
                let px = (x + dx) & (GPU_VRAM_WIDTH - 1);
                self.vram[py * GPU_VRAM_WIDTH + px] = color;
            }
        }
    }

    fn write_vram_upload_word(
        &mut self,
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        pixel_index: usize,
        word: u32,
    ) {
        let width = width.max(1) as usize;
        let height = height.max(1) as usize;
        for lane in 0..2 {
            let index = pixel_index + lane;
            if index >= width * height {
                return;
            }
            let px = (x as usize + (index % width)) & (GPU_VRAM_WIDTH - 1);
            let py = (y as usize + (index / width)) % GPU_VRAM_HEIGHT;
            let pixel = if lane == 0 {
                word as u16
            } else {
                (word >> 16) as u16
            };
            self.vram[py * GPU_VRAM_WIDTH + px] = pixel;
        }
    }

    fn set_display_mode(&mut self, word: u32) {
        let horizontal_mode = word & 0x3;
        let high_res = (word & (1 << 6)) != 0;
        self.display_width = match (high_res, horizontal_mode) {
            (true, _) => 640,
            (false, 0) => 256,
            (false, 1) => 320,
            (false, 2) => 512,
            (false, _) => 640,
        };
        self.display_height = if (word & (1 << 2)) != 0 { 480 } else { 240 };
    }

    fn gp1_info(&self, selector: u32) -> u32 {
        match selector {
            0x2 => ((self.display_y as u32) << 10) | self.display_x as u32,
            0x7 => 2,
            _ => 0,
        }
    }

    fn reset(&mut self) {
        self.vram.fill(0);
        self.status = 0x1c00_0000;
        self.read_response = 0;
        self.packet = Gp0Packet::Idle;
        self.display_x = 0;
        self.display_y = 0;
        self.display_width = 320;
        self.display_height = 240;
        self.dma_direction = 0;
    }
}

impl Default for Gpu {
    fn default() -> Self {
        Self::new()
    }
}

fn rgb24_to_bgr555(rgb: u32) -> u16 {
    let r = ((rgb >> 3) & 0x1f) as u16;
    let g = ((rgb >> 11) & 0x1f) as u16;
    let b = ((rgb >> 19) & 0x1f) as u16;
    r | (g << 5) | (b << 10)
}
