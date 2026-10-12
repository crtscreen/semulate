//! sprite evaluation overflow glitches and analogue video effects are not emulated
use crate::cartridge::Cartridge;
pub const WIDTH: usize = 256;
pub const HEIGHT: usize = 240;

#[derive(Clone, Copy)]
struct Sprite {
    index: usize,
    x: u8,
    flags: u8,
    address: u16,
    low: u8,
    high: u8,
}

pub struct Ppu {
    pub pixels: Vec<u32>,
    pub frame: u64,
    pub scanline: u16,
    pub dot: u16,
    pub oam: [u8; 256],
    pub oam_addr: u8,
    ctrl: u8,
    mask: u8,
    status: u8,
    latch: u8,
    buffer: u8,
    v: u16,
    t: u16,
    fine_x: u8,
    w: bool,
    vram: [u8; 4096],
    palette: [u8; 32],
    nmi_line: bool,
    suppress_vblank: bool,
    pub nmi_pending: bool,
    low: u16,
    high: u16,
    attr_low: u16,
    attr_high: u16,
    tile: u8,
    attr: u8,
    next_low: u8,
    next_high: u8,
    sprites: Vec<Sprite>,
    next_sprites: Vec<Sprite>,
    skip_idle_fetch: bool,
}
impl Default for Ppu {
    fn default() -> Self {
        Self {
            pixels: vec![0xff000000; WIDTH * HEIGHT],
            frame: 0,
            scanline: 261,
            dot: 0,
            oam: [0xff; 256],
            oam_addr: 0,
            ctrl: 0,
            mask: 0,
            status: 0,
            latch: 0,
            buffer: 0,
            v: 0,
            t: 0,
            fine_x: 0,
            w: false,
            vram: [0; 4096],
            palette: [0; 32],
            nmi_line: false,
            suppress_vblank: false,
            nmi_pending: false,
            low: 0,
            high: 0,
            attr_low: 0,
            attr_high: 0,
            tile: 0,
            attr: 0,
            next_low: 0,
            next_high: 0,
            sprites: Vec::with_capacity(8),
            next_sprites: Vec::with_capacity(8),
            skip_idle_fetch: false,
        }
    }
}
impl Ppu {
    fn rendering(&self) -> bool {
        self.mask & 0x18 != 0
    }
    fn palette_index(address: u16) -> usize {
        let i = address as usize & 31;
        if i >= 16 && i & 3 == 0 { i - 16 } else { i }
    }
    pub fn read_vram(&self, cart: &Cartridge, address: u16) -> u8 {
        let a = address & 0x3fff;
        match a {
            0..=0x3eff => cart.ppu_read(a, &self.vram),
            _ => self.palette[Self::palette_index(a)],
        }
    }
    pub fn write_vram(&mut self, cart: &mut Cartridge, address: u16, value: u8) {
        let a = address & 0x3fff;
        match a {
            0..=0x3eff => cart.ppu_write(a, value, &mut self.vram),
            _ => self.palette[Self::palette_index(a)] = value & 63,
        }
    }
    fn nmi(&mut self) {
        let line = self.ctrl & 128 != 0 && self.status & 128 != 0;
        if line && !self.nmi_line {
            self.nmi_pending = true;
        }
        self.nmi_line = line;
    }
    pub fn read_register(&mut self, cart: &mut Cartridge, register: u16) -> u8 {
        let result = match register & 7 {
            2 => {
                if self.scanline == 241 && self.dot == 1 {
                    self.suppress_vblank = true;
                }
                let s = (self.status & 0xe0) | (self.latch & 31);
                self.status &= !128;
                self.w = false;
                self.nmi();
                s
            }
            4 => self.oam[self.oam_addr as usize],
            7 => {
                let a = self.v & 0x3fff;
                let raw = self.read_vram(cart, a);
                let result = if a >= 0x3f00 {
                    (raw & if self.mask & 1 != 0 { 0x30 } else { 0x3f }) | (self.latch & 0xc0)
                } else {
                    self.buffer
                };
                self.buffer = self.fetch(cart, if a >= 0x3f00 { a - 0x1000 } else { a });
                self.increment_address();
                self.drive_idle_address(cart);
                result
            }
            _ => self.latch,
        };
        self.latch = result;
        result
    }
    pub fn write_register(&mut self, cart: &mut Cartridge, register: u16, value: u8) {
        self.latch = value;
        match register & 7 {
            0 => {
                self.ctrl = value;
                self.t = (self.t & !0xc00) | ((value as u16 & 3) << 10);
                self.nmi();
            }
            1 => self.mask = value,
            3 => self.oam_addr = value,
            4 => {
                self.oam[self.oam_addr as usize] = value;
                self.oam_addr = self.oam_addr.wrapping_add(1);
            }
            5 => {
                if !self.w {
                    self.fine_x = value & 7;
                    self.t = (self.t & !31) | (value as u16 >> 3);
                } else {
                    self.t = (self.t & !0x73e0)
                        | ((value as u16 & 7) << 12)
                        | ((value as u16 & 0xf8) << 2);
                }
                self.w = !self.w;
            }
            6 => {
                if !self.w {
                    self.t = (self.t & 255) | ((value as u16 & 63) << 8);
                } else {
                    self.t = (self.t & 0x7f00) | value as u16;
                    self.v = self.t;
                    self.drive_idle_address(cart);
                }
                self.w = !self.w;
            }
            7 => {
                cart.mapper.observe_ppu_address(self.v & 0x3fff);
                self.write_vram(cart, self.v, value);
                self.increment_address();
                self.drive_idle_address(cart);
            }
            _ => {}
        }
    }
    fn fetch(&self, cart: &mut Cartridge, address: u16) -> u8 {
        cart.mapper.observe_ppu_address(address);
        self.read_vram(cart, address)
    }
    fn drive_idle_address(&self, cart: &mut Cartridge) {
        if !self.rendering() || (240..261).contains(&self.scanline) {
            cart.mapper.observe_ppu_address(self.v & 0x3fff);
        }
    }
    fn increment_address(&mut self) {
        if self.rendering() && (self.scanline < 240 || self.scanline == 261) {
            self.increment_x();
            self.increment_y();
        } else {
            self.v = self.v.wrapping_add(if self.ctrl & 4 != 0 { 32 } else { 1 }) & 0x7fff;
        }
    }
    fn increment_x(&mut self) {
        if self.v & 31 == 31 {
            self.v &= !31;
            self.v ^= 0x400;
        } else {
            self.v += 1;
        }
    }
    fn increment_y(&mut self) {
        if self.v & 0x7000 != 0x7000 {
            self.v += 0x1000;
        } else {
            self.v &= !0x7000;
            let mut y = (self.v & 0x3e0) >> 5;
            if y == 29 {
                y = 0;
                self.v ^= 0x800;
            } else if y == 31 {
                y = 0;
            } else {
                y += 1;
            }
            self.v = (self.v & !0x3e0) | (y << 5);
        }
    }
    fn load_shifters(&mut self) {
        self.low = (self.low & 0xff00) | self.next_low as u16;
        self.high = (self.high & 0xff00) | self.next_high as u16;
        self.attr_low = (self.attr_low & 0xff00) | if self.attr & 1 != 0 { 255 } else { 0 };
        self.attr_high = (self.attr_high & 0xff00) | if self.attr & 2 != 0 { 255 } else { 0 };
    }
    fn evaluate_sprites(&mut self) {
        self.next_sprites.clear();
        if self.scanline >= 239 {
            return;
        }
        let height = if self.ctrl & 32 != 0 { 16 } else { 8 };
        for n in 0..64 {
            let o = n * 4;
            let row = self.scanline as i32 - self.oam[o] as i32;
            if row < 0 || row >= height {
                continue;
            }
            if self.next_sprites.len() == 8 {
                self.status |= 32;
                break;
            }
            let flags = self.oam[o + 2];
            let row = if flags & 128 != 0 {
                height - 1 - row
            } else {
                row
            } as u16;
            let tile = self.oam[o + 1] as u16;
            let address = if height == 16 {
                (tile & 1) * 0x1000 + (tile & 0xfe) * 16 + (row / 8) * 16 + row % 8
            } else {
                (if self.ctrl & 8 != 0 { 0x1000 } else { 0 }) + tile * 16 + row
            };
            self.next_sprites.push(Sprite {
                index: n,
                x: self.oam[o + 3],
                flags,
                address,
                low: 0,
                high: 0,
            });
        }
    }
    fn fetch_sprite(&mut self, cart: &mut Cartridge) {
        let slot = ((self.dot - 257) / 8) as usize;
        let phase = (self.dot - 257) & 7;
        match phase {
            0 | 2 => {
                self.fetch(cart, 0x2000 | (self.v & 0xfff));
            }
            4 | 6 => {
                let dummy = if self.ctrl & 32 != 0 {
                    0x1ff0
                } else {
                    (if self.ctrl & 8 != 0 { 0x1000 } else { 0 }) + 0xff0
                };
                let address = self.next_sprites.get(slot).map_or(dummy, |s| s.address);
                let mut value = self.fetch(cart, address + if phase == 6 { 8 } else { 0 });
                if let Some(sprite) = self.next_sprites.get_mut(slot) {
                    if sprite.flags & 64 != 0 {
                        value = value.reverse_bits();
                    }
                    if phase == 4 {
                        sprite.low = value;
                    } else {
                        sprite.high = value;
                    }
                }
            }
            _ => {}
        }
    }
    fn pixel(&mut self) {
        let x = (self.dot - 1) as usize;
        let bit = 0x8000 >> self.fine_x;
        let background = self.mask & 8 != 0 && (x >= 8 || self.mask & 2 != 0);
        let bg = if background {
            ((self.low & bit != 0) as u8) | (((self.high & bit != 0) as u8) << 1)
        } else {
            0
        };
        let pal = ((self.attr_low & bit != 0) as u8) | (((self.attr_high & bit != 0) as u8) << 1);
        let mut color = if bg == 0 { 0 } else { pal * 4 + bg };
        if self.mask & 16 != 0 && (x >= 8 || self.mask & 4 != 0) {
            for &Sprite {
                index,
                x: sx,
                flags,
                low: lo,
                high: hi,
                ..
            } in &self.sprites
            {
                let offset = x as i32 - sx as i32;
                if !(0..8).contains(&offset) {
                    continue;
                }
                let p = ((lo >> (7 - offset)) & 1) | (((hi >> (7 - offset)) & 1) << 1);
                if p == 0 {
                    continue;
                }
                if index == 0 && bg != 0 && x < 255 {
                    self.status |= 64;
                }
                if bg == 0 || flags & 32 == 0 {
                    color = 16 + (flags & 3) * 4 + p;
                }
                break;
            }
        }
        if !self.rendering() && self.v & 0x3f00 == 0x3f00 {
            color = Self::palette_index(self.v) as u8;
        }
        let mut index = self.palette[color as usize] & 63;
        if self.mask & 1 != 0 {
            index &= 0x30;
        }
        let [mut r, mut g, mut b] = PALETTE[index as usize];
        if self.mask & 0x20 != 0 {
            g = (g as u16 * 3 / 4) as u8;
            b = (b as u16 * 3 / 4) as u8;
        }
        if self.mask & 0x40 != 0 {
            r = (r as u16 * 3 / 4) as u8;
            b = (b as u16 * 3 / 4) as u8;
        }
        if self.mask & 0x80 != 0 {
            r = (r as u16 * 3 / 4) as u8;
            g = (g as u16 * 3 / 4) as u8;
        }
        self.pixels[self.scanline as usize * WIDTH + x] =
            0xff000000 | ((r as u32) << 16) | ((g as u32) << 8) | b as u32;
    }
    pub fn tick(&mut self, cart: &mut Cartridge) {
        let pre = self.scanline == 261;
        let visible = self.scanline < 240;
        if pre && self.dot == 1 {
            self.status &= !0xe0;
            self.suppress_vblank = false;
            self.nmi();
        }
        if self.scanline == 241 && self.dot == 1 {
            if !self.suppress_vblank {
                self.status |= 128;
            }
            self.nmi();
        }
        self.drive_idle_address(cart);
        if visible && self.dot == 0 {
            std::mem::swap(&mut self.sprites, &mut self.next_sprites);
            self.next_sprites.clear();
            if self.rendering() && !self.skip_idle_fetch {
                let address =
                    ((self.ctrl as u16 & 16) << 8) + self.tile as u16 * 16 + (self.v >> 12 & 7);
                cart.mapper.observe_ppu_address(address);
            }
            self.skip_idle_fetch = false;
        }
        if (pre || visible) && self.rendering() {
            if (1..=256).contains(&self.dot) || (321..=336).contains(&self.dot) {
                self.low <<= 1;
                self.high <<= 1;
                self.attr_low <<= 1;
                self.attr_high <<= 1;
                match (self.dot - 1) & 7 {
                    0 => {
                        self.load_shifters();
                        self.tile = self.fetch(cart, 0x2000 | (self.v & 0xfff));
                    }
                    2 => {
                        let a = 0x23c0
                            | (self.v & 0xc00)
                            | ((self.v >> 4) & 0x38)
                            | ((self.v >> 2) & 7);
                        let shift = ((self.v >> 4) & 4) | (self.v & 2);
                        self.attr = (self.fetch(cart, a) >> shift) & 3;
                    }
                    4 => {
                        let a = ((self.ctrl as u16 & 16) << 8)
                            + (self.tile as u16) * 16
                            + (self.v >> 12 & 7);
                        self.next_low = self.fetch(cart, a);
                    }
                    6 => {
                        let a = ((self.ctrl as u16 & 16) << 8)
                            + (self.tile as u16) * 16
                            + (self.v >> 12 & 7)
                            + 8;
                        self.next_high = self.fetch(cart, a);
                    }
                    7 => self.increment_x(),
                    _ => {}
                }
            }
            if self.dot == 256 {
                self.increment_y();
            }
            if self.dot == 257 {
                self.evaluate_sprites();
                self.load_shifters();
                self.v = (self.v & !0x41f) | (self.t & 0x41f);
            }
            if (257..=320).contains(&self.dot) {
                self.fetch_sprite(cart);
            }
            if pre && (280..=304).contains(&self.dot) {
                self.v = (self.v & !0x7be0) | (self.t & 0x7be0);
            }
            if self.dot == 337 || self.dot == 339 {
                self.tile = self.fetch(cart, 0x2000 | (self.v & 0xfff));
            }
        }
        if visible && (1..=256).contains(&self.dot) {
            self.pixel();
        }
        if pre && self.dot == 339 && self.frame & 1 != 0 && self.rendering() {
            self.skip_idle_fetch = true;
            self.dot = 0;
            self.scanline = 0;
            return;
        }
        self.dot += 1;
        if self.dot == 341 {
            self.dot = 0;
            self.scanline += 1;
            if self.scanline == 262 {
                self.scanline = 0;
            }
            if self.scanline == 241 {
                self.frame += 1;
            }
        }
    }
}

pub const PALETTE: [[u8; 3]; 64] = [
    [84, 84, 84],
    [0, 30, 116],
    [8, 16, 144],
    [48, 0, 136],
    [68, 0, 100],
    [92, 0, 48],
    [84, 4, 0],
    [60, 24, 0],
    [32, 42, 0],
    [8, 58, 0],
    [0, 64, 0],
    [0, 60, 0],
    [0, 50, 60],
    [0, 0, 0],
    [0, 0, 0],
    [0, 0, 0],
    [152, 150, 152],
    [8, 76, 196],
    [48, 50, 236],
    [92, 30, 228],
    [136, 20, 176],
    [160, 20, 100],
    [152, 34, 32],
    [120, 60, 0],
    [84, 90, 0],
    [40, 114, 0],
    [8, 124, 0],
    [0, 118, 40],
    [0, 102, 120],
    [0, 0, 0],
    [0, 0, 0],
    [0, 0, 0],
    [236, 238, 236],
    [76, 154, 236],
    [120, 124, 236],
    [176, 98, 236],
    [228, 84, 236],
    [236, 88, 180],
    [236, 106, 100],
    [212, 136, 32],
    [160, 170, 0],
    [116, 196, 0],
    [76, 208, 32],
    [56, 204, 108],
    [56, 180, 204],
    [60, 60, 60],
    [0, 0, 0],
    [0, 0, 0],
    [236, 238, 236],
    [168, 204, 236],
    [188, 188, 236],
    [212, 178, 236],
    [236, 174, 236],
    [236, 174, 212],
    [236, 180, 176],
    [228, 196, 144],
    [204, 210, 120],
    [180, 222, 120],
    [168, 226, 144],
    [152, 226, 180],
    [160, 214, 228],
    [160, 162, 160],
    [0, 0, 0],
    [0, 0, 0],
];
