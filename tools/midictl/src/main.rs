//! midictl — tiny host-side MIDI sender for the rp2350-synth.
//!
//! Auto-targets the first MIDI output port whose name contains "picodsp" (case-insensitive;
//! the firmware enumerates as "PicoDSP …"). Override the match substring with the `MIDI_PORT`
//! environment variable.
//!
//! Usage:
//!   midictl list                         List output ports
//!   midictl pc <program>                 Program Change (select preset)
//!   midictl note <note> [secs] [vel]     Note On, hold `secs` (default 3), then Note Off
//!   midictl cc <controller> <value>      Control Change
//!   midictl raw <byte> [byte ...]        Send raw bytes (decimal or 0x-hex)
//!   midictl dump                         Request a full-bank SysEx dump; validate the response
//!   midictl writeback                    Dump then write it straight back (WRITE round-trip)
//!
//! Examples:
//!   midictl pc 12                        Load preset 12
//!   midictl note 60 8                    Hold middle C for 8 s
//!   midictl cc 67 127 && midictl cc 67 0 Soft-pedal tap (cycles preset)
//!   midictl dump                         Check the editor SysEx dump path (prints magic/version/count)

use std::env;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use midir::{MidiInput, MidiOutput, MidiOutputPort};

fn parse_byte(s: &str) -> Option<u8> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u8::from_str_radix(hex, 16).ok()
    } else {
        s.parse().ok()
    }
}

fn find_port(out: &MidiOutput) -> Option<MidiOutputPort> {
    let needle = env::var("MIDI_PORT").unwrap_or_else(|_| "picodsp".to_string());
    let needle = needle.to_lowercase();
    out.ports().into_iter().find(|p| {
        out.port_name(p)
            .map(|n| n.to_lowercase().contains(&needle))
            .unwrap_or(false)
    })
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let cmd = args.first().map(|s| s.as_str()).unwrap_or("");

    let out = MidiOutput::new("midictl").expect("create MIDI output");

    if cmd == "list" || cmd.is_empty() {
        println!("MIDI output ports:");
        for p in out.ports() {
            println!("  - {}", out.port_name(&p).unwrap_or_default());
        }
        if cmd.is_empty() {
            eprintln!("\nNo command given. See `midictl` source header for usage.");
            std::process::exit(2);
        }
        return;
    }

    let Some(port) = find_port(&out) else {
        eprintln!("No matching MIDI output port (looking for substring from MIDI_PORT or \"picodsp\").");
        eprintln!("Run `midictl list` to see available ports.");
        std::process::exit(2);
    };
    let name = out.port_name(&port).unwrap_or_default();
    let mut conn = out.connect(&port, "midictl").expect("connect MIDI output");

    match cmd {
        "pc" => {
            let prog: u8 = args.get(1).and_then(|s| parse_byte(s)).expect("program number");
            conn.send(&[0xC0, prog & 0x7F]).unwrap();
            println!("PC {prog} -> \"{name}\"");
        }
        "note" => {
            let note: u8 = args.get(1).and_then(|s| parse_byte(s)).expect("note number");
            let secs: f32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(3.0);
            let vel: u8 = args.get(3).and_then(|s| parse_byte(s)).unwrap_or(100);
            conn.send(&[0x90, note & 0x7F, vel & 0x7F]).unwrap();
            println!("Note On {note} (vel {vel}) -> \"{name}\", holding {secs}s");
            std::thread::sleep(Duration::from_secs_f32(secs));
            conn.send(&[0x80, note & 0x7F, 0]).unwrap();
            println!("Note Off {note}");
        }
        "cc" => {
            let ctrl: u8 = args.get(1).and_then(|s| parse_byte(s)).expect("controller number");
            let val: u8 = args.get(2).and_then(|s| parse_byte(s)).expect("value");
            conn.send(&[0xB0, ctrl & 0x7F, val & 0x7F]).unwrap();
            println!("CC {ctrl} = {val} -> \"{name}\"");
        }
        "raw" => {
            let bytes: Vec<u8> = args[1..].iter().filter_map(|s| parse_byte(s)).collect();
            assert!(!bytes.is_empty(), "no bytes to send");
            conn.send(&bytes).unwrap();
            println!("raw {bytes:02X?} -> \"{name}\"");
        }
        "dump" => {
            // Request a full-bank SysEx dump and validate the device's response (exercises the
            // firmware's CMD_DUMP_REQ path + nibbleized streaming). Opens a MIDI *input* on the
            // same device, accumulates the F0..F7 message, then de-nibbleizes the header.
            let inp = MidiInput::new("midictl-in").expect("create MIDI input");
            let needle = env::var("MIDI_PORT")
                .unwrap_or_else(|_| "picodsp".to_string())
                .to_lowercase();
            let in_port = inp
                .ports()
                .into_iter()
                .find(|p| {
                    inp.port_name(p)
                        .map(|n| n.to_lowercase().contains(&needle))
                        .unwrap_or(false)
                })
                .expect("no matching MIDI input port");

            let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
            let done = Arc::new(AtomicBool::new(false));
            let (buf2, done2) = (buf.clone(), done.clone());
            let _in_conn = inp
                .connect(
                    &in_port,
                    "midictl-in",
                    move |_ts, msg, _| {
                        let mut b = buf2.lock().unwrap();
                        for &x in msg {
                            if x == 0xF0 {
                                b.clear();
                                b.push(x);
                            } else if !b.is_empty() {
                                b.push(x);
                                if x == 0xF7 {
                                    done2.store(true, Ordering::SeqCst);
                                }
                            }
                        }
                    },
                    (),
                )
                .expect("connect MIDI input");

            conn.send(&[0xF0, 0x7D, 0x01, 0x01, 0xF7]).unwrap();
            println!("Sent DUMP_REQ -> \"{name}\"; waiting for response...");

            let start = Instant::now();
            while !done.load(Ordering::SeqCst) && start.elapsed() < Duration::from_secs(8) {
                std::thread::sleep(Duration::from_millis(20));
            }

            let b = buf.lock().unwrap();
            println!("Received {} SysEx bytes", b.len());
            if b.len() < 6
                || b[0] != 0xF0
                || b[1] != 0x7D
                || b[2] != 0x01
                || b[3] != 0x02
                || *b.last().unwrap() != 0xF7
            {
                eprintln!(
                    "BAD framing: {:02X?}{}",
                    &b[..b.len().min(8)],
                    if b.is_empty() { " (no data)" } else { "" }
                );
                std::process::exit(1);
            }
            let payload = &b[4..b.len() - 1];
            let de = |i: usize| (payload[i * 2] << 4) | (payload[i * 2 + 1] & 0x0F);
            let magic = u32::from_le_bytes([de(0), de(1), de(2), de(3)]);
            let version = u32::from_le_bytes([de(4), de(5), de(6), de(7)]);
            let num = u32::from_le_bytes([de(8), de(9), de(10), de(11)]);
            println!(
                "payload {} nibbles => {} bytes | magic=0x{:08X} version={} num_presets={}",
                payload.len(),
                payload.len() / 2,
                magic,
                version,
                num
            );
        }
        "writeback" => {
            // Round-trip the WRITE path: dump the current bank (a CMD_WRITE_REQ message), then
            // send it straight back in 240-byte chunks (as picoDSP-Edit does) and confirm the
            // device replies CMD_WRITE_SUCCESS. Exercises the firmware's on-the-fly de-nibbleize
            // accumulator + write_raw. Idempotent (writes the same bytes back).
            let inp = MidiInput::new("midictl-in").expect("create MIDI input");
            let needle = env::var("MIDI_PORT")
                .unwrap_or_else(|_| "picodsp".to_string())
                .to_lowercase();
            let in_port = inp
                .ports()
                .into_iter()
                .find(|p| {
                    inp.port_name(p)
                        .map(|n| n.to_lowercase().contains(&needle))
                        .unwrap_or(false)
                })
                .expect("no matching MIDI input port");
            let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
            let done = Arc::new(AtomicBool::new(false));
            let (buf2, done2) = (buf.clone(), done.clone());
            let _in_conn = inp
                .connect(
                    &in_port,
                    "midictl-in",
                    move |_ts, msg, _| {
                        let mut b = buf2.lock().unwrap();
                        for &x in msg {
                            if x == 0xF0 {
                                b.clear();
                                b.push(x);
                            } else if !b.is_empty() {
                                b.push(x);
                                if x == 0xF7 {
                                    done2.store(true, Ordering::SeqCst);
                                }
                            }
                        }
                    },
                    (),
                )
                .expect("connect MIDI input");

            // 1) Dump.
            conn.send(&[0xF0, 0x7D, 0x01, 0x01, 0xF7]).unwrap();
            let start = Instant::now();
            while !done.load(Ordering::SeqCst) && start.elapsed() < Duration::from_secs(8) {
                std::thread::sleep(Duration::from_millis(20));
            }
            let dumped = buf.lock().unwrap().clone();
            assert!(
                dumped.len() > 6 && dumped[3] == 0x02 && *dumped.last().unwrap() == 0xF7,
                "dump failed: got {} bytes",
                dumped.len()
            );
            println!("Dumped {} bytes; writing back in chunks...", dumped.len());

            // 2) Send it back as the editor does, then wait for the status reply.
            done.store(false, Ordering::SeqCst);
            buf.lock().unwrap().clear();
            for chunk in dumped.chunks(240) {
                conn.send(chunk).unwrap();
                std::thread::sleep(Duration::from_millis(2));
            }
            let start = Instant::now();
            while !done.load(Ordering::SeqCst) && start.elapsed() < Duration::from_secs(8) {
                std::thread::sleep(Duration::from_millis(20));
            }
            let reply = buf.lock().unwrap().clone();
            println!("Reply: {:02X?}", reply);
            match reply.as_slice() {
                [0xF0, 0x7D, 0x01, 0x03, 0xF7] => println!("WRITE_SUCCESS ✓"),
                [0xF0, 0x7D, 0x01, 0x04, code, 0xF7] => {
                    eprintln!("WRITE_ERROR code={code:#x}");
                    std::process::exit(1);
                }
                _ => {
                    eprintln!("Unexpected/empty reply");
                    std::process::exit(1);
                }
            }
        }
        other => {
            eprintln!("Unknown command '{other}'. Commands: list, pc, note, cc, raw, dump, writeback.");
            std::process::exit(2);
        }
    }
}
