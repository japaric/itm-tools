#![deny(warnings)]

use std::{
    fs::File,
    io::{self, Read},
};

use clap::{App, Arg};
use exitfailure::ExitFailure;
use itm::{Packet, Stream};

fn main() -> Result<(), ExitFailure> {
    run().map_err(|e| e.into())
}

fn run() -> Result<(), failure::Error> {
    let matches = App::new("itm-decode")
        .about("Decodes an ITM binary dump into packets")
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

    while let Some(res) = stream.next()? {
        match res {
            Ok(Packet::DataTraceAddress(dta)) => println!("{:?}", dta),
            Ok(Packet::DataTraceDataValue(dtdv)) => println!("{:?}", dtdv),
            Ok(Packet::DataTracePcValue(dtpv)) => println!("{:?}", dtpv),
            Ok(Packet::EventCounter(ec)) => println!("{:?}", ec),
            Ok(Packet::ExceptionTrace(et)) => println!("{:?}", et),
            Ok(Packet::GTS1(gts)) => println!("{:?}", gts),
            Ok(Packet::GTS2(gts)) => println!("{:?}", gts),
            Ok(Packet::Instrumentation(i)) => println!("{:?}", i),
            Ok(Packet::LocalTimestamp(lt)) => println!("{:?}", lt),
            Ok(Packet::PeriodicPcSample(pps)) => println!("{:?}", pps),
            Ok(Packet::StimulusPortPage(spp)) => println!("{:?}", spp),
            Ok(Packet::Synchronization(s)) => println!("{:?}", s),
            Ok(packet @ Packet::Overflow) => println!("{:?}", packet),
            Err(e) => eprintln!("{:?}", e),
        }
    }

    Ok(())
}
