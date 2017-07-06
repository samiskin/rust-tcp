pub mod segment;
pub mod config;
use std::str;
use std::net::*;
pub mod conn;
use segment::*;
use conn::*;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::time::Duration;
use std::sync::mpsc::{Sender, Receiver, channel};

fn run_handshake(tcb: &mut TCB, rx: &Receiver<Segment>) -> Result<(), ()> {
    'estab: loop {
        let seg = rx.recv().unwrap();
        if let Some(evt) = tcb.handle_shake(seg) {
            match evt {
                TCBEvent::ESTAB => {
                    return Ok(());
                }
                TCBEvent::CLOSED => {
                    println!("SERVER CLOSED");
                    return Err(());
                }
            }
        }
    }

}

fn run_tcb(mut tcb: TCB, rx: Receiver<Segment>) {
    run_handshake(&mut tcb, &rx).unwrap();

    let str = format!(
        "{}.{}.{}.{}",
        tcb.tuple.dst_ip,
        tcb.tuple.dst_port,
        Ipv4Addr::new(127, 0, 0, 1),
        tcb.tuple.src_port
    );

    println!("Server Estab!  looking for file {}", str);
}
use std::error::Error;
fn multiplexed_receive(
    channels: &mut HashMap<TCPTuple, Sender<Segment>>,
    socket: &UdpSocket,
) -> Result<(), ()> {
    let mut buf = vec![0; (1 << 16) - 1];
    match socket.recv_from(&mut buf) {
        Ok((amt, src)) => {
            if amt == 1 {
                return Err(());
            }
            let buf = Vec::from(&mut buf[..amt]);
            let seg = Segment::from_buf(buf);
            if seg.validate() {
                println!("{:?} {:?}", seg, src);
                let tuple = TCPTuple::from(seg.dst_port(), &src);
                match channels.entry(tuple) {
                    Entry::Occupied(entry) => {
                        entry.into_mut().send(seg).unwrap();
                    }
                    Entry::Vacant(v) => {
                        println!("New connection! {:?}", tuple);
                        let (tx, rx) = channel();
                        tx.send(seg).unwrap();
                        v.insert(tx);
                        let tcb = TCB::new(tuple, socket.try_clone().unwrap());
                        std::thread::spawn(move || { run_tcb(tcb, rx); });
                    }
                }

            }
        }
        Err(err) => return Err(()),
    };

    return Ok(());
}

pub fn run_server(config: config::Config) -> Result<(), ()> {
    println!("Starting Server...");

    let mut channels: HashMap<TCPTuple, Sender<Segment>> = HashMap::new();
    let socket = UdpSocket::bind(format!("127.0.0.1:{}", config.port)).unwrap();

    'event_loop: loop {
        multiplexed_receive(&mut channels, &socket)?;
    }
}




pub fn run_client(config: config::Config) -> Result<(), ()> {
    println!("Starting Client...");
    let client_port = 54321;
    let socket = UdpSocket::bind(format!("127.0.0.1:{}", client_port)).unwrap();
    let mut channels: HashMap<TCPTuple, Sender<Segment>> = HashMap::new();

    let dst = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), config.port);
    let tuple = TCPTuple::from(client_port, &dst);

    let (tx, rx) = channel();
    channels.insert(tuple, tx);

    let mut tcb = TCB::new(tuple, socket.try_clone().unwrap());
    std::thread::spawn(move || {
        tcb.send_syn();
        run_handshake(&mut tcb, &rx).unwrap();
        println!("CLIENT ESTAB");
    });

    loop {
        multiplexed_receive(&mut channels, &socket)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    fn tcb_pair() -> (TCB, TCB) {
        let server_addr = "127.0.0.1:12345".parse().unwrap();
        let client_addr = "127.0.0.1:54321".parse().unwrap();
        let server_sock = UdpSocket::bind(server_addr).unwrap();
        let client_sock = UdpSocket::bind(client_addr).unwrap();
        let server_tuple = TCPTuple::from_addrs(&server_addr, &client_addr);
        let client_tuple = TCPTuple::from_addrs(&client_addr, &server_addr);
        (
            TCB::new(server_tuple, server_sock),
            TCB::new(client_tuple, client_sock),
        )
    }

    #[test]
    fn handshake() {
        let (mut server_tcb, mut client_tcb) = tests::tcb_pair();
        let server_sock = server_tcb.sock.try_clone().unwrap();
        let client_sock = client_tcb.sock.try_clone().unwrap();
        let server_sock_2 = server_tcb.sock.try_clone().unwrap();
        let client_sock_2 = client_tcb.sock.try_clone().unwrap();
        let (server_tx, server_rx) = channel();
        let (client_tx, client_rx) = channel();
        let mut server_channels: HashMap<TCPTuple, Sender<Segment>> = HashMap::new();
        server_channels.insert(server_tcb.tuple, server_tx);
        let mut client_channels: HashMap<TCPTuple, Sender<Segment>> = HashMap::new();
        client_channels.insert(client_tcb.tuple, client_tx);

        let server_tcb_thread = thread::spawn(move || {
            run_handshake(&mut server_tcb, &server_rx).unwrap();
            println!("Server ESTAB");
        });
        let client_tcb_thread = thread::spawn(move || {
            client_tcb.send_syn();
            run_handshake(&mut client_tcb, &client_rx).unwrap();
            println!("Client ESTAB");
        });

        let server = thread::spawn(move || 'event_loop: loop {
            match multiplexed_receive(&mut server_channels, &server_sock) {
                Err(_) => break 'event_loop,
                Ok(_) => {}
            }
        });

        let client = thread::spawn(move || 'event_loop: loop {
            match multiplexed_receive(&mut client_channels, &client_sock) {
                Err(_) => break 'event_loop,
                Ok(_) => {}
            }
        });

        server_tcb_thread.join().unwrap();
        println!("Server completed!");
        client_tcb_thread.join().unwrap();
        println!("Client completed!");
        let bytes = vec![0];
        server_sock_2
            .send_to(&bytes[..], &client_sock_2.local_addr().unwrap())
            .unwrap();
        client_sock_2
            .send_to(&bytes[..], &server_sock_2.local_addr().unwrap())
            .unwrap();

        server.join().unwrap();
        client.join().unwrap();
    }
}
