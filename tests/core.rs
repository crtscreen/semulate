use semulate::{
    Bus, Nes,
    apu::{Apu, CPU_HZ, SAMPLE_RATE},
    cartridge::{Cartridge, Mirroring},
    controller::Controller,
    cpu::{C, Cpu, CpuBus, D, N, V, Z},
    ppu::Ppu,
};
mod common;
fn fixture() -> Vec<u8> {
    common::rom(0, 0, 1, 1, &[0x4c, 0x00, 0x80])
}
fn cart() -> Cartridge {
    Cartridge::load(&fixture()).unwrap()
}
fn frame(n: &mut Nes) {
    n.run_frame().unwrap();
    n.bus.apu.samples.clear();
}

#[test]
fn rejects_bad_truncated_and_unsupported_roms() {
    for bytes in [vec![], vec![0; 16], {
        let mut bytes = fixture();
        bytes.pop();
        bytes
    }] {
        assert!(Cartridge::load(&bytes).is_err());
    }
    for (index, value) in [
        (4, 0),
        (4, 3),
        (5, 2),
        (6, 0x50),
        (7, 0x0c),
        (7, 1),
        (9, 1),
        (8, 2),
    ] {
        let mut bytes = fixture();
        bytes[index] = value;
        assert!(Cartridge::load(&bytes).is_err(), "header {index}={value}");
    }
}
#[test]
fn trainer_and_chr_ram() {
    let mut bytes = fixture()[..16].to_vec();
    bytes[5] = 0;
    bytes[6] |= 4;
    bytes.extend_from_slice(&[0xA5; 512]);
    bytes.extend_from_slice(&fixture()[16..16 + 16384]);
    let mut c = Cartridge::load(&bytes).unwrap();
    assert!(c.chr_ram);
    assert_eq!(&c.ram[0x1000..0x1200], &[0xa5; 512]);
    let mut p = Ppu::default();
    p.write_vram(&mut c, 0x100, 0x7f);
    assert_eq!(p.read_vram(&c, 0x100), 0x7f);
}
#[test]
fn cpu_ram_and_nrom_mirrors() {
    let mut b = Bus::new(cart());
    b.write(0x17ff, 0x45);
    assert_eq!(b.read(0x7ff), 0x45);
    assert_eq!(b.read(0x1fff), 0x45);
    for a in 0x8000..0xc000 {
        assert_eq!(b.peek(a), b.peek(a + 0x4000));
    }
}
#[test]
fn controllers_shift_and_preserve_latch() {
    let mut c = Controller::default();
    c.buttons = 0b10100101;
    c.write(1);
    assert_eq!(c.read(), 1);
    c.buttons = 0;
    assert_eq!(c.read(), 0);
    c.buttons = 0b10100101;
    c.write(0);
    c.buttons = 0;
    for bit in 0..8 {
        assert_eq!(c.read(), (0b10100101 >> bit) & 1);
        c.write(0);
    }
    for _ in 0..4 {
        assert_eq!(c.read(), 1);
    }
}
#[test]
fn nametable_and_palette_aliases() {
    for (mirror, pairs) in [
        (Mirroring::Vertical, [(0x2000, 0x2800), (0x2400, 0x2c00)]),
        (Mirroring::Horizontal, [(0x2000, 0x2400), (0x2800, 0x2c00)]),
    ] {
        let mut c = cart();
        c.mirroring = mirror;
        let mut p = Ppu::default();
        for (i, (a, b)) in pairs.into_iter().enumerate() {
            p.write_vram(&mut c, a, 10 + i as u8);
            assert_eq!(p.read_vram(&c, b), 10 + i as u8);
        }
        p.write_vram(&mut c, 0x3f10, 0x72);
        assert_eq!(p.read_vram(&c, 0x3f00), 0x32);
        assert_eq!(p.read_vram(&c, 0x3f30), 0x32);
    }
}
#[test]
fn ppudata_read_buffer_and_palette_bypass() {
    let mut c = cart();
    let mut p = Ppu::default();
    p.write_vram(&mut c, 0x2000, 0x4c);
    p.write_vram(&mut c, 0x2001, 0x35);
    p.write_register(&mut c, 6, 0x20);
    p.write_register(&mut c, 6, 0);
    assert_eq!(p.read_register(&mut c, 7), 0);
    assert_eq!(p.read_register(&mut c, 7), 0x4c);
    assert_eq!(p.read_register(&mut c, 7), 0x35);
    p.write_vram(&mut c, 0x3f00, 0x21);
    p.write_vram(&mut c, 0x2f00, 0x63);
    p.write_register(&mut c, 6, 0x3f);
    p.write_register(&mut c, 6, 0);
    assert_eq!(p.read_register(&mut c, 7) & 63, 0x21);
    p.write_register(&mut c, 6, 0x20);
    p.write_register(&mut c, 6, 0);
    assert_eq!(p.read_register(&mut c, 7), 0x63);
}
#[test]
fn ppustatus_clears_write_toggle() {
    let mut c = cart();
    let mut p = Ppu::default();
    p.write_register(&mut c, 6, 0x12);
    p.read_register(&mut c, 2);
    p.write_register(&mut c, 6, 0x20);
    p.write_register(&mut c, 6, 0x42);
    p.write_register(&mut c, 7, 0x55);
    assert_eq!(p.read_vram(&c, 0x2042), 0x55);
}
#[test]
fn oam_dma_bytes_and_stall_cycles() {
    for odd in [false, true] {
        let mut n = Nes::new(&fixture()).unwrap();
        n.cpu.pc = 0;
        n.bus.ram[0] = 0xea;
        for i in 0..256 {
            n.bus.ram[0x300 + i] = i as u8;
        }
        n.bus.ppu.oam_addr = 17;
        if (n.bus.cycles + 1) & 1 != odd as u64 {
            n.bus.read(0);
        }
        n.bus.write(0x4014, 3);
        let start = n.bus.cycles;
        n.step().unwrap();
        assert_eq!(n.bus.cycles - start, 513 + odd as u64 + 2);
        for i in 0..256 {
            assert_eq!(n.bus.ppu.oam[(i + 17) & 255], i as u8);
        }
    }
}
struct Flat {
    ram: Vec<u8>,
    cycles: u64,
}
impl CpuBus for Flat {
    fn read(&mut self, a: u16) -> u8 {
        self.cycles += 1;
        self.ram[a as usize]
    }
    fn write(&mut self, a: u16, v: u8) {
        self.cycles += 1;
        self.ram[a as usize] = v;
    }
}
#[test]
fn exhaustive_binary_adc_sbc_including_decimal_flag() {
    let mut b = Flat {
        ram: vec![0; 65536],
        cycles: 0,
    };
    for subtract in [false, true] {
        for carry in 0..=1u8 {
            for a in 0..=255u8 {
                for value in 0..=255u8 {
                    let mut cpu = Cpu::default();
                    cpu.a = a;
                    cpu.p = 0x20 | D | carry;
                    cpu.pc = 0x8000;
                    b.ram[0x8000] = if subtract { 0xe9 } else { 0x69 };
                    b.ram[0x8001] = value;
                    let signed = if subtract {
                        a as i8 as i16 - value as i8 as i16 - (1 - carry) as i16
                    } else {
                        a as i8 as i16 + value as i8 as i16 + carry as i16
                    };
                    let full = if subtract {
                        a as i16 - value as i16 - (1 - carry) as i16
                    } else {
                        a as i16 + value as i16 + carry as i16
                    };
                    cpu.step(&mut b).unwrap();
                    let result = full as u8;
                    assert_eq!(cpu.a, result);
                    assert_eq!(
                        cpu.p & C != 0,
                        if subtract { full >= 0 } else { full > 255 }
                    );
                    assert_eq!(cpu.p & Z != 0, result == 0);
                    assert_eq!(cpu.p & N != 0, result >= 128);
                    assert_eq!(cpu.p & V != 0, !(-128..=127).contains(&signed));
                }
            }
        }
    }
}
#[test]
fn indirect_jump_wraps_within_page() {
    let mut b = Flat {
        ram: vec![0; 65536],
        cycles: 0,
    };
    let mut c = Cpu::default();
    c.pc = 0x8000;
    b.ram[0x8000..0x8003].copy_from_slice(&[0x6c, 0xff, 0x12]);
    b.ram[0x12ff] = 0x34;
    b.ram[0x1200] = 0x56;
    b.ram[0x1300] = 0x99;
    c.step(&mut b).unwrap();
    assert_eq!(c.pc, 0x5634);
    assert_eq!(b.cycles, 5);
}
#[test]
fn audio_sample_rate_and_finite_output() {
    let mut a = Apu::default();
    a.write(0x4015, 1);
    a.write(0x4000, 0xbf);
    a.write(0x4002, 128);
    a.write(0x4003, 0x08);
    for _ in 0..CPU_HZ {
        a.tick();
    }
    assert_eq!(a.samples.len(), SAMPLE_RATE as usize);
    assert!(a.samples.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
    assert!(a.samples.iter().copied().fold(0f32, f32::max) > 0.02);
}

#[test]
fn jsr_rts_and_nmi_rti_restore_stack_and_pc() {
    let mut bytes = common::rom(0, 0, 1, 0, &[0x20, 0x10, 0x80, 0xa9, 0x42]);
    bytes[16 + 0x10..16 + 0x12].copy_from_slice(&[0xe8, 0x60]); // INX; RTS
    bytes[16 + 0x20..16 + 0x23].copy_from_slice(&[0xe6, 0x00, 0x40]); // INC $00; RTI
    bytes[16 + 0x3ffa..16 + 0x3ffc].copy_from_slice(&0x8020u16.to_le_bytes());
    let mut n = Nes::new(&bytes).unwrap();
    let sp = n.cpu.sp;
    n.step().unwrap(); // JSR
    assert_eq!(n.cpu.pc, 0x8010);
    assert_eq!(n.cpu.sp, sp - 2);
    n.step().unwrap(); // INX
    let cycles = n.bus.cycles;
    n.step().unwrap(); // RTS
    assert_eq!(n.bus.cycles - cycles, 6);
    assert_eq!((n.cpu.pc, n.cpu.sp, n.cpu.x), (0x8003, sp, 1));
    n.step().unwrap(); // LDA #$42
    let flags = n.cpu.p;
    n.cpu.interrupt(&mut n.bus, true);
    assert_eq!(n.cpu.pc, 0x8020);
    n.step().unwrap(); // INC $00
    let cycles = n.bus.cycles;
    n.step().unwrap(); // RTI
    assert_eq!(n.bus.cycles - cycles, 6);
    assert_eq!((n.cpu.pc, n.cpu.sp, n.cpu.p), (0x8005, sp, flags));
    assert_eq!((n.cpu.a, n.bus.ram[0]), (0x42, 1));
}

#[test]
fn nrom_background_matches_synthetic_chr_pixels() {
    let mut bytes = fixture();
    let chr = &mut bytes[16 + 16384..];
    for tile in 0..256 {
        for row in 0..8 {
            chr[tile * 16 + row] = (0x81u8).rotate_left((tile + row) as u32);
            chr[tile * 16 + row + 8] = (0x18u8).rotate_right((tile + row) as u32);
        }
    }
    for fine_x in 0..8 {
        let mut n = Nes::new(&bytes).unwrap();
        for i in 0..960 {
            n.bus.ppu.write_vram(&mut n.bus.cart, 0x2000 + i, i as u8);
        }
        for (offset, color) in [0x0f, 0x10, 0x21, 0x30].into_iter().enumerate() {
            n.bus
                .ppu
                .write_vram(&mut n.bus.cart, 0x3f00 + offset as u16, color);
        }
        n.bus.ppu.write_register(&mut n.bus.cart, 5, fine_x);
        n.bus.ppu.write_register(&mut n.bus.cart, 5, 0);
        n.bus.ppu.write_register(&mut n.bus.cart, 1, 0x0a);
        for _ in 0..3 {
            frame(&mut n);
        }
        for y in 0..64 {
            for x in 0..256 {
                let source_x = (x + fine_x as usize) % 256;
                let tile = (y / 8 * 32 + source_x / 8) & 255;
                let bit = 7 - source_x % 8;
                let lo = (n.bus.cart.chr[tile * 16 + y % 8] >> bit) & 1;
                let hi = (n.bus.cart.chr[tile * 16 + y % 8 + 8] >> bit) & 1;
                let index = [0x0f, 0x10, 0x21, 0x30][(lo | (hi << 1)) as usize];
                let [r, g, b] = semulate::ppu::PALETTE[index];
                let expected = 0xff000000 | ((r as u32) << 16) | ((g as u32) << 8) | b as u32;
                assert_eq!(n.bus.ppu.pixels[y * 256 + x], expected, "pixel {x},{y}");
            }
        }
    }
}
