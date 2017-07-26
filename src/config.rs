use std::env;
use std::path::PathBuf;

#[derive(Debug, PartialEq, Clone)]
pub struct Config {
    pub port: u16,
    pub filepath: PathBuf,
}

impl Config {
    pub fn new(mut args: env::Args) -> Result<Config, &'static str> {
        args.next(); // skip the filename
        let port = match args.next() {
            Some(arg) => arg.parse::<u16>().unwrap(),
            None => return Err("Didn't get port"),
        };

        let filepath = match args.next() {
            Some(arg) => arg,
            None => return Err("Didnt' get file name"),
        };

        Ok(Config {
            port: port,
            filepath: PathBuf::from(filepath),
        })
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct ClientConfig {
    pub src_port: u16,
    pub dst_port: u16,
}

impl ClientConfig {
    pub fn new(mut args: env::Args) -> Result<ClientConfig, &'static str> {
        args.next(); // skip the filename
        let src_port = match args.next() {
            Some(arg) => arg.parse::<u16>().unwrap(),
            None => return Err("Didn't get port"),
        };
        let dst_port = match args.next() {
            Some(arg) => arg.parse::<u16>().unwrap(),
            None => return Err("Didn't get port"),
        };
        Ok(ClientConfig {
            src_port: src_port,
            dst_port: dst_port,
        })
    }
}
