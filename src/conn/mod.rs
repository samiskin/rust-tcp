use std::net::*;
use segment::*;
use std::time::SystemTime;

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum TCBState {
    LISTEN,
    SYN_SENT,
    SYN_RECD,
    ESTAB,
    CLOSED,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct TCPTuple {
    pub src_port: u16,
    pub dst_ip: IpAddr,
    pub dst_port: u16,
}

impl TCPTuple {
    pub fn from(src_port: u16, dst: &SocketAddr) -> TCPTuple {
        TCPTuple {
            dst_ip: dst.ip(),
            dst_port: dst.port(),
            src_port: src_port,
        }
    }
}

static mut TCB_COUNT: u32 = 0;

#[derive(Debug)]
pub struct TCB {
    pub id: u32,
    pub state: TCBState,
    pub tuple: TCPTuple,
    pub expected_seq: u32,
    pub timeout: SystemTime,
    pub sock: UdpSocket,
}

impl TCB {
    pub fn new(tuple: TCPTuple, sock: UdpSocket) -> TCB {
        unsafe {
            TCB_COUNT += 1;
        }
        TCB {
            id: unsafe { TCB_COUNT },
            state: TCBState::LISTEN,
            tuple: tuple,
            expected_seq: 0,
            timeout: SystemTime::now(),
            sock: sock,
        }
    }

    pub fn target_addr(&self) -> SocketAddr {
        SocketAddr::new(self.tuple.dst_ip, self.tuple.dst_port)
    }

    pub fn reset(&mut self) {
        unsafe {
            TCB_COUNT += 1;
        }
        self.id = unsafe { TCB_COUNT };
        self.state = TCBState::LISTEN;
    }

    fn next_seg(&mut self) -> Segment {
        let mut seg = Segment::new(self.tuple.dst_port, self.tuple.src_port);
        seg.seq_num = self.expected_seq;
        self.expected_seq += 1;
        seg
    }

    fn send_seg(&self, seg: &Segment) {
        let bytes = seg.to_byte_vec();
        let target = self.target_addr();
        self.sock.send_to(&bytes[..], &target).unwrap();
    }

    pub fn check_timeout(&mut self) {}

    pub fn open(&mut self) {
        self.state = TCBState::LISTEN;
    }

    pub fn close(&mut self) {
        let mut fin = self.next_seg();
        fin.set_flag(Flag::FIN);
        self.send_seg(&fin);
        self.state = TCBState::CLOSED;
    }

    pub fn send_syn(&mut self) {
        let mut syn = self.next_seg();
        syn.set_flag(Flag::SYN);
        self.send_seg(&syn);
        self.state = TCBState::SYN_SENT;
    }

    pub fn handle_segment(&mut self, seg: Segment) {
        match self.state {
            TCBState::LISTEN => {
                if seg.get_flag(Flag::SYN) {
                    println!("SYN");
                    self.state = TCBState::SYN_RECD;
                    let mut synack = self.next_seg();
                    synack.set_flag(Flag::SYN);
                    synack.set_flag(Flag::ACK);
                    synack.ack_num = self.expected_seq;
                    self.send_seg(&synack);
                }
            }
            TCBState::SYN_RECD => {
                if seg.seq_num != self.expected_seq {
                    println!("actual {} != expected {}", seg.seq_num, self.expected_seq);
                }
                if seg.get_flag(Flag::ACK) && seg.seq_num == self.expected_seq {
                    println!("ESTAB!");
                    self.expected_seq = seg.seq_num + 1;
                    self.state = TCBState::ESTAB;
                }
            }
            TCBState::ESTAB => {
                if seg.get_flag(Flag::FIN) {
                    self.state = TCBState::CLOSED;
                    let mut ack = self.next_seg();
                    ack.set_flag(Flag::ACK);
                    ack.ack_num = self.expected_seq;
                    self.send_seg(&ack);
                    println!("CLOSED!");
                }
            }
            TCBState::SYN_SENT => {
                if seg.get_flag(Flag::ACK) && seg.get_flag(Flag::SYN) {
                    let mut ack = self.next_seg();
                    ack.set_flag(Flag::ACK);
                    ack.ack_num = self.expected_seq;
                    self.send_seg(&ack);
                    self.state = TCBState::ESTAB;
                }
            }
            TCBState::CLOSED => {}
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn tcb() {
        let s = Segment::new(0, 0);
        let dst = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 8080);
        let tuple = TCPTuple::from(0, &dst);
        let tcb = TCB::new(tuple, UdpSocket::bind("0.0.0.0:0").unwrap());
        let tcb2 = TCB::new(tuple, UdpSocket::bind("0.0.0.0:0").unwrap());
        assert_ne!(tcb2.id, tcb.id);

        assert_eq!(tcb.state, TCBState::LISTEN);
    }

    #[test]
    fn connection_management() {
        let client_sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        client_sock
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let client_addr = client_sock.local_addr().unwrap();
        let server_port = 0;
        let tuple = TCPTuple::from(server_port, &client_addr);
        let mut tcb = TCB::new(tuple, client_sock.try_clone().unwrap());

        let recv = || {
            let mut buf = vec![0; (1 << 16) - 1];
            let (amt, _) = client_sock.recv_from(&mut buf).unwrap();
            let buf = Vec::from(&mut buf[..amt]);
            Segment::from_buf(buf)
        };

        let mut syn = Segment::new(client_addr.port(), server_port);
        syn.set_flag(Flag::SYN);
        tcb.handle_segment(syn);
        assert_eq!(tcb.state, TCBState::SYN_RECD);
        let answer: Segment = recv();
        assert!(answer.get_flag(Flag::ACK));
        assert!(answer.get_flag(Flag::SYN));

        let mut ack = Segment::new(client_addr.port(), server_port);
        ack.seq_num = 1;
        ack.set_flag(Flag::ACK);
        tcb.handle_segment(ack);
        assert_eq!(tcb.state, TCBState::ESTAB);

        let mut fin = Segment::new(client_addr.port(), server_port);
        fin.seq_num = 2;
        fin.set_flag(Flag::FIN);
        tcb.handle_segment(fin);
        assert_eq!(tcb.state, TCBState::CLOSED);
        let answer: Segment = recv();
        assert!(answer.get_flag(Flag::ACK));

        tcb.send_syn();
        assert_eq!(tcb.state, TCBState::SYN_SENT);
        let answer: Segment = recv();
        assert!(answer.get_flag(Flag::SYN));

        let mut synack = Segment::new(client_addr.port(), server_port);
        synack.seq_num = 1;
        synack.set_flag(Flag::ACK);
        synack.set_flag(Flag::SYN);
        tcb.handle_segment(synack);
        assert_eq!(tcb.state, TCBState::ESTAB);

        let answer: Segment = recv();
        assert!(answer.get_flag(Flag::ACK));

        tcb.close();
        let answer: Segment = recv();
        assert!(answer.get_flag(Flag::FIN));
    }
}
