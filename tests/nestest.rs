use semulate::Nes;
fn hex(line: &str, key: &str, len: usize) -> u16 {
    let i = line.find(key).expect("reference field") + key.len();
    u16::from_str_radix(&line[i..i + len], 16).unwrap()
}

/// Explicitly ignored in offline builds: tools/check_nestest.py downloads the reference files.
#[test]
#[ignore = "requires external nestest fixtures; run python3 tools/check_nestest.py"]
fn nestest_reference_trace() {
    let rom = std::env::var("NESTEST_ROM").unwrap_or_else(|_| "test-roms/nestest.nes".into());
    let log = std::env::var("NESTEST_LOG").unwrap_or_else(|_| "test-roms/nestest.log".into());
    let mut nes = Nes::new(&std::fs::read(rom).expect("nestest ROM")).unwrap();
    nes.cpu.pc = 0xc000;
    let log = std::fs::read_to_string(log).expect("nestest reference log");
    let mut count = 0;
    for (i, line) in log.lines().enumerate() {
        let pc = u16::from_str_radix(&line[..4], 16).unwrap();
        let actual = [
            nes.cpu.pc,
            nes.cpu.a as u16,
            nes.cpu.x as u16,
            nes.cpu.y as u16,
            nes.cpu.p as u16,
            nes.cpu.sp as u16,
        ];
        let expected = [
            pc,
            hex(line, "A:", 2),
            hex(line, "X:", 2),
            hex(line, "Y:", 2),
            hex(line, "P:", 2),
            hex(line, "SP:", 2),
        ];
        assert_eq!(
            actual,
            expected,
            "line {}\nExpected: {}\nActual: {}",
            i + 1,
            line,
            nes.trace()
        );
        let cycle = line
            .split("CYC:")
            .nth(1)
            .unwrap()
            .trim()
            .parse::<u64>()
            .unwrap();
        assert_eq!(
            nes.bus.cycles,
            cycle,
            "cycle mismatch at line {} ({})",
            i + 1,
            nes.trace()
        );
        // The fixture validates the CPU in isolation; its diagnostic ends after this trace.
        nes.cpu.step(&mut nes.bus).unwrap();
        nes.bus.apu.samples.clear();
        count += 1;
    }
    assert_eq!(count, 8991, "unexpected/truncated reference log");
    println!("Matched all {count} CPU register states and cumulative cycle counts.");
}
