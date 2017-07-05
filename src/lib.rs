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
use std::sync::mpsc::{Sender, channel};

pub fn run_server(config: config::Config) -> Result<(), std::io::Error> {
    println!("Starting Server...");

    let mut tcbs: HashMap<TCPTuple, TCB> = HashMap::new();
    let socket = UdpSocket::bind(format!("127.0.0.1:{}", config.port)).unwrap();

    let mut channels: HashMap<TCPTuple, Sender<Segment>> = HashMap::new();

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
                if seg.validate() {
                    let tuple = TCPTuple::from(seg.dst_port, &src);
                    let tcb: &mut TCB = tcbs.entry(tuple).or_insert(TCB::new(
                        tuple,
                        socket.try_clone().unwrap(),
                    ));

                    match channels.entry(tuple) {
                        Entry::Occupied(entry) => {
                            entry.into_mut().send(seg).unwrap();
                        }
                        Entry::Vacant(v) => {
                            let (tx, rx) = channel();
                            v.insert(tx);

                            std::thread::spawn(move || {});
                        }
                    }

                }
            }
            Err(ref err) if err.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(err) => panic!(err),
        };

        for tcb in tcbs.values_mut() {
            tcb.check_timeout();
        }
    }

    Ok(())
}




pub fn run_client(config: config::Config) -> Result<(), std::io::Error> {
    println!("Starting Client...");
    Ok(())
}
