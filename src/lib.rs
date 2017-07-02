pub mod segment;
pub mod config;
use std::str;
use std::net::*;
pub mod tcb;

pub fn run_server(config: config::Config) -> Result<(), std::io::Error> {
    println!("Starting Server...");

    let socket = UdpSocket::bind(format!("127.0.0.1:{}", config.port)).unwrap();

    Ok(())
}

pub fn run_client(config: config::Config) -> Result<(), std::io::Error> {
    println!("Starting Client...");

    let socket = UdpSocket::bind(format!("127.0.0.1:{}", config.port)).unwrap();

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
