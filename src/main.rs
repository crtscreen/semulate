use semulate::{Nes, apu::SAMPLE_RATE};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
#[cfg(feature = "desktop")]
mod desktop;

fn main() {
    if let Err(e) = run() {
        eprintln!("Semulate NES: {e}");
        std::process::exit(1);
    }
}
pub fn read_rom(path: &Path) -> Result<Vec<u8>, String> {
    let meta = fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if meta.len() > 16 * 1024 * 1024 {
        return Err("ROM is too large (16 MiB limit).".into());
    }
    fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}
pub fn load_save(nes: &mut Nes, path: &Path) -> Result<(), String> {
    if !nes.bus.cart.battery {
        return Ok(());
    }
    let save = path.with_extension("sav");
    match fs::read(&save) {
        Ok(bytes) => {
            if bytes.len() != nes.bus.cart.ram.len() {
                return Err(format!(
                    "{} has an unexpected size; refusing to overwrite it.",
                    save.display()
                ));
            }
            nes.bus.cart.ram.copy_from_slice(&bytes);
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("{}: {e}", save.display())),
    }
    Ok(())
}
pub fn save_ram(nes: &Nes, path: Option<&Path>) -> Result<(), String> {
    if !nes.bus.cart.battery {
        return Ok(());
    }
    if let Some(path) = path {
        let save = path.with_extension("sav");
        let temp = path.with_extension(format!("sav.{}.tmp", std::process::id()));
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        let result = (|| -> std::io::Result<()> {
            file.write_all(&nes.bus.cart.ram)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temp, &save)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result.map_err(|e| format!("Cannot save {}: {e}", save.display()))?;
    }
    Ok(())
}
pub fn screenshot(nes: &Nes, path: &Path) -> Result<(), String> {
    let mut out = b"P6\n256 240\n255\n".to_vec();
    for p in &nes.bus.ppu.pixels {
        out.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
    }
    fs::write(path, out).map_err(|e| e.to_string())
}
fn wav(samples: &[f32], path: &Path) -> Result<(), String> {
    let n = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(n as usize + 44);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + n).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&n.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    fs::write(path, out).map_err(|e| e.to_string())
}
fn help() {
    println!(
        "Semulate NES {} -- NTSC / mappers 0, 1, 2, 3, 4, 7, 218\n\n\
Usage: semulate <game.nes> [options]\n\
Example: cargo run --release -- \"roms/my-game.nes\"\n\n\
No ROM argument: show usage and list .nes files in ./roms.\n\
Files are never loaded automatically.\n\n\
  --headless N       Run N frames without a window (1..10000)\n\
  --screenshot FILE  Export final frame as binary PPM (use .ppm)\n\
  --wav FILE         Export headless audio as PCM WAV\n\
  --trace N          Print N CPU states, then exit\n\
  --pc HEX           Override initial PC, e.g. C000 for nestest\n\
  --test-rom N       Run a Blargg protocol test for at most N frames\n\
  --frames N         Exit desktop after N frames (smoke testing)\n\
  --help             Show this help\n\n\
Keys: arrows=D-pad, Z=A, X=B, Enter=Start, right Shift=Select.\n\
P / Space: pause. N: single instruction while paused. R: reset.\n\
M: mute. F11: fullscreen. F12: screenshot. Escape: quit.\n\
SDL-compatible gamepads are supported. Drop a .nes file onto an open game window.\n\n\
Accuracy details and known limits: README.md",
        env!("CARGO_PKG_VERSION")
    );
}

fn list_roms() -> Result<(), String> {
    let directory = Path::new("roms");
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("\nPut your homebrew .nes files in roms/, or pass a path elsewhere.");
            return Ok(());
        }
        Err(e) => return Err(format!("Cannot list roms/: {e}")),
    };
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("Cannot list roms/: {e}"))?;
        let path = entry.path();
        if path.is_file()
            && path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("nes"))
        {
            paths.push(path);
        }
    }
    paths.sort();
    if paths.is_empty() {
        println!("\nNo .nes files found in roms/. Pass a ROM path to play.");
    } else {
        println!("\nROM files in roms/ (compatibility is checked when loaded):");
        for path in paths {
            println!("  {}", path.display());
        }
    }
    Ok(())
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1).peekable();
    if args.peek().is_none() {
        help();
        return list_roms();
    }
    let mut path: Option<PathBuf> = None;
    let mut headless = None;
    let mut shot = None;
    let mut sound = None;
    let mut trace = None;
    let mut pc = None;
    let mut test = false;
    let mut frames = None;
    while let Some(arg) = args.next() {
        let mut next = || args.next().ok_or_else(|| format!("{arg} requires a value"));
        match arg.as_str() {
            "--help" | "-h" => {
                help();
                return Ok(());
            }
            "--headless" | "--test-rom" => {
                let n = next()?
                    .parse::<u64>()
                    .map_err(|_| "Frame count must be an integer")?;
                if !(1..=10000).contains(&n) {
                    return Err("Frame count must be 1..10000.".into());
                }
                headless = Some(n);
                test = arg == "--test-rom";
            }
            "--screenshot" => shot = Some(PathBuf::from(next()?)),
            "--wav" => sound = Some(PathBuf::from(next()?)),
            "--trace" => {
                let n = next()?
                    .parse::<u64>()
                    .map_err(|_| "Trace count must be an integer")?;
                if n > 1_000_000 {
                    return Err("Trace limit is 1000000 instructions.".into());
                }
                trace = Some(n);
            }
            "--pc" => {
                pc = Some(
                    u16::from_str_radix(next()?.trim_start_matches("0x"), 16)
                        .map_err(|_| "Invalid hexadecimal PC")?,
                )
            }
            "--frames" => {
                frames = Some(
                    next()?
                        .parse::<u64>()
                        .map_err(|_| "Frame count must be an integer")?,
                );
            }
            _ if arg.starts_with('-') => return Err(format!("Unknown option {arg}. Use --help.")),
            _ => {
                if path.is_some() {
                    return Err("Pass only one ROM path.".into());
                }
                path = Some(PathBuf::from(arg));
            }
        }
    }
    if sound.is_some() && headless.is_none() {
        return Err("--wav requires --headless.".into());
    }
    let selected = path
        .as_deref()
        .ok_or("A ROM path is required. Example: cargo run --release -- \"roms/my-game.nes\"")?;
    let rom = read_rom(selected)?;
    let mut nes = Nes::new(&rom)?;
    load_save(&mut nes, selected)?;
    if let Some(pc) = pc {
        nes.cpu.pc = pc;
    }
    if let Some(count) = trace {
        for _ in 0..count {
            println!("{}", nes.trace());
            nes.step()?;
            nes.bus.apu.samples.clear();
        }
        return Ok(());
    }
    if let Some(count) = headless {
        let mut audio = Vec::new();
        let mut completed = false;
        let mut reset_after = None;
        for f in 0..count {
            nes.run_frame()?;
            if sound.is_some() {
                audio.append(&mut nes.bus.apu.samples);
            } else {
                nes.bus.apu.samples.clear();
            }
            if test && nes.bus.cart.ram.get(1..4) == Some(&[0xde, 0xb0, 0x61]) {
                let result = nes.bus.cart.ram[0];
                if result == 0x81 {
                    let deadline = *reset_after.get_or_insert(f + 30);
                    if f >= deadline {
                        nes.reset();
                        reset_after = Some(f + 30);
                    }
                } else if result < 0x80 {
                    let text: Vec<u8> = nes.bus.cart.ram[4..]
                        .iter()
                        .copied()
                        .take_while(|v| *v != 0)
                        .collect();
                    println!("{}", String::from_utf8_lossy(&text));
                    if result != 0 {
                        return Err(format!("Test ROM failed with status {result}."));
                    }
                    completed = true;
                    break;
                }
            }
        }
        if test && !completed {
            return Err(format!("Test ROM did not complete within {count} frames."));
        }
        if let Some(p) = shot {
            screenshot(&nes, &p)?;
        }
        if let Some(p) = sound {
            wav(&audio, &p)?;
        }
        println!(
            "Completed {} frames / {} CPU cycles.",
            nes.bus.ppu.frame, nes.bus.cycles
        );
        save_ram(&nes, path.as_deref())?;
        return Ok(());
    }
    #[cfg(feature = "desktop")]
    {
        desktop::run(&mut nes, &mut path, frames)?;
        if let Some(p) = shot {
            screenshot(&nes, &p)?;
        }
        save_ram(&nes, path.as_deref())
    }
    #[cfg(not(feature = "desktop"))]
    {
        let _ = (frames, shot);
        Err(
            "This build has no desktop frontend. Use --headless N or build with default features."
                .into(),
        )
    }
}
