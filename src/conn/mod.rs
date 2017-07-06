use std::net::*;
use segment::*;
use std::time::SystemTime;

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

use std::sync::mpsc::Receiver;
#[derive(Debug)]
pub struct TCB {
    pub state: TCBState,
    pub tuple: TCPTuple,
    pub timeout: SystemTime,
    pub sock: UdpSocket,
}

impl TCB {
    pub fn new(tuple: TCPTuple, sock: UdpSocket) -> TCB {
        TCB {
            state: TCBState::Listen,
            tuple: tuple,
            timeout: SystemTime::now(),
            sock: sock,
        }
    }

    pub fn send(&mut self, payload: Vec<u8>) {
        // TODO
    }

    pub fn recv(&mut self, size: u32) -> Vec<u8> {
        // TODO
        Vec::new()
    }

    pub fn reset(&mut self) {
        self.state = TCBState::Listen;
    }

    fn next_seg(&mut self) -> Segment {
        let seg = Segment::new(self.tuple.src.port(), self.tuple.dst.port());
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
        match self.state {
            TCBState::Listen => {
                if seg.get_flag(Flag::SYN) {
                    self.state = TCBState::SynRecd;
                    let mut synack = self.next_seg();
                    synack.set_flag(Flag::SYN);
                    synack.set_flag(Flag::ACK);
                    self.send_seg(&synack);
                }
            }
            TCBState::SynRecd => {
                if seg.get_flag(Flag::ACK) {
                    self.state = TCBState::Estab;
                }
            }
            TCBState::Estab => {
                if seg.get_flag(Flag::FIN) {
                    self.state = TCBState::Closed;
                    let mut ack = self.next_seg();
                    ack.set_flag(Flag::ACK);
                    self.send_seg(&ack);
                }
            }
            TCBState::SynSent => {
                if seg.get_flag(Flag::ACK) && seg.get_flag(Flag::SYN) {
                    let mut ack = self.next_seg();
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

    fn default_tcb() -> TCB {
        let server_sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let client_sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let tuple = TCPTuple::from(
            server_sock.local_addr().unwrap(),
            client_sock.local_addr().unwrap(),
        );
        TCB::new(tuple, server_sock)
    }

    #[test]
    fn tcb() {
        let tcb = tests::default_tcb();
        assert_eq!(tcb.state, TCBState::Listen);
    }

    #[test]
    fn handshake() {
        let mut tcb = tests::default_tcb();
        let server_addr = tcb.tuple.src;
        let client_addr = tcb.tuple.dst;
        let client_sock = UdpSocket::bind(client_addr).unwrap();

        let recv = || {
            let mut buf = vec![0; (1 << 16) - 1];
            let (amt, _) = client_sock.recv_from(&mut buf).unwrap();
            let buf = Vec::from(&mut buf[..amt]);
            Segment::from_buf(buf)
        };

        let mut syn = Segment::new(client_addr.port(), server_addr.port());
        syn.set_flag(Flag::SYN);
        tcb.handle_shake(syn);
        assert_eq!(tcb.state, TCBState::SynRecd);
        let answer: Segment = recv();
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
        let answer: Segment = recv();
        assert!(answer.get_flag(Flag::ACK));

        tcb.send_syn();
        assert_eq!(tcb.state, TCBState::SynSent);
        let answer: Segment = recv();
        assert!(answer.get_flag(Flag::SYN));

        let mut synack = Segment::new(client_addr.port(), server_addr.port());
        synack.set_seq(1);
        synack.set_flag(Flag::ACK);
        synack.set_flag(Flag::SYN);
        tcb.handle_shake(synack);
        assert_eq!(tcb.state, TCBState::Estab);

        let answer: Segment = recv();
        assert!(answer.get_flag(Flag::ACK));

        tcb.close();
        let answer: Segment = recv();
        assert!(answer.get_flag(Flag::FIN));
    }
}
