pub mod utils;
pub mod segment;
pub mod config;
use std::str;
use std::net::*;
pub mod conn;
use config::*;
use segment::*;
use conn::*;
use utils::*;
use std::collections::HashMap;
use std::io::prelude::*;
use std::collections::hash_map::Entry;
use std::sync::mpsc::{Sender, Receiver, channel};
use std::fs::File;


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


fn get_file(tuple: &TCPTuple, folder: String) -> Result<File, std::io::Error> {
    let filename = format!(
        "{}.{}.{}.{}",
        tuple.dst.ip(),
        tuple.dst.port(),
        tuple.src.ip(),
        tuple.src.port()
    );

    let filepath = format!("{}/{}", folder, filename);
    let file = if let Ok(file) = File::open(filepath.clone()) {
        Ok(file)
    } else {
        File::create(filepath.clone()).unwrap();
        File::open(filepath.clone())
    };

    println!("Got file {:?}", file);
    file
}

fn send_str(tcb: &mut TCB, rx: &Receiver<Segment>, s: String) {
    let len: u32 = s.len() as u32;
    let mut bytes = u32_to_u8(len);
    bytes.extend(s.into_bytes());
    tcb.send(bytes, &rx);
}

fn recv_str(tcb: &mut TCB, rx: &Receiver<Segment>) -> String {
    let size = buf_to_u32(&tcb.recv(4, &rx));
    String::from_utf8(tcb.recv(size, &rx)).unwrap()
}

fn run_tcb(config: Config, tuple: TCPTuple, rx: Receiver<Segment>, socket: UdpSocket) {
    let mut tcb = TCB::new(tuple, socket.try_clone().unwrap());
    handshake(&mut tcb, &rx, false).unwrap();
    let mut file = get_file(&tuple, config.filepath).unwrap();
    let mut s = String::new();
    file.read_to_string(&mut s).unwrap();
}

fn multiplexed_receive(
    config: &Config,
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
                        let config = config.clone();
                        std::thread::spawn(move || { run_tcb(config, tuple, rx, socket); });
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
        multiplexed_receive(&config, &mut channels, &socket)?;
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

    fn config() -> Config {
        Config {
            port: 12345,
            filepath: String::from("./"),
        }
    }

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
    fn get_file_test() {
        let tuple = TCPTuple::from(
            "127.0.0.1:54321".parse().unwrap(),
            "127.0.0.1:12345".parse().unwrap(),
        );
        let mut file = get_file(&tuple, config().filepath).unwrap();
        let mut s = String::new();
        file.read_to_string(&mut s).unwrap();
        println!("Got file of length {}", s.len());
    }

    fn run_clientserver_test<F1, F2>(server_fn: F1, client_fn: F2)
    where
        F1: FnOnce(TCB, Receiver<Segment>) -> () + Send + 'static,
        F2: FnOnce(TCB, Receiver<Segment>) -> () + Send + 'static,
    {
        let config = config();
        let (server_tx, server_rx) = channel();
        let (client_tx, client_rx) = channel();
        let (server_tcb, client_tcb) = tests::tcb_pair();
        let server_sock = server_tcb.sock.try_clone().unwrap();
        let client_sock = client_tcb.sock.try_clone().unwrap();
        let server_sock_2 = server_tcb.sock.try_clone().unwrap();
        let client_sock_2 = client_tcb.sock.try_clone().unwrap();
        let mut server_channels: HashMap<TCPTuple, Sender<Segment>> = HashMap::new();
        server_channels.insert(server_tcb.tuple, server_tx);
        let mut client_channels: HashMap<TCPTuple, Sender<Segment>> = HashMap::new();
        client_channels.insert(client_tcb.tuple, client_tx);


        let server_tcb_thread = thread::spawn(move || { server_fn(server_tcb, server_rx); });
        let client_tcb_thread = thread::spawn(move || { client_fn(client_tcb, client_rx); });

        let server_config = config.clone();
        let server = thread::spawn(move || 'event_loop: loop {
            match multiplexed_receive(&server_config, &mut server_channels, &server_sock) {
                Err(_) => break 'event_loop,
                Ok(_) => {}
            }
        });

        let client = thread::spawn(move || 'event_loop: loop {
            match multiplexed_receive(&config, &mut client_channels, &client_sock) {
                Err(_) => break 'event_loop,
                Ok(_) => {}
            }
        });

        server_tcb_thread.join().expect("couldnt server unwrap");
        client_tcb_thread.join().expect("couldnt client unwrap");
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

    #[test]
    fn estab_handshake() {
        tests::run_clientserver_test(
            |mut server_tcb: TCB, server_rx: Receiver<Segment>| {
                handshake(&mut server_tcb, &server_rx, false).expect("serverhandshake");
                assert_eq!(server_tcb.state, TCBState::Estab);
            },
            |mut client_tcb: TCB, client_rx: Receiver<Segment>| {
                handshake(&mut client_tcb, &client_rx, true).expect("clienthandshake");
                assert_eq!(client_tcb.state, TCBState::Estab);
            },
        );
    }

    #[test]
    fn transfer_data() {
        tests::run_clientserver_test(
            |mut server_tcb: TCB, server_rx: Receiver<Segment>| {
                handshake(&mut server_tcb, &server_rx, false).expect("serverhandshake");
                let str = String::from("Hello World");
                send_str(&mut server_tcb, &server_rx, str);
            },
            |mut client_tcb: TCB, client_rx: Receiver<Segment>| {
                handshake(&mut client_tcb, &client_rx, true).expect("clienthandshake");
                let str = recv_str(&mut client_tcb, &client_rx);
                assert_eq!(str, String::from("Hello World"));
            },
        );
    }
}
