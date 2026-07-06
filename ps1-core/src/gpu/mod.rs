use crate::{GPU_VRAM_HEIGHT, GPU_VRAM_PIXELS, GPU_VRAM_WIDTH};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

const GPUSTAT_READY_CMD: u32 = 1 << 26;
const GPUSTAT_READY_READ: u32 = 1 << 27;
const GPUSTAT_READY_DMA: u32 = 1 << 28;
const GPUSTAT_DISPLAY_DISABLED: u32 = 1 << 23;
const GPUSTAT_IRQ: u32 = 1 << 24;
const GPUSTAT_MASK_SET: u32 = 1 << 11;
const GPUSTAT_MASK_CHECK: u32 = 1 << 12;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gpu {
    vram: Vec<u16>,
    status: u32,
    read_response: u32,
    packet: Gp0Packet,
    vram_read: VecDeque<u32>,
    display_x: u16,
    display_y: u16,
    display_h_start: u16,
    display_h_end: u16,
    display_v_start: u16,
    display_v_end: u16,
    display_width: usize,
    display_height: usize,
    display_mode: u32,
    dma_direction: u8,
    draw_mode: u16,
    texture_window: u32,
    draw_area_x1: u16,
    draw_area_y1: u16,
    draw_area_x2: u16,
    draw_area_y2: u16,
    draw_offset_x: i16,
    draw_offset_y: i16,
    mask_set: bool,
    mask_check: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum Gp0Packet {
    Idle,
    Collect {
        words: Vec<u32>,
        expected_words: usize,
    },
    CpuToVram {
        x: u16,
        y: u16,
        width: u16,
        height: u16,
        words_remaining: usize,
        pixel_index: usize,
    },
    Polyline {
        command: u32,
        words: Vec<u32>,
        gouraud: bool,
    },
}

#[derive(Debug, Clone, Copy)]
struct Vertex {
    x: i32,
    y: i32,
    u: i32,
    v: i32,
    color: u32,
}

impl Gpu {
    pub fn new() -> Self {
        let mut gpu = Self {
            vram: vec![0; GPU_VRAM_PIXELS],
            status: 0,
            read_response: 0,
            packet: Gp0Packet::Idle,
            vram_read: VecDeque::new(),
            display_x: 0,
            display_y: 0,
            display_h_start: 0x200,
            display_h_end: 0x200 + 320 * 8,
            display_v_start: 0x10,
            display_v_end: 0x10 + 240,
            display_width: 320,
            display_height: 240,
            display_mode: 0,
            dma_direction: 0,
            draw_mode: 0,
            texture_window: 0,
            draw_area_x1: 0,
            draw_area_y1: 0,
            draw_area_x2: (GPU_VRAM_WIDTH - 1) as u16,
            draw_area_y2: (GPU_VRAM_HEIGHT - 1) as u16,
            draw_offset_x: 0,
            draw_offset_y: 0,
            mask_set: false,
            mask_check: false,
        };
        gpu.reset();
        gpu
    }

    pub fn read_gp0(&mut self) -> u32 {
        if let Some(value) = self.vram_read.pop_front() {
            self.read_response = value;
        }
        self.read_response
    }

    pub fn read_gp1(&self) -> u32 {
        let mut status = self.status | GPUSTAT_READY_CMD | GPUSTAT_READY_DMA;
        if !self.vram_read.is_empty() {
            status |= GPUSTAT_READY_READ;
        }
        if self.dma_request(status) {
            status |= 1 << 25;
        }
        status | ((self.dma_direction as u32) << 29)
    }

    pub fn write_gp0(&mut self, word: u32) {
        let packet = std::mem::replace(&mut self.packet, Gp0Packet::Idle);
        match packet {
            Gp0Packet::Idle => self.start_gp0(word),
            Gp0Packet::Collect {
                mut words,
                expected_words,
            } => {
                words.push(word);
                if words.len() == expected_words {
                    self.execute_packet(&words);
                } else {
                    self.packet = Gp0Packet::Collect {
                        words,
                        expected_words,
                    };
                }
            }
            Gp0Packet::CpuToVram {
                x,
                y,
                width,
                height,
                mut words_remaining,
                mut pixel_index,
            } => {
                self.write_vram_upload_word(x, y, width, height, pixel_index, word);
                pixel_index += 2;
                words_remaining = words_remaining.saturating_sub(1);
                if words_remaining != 0 {
                    self.packet = Gp0Packet::CpuToVram {
                        x,
                        y,
                        width,
                        height,
                        words_remaining,
                        pixel_index,
                    };
                }
            }
            Gp0Packet::Polyline {
                command,
                mut words,
                gouraud,
            } => {
                if (word & 0xf000_f000) == 0x5000_5000 {
                    self.render_polyline(command, &words, gouraud);
                } else {
                    words.push(word);
                    self.packet = Gp0Packet::Polyline {
                        command,
                        words,
                        gouraud,
                    };
                }
            }
        }
    }

    pub fn write_gp1(&mut self, word: u32) {
        let command = (word >> 24) as u8;
        match command {
            0x00 => self.reset(),
            0x01 => self.packet = Gp0Packet::Idle,
            0x02 => self.status &= !GPUSTAT_IRQ,
            0x03 => {
                if (word & 1) != 0 {
                    self.status |= GPUSTAT_DISPLAY_DISABLED;
                } else {
                    self.status &= !GPUSTAT_DISPLAY_DISABLED;
                }
            }
            0x04 => self.dma_direction = (word & 0x3) as u8,
            0x05 => {
                self.display_x = (word & 0x3ff) as u16;
                self.display_y = ((word >> 10) & 0x1ff) as u16;
            }
            0x06 => {
                self.display_h_start = (word & 0x0fff) as u16;
                self.display_h_end = ((word >> 12) & 0x0fff) as u16;
                self.update_display_size();
            }
            0x07 => {
                self.display_v_start = (word & 0x03ff) as u16;
                self.display_v_end = ((word >> 10) & 0x03ff) as u16;
                self.update_display_size();
            }
            0x08 => {
                self.display_mode = word & 0xff;
                self.sync_display_mode_status();
                self.update_display_size();
            }
            0x09 => {}
            0x10..=0x1f => self.read_response = self.gp1_info(word & 0x00ff_ffff),
            _ => {}
        }
    }

    pub fn display_frame(&self) -> Vec<u16> {
        let mut out = vec![0; self.display_width * self.display_height];
        if (self.status & GPUSTAT_DISPLAY_DISABLED) != 0 {
            return out;
        }

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
        let command = (word >> 24) as u8;
        match command {
            0x00 | 0x01 | 0x04..=0x1e | 0xe0 | 0xe7..=0xef => {}
            0x02 => self.collect(word, 3),
            0x03 => self.collect(word, 1),
            0x1f => self.status |= GPUSTAT_IRQ,
            0xe1 => self.set_draw_mode(word),
            0xe2 => self.texture_window = word & 0x000f_ffff,
            0xe3 => {
                self.draw_area_x1 = (word & 0x03ff) as u16;
                self.draw_area_y1 = ((word >> 10) & 0x01ff) as u16;
            }
            0xe4 => {
                self.draw_area_x2 = (word & 0x03ff) as u16;
                self.draw_area_y2 = ((word >> 10) & 0x01ff) as u16;
            }
            0xe5 => {
                self.draw_offset_x = sign_extend(word & 0x07ff, 11) as i16;
                self.draw_offset_y = sign_extend((word >> 11) & 0x07ff, 11) as i16;
            }
            0xe6 => {
                self.mask_set = (word & 1) != 0;
                self.mask_check = (word & 2) != 0;
                self.sync_mask_status();
            }
            _ => match command >> 5 {
                0b001 => self.collect(word, polygon_word_count(command)),
                0b010 => self.start_line(word),
                0b011 => self.collect(word, rectangle_word_count(command)),
                0b100 => self.collect(word, 4),
                0b101 => self.collect(word, 3),
                0b110 => self.collect(word, 3),
                _ => {}
            },
        }
    }

    fn collect(&mut self, word: u32, expected_words: usize) {
        if expected_words <= 1 {
            self.execute_packet(&[word]);
            return;
        }
        self.packet = Gp0Packet::Collect {
            words: vec![word],
            expected_words,
        };
    }

    fn start_line(&mut self, word: u32) {
        let command = (word >> 24) as u8;
        let gouraud = (command & 0x10) != 0;
        if (command & 0x08) != 0 {
            self.packet = Gp0Packet::Polyline {
                command: word,
                words: Vec::new(),
                gouraud,
            };
        } else {
            let words = if gouraud { 4 } else { 3 };
            self.collect(word, words);
        }
    }

    fn execute_packet(&mut self, words: &[u32]) {
        let command = (words[0] >> 24) as u8;
        match command {
            0x02 => self.fill_rect(words[0], words[1], words[2]),
            _ => match command >> 5 {
                0b001 => self.render_polygon(words),
                0b010 => self.render_line_packet(words),
                0b011 => self.render_rectangle(words),
                0b100 => self.copy_vram_to_vram(words[1], words[2], words[3]),
                0b101 => self.begin_cpu_to_vram(words[1], words[2]),
                0b110 => self.prepare_vram_read(words[1], words[2]),
                _ => {}
            },
        }
    }

    fn begin_cpu_to_vram(&mut self, xy: u32, wh: u32) {
        let x = (xy & 0x03ff) as u16;
        let y = ((xy >> 16) & 0x01ff) as u16;
        let (width, height) = copy_size(wh);
        let pixels = width as usize * height as usize;
        self.packet = Gp0Packet::CpuToVram {
            x,
            y,
            width,
            height,
            words_remaining: pixels.div_ceil(2),
            pixel_index: 0,
        };
    }

    fn fill_rect(&mut self, command: u32, xy: u32, wh: u32) {
        let color = rgb24_to_bgr555(command & 0x00ff_ffff);
        let x = (xy & 0x03f0) as usize;
        let y = ((xy >> 16) & 0x01ff) as usize;
        let width = (((wh & 0x03ff) + 0x0f) & !0x0f) as usize;
        let height = ((wh >> 16) & 0x01ff) as usize;
        if width == 0 || height == 0 {
            return;
        }

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
        let width = width as usize;
        let height = height as usize;
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
            self.write_transfer_pixel(px, py, pixel);
        }
    }

    fn prepare_vram_read(&mut self, xy: u32, wh: u32) {
        let x = (xy & 0x03ff) as usize;
        let y = ((xy >> 16) & 0x01ff) as usize;
        let (width, height) = copy_size(wh);
        self.vram_read.clear();

        let pixels = width as usize * height as usize;
        let mut pixel_index = 0usize;
        while pixel_index < pixels {
            let lo = self.read_vram_transfer_pixel(x, y, width as usize, pixel_index) as u32;
            let hi = if pixel_index + 1 < pixels {
                self.read_vram_transfer_pixel(x, y, width as usize, pixel_index + 1) as u32
            } else {
                0
            };
            self.vram_read.push_back(lo | (hi << 16));
            pixel_index += 2;
        }
    }

    fn read_vram_transfer_pixel(
        &self,
        x: usize,
        y: usize,
        width: usize,
        pixel_index: usize,
    ) -> u16 {
        let px = (x + (pixel_index % width)) & (GPU_VRAM_WIDTH - 1);
        let py = (y + (pixel_index / width)) % GPU_VRAM_HEIGHT;
        self.vram[py * GPU_VRAM_WIDTH + px]
    }

    fn copy_vram_to_vram(&mut self, src_xy: u32, dst_xy: u32, wh: u32) {
        let src_x = (src_xy & 0x03ff) as usize;
        let src_y = ((src_xy >> 16) & 0x01ff) as usize;
        let dst_x = (dst_xy & 0x03ff) as usize;
        let dst_y = ((dst_xy >> 16) & 0x01ff) as usize;
        let (width, height) = copy_size(wh);
        let mut pixels = Vec::with_capacity(width as usize * height as usize);
        for y in 0..height as usize {
            for x in 0..width as usize {
                let sx = (src_x + x) & (GPU_VRAM_WIDTH - 1);
                let sy = (src_y + y) % GPU_VRAM_HEIGHT;
                pixels.push(self.vram[sy * GPU_VRAM_WIDTH + sx]);
            }
        }
        for y in 0..height as usize {
            for x in 0..width as usize {
                let dx = (dst_x + x) & (GPU_VRAM_WIDTH - 1);
                let dy = (dst_y + y) % GPU_VRAM_HEIGHT;
                self.write_transfer_pixel(dx, dy, pixels[y * width as usize + x]);
            }
        }
    }

    fn render_rectangle(&mut self, words: &[u32]) {
        let command = (words[0] >> 24) as u8;
        let textured = (command & 0x04) != 0;
        let semi_transparent = (command & 0x02) != 0;
        let raw_texture = (command & 0x01) != 0;
        let size_code = (command >> 3) & 0x03;
        let mut index = 1usize;
        let (x, y) = self.decode_vertex(words[index]);
        index += 1;
        let mut u = 0i32;
        let mut v = 0i32;
        let mut clut = 0u16;
        if textured {
            let uv = words[index];
            index += 1;
            u = (uv & 0xff) as i32;
            v = ((uv >> 8) & 0xff) as i32;
            clut = (uv >> 16) as u16;
        }
        let (width, height) = match size_code {
            0 => {
                let wh = words[index];
                ((wh & 0xffff) as i32, ((wh >> 16) & 0xffff) as i32)
            }
            1 => (1, 1),
            2 => (8, 8),
            _ => (16, 16),
        };
        if width <= 0 || height <= 0 || width > 1023 || height > 511 {
            return;
        }

        let base_color = words[0] & 0x00ff_ffff;
        let flip_x = (self.draw_mode & (1 << 12)) != 0;
        let flip_y = (self.draw_mode & (1 << 13)) != 0;
        for dy in 0..height {
            for dx in 0..width {
                let tex_u = if flip_x { u - dx } else { u + dx };
                let tex_v = if flip_y { v - dy } else { v + dy };
                let color = if textured {
                    let Some((texel, transparent)) = self.sample_texture(TextureSample {
                        u: tex_u,
                        v: tex_v,
                        clut,
                        texpage: self.draw_mode,
                        raw_texture,
                        shade: base_color,
                        screen_x: x + dx,
                        screen_y: y + dy,
                    }) else {
                        continue;
                    };
                    if transparent {
                        continue;
                    }
                    texel
                } else {
                    self.render_color(base_color, x + dx, y + dy)
                };
                self.write_render_pixel(x + dx, y + dy, color, semi_transparent, textured);
            }
        }
    }

    fn render_polygon(&mut self, words: &[u32]) {
        let command = (words[0] >> 24) as u8;
        let gouraud = (command & 0x10) != 0;
        let quad = (command & 0x08) != 0;
        let textured = (command & 0x04) != 0;
        let semi_transparent = (command & 0x02) != 0;
        let raw_texture = (command & 0x01) != 0;
        let vertex_count = if quad { 4 } else { 3 };
        let mut vertices = Vec::with_capacity(vertex_count);
        let mut color = words[0] & 0x00ff_ffff;
        let mut index = 1usize;
        let mut clut = 0u16;
        let mut texpage = self.draw_mode;

        for vertex_index in 0..vertex_count {
            if gouraud && vertex_index != 0 {
                color = words[index] & 0x00ff_ffff;
                index += 1;
            }
            let (x, y) = self.decode_vertex(words[index]);
            index += 1;
            let mut u = 0i32;
            let mut v = 0i32;
            if textured {
                let uv = words[index];
                index += 1;
                u = (uv & 0xff) as i32;
                v = ((uv >> 8) & 0xff) as i32;
                if vertex_index == 0 {
                    clut = (uv >> 16) as u16;
                } else if vertex_index == 1 {
                    texpage = (texpage & !0x09ff) | ((uv >> 16) as u16 & 0x09ff);
                }
            }
            vertices.push(Vertex { x, y, u, v, color });
        }

        self.render_triangle(
            [vertices[0], vertices[1], vertices[2]],
            PolygonStyle {
                textured,
                gouraud,
                semi_transparent,
                raw_texture,
                clut,
                texpage,
                flat_color: words[0] & 0x00ff_ffff,
            },
        );
        if quad {
            self.render_triangle(
                [vertices[1], vertices[2], vertices[3]],
                PolygonStyle {
                    textured,
                    gouraud,
                    semi_transparent,
                    raw_texture,
                    clut,
                    texpage,
                    flat_color: words[0] & 0x00ff_ffff,
                },
            );
        }
    }

    fn render_triangle(&mut self, vertices: [Vertex; 3], style: PolygonStyle) {
        let min_x = vertices
            .iter()
            .map(|v| v.x)
            .min()
            .unwrap()
            .max(self.draw_area_x1 as i32)
            .max(0);
        let max_x = vertices
            .iter()
            .map(|v| v.x)
            .max()
            .unwrap()
            .min(self.draw_area_x2 as i32)
            .min((GPU_VRAM_WIDTH - 1) as i32);
        let min_y = vertices
            .iter()
            .map(|v| v.y)
            .min()
            .unwrap()
            .max(self.draw_area_y1 as i32)
            .max(0);
        let max_y = vertices
            .iter()
            .map(|v| v.y)
            .max()
            .unwrap()
            .min(self.draw_area_y2 as i32)
            .min((GPU_VRAM_HEIGHT - 1) as i32);
        if min_x > max_x || min_y > max_y {
            return;
        }
        if max_vertex_delta(vertices.iter().map(|v| v.x)) > 1023
            || max_vertex_delta(vertices.iter().map(|v| v.y)) > 511
        {
            return;
        }

        let area = edge(vertices[0], vertices[1], vertices[2].x, vertices[2].y);
        if area == 0 {
            return;
        }
        let sign = area.signum();
        let area_abs = area.abs();

        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let w0 = edge(vertices[1], vertices[2], x, y) * sign;
                let w1 = edge(vertices[2], vertices[0], x, y) * sign;
                let w2 = edge(vertices[0], vertices[1], x, y) * sign;
                if w0 < 0 || w1 < 0 || w2 < 0 {
                    continue;
                }

                let shade = if style.gouraud {
                    interpolate_rgb24(vertices, [w0, w1, w2], area_abs)
                } else {
                    style.flat_color
                };
                let color = if style.textured {
                    let u = interpolate_i32(
                        vertices[0].u,
                        vertices[1].u,
                        vertices[2].u,
                        [w0, w1, w2],
                        area_abs,
                    );
                    let v = interpolate_i32(
                        vertices[0].v,
                        vertices[1].v,
                        vertices[2].v,
                        [w0, w1, w2],
                        area_abs,
                    );
                    let Some((texel, transparent)) = self.sample_texture(TextureSample {
                        u,
                        v,
                        clut: style.clut,
                        texpage: style.texpage,
                        raw_texture: style.raw_texture,
                        shade,
                        screen_x: x,
                        screen_y: y,
                    }) else {
                        continue;
                    };
                    if transparent {
                        continue;
                    }
                    texel
                } else {
                    self.render_color(shade, x, y)
                };
                self.write_render_pixel(x, y, color, style.semi_transparent, style.textured);
            }
        }
    }

    fn render_line_packet(&mut self, words: &[u32]) {
        let command = (words[0] >> 24) as u8;
        let gouraud = (command & 0x10) != 0;
        let (x0, y0) = self.decode_vertex(words[1]);
        let (color0, vertex1_index) = if gouraud {
            (words[2] & 0x00ff_ffff, 3)
        } else {
            (words[0] & 0x00ff_ffff, 2)
        };
        let (x1, y1) = self.decode_vertex(words[vertex1_index]);
        self.draw_line(
            Vertex {
                x: x0,
                y: y0,
                u: 0,
                v: 0,
                color: words[0] & 0x00ff_ffff,
            },
            Vertex {
                x: x1,
                y: y1,
                u: 0,
                v: 0,
                color: color0,
            },
            gouraud,
            (command & 0x02) != 0,
        );
    }

    fn render_polyline(&mut self, command: u32, words: &[u32], gouraud: bool) {
        let mut vertices = Vec::new();
        let mut colors = Vec::new();
        let mut index = 0usize;
        let mut current_color = command & 0x00ff_ffff;
        while index < words.len() {
            if gouraud {
                current_color = words[index] & 0x00ff_ffff;
                index += 1;
                if index >= words.len() {
                    break;
                }
            }
            let (x, y) = self.decode_vertex(words[index]);
            index += 1;
            vertices.push((x, y));
            colors.push(current_color);
        }
        for pair in vertices.windows(2).zip(colors.windows(2)) {
            let (points, colors) = pair;
            self.draw_line(
                Vertex {
                    x: points[0].0,
                    y: points[0].1,
                    u: 0,
                    v: 0,
                    color: colors[0],
                },
                Vertex {
                    x: points[1].0,
                    y: points[1].1,
                    u: 0,
                    v: 0,
                    color: colors[1],
                },
                gouraud,
                ((command >> 24) & 0x02) != 0,
            );
        }
    }

    fn draw_line(&mut self, start: Vertex, end: Vertex, gouraud: bool, semi_transparent: bool) {
        let dx = (end.x - start.x).abs();
        let dy = -(end.y - start.y).abs();
        if dx > 1023 || -dy > 511 {
            return;
        }

        let steps = dx.max(-dy).max(1);
        let sx = if start.x < end.x { 1 } else { -1 };
        let sy = if start.y < end.y { 1 } else { -1 };
        let mut err = dx + dy;
        let mut x = start.x;
        let mut y = start.y;
        let mut step = 0i32;

        loop {
            let color = if gouraud {
                interpolate_line_color(start.color, end.color, step, steps)
            } else {
                start.color
            };
            let pixel = self.render_color(color, x, y);
            self.write_render_pixel(x, y, pixel, semi_transparent, false);
            if x == end.x && y == end.y {
                break;
            }
            let err2 = err * 2;
            if err2 >= dy {
                err += dy;
                x += sx;
            }
            if err2 <= dx {
                err += dx;
                y += sy;
            }
            step += 1;
        }
    }

    fn decode_vertex(&self, word: u32) -> (i32, i32) {
        let x = sign_extend(word & 0x07ff, 11) + self.draw_offset_x as i32;
        let y = sign_extend((word >> 16) & 0x07ff, 11) + self.draw_offset_y as i32;
        (x, y)
    }

    fn render_color(&self, rgb: u32, x: i32, y: i32) -> u16 {
        if (self.draw_mode & (1 << 9)) != 0 {
            rgb24_to_bgr555_dither(rgb, x, y)
        } else {
            rgb24_to_bgr555(rgb)
        }
    }

    fn sample_texture(&self, sample: TextureSample) -> Option<(u16, bool)> {
        let (u, v) = self.apply_texture_window(sample.u, sample.v);
        let mode = ((sample.texpage >> 7) & 0x03).min(2);
        let base_x = ((sample.texpage & 0x0f) as usize) * 64;
        let base_y = (((sample.texpage >> 4) & 1) as usize) * 256;
        let texel = match mode {
            0 => {
                let packed = self.vram_at(base_x + (u as usize / 4), base_y + v as usize);
                let index = ((packed >> ((u & 3) * 4)) & 0x0f) as usize;
                let clut_x = ((sample.clut & 0x003f) as usize) * 16;
                let clut_y = ((sample.clut >> 6) as usize) & 0x01ff;
                self.vram_at(clut_x + index, clut_y)
            }
            1 => {
                let packed = self.vram_at(base_x + (u as usize / 2), base_y + v as usize);
                let index = if (u & 1) == 0 {
                    packed & 0x00ff
                } else {
                    packed >> 8
                } as usize;
                let clut_x = ((sample.clut & 0x003f) as usize) * 16;
                let clut_y = ((sample.clut >> 6) as usize) & 0x01ff;
                self.vram_at(clut_x + index, clut_y)
            }
            _ => self.vram_at(base_x + u as usize, base_y + v as usize),
        };
        let transparent = (texel & 0x7fff) == 0 && (texel & 0x8000) == 0;
        if transparent {
            return Some((0, true));
        }
        if sample.raw_texture {
            Some((texel, false))
        } else {
            Some((
                modulate_texel(
                    texel,
                    sample.shade,
                    self.draw_mode & (1 << 9) != 0,
                    sample.screen_x,
                    sample.screen_y,
                ),
                false,
            ))
        }
    }

    fn apply_texture_window(&self, u: i32, v: i32) -> (i32, i32) {
        let mask_x = ((self.texture_window & 0x1f) as i32) * 8;
        let mask_y = (((self.texture_window >> 5) & 0x1f) as i32) * 8;
        let offset_x = (((self.texture_window >> 10) & 0x1f) as i32) * 8;
        let offset_y = (((self.texture_window >> 15) & 0x1f) as i32) * 8;
        let u = (u & !mask_x) | (offset_x & mask_x);
        let v = (v & !mask_y) | (offset_y & mask_y);
        (u & 0xff, v & 0xff)
    }

    fn write_render_pixel(
        &mut self,
        x: i32,
        y: i32,
        mut color: u16,
        semi_transparent: bool,
        textured: bool,
    ) {
        if x < self.draw_area_x1 as i32
            || x > self.draw_area_x2 as i32
            || y < self.draw_area_y1 as i32
            || y > self.draw_area_y2 as i32
            || x < 0
            || y < 0
            || x >= GPU_VRAM_WIDTH as i32
            || y >= GPU_VRAM_HEIGHT as i32
        {
            return;
        }

        let index = y as usize * GPU_VRAM_WIDTH + x as usize;
        let old = self.vram[index];
        if self.mask_check && (old & 0x8000) != 0 {
            return;
        }
        if semi_transparent && (!textured || (color & 0x8000) != 0) {
            color = blend555(old, color, (self.draw_mode >> 5) & 0x03);
        }
        if self.mask_set {
            color |= 0x8000;
        } else if !textured {
            color &= 0x7fff;
        }
        self.vram[index] = color;
    }

    fn write_transfer_pixel(&mut self, x: usize, y: usize, mut pixel: u16) {
        let index = (y % GPU_VRAM_HEIGHT) * GPU_VRAM_WIDTH + (x & (GPU_VRAM_WIDTH - 1));
        if self.mask_check && (self.vram[index] & 0x8000) != 0 {
            return;
        }
        if self.mask_set {
            pixel |= 0x8000;
        }
        self.vram[index] = pixel;
    }

    fn vram_at(&self, x: usize, y: usize) -> u16 {
        self.vram[(y % GPU_VRAM_HEIGHT) * GPU_VRAM_WIDTH + (x & (GPU_VRAM_WIDTH - 1))]
    }

    fn set_draw_mode(&mut self, word: u32) {
        self.draw_mode = (word & 0x3fff) as u16;
        self.sync_draw_mode_status();
    }

    fn sync_draw_mode_status(&mut self) {
        self.status &= !0x0000_87ff;
        self.status |= (self.draw_mode as u32) & 0x07ff;
        self.status |= (((self.draw_mode >> 11) & 1) as u32) << 15;
    }

    fn sync_mask_status(&mut self) {
        self.status &= !(GPUSTAT_MASK_SET | GPUSTAT_MASK_CHECK);
        if self.mask_set {
            self.status |= GPUSTAT_MASK_SET;
        }
        if self.mask_check {
            self.status |= GPUSTAT_MASK_CHECK;
        }
    }

    fn sync_display_mode_status(&mut self) {
        self.status &= !0x0075_c000;
        self.status |= ((self.display_mode & 0x03) << 17)
            | (((self.display_mode >> 2) & 1) << 19)
            | (((self.display_mode >> 3) & 1) << 20)
            | (((self.display_mode >> 4) & 1) << 21)
            | (((self.display_mode >> 5) & 1) << 22)
            | (((self.display_mode >> 6) & 1) << 16)
            | (((self.display_mode >> 7) & 1) << 14);
        if (self.display_mode & (1 << 5)) == 0 {
            self.status |= 1 << 13;
        } else {
            self.status &= !(1 << 13);
        }
    }

    fn update_display_size(&mut self) {
        let mode_width = if (self.display_mode & (1 << 6)) != 0 {
            368
        } else {
            match self.display_mode & 0x03 {
                0 => 256,
                1 => 320,
                2 => 512,
                _ => 640,
            }
        };
        let cycles_per_pixel = match mode_width {
            256 => 10,
            320 => 8,
            368 => 7,
            512 => 5,
            _ => 4,
        };
        let range = self.display_h_end.saturating_sub(self.display_h_start) as usize;
        self.display_width = if range == 0 {
            mode_width
        } else {
            (((range / cycles_per_pixel) + 2) & !3).clamp(1, 640)
        };

        let mode_height =
            if (self.display_mode & (1 << 5)) != 0 && (self.display_mode & (1 << 2)) != 0 {
                480
            } else {
                240
            };
        let range_height = self.display_v_end.saturating_sub(self.display_v_start) as usize;
        self.display_height = if range_height == 0 {
            mode_height
        } else {
            range_height.clamp(1, 480)
        };
    }

    fn dma_request(&self, status: u32) -> bool {
        match self.dma_direction {
            0 => false,
            1 => true,
            2 => (status & GPUSTAT_READY_DMA) != 0,
            3 => (status & GPUSTAT_READY_READ) != 0,
            _ => false,
        }
    }

    fn gp1_info(&self, selector: u32) -> u32 {
        match selector & 0x0f {
            0x2 => self.texture_window,
            0x3 => ((self.draw_area_y1 as u32) << 10) | self.draw_area_x1 as u32,
            0x4 => ((self.draw_area_y2 as u32) << 10) | self.draw_area_x2 as u32,
            0x5 => encode_draw_offset(self.draw_offset_x, self.draw_offset_y),
            0x7 => 2,
            _ => self.read_response,
        }
    }

    fn reset(&mut self) {
        self.vram.fill(0);
        self.status = 0x1480_2000;
        self.read_response = 0;
        self.packet = Gp0Packet::Idle;
        self.vram_read.clear();
        self.display_x = 0;
        self.display_y = 0;
        self.display_h_start = 0x200;
        self.display_h_end = 0x200 + 320 * 8;
        self.display_v_start = 0x10;
        self.display_v_end = 0x10 + 240;
        self.display_width = 320;
        self.display_height = 240;
        self.display_mode = 0;
        self.dma_direction = 0;
        self.draw_mode = 0;
        self.texture_window = 0;
        self.draw_area_x1 = 0;
        self.draw_area_y1 = 0;
        self.draw_area_x2 = (GPU_VRAM_WIDTH - 1) as u16;
        self.draw_area_y2 = (GPU_VRAM_HEIGHT - 1) as u16;
        self.draw_offset_x = 0;
        self.draw_offset_y = 0;
        self.mask_set = false;
        self.mask_check = false;
        self.sync_draw_mode_status();
        self.sync_mask_status();
        self.sync_display_mode_status();
    }
}

impl Default for Gpu {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy)]
struct PolygonStyle {
    textured: bool,
    gouraud: bool,
    semi_transparent: bool,
    raw_texture: bool,
    clut: u16,
    texpage: u16,
    flat_color: u32,
}

#[derive(Debug, Clone, Copy)]
struct TextureSample {
    u: i32,
    v: i32,
    clut: u16,
    texpage: u16,
    raw_texture: bool,
    shade: u32,
    screen_x: i32,
    screen_y: i32,
}

fn polygon_word_count(command: u8) -> usize {
    let vertices = if (command & 0x08) != 0 { 4 } else { 3 };
    let textured = (command & 0x04) != 0;
    let gouraud = (command & 0x10) != 0;
    1 + vertices * (1 + usize::from(textured)) + if gouraud { vertices - 1 } else { 0 }
}

fn rectangle_word_count(command: u8) -> usize {
    let textured = (command & 0x04) != 0;
    let variable = ((command >> 3) & 0x03) == 0;
    1 + 1 + usize::from(textured) + usize::from(variable)
}

fn copy_size(wh: u32) -> (u16, u16) {
    let width = (((wh & 0xffff).wrapping_sub(1) & 0x03ff) + 1) as u16;
    let height = ((((wh >> 16) & 0xffff).wrapping_sub(1) & 0x01ff) + 1) as u16;
    (width, height)
}

fn rgb24_to_bgr555(rgb: u32) -> u16 {
    let r = ((rgb >> 3) & 0x1f) as u16;
    let g = ((rgb >> 11) & 0x1f) as u16;
    let b = ((rgb >> 19) & 0x1f) as u16;
    r | (g << 5) | (b << 10)
}

fn rgb24_to_bgr555_dither(rgb: u32, x: i32, y: i32) -> u16 {
    const DITHER: [[i32; 4]; 4] = [
        [-4, 0, -3, 1],
        [2, -2, 3, -1],
        [-3, 1, -4, 0],
        [3, -1, 2, -2],
    ];
    let d = DITHER[(y as usize) & 3][(x as usize) & 3];
    let r = dither_channel((rgb & 0xff) as i32, d);
    let g = dither_channel(((rgb >> 8) & 0xff) as i32, d);
    let b = dither_channel(((rgb >> 16) & 0xff) as i32, d);
    r | (g << 5) | (b << 10)
}

fn dither_channel(value: i32, dither: i32) -> u16 {
    ((value + dither).clamp(0, 255) as u16) >> 3
}

fn sign_extend(value: u32, bits: u32) -> i32 {
    let shift = 32 - bits;
    ((value << shift) as i32) >> shift
}

fn encode_draw_offset(x: i16, y: i16) -> u32 {
    ((x as i32 as u32) & 0x07ff) | (((y as i32 as u32) & 0x07ff) << 11)
}

fn edge(a: Vertex, b: Vertex, x: i32, y: i32) -> i64 {
    ((b.x - a.x) as i64) * ((y - a.y) as i64) - ((b.y - a.y) as i64) * ((x - a.x) as i64)
}

fn max_vertex_delta(values: impl Iterator<Item = i32>) -> i32 {
    let mut min = i32::MAX;
    let mut max = i32::MIN;
    for value in values {
        min = min.min(value);
        max = max.max(value);
    }
    max - min
}

fn interpolate_rgb24(vertices: [Vertex; 3], weights: [i64; 3], area: i64) -> u32 {
    let r = interpolate_component(
        vertices[0].color & 0xff,
        vertices[1].color & 0xff,
        vertices[2].color & 0xff,
        weights,
        area,
    );
    let g = interpolate_component(
        (vertices[0].color >> 8) & 0xff,
        (vertices[1].color >> 8) & 0xff,
        (vertices[2].color >> 8) & 0xff,
        weights,
        area,
    );
    let b = interpolate_component(
        (vertices[0].color >> 16) & 0xff,
        (vertices[1].color >> 16) & 0xff,
        (vertices[2].color >> 16) & 0xff,
        weights,
        area,
    );
    r | (g << 8) | (b << 16)
}

fn interpolate_component(a: u32, b: u32, c: u32, weights: [i64; 3], area: i64) -> u32 {
    ((a as i64 * weights[0] + b as i64 * weights[1] + c as i64 * weights[2]) / area) as u32
}

fn interpolate_i32(a: i32, b: i32, c: i32, weights: [i64; 3], area: i64) -> i32 {
    ((a as i64 * weights[0] + b as i64 * weights[1] + c as i64 * weights[2]) / area) as i32
}

fn interpolate_line_color(start: u32, end: u32, step: i32, steps: i32) -> u32 {
    let scale = |shift: u32| {
        let a = ((start >> shift) & 0xff) as i32;
        let b = ((end >> shift) & 0xff) as i32;
        (a + ((b - a) * step / steps)).clamp(0, 255) as u32
    };
    scale(0) | (scale(8) << 8) | (scale(16) << 16)
}

fn modulate_texel(texel: u16, shade: u32, dither: bool, x: i32, y: i32) -> u16 {
    let r = (((texel & 0x001f) as u32 * (shade & 0xff)) >> 7).min(0x1f);
    let g = ((((texel >> 5) & 0x001f) as u32 * ((shade >> 8) & 0xff)) >> 7).min(0x1f);
    let b = ((((texel >> 10) & 0x001f) as u32 * ((shade >> 16) & 0xff)) >> 7).min(0x1f);
    let rgb = (r << 3) | ((g << 3) << 8) | ((b << 3) << 16);
    let mut color = if dither {
        rgb24_to_bgr555_dither(rgb, x, y)
    } else {
        (r | (g << 5) | (b << 10)) as u16
    };
    color |= texel & 0x8000;
    color
}

fn blend555(back: u16, front: u16, mode: u16) -> u16 {
    let br = (back & 0x1f) as i32;
    let bg = ((back >> 5) & 0x1f) as i32;
    let bb = ((back >> 10) & 0x1f) as i32;
    let fr = (front & 0x1f) as i32;
    let fg = ((front >> 5) & 0x1f) as i32;
    let fb = ((front >> 10) & 0x1f) as i32;
    let blend = |b: i32, f: i32| {
        match mode {
            0 => (b + f) / 2,
            1 => b + f,
            2 => b - f,
            _ => b + f / 4,
        }
        .clamp(0, 31) as u16
    };
    blend(br, fr) | (blend(bg, fg) << 5) | (blend(bb, fb) << 10) | (front & 0x8000)
}

#[cfg(test)]
mod tests {
    use super::Gpu;

    #[test]
    fn cpu_to_vram_and_vram_to_cpu_round_trip_pixels() {
        let mut gpu = Gpu::new();
        gpu.write_gp0(0xa000_0000);
        gpu.write_gp0(0x0004_0002);
        gpu.write_gp0(0x0001_0003);
        gpu.write_gp0(0x2222_1111);
        gpu.write_gp0(0x0000_3333);

        gpu.write_gp0(0xc000_0000);
        gpu.write_gp0(0x0004_0002);
        gpu.write_gp0(0x0001_0003);

        assert_eq!(gpu.read_gp0(), 0x2222_1111);
        assert_eq!(gpu.read_gp0(), 0x0000_3333);
    }

    #[test]
    fn vram_to_vram_copy_respects_mask_check() {
        let mut gpu = Gpu::new();
        gpu.write_gp0(0xa000_0000);
        gpu.write_gp0(0x0000_0000);
        gpu.write_gp0(0x0001_0002);
        gpu.write_gp0(0x2222_1111);
        gpu.write_gp0(0xe600_0001);
        gpu.write_gp0(0xa000_0000);
        gpu.write_gp0(0x0000_0010);
        gpu.write_gp0(0x0001_0001);
        gpu.write_gp0(0x0000_3333);
        gpu.write_gp0(0xe600_0002);

        gpu.write_gp0(0x8000_0000);
        gpu.write_gp0(0x0000_0000);
        gpu.write_gp0(0x0000_0010);
        gpu.write_gp0(0x0001_0002);

        assert_eq!(gpu.vram()[16], 0xb333);
        assert_eq!(gpu.vram()[17], 0x2222);
    }

    #[test]
    fn draw_area_and_offset_clip_flat_rectangle() {
        let mut gpu = Gpu::new();
        gpu.write_gp1(0x0300_0000);
        gpu.write_gp0(0xe300_0000);
        gpu.write_gp0(0xe400_2002);
        gpu.write_gp0(0xe500_0801);

        gpu.write_gp0(0x6000_00ff);
        gpu.write_gp0(0x0000_0000);
        gpu.write_gp0(0x0004_0004);

        assert_eq!(gpu.vram()[1 + GPU_WIDTH_FOR_TEST], 0x001f);
        assert_eq!(gpu.vram()[3 + GPU_WIDTH_FOR_TEST], 0x0000);
    }

    #[test]
    fn flat_triangle_renders_inside_pixels() {
        let mut gpu = Gpu::new();
        gpu.write_gp0(0x2000_ff00);
        gpu.write_gp0(0x0001_0001);
        gpu.write_gp0(0x0001_0005);
        gpu.write_gp0(0x0005_0001);

        assert_eq!(gpu.vram()[2 * GPU_WIDTH_FOR_TEST + 2], 0x03e0);
    }

    #[test]
    fn textured_rectangle_samples_4bpp_clut() {
        let mut gpu = Gpu::new();
        gpu.write_gp0(0xa000_0000);
        gpu.write_gp0(0x0001_0000);
        gpu.write_gp0(0x0001_0002);
        gpu.write_gp0(0x03e0_001f);
        gpu.write_gp0(0xa000_0000);
        gpu.write_gp0(0x0000_0002);
        gpu.write_gp0(0x0001_0001);
        gpu.write_gp0(0x0000_0010);

        gpu.write_gp0(0x6500_0000);
        gpu.write_gp0(0x0000_0000);
        gpu.write_gp0(0x0040_0008);
        gpu.write_gp0(0x0001_0002);

        assert_eq!(gpu.vram()[0], 0x001f);
        assert_eq!(gpu.vram()[1], 0x03e0);
    }

    const GPU_WIDTH_FOR_TEST: usize = crate::GPU_VRAM_WIDTH;
}
