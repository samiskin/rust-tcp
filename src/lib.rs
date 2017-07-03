pub mod segment;
pub mod config;
use std::str;
use std::net::*;
pub mod tcb;
use segment::*;

pub fn run_server(config: config::Config) -> Result<(), std::io::Error> {
    println!("Starting Server...");

    let socket = UdpSocket::bind(format!("127.0.0.1:{}", config.port)).unwrap();

    println!("{}", 1 << 16);
    'event_loop: loop {
        let mut buf = vec![0; (1 << 16) - 1];
        let (amt, src) = socket.recv_from(&mut buf).unwrap();
        let buf = Vec::from(&mut buf[..amt]);
        let seg = Segment::from_buf(buf);

        break 'event_loop;
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
