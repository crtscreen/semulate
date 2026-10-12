//! tiny synthetic fixtures built in memory
pub fn rom(mapper: u8, flags: u8, prg_banks: u8, chr_banks: u8, code: &[u8]) -> Vec<u8> {
    let prg_len = prg_banks as usize * 16384;
    assert!(prg_len >= 6 && code.len() < prg_len - 6);
    let mut bytes = vec![0; 16 + prg_len + chr_banks as usize * 8192];
    bytes[..4].copy_from_slice(b"NES\x1a");
    bytes[4] = prg_banks;
    bytes[5] = chr_banks;
    bytes[6] = (mapper << 4) | (flags & 15);
    bytes[7] = mapper & 0xf0;
    bytes[16..16 + prg_len].fill(0xea);
    bytes[16..16 + code.len()].copy_from_slice(code);
    for offset in [6, 4, 2] {
        bytes[16 + prg_len - offset..16 + prg_len - offset + 2]
            .copy_from_slice(&0x8000u16.to_le_bytes());
    }
    bytes
}
