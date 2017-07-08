use segment::*;
use std::net::*;
use std::sync::{Arc, Mutex};
use std::sync::mpsc::*;
use std::collections::VecDeque;
use std::cmp::*;

const WINDOW_SIZE: usize = 5;
const MAX_PAYLOAD_SIZE: usize = 2;

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
pub struct TCB {
    pub tuple: TCPTuple,
    pub state: TCBState,
    pub socket: UdpSocket,
    pub seg_input: Receiver<Segment>,
    pub send_input: Receiver<Vec<u8>>,
    pub recv_output: Sender<u8>,

    pub send_buffer: VecDeque<u8>, // Data to be sent that hasn't been
    pub send_window: VecDeque<u8>,

    pub seq_base: u32,
    pub ack_base: u32,

    pub unacked_segs: VecDeque<Segment>,
}

pub enum TCBInput {
    Receive(Segment),
    Send(Vec<u8>),
}

impl TCB {
    pub fn new(
        tuple: TCPTuple,
        udp_sock: UdpSocket,
    ) -> (TCB, Sender<Segment>, Sender<Vec<u8>>, Receiver<u8>) {
        let (seg_input_tx, seg_input_rx) = channel();
        let (send_input_tx, send_input_rx) = channel();
        let (recv_output_tx, recv_output_rx) = channel();
        (
            TCB {
                tuple: tuple,
                state: TCBState::Listen,
                socket: udp_sock,
                seg_input: seg_input_rx,
                send_input: send_input_rx,
                recv_output: recv_output_tx,

                send_buffer: VecDeque::new(),
                send_window: VecDeque::new(),

                seq_base: 1,
                ack_base: 1,

                unacked_segs: VecDeque::new(),
            },
            seg_input_tx,
            send_input_tx, // Call send_tx.send() to send bytes over tcp
            recv_output_rx, // Call recv_rx.recv() to get the bytes sent over tcp
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
            let mut syn = self.make_seg();
            syn.set_flag(Flag::SYN);
            syn.set_seq(self.seq_base);
            self.unacked_segs.push_back(syn);
        }
        while self.state != TCBState::Closed {
            let seg = self.seg_input.recv().unwrap(); // TODO: Deal with timeout
            self.handle_shake(&seg);
            // Handle ack including handling syn retransmit when timeout
        }
    }

    fn send_syn(&mut self) {}

    fn handle_shake(&mut self, seg: &Segment) {
        match self.state {
            TCBState::Listen => {
                if seg.get_flag(Flag::SYN) {
                    self.state = TCBState::SynRecd;
                    self.ack_base = seg.seq_num() + 1;
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
                    self.ack_base = seg.seq_num() + 1;
                    assert_eq!(seg.ack_num(), self.seq_base + 1); // TODO: What do here?
                    let mut ack = self.make_seg();
                    ack.set_flag(Flag::ACK);
                    ack.set_ack_num(self.ack_base);
                    self.send_ack(ack);
                }
            }
            TCBState::SynRecd => {
                // TODO: Verify what seq num should be here
                if seg.get_flag(Flag::ACK) {
                    self.state = TCBState::Estab;
                }
            }
            TCBState::Estab => {}
            TCBState::Closed => {}
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
