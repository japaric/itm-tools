#![deny(warnings)]

use core::cmp::Ordering;
use std::{
    collections::HashMap,
    fs::{self, File},
};

use clap::{App, Arg};
use exitfailure::ExitFailure;
use failure::bail;
use itm::{Packet, Stream};
use xmas_elf::{
    sections::SectionData,
    symbol_table::{Entry, Type},
    ElfFile,
};

fn main() -> Result<(), ExitFailure> {
    run().map_err(|e| e.into())
}

fn run() -> Result<(), failure::Error> {
    let matches = App::new("pcsampl")
        .about("ITM-based program profiler")
        .arg(
            Arg::with_name("elf")
                .help("ELF file that corresponds to the profiled program")
                .short("e")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name("FILE")
                .help("ITM binary dump to process")
                .required(true)
                .index(1),
        )
        .get_matches();

    // collect samples
    let mut stream = Stream::new(File::open(matches.value_of("FILE").unwrap())?, false);

    let mut samples = vec![];
    while let Some(res) = stream.next()? {
        match res {
            Ok(Packet::PeriodicPcSample(pps)) => samples.push(pps),
            Ok(_) => {} // don't care
            Err(e) => eprintln!("{:?}", e),
        }
    }

    // extract routines from the ELF file
    let data = fs::read(matches.value_of("elf").unwrap())?;
    let elf = ElfFile::new(&data).map_err(failure::err_msg)?;
    let mut routines = vec![];
    if let Some(section) = elf.find_section_by_name(".symtab") {
        match section.get_data(&elf).map_err(failure::err_msg)? {
            SectionData::SymbolTable32(entries) => {
                for entry in entries {
                    if entry.get_type() == Ok(Type::Func) {
                        let name = entry.get_name(&elf).map_err(failure::err_msg)?;
                        // clear the thumb (T) bit
                        let address = entry.value() & !1;
                        let size = entry.size();

                        routines.push(Routine {
                            address,
                            name,
                            size,
                        });
                    }
                }
            }
            _ => bail!("malformed .symtab section"),
        }
    } else {
        bail!(".symtab section is missing")
    }

    routines.sort();

    // map samples to routines
    let mut stats = HashMap::new();
    let mut needle = Routine {
        address: 0,
        name: "",
        size: 0,
    };
    let min_pc = routines[0].address;
    let mut total = samples.len();
    let mut sleep = 0; // sleep cycles
    for sample in samples {
        if let Some(pc) = sample.pc().map(u64::from) {
            if pc < min_pc {
                // bogus value; ignore
                eprintln!("bogus PC ({:#010x})", pc);
                total -= 1;
                continue;
            }

            needle.address = pc;
            let pos = routines.binary_search(&needle).unwrap_or_else(|e| e - 1);

            let hit = &routines[pos];
            if pc > hit.address + hit.size {
                // bogus value; ignore
                eprintln!("bogus PC ({:#010x})", pc);
                total -= 1;
                continue;
            }

            *stats.entry(hit.name).or_insert(0) += 1;
        } else {
            sleep += 1;
        }
    }

    let mut ranking = stats.into_iter().collect::<Vec<_>>();
    ranking.sort_by(|a, b| b.1.cmp(&a.1));

    // report statistics
    let pct = |x| 100. * f64::from(x) / total as f64;
    println!("    % FUNCTION");
    // we always report sleep time first
    println!("{:5.02} *SLEEP*", pct(sleep));
    for entry in ranking {
        println!(
            "{:5.02} {}",
            pct(entry.1),
            rustc_demangle::demangle(entry.0).to_string(),
        );
    }

    println!("-----\n 100% {} samples", total);

    Ok(())
}

#[derive(Clone, Copy, Debug, Eq)]
struct Routine<'a> {
    address: u64,
    name: &'a str,
    size: u64,
}

impl<'a> Ord for Routine<'a> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.address.cmp(&other.address)
    }
}

impl<'a> PartialOrd for Routine<'a> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<'a> PartialEq for Routine<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.address == other.address
    }
}
