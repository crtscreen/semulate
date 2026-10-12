//! end to end cartridge tests. bank contents encode physical chip addresses;
use semulate::{Bus, Nes, cartridge::Cartridge, cpu::CpuBus, ppu::Ppu};
mod common;

fn image(mapper: u8, prg: u8, chr: u8) -> Vec<u8> {
    let mut bytes = common::rom(mapper, 0, prg, chr, &[]);
    for (bank, data) in bytes[16..16 + prg as usize * 16384]
        .chunks_mut(8192)
        .enumerate()
    {
        data.fill(bank as u8);
        data[0x100] = 0xff; // safe write location for bus-conflict boards
    }
    for (bank, data) in bytes[16 + prg as usize * 16384..]
        .chunks_mut(1024)
        .enumerate()
    {
        data.fill(bank as u8);
    }
    bytes
}
fn nes2(mut bytes: Vec<u8>, submapper: u8, prg_ram: u8) -> Vec<u8> {
    bytes[7] |= 8;
    bytes[8] = submapper << 4;
    bytes[10] = prg_ram;
    if bytes[5] == 0 {
        bytes[11] = 7;
    }
    bytes
}
fn serial(b: &mut Bus, address: u16, value: u8) {
    for bit in 0..5 {
        b.write(address, (value >> bit) & 1);
        b.read(0);
    }
}
fn windows(b: &Bus) -> [u8; 4] {
    [
        b.peek(0x8000),
        b.peek(0xa000),
        b.peek(0xc000),
        b.peek(0xe000),
    ]
}
fn chr_windows(b: &Bus) -> [u8; 8] {
    std::array::from_fn(|i| b.ppu.read_vram(&b.cart, i as u16 * 1024))
}
fn mmc3_bank(b: &mut Bus, register: u8, value: u8) {
    b.write(0x8000, register);
    b.write(0x8001, value);
}

#[test]
fn uxrom_switches_low_bank_keeps_last_bank_and_chr_ram() {
    let mut b = Bus::new(Cartridge::load(&image(2, 16, 0)).unwrap());
    for bank in 0..16u8 {
        b.write(0xc100, bank);
        assert_eq!(windows(&b), [bank * 2, bank * 2 + 1, 30, 31]);
    }
    b.ppu.write_vram(&mut b.cart, 0x1234, 0x92);
    b.write(0xc100, 255); // absent address lines wrap at chip capacity
    assert_eq!(windows(&b), [30, 31, 30, 31]);
    assert_eq!(b.ppu.read_vram(&b.cart, 0x1234), 0x92);
    assert_eq!(b.cart.cpu_read(0x6000), None);
}

#[test]
fn cnrom_switches_chr_without_changing_prg_and_protects_rom() {
    let mut b = Bus::new(Cartridge::load(&image(3, 2, 4)).unwrap());
    for bank in 0..4u8 {
        b.write(0x8100, bank);
        assert_eq!(windows(&b), [0, 1, 2, 3]);
        assert_eq!(chr_windows(&b), std::array::from_fn(|i| bank * 8 + i as u8));
        b.ppu.write_vram(&mut b.cart, 0x1fff, 0x83);
        assert_eq!(b.ppu.read_vram(&b.cart, 0x1fff), bank * 8 + 7);
    }
}

#[test]
fn discrete_bus_conflicts_use_the_old_mapped_rom_byte_and_submapper() {
    for mapper in [2, 3, 7] {
        for sub in 0..=2 {
            let chr = if mapper == 3 { 4 } else { 0 };
            let prg = if mapper == 3 { 2 } else { 8 };
            let mut bytes = image(mapper, prg, chr);
            bytes[16 + 0x100] = 1;
            let mut b = Bus::new(Cartridge::load(&nes2(bytes, sub, 0)).unwrap());
            b.write(0x8100, 3);
            let bank = if sub == 2 || (sub == 0 && mapper != 7) {
                1
            } else {
                3
            };
            let actual = match mapper {
                2 => b.peek(0x8000) / 2,
                7 => b.peek(0x8000) / 4,
                _ => b.ppu.read_vram(&b.cart, 0) / 8,
            };
            assert_eq!(actual, bank, "mapper {mapper}, submapper {sub}");
        }
    }
}

#[test]
fn axrom_switches_32k_and_both_single_screen_pages() {
    let mut b = Bus::new(Cartridge::load(&image(7, 16, 0)).unwrap());
    for bank in 0..8u8 {
        b.write(0x8000, bank);
        assert_eq!(
            windows(&b),
            [bank * 4, bank * 4 + 1, bank * 4 + 2, bank * 4 + 3]
        );
    }
    b.write(0x8000, 0);
    b.ppu.write_vram(&mut b.cart, 0x2000, 0x31);
    b.write(0x8000, 0x10);
    b.ppu.write_vram(&mut b.cart, 0x2800, 0x72);
    for (bank, value) in [(0, 0x31), (0x10, 0x72)] {
        b.write(0x8000, bank);
        for a in [0x2000, 0x2400, 0x2800, 0x2c00, 0x3000] {
            assert_eq!(b.ppu.read_vram(&b.cart, a), value);
        }
    }
}

#[test]
fn mmc1_all_prg_modes_and_chr_modes() {
    let mut b = Bus::new(Cartridge::load(&image(1, 16, 16)).unwrap());
    assert_eq!(windows(&b), [0, 1, 30, 31]);
    for mode in 0..4 {
        serial(&mut b, 0x8000, mode << 2);
        for bank in 0..16u8 {
            serial(&mut b, 0xe000, bank);
            let expected = match mode {
                0 | 1 => [
                    (bank & !1) * 2,
                    (bank & !1) * 2 + 1,
                    (bank & !1) * 2 + 2,
                    (bank & !1) * 2 + 3,
                ],
                2 => [0, 1, bank * 2, bank * 2 + 1],
                _ => [bank * 2, bank * 2 + 1, 30, 31],
            };
            assert_eq!(windows(&b), expected);
        }
    }
    serial(&mut b, 0x8000, 0x1c);
    serial(&mut b, 0xa000, 5);
    serial(&mut b, 0xc000, 19);
    assert_eq!(chr_windows(&b), [20, 21, 22, 23, 76, 77, 78, 79]);
    serial(&mut b, 0x8000, 0x0c);
    assert_eq!(chr_windows(&b), [16, 17, 18, 19, 20, 21, 22, 23]);
}

#[test]
fn mmc1_serial_port_uses_fifth_address_reset_and_ignores_consecutive_writes() {
    let mut b = Bus::new(Cartridge::load(&image(1, 16, 1)).unwrap());
    for (i, bit) in [1, 0, 1, 0, 0].into_iter().enumerate() {
        b.write(if i == 4 { 0xe000 } else { 0x8000 }, bit);
        b.write(0xe000, 0x80); // ignored, even though it sets reset
        b.read(0);
    }
    assert_eq!(windows(&b), [10, 11, 30, 31]);
    serial(&mut b, 0x8000, 0); // 32 KiB mode
    b.write(0xe000, 1);
    b.read(0); // partial command
    b.write(0xc000, 0x80);
    b.read(0); // discard partial and force mode 3
    assert_eq!(windows(&b), [10, 11, 30, 31]);
    serial(&mut b, 0xe000, 2);
    assert_eq!(windows(&b), [4, 5, 30, 31]);
}

#[test]
fn mmc1_rmw_filter_is_connected_to_cpu_instruction_cycles() {
    let mut bytes = image(1, 16, 1);
    // INC $E100 writes $00 then $01. Four further zero bits must select bank 0.
    // If both RMW writes enter the serial port, the committed bank becomes 2.
    let code = [
        0xee, 0x00, 0xe1, 0xa9, 0, 0x8d, 0, 0xe0, 0x8d, 0, 0xe0, 0x8d, 0, 0xe0, 0x8d, 0, 0xe0,
    ];
    let last = 16 + 15 * 16384;
    bytes[last..last + code.len()].copy_from_slice(&code);
    bytes[last + 0x2100] = 0;
    bytes[last + 0x3ffc..last + 0x3ffe].copy_from_slice(&0xc000u16.to_le_bytes());
    let mut n = Nes::new(&bytes).unwrap();
    for _ in 0..6 {
        n.step().unwrap();
    }
    assert_eq!(&windows(&n.bus)[..2], &[0, 1]);
}

#[test]
fn mmc1_mirroring_ram_enable_and_fixed_serom() {
    let mut b = Bus::new(Cartridge::load(&image(1, 4, 1)).unwrap());
    let patterns = [[1, 1, 1, 1], [2, 2, 2, 2], [1, 2, 1, 2], [1, 1, 2, 2]];
    serial(&mut b, 0x8000, 0x0e);
    b.ppu.write_vram(&mut b.cart, 0x2000, 1);
    b.ppu.write_vram(&mut b.cart, 0x2400, 2);
    for (mode, pattern) in patterns.into_iter().enumerate() {
        serial(&mut b, 0x8000, 0x0c | mode as u8);
        for (table, expected) in pattern.into_iter().enumerate() {
            assert_eq!(
                b.ppu.read_vram(&b.cart, 0x2000 + table as u16 * 1024),
                expected
            );
        }
    }
    b.write(0x6000, 0x6b);
    b.read(0);
    serial(&mut b, 0xe000, 0x10);
    assert_eq!(b.cart.cpu_read(0x6000), None);
    b.write(0x6000, 0x84);
    b.read(0);
    serial(&mut b, 0xe000, 0);
    assert_eq!(b.read(0x6000), 0x6b);
    let mut fixed = Bus::new(Cartridge::load(&nes2(image(1, 2, 1), 5, 0)).unwrap());
    for mode in [0, 4, 8, 12] {
        serial(&mut fixed, 0x8000, mode);
        serial(&mut fixed, 0xe000, 15);
        assert_eq!(windows(&fixed), [0, 1, 2, 3]);
    }
}

#[test]
fn mmc1_surom_sxrom_banks_prg_ram_and_tracks_ppu_a12() {
    let mut b = Bus::new(Cartridge::load(&nes2(image(1, 32, 0), 0, 9)).unwrap());
    serial(&mut b, 0x8000, 0x1c);
    serial(&mut b, 0xa000, 0x10);
    serial(&mut b, 0xc000, 0);
    serial(&mut b, 0xe000, 3);
    b.write(0x2006, 0);
    b.write(0x2006, 0);
    assert_eq!(windows(&b), [38, 39, 62, 63]);
    b.write(0x2006, 0x10);
    b.write(0x2006, 0);
    assert_eq!(windows(&b), [6, 7, 30, 31]);
    b.write(0x2006, 0);
    b.write(0x2006, 0);
    b.read(0);
    for bank in 0..4u8 {
        serial(&mut b, 0xa000, bank << 2);
        b.write(0x6000, 0x51 + bank);
        b.read(0);
    }
    for bank in 0..4u8 {
        serial(&mut b, 0xa000, bank << 2);
        assert_eq!(b.read(0x6000), 0x51 + bank);
    }
    assert_eq!(b.cart.ram.len(), 32768);
    assert_eq!(
        [
            b.cart.ram[0],
            b.cart.ram[8192],
            b.cart.ram[16384],
            b.cart.ram[24576]
        ],
        [0x51, 0x52, 0x53, 0x54]
    );
}

#[test]
fn mmc1_sorom_and_snrom_ram_wiring() {
    let mut b = Bus::new(Cartridge::load(&nes2(image(1, 8, 0), 0, 8)).unwrap());
    b.write(0x6000, 1);
    b.read(0);
    serial(&mut b, 0xa000, 8);
    b.write(0x6000, 2);
    b.read(0);
    serial(&mut b, 0xa000, 0);
    assert_eq!(b.read(0x6000), 1);
    serial(&mut b, 0xa000, 8);
    assert_eq!(b.read(0x6000), 2);
    let mut b = Bus::new(Cartridge::load(&nes2(image(1, 8, 0), 0, 7)).unwrap());
    b.write(0x6000, 0x62);
    b.read(0);
    serial(&mut b, 0xa000, 0x10);
    assert_eq!(b.cart.cpu_read(0x6000), None);
    serial(&mut b, 0xa000, 0);
    assert_eq!(b.read(0x6000), 0x62);
}

#[test]
fn mmc3_prg_and_chr_windows_in_both_modes_with_address_aliases() {
    let mut b = Bus::new(Cartridge::load(&image(4, 32, 32)).unwrap());
    mmc3_bank(&mut b, 6, 0xff); // only low six PRG bits connected
    mmc3_bank(&mut b, 7, 11);
    assert_eq!(windows(&b), [63, 11, 62, 63]);
    b.write(0x9ffe, 0x46);
    b.write(0x9fff, 9);
    assert_eq!(windows(&b), [62, 11, 9, 63]);
    for (r, v) in [3, 9, 13, 17, 21, 25].into_iter().enumerate() {
        mmc3_bank(&mut b, r as u8, v);
    }
    assert_eq!(chr_windows(&b), [2, 3, 8, 9, 13, 17, 21, 25]);
    b.write(0x8000, 0x80);
    assert_eq!(chr_windows(&b), [13, 17, 21, 25, 2, 3, 8, 9]);
}

#[test]
fn mmc3_chr_ram_is_banked_and_prg_ram_is_enabled_and_write_protected() {
    let mut b = Bus::new(Cartridge::load(&image(4, 4, 0)).unwrap());
    mmc3_bank(&mut b, 2, 7);
    b.ppu.write_vram(&mut b.cart, 0x1000, 0x59);
    mmc3_bank(&mut b, 2, 3);
    assert_eq!(b.ppu.read_vram(&b.cart, 0x1000), 0);
    mmc3_bank(&mut b, 3, 7);
    assert_eq!(b.ppu.read_vram(&b.cart, 0x1400), 0x59);
    b.write(0xa001, 0x80);
    b.write(0x6000, 0x64);
    b.write(0xa001, 0xc0);
    b.write(0x6000, 0x23);
    assert_eq!(b.read(0x6000), 0x64);
    b.write(0xa001, 0);
    assert_eq!(b.cart.cpu_read(0x6000), None);
    b.write(0x6000, 0x12);
    b.write(0xa001, 0x80);
    assert_eq!(b.read(0x6000), 0x64);
}

#[test]
fn mmc3_mirroring_four_screen_and_hardwired_variant() {
    for (four, sub) in [(false, 0), (true, 0), (false, 2)] {
        let mut bytes = image(4, 4, 1);
        bytes[6] |= if four { 8 } else { 1 };
        let mut b = Bus::new(Cartridge::load(&nes2(bytes, sub, 0)).unwrap());
        for (a, v) in [(0x2000, 1), (0x2400, 2), (0x2800, 3), (0x2c00, 4)] {
            b.ppu.write_vram(&mut b.cart, a, v);
        }
        b.write(0xa000, 1);
        let actual: [u8; 4] =
            std::array::from_fn(|i| b.ppu.read_vram(&b.cart, 0x2000 + i as u16 * 1024));
        assert_eq!(
            actual,
            if four {
                [1, 2, 3, 4]
            } else if sub == 2 {
                [3, 4, 3, 4]
            } else {
                [3, 3, 4, 4]
            }
        );
    }
}

fn edge(c: &mut Cartridge, low_cycles: u8) {
    c.mapper.observe_ppu_address(0);
    for _ in 0..low_cycles {
        c.mapper.clock_cpu();
    }
    c.mapper.observe_ppu_address(0x1000);
}
#[test]
fn mmc3_irq_filter_reload_enable_acknowledge_and_revision() {
    for sub in [0, 4] {
        let mut c = Cartridge::load(&nes2(image(4, 4, 1), sub, 7)).unwrap();
        c.cpu_write(0xc000, 2, 0);
        c.cpu_write(0xc001, 0, 1);
        c.cpu_write(0xe001, 0, 2);
        edge(&mut c, 3);
        edge(&mut c, 2);
        edge(&mut c, 3);
        assert!(
            !c.mapper.irq_pending(),
            "short A12 low period must not clock"
        );
        edge(&mut c, 3);
        assert!(c.mapper.irq_pending());
        c.cpu_write(0xe000, 0, 3);
        assert!(!c.mapper.irq_pending());
        c.cpu_write(0xc000, 0, 4);
        c.cpu_write(0xe001, 0, 5);
        edge(&mut c, 3);
        assert_eq!(
            c.mapper.irq_pending(),
            sub == 0,
            "normal versus alternate zero reload"
        );
        c.cpu_write(0xe000, 0, 6);
        c.cpu_write(0xe001, 0, 7);
        c.cpu_write(0xc001, 0, 8);
        edge(&mut c, 3);
        assert!(
            c.mapper.irq_pending(),
            "forced reload also triggers alternate revision"
        );
    }
}

#[test]
fn mmc3_irq_is_driven_by_cpu_ppu_registers_and_delivered_to_cpu() {
    let mut bytes = common::rom(4, 0, 2, 1, &[0x58, 0xea, 0x4c, 1, 0x80]);
    bytes[16 + 0x7ffe..16 + 0x8000].copy_from_slice(&0x9000u16.to_le_bytes());
    bytes[16 + 0x1000..16 + 0x1004].copy_from_slice(&[0xe6, 0x10, 0x40, 0xea]);
    let mut n = Nes::new(&bytes).unwrap();
    n.bus.write(0x4017, 0x40); // isolate mapper IRQ from APU
    n.step().unwrap();
    n.step().unwrap(); // CLI and its delayed I-mask poll
    n.bus.write(0xc000, 0);
    n.bus.write(0xc001, 0);
    n.bus.write(0xe001, 0);
    n.bus.write(0x2006, 0);
    n.bus.write(0x2006, 0);
    for _ in 0..3 {
        n.bus.read(0);
    }
    n.bus.write(0x2006, 0x10);
    n.bus.write(0x2006, 0);
    assert!(n.bus.cart.mapper.irq_pending());
    n.step().unwrap(); // IRQ detector becomes ready during this JMP.
    assert_eq!(n.cpu.pc, 0x8001);
    n.step().unwrap();
    assert_eq!(n.cpu.pc, 0x9000);
    n.step().unwrap();
    assert_eq!(n.bus.ram[0x10], 1);
    n.bus.write(0xe000, 0);
    n.step().unwrap();
    assert_eq!(n.cpu.pc, 0x8001);
}

#[test]
fn mmc3_sprite_dummy_fetches_clock_irq_without_visible_sprites() {
    for ctrl in [0x08, 0x20] {
        // 8x8 at $1000; empty 8x16 sprites use tile $FF
        let mut b = Bus::new(Cartridge::load(&image(4, 2, 1)).unwrap());
        b.write(0x2000, ctrl);
        b.write(0x2001, 0x08);
        b.write(0xc000, 1);
        b.write(0xc001, 0);
        b.write(0xe001, 0);
        // Pre-render reload then first visible scanline decrements to zero.
        for _ in 0..240 {
            b.read(0);
        }
        assert!(b.cart.mapper.irq_pending(), "ctrl={ctrl:02x}");
    }
}

#[test]
fn selected_nes2_headers_sizes_and_invalid_boards_are_checked() {
    let valid = nes2(image(0, 1, 1), 0, 0);
    let c = Cartridge::load(&valid).unwrap();
    assert!(c.nes2);
    assert!(c.ram.is_empty());
    // Exponent/multiplier encoding: 2^14 PRG bytes and 2^13 CHR bytes.
    let mut exp = valid.clone();
    exp[4] = 14 << 2;
    exp[5] = 13 << 2;
    exp[9] = 0xff;
    assert_eq!(Cartridge::load(&exp).unwrap().prg.len(), 16384);
    for (index, value) in [
        (8, 0x10),
        (8, 1),
        (9, 0x0f),
        (10, 0x77),
        (11, 0x70),
        (12, 1),
        (12, 3),
        (14, 1),
        (15, 8),
    ] {
        let mut bad = valid.clone();
        bad[index] = value;
        assert!(Cartridge::load(&bad).is_err(), "header {index}={value}");
    }
    for sub in [1, 3, 5] {
        assert!(Cartridge::load(&nes2(image(4, 4, 1), sub, 7)).is_err());
    }
    let mut huge = nes2(image(2, 1, 0), 1, 0);
    huge[4] = 0xff;
    huge[9] = 15;
    assert!(Cartridge::load(&huge).is_err());
    let mut dual = valid;
    dual[12] = 2;
    assert!(
        Cartridge::load(&dual).is_ok(),
        "dual-region ROMs run with NTSC timing"
    );
}

#[test]
fn cartridge_ram_defaults_sizes_and_battery_flags() {
    let mut legacy = image(1, 8, 0);
    assert_eq!(Cartridge::load(&legacy).unwrap().ram.len(), 8192);
    legacy[8] = 4;
    assert_eq!(Cartridge::load(&legacy).unwrap().ram.len(), 32768);
    let mut bytes = nes2(image(1, 32, 0), 0, 0x90);
    bytes[6] |= 2;
    let c = Cartridge::load(&bytes).unwrap();
    assert!(c.battery);
    assert_eq!(c.ram.len(), 32768);
    let mut bytes = nes2(image(2, 4, 0), 1, 0x70);
    bytes[6] |= 2;
    let mut b = Bus::new(Cartridge::load(&bytes).unwrap());
    b.write(0x6000, 0x71);
    assert_eq!(b.read(0x6000), 0x71);
    bytes[6] &= !2;
    assert!(Cartridge::load(&bytes).is_err());
}

#[test]
fn ppu_inspection_does_not_clock_mapper_irqs() {
    let mut c = Cartridge::load(&image(4, 2, 1)).unwrap();
    c.cpu_write(0xc000, 0, 0);
    c.cpu_write(0xe001, 0, 1);
    let p = Ppu::default();
    for _ in 0..20 {
        p.read_vram(&c, 0);
        for _ in 0..3 {
            c.mapper.clock_cpu();
        }
        p.read_vram(&c, 0x1000);
    }
    assert!(!c.mapper.irq_pending());
}

#[test]
fn mmc3_upper_background_does_not_double_clock_at_scanline_boundaries() {
    let mut b = Bus::new(Cartridge::load(&image(4, 2, 1)).unwrap());
    b.ppu.scanline = 260; // arm during vblank, before the first pre-render fetch
    b.write(0x2000, 0x10);
    b.write(0x2001, 0x08);
    b.write(0xc000, 3);
    b.write(0xc001, 0);
    b.write(0xe001, 0);
    for _ in 0..1000 {
        b.read(0);
        if b.cart.mapper.irq_pending() {
            // Pre-render dot 5 loads 3; pre-render 325, line 0/325 and
            // line 1/325 decrement it. The aborted dot-0 fetch prevents
            // additional counter clocks at the beginning of each line.
            assert_eq!(b.ppu.scanline, 1);
            assert!((326..=328).contains(&b.ppu.dot));
            return;
        }
    }
    panic!("MMC3 IRQ did not occur");
}
