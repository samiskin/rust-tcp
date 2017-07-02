pub mod segment;
pub mod config;
use std::str;
use std::net::*;
pub mod tcb;

pub fn run_server(config: config::Config) -> Result<(), std::io::Error> {
    println!("Starting Server...");

    let socket = UdpSocket::bind(format!("127.0.0.1:{}", config.port)).unwrap();
    let mut buf = [0; 10];
    let (amt, src) = socket.recv_from(&mut buf).unwrap();

    let buf = &mut buf[..amt];
    println!("Server got: {:?}", str::from_utf8(&buf).unwrap());

    buf.reverse();
    socket.send_to(buf, &src)?;

    Ok(())
}

pub fn run_client(config: config::Config) -> Result<(), std::io::Error> {
    println!("Starting Client...");

    let socket = UdpSocket::bind(format!("127.0.0.1:{}", config.port)).unwrap();
    let buf: Vec<u8> = "lolwat".bytes().collect();
    socket.send_to(&*buf, "127.0.0.1:35829")?;

    let mut buf = [0; 10];
    let (amt, _) = socket.recv_from(&mut buf)?;

    let buf = &mut buf[..amt];
    println!("Client got: {:?}", str::from_utf8(&buf).unwrap());

    Ok(())
}
