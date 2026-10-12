use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};
mod common;

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "semulate-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_semulate"))
            .args(args)
            .current_dir(&self.0)
            .output()
            .unwrap()
    }
    fn write_rom(&self, path: &str, mapper: u8) {
        fs::write(
            self.0.join(path),
            common::rom(mapper, 9, 1, 0, &[0x4c, 0, 0x80]),
        )
        .unwrap();
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}
fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn no_arguments_lists_roms_without_loading_even_a_single_file() {
    let work = Workspace::new();
    let output = work.run(&[]);
    assert!(output.status.success());
    assert!(stdout(&output).contains("Usage: semulate <game.nes>"));
    fs::create_dir(work.0.join("roms")).unwrap();
    // Invalid contents would fail if the only file were automatically opened.
    fs::write(work.0.join("roms/one game.NES"), b"not a ROM").unwrap();
    let output = work.run(&[]);
    assert!(output.status.success());
    assert!(stdout(&output).contains("roms/one game.NES"));
    assert!(!stdout(&output).contains("Completed"));
    assert!(!stdout(&output).contains("Lunar"));
}

#[test]
fn execution_options_require_an_explicit_rom_path() {
    let work = Workspace::new();
    for args in [
        &["--headless", "1"][..],
        &["--trace", "1"],
        &["--frames", "1"],
    ] {
        let output = work.run(args);
        assert!(!output.status.success());
        assert!(stderr(&output).contains("A ROM path is required"));
    }
}

#[test]
fn removed_demo_option_and_unsupported_mapper_fail_clearly() {
    let work = Workspace::new();
    let output = work.run(&["--demo"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("Unknown option --demo"));
    work.write_rom("other.nes", 5);
    let output = work.run(&["other.nes", "--headless", "1"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("Mapper 5 is not supported"));
}

#[test]
fn explicit_mapper218_rom_with_spaces_runs_and_exports_a_frame() {
    let work = Workspace::new();
    work.write_rom("own game.nes", 218);
    let output = work.run(&[
        "own game.nes",
        "--headless",
        "3",
        "--screenshot",
        "frame.ppm",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("Completed 3 frames"));
    let ppm = fs::read(work.0.join("frame.ppm")).unwrap();
    assert!(ppm.starts_with(b"P6\n256 240\n255\n"));
    assert_eq!(ppm.len(), b"P6\n256 240\n255\n".len() + 256 * 240 * 3);
}

#[test]
fn missing_and_truncated_roms_fail_without_falling_back() {
    let work = Workspace::new();
    assert!(
        !work
            .run(&["missing.nes", "--headless", "1"])
            .status
            .success()
    );
    fs::write(work.0.join("short.nes"), b"NES\x1a").unwrap();
    let output = work.run(&["short.nes", "--headless", "1"]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("16-byte NES header"));
    assert!(!Path::new(&work.0.join("short.sav")).exists());
}

#[test]
fn battery_saves_round_trip_all_supported_ram_sizes() {
    for (mapper, size) in [(0, 8192), (1, 16384), (1, 32768), (4, 8192)] {
        let work = Workspace::new();
        let mut rom = common::rom(mapper, 2, 2, 0, &[0x4c, 0, 0x80]);
        rom[8] = (size / 8192) as u8;
        fs::write(work.0.join("game.nes"), rom).unwrap();
        let expected: Vec<u8> = (0..size).map(|i| (i / 8192 + i % 251) as u8).collect();
        fs::write(work.0.join("game.sav"), &expected).unwrap();
        let out = work.run(&["game.nes", "--headless", "2"]);
        assert!(out.status.success(), "{}", stderr(&out));
        assert_eq!(fs::read(work.0.join("game.sav")).unwrap(), expected);
        fs::write(work.0.join("game.sav"), b"preserve this mismatched save").unwrap();
        let out = work.run(&["game.nes", "--headless", "1"]);
        assert!(!out.status.success());
        assert!(stderr(&out).contains("refusing to overwrite"));
        assert_eq!(
            fs::read(work.0.join("game.sav")).unwrap(),
            b"preserve this mismatched save"
        );
    }
}

#[test]
fn test_protocol_on_a_board_without_ram_fails_cleanly() {
    let work = Workspace::new();
    work.write_rom("no-ram.nes", 218);
    let out = work.run(&["no-ram.nes", "--test-rom", "1"]);
    assert!(!out.status.success());
    assert!(stderr(&out).contains("did not complete"));
    assert!(!stderr(&out).contains("panicked"));
}
