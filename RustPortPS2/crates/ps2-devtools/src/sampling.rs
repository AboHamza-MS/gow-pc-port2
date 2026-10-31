//! `sampling` (Linux, `nm` symbols) and `sampling-win` (the linker's `.map`): per-thread summaries of the
//! `GOW_SAMPLING` file written by `gow_sampling.cpp`.

use std::collections::HashMap;
use std::process::Command;

use crate::util::Counter;

fn hex(text: &str) -> Option<i64> {
    i64::from_str_radix(text.trim_start_matches("0x"), 16).ok()
}

fn truncate(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

fn read_lossy(path: &str) -> Result<String, String> {
    std::fs::read(path).map(|b| String::from_utf8_lossy(&b).into_owned()).map_err(|e| format!("{path}: {e}"))
}

/// Index of the last element `<= value`, or -1.
fn floor_index(sorted: &[i64], value: i64) -> i64 {
    sorted.partition_point(|a| *a <= value) as i64 - 1
}

pub fn linux(args: &[String]) -> i32 {
    let (Some(sampling_file), Some(exe)) = (args.first(), args.get(1)) else {
        eprintln!("usage: ps2-dev sampling SAMPLING.txt EXECUTABLE [FUNCTIONS_PER_THREAD]");
        return 2;
    };
    let top: usize = args.get(2).and_then(|t| t.parse().ok()).unwrap_or(25);
    let nm = Command::new("nm").args(["-C", "--defined-only", exe]).output();
    let listing = nm.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
    let mut symbols: Vec<(i64, String)> = Vec::new();
    for line in listing.lines() {
        let parts: Vec<&str> = line.splitn(3, ' ').collect();
        if parts.len() == 3 && matches!(parts[1].to_lowercase().as_str(), "t" | "w") {
            if let Some(address) = hex(parts[0]) {
                symbols.push((address, parts[2].to_owned()));
            }
        }
    }
    symbols.sort();
    let addresses: Vec<i64> = symbols.iter().map(|s| s.0).collect();
    let text = match read_lossy(sampling_file) {
        Ok(text) => text,
        Err(message) => {
            eprintln!("{message}");
            return 1;
        }
    };
    let mut base = 0i64;
    let mut names: HashMap<i64, String> = HashMap::new();
    let mut thread_order: Vec<i64> = Vec::new();
    let mut counts: HashMap<i64, Counter<String>> = HashMap::new();
    let mut maps: Vec<(i64, i64, i64, String)> = Vec::new();
    for line in text.lines() {
        let p: Vec<&str> = line.split_whitespace().collect();
        let Some(&first) = p.first() else { continue };
        match first {
            "base" => base = p.get(1).and_then(|v| hex(v)).unwrap_or(0),
            "map" if p.len() >= 5 => {
                maps.push((hex(p[1]).unwrap_or(0), hex(p[2]).unwrap_or(0), hex(p[3]).unwrap_or(0), p[4].to_owned()));
            }
            "thread" if p.len() >= 2 => {
                names.insert(p[1].parse().unwrap_or(0), p[2..].join(" "));
            }
            _ if p.len() >= 2 => {
                let tid: i64 = first.parse().unwrap_or(0);
                let pc = hex(p[1]).unwrap_or(0) - base;
                let i = floor_index(&addresses, pc);
                let function = if i >= 0 && pc - addresses[i as usize] < 0x10_0000 {
                    symbols[i as usize].1.clone()
                } else {
                    let absolute = pc + base;
                    match maps.iter().find(|m| m.0 <= absolute && absolute < m.1) {
                        Some(m) => format!("[{}]", m.3.rsplit('/').next().unwrap_or("")),
                        None => "(outside the executable)".to_owned(),
                    }
                };
                if !counts.contains_key(&tid) {
                    thread_order.push(tid);
                }
                counts.entry(tid).or_default().add(function, 1);
            }
            _ => {}
        }
    }
    let total: u64 = counts.values().map(Counter::total).sum();
    let mut threads = thread_order;
    threads.sort_by_key(|tid| std::cmp::Reverse(counts[tid].total()));
    for tid in threads {
        let c = &counts[&tid];
        let n = c.total();
        println!(
            "== thread {tid} {}: {n} samples ({:.1} %)",
            names.get(&tid).map_or("?", String::as_str),
            100.0 * n as f64 / total as f64
        );
        for (function, k) in c.most_common(top) {
            println!("  {:5.1} %  {}", 100.0 * k as f64 / n as f64, truncate(&function, 150));
        }
    }
    0
}

// --- Windows ----------------------------------------------------------------------------------------------------

/// (preferred base address, `(address, name, object)` of the code section, sorted)
/// `(address, name, object)` of a symbol in the linker map.
type MapSymbol = (i64, String, String);

fn read_map(path: &str) -> Result<(i64, Vec<MapSymbol>), String> {
    let mut preferred = 0x1_4000_0000i64;
    let mut symbols = Vec::new();
    for line in read_lossy(path)?.lines() {
        if line.contains("Preferred load address is") {
            preferred = line.split_whitespace().last().and_then(hex).unwrap_or(preferred);
            continue;
        }
        if !line.starts_with(char::is_whitespace) {
            continue;
        }
        let tokens: Vec<&str> = line.split_whitespace().collect();
        let is_hex =
            |t: &str, len: usize| t.len() == len && t.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        let section = tokens.first().and_then(|t| t.strip_prefix("0001:"));
        if tokens.len() < 4 || !section.is_some_and(|s| is_hex(s, 8)) || !is_hex(tokens[2], 16) {
            continue;
        }
        symbols.push((hex(tokens[2]).unwrap_or(0), tokens[1].to_owned(), tokens[tokens.len() - 1].to_owned()));
    }
    symbols.sort();
    Ok((preferred, symbols))
}

/// Names undecorated by `undname.exe`, 40 at a time.
fn demangle(names: &[String], undname: Option<&str>) -> HashMap<String, String> {
    let mut result = HashMap::new();
    let Some(undname) = undname else { return result };
    let pending: Vec<&String> = names.iter().filter(|n| n.starts_with('?')).collect();
    for chunk in pending.chunks(40) {
        let Ok(output) = Command::new(undname).args(chunk).output() else { continue };
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        let mut rest = text.as_str();
        while let Some(at) = rest.find("Undecoration of :- \"") {
            rest = &rest[at + "Undecoration of :- \"".len()..];
            let Some(end) = rest.find('"') else { break };
            let (decorated, after) = (&rest[..end], &rest[end + 1..]);
            let Some(marker) = after.find("\nis :- \"") else { continue };
            if !after[..marker].chars().all(char::is_whitespace) {
                continue;
            }
            let value = &after[marker + "\nis :- \"".len()..];
            if let Some(close) = value.find('"') {
                result.insert(decorated.to_owned(), value[..close].to_owned());
                rest = &value[close + 1..];
            }
        }
    }
    result
}

fn shorten(name: &str, limit: usize) -> String {
    if name.chars().count() <= limit {
        name.to_owned()
    } else {
        format!("{}...", truncate(name, limit - 3))
    }
}

struct Thread {
    cpu_ms: i64,
    samples: i64,
    name: String,
}

pub fn windows(args: &[String]) -> i32 {
    if args.len() < 2 {
        eprintln!("usage: ps2-dev sampling-win SAMPLING.txt gow-2005.map [--top N] [--undname undname.exe]");
        return 2;
    }
    let (sampling_file, map_path) = (&args[0], &args[1]);
    let (mut top, mut undname) = (25usize, None);
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--top" => {
                top = args.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(25);
                i += 2;
            }
            "--undname" => {
                undname = args.get(i + 1).cloned();
                i += 2;
            }
            _ => i += 1,
        }
    }
    let (preferred, symbols) = match read_map(map_path) {
        Ok(map) => map,
        Err(message) => {
            eprintln!("{message}");
            return 1;
        }
    };
    let addresses: Vec<i64> = symbols.iter().map(|s| s.0).collect();
    let text = match read_lossy(sampling_file) {
        Ok(text) => text,
        Err(message) => {
            eprintln!("{message}");
            return 1;
        }
    };
    let (mut base, mut duration, mut interval) = (0i64, 0.0f64, 0i64);
    let mut modules: Vec<(i64, i64, String)> = Vec::new();
    let mut threads: Vec<(i64, Thread)> = Vec::new();
    let mut samples: Vec<(i64, i64, i64)> = Vec::new();
    for line in text.lines() {
        let p: Vec<&str> = line.split_whitespace().collect();
        let Some(&first) = p.first() else { continue };
        match first {
            "base" => base = p.get(1).and_then(|v| hex(v)).unwrap_or(0),
            "duration" if p.len() >= 4 => {
                duration = p[1].parse().unwrap_or(0.0);
                interval = p[3].parse().unwrap_or(0);
            }
            "module" if p.len() >= 3 => {
                modules.push((hex(p[1]).unwrap_or(0), hex(p[2]).unwrap_or(0), p[3..].join(" ")))
            }
            "thread" if p.len() >= 6 => {
                let tid = p[1].parse().unwrap_or(0);
                let thread = Thread {
                    cpu_ms: p[3].parse().unwrap_or(0),
                    samples: p[5].parse().unwrap_or(0),
                    name: p[6..].join(" "),
                };
                threads.retain(|(t, _)| *t != tid);
                threads.push((tid, thread));
            }
            "s" if p.len() >= 4 => {
                samples.push((p[1].parse().unwrap_or(0), hex(p[2]).unwrap_or(0), p[3].parse().unwrap_or(0)))
            }
            _ => {}
        }
    }
    let mut exe_end = base;
    for (b, t, _) in &modules {
        if *b == base {
            exe_end = b + t;
        }
    }

    // (kind, name, object)
    type Frame = (&'static str, String, String);
    let classify = |rip: i64| -> Frame {
        if base <= rip && rip < exe_end {
            let preferred_address = rip - base + preferred;
            let k = floor_index(&addresses, preferred_address);
            return if k >= 0 {
                let (_, name, object) = &symbols[k as usize];
                ("exe", name.clone(), object.clone())
            } else {
                ("exe", "?".to_owned(), "?".to_owned())
            };
        }
        for (b, t, name) in &modules {
            if *b <= rip && rip < b + t {
                let short = name.rsplit('\\').next().unwrap_or(name).to_owned();
                return ("dll", short.clone(), short);
            }
        }
        ("other", "?".to_owned(), "?".to_owned())
    };

    let mut by_thread: HashMap<i64, Counter<Frame>> = HashMap::new();
    let mut thread_total: HashMap<i64, i64> = HashMap::new();
    for (tid, rip, n) in &samples {
        by_thread.entry(*tid).or_default().add(classify(*rip), *n as u64);
        *thread_total.entry(*tid).or_default() += n;
    }
    let exe_names: Vec<String> = {
        let mut names = Vec::new();
        for counter in by_thread.values() {
            for ((kind, name, _), _) in counter.items() {
                if *kind == "exe" && !names.contains(name) {
                    names.push(name.clone());
                }
            }
        }
        names
    };
    let readable = demangle(&exe_names, undname.as_deref());
    let show = |name: &str| readable.get(name).cloned().unwrap_or_else(|| name.to_owned());

    let group = |frame: &Frame| -> String {
        let (kind, name, object) = frame;
        if *kind == "dll" {
            return format!("DLL {object}");
        }
        if object.starts_with("unity_") {
            return if name.starts_with("sub_") || name.starts_with("?sub_") {
                "recompiled game (unity_*.obj)".to_owned()
            } else {
                "port runtime (unity_*.obj)".to_owned()
            };
        }
        if object.contains("program") || object.contains("vu1n_part") {
            return "compiled VU1 (program*/vu1n_part*)".to_owned();
        }
        object.clone()
    };

    let total_samples: i64 = thread_total.values().sum();
    println!(
        "sampling: {duration:.1} s, an interval of {interval} us; threads: {}; samples: {total_samples}",
        threads.len()
    );
    threads.sort_by_key(|(_, t)| std::cmp::Reverse(t.cpu_ms));
    println!("\n== Threads (by CPU consumed in the window) ==");
    println!("{:>7} {:>8} {:>9}  name / main function", "thread", "cpu ms", "samples");
    for (tid, h) in &threads {
        let mut main_function = String::new();
        if let Some((frame, n)) = by_thread.get(tid).and_then(|c| c.most_common(1).into_iter().next()) {
            let total = thread_total.get(tid).copied().unwrap_or(0).max(1);
            main_function = format!("{} ({:.0} %)", shorten(&show(&frame.1), 60), 100.0 * n as f64 / total as f64);
        }
        println!(
            "{tid:>7} {:>8} {:>9}  {} | {main_function}",
            h.cpu_ms,
            h.samples,
            if h.name.is_empty() { "-" } else { &h.name }
        );
    }

    for (tid, h) in threads.iter().take(4) {
        let total = thread_total.get(tid).copied().unwrap_or(0);
        if total == 0 {
            continue;
        }
        println!("\n== Thread {tid} {} (cpu {} ms, {total} samples) ==", h.name, h.cpu_ms);
        let mut groups: Counter<String> = Counter::new();
        for (frame, n) in by_thread[tid].items() {
            groups.add(group(frame), n);
        }
        println!("  by group:");
        for (g, n) in groups.most_common(8) {
            println!("    {:5.1} %  {g}", 100.0 * n as f64 / total as f64);
        }
        println!("  top {top} functions:");
        for (frame, n) in by_thread[tid].most_common(top) {
            println!(
                "    {:5.1} %  {}  [{}]",
                100.0 * n as f64 / total as f64,
                shorten(&show(&frame.1), 96),
                shorten(&frame.2, 28)
            );
        }
    }
    0
}
