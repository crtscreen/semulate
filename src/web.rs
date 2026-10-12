//! Small, dependency-free WebAssembly ABI. One emulator belongs to each instance.
//!
//! JavaScript copies a ROM/save into `semulate_input(size)`, then calls load/import.
//! Returned pointers are borrowed: copy their contents before the next mutating
//! call, and recreate JS views after a call that could grow WebAssembly memory.
//! No caller-provided pointer is dereferenced by Rust.
use crate::{
    Nes,
    apu::SAMPLE_RATE,
    ppu::{HEIGHT, WIDTH},
};
use std::{cell::RefCell, ptr};

const MAX_INPUT: usize = 16 * 1024 * 1024;

struct Player {
    nes: Option<Nes>,
    input: Vec<u8>,
    rgba: Vec<u8>,
    error: String,
}
impl Default for Player {
    fn default() -> Self {
        Self {
            nes: None,
            input: Vec::new(),
            rgba: vec![0; WIDTH * HEIGHT * 4],
            error: String::new(),
        }
    }
}
impl Player {
    fn paint(&mut self) {
        if let Some(nes) = &self.nes {
            for (rgb, rgba) in nes.bus.ppu.pixels.iter().zip(self.rgba.chunks_exact_mut(4)) {
                rgba.copy_from_slice(&[(rgb >> 16) as u8, (rgb >> 8) as u8, *rgb as u8, 255]);
            }
        }
    }
    fn result(&mut self, result: Result<(), String>) -> u32 {
        match result {
            Ok(()) => {
                self.error.clear();
                1
            }
            Err(error) => {
                self.error = error;
                0
            }
        }
    }
}
thread_local! { static PLAYER: RefCell<Player> = RefCell::new(Player::default()); }

#[unsafe(no_mangle)]
pub extern "C" fn semulate_input(size: u32) -> *mut u8 {
    PLAYER.with_borrow_mut(|p| {
        if size == 0 || size as usize > MAX_INPUT {
            p.error = "File must contain 1 byte to 16 MiB.".into();
            p.input.clear();
            return ptr::null_mut();
        }
        p.input.resize(size as usize, 0);
        p.input.as_mut_ptr()
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn semulate_load() -> u32 {
    PLAYER.with_borrow_mut(|p| {
        // Keep the currently loaded game intact if the new cartridge is invalid.
        match Nes::new(&p.input) {
            Ok(nes) => {
                p.nes = Some(nes);
                p.input.clear();
                p.paint();
                p.result(Ok(()))
            }
            Err(error) => p.result(Err(error)),
        }
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn semulate_frame() -> u32 {
    PLAYER.with_borrow_mut(|p| {
        let result = match &mut p.nes {
            Some(nes) => {
                nes.bus.apu.samples.clear();
                nes.run_frame()
            }
            None => Err("Choose a ROM before playing.".into()),
        };
        p.paint();
        p.result(result)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn semulate_reset() -> u32 {
    PLAYER.with_borrow_mut(|p| {
        let result = match &mut p.nes {
            Some(nes) => {
                nes.reset();
                nes.bus.apu.samples.clear();
                for controller in &mut nes.bus.controllers {
                    controller.buttons = 0;
                }
                Ok(())
            }
            None => Err("Choose a ROM before resetting.".into()),
        };
        p.result(result)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn semulate_buttons(port: u32, mask: u32) {
    PLAYER.with_borrow_mut(|p| {
        if let Some(nes) = &mut p.nes {
            if let Some(controller) = nes.bus.controllers.get_mut(port as usize) {
                controller.buttons = mask as u8;
            }
        }
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn semulate_pixels() -> *const u8 {
    PLAYER.with_borrow(|p| p.rgba.as_ptr())
}
#[unsafe(no_mangle)]
pub extern "C" fn semulate_pixels_len() -> u32 {
    (WIDTH * HEIGHT * 4) as u32
}
#[unsafe(no_mangle)]
pub extern "C" fn semulate_audio() -> *const f32 {
    PLAYER.with_borrow(|p| {
        p.nes
            .as_ref()
            .map_or(ptr::null(), |n| n.bus.apu.samples.as_ptr())
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn semulate_audio_len() -> u32 {
    PLAYER.with_borrow(|p| p.nes.as_ref().map_or(0, |n| n.bus.apu.samples.len() as u32))
}
#[unsafe(no_mangle)]
pub extern "C" fn semulate_sample_rate() -> u32 {
    SAMPLE_RATE
}
#[unsafe(no_mangle)]
pub extern "C" fn semulate_mapper() -> u32 {
    PLAYER.with_borrow(|p| {
        p.nes
            .as_ref()
            .map_or(u32::MAX, |n| n.bus.cart.mapper.number() as u32)
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn semulate_save_len() -> u32 {
    PLAYER.with_borrow(|p| {
        p.nes
            .as_ref()
            .filter(|n| n.bus.cart.battery)
            .map_or(0, |n| n.bus.cart.ram.len() as u32)
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn semulate_save() -> *const u8 {
    PLAYER.with_borrow(|p| {
        p.nes
            .as_ref()
            .filter(|n| n.bus.cart.battery)
            .map_or(ptr::null(), |n| n.bus.cart.ram.as_ptr())
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn semulate_import_save() -> u32 {
    PLAYER.with_borrow_mut(|p| {
        let result = match &mut p.nes {
            Some(nes) if !nes.bus.cart.battery => {
                Err("This cartridge has no battery-backed save RAM.".into())
            }
            Some(nes) if nes.bus.cart.ram.len() != p.input.len() => Err(format!(
                "Save must be exactly {} bytes; this file has {}.",
                nes.bus.cart.ram.len(),
                p.input.len()
            )),
            Some(nes) => {
                nes.bus.cart.ram.copy_from_slice(&p.input);
                Ok(())
            }
            None => Err("Choose a ROM before importing a save.".into()),
        };
        p.input.clear();
        p.result(result)
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn semulate_error() -> *const u8 {
    PLAYER.with_borrow(|p| p.error.as_ptr())
}
#[unsafe(no_mangle)]
pub extern "C" fn semulate_error_len() -> u32 {
    PLAYER.with_borrow(|p| p.error.len() as u32)
}
