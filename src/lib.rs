//! emulation core
pub mod apu;
pub mod cartridge;
pub mod controller;
pub mod cpu;
pub mod mapper;
pub mod ppu;
#[cfg(feature = "web")]
pub mod web;
use cartridge::Cartridge;
use cpu::{Cpu, CpuBus};

pub struct Bus {
    pub ram: [u8; 2048],
    pub cart: Cartridge,
    pub ppu: ppu::Ppu,
    pub apu: apu::Apu,
    pub controllers: [controller::Controller; 2],
    pub cycles: u64,
    open_bus: u8,
    dma: Option<u8>,
    dmc_stalls: u32,
    nmi_latched: bool,
    nmi_ready: bool,
    irq_ready: bool,
    irq_sampled: bool,
}
impl Bus {
    pub fn new(cart: Cartridge) -> Self {
        Self {
            ram: [0; 2048],
            cart,
            ppu: ppu::Ppu::default(),
            apu: apu::Apu::default(),
            controllers: Default::default(),
            cycles: 0,
            open_bus: 0,
            dma: None,
            dmc_stalls: 0,
            nmi_latched: false,
            nmi_ready: false,
            irq_ready: false,
            irq_sampled: false,
        }
    }
    fn clock(&mut self) {
        self.nmi_ready = self.nmi_latched;
        self.irq_ready = self.irq_sampled;
        self.cycles += 1;
        self.apu.tick();
        for phase in 0..3 {
            self.ppu.tick(&mut self.cart);
            if phase == 0 {
                self.irq_sampled =
                    self.apu.frame_irq || self.apu.dmc_irq || self.cart.mapper.irq_pending();
            }
        }
        self.cart.mapper.clock_cpu();
        self.nmi_latched |= std::mem::take(&mut self.ppu.nmi_pending);
        if let Some(a) = self.apu.dmc_request() {
            let value = self.cart.cpu_read(a).unwrap_or(self.open_bus);
            self.apu.dmc_supply(value);
            self.dmc_stalls += 4;
        }
    }
    fn raw_read(&mut self, a: u16) -> u8 {
        match a {
            0..=0x1fff => self.ram[a as usize & 0x7ff],
            0x2000..=0x3fff => self.ppu.read_register(&mut self.cart, a),
            0x4015 => self.apu.status() | (self.open_bus & 32),
            0x4016 | 0x4017 => {
                (self.open_bus & 0xe0) | self.controllers[(a - 0x4016) as usize].read()
            }
            0x4020..=0xffff => self.cart.cpu_read(a).unwrap_or(self.open_bus),
            _ => self.open_bus,
        }
    }
    pub fn peek(&self, a: u16) -> u8 {
        match a {
            0..=0x1fff => self.ram[a as usize & 0x7ff],
            0x4020..=0xffff => self.cart.cpu_read(a).unwrap_or(0),
            _ => 0,
        }
    }
    fn stalls(&mut self, pc: u16) {
        if let Some(page) = self.dma.take() {
            let odd = self.cycles & 1 != 0;
            self.read(pc);
            if odd {
                self.read(pc);
            }
            for i in 0..256u16 {
                let value = self.read(((page as u16) << 8) | i);
                self.ppu.oam[self.ppu.oam_addr as usize] = value;
                self.ppu.oam_addr = self.ppu.oam_addr.wrapping_add(1);
                self.clock();
            }
        }
        while self.dmc_stalls > 0 {
            self.dmc_stalls -= 1;
            self.clock();
        }
    }
}
impl CpuBus for Bus {
    fn read(&mut self, a: u16) -> u8 {
        let v = self.raw_read(a);
        self.open_bus = v;
        self.clock();
        v
    }
    fn write(&mut self, a: u16, v: u8) {
        self.open_bus = v;
        match a {
            0..=0x1fff => self.ram[a as usize & 0x7ff] = v,
            0x2000..=0x3fff => self.ppu.write_register(&mut self.cart, a, v),
            0x4000..=0x4013 | 0x4015 | 0x4017 => self.apu.write(a, v),
            0x4014 => self.dma = Some(v),
            0x4016 => {
                for c in &mut self.controllers {
                    c.write(v)
                }
            }
            _ => {}
        }
        self.cart.cpu_write(a, v, self.cycles);
        self.clock();
    }
}
pub struct Nes {
    pub cpu: Cpu,
    pub bus: Bus,
}
impl Nes {
    pub fn new(rom: &[u8]) -> Result<Self, String> {
        let mut nes = Self {
            cpu: Cpu::default(),
            bus: Bus::new(Cartridge::load(rom)?),
        };
        nes.cpu.reset(&mut nes.bus);
        Ok(nes)
    }
    pub fn reset(&mut self) {
        self.cpu.reset(&mut self.bus);
    }
    pub fn step(&mut self) -> Result<(), String> {
        self.bus.stalls(self.cpu.pc);
        if self.bus.nmi_ready {
            self.bus.nmi_ready = false;
            self.bus.nmi_latched = false;
            self.cpu.interrupt(&mut self.bus, true);
        } else if self.cpu.can_irq() && self.bus.irq_ready {
            self.cpu.interrupt(&mut self.bus, false);
        } else {
            self.cpu.step(&mut self.bus)?;
        }
        Ok(())
    }
    pub fn run_frame(&mut self) -> Result<(), String> {
        let frame = self.bus.ppu.frame;
        while self.bus.ppu.frame == frame {
            self.step()?;
        }
        Ok(())
    }
    pub fn trace(&self) -> String {
        format!(
            "{:04X}  {:02X} A:{:02X} X:{:02X} Y:{:02X} P:{:02X} SP:{:02X} CYC:{}",
            self.cpu.pc,
            self.bus.peek(self.cpu.pc),
            self.cpu.a,
            self.cpu.x,
            self.cpu.y,
            self.cpu.p,
            self.cpu.sp,
            self.bus.cycles
        )
    }
}
