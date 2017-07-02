use std::net::*;

#[derive(Debug, Copy, Clone)]
enum State {
    LISTEN,
    SYN_SENT,
    SYN_RECD,
    ESTAB,
}

#[derive(Debug)]
pub struct TCB {
    sock: UdpSocket,
    src_port: u16,
    src_ip: Ipv4Addr,
    dst_port: u16,
    dst_ip: Ipv4Addr,
    state: State,
}
