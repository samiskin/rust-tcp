pub mod utils;
pub mod tcp;
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
use std::sync::mpsc::{Sender, Receiver, RecvError, SendError};
use std::fs::{File, OpenOptions};
use std::path::Path;


fn tuple_to_filename(tuple: &TCPTuple) -> String {
    format!(
        "{}.{}.{}.{}",
        tuple.dst.ip(),
        tuple.dst.port(),
        tuple.src.ip(),
        tuple.src.port()
    )
}

fn get_file(tuple: &TCPTuple, folder: &Path) -> Result<File, std::io::Error> {
    let filepath = folder.join(tuple_to_filename(&tuple));
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .append(true)
        .create(true)
        .open(filepath)?;

    Ok(file)
}

fn send_str(tcb_input: &Sender<TCBInput>, s: String) -> Result<(), SendError<TCBInput>> {
    let len: u32 = s.len() as u32;
    let mut bytes = u32_to_u8(len);
    bytes.extend(s.into_bytes());
    tcb_input.send(TCBInput::Send(bytes))?;
    Ok(())
}

fn recv_str(tcb_output: &Receiver<u8>) -> Result<String, RecvError> {
    let size = buf_to_u32(&TCB::recv(&tcb_output, 4)?[..]);
    Ok(String::from_utf8(TCB::recv(&tcb_output, size)?).unwrap())
}

fn run_server_tcb(config: Config, tuple: TCPTuple, input: Sender<TCBInput>, output: Receiver<u8>) {
    let mut file = if let Ok(file) = get_file(&tuple, config.filepath.as_path()) {
        file
    } else {
        input.send(TCBInput::Close).unwrap();
        return;
    };

    let mut s = String::new();
    file.read_to_string(&mut s).unwrap();
    send_str(&input, s).unwrap_or_else(|_| return);

    'main_application_loop: loop {
        match recv_str(&output) {
            Ok(data) => {
                file.write_all(&data.as_bytes()).unwrap();
                // Errors when Closed
                if send_str(&input, data).is_err() {
                    break 'main_application_loop;
                }
            }
            Err(_) => break 'main_application_loop,
        }
    }
    file.sync_all().unwrap();
}

fn multiplexed_receive(
    config: &Config,
    channels: &mut HashMap<TCPTuple, Sender<TCBInput>>,
    socket: &UdpSocket,
) -> Result<(), ()> {
    let mut buf = vec![0; (1 << 16) - 1];
    match socket.recv_from(&mut buf) {
        Ok((amt, src)) => {
            buf.truncate(amt);
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
                        std::thread::spawn(move || tcb.run_tcp());
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

pub fn run_server(config: Config) -> Result<(), ()> {
    println!("Starting Server...");

    let mut channels: HashMap<TCPTuple, Sender<TCBInput>> = HashMap::new();
    let socket = UdpSocket::bind(format!("127.0.0.1:{}", config.port)).unwrap();

    'event_loop: loop {
        multiplexed_receive(&config, &mut channels, &socket)?;
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn get_file_test() {
        let tuple = TCPTuple {
            src: "127.0.0.1:54321".parse().unwrap(),
            dst: "127.0.0.1:12345".parse().unwrap(),
        };
        let folderpath = Path::new("./");
        let mut file = get_file(&tuple, &folderpath).unwrap();
        let mut s = String::new();
        file.read_to_string(&mut s).unwrap();
        println!("Got file of length {}", s.len());

        let filepath = folderpath.join(tuple_to_filename(&tuple));
        std::fs::remove_file(filepath).unwrap();
    }

    const SCRIPT: &'static str = "Did you ever hear the tragedy of Darth Plagueis The Wise? I thought not. It’s not a story the Jedi would tell you. It’s a Sith legend. Darth Plagueis was a Dark Lord of the Sith, so powerful and so wise he could use the Force to influence the midichlorians to create life… He had such a knowledge of the dark side that he could even keep the ones he cared about from dying. The dark side of the Force is a pathway to many abilities some consider to be unnatural. He became so powerful… the only thing he was afraid of was losing his power, which eventually, of course, he did. Unfortunately, he taught his apprentice everything he knew, then his apprentice killed him in his sleep. Ironic. He could save others from death, but not himself.";

    #[test]
    fn transfer_data() {
        let ((server_input, server_output, _), (client_input, client_output, _)) =
            tcp::tests::run_e2e_pair(
                |mut server_tcb: TCB| server_tcb.run_tcp(),
                |mut client_tcb: TCB| client_tcb.run_tcp(),
            );

        client_input.send(TCBInput::SendSyn).unwrap();

        send_str(&server_input, String::from(SCRIPT)).unwrap();
        let output = recv_str(&client_output).unwrap();
        assert_eq!(output, String::from(SCRIPT));

        send_str(&client_input, String::from(SCRIPT)).unwrap();
        let output = recv_str(&server_output).unwrap();
        assert_eq!(output, String::from(SCRIPT));
    }

    fn get_tuples_from_socks(
        server_sock: &UdpSocket,
        client_sock: &UdpSocket,
    ) -> (TCPTuple, TCPTuple) {
        let server_tuple = TCPTuple {
            src: server_sock.local_addr().unwrap(),
            dst: client_sock.local_addr().unwrap(),
        };
        let client_tuple = TCPTuple {
            src: client_sock.local_addr().unwrap(),
            dst: server_sock.local_addr().unwrap(),
        };
        (server_tuple, client_tuple)
    }

    #[test]
    fn file_echo_test() {
        let ((server_input, server_output, server_sock),
             (client_input, client_output, client_sock)) =
            tcp::tests::run_e2e_pair(
                |mut server_tcb: TCB| server_tcb.run_tcp(),
                |mut client_tcb: TCB| client_tcb.run_tcp(),
            );

        let (server_tuple, _) = get_tuples_from_socks(&server_sock, &client_sock);

        let server_config = Config {
            port: server_sock.local_addr().unwrap().port(),
            filepath: PathBuf::from("./"),
        };

        let _server = std::thread::spawn(move || {
            run_server_tcb(server_config, server_tuple, server_input, server_output);
        });

        let filepath = Path::new("./");
        let filepath = filepath.join(tuple_to_filename(&server_tuple));
        let mut file = File::create(filepath.clone()).unwrap();
        let initial_contents = String::from(
            "Did you ever hear the tragedy of Darth Plagueis the wise?\n",
        );
        file.write_all(&initial_contents.as_bytes()).unwrap();
        file.flush().unwrap();
        file.sync_data().unwrap();
        drop(file);

        client_input.send(TCBInput::SendSyn).unwrap();
        let file_contents = recv_str(&client_output).unwrap();

        // NOTE: Sometimes the write doesn't actually succeed even though both flush and sync_data
        //       are called, so this assertion might fail...  just re-run the test if it does
        assert_eq!(
            file_contents,
            initial_contents,
            "\n\x1b[35m NOTE: This test may not have actually failed, try re-running \x1b[0m"
        );

        let response = String::from("It's not a story the jedi would tell you");
        send_str(&client_input, response.clone()).unwrap();
        let ack = recv_str(&client_output).unwrap();

        assert_eq!(ack, response);

        client_input.send(TCBInput::Close).unwrap();
        _server.join().unwrap();

        assert!(client_input.send(TCBInput::Close).is_err());

        std::fs::remove_file(filepath.clone()).unwrap();
    }

    #[test]
    #[ignore] // Reliant on existance of root_only_dir which is owned by root with permissions 700
    fn server_close_test() {
        let ((server_input, server_output, server_sock),
             (client_input, client_output, client_sock)) =
            tcp::tests::run_e2e_pair(
                |mut server_tcb: TCB| server_tcb.run_tcp(),
                |mut client_tcb: TCB| client_tcb.run_tcp(),
            );

        let (server_tuple, _) = get_tuples_from_socks(&server_sock, &client_sock);

        let server_config = Config {
            port: server_sock.local_addr().unwrap().port(),
            filepath: PathBuf::from("./root_only_dir/"),
        };

        let _server = std::thread::spawn(move || {
            run_server_tcb(server_config, server_tuple, server_input, server_output);
        });

        client_input.send(TCBInput::SendSyn).unwrap();
        assert!(client_output.recv().is_err());
    }
}
