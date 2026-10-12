//! bit order A B Select Start Up Down Left Right
#[derive(Clone, Default)]
pub struct Controller {
    pub buttons: u8,
    shift: u8,
    strobe: bool,
}
impl Controller {
    pub fn write(&mut self, value: u8) {
        if self.strobe || value & 1 != 0 {
            self.shift = self.buttons;
        }
        self.strobe = value & 1 != 0;
    }
    pub fn read(&mut self) -> u8 {
        if self.strobe {
            return self.buttons & 1;
        }
        let bit = self.shift & 1;
        self.shift = (self.shift >> 1) | 0x80;
        bit
    }
}
