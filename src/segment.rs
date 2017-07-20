use utils::*;

#[derive(Debug, Clone)]
pub struct Segment {
    src_port: u16,
    dst_port: u16,
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
    ACK,
    SYN,
    FIN,
}

impl Segment {
    pub fn src_port(&self) -> u16 {
        self.src_port
    }

    pub fn dst_port(&self) -> u16 {
        self.dst_port
    }

    pub fn seq_num(&self) -> u32 {
        self.seq_num
    }

    pub fn ack_num(&self) -> u32 {
        self.ack_num
    }

    pub fn set_seq(&mut self, seq_num: u32) {
        self.seq_num = seq_num;
        self.checksum = self.generate_checksum();
    }

    pub fn set_ack_num(&mut self, ack_num: u32) {
        self.ack_num = ack_num;
        self.checksum = self.generate_checksum();
    }

    pub fn payload(&self) -> Vec<u8> {
        Vec::from(&*self.payload)
    }

    pub fn new(src_port: u16, dst_port: u16) -> Segment {
        let mut base = Segment {
            src_port: src_port,
            dst_port: dst_port,
            seg_size: 20,
            seq_num: 0,
            ack_num: 0,
            flags: 0,
            checksum: 0,
            payload: Box::new([]),
        };
        base.checksum = base.generate_checksum();
        base
    }

    pub fn from_buf(buf: Vec<u8>) -> Segment {
        assert!(buf.len() >= 20);
        Segment {
            src_port: buf_to_u16(&buf[0..2]),
            dst_port: buf_to_u16(&buf[2..4]),
            seg_size: buf_to_u32(&buf[4..8]),
            seq_num: buf_to_u32(&buf[8..12]),
            ack_num: buf_to_u32(&buf[12..16]),
            flags: buf_to_u16(&buf[16..18]),
            checksum: buf_to_u16(&buf[18..20]),
            payload: Vec::from(&buf[20..]).into_boxed_slice(),
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

    pub fn unset_flag(&mut self, flag: Flag) {
        let mut flipped = !self.flags;
        flipped |= 1 <<
            match flag {
                Flag::SYN => 15,
                Flag::ACK => 14,
                Flag::FIN => 13,
            };
        self.flags = !flipped;
        self.checksum = self.generate_checksum();
    }

    pub fn get_flag(&self, flag: Flag) -> bool {
        match flag {
            Flag::SYN => self.flags & 1 << 15 > 0,
            Flag::ACK => self.flags & 1 << 14 > 0,
            Flag::FIN => self.flags & 1 << 13 > 0,
        }
    }

    pub fn set_data(&mut self, data: Vec<u8>) {
        self.seg_size = 20 + data.len() as u32;
        self.payload = data.into_boxed_slice();
        self.checksum = self.generate_checksum();
    }

    pub fn to_byte_vec(&self) -> Vec<u8> {
        let mut set = u16_to_u8(self.src_port);
        set.extend(u16_to_u8(self.dst_port));
        set.extend(u32_to_u8(self.seg_size));
        set.extend(u32_to_u8(self.seq_num));
        set.extend(u32_to_u8(self.ack_num));
        set.extend(u16_to_u8(self.flags));
        set.extend(u16_to_u8(self.checksum));
        set.extend(self.payload.clone().iter());

        set
    }


    pub fn generate_checksum(&mut self) -> u16 {
        self.checksum = 0;
        let mut bytes = self.to_byte_vec();
        let mut sum = ones_complement_sum(&mut bytes);

        if sum == 0 {
            sum = !sum;
        }
        !sum
    }

    pub fn validate(&self) -> bool {
        let mut bytes = self.to_byte_vec();
        ones_complement_sum(&mut bytes) == 0xFFFF
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seg_size() {
        let mut seg = Segment::new(0, 0);
        let data: Vec<u8> = vec![2, 4, 6, 8];
        seg.set_data(data);
        assert_eq!(seg.seg_size, 24);
    }

    #[test]
    fn checksum() {
        let mut seg = Segment::new(0, 0);
        seg.src_port = 2;
        seg.dst_port = 5;
        seg.seq_num = 32 + (32 << 16);
        seg.flags = 4;
        let data: Vec<u8> = vec![2, 4, 6, 8];
        seg.set_data(data);

        assert_eq!(seg.checksum, 63376);

        let old_checksum = seg.checksum;
        seg.set_flag(Flag::SYN);

        assert_ne!(old_checksum, seg.checksum);

        assert!(seg.validate());
        seg.flags |= 0b00010;
        assert!(!seg.validate());
    }

    #[test]
    fn checksum_tpp() {
        let mut bytes = vec![
            147,
            162,
            39,
            35,
            0,
            0,
            0,
            20,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            128,
            0,
            68,
            166,
        ];
        println!("TPP: {:17b}", ones_complement_sum(&mut bytes));
        let seg = Segment::from_buf(bytes);
        // println!("{:17b} == {:17b}", seg.checksum, seg.generate_checksum());
        assert!(seg.validate());
    }

    #[test]
    fn checksum_handout() {
        let mut bytes: Vec<u8> = vec![
            0b00001100,
            0b00001000,
            0b00010000,
            0b00001000, //| Source Port | Dest. Port
            0b00000000,
            0b00000000,
            0b00000000,
            0b00010111, //| Segment Size (23 bytes)
            0b00000000,
            0b00000000,
            0b00000000,
            0b00000011, //| Sequence Number
            0b00000000,
            0b00000000,
            0b00000000,
            0b00000011, //| Acknowledgement Number
            0b01000000,
            0b00000000,
            0b01001111,
            0b01111100, //| Flags | Checksum
            0b01010101,
            0b01010101,
            0b11111111,
        ];
        println!("TPP: {:17b}", ones_complement_sum(&mut bytes));
        let seg = Segment::from_buf(bytes);
        assert!(seg.validate());
    }

    #[test]
    fn checksum_website() {
        let mut bytes: Vec<u8> = vec![
            0b10000110,
            0b01011110,
            0b10101100,
            0b01100000,
            0b01110001,
            0b00101010,
            0b10000001,
            0b10110101,
        ];
        assert_eq!(ones_complement_sum(&mut bytes), 0b0010010110011111);
    }

    #[test]
    fn flags() {
        let mut seg = Segment::new(0, 0);
        let get_flags = |seg: &Segment| {
            (
                seg.get_flag(Flag::SYN),
                seg.get_flag(Flag::ACK),
                seg.get_flag(Flag::FIN),
            )
        };
        assert_eq!(get_flags(&seg), (false, false, false));
        seg.set_flag(Flag::SYN);
        assert_eq!(get_flags(&seg), (true, false, false));
        seg.set_flag(Flag::FIN);
        assert_eq!(get_flags(&seg), (true, false, true));
        seg.set_flag(Flag::ACK);
        assert_eq!(get_flags(&seg), (true, true, true));
        seg.unset_flag(Flag::SYN);
        assert_eq!(get_flags(&seg), (false, true, true));
        assert!(seg.validate());
    }
}
