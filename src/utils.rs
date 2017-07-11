pub fn buf_to_u16(buf: &[u8]) -> u16 {
    (buf[0] as u16) << 8 | (buf[1] as u16)
}

pub fn buf_to_u32(buf: &[u8]) -> u32 {
    (buf_to_u16(&buf[0..2]) as u32) << 16 | (buf_to_u16(&buf[2..4]) as u32)
}

pub fn u16_to_u8(v: u16) -> Vec<u8> {
    vec![(v >> 8) as u8, (v & 0xff) as u8]
}

pub fn u32_to_u16(v: u32) -> Vec<u16> {
    vec![(v >> 16) as u16, (v & 0xffff) as u16]
}

pub fn u32_to_u8(v: u32) -> Vec<u8> {
    u32_to_u16(v)
        .iter()
        .flat_map(|x| u16_to_u8(*x))
        .collect::<Vec<u8>>()
}


pub fn u8_to_u16_vec(v: &mut Vec<u8>) -> Vec<u16> {
    if v.len() % 2 == 1 {
        v.push(0u8);
    }
    v.iter()
        .zip(v.iter().skip(1))
        .enumerate()
        .filter(|&(i, _)| i % 2 == 0)
        .map(|(_, p)| p)
        .map(|(a, b)| ((*a as u16) << 8) | (*b as u16))
        .collect::<Vec<u16>>()
}

pub fn in_wrapped_range((l, r): (u32, u32), num: u32) -> bool {
    (r < l && (num >= l || num < r)) || (num >= l && num < r)
}

#[cfg(tests)]
mod tests {}
