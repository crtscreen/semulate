//! bounded iNES / NES 2.0 loader for NTSC cartridge boards
pub use crate::mapper::Mirroring;
use crate::mapper::{CpuMapping, Mapper, PpuMapping};

#[derive(Clone)]
pub struct Cartridge {
    pub mapper: Mapper,
    pub prg: Vec<u8>,
    pub chr: Vec<u8>,
    pub chr_ram: bool,
    pub ram: Vec<u8>,
    pub mirroring: Mirroring,
    pub battery: bool,
    pub submapper: u8,
    pub nes2: bool,
}

fn ram_size(shift: u8) -> usize {
    if shift == 0 { 0 } else { 64usize << shift }
}
fn rom_size(low: u8, high: u8, unit: usize) -> Result<usize, String> {
    let size = if high == 15 {
        let exponent = low >> 2;
        1usize
            .checked_shl(exponent as u32)
            .and_then(|n| n.checked_mul(((low & 3) * 2 + 1) as usize))
    } else {
        Some((((high as usize) << 8) | low as usize) * unit)
    };
    size.filter(|&n| n <= 4 * 1024 * 1024).ok_or_else(|| {
        "ROM size is beyond the supported board capacity (maximum 4 MiB per chip).".into()
    })
}

impl Cartridge {
    pub fn load(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 16 || &bytes[..4] != b"NES\x1a" {
            return Err("Not an iNES ROM: expected a 16-byte NES header.".into());
        }
        let nes2 = match bytes[7] & 0x0c {
            0 => false,
            8 => true,
            _ => return Err("Unrecognised iNES header version.".into()),
        };
        if bytes[7] & 3 != 0 {
            return Err(
                "VS System, PlayChoice and extended console types are not supported.".into(),
            );
        }
        let number = ((bytes[6] >> 4) | (bytes[7] & 0xf0)) as u16
            | if nes2 {
                ((bytes[8] & 15) as u16) << 8
            } else {
                0
            };
        let submapper = if nes2 { bytes[8] >> 4 } else { 0 };
        let flags = bytes[6];
        if (nes2 && bytes[12] & 3 != 0 && bytes[12] & 3 != 2) || (!nes2 && bytes[9] & 1 != 0) {
            return Err("PAL and Dendy timing are not supported. Select an NTSC ROM.".into());
        }
        if nes2
            && (bytes[12] & 0xfc != 0 || bytes[13] != 0 || bytes[14] != 0 || bytes[15] & 0xc0 != 0)
        {
            return Err("Unsupported NES 2.0 reserved fields or miscellaneous ROMs.".into());
        }
        if nes2 && !matches!(bytes[15] & 0x3f, 0 | 1) {
            return Err("This NES 2.0 ROM requires an unsupported input device (only standard controllers are supported).".into());
        }
        let prg_len = rom_size(bytes[4], if nes2 { bytes[9] & 15 } else { 0 }, 16384)?;
        let chr_len = rom_size(bytes[5], if nes2 { bytes[9] >> 4 } else { 0 }, 8192)?;
        let (ram_len, battery, chr_ram_len) = if nes2 {
            let ram = ram_size(bytes[10] & 15);
            let nvram = ram_size(bytes[10] >> 4);
            if ram != 0 && nvram != 0 {
                return Err("Mixed volatile and battery-backed PRG RAM is not supported.".into());
            }
            if bytes[11] & 0xf0 != 0 {
                return Err("Battery-backed CHR RAM is not supported.".into());
            }
            if (nvram != 0) != (flags & 2 != 0) {
                return Err("NES 2.0 battery flag and PRG NVRAM size disagree.".into());
            }
            (ram + nvram, nvram != 0, ram_size(bytes[11] & 15))
        } else {
            let default_ram = match number {
                218 => 0,
                0 | 1 | 4 => 8192,
                _ if flags & 6 != 0 => 8192,
                _ => 0,
            };
            let ram = if bytes[8] != 0 {
                bytes[8] as usize * 8192
            } else {
                default_ram
            };
            (
                ram,
                flags & 2 != 0,
                if chr_len == 0 && number != 218 {
                    8192
                } else {
                    0
                },
            )
        };
        let mirroring = if number == 218 && flags & 8 != 0 {
            if flags & 1 == 0 {
                Mirroring::SingleScreenLower
            } else {
                Mirroring::SingleScreenUpper
            }
        } else if flags & 8 != 0 {
            Mirroring::FourScreen
        } else if flags & 1 != 0 {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        };
        let mapper = Mapper::from_header(number, submapper, flags, mirroring, chr_len == 0)?;
        if !prg_len.is_power_of_two() || prg_len < 16384 {
            return Err("These boards require a power-of-two PRG ROM of at least 16 KiB.".into());
        }
        if chr_len != 0 && (!chr_len.is_power_of_two() || chr_len < 8192) {
            return Err("These boards require a power-of-two CHR ROM of at least 8 KiB.".into());
        }
        if chr_len != 0 && chr_ram_len != 0 {
            return Err("Boards combining CHR ROM and CHR RAM are not supported.".into());
        }
        if number != 218 && chr_len == 0 && chr_ram_len != 8192 {
            return Err("This board requires 8 KiB CHR RAM when there is no CHR ROM.".into());
        }
        let (max_prg, max_chr, max_ram) = match number {
            0 => (32768, 8192, 8192),
            1 => (
                if chr_len == 0 { 524288 } else { 262144 },
                131072,
                if chr_len == 0 { 32768 } else { 8192 },
            ),
            2 => (4194304, 0, 8192),
            3 => (32768, 2097152, 8192),
            4 => (524288, 262144, 8192),
            7 => (262144, 0, 8192),
            218 => (32768, 0, 0),
            _ => unreachable!(),
        };
        if prg_len > max_prg || chr_len > max_chr || ram_len > max_ram {
            return Err(format!(
                "Mapper {number}: unsupported memory sizes (PRG {prg_len}, CHR {chr_len}, RAM {ram_len} bytes)."
            ));
        }
        if ram_len != 0 && (ram_len < 8192 || !ram_len.is_power_of_two()) {
            return Err(
                "Supported PRG RAM sizes are 8 KiB, or 16/32 KiB on MMC1 CHR-RAM boards.".into(),
            );
        }
        if (number == 7 && prg_len < 32768) || (number == 1 && submapper == 5 && prg_len != 32768) {
            return Err("This board requires at least one complete 32 KiB PRG bank (exactly 32 KiB for SEROM).".into());
        }
        if matches!(number, 1 | 2 | 7) && flags & 8 != 0 {
            return Err(format!(
                "Four-screen VRAM is not supported on mapper {number}."
            ));
        }
        if number == 218 && (chr_ram_len != 0 || flags & 6 != 0) {
            return Err(
                "Mapper 218 uses shared CIRAM and has no cartridge RAM, trainer or battery save."
                    .into(),
            );
        }
        let trainer = flags & 4 != 0;
        if trainer && ram_len < 8192 {
            return Err("A trainer requires PRG RAM at $7000.".into());
        }
        let start = 16 + if trainer { 512 } else { 0 };
        if bytes.len() < start + prg_len + chr_len {
            return Err("ROM is truncated: data is shorter than its header declares.".into());
        }
        let mut ram = vec![0; ram_len];
        if trainer {
            ram[0x1000..0x1200].copy_from_slice(&bytes[16..528]);
        }
        Ok(Self {
            mapper,
            prg: bytes[start..start + prg_len].to_vec(),
            chr: if chr_len == 0 {
                vec![0; chr_ram_len]
            } else {
                bytes[start + prg_len..start + prg_len + chr_len].to_vec()
            },
            chr_ram: chr_ram_len != 0,
            ram,
            mirroring,
            battery,
            submapper,
            nes2,
        })
    }
    pub fn cpu_read(&self, address: u16) -> Option<u8> {
        self.mapper
            .cpu_mapping(address, self.prg.len(), self.ram.len())
            .map(|target| match target {
                CpuMapping::Prg(index) => self.prg[index],
                CpuMapping::Ram(index) => self.ram[index],
            })
    }
    pub fn cpu_write(&mut self, address: u16, value: u8, cycle: u64) {
        if self.mapper.ram_writable() {
            if let Some(CpuMapping::Ram(index)) =
                self.mapper
                    .cpu_mapping(address, self.prg.len(), self.ram.len())
            {
                self.ram[index] = value;
            }
        }
        let value = if address >= 0x8000 && self.mapper.bus_conflicts() {
            value & self.cpu_read(address).unwrap_or(0xff)
        } else {
            value
        };
        self.mapper.cpu_write(address, value, cycle);
    }
    pub(crate) fn ppu_read(&self, address: u16, vram: &[u8; 4096]) -> u8 {
        match self
            .mapper
            .ppu_mapping(address, self.mirroring, self.chr.len())
        {
            PpuMapping::Chr(index) => self.chr[index],
            PpuMapping::Vram(index) => vram[index],
        }
    }
    pub(crate) fn ppu_write(&mut self, address: u16, value: u8, vram: &mut [u8; 4096]) {
        match self
            .mapper
            .ppu_mapping(address, self.mirroring, self.chr.len())
        {
            PpuMapping::Chr(index) if self.chr_ram => self.chr[index] = value,
            PpuMapping::Chr(_) => {}
            PpuMapping::Vram(index) => vram[index] = value,
        }
    }
}
