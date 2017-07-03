pub mod segment;
pub mod config;
use std::str;
use std::net::*;
pub mod conn;
use segment::*;
use conn::*;
use std::collections::HashMap;
use std::time::Duration;
use std::time::SystemTime;

pub fn run_server(config: config::Config) -> Result<(), std::io::Error> {
    println!("Starting Server...");

    let mut tcbs: HashMap<TCPTuple, TCB> = HashMap::new();
    let socket = UdpSocket::bind(format!("127.0.0.1:{}", config.port)).unwrap();

    socket
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();

    'event_loop: loop {
        let mut buf = vec![0; (1 << 16) - 1];
        match socket.recv_from(&mut buf) {
            Ok((amt, src)) => {
                let buf = Vec::from(&mut buf[..amt]);
                let seg = Segment::from_buf(buf);
                println!("Received {} bytes with checksum {}", amt, seg.checksum);
                if !seg.validate() {
                    println!("Corrupted packet!");
                    continue 'event_loop;
                }
                let tuple = TCPTuple::from(config.port, &src);
                let tcb: &mut TCB = tcbs.entry(tuple).or_insert(TCB::new(
                    tuple,
                    socket.try_clone().unwrap(),
                ));

                tcb.handle_segment(seg);
            }
            Err(ref err) if err.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(err) => panic!(err),
        };

        for tcb in tcbs.values_mut() {
            tcb.check_timeout();
        }
    }
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
    ack.seq_num = 1;
    ack.set_flag(Flag::ACK);
    let bytes = ack.to_byte_vec();
    let amt = socket
        .send_to(&*bytes, format!("127.0.0.1:{}", config.port))
        .unwrap();
    println!("Sent ACK");

    let mut fin = Segment::new(socket.local_addr().unwrap().port(), config.port);
    fin.seq_num = 2;
    fin.set_flag(Flag::FIN);
    let bytes = fin.to_byte_vec();
    let amt = socket
        .send_to(&*bytes, format!("127.0.0.1:{}", config.port))
        .unwrap();
    println!("Sent FIN");
    Ok(())
}
