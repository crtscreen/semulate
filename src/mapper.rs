//! Cartridge banking, nametable wiring and mapper IRQs.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mirroring {
    Horizontal,
    Vertical,
    FourScreen,
    SingleScreenLower,
    SingleScreenUpper,
}

#[derive(Clone, Debug)]
pub enum Mapper {
    Nrom,
    Mmc1(Mmc1),
    Uxrom { bank: u8, conflicts: bool },
    Cnrom { bank: u8, conflicts: bool },
    Mmc3(Mmc3),
    Axrom { bank: u8, conflicts: bool },
    Mapper218 { ciram_a10: u8 },
}

#[derive(Clone, Debug)]
pub struct Mmc1 {
    shift: u8,
    control: u8,
    chr: [u8; 2],
    prg: u8,
    last_write: Option<u64>,
    a12: bool,
    fixed_prg: bool,
    chr_ram: bool,
}
impl Mmc1 {
    fn output_chr(&self) -> usize {
        self.chr[usize::from(self.control & 0x10 != 0 && self.a12)] as usize
    }
    fn write(&mut self, address: u16, value: u8, cycle: u64) {
        let consecutive = self.last_write.is_some_and(|last| cycle == last + 1);
        self.last_write = Some(cycle);
        if consecutive || address < 0x8000 {
            return;
        }
        if value & 0x80 != 0 {
            self.shift = 0x10;
            self.control |= 0x0c;
            return;
        }
        let full = self.shift & 1 != 0;
        self.shift = (self.shift >> 1) | ((value & 1) << 4);
        if full {
            match (address >> 13) & 3 {
                0 => self.control = self.shift,
                1 => self.chr[0] = self.shift,
                2 => self.chr[1] = self.shift,
                3 => self.prg = self.shift,
                _ => unreachable!(),
            }
            self.shift = 0x10;
        }
    }
}

#[derive(Clone, Debug)]
pub struct Mmc3 {
    select: u8,
    banks: [u8; 8],
    mirroring: Mirroring,
    fixed_mirroring: bool,
    ram_control: u8,
    irq_latch: u8,
    irq_counter: u8,
    irq_reload: bool,
    irq_enable: bool,
    irq_pending: bool,
    alternate_irq: bool,
    a12: bool,
    low_m2_edges: u8,
}
impl Mmc3 {
    fn write(&mut self, address: u16, value: u8) {
        match address & 0xe001 {
            0x8000 => self.select = value,
            0x8001 => self.banks[(self.select & 7) as usize] = value,
            0xa000 if !self.fixed_mirroring => {
                self.mirroring = if value & 1 == 0 {
                    Mirroring::Vertical
                } else {
                    Mirroring::Horizontal
                };
            }
            0xa001 => self.ram_control = value,
            0xc000 => self.irq_latch = value,
            0xc001 => self.irq_reload = true,
            0xe000 => {
                self.irq_enable = false;
                self.irq_pending = false;
            }
            0xe001 => self.irq_enable = true,
            _ => {}
        }
    }
    fn clock_counter(&mut self) {
        let was_active = self.irq_counter != 0 || self.irq_reload;
        if self.irq_counter == 0 || self.irq_reload {
            self.irq_counter = self.irq_latch;
        } else {
            self.irq_counter -= 1;
        }
        self.irq_reload = false;
        if self.irq_counter == 0 && self.irq_enable && (!self.alternate_irq || was_active) {
            self.irq_pending = true;
        }
    }
}

pub(crate) enum CpuMapping {
    Prg(usize),
    Ram(usize),
}
pub(crate) enum PpuMapping {
    Chr(usize),
    Vram(usize),
}

impl Mapper {
    pub(crate) fn from_header(
        number: u16,
        submapper: u8,
        flags6: u8,
        mirroring: Mirroring,
        chr_ram: bool,
    ) -> Result<Self, String> {
        let conflicts = submapper == 2 || (submapper == 0 && number != 7);
        match (number, submapper) {
            (0, 0) => Ok(Self::Nrom),
            (1, 0 | 5) => Ok(Self::Mmc1(Mmc1 {
                shift: 0x10,
                control: 0x0c,
                chr: [0; 2],
                prg: 0,
                last_write: None,
                a12: false,
                fixed_prg: submapper == 5,
                chr_ram,
            })),
            (2, 0..=2) => Ok(Self::Uxrom { bank: 0, conflicts }),
            (3, 0..=2) => Ok(Self::Cnrom { bank: 0, conflicts }),
            (4, 0 | 2 | 4) => Ok(Self::Mmc3(Mmc3 {
                select: 0,
                banks: [0, 2, 4, 5, 6, 7, 0, 1],
                mirroring,
                fixed_mirroring: submapper == 2 || mirroring == Mirroring::FourScreen,
                ram_control: 0x80,
                irq_latch: 0,
                irq_counter: 0,
                irq_reload: false,
                irq_enable: false,
                irq_pending: false,
                alternate_irq: submapper == 4,
                a12: false,
                low_m2_edges: 0,
            })),
            (7, 0..=2) => Ok(Self::Axrom { bank: 0, conflicts }),
            (218, 0) => Ok(Self::Mapper218 {
                ciram_a10: match flags6 & 9 {
                    1 => 10,
                    0 => 11,
                    8 => 12,
                    9 => 13,
                    _ => unreachable!(),
                },
            }),
            (0 | 1 | 2 | 3 | 4 | 7 | 218, _) => Err(format!(
                "Mapper {number} submapper {submapper} is not supported. See README.md for board variants."
            )),
            _ => Err(format!(
                "Mapper {number} is not supported. Supported mappers: 0, 1, 2, 3, 4, 7, 218."
            )),
        }
    }
    pub fn number(&self) -> u16 {
        match self {
            Self::Nrom => 0,
            Self::Mmc1(_) => 1,
            Self::Uxrom { .. } => 2,
            Self::Cnrom { .. } => 3,
            Self::Mmc3(_) => 4,
            Self::Axrom { .. } => 7,
            Self::Mapper218 { .. } => 218,
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            Self::Nrom => "NROM (0)",
            Self::Mmc1(_) => "MMC1 (1)",
            Self::Uxrom { .. } => "UxROM (2)",
            Self::Cnrom { .. } => "CNROM (3)",
            Self::Mmc3(_) => "MMC3 (4)",
            Self::Axrom { .. } => "AxROM (7)",
            Self::Mapper218 { .. } => "Mapper 218",
        }
    }
    pub(crate) fn bus_conflicts(&self) -> bool {
        matches!(
            self,
            Self::Uxrom {
                conflicts: true,
                ..
            } | Self::Cnrom {
                conflicts: true,
                ..
            } | Self::Axrom {
                conflicts: true,
                ..
            }
        )
    }
    pub(crate) fn cpu_mapping(
        &self,
        address: u16,
        prg_len: usize,
        ram_len: usize,
    ) -> Option<CpuMapping> {
        let a = address as usize;
        if (0x6000..0x8000).contains(&a) && ram_len != 0 {
            let bank = match self {
                Self::Mapper218 { .. } => return None,
                Self::Mmc3(m) if m.ram_control & 0x80 == 0 => return None,
                Self::Mmc1(m) => {
                    if m.prg & 0x10 != 0 {
                        return None;
                    }
                    let chr = m.output_chr();
                    if m.chr_ram && prg_len <= 262144 && ram_len == 8192 && chr & 0x10 != 0 {
                        return None;
                    }
                    if m.chr_ram {
                        match ram_len {
                            16384 => (chr >> 3) & 1,
                            32768 => (chr >> 2) & 3,
                            _ => 0,
                        }
                    } else {
                        0
                    }
                }
                _ => 0,
            };
            return Some(CpuMapping::Ram((bank * 8192 + a - 0x6000) % ram_len));
        }
        if a < 0x8000 {
            return None;
        }
        let offset = a - 0x8000;
        let index = match self {
            Self::Uxrom { bank, .. } => {
                let bank = if a < 0xc000 {
                    *bank as usize
                } else {
                    prg_len / 16384 - 1
                };
                bank * 16384 + (a & 0x3fff)
            }
            Self::Axrom { bank, .. } => (*bank as usize & 7) * 32768 + offset,
            Self::Mmc1(m) if !m.fixed_prg => {
                let outer = if m.chr_ram && prg_len > 262144 {
                    (m.output_chr() & 0x10) * 16384
                } else {
                    0
                };
                let bank = (m.prg & 15) as usize;
                let inner = match (m.control >> 2) & 3 {
                    0 | 1 => (bank & !1) * 16384 + offset,
                    2 => (if a < 0xc000 { 0 } else { bank }) * 16384 + (a & 0x3fff),
                    3 => (if a < 0xc000 { bank } else { 15 }) * 16384 + (a & 0x3fff),
                    _ => unreachable!(),
                };
                outer + inner
            }
            Self::Mmc3(m) => {
                let slot = (a - 0x8000) / 8192;
                let last = prg_len / 8192 - 1;
                let bank = match (slot, m.select & 0x40 != 0) {
                    (0, false) | (2, true) => (m.banks[6] & 0x3f) as usize,
                    (1, _) => (m.banks[7] & 0x3f) as usize,
                    (0, true) | (2, false) => last - 1,
                    _ => last,
                };
                bank * 8192 + (a & 8191)
            }
            _ => offset,
        };
        Some(CpuMapping::Prg(index % prg_len))
    }
    pub(crate) fn ram_writable(&self) -> bool {
        !matches!(self, Self::Mmc3(m) if m.ram_control & 0x40 != 0)
    }
    pub(crate) fn cpu_write(&mut self, address: u16, value: u8, cycle: u64) {
        if let Self::Mmc1(m) = self {
            m.write(address, value, cycle);
        } else if address >= 0x8000 {
            match self {
                Self::Uxrom { bank, .. } | Self::Cnrom { bank, .. } | Self::Axrom { bank, .. } => {
                    *bank = value
                }
                Self::Mmc3(m) => m.write(address, value),
                _ => {}
            }
        }
    }
    pub fn mirroring(&self, header: Mirroring) -> Mirroring {
        match self {
            Self::Mmc1(m) => match m.control & 3 {
                0 => Mirroring::SingleScreenLower,
                1 => Mirroring::SingleScreenUpper,
                2 => Mirroring::Vertical,
                _ => Mirroring::Horizontal,
            },
            Self::Mmc3(m) => m.mirroring,
            Self::Axrom { bank, .. } => {
                if bank & 0x10 == 0 {
                    Mirroring::SingleScreenLower
                } else {
                    Mirroring::SingleScreenUpper
                }
            }
            _ => header,
        }
    }
    pub(crate) fn ppu_mapping(
        &self,
        address: u16,
        header_mirroring: Mirroring,
        chr_len: usize,
    ) -> PpuMapping {
        debug_assert!(address < 0x3f00, "the PPU owns palette accesses");
        if let Self::Mapper218 { ciram_a10 } = self {
            let bank = ((address >> ciram_a10) & 1) as usize;
            return PpuMapping::Vram(bank * 1024 + (address as usize & 1023));
        }
        let a = address as usize;
        if a < 0x2000 {
            let index = match self {
                Self::Cnrom { bank, .. } => *bank as usize * 8192 + a,
                Self::Mmc1(m) => {
                    if m.control & 0x10 == 0 {
                        (m.chr[0] as usize & !1) * 4096 + a
                    } else {
                        m.chr[a / 4096] as usize * 4096 + (a & 4095)
                    }
                }
                Self::Mmc3(m) => {
                    let slot = (a / 1024) ^ if m.select & 0x80 != 0 { 4 } else { 0 };
                    let bank = match slot {
                        0..=3 => (m.banks[slot / 2] as usize & !1) + (slot & 1),
                        _ => m.banks[slot - 2] as usize,
                    };
                    bank * 1024 + (a & 1023)
                }
                _ => a,
            };
            return PpuMapping::Chr(index % chr_len);
        }
        let i = (a - 0x2000) & 0xfff;
        let table = i / 1024;
        let bank = match self.mirroring(header_mirroring) {
            Mirroring::Vertical => table & 1,
            Mirroring::Horizontal => table >> 1,
            Mirroring::FourScreen => table,
            Mirroring::SingleScreenLower => 0,
            Mirroring::SingleScreenUpper => 1,
        };
        PpuMapping::Vram(bank * 1024 + (i & 1023))
    }
    pub fn observe_ppu_address(&mut self, address: u16) {
        let a12 = address & 0x1000 != 0;
        match self {
            Self::Mmc1(m) => m.a12 = a12,
            Self::Mmc3(m) => {
                if a12 && !m.a12 && m.low_m2_edges >= 3 {
                    m.clock_counter();
                }
                if a12 {
                    m.low_m2_edges = 0;
                }
                m.a12 = a12;
            }
            _ => {}
        }
    }
    pub fn clock_cpu(&mut self) {
        if let Self::Mmc3(m) = self {
            if !m.a12 {
                m.low_m2_edges = (m.low_m2_edges + 1).min(3);
            }
        }
    }
    pub fn irq_pending(&self) -> bool {
        matches!(self, Self::Mmc3(m) if m.irq_pending)
    }
}
