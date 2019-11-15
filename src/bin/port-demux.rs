#![deny(warnings)]

use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, Read, Write},
};

use clap::{App, Arg};
use exitfailure::ExitFailure;
use itm::{Packet, Stream};

fn main() -> Result<(), ExitFailure> {
    run().map_err(|e| e.into())
}

fn run() -> Result<(), failure::Error> {
    let matches = App::new("port-demux")
        .about("Demuxes instrumentation packets")
        .arg(
            Arg::with_name("FILE")
                .help("ITM binary dump to process, if omitted stdin will be read")
                .required(false)
                .index(1),
        )
        .arg(
            Arg::with_name("follow")
                .help("Process appended data as the file grows")
                .required(false)
                .short("f"),
        )
        .get_matches();

    let stdin;
    let reader: Box<dyn Read> = if let Some(file) = matches.value_of("FILE") {
        Box::new(File::open(file)?)
    } else {
        stdin = io::stdin();
        Box::new(stdin.lock())
    };

    let mut stream = Stream::new(reader, matches.is_present("follow"));

    let mut sinks = BTreeMap::new();
    while let Some(res) = stream.next()? {
        match res {
            Ok(Packet::Instrumentation(ip)) => {
                let port = ip.port();
                let payload = ip.payload();

                let sink = if let Some(sink) = sinks.get_mut(&port) {
                    sink
                } else {
                    let f = File::create(format!("{}.stim", port))?;
                    sinks.insert(port, f);
                    sinks.get_mut(&port).unwrap()
                };

                sink.write_all(payload)?;
            }
            Ok(_) => {} // don't care
            Err(e) => eprintln!("{:?}", e),
        }
    }

    Ok(())
}
