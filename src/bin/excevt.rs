#![deny(warnings)]

use core::{fmt, u32};
use std::{
    fs::File,
    io::{self, Read, StdoutLock, Write},
};

use clap::{App, Arg};
use exitfailure::ExitFailure;
use itm::{
    packet::{ExceptionTrace, Function},
    Packet, Stream,
};

fn main() -> Result<(), ExitFailure> {
    run().map_err(|e| e.into())
}

// special "instant" values
const INSTANT_DISABLED: u32 = u32::MAX;
const INSTANT_UNKNOWN: u32 = u32::MAX - 1;

enum Instant {
    Unknown,
    Reset,
    Known { now: u32, precise: bool },
}

fn run() -> Result<(), failure::Error> {
    let matches = App::new("excevt")
        .about("Pretty prints exception traces contained in an ITM binary dump")
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
        .arg(
            Arg::with_name("timestamp")
                .help("Expect timestamps")
                .required(false)
                .short("t"),
        )
        .get_matches();

    let stdin;
    let reader: Box<dyn Read> = if let Some(file) = matches.value_of("FILE") {
        Box::new(File::open(file)?)
    } else {
        stdin = io::stdin();
        Box::new(stdin.lock())
    };

    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    writeln!(stdout, " TIMESTAMP   EXCEPTION")?;

    let mut stream = Stream::new(reader, matches.is_present("follow"));

    const MAX: u32 = 1_000_000_000;
    let mut now = if matches.is_present("timestamp") {
        // we expect timestamps
        INSTANT_UNKNOWN
    } else {
        // assume that timestamps are disabled
        INSTANT_DISABLED
    };
    let mut next = None;
    'main: loop {
        let packet = if let Some(p) = next.take() {
            p
        } else {
            loop {
                match stream.next()? {
                    Some(Ok(p)) => break p,

                    Some(Err(e)) => {
                        eprintln!("{}", e);

                        if now != INSTANT_DISABLED {
                            // we may have lost a timestamp packet; computed instant is now
                            // unreliable
                            now = INSTANT_UNKNOWN;
                        }
                    }

                    // EOF
                    None => break 'main,
                }
            }
        };

        match packet {
            Packet::Overflow => {
                if now != INSTANT_DISABLED {
                    // a packet was lost due to limited bandwidth; computed instant is no longer
                    // reliable
                    now = INSTANT_UNKNOWN;
                }
            }

            Packet::ExceptionTrace(et) => {
                // if we know we are receiving timestamps ...
                if now != INSTANT_DISABLED {
                    // ... then look ahead for a timestamp
                    match stream.next()? {
                        Some(Ok(Packet::LocalTimestamp(lt))) => {
                            if now == INSTANT_UNKNOWN {
                                now = 0;

                                report(&mut stdout, &et, Instant::Reset)?;
                            } else {
                                let precise = lt.is_precise();

                                now = (now + lt.delta()) % MAX;

                                report(&mut stdout, &et, Instant::Known { now, precise })?;
                            }

                            continue;
                        }

                        // it's possible to receive two exception traces and then a timestamp
                        Some(Ok(Packet::ExceptionTrace(et2))) => {
                            match stream.next()? {
                                Some(Ok(Packet::LocalTimestamp(lt))) => {
                                    let precise = lt.is_precise();

                                    now = (now + lt.delta()) % MAX;

                                    // first trace has no timestamp so it's imprecise
                                    report(
                                        &mut stdout,
                                        &et,
                                        Instant::Known {
                                            now,
                                            precise: false,
                                        },
                                    )?;

                                    report(&mut stdout, &et2, Instant::Known { now, precise })?;

                                    continue;
                                }

                                // unexpected packet
                                Some(Ok(packet)) => {
                                    next = Some(packet);

                                    // fall through: report traces with unknown timestamp
                                }

                                // some byte was lost
                                Some(Err(e)) => {
                                    eprintln!("{}", e);

                                    // fall through: report traces with unknown timestamp
                                }

                                // EOF
                                None => {
                                    // report traces with unknown timestamp
                                    report(&mut stdout, &et, Instant::Unknown)?;
                                    report(&mut stdout, &et2, Instant::Unknown)?;

                                    break 'main;
                                }
                            }

                            // report traces with unknown timestamp
                            report(&mut stdout, &et, Instant::Unknown)?;
                            report(&mut stdout, &et2, Instant::Unknown)?;

                            // computed instant is now unknown
                            now = INSTANT_UNKNOWN;

                            continue;
                        }

                        // unexpected packet
                        Some(Ok(packet)) => {
                            next = Some(packet);

                            // fall through: report with unknown timestamp
                        }

                        // some byte was lost
                        Some(Err(e)) => {
                            eprintln!("{}", e);

                            // fall through: report with unknown timestamp
                        }

                        // EOF
                        None => {
                            // flush
                            report(&mut stdout, &et, Instant::Unknown)?;

                            break 'main;
                        }
                    }
                } else {
                    // fall through: report with unknown timestamp
                }

                // report this trace with unknown timestamp
                report(&mut stdout, &et, Instant::Unknown)?;

                // computed instant is now unknown
                now = INSTANT_UNKNOWN;
            }

            Packet::LocalTimestamp(lt) => {
                if now == INSTANT_DISABLED {
                    // first timestamp
                    now = INSTANT_UNKNOWN;
                } else {
                    if now != INSTANT_DISABLED && lt.delta() == 1_999_999 {
                        // standalone LTS1 packets are possible when the timestamp counter wraps
                        // around at the 2 million count
                        now += 2_000_000;
                    } else {
                        // we likely lost a packet; time is now unreliable
                        now = INSTANT_UNKNOWN;
                    }
                }
            }

            _ => {
                eprintln!("unexpected packet; exiting");

                break;
            }
        }
    }

    Ok(())
}

fn report(stdout: &mut StdoutLock, et: &ExceptionTrace, now: Instant) -> io::Result<()> {
    let f = match et.function() {
        Function::Enter => '→',
        Function::Exit => '←',
        Function::Return => '↓',
    };

    let en = ExceptionNumber(et.number());
    match now {
        Instant::Unknown => {
            writeln!(stdout, " ????????? {} {}", f, en)?;
        }

        Instant::Reset => {
            writeln!(stdout, "!000000000 {} {}", f, en)?;
        }

        Instant::Known { now, precise } => {
            writeln!(
                stdout,
                "{}{:09} {} {}",
                if precise { '=' } else { '<' },
                now,
                f,
                en
            )?;
        }
    }
    Ok(())
}

// Adapter for pretty printing the exception number
struct ExceptionNumber(u16);

impl fmt::Display for ExceptionNumber {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            0 => f.write_str("Thread"),
            1 => f.write_str("Reset"),
            2 => f.write_str("NMI"),
            3 => f.write_str("HardFault"),
            4 => f.write_str("MemManage"),
            5 => f.write_str("BusFault"),
            6 => f.write_str("UsageFault"),
            11 => f.write_str("SVCall"),
            12 => f.write_str("DebugMonitor"),
            14 => f.write_str("PendSV"),
            15 => f.write_str("SysTick"),
            n => write!(f, "IRQ({})", n - 16),
        }
    }
}
