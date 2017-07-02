#[derive(Debug, Clone)]
pub struct Segment {
    src_port: u16,
    dest_port: u16,
    seg_size: u32,
    seq_num: u32,
    ack_num: u32,
    flags: u16,
    checksum: u16,
    payload: Box<[u8]>,
}

use std::fmt::{Binary, Formatter, Error};

impl Binary for Segment {
    fn fmt(&self, f: &mut Formatter) -> Result<(), Error> {
        let bytes = self.to_byte_vec();
        for (i, b) in bytes.iter().enumerate() {
            if i % 2 == 0 {
                write!(f, "\n")?;
            }
            write!(f, "{:01$b} ", b, 8)?;
        }
        Ok(())
    }
}



pub enum Flag {
    SYN,
    ACK,
    FIN,
}

impl Segment {
    pub fn new() -> Segment {
        Segment {
            src_port: 0,
            dest_port: 0,
            seg_size: 0,
            seq_num: 0,
            ack_num: 0,
            flags: 0,
            checksum: 0,
            payload: Box::new([]),
        }
    }

    pub fn set_flag(&mut self, flag: Flag) {
        self.flags |= 1 <<
            match flag {
                Flag::SYN => 15,
                Flag::ACK => 14,
                Flag::FIN => 13,
            };
        self.checksum = self.generate_checksum();
    }

    pub fn set_data(&mut self, data: Vec<u8>) {
        self.payload = data.into_boxed_slice();
        self.checksum = self.generate_checksum();
    }

    fn to_byte_vec(&self) -> Vec<u8> {
        let u16_to_u8 = |v: u16| vec![(v >> 8) as u8, (v & 0xff) as u8];
        let u32_to_u16 = |v: u32| vec![(v >> 16) as u16, (v & 0xffff) as u16];
        let u32_to_u8 = |v: u32| {
            u32_to_u16(v)
                .iter()
                .flat_map(|x| u16_to_u8(*x))
                .collect::<Vec<u8>>()
        };

        let mut set = u16_to_u8(self.src_port);
        set.extend(u16_to_u8(self.dest_port).iter());
        set.extend(u32_to_u8(self.seg_size).iter());
        set.extend(u32_to_u8(self.seq_num).iter());
        set.extend(u32_to_u8(self.ack_num).iter());
        set.extend(u16_to_u8(self.flags).iter());
        set.extend(u16_to_u8(self.checksum).iter());
        set.extend(self.payload.clone().iter());

        set
    }

    fn u8_to_u16_vec(v: &mut Vec<u8>) -> Vec<u16> {
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


    pub fn generate_checksum(&self) -> u16 {
        let mut bytes = self.to_byte_vec();
        bytes = bytes.iter().take(18).map(|x| *x).collect(); // Skip checksum field
        bytes.extend(self.payload.clone().iter());

        let checksum_pairs = Segment::u8_to_u16_vec(&mut bytes);
        let mut sum = checksum_pairs.iter().fold(0u32, |acc, x| {
            let sum = (0u32 | (*x as u32)) + acc;
            (sum % (1 << 16)) + (sum / (1 << 15))
        }) as u16;

        if sum == 0 {
            sum = !sum;
        }
        !sum
    }

    pub fn validate(&self) -> bool {
        self.checksum == self.generate_checksum()
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checksum() {
        let mut seg = Segment::new();
        seg.src_port = 2;
        seg.dest_port = 5;
        seg.seq_num = 32 + (32 << 16);
        seg.flags = 4;
        let data: Vec<u8> = vec![2, 4, 6, 8];
        seg.set_data(data);

        assert_eq!(seg.checksum, 63400);

        let old_checksum = seg.checksum;
        seg.set_flag(Flag::SYN);

        assert_ne!(old_checksum, seg.checksum);

        assert!(seg.validate());
        seg.flags |= 0b00010;
        assert!(!seg.validate());
    }
}
