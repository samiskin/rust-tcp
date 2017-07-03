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

    println!("{}", 1 << 16);
    'event_loop: loop {
        let mut buf = vec![0; (1 << 16) - 1];
        let (amt, src): (usize, SocketAddr) = socket.recv_from(&mut buf).unwrap();
        let buf = Vec::from(&mut buf[..amt]);
        let seg = Segment::from_buf(buf);
        assert!(seg.validate()); // TODO: Replace
        let tuple = TCPTuple::from(&seg, &src);
        break 'event_loop;

        let tcb: &mut TCB = tcbs.entry(tuple).or_insert(TCB::from(tuple));
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

    let socket = UdpSocket::bind(format!("127.0.0.1:{}", config.port)).unwrap();

    Ok(())
}
