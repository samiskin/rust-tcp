pub mod segment;
pub mod config;
use std::str;
use std::net::*;
pub mod conn;
use segment::*;
use conn::*;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::mpsc::{Sender, Receiver, channel};


pub fn handshake(tcb: &mut TCB, rx: &Receiver<Segment>, initiate: bool) -> Result<(), TCBState> {
    assert_eq!(tcb.state, TCBState::Listen);
    if initiate {
        tcb.send_syn();
    }


    loop {
        let seg = rx.recv().unwrap();
        tcb.handle_shake(seg);
        if tcb.state == TCBState::Estab {
            return Ok(());
        } else if tcb.state == TCBState::Closed {
            return Err(tcb.state);
        }
    }
}


fn run_tcb(tuple: TCPTuple, rx: Receiver<Segment>, socket: UdpSocket) {
    let mut tcb = TCB::new(tuple, socket.try_clone().unwrap());
    handshake(&mut tcb, &rx, false).unwrap();

    let str = format!(
        "{}.{}.{}.{}",
        tcb.tuple.dst.ip(),
        tcb.tuple.dst.port(),
        tcb.tuple.src.ip(),
        tcb.tuple.src.port()
    );

    println!("Server Estab!  looking for file {}", str);
}

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
                let tuple = TCPTuple::from(socket.local_addr().unwrap(), src);
                match channels.entry(tuple) {
                    Entry::Occupied(entry) => {
                        entry.into_mut().send(seg).unwrap();
                    }
                    Entry::Vacant(v) => {
                        println!("New connection! {:?}", tuple);
                        let (tx, rx) = channel();
                        tx.send(seg).unwrap();
                        v.insert(tx);
                        let socket = socket.try_clone().unwrap();
                        std::thread::spawn(move || { run_tcb(tuple, rx, socket); });
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

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    fn tcb_pair() -> (TCB, TCB) {
        let server_sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let client_sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let server_addr = server_sock.local_addr().unwrap();
        let client_addr = client_sock.local_addr().unwrap();
        let server_tuple = TCPTuple::from(server_addr, client_addr);
        let client_tuple = TCPTuple::from(client_addr, server_addr);
        (
            TCB::new(server_tuple, server_sock),
            TCB::new(client_tuple, client_sock),
        )
    }

    #[test]
    fn estab_handshake() {
        let (server_tx, server_rx) = channel();
        let (client_tx, client_rx) = channel();
        let (mut server_tcb, mut client_tcb) = tests::tcb_pair();
        let server_sock = server_tcb.sock.try_clone().unwrap();
        let client_sock = client_tcb.sock.try_clone().unwrap();
        let server_sock_2 = server_tcb.sock.try_clone().unwrap();
        let client_sock_2 = client_tcb.sock.try_clone().unwrap();
        let mut server_channels: HashMap<TCPTuple, Sender<Segment>> = HashMap::new();
        server_channels.insert(server_tcb.tuple, server_tx);
        let mut client_channels: HashMap<TCPTuple, Sender<Segment>> = HashMap::new();
        client_channels.insert(client_tcb.tuple, client_tx);


        let server_tcb_thread = thread::spawn(move || {
            handshake(&mut server_tcb, &server_rx, false).expect("serverhandshake");
            println!("Server ESTAB");
        });
        let client_tcb_thread = thread::spawn(move || {
            handshake(&mut client_tcb, &client_rx, true).expect("clienthandshake");
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

        server_tcb_thread.join().expect("couldnt server unwrap");
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
