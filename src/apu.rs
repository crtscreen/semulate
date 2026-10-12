//! 5 channel NTSC APU with envelopes, length counters, sweep, DMC and PCM output
pub const SAMPLE_RATE: u32 = 48_000;
pub const CPU_HZ: u32 = 1_789_773;
const LENGTH: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14, 12, 16, 24, 18, 48, 20, 96, 22,
    192, 24, 72, 26, 16, 28, 32, 30,
];
const NOISE: [u16; 16] = [
    4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068,
];
const DMC: [u16; 16] = [
    428, 380, 340, 320, 286, 254, 226, 214, 190, 160, 142, 128, 106, 84, 72, 54,
];
const DUTY: [[u8; 8]; 4] = [
    [0, 1, 0, 0, 0, 0, 0, 0],
    [0, 1, 1, 0, 0, 0, 0, 0],
    [0, 1, 1, 1, 1, 0, 0, 0],
    [1, 0, 0, 1, 1, 1, 1, 1],
];
#[derive(Default)]
struct Envelope {
    reg: u8,
    start: bool,
    divider: u8,
    decay: u8,
}
impl Envelope {
    fn clock(&mut self) {
        if self.start {
            self.start = false;
            self.decay = 15;
            self.divider = self.reg & 15;
        } else if self.divider > 0 {
            self.divider -= 1;
        } else {
            self.divider = self.reg & 15;
            if self.decay > 0 {
                self.decay -= 1;
            } else if self.reg & 32 != 0 {
                self.decay = 15;
            }
        }
    }
    fn volume(&self) -> u8 {
        if self.reg & 16 != 0 {
            self.reg & 15
        } else {
            self.decay
        }
    }
}
#[derive(Default)]
struct Pulse {
    env: Envelope,
    period: u16,
    timer: u16,
    step: u8,
    length: u8,
    enabled: bool,
    sweep: u8,
    sweep_div: u8,
    sweep_reload: bool,
}
impl Pulse {
    fn target(&self, first: bool) -> i32 {
        let delta = (self.period >> (self.sweep & 7)) as i32;
        if self.sweep & 8 != 0 {
            self.period as i32 - delta - if first { 1 } else { 0 }
        } else {
            self.period as i32 + delta
        }
    }
    fn output(&self, first: bool) -> u8 {
        if self.length == 0
            || self.period < 8
            || self.target(first) > 0x7ff
            || DUTY[(self.env.reg >> 6) as usize][self.step as usize] == 0
        {
            0
        } else {
            self.env.volume()
        }
    }
    fn tick(&mut self) {
        if self.timer == 0 {
            self.timer = self.period;
            self.step = (self.step + 1) & 7;
        } else {
            self.timer -= 1;
        }
    }
    fn half(&mut self, first: bool) {
        if self.env.reg & 32 == 0 && self.length > 0 {
            self.length -= 1;
        }
        let target = self.target(first);
        if self.sweep_div == 0
            && self.sweep & 128 != 0
            && self.sweep & 7 != 0
            && self.period >= 8
            && (0..=0x7ff).contains(&target)
        {
            self.period = target as u16;
        }
        if self.sweep_div == 0 || self.sweep_reload {
            self.sweep_div = (self.sweep >> 4) & 7;
            self.sweep_reload = false;
        } else {
            self.sweep_div -= 1;
        }
    }
}
pub struct Apu {
    pulses: [Pulse; 2],
    tri_ctrl: u8,
    tri_length: u8,
    tri_linear: u8,
    tri_reload: bool,
    tri_period: u16,
    tri_timer: u16,
    tri_step: u8,
    tri_enabled: bool,
    noise_env: Envelope,
    noise_length: u8,
    noise_reg: u8,
    noise_timer: u16,
    noise_shift: u16,
    noise_enabled: bool,
    dmc_ctrl: u8,
    dmc_level: u8,
    dmc_start: u8,
    dmc_len: u8,
    dmc_address: u16,
    dmc_remaining: u16,
    dmc_buffer: Option<u8>,
    dmc_shift: u8,
    dmc_bits: u8,
    dmc_silent: bool,
    dmc_timer: u16,
    pub dmc_irq: bool,
    frame_cycle: u32,
    five_step: bool,
    irq_inhibit: bool,
    pub frame_irq: bool,
    reset_delay: u8,
    phase: bool,
    sample_phase: u32,
    integral: f32,
    integral_n: u32,
    last_in: f32,
    hp: f32,
    lp: f32,
    pub samples: Vec<f32>,
}
impl Default for Apu {
    fn default() -> Self {
        Self {
            pulses: [Pulse::default(), Pulse::default()],
            tri_ctrl: 0,
            tri_length: 0,
            tri_linear: 0,
            tri_reload: false,
            tri_period: 0,
            tri_timer: 0,
            tri_step: 0,
            tri_enabled: false,
            noise_env: Envelope::default(),
            noise_length: 0,
            noise_reg: 0,
            noise_timer: 0,
            noise_shift: 1,
            noise_enabled: false,
            dmc_ctrl: 0,
            dmc_level: 0,
            dmc_start: 0,
            dmc_len: 0,
            dmc_address: 0xc000,
            dmc_remaining: 0,
            dmc_buffer: None,
            dmc_shift: 0,
            dmc_bits: 8,
            dmc_silent: true,
            dmc_timer: 0,
            dmc_irq: false,
            frame_cycle: 0,
            five_step: false,
            irq_inhibit: false,
            frame_irq: false,
            reset_delay: 0,
            phase: false,
            sample_phase: 0,
            integral: 0.0,
            integral_n: 0,
            last_in: 0.0,
            hp: 0.0,
            lp: 0.0,
            samples: Vec::with_capacity(2048),
        }
    }
}
impl Apu {
    fn quarter(&mut self) {
        for p in &mut self.pulses {
            p.env.clock();
        }
        self.noise_env.clock();
        if self.tri_reload {
            self.tri_linear = self.tri_ctrl & 127;
        } else if self.tri_linear > 0 {
            self.tri_linear -= 1;
        }
        if self.tri_ctrl & 128 == 0 {
            self.tri_reload = false;
        }
    }
    fn half(&mut self) {
        self.pulses[0].half(true);
        self.pulses[1].half(false);
        if self.tri_ctrl & 128 == 0 && self.tri_length > 0 {
            self.tri_length -= 1;
        }
        if self.noise_env.reg & 32 == 0 && self.noise_length > 0 {
            self.noise_length -= 1;
        }
    }
    fn restart_dmc(&mut self) {
        self.dmc_address = 0xc000 + (self.dmc_start as u16) * 64;
        self.dmc_remaining = (self.dmc_len as u16) * 16 + 1;
    }
    pub fn dmc_request(&self) -> Option<u16> {
        if self.dmc_buffer.is_none() && self.dmc_remaining > 0 {
            Some(self.dmc_address)
        } else {
            None
        }
    }
    pub fn dmc_supply(&mut self, value: u8) {
        self.dmc_buffer = Some(value);
        self.dmc_address = if self.dmc_address == 0xffff {
            0x8000
        } else {
            self.dmc_address + 1
        };
        self.dmc_remaining -= 1;
        if self.dmc_remaining == 0 {
            if self.dmc_ctrl & 64 != 0 {
                self.restart_dmc();
            } else if self.dmc_ctrl & 128 != 0 {
                self.dmc_irq = true;
            }
        }
    }
    pub fn status(&mut self) -> u8 {
        let v = (self.pulses[0].length > 0) as u8
            | (((self.pulses[1].length > 0) as u8) << 1)
            | (((self.tri_length > 0) as u8) << 2)
            | (((self.noise_length > 0) as u8) << 3)
            | (((self.dmc_remaining > 0) as u8) << 4)
            | ((self.frame_irq as u8) << 6)
            | ((self.dmc_irq as u8) << 7);
        self.frame_irq = false;
        v
    }
    pub fn write(&mut self, a: u16, v: u8) {
        match a {
            0x4000..=0x4007 => {
                let p = &mut self.pulses[((a - 0x4000) / 4) as usize];
                match a & 3 {
                    0 => p.env.reg = v,
                    1 => {
                        p.sweep = v;
                        p.sweep_reload = true;
                    }
                    2 => p.period = (p.period & 0x700) | v as u16,
                    _ => {
                        p.period = (p.period & 255) | ((v as u16 & 7) << 8);
                        if p.enabled {
                            p.length = LENGTH[(v >> 3) as usize];
                        }
                        p.step = 0;
                        p.env.start = true;
                    }
                }
            }
            0x4008 => self.tri_ctrl = v,
            0x400a => self.tri_period = (self.tri_period & 0x700) | v as u16,
            0x400b => {
                self.tri_period = (self.tri_period & 255) | ((v as u16 & 7) << 8);
                if self.tri_enabled {
                    self.tri_length = LENGTH[(v >> 3) as usize];
                }
                self.tri_reload = true;
            }
            0x400c => self.noise_env.reg = v,
            0x400e => self.noise_reg = v,
            0x400f => {
                if self.noise_enabled {
                    self.noise_length = LENGTH[(v >> 3) as usize];
                }
                self.noise_env.start = true;
            }
            0x4010 => {
                self.dmc_ctrl = v;
                if v & 128 == 0 {
                    self.dmc_irq = false;
                }
            }
            0x4011 => self.dmc_level = v & 127,
            0x4012 => self.dmc_start = v,
            0x4013 => self.dmc_len = v,
            0x4015 => {
                for n in 0..2 {
                    self.pulses[n].enabled = v & (1 << n) != 0;
                    if !self.pulses[n].enabled {
                        self.pulses[n].length = 0;
                    }
                }
                self.tri_enabled = v & 4 != 0;
                if !self.tri_enabled {
                    self.tri_length = 0;
                }
                self.noise_enabled = v & 8 != 0;
                if !self.noise_enabled {
                    self.noise_length = 0;
                }
                self.dmc_irq = false;
                if v & 16 == 0 {
                    self.dmc_remaining = 0;
                } else if self.dmc_remaining == 0 {
                    self.restart_dmc();
                }
            }
            0x4017 => {
                self.five_step = v & 128 != 0;
                self.irq_inhibit = v & 64 != 0;
                if self.irq_inhibit {
                    self.frame_irq = false;
                }
                self.reset_delay = if self.phase { 3 } else { 4 };
            }
            _ => {}
        }
    }
    pub fn tick(&mut self) {
        self.phase = !self.phase;
        if self.phase {
            for p in &mut self.pulses {
                p.tick();
            }
        }
        if self.tri_timer == 0 {
            self.tri_timer = self.tri_period;
            if self.tri_length > 0 && self.tri_linear > 0 && self.tri_period > 1 {
                self.tri_step = (self.tri_step + 1) & 31;
            }
        } else {
            self.tri_timer -= 1;
        }
        if self.noise_timer == 0 {
            self.noise_timer = NOISE[(self.noise_reg & 15) as usize] - 1;
            let tap = if self.noise_reg & 128 != 0 { 6 } else { 1 };
            let feedback = (self.noise_shift ^ (self.noise_shift >> tap)) & 1;
            self.noise_shift = (self.noise_shift >> 1) | (feedback << 14);
        } else {
            self.noise_timer -= 1;
        }
        if self.dmc_timer == 0 {
            self.dmc_timer = DMC[(self.dmc_ctrl & 15) as usize] - 1;
            if !self.dmc_silent {
                if self.dmc_shift & 1 != 0 {
                    if self.dmc_level <= 125 {
                        self.dmc_level += 2;
                    }
                } else if self.dmc_level >= 2 {
                    self.dmc_level -= 2;
                }
            }
            self.dmc_shift >>= 1;
            self.dmc_bits -= 1;
            if self.dmc_bits == 0 {
                self.dmc_bits = 8;
                if let Some(v) = self.dmc_buffer.take() {
                    self.dmc_shift = v;
                    self.dmc_silent = false;
                } else {
                    self.dmc_silent = true;
                }
            }
        } else {
            self.dmc_timer -= 1;
        }
        self.frame_cycle += 1;
        match self.frame_cycle {
            7457 | 22371 => self.quarter(),
            14913 => {
                self.quarter();
                self.half();
            }
            29829 if !self.five_step => {
                self.quarter();
                self.half();
            }
            37281 if self.five_step => {
                self.quarter();
                self.half();
            }
            _ => {}
        }
        if !self.five_step && (29828..=29830).contains(&self.frame_cycle) && !self.irq_inhibit {
            self.frame_irq = true;
        }
        if self.frame_cycle == if self.five_step { 37282 } else { 29830 } {
            self.frame_cycle = 0;
        }
        if self.reset_delay > 0 {
            self.reset_delay -= 1;
            if self.reset_delay == 0 {
                self.frame_cycle = 0;
                if self.five_step {
                    self.quarter();
                    self.half();
                }
            }
        }
        let pulse = (self.pulses[0].output(true) + self.pulses[1].output(false)) as f32;
        let triangle = if self.tri_step < 16 {
            15 - self.tri_step
        } else {
            self.tri_step - 16
        } as f32;
        let noise = if self.noise_length > 0 && self.noise_shift & 1 == 0 {
            self.noise_env.volume()
        } else {
            0
        } as f32;
        let tnd = triangle / 8227.0 + noise / 12241.0 + self.dmc_level as f32 / 22638.0;
        let mixed = if pulse > 0.0 {
            95.88 / (8128.0 / pulse + 100.)
        } else {
            0.0
        } + if tnd > 0.0 {
            159.79 / (1.0 / tnd + 100.0)
        } else {
            0.0
        };
        self.integral += mixed;
        self.integral_n += 1;
        self.sample_phase += SAMPLE_RATE;
        if self.sample_phase >= CPU_HZ {
            self.sample_phase -= CPU_HZ;
            let input = self.integral / self.integral_n as f32;
            self.integral = 0.0;
            self.integral_n = 0;
            self.hp = 0.9883 * (self.hp + input - self.last_in);
            self.last_in = input;
            self.lp += 0.84 * (self.hp - self.lp);
            self.samples.push((self.lp * 1.7).clamp(-1.0, 1.0));
        }
    }
}
