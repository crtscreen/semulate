//! Ricoh 2A03 CPU. every memory access (including dummy accesses) clocks the bus.
pub trait CpuBus {
    fn read(&mut self, address: u16) -> u8;
    fn write(&mut self, address: u16, value: u8);
}
pub const C: u8 = 1;
pub const Z: u8 = 2;
pub const I: u8 = 4;
pub const D: u8 = 8;
pub const V: u8 = 64;
pub const N: u8 = 128;

#[derive(Clone, Debug)]
pub struct Cpu {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub sp: u8,
    pub p: u8,
    pub pc: u16,
    pub halted: bool,
    irq_mask: bool,
}
impl Default for Cpu {
    fn default() -> Self {
        Self {
            a: 0,
            x: 0,
            y: 0,
            sp: 0,
            p: 0x24,
            pc: 0,
            halted: false,
            irq_mask: true,
        }
    }
}

impl Cpu {
    fn fetch(&mut self, bus: &mut impl CpuBus) -> u8 {
        let value = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        value
    }
    fn word(&mut self, bus: &mut impl CpuBus) -> u16 {
        let lo = self.fetch(bus) as u16;
        lo | ((self.fetch(bus) as u16) << 8)
    }
    fn push(&mut self, bus: &mut impl CpuBus, value: u8) {
        bus.write(0x100 | self.sp as u16, value);
        self.sp = self.sp.wrapping_sub(1);
    }
    fn pull(&mut self, bus: &mut impl CpuBus) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        bus.read(0x100 | self.sp as u16)
    }
    fn flag(&mut self, bit: u8, on: bool) {
        if on { self.p |= bit } else { self.p &= !bit }
    }
    fn nz(&mut self, v: u8) {
        self.flag(Z, v == 0);
        self.flag(N, v & 0x80 != 0);
    }
    fn adc(&mut self, value: u8) {
        let sum = self.a as u16 + value as u16 + (self.p & C) as u16;
        let result = sum as u8;
        self.flag(C, sum > 255);
        self.flag(V, (!(self.a ^ value) & (self.a ^ result) & 0x80) != 0);
        self.a = result;
        self.nz(result);
    }
    fn compare(&mut self, lhs: u8, rhs: u8) {
        self.flag(C, lhs >= rhs);
        self.nz(lhs.wrapping_sub(rhs));
    }
    pub fn reset(&mut self, bus: &mut impl CpuBus) {
        bus.read(self.pc);
        bus.read(self.pc);
        for _ in 0..3 {
            bus.read(0x100 | self.sp as u16);
            self.sp = self.sp.wrapping_sub(1);
        }
        let lo = bus.read(0xfffc) as u16;
        self.pc = lo | ((bus.read(0xfffd) as u16) << 8);
        self.p = (self.p | I | 0x20) & !0x10;
        self.halted = false;
        self.irq_mask = true;
    }
    pub fn can_irq(&self) -> bool {
        !self.irq_mask
    }
    pub fn interrupt(&mut self, bus: &mut impl CpuBus, nmi: bool) {
        bus.read(self.pc);
        bus.read(self.pc);
        self.push(bus, (self.pc >> 8) as u8);
        self.push(bus, self.pc as u8);
        self.push(bus, (self.p | 0x20) & !0x10);
        self.p |= I;
        self.irq_mask = true;
        let vec = if nmi { 0xfffa } else { 0xfffe };
        let lo = bus.read(vec) as u16;
        self.pc = lo | ((bus.read(vec + 1) as u16) << 8);
    }
    fn address(&mut self, bus: &mut impl CpuBus, mode: &str, write: bool) -> u16 {
        match mode {
            "i" => {
                let a = self.pc;
                self.pc = self.pc.wrapping_add(1);
                a
            }
            "z" => self.fetch(bus) as u16,
            "zx" | "zy" => {
                let z = self.fetch(bus);
                bus.read(z as u16);
                z.wrapping_add(if mode == "zx" { self.x } else { self.y }) as u16
            }
            "a" => self.word(bus),
            "ax" | "ay" => {
                let base = self.word(bus);
                let a = base.wrapping_add(if mode == "ax" { self.x } else { self.y } as u16);
                if write || base & 0xff00 != a & 0xff00 {
                    bus.read((base & 0xff00) | (a & 255));
                }
                a
            }
            "ix" => {
                let z = self.fetch(bus);
                bus.read(z as u16);
                let z = z.wrapping_add(self.x);
                let lo = bus.read(z as u16) as u16;
                lo | ((bus.read(z.wrapping_add(1) as u16) as u16) << 8)
            }
            "iy" => {
                let z = self.fetch(bus);
                let lo = bus.read(z as u16) as u16;
                let base = lo | ((bus.read(z.wrapping_add(1) as u16) as u16) << 8);
                let a = base.wrapping_add(self.y as u16);
                if write || base & 0xff00 != a & 0xff00 {
                    bus.read((base & 0xff00) | (a & 255));
                }
                a
            }
            _ => unreachable!("address mode {mode}"),
        }
    }
    pub fn step(&mut self, bus: &mut impl CpuBus) -> Result<(), String> {
        if self.halted {
            return Err("CPU is halted. Reset to continue.".into());
        }
        let at = self.pc;
        let opcode = self.fetch(bus);
        let (op, mode) = OPCODES[opcode as usize];
        let old_i = self.p & I != 0;
        match op {
            "KIL" | "UNS" => {
                self.halted = true;
                return Err(format!(
                    "Unsupported/halt opcode ${opcode:02X} at ${at:04X}. Unstable unofficial instructions are deliberately rejected."
                ));
            }
            "BRK" => {
                self.fetch(bus);
                self.push(bus, (self.pc >> 8) as u8);
                self.push(bus, self.pc as u8);
                self.push(bus, self.p | 0x30);
                self.p |= I;
                let lo = bus.read(0xfffe) as u16;
                self.pc = lo | ((bus.read(0xffff) as u16) << 8);
            }
            "JSR" => {
                let lo = self.fetch(bus) as u16;
                bus.read(0x100 | self.sp as u16);
                self.push(bus, (self.pc >> 8) as u8);
                self.push(bus, self.pc as u8);
                self.pc = lo | ((self.fetch(bus) as u16) << 8);
            }
            "JMP" => {
                let a = self.word(bus);
                self.pc = if mode == "ind" {
                    let lo = bus.read(a) as u16;
                    lo | ((bus.read((a & 0xff00) | (a.wrapping_add(1) & 255)) as u16) << 8)
                } else {
                    a
                };
            }
            "RTS" | "RTI" => {
                bus.read(self.pc);
                bus.read(0x100 | self.sp as u16);
                if op == "RTI" {
                    self.p = (self.pull(bus) | 0x20) & !0x10;
                }
                let lo = self.pull(bus) as u16;
                self.pc = lo | ((self.pull(bus) as u16) << 8);
                if op == "RTS" {
                    bus.read(self.pc);
                    self.pc = self.pc.wrapping_add(1);
                }
            }
            "PHA" | "PHP" => {
                bus.read(self.pc);
                self.push(bus, if op == "PHA" { self.a } else { self.p | 0x30 });
            }
            "PLA" | "PLP" => {
                bus.read(self.pc);
                bus.read(0x100 | self.sp as u16);
                let value = self.pull(bus);
                if op == "PLA" {
                    self.a = value;
                    self.nz(value);
                } else {
                    self.p = (value | 0x20) & !0x10;
                }
            }
            "BPL" | "BMI" | "BVC" | "BVS" | "BCC" | "BCS" | "BNE" | "BEQ" => {
                let offset = self.fetch(bus) as i8;
                let (bit, set) = match op {
                    "BPL" => (N, false),
                    "BMI" => (N, true),
                    "BVC" => (V, false),
                    "BVS" => (V, true),
                    "BCC" => (C, false),
                    "BCS" => (C, true),
                    "BNE" => (Z, false),
                    _ => (Z, true),
                };
                if (self.p & bit != 0) == set {
                    bus.read(self.pc);
                    let next = self.pc.wrapping_add_signed(offset as i16);
                    if self.pc & 0xff00 != next & 0xff00 {
                        bus.read((self.pc & 0xff00) | (next & 255));
                    }
                    self.pc = next;
                }
            }
            _ if mode == "s" => {
                bus.read(self.pc);
                match op {
                    "CLC" => self.p &= !C,
                    "SEC" => self.p |= C,
                    "CLI" => self.p &= !I,
                    "SEI" => self.p |= I,
                    "CLD" => self.p &= !D,
                    "SED" => self.p |= D,
                    "CLV" => self.p &= !V,
                    "TAX" => {
                        self.x = self.a;
                        self.nz(self.x)
                    }
                    "TAY" => {
                        self.y = self.a;
                        self.nz(self.y)
                    }
                    "TXA" => {
                        self.a = self.x;
                        self.nz(self.a)
                    }
                    "TYA" => {
                        self.a = self.y;
                        self.nz(self.a)
                    }
                    "TSX" => {
                        self.x = self.sp;
                        self.nz(self.x)
                    }
                    "TXS" => self.sp = self.x,
                    "DEX" => {
                        self.x = self.x.wrapping_sub(1);
                        self.nz(self.x)
                    }
                    "DEY" => {
                        self.y = self.y.wrapping_sub(1);
                        self.nz(self.y)
                    }
                    "INX" => {
                        self.x = self.x.wrapping_add(1);
                        self.nz(self.x)
                    }
                    "INY" => {
                        self.y = self.y.wrapping_add(1);
                        self.nz(self.y)
                    }
                    "NOP" => {}
                    _ => unreachable!("implied {op}"),
                }
            }
            _ => {
                let store = matches!(op, "STA" | "STX" | "STY" | "SAX");
                let rmw = matches!(
                    op,
                    "ASL"
                        | "LSR"
                        | "ROL"
                        | "ROR"
                        | "INC"
                        | "DEC"
                        | "SLO"
                        | "RLA"
                        | "SRE"
                        | "RRA"
                        | "DCP"
                        | "ISC"
                );
                let a = if mode == "acc" {
                    bus.read(self.pc);
                    0
                } else {
                    self.address(bus, mode, store || rmw)
                };
                if store {
                    bus.write(
                        a,
                        match op {
                            "STA" => self.a,
                            "STX" => self.x,
                            "STY" => self.y,
                            _ => self.a & self.x,
                        },
                    );
                } else {
                    let value = if mode == "acc" { self.a } else { bus.read(a) };
                    if rmw {
                        if mode != "acc" {
                            bus.write(a, value);
                        }
                        let carry = self.p & C;
                        let result = match op {
                            "ASL" | "SLO" => {
                                self.flag(C, value & 128 != 0);
                                value << 1
                            }
                            "LSR" | "SRE" => {
                                self.flag(C, value & 1 != 0);
                                value >> 1
                            }
                            "ROL" | "RLA" => {
                                self.flag(C, value & 128 != 0);
                                (value << 1) | carry
                            }
                            "ROR" | "RRA" => {
                                self.flag(C, value & 1 != 0);
                                (value >> 1) | (carry << 7)
                            }
                            "INC" | "ISC" => value.wrapping_add(1),
                            _ => value.wrapping_sub(1),
                        };
                        if mode == "acc" {
                            self.a = result
                        } else {
                            bus.write(a, result)
                        };
                        match op {
                            "SLO" => {
                                self.a |= result;
                                self.nz(self.a)
                            }
                            "RLA" => {
                                self.a &= result;
                                self.nz(self.a)
                            }
                            "SRE" => {
                                self.a ^= result;
                                self.nz(self.a)
                            }
                            "RRA" => self.adc(result),
                            "DCP" => self.compare(self.a, result),
                            "ISC" => self.adc(!result),
                            _ => self.nz(result),
                        }
                    } else {
                        match op {
                            "LDA" => {
                                self.a = value;
                                self.nz(value)
                            }
                            "LDX" => {
                                self.x = value;
                                self.nz(value)
                            }
                            "LDY" => {
                                self.y = value;
                                self.nz(value)
                            }
                            "LAX" => {
                                self.a = value;
                                self.x = value;
                                self.nz(value)
                            }
                            "ORA" => {
                                self.a |= value;
                                self.nz(self.a)
                            }
                            "AND" => {
                                self.a &= value;
                                self.nz(self.a)
                            }
                            "EOR" => {
                                self.a ^= value;
                                self.nz(self.a)
                            }
                            "ADC" => self.adc(value),
                            "SBC" => self.adc(!value),
                            "CMP" => self.compare(self.a, value),
                            "CPX" => self.compare(self.x, value),
                            "CPY" => self.compare(self.y, value),
                            "BIT" => {
                                self.flag(Z, self.a & value == 0);
                                self.p = (self.p & !0xc0) | (value & 0xc0)
                            }
                            "ANC" => {
                                self.a &= value;
                                self.nz(self.a);
                                self.flag(C, self.a & 128 != 0)
                            }
                            "ALR" => {
                                self.a &= value;
                                self.flag(C, self.a & 1 != 0);
                                self.a >>= 1;
                                self.nz(self.a)
                            }
                            "ARR" => {
                                self.a = ((self.a & value) >> 1) | ((self.p & C) << 7);
                                self.nz(self.a);
                                self.flag(C, self.a & 64 != 0);
                                self.flag(V, ((self.a >> 6) ^ (self.a >> 5)) & 1 != 0)
                            }
                            "AXS" => {
                                let lhs = self.a & self.x;
                                self.flag(C, lhs >= value);
                                self.x = lhs.wrapping_sub(value);
                                self.nz(self.x)
                            }
                            "LAS" => {
                                self.a = value & self.sp;
                                self.x = self.a;
                                self.sp = self.a;
                                self.nz(self.a)
                            }
                            "NOP" => {}
                            _ => unreachable!("read {op}"),
                        }
                    }
                }
            }
        }
        self.irq_mask = if matches!(op, "CLI" | "SEI" | "PLP") {
            old_i
        } else {
            self.p & I != 0
        };
        Ok(())
    }
}

const OPCODES: [(&str, &str); 256] = [
    ("BRK", "s"),
    ("ORA", "ix"),
    ("KIL", "s"),
    ("SLO", "ix"),
    ("NOP", "z"),
    ("ORA", "z"),
    ("ASL", "z"),
    ("SLO", "z"),
    ("PHP", "s"),
    ("ORA", "i"),
    ("ASL", "acc"),
    ("ANC", "i"),
    ("NOP", "a"),
    ("ORA", "a"),
    ("ASL", "a"),
    ("SLO", "a"),
    ("BPL", "r"),
    ("ORA", "iy"),
    ("KIL", "s"),
    ("SLO", "iy"),
    ("NOP", "zx"),
    ("ORA", "zx"),
    ("ASL", "zx"),
    ("SLO", "zx"),
    ("CLC", "s"),
    ("ORA", "ay"),
    ("NOP", "s"),
    ("SLO", "ay"),
    ("NOP", "ax"),
    ("ORA", "ax"),
    ("ASL", "ax"),
    ("SLO", "ax"),
    ("JSR", "a"),
    ("AND", "ix"),
    ("KIL", "s"),
    ("RLA", "ix"),
    ("BIT", "z"),
    ("AND", "z"),
    ("ROL", "z"),
    ("RLA", "z"),
    ("PLP", "s"),
    ("AND", "i"),
    ("ROL", "acc"),
    ("ANC", "i"),
    ("BIT", "a"),
    ("AND", "a"),
    ("ROL", "a"),
    ("RLA", "a"),
    ("BMI", "r"),
    ("AND", "iy"),
    ("KIL", "s"),
    ("RLA", "iy"),
    ("NOP", "zx"),
    ("AND", "zx"),
    ("ROL", "zx"),
    ("RLA", "zx"),
    ("SEC", "s"),
    ("AND", "ay"),
    ("NOP", "s"),
    ("RLA", "ay"),
    ("NOP", "ax"),
    ("AND", "ax"),
    ("ROL", "ax"),
    ("RLA", "ax"),
    ("RTI", "s"),
    ("EOR", "ix"),
    ("KIL", "s"),
    ("SRE", "ix"),
    ("NOP", "z"),
    ("EOR", "z"),
    ("LSR", "z"),
    ("SRE", "z"),
    ("PHA", "s"),
    ("EOR", "i"),
    ("LSR", "acc"),
    ("ALR", "i"),
    ("JMP", "a"),
    ("EOR", "a"),
    ("LSR", "a"),
    ("SRE", "a"),
    ("BVC", "r"),
    ("EOR", "iy"),
    ("KIL", "s"),
    ("SRE", "iy"),
    ("NOP", "zx"),
    ("EOR", "zx"),
    ("LSR", "zx"),
    ("SRE", "zx"),
    ("CLI", "s"),
    ("EOR", "ay"),
    ("NOP", "s"),
    ("SRE", "ay"),
    ("NOP", "ax"),
    ("EOR", "ax"),
    ("LSR", "ax"),
    ("SRE", "ax"),
    ("RTS", "s"),
    ("ADC", "ix"),
    ("KIL", "s"),
    ("RRA", "ix"),
    ("NOP", "z"),
    ("ADC", "z"),
    ("ROR", "z"),
    ("RRA", "z"),
    ("PLA", "s"),
    ("ADC", "i"),
    ("ROR", "acc"),
    ("ARR", "i"),
    ("JMP", "ind"),
    ("ADC", "a"),
    ("ROR", "a"),
    ("RRA", "a"),
    ("BVS", "r"),
    ("ADC", "iy"),
    ("KIL", "s"),
    ("RRA", "iy"),
    ("NOP", "zx"),
    ("ADC", "zx"),
    ("ROR", "zx"),
    ("RRA", "zx"),
    ("SEI", "s"),
    ("ADC", "ay"),
    ("NOP", "s"),
    ("RRA", "ay"),
    ("NOP", "ax"),
    ("ADC", "ax"),
    ("ROR", "ax"),
    ("RRA", "ax"),
    ("NOP", "i"),
    ("STA", "ix"),
    ("NOP", "i"),
    ("SAX", "ix"),
    ("STY", "z"),
    ("STA", "z"),
    ("STX", "z"),
    ("SAX", "z"),
    ("DEY", "s"),
    ("NOP", "i"),
    ("TXA", "s"),
    ("UNS", "i"),
    ("STY", "a"),
    ("STA", "a"),
    ("STX", "a"),
    ("SAX", "a"),
    ("BCC", "r"),
    ("STA", "iy"),
    ("KIL", "s"),
    ("UNS", "iy"),
    ("STY", "zx"),
    ("STA", "zx"),
    ("STX", "zy"),
    ("SAX", "zy"),
    ("TYA", "s"),
    ("STA", "ay"),
    ("TXS", "s"),
    ("UNS", "ay"),
    ("UNS", "ax"),
    ("STA", "ax"),
    ("UNS", "ay"),
    ("UNS", "ay"),
    ("LDY", "i"),
    ("LDA", "ix"),
    ("LDX", "i"),
    ("LAX", "ix"),
    ("LDY", "z"),
    ("LDA", "z"),
    ("LDX", "z"),
    ("LAX", "z"),
    ("TAY", "s"),
    ("LDA", "i"),
    ("TAX", "s"),
    ("UNS", "i"),
    ("LDY", "a"),
    ("LDA", "a"),
    ("LDX", "a"),
    ("LAX", "a"),
    ("BCS", "r"),
    ("LDA", "iy"),
    ("KIL", "s"),
    ("LAX", "iy"),
    ("LDY", "zx"),
    ("LDA", "zx"),
    ("LDX", "zy"),
    ("LAX", "zy"),
    ("CLV", "s"),
    ("LDA", "ay"),
    ("TSX", "s"),
    ("LAS", "ay"),
    ("LDY", "ax"),
    ("LDA", "ax"),
    ("LDX", "ay"),
    ("LAX", "ay"),
    ("CPY", "i"),
    ("CMP", "ix"),
    ("NOP", "i"),
    ("DCP", "ix"),
    ("CPY", "z"),
    ("CMP", "z"),
    ("DEC", "z"),
    ("DCP", "z"),
    ("INY", "s"),
    ("CMP", "i"),
    ("DEX", "s"),
    ("AXS", "i"),
    ("CPY", "a"),
    ("CMP", "a"),
    ("DEC", "a"),
    ("DCP", "a"),
    ("BNE", "r"),
    ("CMP", "iy"),
    ("KIL", "s"),
    ("DCP", "iy"),
    ("NOP", "zx"),
    ("CMP", "zx"),
    ("DEC", "zx"),
    ("DCP", "zx"),
    ("CLD", "s"),
    ("CMP", "ay"),
    ("NOP", "s"),
    ("DCP", "ay"),
    ("NOP", "ax"),
    ("CMP", "ax"),
    ("DEC", "ax"),
    ("DCP", "ax"),
    ("CPX", "i"),
    ("SBC", "ix"),
    ("NOP", "i"),
    ("ISC", "ix"),
    ("CPX", "z"),
    ("SBC", "z"),
    ("INC", "z"),
    ("ISC", "z"),
    ("INX", "s"),
    ("SBC", "i"),
    ("NOP", "s"),
    ("SBC", "i"),
    ("CPX", "a"),
    ("SBC", "a"),
    ("INC", "a"),
    ("ISC", "a"),
    ("BEQ", "r"),
    ("SBC", "iy"),
    ("KIL", "s"),
    ("ISC", "iy"),
    ("NOP", "zx"),
    ("SBC", "zx"),
    ("INC", "zx"),
    ("ISC", "zx"),
    ("SED", "s"),
    ("SBC", "ay"),
    ("NOP", "s"),
    ("ISC", "ay"),
    ("NOP", "ax"),
    ("SBC", "ax"),
    ("INC", "ax"),
    ("ISC", "ax"),
];
