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
//!
//! Examples:
//!   midictl pc 12                        Load preset 12
//!   midictl note 60 8                    Hold middle C for 8 s
//!   midictl cc 67 127 && midictl cc 67 0 Soft-pedal tap (cycles preset)

use std::env;
use std::time::Duration;

use midir::{MidiOutput, MidiOutputPort};

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
        other => {
            eprintln!("Unknown command '{other}'. Commands: list, pc, note, cc, raw.");
            std::process::exit(2);
        }
    }
}
