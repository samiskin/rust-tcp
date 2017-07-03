pub mod segment;
pub mod config;
use std::str;
use std::net::*;
pub mod conn;
use segment::*;
use conn::*;
use std::collections::HashMap;

pub fn run_server(config: config::Config) -> Result<(), std::io::Error> {
    println!("Starting Server...");

    let socket = UdpSocket::bind(format!("127.0.0.1:{}", config.port)).unwrap();
    let mut tcbs: HashMap<TCPTuple, TCB> = HashMap::new();

    'event_loop: loop {
        let mut buf = vec![0; (1 << 16) - 1];
        let (amt, src): (usize, SocketAddr) = socket.recv_from(&mut buf).unwrap();
        let buf = Vec::from(&mut buf[..amt]);
        let seg = Segment::from_buf(buf);
        println!("Received {} bytes with checksum {}", amt, seg.checksum);
        assert!(seg.validate()); // TODO: Replace
        let tuple = TCPTuple::from(&seg, &src);

        let tcb: &mut TCB = tcbs.entry(tuple).or_insert(TCB::from(tuple));
        println!("Using TCB: {:?}", tcb);

        match tcb.state {
            TCBState::LISTEN => {
                if seg.get_flag(Flag::SYN) {
                    println!("GOT SYN");
                    tcb.state = TCBState::SYN_RECD;
                    let mut reply = Segment::new(config.port, tcb.tuple.src_port);
                    reply.set_flag(Flag::SYN);
                    reply.set_flag(Flag::ACK);
                    let bytes = reply.to_byte_vec();
                    let target = tcb.target_addr();
                    socket.send_to(&bytes[..], &target).unwrap();
                }
            }
            TCBState::SYN_SENT => {}
            TCBState::SYN_RECD => {
                if seg.get_flag(Flag::ACK) {
                    println!("ESTAB!");
                    tcb.state = TCBState::ESTAB;
                }
            }
            TCBState::ESTAB => {
                if seg.get_flag(Flag::FIN) {
                    tcb.reset();
                    println!("CLOSED!");
                    break 'event_loop;
                }
            }
        };
    }

    Ok(())
}




fn accept(sock: UdpSocket) -> u32 {
    0
}

fn send_sock(sock: UdpSocket, data: &Vec<u8>, port: u16) -> i32 {
    0
}

fn read_sock(sock: UdpSocket, data: &Vec<u8>, port: u16) -> i32 {
    0
}

pub fn run_client(config: config::Config) -> Result<(), std::io::Error> {
    println!("Starting Client...");

    let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
    let mut syn = Segment::new(socket.local_addr().unwrap().port(), config.port);
    syn.set_flag(Flag::SYN);
    let bytes = syn.to_byte_vec();
    let amt = socket
        .send_to(&*bytes, format!("127.0.0.1:{}", config.port))
        .unwrap();

    let mut buf = vec![0; (1 << 16) - 1];
    let (amt, src): (usize, SocketAddr) = socket.recv_from(&mut buf).unwrap();
    let buf = Vec::from(&mut buf[..amt]);
    let seg = Segment::from_buf(buf);
    assert!(seg.get_flag(Flag::ACK));
    println!("Received SYN ACK: {:?}", seg);

    let mut ack = Segment::new(socket.local_addr().unwrap().port(), config.port);
    ack.set_flag(Flag::ACK);
    let bytes = ack.to_byte_vec();
    let amt = socket
        .send_to(&*bytes, format!("127.0.0.1:{}", config.port))
        .unwrap();
    println!("Sent ACK");

    let mut fin = Segment::new(socket.local_addr().unwrap().port(), config.port);
    fin.set_flag(Flag::FIN);
    let bytes = fin.to_byte_vec();
    let amt = socket
        .send_to(&*bytes, format!("127.0.0.1:{}", config.port))
        .unwrap();
    println!("Sent FIN");
    Ok(())
}
