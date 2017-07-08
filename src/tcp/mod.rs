use segment::*;
use std::net::*;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::*;
use std::collections::VecDeque;
use std::cmp::*;
use std::time::Duration;
use utils::*;

const WINDOW_SIZE: usize = 5;
const MAX_PAYLOAD_SIZE: usize = 2;
const TIMEOUT: u64 = 1; // In seconds

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


#[derive(Debug)]
pub enum TCBInput {
    Receive(Segment),
    Send(Vec<u8>),
}

#[derive(Debug)]
pub struct TCB {
    pub tuple: TCPTuple,
    pub state: TCBState,
    pub socket: UdpSocket,
    pub data_input: Receiver<TCBInput>,
    pub byte_output: Sender<u8>,

    pub send_buffer: VecDeque<u8>, // Data to be sent that hasn't been
    pub send_window: VecDeque<u8>,

    pub seq_base: u32,
    pub ack_base: u32,

    pub unacked_segs: VecDeque<Segment>,
}

impl TCB {
    pub fn new(tuple: TCPTuple, udp_sock: UdpSocket) -> (TCB, Sender<TCBInput>, Receiver<u8>) {
        let (data_input_tx, data_input_rx) = channel();
        let (byte_output_tx, byte_output_rx) = channel();
        (
            TCB {
                tuple: tuple,
                state: TCBState::Listen,
                socket: udp_sock,
                data_input: data_input_rx,
                byte_output: byte_output_tx,

                send_buffer: VecDeque::new(),
                send_window: VecDeque::new(),

                seq_base: 1,
                ack_base: 1,

                unacked_segs: VecDeque::new(),
            },
            data_input_tx,
            byte_output_rx,
        )
    }

    fn fill_window(&mut self) -> Vec<u8> {
        let buffer = &mut self.send_buffer;
        let window = &mut self.send_window;
        let fill_amt = min(buffer.len(), WINDOW_SIZE - window.len());
        let mut data_to_send = Vec::with_capacity(fill_amt);
        window.extend(buffer.iter().take(fill_amt));
        for i in 0..fill_amt {
            data_to_send.push(buffer.pop_front().unwrap());
        }
        data_to_send
    }

    fn run_tcp(&mut self, send_syn: bool) {
        if send_syn {
            self.send_syn();
        }
        'event_loop: while self.state != TCBState::Closed {
            self.handle_input_recv();
        }
    }

    fn send_syn(&mut self) {
        let mut syn = self.make_seg();
        syn.set_flag(Flag::SYN);
        syn.set_seq(self.seq_base);
        self.send_seg(syn);
        self.state = TCBState::SynSent;
    }

    fn handle_input_recv(&mut self) {
        match self.data_input.recv_timeout(Duration::from_secs(TIMEOUT)) {
            Ok(input) => {
                match input {
                    TCBInput::Receive(seg) => self.handle_seg(seg),
                    TCBInput::Send(data) => self.handle_data(data),
                }
            }
            Err(e) if e == RecvTimeoutError::Timeout => {
                self.handle_timeout();
            }
            Err(e) => panic!(e),
        }
    }

    fn handle_data(&mut self, data: Vec<u8>) {}
    fn handle_seg(&mut self, seg: Segment) {
        // handle FIN
        if seg.get_flag(Flag::FIN) {
            self.state = TCBState::Closed;
            return;
        }
        self.handle_acks(&seg);
        self.handle_shake(&seg);
        self.handle_payload(&seg);
    }

    fn handle_payload(&self, seg: &Segment) {}
    fn handle_acks(&mut self, seg: &Segment) {
        let ack_lb = self.seq_base.wrapping_add(1);
        let ack_ub = ack_lb.wrapping_add(WINDOW_SIZE as u32);
        if seg.get_flag(Flag::ACK) && in_wrapped_range((ack_lb, ack_ub), seg.ack_num()) {
            self.seq_base = seg.ack_num();
            self.unacked_segs.retain(|unacked_seg: &Segment| {
                unacked_seg.seq_num() >= seg.ack_num()
            });

            // Handle payload data, only valid after Estab
            if self.state == TCBState::Estab {}
        }
    }

    fn handle_shake(&mut self, seg: &Segment) {
        match self.state {
            TCBState::Listen => {
                if seg.get_flag(Flag::SYN) {
                    self.state = TCBState::SynRecd;
                    self.ack_base = seg.seq_num().wrapping_add(1);
                    let mut synack = self.make_seg();
                    synack.set_flag(Flag::SYN);
                    synack.set_flag(Flag::ACK);
                    synack.set_seq(self.seq_base);
                    synack.set_ack_num(self.ack_base);
                    self.send_seg(synack);
                }
            }
            TCBState::SynSent => {
                if seg.get_flag(Flag::SYN) && seg.get_flag(Flag::ACK) {
                    self.state = TCBState::Estab;
                    self.ack_base = seg.seq_num().wrapping_add(1);
                    let mut ack = self.make_seg();
                    ack.set_flag(Flag::ACK);
                    ack.set_ack_num(self.ack_base);
                    self.send_ack(ack);
                    println!("Client ESTAB");
                }
            }
            TCBState::SynRecd => {
                // TODO: Verify what seq num should be here
                if seg.get_flag(Flag::ACK) {
                    self.state = TCBState::Estab;
                    println!("Server ESTAB");
                }
            }
            TCBState::Estab => {}
            TCBState::Closed => {}
        }
    }


    fn handle_timeout(&mut self) {
        match self.unacked_segs.front() {
            Some(ref seg) => self.resend_seg(&seg),
            _ => {}
        }
    }

    fn make_seg(&self) -> Segment {
        Segment::new(self.tuple.src.port(), self.tuple.dst.port())
    }

    fn send_seg(&mut self, seg: Segment) {
        assert!(seg.seq_num() >= self.seq_base);
        self.resend_seg(&seg);
        self.unacked_segs.push_back(seg);
    }

    fn send_ack(&self, seg: Segment) {
        self.resend_seg(&seg);
    }

    fn resend_seg(&self, seg: &Segment) {
        let bytes = seg.to_byte_vec();
        self.socket.send_to(&bytes[..], &self.tuple.dst).unwrap();
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    type TcbTup = (TCB, Sender<TCBInput>, Receiver<u8>);
    fn tcb_pair() -> (TcbTup, TcbTup, UdpSocket, UdpSocket) {
        let server_sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let client_sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let server_tuple = TCPTuple {
            src: server_sock.local_addr().unwrap(),
            dst: client_sock.local_addr().unwrap(),
        };
        let client_tuple = TCPTuple {
            src: client_sock.local_addr().unwrap(),
            dst: server_sock.local_addr().unwrap(),
        };
        let server_tuple = TCB::new(server_tuple, server_sock.try_clone().unwrap());
        let client_tuple = TCB::new(client_tuple, client_sock.try_clone().unwrap());
        (server_tuple, client_tuple, server_sock, client_sock)
    }

    fn sock_recv(sock: &UdpSocket) -> Segment {
        let mut buf = vec![0; (1 << 16) - 1];
        let (amt, _) = sock.recv_from(&mut buf).unwrap();
        let buf = Vec::from(&mut buf[..amt]);
        Segment::from_buf(buf)
    }

    #[test]
    fn full_handshake() {
        let (server_tuple, client_tuple, server_sock, client_sock) = tcb_pair();
        let (mut server_tcb, server_input, _) = server_tuple;
        let (mut client_tcb, client_input, _) = client_tuple;

        let server_thread = thread::spawn(move || {
            server_tcb.handle_input_recv();
            assert_eq!(server_tcb.state, TCBState::SynRecd);
            server_tcb.handle_input_recv();
            assert_eq!(server_tcb.state, TCBState::Estab);
        });

        let client_thread = thread::spawn(move || {
            client_tcb.send_syn();
            assert_eq!(client_tcb.state, TCBState::SynSent);
            client_tcb.handle_input_recv();
            assert_eq!(client_tcb.state, TCBState::Estab);
        });

        let client_syn: Segment = sock_recv(&server_sock);
        assert!(client_syn.get_flag(Flag::SYN));
        server_input.send(TCBInput::Receive(client_syn)).unwrap();

        let server_synack: Segment = sock_recv(&client_sock);
        assert!(server_synack.get_flag(Flag::SYN));
        assert!(server_synack.get_flag(Flag::ACK));
        client_input.send(TCBInput::Receive(server_synack)).unwrap();

        let client_ack: Segment = sock_recv(&server_sock);
        assert!(client_ack.get_flag(Flag::ACK));
        server_input.send(TCBInput::Receive(client_ack)).unwrap();

        server_thread.join().unwrap();
        client_thread.join().unwrap();
    }
}


/*
recv_buffer: MessageQueue<u8>
send_buffer: Box<[u8]>

recv() {
   take from recv buffer, block if empty
}

send() {
   append to send buffer
   send_until_window_full()
}

TCPThread

while state != CLOSED {
   seg = recv()
   if seg(SYN)
      setup ack_base, seq_base

// Handle handshake
   run handshake state machine

// Handle send
   if seg(ACK) && acknum in send_range
   {
      move send_window (taking from send_buffer)
      move send_base
   }

// Handle Receive
   if seq in receive_range {
      fill receive_window
   }

   if seq == ack_base {
      put completed segments in receive buffer
      move ack_base
      move window

      if payload.size > 0{
         send ack with ack_base
         TODO delayed ack
      }
   }
}
*/
