//! cafsctl — poke a CAFS image or device through the I/O Engine.
//! PoC tool: inspect anchors, run check(), and read/write raw bytes,
//! with or without mounting (`--bypass`).
//!
//! SPDX-License-Identifier: BSD-3-Clause
//! Copyright (c) 2026, Imran Bin Gifary (System Delta)

use std::io::IsTerminal;
use std::process::exit;

use cafs_io::{ByteOffset, CafsCtx, MountMode};

struct Style {
    on: bool,
}
impl Style {
    fn new() -> Self {
        Style { on: std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none() }
    }
    fn p(&self, code: &str, s: &str) -> String {
        if self.on { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_string() }
    }
    fn head(&self, s: &str) -> String { self.p("1;36", s) }
    fn cmd(&self, s: &str) -> String { self.p("32", s) }
    fn flag(&self, s: &str) -> String { self.p("33", s) }
    fn dim(&self, s: &str) -> String { self.p("2", s) }
    fn ok(&self, s: &str) -> String { self.p("32", s) }
    fn bad(&self, s: &str) -> String { self.p("1;31", s) }
}

fn help(st: &Style) {
    println!("{}  poke a CAFS image or device through the I/O Engine\n", st.head("cafsctl"));
    println!("{}  cafsctl <command> <path> [options]\n", st.head("USAGE"));
    println!("{}", st.head("INSPECT"));
    println!("  {}  <path>                           all 12 pointer blocks (head + end-of-device), which end mount would use", st.cmd("anchors"));
    println!("  {}    <path> [{}]               mount and run check(); {} opens read-write and repairs\n", st.cmd("check"), st.flag("--repair"), st.flag("--repair"));
    println!("{}", st.head("RAW I/O  (engine read_block / write_block)"));
    println!("  {}     <path> <offset> <len>          hex dump", st.cmd("read"));
    println!("  {}    <path> <offset> {} | {} <len> | {}", st.cmd("write"), st.flag("--hex HEX"), st.flag("--fill BYTE --len"), st.flag("--file FILE"));
    println!("{}", st.dim("                                           numbers: decimal, 0x hex, or with K/M suffix\n"));
    println!("{}", st.head("OPTIONS"));
    println!("  {}   open without mounting: no anchor is read, trusted or required (works on damaged volumes)", st.flag("--bypass"));
    println!("  {}     fdatasync after a write", st.flag("--sync"));
    println!("  {}   read-write open for `check`", st.flag("--repair"));
    println!("  {}     this text", st.flag("-h, --help"));
}

fn die(st: &Style, msg: &str) -> ! {
    eprintln!("{} {msg}", st.bad("cafsctl:"));
    exit(1);
}

fn num(st: &Style, s: &str) -> u64 {
    let (body, mult) = match s.chars().last() {
        Some('K') | Some('k') => (&s[..s.len() - 1], 1024),
        Some('M') | Some('m') => (&s[..s.len() - 1], 1024 * 1024),
        _ => (s, 1),
    };
    let v = if let Some(h) = body.strip_prefix("0x") { u64::from_str_radix(h, 16) } else { body.parse() };
    v.map(|v| v * mult).unwrap_or_else(|_| die(st, &format!("not a number: {s}")))
}

fn hex(st: &Style, s: &str) -> Vec<u8> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if !s.len().is_multiple_of(2) {
        die(st, "--hex needs an even number of digits");
    }
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap_or_else(|_| die(st, "bad hex digit")))
        .collect()
}

fn dump(base: u64, data: &[u8]) {
    for (i, chunk) in data.chunks(16).enumerate() {
        let hexs: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
        let asc: String = chunk.iter().map(|&b| if (0x20..0x7f).contains(&b) { b as char } else { '.' }).collect();
        println!("{:010x}  {:<47}  {}", base + (i * 16) as u64, hexs.join(" "), asc);
    }
}

fn main() {
    let st = Style::new();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        help(&st);
        return;
    }
    let flag = |f: &str| args.iter().any(|a| a == f);
    let opt = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();
    let pos: Vec<&String> = {
        let mut v = Vec::new();
        let mut skip = false;
        for a in &args {
            if skip { skip = false; continue; }
            if ["--hex", "--fill", "--len", "--file"].contains(&a.as_str()) { skip = true; continue; }
            if a.starts_with("--") { continue; }
            v.push(a);
        }
        v
    };
    if pos.len() < 2 {
        help(&st);
        exit(1);
    }
    let (cmd, path) = (pos[0].as_str(), pos[1].as_str());
    let mode = if flag("--bypass") { MountMode::Bypass } else { MountMode::Full };

    match cmd {
        "anchors" => {
            let mut c = CafsCtx::mount_with(path, false, MountMode::Bypass).unwrap_or_else(|e| die(&st, &format!("open: {e}")));
            println!("device size {} bytes\n", c.device_size());
            println!("{}", st.head("slot  block   head                                tail                                use"));
            for r in c.scan_anchors() {
                let side = |x: &cafs_io::anchor::PtrSide| match x {
                    Ok((b, _)) => st.ok(&format!("ok  -> {:<10} {:<10}", b.real_offset.0, b.fallback.map_or("-".into(), |f| format!("fb {}", f.0.0)))),
                    Err(e) => st.bad(&format!("BAD {:<31}", format!("{e:?}"))),
                };
                let used = r.used().map_or(st.bad("NONE"), |u| format!("{u:?}"));
                println!("{:<5} {:<7} {}  {}  {}{}", r.slot, cafs_io::layout::slot_block_size(r.slot), side(&r.head), side(&r.tail), used, if r.disagree() { st.bad("  DISAGREE") } else { String::new() });
            }
        }
        "check" => {
            if flag("--bypass") {
                die(&st, "check needs the anchors; drop --bypass");
            }
            let mut c = CafsCtx::mount(path, flag("--repair")).unwrap_or_else(|e| die(&st, &format!("mount: {e}")));
            let r = c.check().unwrap_or_else(|e| die(&st, &format!("check: {e}")));
            println!("{r:#?}");
        }
        "read" => {
            if pos.len() < 4 { die(&st, "read <path> <offset> <len>"); }
            let (off, len) = (num(&st, pos[2]), num(&st, pos[3]));
            let mut c = CafsCtx::mount_with(path, false, mode).unwrap_or_else(|e| die(&st, &format!("open: {e}")));
            let mut buf = vec![0u8; len as usize];
            c.read_block(ByteOffset(off), &mut buf).unwrap_or_else(|e| die(&st, &format!("read: {e}")));
            dump(off, &buf);
        }
        "write" => {
            if pos.len() < 3 { die(&st, "write <path> <offset> (--hex HEX | --fill BYTE --len N | --file FILE)"); }
            let off = num(&st, pos[2]);
            let data = if let Some(h) = opt("--hex") {
                hex(&st, &h)
            } else if let Some(b) = opt("--fill") {
                let n = opt("--len").map(|s| num(&st, &s)).unwrap_or_else(|| die(&st, "--fill needs --len"));
                vec![num(&st, &b) as u8; n as usize]
            } else if let Some(f) = opt("--file") {
                std::fs::read(&f).unwrap_or_else(|e| die(&st, &format!("{f}: {e}")))
            } else {
                die(&st, "write needs --hex, --fill or --file")
            };
            let mut c = CafsCtx::mount_with(path, true, mode).unwrap_or_else(|e| die(&st, &format!("open: {e}")));
            c.write_block(ByteOffset(off), &data).unwrap_or_else(|e| die(&st, &format!("write: {e}")));
            if flag("--sync") {
                c.sync().unwrap_or_else(|e| die(&st, &format!("sync: {e}")));
            }
            println!("wrote {} bytes at {off:#x}{}", data.len(), if flag("--sync") { " (synced)" } else { "" });
        }
        _ => {
            help(&st);
            exit(1);
        }
    }
}
