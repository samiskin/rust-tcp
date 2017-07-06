extern crate ece358;
use std::process;
use std::env;
use ece358::config::Config;
use std::io::prelude::*;


fn main() {
    let mut stderr = std::io::stderr();

    let config = Config::new(env::args()).unwrap_or_else(|err| {
        writeln!(&mut stderr, "Problem parsing arguments: {}", err)
            .expect("Could not write to stderr");
        process::exit(1);
    });

    if let Err(_) = ece358::run_server(config) {
        // writeln!(&mut stderr, "Application error: {}", e).expect("Could not write to stderr");
        process::exit(1);
    };
}
