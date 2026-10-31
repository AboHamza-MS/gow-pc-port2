//! `perf-summary`: windows of the optional `GOW_PERF_DIAG` profile. Times are elapsed and exclusive per thread.

use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::{json, Map, Value};

use crate::util::{first_positional, flag, option, split_lines};

pub type Row = HashMap<String, f64>;

/// `re.findall(r"(\w+)=([0-9.]+)")` turned into numbers.
fn key_values(line: &str) -> Result<Vec<(String, f64)>, String> {
    let chars: Vec<char> = line.chars().collect();
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let mut pairs = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !word(chars[i]) {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < chars.len() && word(chars[j]) {
            j += 1;
        }
        if chars.get(j) == Some(&'=') {
            let mut k = j + 1;
            while k < chars.len() && (chars[k].is_ascii_digit() || chars[k] == '.') {
                k += 1;
            }
            if k > j + 1 {
                let key: String = chars[i..j].iter().collect();
                let value: String = chars[j + 1..k].iter().collect();
                let number =
                    value.parse::<f64>().map_err(|_| format!("could not convert string to float: '{value}'"))?;
                pairs.push((key, number));
                i = k;
                continue;
            }
        }
        i = j;
    }
    Ok(pairs)
}

/// `re.findall(r"(state|pending|levelReady)=(\d+)")` as a map (the last value of each key wins).
fn pad_state(line: &str) -> HashMap<&'static str, i64> {
    let mut found = HashMap::new();
    let mut i = 0;
    while i < line.len() {
        let rest = &line[i..];
        let key = ["state", "pending", "levelReady"].into_iter().find(|key| {
            rest.strip_prefix(key)
                .and_then(|r| r.strip_prefix('='))
                .is_some_and(|r| r.starts_with(|c: char| c.is_ascii_digit()))
        });
        match key {
            Some(key) => {
                let digits_at = i + key.len() + 1;
                let digits: String = line[digits_at..].chars().take_while(char::is_ascii_digit).collect();
                found.insert(key, digits.parse().unwrap_or(i64::MAX));
                i = digits_at + digits.len();
            }
            None => i += rest.chars().next().map_or(1, char::len_utf8),
        }
    }
    found
}

/// With `game`, requires loaded states that enclose each profile window. The pad clock starts at its first read and the
/// profile's at the presentation, so line order is used and those clocks are never equated.
pub fn select_windows(text: &str, start: f64, end: f64, game: bool) -> Result<Vec<Row>, String> {
    if text.contains("[gow-perf:invalid]") {
        return Err("profile invalidated during the run; repeat without external load".to_owned());
    }
    let mut rows: Vec<Row> = Vec::new();
    let mut positions: Vec<usize> = Vec::new();
    let mut runs: Vec<Vec<usize>> = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    for (position, line) in split_lines(text).into_iter().enumerate() {
        if line.contains("[gow-perf]") {
            let row: Row = key_values(line)?.into_iter().collect();
            if row.contains_key("window") && row.contains_key("t") {
                rows.push(row);
                positions.push(position);
            }
        } else if line.contains("[gow-pad2:state]") {
            let state = pad_state(line);
            let loaded = state.len() == 3 && state["state"] == 11 && state["pending"] == 0 && state["levelReady"] == 1;
            if loaded {
                current.push(position);
            } else {
                if current.len() >= 2 {
                    runs.push(std::mem::take(&mut current));
                }
                current.clear();
            }
        }
    }
    if current.len() >= 2 {
        runs.push(current);
    }

    let mut spans: Vec<(f64, f64)> = Vec::new();
    if game {
        for run in &runs {
            // The first report after the initial state bounds the start from above; the last one before the final state
            // bounds the end from below. This excludes the windows that touch loading or transition.
            let first = positions.partition_point(|p| *p <= run[0]) as i64;
            let last = positions.partition_point(|p| *p < run[run.len() - 1]) as i64 - 1;
            if first <= last && (first as usize) < rows.len() {
                spans.push((rows[first as usize]["t"], rows[last as usize]["t"]));
            }
        }
    }

    let rounding = if game { 0.011 } else { 0.0 };
    let mut selected = Vec::new();
    for row in rows {
        let (window_start, window_end) = (row["t"] - row["window"], row["t"]);
        if window_start < start - rounding || window_end > end + rounding {
            continue;
        }
        // t and window are printed rounded to hundredths separately.
        if game && !spans.iter().any(|(low, high)| window_start >= low - 0.011 && window_end <= high + 0.011) {
            continue;
        }
        selected.push(row);
    }
    Ok(selected)
}

const KEYS: [&str; 7] = ["ee_ms", "vu_ms", "gs_ms", "iop_ms", "guest_wait_ms", "upload_ms", "host_wait_ms"];

/// A sum like Python's `sum(row.get(key, 0) ...)`: an int `0` when no row has the key.
fn sum_value(rows: &[Row], f: impl Fn(&Row) -> Option<f64>) -> (f64, bool) {
    let mut any = false;
    let mut total = 0.0;
    for row in rows {
        if let Some(v) = f(row) {
            any = true;
            total += v;
        } else {
            total += 0.0;
        }
    }
    (total, any)
}

fn number(value: f64, is_float: bool) -> Value {
    if is_float {
        json!(value)
    } else {
        json!(value as i64)
    }
}

pub fn summarize(rows: &[Row], game: bool) -> Value {
    let seconds: f64 = rows.iter().map(|r| r["window"]).sum();
    let mut totals = Map::new();
    let mut total_values = Vec::new();
    for key in KEYS {
        let (total, any) = sum_value(rows, |r| r.get(key).copied());
        totals.insert(key.to_owned(), number(total, any));
        total_values.push(total);
    }
    let guest_ms: f64 = total_values[..5].iter().sum();
    let mut percent = Map::new();
    for (key, total) in KEYS.iter().zip(&total_values).take(5) {
        percent.insert((*key).to_owned(), if guest_ms != 0.0 { json!(100.0 * total / guest_ms) } else { json!(0) });
    }
    let weighted =
        |key: &str| rows.iter().map(|r| r.get(key).copied().unwrap_or(0.0) * r["window"]).sum::<f64>() / seconds;
    let mut result = Map::new();
    result.insert("windows".into(), json!(rows.len()));
    result.insert("seconds".into(), json!(seconds));
    result.insert("from".into(), json!(rows[0]["t"] - rows[0]["window"]));
    result.insert("to".into(), json!(rows[rows.len() - 1]["t"]));
    result.insert("guest_flip_hz".into(), json!(weighted("guest_flip_hz")));
    result.insert("host_hz".into(), json!(weighted("host_hz")));
    result.insert("time_ms".into(), Value::Object(totals));
    result.insert("game_thread_percent".into(), Value::Object(percent));
    if game {
        result.insert("selection".into(), json!("observed game: state=11, pending=0, levelReady=1"));
    }
    Value::Object(result)
}

fn fail(message: &str) -> i32 {
    eprintln!("usage: ps2-dev perf-summary LOG [--from N] [--to N] [--game] [--json FILE]");
    eprintln!("ps2-dev perf-summary: error: {message}");
    2
}

pub fn run(args: &[String]) -> i32 {
    let Some(log) = first_positional(args, &["--from", "--to", "--json"]) else {
        return fail("the following arguments are required: log");
    };
    let parse = |name: &str, default: f64| -> Result<f64, String> {
        match option(args, name) {
            Some(text) => text.parse().map_err(|_| format!("argument {name}: invalid float value: '{text}'")),
            None => Ok(default),
        }
    };
    let (start, end) = match (parse("--from", 100.0), parse("--to", 145.0)) {
        (Ok(start), Ok(end)) => (start, end),
        (Err(message), _) | (_, Err(message)) => return fail(&message),
    };
    let game = flag(args, "--game");
    let bytes = match std::fs::read(log) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("{log}: {error}");
            return 1;
        }
    };
    let rows = match select_windows(&String::from_utf8_lossy(&bytes), start, end, game) {
        Ok(rows) => rows,
        Err(message) => return fail(&message),
    };
    if rows.is_empty() {
        let qualifier = if game { " of a loaded game" } else { "" };
        return fail(&format!("no complete windows{qualifier} in the chosen interval"));
    }
    let result = summarize(&rows, game);
    let pretty = serde_json::to_string_pretty(&result).expect("json");
    println!("{pretty}");
    if let Some(output) = option(args, "--json") {
        if let Err(error) = std::fs::write(PathBuf::from(&output), pretty + "\n") {
            eprintln!("{output}: {error}");
            return 1;
        }
    }
    0
}
