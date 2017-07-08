use std::net::*;
use segment::*;
use std::time::{SystemTime, Duration};
use std::cmp::min;
use utils::*;

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum TCBEvent {
    Estab,
    Closed,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum RDTState {
    IDLE,
    SENDING, // remaining
    RECEIVING, // remaining
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum TCBState {
    Listen,
    SynSent,
    SynRecd,
    Estab,
    Closed,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct TCPTuple {
    pub src: SocketAddr,
    pub dst: SocketAddr,
}

impl TCPTuple {
    pub fn from(src: SocketAddr, dst: SocketAddr) -> TCPTuple {
        TCPTuple { src: src, dst: dst }
    }
}

use std::sync::mpsc::*;
use std::collections::VecDeque;
#[derive(Debug)]
pub struct TCB {
    pub state: TCBState,
    pub tuple: TCPTuple,
    pub timeout: SystemTime,
    pub sock: UdpSocket,

    // Sender state
    seq_base: u32,
    next_seq: u32,
    send_buffer: Box<[u8]>,
    bytes_sent: u32,
    window: VecDeque<u8>,

    // Receiver state
    ack_base: u32,
    recv_buffer: Vec<u8>,
    recv_window: VecDeque<Option<u8>>,
}

const WINDOW_SIZE: usize = 5;
const MAX_PAYLOAD_SIZE: usize = 2;


impl TCB {
    pub fn new(tuple: TCPTuple, sock: UdpSocket) -> TCB {
        TCB {
            state: TCBState::Listen,
            tuple: tuple,
            timeout: SystemTime::now(),
            sock: sock,

            seq_base: 1,
            next_seq: 1,
            send_buffer: Box::new([]),
            bytes_sent: 0,
            window: VecDeque::with_capacity(WINDOW_SIZE),
            recv_window: VecDeque::from(vec![Option::None; WINDOW_SIZE]),

            ack_base: 1,
            recv_buffer: Vec::new(),
        }
    }

    fn sr_sender(&mut self, payload: Vec<u8>, rx: &Receiver<Segment>) -> Result<(), ()> {
        let total_bytes = payload.len();
        let mut bytes_acked = 0;
        let mut bytes_sent = 0;
        let mut window = VecDeque::new();
        let mut unacked_segs = VecDeque::new();
        let mut sent_not_acked = 0;
        let mut next_seq = self.seq_base;
        while bytes_acked < total_bytes {
            window.extend(
                &payload[bytes_sent..bytes_sent + min(WINDOW_SIZE, total_bytes - bytes_sent)],
            );

            // Send until window is full
            while sent_not_acked < window.len() {
                let mut seg = self.new_seg();
                seg.set_seq(next_seq);
                let size = min(total_bytes - sent_not_acked, MAX_PAYLOAD_SIZE);
                let size = min(size, window.len() - sent_not_acked);
                let data = window
                    .iter()
                    .map(|r| *r)
                    .skip(sent_not_acked)
                    .take(size)
                    .collect();
                seg.set_data(data);
                self.send_seg(&seg);
                unacked_segs.push_back(seg);

                next_seq = self.seq_base.wrapping_add((sent_not_acked + size) as u32);
                bytes_sent += size;
                sent_not_acked += size;
            }

            'await_ack: loop {
                match rx.recv_timeout(Duration::from_secs(1)) {
                    Ok(seg) => {
                        if seg.get_flag(Flag::FIN) {
                            return Err(());
                        }
                        let inrange = wrapping_range_check(
                            (
                                self.seq_base.wrapping_add(1),
                                self.seq_base.wrapping_add((WINDOW_SIZE + 1) as u32),
                            ),
                            seg.ack_num(),
                        );
                        if seg.get_flag(Flag::ACK) && inrange {
                            let num_acked_bytes = seg.ack_num().wrapping_sub(self.seq_base);
                            self.seq_base = seg.ack_num();
                            if num_acked_bytes < window.len() as u32 {
                                window = window.split_off(num_acked_bytes as usize);
                                while unacked_segs.front().unwrap().seq_num() < seg.ack_num() {
                                    unacked_segs = unacked_segs.split_off(1);
                                }
                            } else {
                                window.clear();
                                unacked_segs.clear();
                            }
                            sent_not_acked -= num_acked_bytes as usize;
                            bytes_acked += num_acked_bytes as usize;
                            // println!(
                            //     "Got {} bytes, now at {}/{}",
                            //     num_acked_bytes,
                            //     bytes_acked,
                            //     total_bytes
                            // );
                            break 'await_ack;
                        } else {
                            println!("Got ack num {} <= {}", seg.ack_num(), self.seq_base);
                        }
                    }
                    Err(e) if e == RecvTimeoutError::Timeout => {
                        println!("TIMEOUT: Sending Seg: {:?}", unacked_segs.front().unwrap());
                        self.send_seg(unacked_segs.front().unwrap());
                    }
                    Err(_) => panic!(),
                }
            }

        }
        Ok(())

    }

    fn sr_receiver(&mut self, bytes_requested: u32, rx: &Receiver<Segment>) -> Result<Vec<u8>, ()> {
        while self.recv_buffer.len() < bytes_requested as usize {
            let seg = rx.recv().unwrap();
            if seg.get_flag(Flag::FIN) {
                return Err(());
            }

            //            println!("Received seg: {:?}", seg);

            let max_ack = self.ack_base.wrapping_add(WINDOW_SIZE as u32);
            if wrapping_range_check((self.ack_base, max_ack), seg.seq_num()) {
                let window_index_base = seg.seq_num().wrapping_sub(self.ack_base) as usize;
                for (i, byte) in seg.payload().iter().enumerate() {
                    self.recv_window[window_index_base + i] = Some(*byte);
                }
            }

            if seg.seq_num() == self.ack_base {
                loop {
                    // Move
                    match self.recv_window.front() {
                        Some(opt) => {
                            match *opt {
                                Some(byte) => {
                                    self.recv_buffer.push(byte);
                                }
                                None => break,
                            }
                        }
                        None => break,
                    }
                    self.ack_base += 1;
                    self.recv_window.pop_front();
                    self.recv_window.push_back(None);
                }

                let mut ack = self.new_seg();
                ack.set_flag(Flag::ACK);
                ack.set_ack_num(self.ack_base);
                //                println!("Sending ack: {:?}", ack);
                self.send_seg(&ack);
            } else {
                println!("Got seg: {}, expected {}", seg.seq_num(), self.ack_base);
            }

            //println!("Got {}/{} bytes", self.recv_buffer.len(), bytes_requested);
        }

        let payload = Vec::from(&self.recv_buffer[0..bytes_requested as usize]);
        self.recv_buffer = Vec::from(&self.recv_buffer[bytes_requested as usize..]);
        Ok(payload)
    }

    pub fn send(&mut self, payload: Vec<u8>, rx: &Receiver<Segment>) -> Result<(), ()> {
        self.sr_sender(payload, rx)
    }

    pub fn recv(&mut self, size: u32, rx: &Receiver<Segment>) -> Result<Vec<u8>, ()> {
        self.sr_receiver(size, rx)
    }

    pub fn reset(&mut self) {
        self.state = TCBState::Listen;
    }

    fn new_seg(&self) -> Segment {
        let mut seg = Segment::new(self.tuple.src.port(), self.tuple.dst.port());
        seg.set_seq(self.next_seq);
        seg
    }

    fn next_seg(&mut self) -> Segment {
        let seg = self.new_seg();
        self.seq_base += 1;
        seg
    }

    fn next_ack(&mut self) -> Segment {
        let mut seg = Segment::new(self.tuple.src.port(), self.tuple.dst.port());
        seg.set_ack_num(self.ack_base);
        seg
    }

    pub fn check_timeout(&mut self) -> Option<TCBEvent> {
        None
    }

    pub fn open(&mut self) {
        self.state = TCBState::Listen;
    }

    pub fn close(&mut self) {
        let mut fin = self.next_seg();
        fin.set_flag(Flag::FIN);
        self.send_seg(&fin);
        self.state = TCBState::Closed;
    }

    pub fn send_syn(&mut self) {
        self.seq_base = 1337;
        // println!("{} sending SYN", self.tuple.src);
        let mut syn = self.next_seg();
        syn.set_flag(Flag::SYN);
        self.send_seg(&syn);
        self.state = TCBState::SynSent;
    }

    fn send_seg(&self, seg: &Segment) {
        let bytes = seg.to_byte_vec();
        self.sock.send_to(&bytes[..], &self.tuple.dst).unwrap();
    }

    pub fn handle_shake(&mut self, seg: Segment) {
        if self.state != TCBState::Listen && !seg.get_flag(Flag::ACK) &&
            seg.seq_num() < self.ack_base
        {
            println!(
                "{} Received segment thats already been handled",
                self.tuple.src
            );
            return;
        }
        match self.state {
            TCBState::Listen => {
                if seg.get_flag(Flag::SYN) {
                    // For receives
                    self.ack_base = seg.seq_num() + 1;

                    self.state = TCBState::SynRecd;
                    let mut synack = self.next_ack();
                    synack.set_flag(Flag::SYN);
                    synack.set_flag(Flag::ACK);
                    synack.set_seq(self.seq_base);

                    self.send_seg(&synack);
                }
            }
            TCBState::SynRecd => {
                if seg.get_flag(Flag::ACK) {
                    self.seq_base += 1;
                    self.state = TCBState::Estab;
                }
            }
            TCBState::Estab => {
                if seg.get_flag(Flag::FIN) {
                    self.state = TCBState::Closed;
                    let mut ack = self.next_ack();
                    ack.set_flag(Flag::ACK);
                    self.send_seg(&ack);
                }
            }
            TCBState::SynSent => {
                if seg.get_flag(Flag::ACK) && seg.get_flag(Flag::SYN) {
                    self.ack_base = seg.seq_num() + 1;
                    let mut ack = self.next_ack();
                    ack.set_flag(Flag::ACK);
                    self.send_seg(&ack);
                    self.state = TCBState::Estab;
                }
            }
            TCBState::Closed => {}
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::channel;

    fn default_tcb() -> TCB {
        let server_sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let client_sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let tuple = TCPTuple::from(
            server_sock.local_addr().unwrap(),
            client_sock.local_addr().unwrap(),
        );
        TCB::new(tuple, server_sock)
    }

    fn mkseg(tcb: &TCB) -> Segment {
        Segment::new(tcb.tuple.dst.port(), tcb.tuple.src.port())
    }

    #[test]
    fn tcb() {
        let tcb = tests::default_tcb();
        assert_eq!(tcb.state, TCBState::Listen);
    }

    fn sock_recv(sock: &UdpSocket) -> Segment {
        let mut buf = vec![0; (1 << 16) - 1];
        let (amt, _) = sock.recv_from(&mut buf).unwrap();
        let buf = Vec::from(&mut buf[..amt]);
        Segment::from_buf(buf)
    }

    #[test]
    fn handshake() {
        let mut tcb = tests::default_tcb();
        let server_addr = tcb.tuple.src;
        let client_addr = tcb.tuple.dst;
        let client_sock = UdpSocket::bind(client_addr).unwrap();

        let mut syn = Segment::new(client_addr.port(), server_addr.port());
        syn.set_flag(Flag::SYN);
        tcb.handle_shake(syn);
        assert_eq!(tcb.state, TCBState::SynRecd);
        let answer: Segment = sock_recv(&client_sock);
        assert!(answer.get_flag(Flag::ACK));
        assert!(answer.get_flag(Flag::SYN));

        let mut ack = Segment::new(client_addr.port(), server_addr.port());
        ack.set_seq(1);
        ack.set_flag(Flag::ACK);
        tcb.handle_shake(ack);
        assert_eq!(tcb.state, TCBState::Estab);

        let mut fin = Segment::new(client_addr.port(), server_addr.port());
        fin.set_seq(2);
        fin.set_flag(Flag::FIN);
        tcb.handle_shake(fin);
        assert_eq!(tcb.state, TCBState::Closed);
        let answer: Segment = sock_recv(&client_sock);
        assert!(answer.get_flag(Flag::ACK));

        tcb.send_syn();
        assert_eq!(tcb.state, TCBState::SynSent);
        let answer: Segment = sock_recv(&client_sock);
        assert!(answer.get_flag(Flag::SYN));

        let mut synack = Segment::new(client_addr.port(), server_addr.port());
        synack.set_seq(tcb.ack_base);
        synack.set_flag(Flag::ACK);
        synack.set_flag(Flag::SYN);
        tcb.handle_shake(synack);
        assert_eq!(tcb.state, TCBState::Estab);

        let answer: Segment = sock_recv(&client_sock);
        assert!(answer.get_flag(Flag::ACK));

        tcb.close();
        let answer: Segment = sock_recv(&client_sock);
        assert!(answer.get_flag(Flag::FIN));
    }

    #[test]
    fn sr_sender_single_seg() {
        let mut tcb = tests::default_tcb();
        tcb.seq_base = 1337;
        let data = String::from("hello").into_bytes();
        assert!(data.len() <= MAX_PAYLOAD_SIZE);
        assert!(data.len() <= WINDOW_SIZE);

        let mut ack1 = mkseg(&tcb);
        ack1.set_flag(Flag::ACK);
        ack1.set_ack_num(tcb.seq_base + data.len() as u32);

        let (tx, rx) = channel();
        tx.send(ack1).unwrap();
        tcb.sr_sender(data, &rx);
    }

    #[test]
    fn sr_sender_single_window() {
        let mut tcb = tests::default_tcb();
        tcb.seq_base = 1;
        let data = String::from("hello there").into_bytes();
        assert!(data.len() > MAX_PAYLOAD_SIZE);
        assert!(data.len() <= WINDOW_SIZE);

        let mut ack1 = mkseg(&tcb);
        ack1.set_flag(Flag::ACK);
        ack1.set_ack_num(tcb.seq_base + MAX_PAYLOAD_SIZE as u32);

        let mut ack2 = mkseg(&tcb);
        ack2.set_flag(Flag::ACK);
        ack2.set_ack_num(tcb.seq_base + data.len() as u32);

        let (tx, rx) = channel();
        tx.send(ack1).unwrap();
        tx.send(ack2).unwrap();
        tcb.sr_sender(data, &rx);
    }

    #[test]
    fn sr_sender_u32_border() {
        // TODO:  Check other border
        let mut tcb = tests::default_tcb();
        let data = String::from("hello there").into_bytes();
        assert!(data.len() > MAX_PAYLOAD_SIZE);
        assert!(data.len() <= WINDOW_SIZE);

        tcb.seq_base = u32::max_value() - ((data.len() / 2) as u32);

        let mut ack1 = mkseg(&tcb);
        ack1.set_flag(Flag::ACK);
        ack1.set_ack_num(tcb.seq_base.wrapping_add(MAX_PAYLOAD_SIZE as u32));

        let mut ack2 = mkseg(&tcb);
        ack2.set_flag(Flag::ACK);
        ack2.set_ack_num(tcb.seq_base.wrapping_add(data.len() as u32));

        let (tx, rx) = channel();
        tx.send(ack1).unwrap();
        tx.send(ack2).unwrap();
        tcb.sr_sender(data, &rx);
    }

    #[test]
    fn sr_sender_multi_window() {
        let mut tcb = tests::default_tcb();
        let recv_socket = UdpSocket::bind(tcb.tuple.dst).unwrap();
        tcb.seq_base = 1337;
        let data = String::from("123456").into_bytes();
        // let data = String::from("Have you ever heard the tragedy of Darth Plaguis the wise?")
        //     .into_bytes();
        // assert!(data.len() > MAX_PAYLOAD_SIZE);
        // assert!(data.len() > WINDOW_SIZE);


        let (tx, rx) = channel();
        let mut ack_num = 0;
        while ack_num < data.len() {
            let mut ack = mkseg(&tcb);
            ack_num += min(MAX_PAYLOAD_SIZE, data.len() - ack_num);
            ack.set_flag(Flag::ACK);
            ack.set_ack_num(tcb.seq_base.wrapping_add(ack_num as u32));
            let mut dupe = ack.clone();
            dupe.set_ack_num(ack.ack_num() - MAX_PAYLOAD_SIZE as u32);
            tx.send(ack).unwrap();
            // tx.send(dupe).unwrap();
        }

        tcb.sr_sender(data.clone(), &rx);

        let mut bytes = vec![];
        while bytes.len() < data.len() {
            let seg = sock_recv(&recv_socket);
            bytes.extend(seg.payload());
        }

        assert_eq!(bytes, data);

    }

    #[test]
    fn sr_receiver_single_seg() {
        let mut tcb = tests::default_tcb();
        tcb.ack_base = 12;
        let recv_socket = UdpSocket::bind(tcb.tuple.dst).unwrap();
        let (tx, rx) = channel();

        let seq_base = 12;
        let str1 = String::from("hello");
        let str2 = String::from(" world");
        assert!(str1.len() + str2.len() < WINDOW_SIZE);
        let mut send1 = mkseg(&tcb);
        send1.set_seq(seq_base);
        send1.set_data(str1.clone().into_bytes());

        let mut send2 = mkseg(&tcb);
        send2.set_seq(seq_base + send1.payload().len() as u32);
        send2.set_data(str2.clone().into_bytes());

        tx.send(send2).unwrap();
        tx.send(send1).unwrap();
        let data = tcb.sr_receiver(str1.len() as u32, &rx).unwrap();
        assert_eq!(String::from_utf8(data).unwrap(), str1);
        let ack: Segment = sock_recv(&recv_socket);
        assert!(ack.get_flag(Flag::ACK));
        assert_eq!(ack.ack_num(), seq_base + (str1.len() + str2.len()) as u32);

        let data = tcb.sr_receiver(str2.len() as u32, &rx).unwrap();
        assert_eq!(String::from_utf8(data).unwrap(), str2);
    }

    #[test]
    fn sr_receiver_long() {
        let mut tcb = tests::default_tcb();
        tcb.ack_base = 12;
        let (tx, rx) = channel();

        let seq_base = 12;

        let str1 = String::from(
            "Did you ever hear the tragedy of Darth Plagueis The Wise? I thought not. It’s not a story the Jedi would tell you. It’s a Sith legend. Darth Plagueis was a Dark Lord of the Sith, so powerful and so wise he could use the Force to influence the midichlorians to create life… He had such a knowledge of the dark side that he could even keep the ones he cared about from dying. The dark side of the Force is a pathway to many abilities some consider to be unnatural. He became so powerful… the only thing he was afraid of was losing his power, which eventually, of course, he did. Unfortunately, he taught his apprentice everything he knew, then his apprentice killed him in his sleep. Ironic. He could save others from death, but not himself.",
        );
        let mut bytes = str1.clone().into_bytes();

        let mut next_seq = seq_base;
        while !bytes.is_empty() {
            let mut seg = mkseg(&tcb);
            seg.set_seq(next_seq);
            let len = min(bytes.len(), MAX_PAYLOAD_SIZE) as usize;
            let payload = Vec::from(&bytes[0..len]);
            bytes = bytes.split_off(len);
            seg.set_data(payload);
            next_seq += len as u32;
            tx.send(seg).unwrap();
        }

        let data = tcb.sr_receiver(str1.len() as u32, &rx).unwrap();
        assert_eq!(String::from_utf8(data).unwrap(), str1);
    }
}
