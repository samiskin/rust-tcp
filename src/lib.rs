pub mod tcp;
pub mod utils;
pub mod segment;
pub mod config;
use tcp::*;
use std::str;
use std::net::*;
use config::*;
use segment::*;
use utils::*;
use std::collections::HashMap;
use std::io::prelude::*;
use std::collections::hash_map::Entry;
use std::sync::mpsc::{Sender, Receiver, channel};
use std::fs::File;


fn get_file(tuple: &TCPTuple, folder: String) -> Result<(File, String), std::io::Error> {
    let filename = format!(
        "{}.{}.{}.{}",
        tuple.dst.ip(),
        tuple.dst.port(),
        tuple.src.ip(),
        tuple.src.port()
    );

    let filepath = format!("{}/{}", folder, filename);
    let file = if let Ok(file) = File::open(filepath.clone()) {
        file
    } else {
        File::create(filepath.clone()).unwrap();
        File::open(filepath.clone())?
    };

    Ok((file, filename))
}

fn _send_str(tcb_input: &Sender<TCBInput>, s: String) {
    let len: u32 = s.len() as u32;
    let mut bytes = u32_to_u8(len);
    bytes.extend(s.into_bytes());
    tcb_input.send(TCBInput::Send(bytes)).unwrap();
}

fn _recv_str(tcb_output: &Receiver<u8>) -> String {
    let size = buf_to_u32(&TCB::recv(&tcb_output, 4)[..]);
    String::from_utf8(TCB::recv(&tcb_output, size)).unwrap()
}

fn run_server_tcb(config: Config, tuple: TCPTuple, input: Sender<TCBInput>, output: Receiver<u8>) {
    let (mut file, filepath): (std::fs::File, String) =
        if let Ok((file, filepath)) = get_file(&tuple, config.filepath) {
            (file, filepath)
        } else {
            input.send(TCBInput::Close).unwrap();
            return;
        };

    let mut s = String::new();
    file.read_to_string(&mut s).unwrap();
    _send_str(&input, s);
    drop(file);

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .append(true)
        .open(filepath)
        .unwrap();

    loop {
        let data = _recv_str(&output);
        file.write_all(&data.as_bytes()).unwrap();
        _send_str(&input, data);
    }
    file.sync_all().unwrap();
}

fn run_client_tcb(config: Config, tuple: TCPTuple, input: Sender<TCBInput>, output: Receiver<u8>) {
    let file_contents = _recv_str(&output);
    println!("Got file: {}", file_contents);
    let response = String::from("It's not a story the jedi would tell you");
    _send_str(&input, response);
    let ack = _recv_str(&output);
    println!("Got ack: {}", ack);
}

fn multiplexed_receive(
    config: &Config,
    channels: &mut HashMap<TCPTuple, Sender<TCBInput>>,
    socket: &UdpSocket,
) -> Result<(), ()> {
    let mut buf = vec![0; (1 << 16) - 1];
    match socket.recv_from(&mut buf) {
        Ok((amt, src)) => {
            let buf = Vec::from(&mut buf[..amt]);
            let seg = Segment::from_buf(buf);
            if seg.validate() {
                let tuple = TCPTuple {
                    src: socket.local_addr().unwrap(),
                    dst: src, // Send replies to the sender
                };
                match channels.entry(tuple) {
                    Entry::Occupied(entry) => {
                        entry.into_mut().send(TCBInput::Receive(seg)).unwrap();
                    }
                    Entry::Vacant(v) => {
                        println!("New connection! {:?}", tuple);
                        let (mut tcb, input, output) = TCB::new(tuple, socket.try_clone().unwrap());
                        let udp_sender = input.clone();
                        udp_sender.send(TCBInput::Receive(seg)).unwrap();
                        v.insert(udp_sender);
                        let config = config.clone();
                        std::thread::spawn(move || tcb.run_tcp(false));
                        std::thread::spawn(
                            move || { run_server_tcb(config, tuple, input, output); },
                        );
                    }
                }
            }
        }
        Err(_) => return Err(()),
    };

    return Ok(());
}

pub fn run_server(config: config::Config) -> Result<(), ()> {
    println!("Starting Server...");

    let mut channels: HashMap<TCPTuple, Sender<TCBInput>> = HashMap::new();
    let socket = UdpSocket::bind(format!("127.0.0.1:{}", config.port)).unwrap();

    'event_loop: loop {
        multiplexed_receive(&config, &mut channels, &socket)?;
    }
}




pub fn run_client(_config: config::Config) -> Result<(), ()> {
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

    #[test]
    fn get_file_test() {
        let tuple = TCPTuple {
            src: "127.0.0.1:54321".parse().unwrap(),
            dst: "127.0.0.1:12345".parse().unwrap(),
        };
        let (mut file, _) = get_file(&tuple, config().filepath).unwrap();
        let mut s = String::new();
        file.read_to_string(&mut s).unwrap();
        println!("Got file of length {}", s.len());
    }

    const SCRIPT: &'static str = "Did you ever hear the tragedy of Darth Plagueis The Wise? I thought not. It’s not a story the Jedi would tell you. It’s a Sith legend. Darth Plagueis was a Dark Lord of the Sith, so powerful and so wise he could use the Force to influence the midichlorians to create life… He had such a knowledge of the dark side that he could even keep the ones he cared about from dying. The dark side of the Force is a pathway to many abilities some consider to be unnatural. He became so powerful… the only thing he was afraid of was losing his power, which eventually, of course, he did. Unfortunately, he taught his apprentice everything he knew, then his apprentice killed him in his sleep. Ironic. He could save others from death, but not himself.";

    #[test]
    fn transfer_data() {
        let ((server_input, server_output, _), (client_input, client_output, _)) =
            tcp::tests::run_e2e_pair(
                |mut server_tcb: TCB| server_tcb.run_tcp(false),
                |mut client_tcb: TCB| client_tcb.run_tcp(true),
            );

        _send_str(&server_input, String::from(SCRIPT));
        let output = _recv_str(&client_output);
        assert_eq!(output, String::from(SCRIPT));

        _send_str(&client_input, String::from(SCRIPT));
        let output = _recv_str(&server_output);
        assert_eq!(output, String::from(SCRIPT));
    }

    #[test]
    fn file_echo_test() {
        let ((server_input, server_output, server_sock),
             (client_input, client_output, client_sock)) =
            tcp::tests::run_e2e_pair(
                |mut server_tcb: TCB| server_tcb.run_tcp(false),
                |mut client_tcb: TCB| client_tcb.run_tcp(true),
            );

        let server_tuple = TCPTuple {
            src: server_sock.local_addr().unwrap(),
            dst: client_sock.local_addr().unwrap(),
        };
        let client_tuple = TCPTuple {
            src: client_sock.local_addr().unwrap(),
            dst: server_sock.local_addr().unwrap(),
        };
        let server_config = Config {
            port: server_sock.local_addr().unwrap().port(),
            filepath: String::from("./"),
        };
        let client_config = Config {
            port: client_sock.local_addr().unwrap().port(),
            filepath: String::from("./"),
        };

        let _server = std::thread::spawn(move || {
            run_server_tcb(server_config, server_tuple, server_input, server_output);
        });

        let _client = std::thread::spawn(move || {
            run_client_tcb(client_config, client_tuple, client_input, client_output);
        });

        _client.join();
    }
}
