use segment::*;
use std::net::*;
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
    SendSyn,
    Receive(Segment),
    Send(Vec<u8>),
    Close,
}

#[derive(Debug)]
pub struct TCB {
    tuple: TCPTuple,
    state: TCBState,
    socket: UdpSocket,
    data_input: Receiver<TCBInput>,
    byte_output: Sender<u8>,

    send_buffer: VecDeque<u8>, // Data to be sent that hasn't been
    send_window: VecDeque<u8>,
    recv_window: VecDeque<Option<u8>>,

    seq_base: u32,
    ack_base: u32,

    unacked_segs: VecDeque<Segment>,
    dupe_acks: u32,
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
                recv_window: VecDeque::from(vec![Option::None; WINDOW_SIZE]),

                seq_base: 1,
                ack_base: 1,

                unacked_segs: VecDeque::new(),
                dupe_acks: 0,
            },
            data_input_tx,
            byte_output_rx,
        )
    }

    pub fn recv(out: &Receiver<u8>, amt: u32) -> Result<Vec<u8>, RecvError> {
        let mut buf = Vec::with_capacity(amt as usize);
        while buf.len() < amt as usize {
            buf.push(out.recv()?);
        }
        Ok(buf)
    }

    pub fn run_tcp(&mut self) {
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
                    TCBInput::SendSyn => self.send_syn(),
                    TCBInput::Receive(seg) => self.handle_seg(seg),
                    TCBInput::Send(data) => {
                        self.send_buffer.extend(data);
                        self.fill_send_window();
                    }
                    TCBInput::Close => {
                        self.send_close();
                    }
                }
            }
            Err(e) if e == RecvTimeoutError::Timeout => {
                self.handle_resend();
            }
            Err(e) => panic!(e),
        }
    }

    fn fill_send_window(&mut self) {
        if self.state != TCBState::Estab {
            return;
        }
        let orig_window_len = self.send_window.len();
        let send_amt = min(self.send_buffer.len(), WINDOW_SIZE - orig_window_len);
        self.send_window.extend(
            self.send_buffer.iter().take(send_amt),
        );
        let data_to_send = self.send_buffer.drain(..send_amt).collect::<Vec<u8>>();
        let next_seq = self.seq_base.wrapping_add(orig_window_len as u32);
        self.send_data(data_to_send, next_seq);
    }

    fn send_data(&mut self, mut data: Vec<u8>, next_seq: u32) {
        let mut sent = 0;
        let bytes_to_send = data.len();
        while sent < bytes_to_send {
            let size = min(MAX_PAYLOAD_SIZE, data.len());
            let payload: Vec<u8> = data.drain(..size).collect();
            let mut seg = self.make_seg();
            seg.set_seq(next_seq.wrapping_add(sent as u32));
            seg.set_data(payload);
            self.send_seg(seg);
            sent += size;
        }
    }

    fn handle_seg(&mut self, seg: Segment) {
        self.handle_acks(&seg); // sender
        self.handle_shake(&seg);
        self.handle_payload(&seg); // receiver
        if seg.get_flag(Flag::FIN) {
            self.handle_close();
        }
    }

    fn handle_payload(&mut self, seg: &Segment) {
        if self.state != TCBState::Estab {
            return;
        }
        let seq_lb = self.ack_base;
        let seq_ub = seq_lb.wrapping_add(WINDOW_SIZE as u32);
        if in_wrapped_range((seq_lb, seq_ub), seg.seq_num()) {
            let window_index_base = seg.seq_num().wrapping_sub(self.ack_base) as usize;
            for (i, byte) in seg.payload().iter().enumerate() {
                self.recv_window[window_index_base + i] = Some(*byte);
            }
        } else if seg.payload().len() > 0 {
            println!(
                "\x1b[31m Seq {} out of range (expected {}), ignoring\x1b[0m",
                seg.seq_num(),
                self.ack_base
            );
        }

        if seg.seq_num() == self.ack_base {
            loop {
                match self.recv_window.front() {
                    Some(opt) => {
                        match *opt {
                            Some(byte) => self.byte_output.send(byte).unwrap(),
                            None => break,
                        }
                    }
                    _ => break,
                }
                self.ack_base = self.ack_base.wrapping_add(1);
                self.recv_window.pop_front();
                self.recv_window.push_back(None);
            }
            let mut ack = self.make_seg();
            ack.set_flag(Flag::ACK);
            ack.set_ack_num(self.ack_base);
            // TODO: Delayed ack
            self.send_ack(ack);
        } else if !seg.get_flag(Flag::ACK) {
            println!(
                "\x1b[32m Out of order Seq {} (expected {}), ignoring\x1b[0m",
                seg.seq_num(),
                self.ack_base
            );
        }
    }

    fn handle_acks(&mut self, seg: &Segment) {
        let ack_lb = self.seq_base.wrapping_add(1);
        let ack_ub = ack_lb.wrapping_add(WINDOW_SIZE as u32);
        if seg.get_flag(Flag::ACK) && in_wrapped_range((ack_lb, ack_ub), seg.ack_num()) {
            self.unacked_segs.retain(|unacked_seg: &Segment| {
                in_wrapped_range(
                    (
                        seg.ack_num(),
                        seg.ack_num().wrapping_add(WINDOW_SIZE as u32),
                    ),
                    unacked_seg.seq_num(),
                )
            });

            let num_acked_bytes = seg.ack_num().wrapping_sub(self.seq_base) as usize;
            self.seq_base = seg.ack_num();

            // Handle payload data, only valid after Estab
            if self.state == TCBState::Estab {
                self.send_window.drain(..num_acked_bytes);
                self.fill_send_window();
            }

        }

        let dupe_ack_lb = self.seq_base.wrapping_sub((WINDOW_SIZE - 1) as u32);
        let dupe_ack_ub = dupe_ack_lb.wrapping_add(WINDOW_SIZE as u32);
        if self.state == TCBState::Estab && seg.get_flag(Flag::ACK) &&
            in_wrapped_range((dupe_ack_lb, dupe_ack_ub), seg.seq_num())
        {
            self.dupe_acks += 1;
            if self.dupe_acks >= 3 {
                self.handle_resend();
                self.dupe_acks = 0;
                println!("\x1b[31m Triple Duplicate ACK! Resending \x1b[0m");
            }
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
                    self.fill_send_window();
                }
            }
            TCBState::SynRecd => {
                // TODO: Verify what seq num should be here
                if seg.get_flag(Flag::ACK) {
                    self.state = TCBState::Estab;
                    self.fill_send_window();
                }
            }
            TCBState::Estab => {}
            TCBState::Closed => {}
        }
    }


    fn handle_close(&mut self) {
        self.state = TCBState::Closed;
    }

    fn send_close(&mut self) {
        let mut fin = self.make_seg();
        fin.set_flag(Flag::FIN);
        fin.set_seq(self.seq_base);
        self.send_seg(fin);
        self.state = TCBState::Closed;
    }

    fn handle_resend(&mut self) {
        match self.unacked_segs.front() {
            Some(ref seg) => self.resend_seg(&seg),
            _ => {}
        }
    }

    fn make_seg(&self) -> Segment {
        Segment::new(self.tuple.src.port(), self.tuple.dst.port())
    }

    fn send_seg(&mut self, seg: Segment) {
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
pub mod tests {
    use super::*;
    use std::thread;

    type TcbTup = (TCB, Sender<TCBInput>, Receiver<u8>);
    pub fn tcb_pair() -> (TcbTup, TcbTup, UdpSocket, UdpSocket) {
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
        buf.truncate(amt);
        Segment::from_buf(buf)
    }

    pub fn perform_handshake(
        server_tuple: &mut TcbTup,
        client_tuple: &mut TcbTup,
        server_sock: &UdpSocket,
        client_sock: &UdpSocket,
    ) {
        let (ref mut server_tcb, ref server_input, _) = *server_tuple;
        let (ref mut client_tcb, ref client_input, _) = *client_tuple;

        client_input.send(TCBInput::SendSyn).unwrap();
        client_tcb.handle_input_recv();
        assert_eq!(client_tcb.state, TCBState::SynSent);

        let client_syn: Segment = sock_recv(&server_sock);
        assert!(client_syn.get_flag(Flag::SYN));
        server_input.send(TCBInput::Receive(client_syn)).unwrap();
        server_tcb.handle_input_recv();
        assert_eq!(server_tcb.state, TCBState::SynRecd);

        let server_synack: Segment = sock_recv(&client_sock);
        assert!(server_synack.get_flag(Flag::SYN));
        assert!(server_synack.get_flag(Flag::ACK));
        client_input.send(TCBInput::Receive(server_synack)).unwrap();
        client_tcb.handle_input_recv();
        assert_eq!(client_tcb.state, TCBState::Estab);

        let client_ack: Segment = sock_recv(&server_sock);
        assert!(client_ack.get_flag(Flag::ACK));
        server_input.send(TCBInput::Receive(client_ack)).unwrap();
        server_tcb.handle_input_recv();
        assert_eq!(server_tcb.state, TCBState::Estab);
    }

    #[test]
    fn full_handshake() {
        let (mut server_tuple, mut client_tuple, server_sock, client_sock) = tcb_pair();
        perform_handshake(
            &mut server_tuple,
            &mut client_tuple,
            &server_sock,
            &client_sock,
        );
    }

    #[test]
    fn handshake_retransmit() {
        let (server_tuple, client_tuple, server_sock, client_sock) = tcb_pair();
        let (mut client_tcb, client_input, _) = client_tuple;
        let (mut server_tcb, server_input, _) = server_tuple;
        let client_thread = thread::spawn(move || {
            client_tcb.send_syn();
            while client_tcb.state != TCBState::Estab {
                client_tcb.handle_input_recv();
            }
        });
        sock_recv(&server_sock);
        let client_syn: Segment = sock_recv(&server_sock); // Wait for second send
        assert!(client_syn.get_flag(Flag::SYN));
        server_input.send(TCBInput::Receive(client_syn)).unwrap();
        server_tcb.handle_input_recv();
        let server_synack: Segment = sock_recv(&client_sock);
        client_input.send(TCBInput::Receive(server_synack)).unwrap();

        client_thread.join().unwrap();
    }

    #[test]
    fn send_test() {
        let (mut server_tuple, mut client_tuple, server_sock, client_sock) = tcb_pair();
        server_tuple.0.seq_base = u32::max_value() - 2; // Test wrapping around u32 boundaries
        perform_handshake(
            &mut server_tuple,
            &mut client_tuple,
            &server_sock,
            &client_sock,
        );

        let (client_tcb, _client_input, _) = client_tuple;
        let (mut server_tcb, server_input, _) = server_tuple;

        let data_len = WINDOW_SIZE;
        let str = String::from("Ok");
        assert!(str.len() <= MAX_PAYLOAD_SIZE);
        let mut data = (0..data_len).map(|r| r as u8).collect::<Vec<u8>>();
        data.extend(str.clone().into_bytes());
        server_input.send(TCBInput::Send(data)).unwrap();
        server_tcb.handle_input_recv();

        let mut segments = vec![];
        for _ in 0..((data_len as f64 / MAX_PAYLOAD_SIZE as f64).ceil() as u32) {
            segments.push(sock_recv(&client_sock));
        }

        let mut ack = client_tcb.make_seg();
        ack.set_flag(Flag::ACK);
        ack.set_ack_num(segments[1].seq_num());
        server_input.send(TCBInput::Receive(ack)).unwrap();
        server_tcb.handle_input_recv();
        let next_seg = sock_recv(&client_sock);
        assert_eq!(next_seg.payload(), str.into_bytes());

        // Check for Duplicate ACK resends
        for _ in 0..5 {
            let mut ack = client_tcb.make_seg();
            ack.set_flag(Flag::ACK);
            ack.set_ack_num(segments[1].seq_num());
            server_input.send(TCBInput::Receive(ack)).unwrap();
            server_tcb.handle_input_recv();
        }

        let next_seg = sock_recv(&client_sock);
        assert_eq!(next_seg.seq_num(), segments[1].seq_num());

        let next_seg = sock_recv(&client_sock);
        assert_eq!(next_seg.seq_num(), segments[1].seq_num());
    }

    pub fn run_e2e_pair<F1, F2>(
        server_fn: F1,
        client_fn: F2,
    ) -> ((Sender<TCBInput>, Receiver<u8>, UdpSocket), (Sender<TCBInput>, Receiver<u8>, UdpSocket))
    where
        F1: FnOnce(TCB) -> () + Send + 'static,
        F2: FnOnce(TCB) -> () + Send + 'static,
    {
        let (server_tuple, client_tuple, server_sock, client_sock) = tcb_pair();
        let (server_tcb, server_input, server_output) = server_tuple;
        let (client_tcb, client_input, client_output) = client_tuple;

        let server_client_sender = client_input.clone();
        let server_client_sock = client_sock.try_clone().unwrap();
        let _server_message_passer = thread::spawn(move || loop {
            let seg = sock_recv(&server_client_sock);
            // println!("\x1b[33m Server->Client {:?} \x1b[0m", seg);
            if server_client_sender.send(TCBInput::Receive(seg)).is_err() {
                break;
            }
        });

        let client_server_sender = server_input.clone();
        let client_server_sock = server_sock.try_clone().unwrap();
        let _client_message_passer = thread::spawn(move || loop {
            let seg = sock_recv(&client_server_sock);
            // println!("\x1b[35m Client->Server {:?} \x1b[0m", seg);
            if client_server_sender.send(TCBInput::Receive(seg)).is_err() {
                break;
            }
        });

        let _server = thread::spawn(move || server_fn(server_tcb));
        let _client = thread::spawn(move || client_fn(client_tcb));

        ((server_input, server_output, server_sock), (
            client_input,
            client_output,
            client_sock,
        ))
    }

    #[test]
    fn send_recv_test() {
        let ((server_input, _, _), (client_input, client_output, _)) =
            run_e2e_pair(
                |mut server_tcb: TCB| server_tcb.run_tcp(),
                |mut client_tcb: TCB| client_tcb.run_tcp(),
            );

        client_input.send(TCBInput::SendSyn).unwrap();

        let text = String::from("Did you ever hear the tragedy of Darth Plagueis the wise?");
        let data = text.clone().into_bytes();
        server_input.send(TCBInput::Send(data)).unwrap();

        let mut buf = vec![];
        while buf.len() < text.len() {
            buf.extend(client_output.recv());
        }

        assert_eq!(String::from_utf8(buf).unwrap(), text);
    }

    #[test]
    fn close_test() {
        let (mut server_tuple, mut client_tuple, server_sock, client_sock) = tcb_pair();
        perform_handshake(
            &mut server_tuple,
            &mut client_tuple,
            &server_sock,
            &client_sock,
        );

        let (mut server_tcb, server_input, _) = server_tuple;
        let (mut client_tcb, client_input, _) = client_tuple;

        client_input.send(TCBInput::Close).unwrap();
        client_tcb.handle_input_recv();
        let client_fin = sock_recv(&server_sock);
        server_input.send(TCBInput::Receive(client_fin)).unwrap();
        server_tcb.handle_input_recv();

        assert_eq!(server_tcb.state, TCBState::Closed);
        assert_eq!(client_tcb.state, TCBState::Closed);
    }
}
