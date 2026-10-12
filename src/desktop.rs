//! minimal audited SDL2 FFI boundary
use semulate::{Nes, apu::SAMPLE_RATE};
use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    path::{Path, PathBuf},
    ptr,
    time::{Duration, Instant},
};
#[repr(C)]
struct AudioSpec {
    freq: c_int,
    format: u16,
    channels: u8,
    silence: u8,
    samples: u16,
    padding: u16,
    size: u32,
    callback: Option<unsafe extern "C" fn(*mut c_void, *mut u8, c_int)>,
    userdata: *mut c_void,
}
#[repr(C, align(8))]
struct Event {
    bytes: [u8; 56],
}
unsafe extern "C" {
    fn SDL_Init(flags: u32) -> c_int;
    fn SDL_InitSubSystem(flags: u32) -> c_int;
    fn SDL_Quit();
    fn SDL_GetError() -> *const c_char;
    fn SDL_CreateWindow(
        title: *const c_char,
        x: c_int,
        y: c_int,
        w: c_int,
        h: c_int,
        flags: u32,
    ) -> *mut c_void;
    fn SDL_DestroyWindow(window: *mut c_void);
    fn SDL_SetWindowTitle(window: *mut c_void, title: *const c_char);
    fn SDL_SetWindowFullscreen(window: *mut c_void, flags: u32) -> c_int;
    fn SDL_CreateRenderer(window: *mut c_void, index: c_int, flags: u32) -> *mut c_void;
    fn SDL_DestroyRenderer(renderer: *mut c_void);
    fn SDL_RenderSetLogicalSize(renderer: *mut c_void, w: c_int, h: c_int) -> c_int;
    fn SDL_RenderSetIntegerScale(renderer: *mut c_void, enabled: c_int) -> c_int;
    fn SDL_CreateTexture(
        renderer: *mut c_void,
        format: u32,
        access: c_int,
        w: c_int,
        h: c_int,
    ) -> *mut c_void;
    fn SDL_DestroyTexture(texture: *mut c_void);
    fn SDL_UpdateTexture(
        texture: *mut c_void,
        rect: *const c_void,
        pixels: *const c_void,
        pitch: c_int,
    ) -> c_int;
    fn SDL_RenderClear(renderer: *mut c_void) -> c_int;
    fn SDL_RenderCopy(
        renderer: *mut c_void,
        texture: *mut c_void,
        src: *const c_void,
        dst: *const c_void,
    ) -> c_int;
    fn SDL_RenderPresent(renderer: *mut c_void);
    fn SDL_PollEvent(event: *mut Event) -> c_int;
    fn SDL_free(p: *mut c_void);
    fn SDL_OpenAudioDevice(
        device: *const c_char,
        capture: c_int,
        wanted: *const AudioSpec,
        obtained: *mut AudioSpec,
        changes: c_int,
    ) -> u32;
    fn SDL_PauseAudioDevice(device: u32, pause: c_int);
    fn SDL_CloseAudioDevice(device: u32);
    fn SDL_QueueAudio(device: u32, data: *const c_void, len: u32) -> c_int;
    fn SDL_ClearQueuedAudio(device: u32);
    fn SDL_GetQueuedAudioSize(device: u32) -> u32;
    fn SDL_NumJoysticks() -> c_int;
    fn SDL_IsGameController(index: c_int) -> c_int;
    fn SDL_GameControllerOpen(index: c_int) -> *mut c_void;
    fn SDL_GameControllerClose(controller: *mut c_void);
    fn SDL_GameControllerGetButton(controller: *mut c_void, button: c_int) -> u8;
    fn SDL_GameControllerGetAttached(controller: *mut c_void) -> c_int;
}
struct Sdl {
    window: *mut c_void,
    renderer: *mut c_void,
    texture: *mut c_void,
    audio: u32,
    pad: *mut c_void,
}
impl Drop for Sdl {
    fn drop(&mut self) {
        unsafe {
            if !self.pad.is_null() {
                SDL_GameControllerClose(self.pad);
            }
            if self.audio != 0 {
                SDL_CloseAudioDevice(self.audio);
            }
            if !self.texture.is_null() {
                SDL_DestroyTexture(self.texture);
            }
            if !self.renderer.is_null() {
                SDL_DestroyRenderer(self.renderer);
            }
            if !self.window.is_null() {
                SDL_DestroyWindow(self.window);
            }
            SDL_Quit();
        }
    }
}
fn error() -> String {
    unsafe {
        CStr::from_ptr(SDL_GetError())
            .to_string_lossy()
            .into_owned()
    }
}
fn check(value: c_int) -> Result<(), String> {
    if value < 0 { Err(error()) } else { Ok(()) }
}
impl Sdl {
    fn new() -> Result<Self, String> {
        unsafe {
            check(SDL_Init(0x20 | 0x2000))?;
            let mut s = Self {
                window: ptr::null_mut(),
                renderer: ptr::null_mut(),
                texture: ptr::null_mut(),
                audio: 0,
                pad: ptr::null_mut(),
            };
            s.window =
                SDL_CreateWindow(c"Semulate".as_ptr(), 0x2fff0000, 0x2fff0000, 768, 720, 0x24);
            if s.window.is_null() {
                return Err(error());
            }
            s.renderer = SDL_CreateRenderer(s.window, -1, 2);
            if s.renderer.is_null() {
                s.renderer = SDL_CreateRenderer(s.window, -1, 1);
            }
            if s.renderer.is_null() {
                return Err(error());
            }
            check(SDL_RenderSetLogicalSize(s.renderer, 256, 240))?;
            check(SDL_RenderSetIntegerScale(s.renderer, 1))?;
            s.texture = SDL_CreateTexture(s.renderer, 0x16362004, 1, 256, 240);
            if s.texture.is_null() {
                return Err(error());
            }
            if SDL_InitSubSystem(0x10) == 0 {
                let desired = AudioSpec {
                    freq: SAMPLE_RATE as c_int,
                    format: if cfg!(target_endian = "little") {
                        0x8120
                    } else {
                        0x9120
                    },
                    channels: 1,
                    silence: 0,
                    samples: 1024,
                    padding: 0,
                    size: 0,
                    callback: None,
                    userdata: ptr::null_mut(),
                };
                s.audio = SDL_OpenAudioDevice(ptr::null(), 0, &desired, ptr::null_mut(), 0);
                if s.audio != 0 {
                    SDL_PauseAudioDevice(s.audio, 0);
                } else {
                    eprintln!("Audio unavailable: {}. Continuing silently.", error());
                }
            } else {
                eprintln!("Audio unavailable: {}. Continuing silently.", error());
            }
            Ok(s)
        }
    }
    fn pad_buttons(&mut self) -> u8 {
        unsafe {
            if !self.pad.is_null() && SDL_GameControllerGetAttached(self.pad) == 0 {
                SDL_GameControllerClose(self.pad);
                self.pad = ptr::null_mut();
            }
            if self.pad.is_null() {
                for i in 0..SDL_NumJoysticks() {
                    if SDL_IsGameController(i) != 0 {
                        self.pad = SDL_GameControllerOpen(i);
                        if !self.pad.is_null() {
                            break;
                        }
                    }
                }
            }
            if self.pad.is_null() {
                return 0;
            }
            let mut bits = 0;
            for (bit, button) in [0, 1, 4, 6, 11, 12, 13, 14].into_iter().enumerate() {
                if SDL_GameControllerGetButton(self.pad, button) != 0 {
                    bits |= 1 << bit;
                }
            }
            bits
        }
    }
    fn title(&self, nes: &Nes, paused: bool, muted: bool) {
        let title = format!(
            "Semulate NES | NTSC / {}{}{} | PC {:04X} A {:02X} X {:02X} Y {:02X} | P pause - N step - R reset",
            nes.bus.cart.mapper.name(),
            if paused { " / PAUSED" } else { "" },
            if muted { " / muted" } else { "" },
            nes.cpu.pc,
            nes.cpu.a,
            nes.cpu.x,
            nes.cpu.y
        );
        let title = CString::new(title).unwrap();
        unsafe {
            SDL_SetWindowTitle(self.window, title.as_ptr());
        }
    }
    fn clear_audio(&self) {
        if self.audio != 0 {
            unsafe {
                SDL_ClearQueuedAudio(self.audio);
            }
        }
    }
}
fn key_bit(scancode: i32) -> Option<u8> {
    match scancode {
        29 => Some(1),
        27 => Some(2),
        229 => Some(4),
        40 => Some(8),
        82 => Some(16),
        81 => Some(32),
        80 => Some(64),
        79 => Some(128),
        _ => None,
    }
}
pub fn run(
    nes: &mut Nes,
    path: &mut Option<PathBuf>,
    max_frames: Option<u64>,
) -> Result<(), String> {
    let mut s = Sdl::new()?;
    let mut event = Event { bytes: [0; 56] };
    let mut keys = 0u8;
    let mut paused = false;
    let mut muted = false;
    let mut fullscreen = false;
    let mut focused = true;
    let interval = Duration::from_secs_f64(89341.5 / (3.0 * semulate::apu::CPU_HZ as f64));
    let mut deadline = Instant::now();
    let mut rendered = 0;
    println!(
        "Arrows = D-pad | Z = A | X = B | Enter = Start | Right Shift = Select\nP/Space pause | N step | R reset | M mute | F11 fullscreen | F12 screenshot | Esc quit"
    );
    loop {
        while unsafe { SDL_PollEvent(&mut event) } > 0 {
            let kind = u32::from_ne_bytes(event.bytes[0..4].try_into().unwrap());
            match kind {
                0x100 => return Ok(()),
                0x200 => {
                    if event.bytes[12] == 13 {
                        focused = false;
                        keys = 0;
                        paused = true;
                        s.clear_audio();
                    } else if event.bytes[12] == 12 {
                        focused = true;
                    }
                }
                0x300 | 0x301 => {
                    let scan = i32::from_ne_bytes(event.bytes[16..20].try_into().unwrap());
                    let down = kind == 0x300;
                    if let Some(bit) = key_bit(scan) {
                        if down {
                            keys |= bit;
                        } else {
                            keys &= !bit;
                        }
                    }
                    if down && event.bytes[13] == 0 {
                        match scan {
                            41 => return Ok(()),
                            19 | 44 => {
                                paused = !paused;
                                s.clear_audio();
                                deadline = Instant::now();
                            }
                            17 if paused => {
                                nes.step()?;
                                nes.bus.apu.samples.clear();
                                println!("{}", nes.trace());
                            }
                            21 => {
                                nes.reset();
                                s.clear_audio();
                            }
                            16 => {
                                muted = !muted;
                                s.clear_audio();
                            }
                            68 => {
                                fullscreen = !fullscreen;
                                check(unsafe {
                                    SDL_SetWindowFullscreen(
                                        s.window,
                                        if fullscreen { 0x1001 } else { 0 },
                                    )
                                })?;
                            }
                            69 => {
                                let name = format!("semulate-frame-{}.ppm", nes.bus.ppu.frame);
                                super::screenshot(nes, Path::new(&name))?;
                                println!("Saved {name}");
                            }
                            _ => {}
                        }
                    }
                }
                0x1000 => {
                    let pointer = unsafe {
                        ptr::read_unaligned(event.bytes.as_ptr().add(8).cast::<*mut c_char>())
                    };
                    if !pointer.is_null() {
                        let dropped =
                            unsafe { CStr::from_ptr(pointer).to_string_lossy().into_owned() };
                        unsafe {
                            SDL_free(pointer.cast());
                        }
                        let dropped = PathBuf::from(dropped);
                        let loaded = super::read_rom(&dropped)
                            .and_then(|bytes| Nes::new(&bytes))
                            .and_then(|mut n| {
                                super::load_save(&mut n, &dropped)?;
                                Ok(n)
                            });
                        match loaded {
                            Ok(n) => {
                                super::save_ram(nes, path.as_deref())?;
                                *nes = n;
                                *path = Some(dropped);
                                keys = 0;
                                paused = false;
                                s.clear_audio();
                                deadline = Instant::now();
                            }
                            Err(e) => eprintln!("Could not load ROM: {e}"),
                        }
                    }
                }
                _ => {}
            }
        }
        nes.bus.controllers[0].buttons = if focused { keys | s.pad_buttons() } else { 0 };
        if !paused {
            nes.run_frame()?;
            if s.audio != 0 && !muted {
                unsafe {
                    if SDL_GetQueuedAudioSize(s.audio) > SAMPLE_RATE / 4 * 4 {
                        SDL_ClearQueuedAudio(s.audio);
                    }
                    check(SDL_QueueAudio(
                        s.audio,
                        nes.bus.apu.samples.as_ptr().cast(),
                        (nes.bus.apu.samples.len() * 4) as u32,
                    ))?;
                }
            }
            nes.bus.apu.samples.clear();
        }
        unsafe {
            check(SDL_UpdateTexture(
                s.texture,
                ptr::null(),
                nes.bus.ppu.pixels.as_ptr().cast(),
                1024,
            ))?;
            check(SDL_RenderClear(s.renderer))?;
            check(SDL_RenderCopy(
                s.renderer,
                s.texture,
                ptr::null(),
                ptr::null(),
            ))?;
            SDL_RenderPresent(s.renderer);
        }
        s.title(nes, paused, muted);
        rendered += 1;
        if max_frames.is_some_and(|n| rendered >= n) {
            return Ok(());
        }
        deadline += interval;
        let now = Instant::now();
        if deadline > now {
            std::thread::sleep(deadline - now);
        } else if now.duration_since(deadline) > interval * 4 {
            deadline = now;
        }
    }
}
