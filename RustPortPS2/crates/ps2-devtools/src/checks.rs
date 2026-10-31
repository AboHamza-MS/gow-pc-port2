//! Drivers for the C++ test executables of `native/tests`: they run the built programs and check what they print.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::util::run_with_timeout;

type Outcome = Result<(), String>;

fn run(command: Command, seconds: u64) -> Result<crate::util::Finished, String> {
    run_with_timeout(command, seconds)
        .map_err(|e| format!("could not start the test: {e}"))?
        .ok_or_else(|| format!("the test did not finish in {seconds} s"))
}

// --- check-vu1-compiled ------------------------------------------------------------------------------------------

/// The check must run real compiled pairs, besides giving parity.
pub fn vu1_compiled(executable: &str) -> Outcome {
    let mut command = Command::new(executable);
    command.env("GOW_VU1C_DIAG", "1");
    for name in ["GOW_VU1C_RANGE", "GOW_VU1C_NO_BLOCKS", "GOW_VU1_NO_COMPILE"] {
        command.env_remove(name);
    }
    let result = run(command, 120)?;
    let report = format!("({}, {:?}, {:?})", result.code, result.stdout, result.stderr);
    let digits_after = |text: &str, marker: &str| -> Option<u64> {
        let at = text.find(marker)? + marker.len();
        let digits: String = text[at..].chars().take_while(char::is_ascii_digit).collect();
        digits.parse().ok()
    };
    let compiled = digits_after(&result.stderr, "[vu1c] compiled=");
    let interpreted = result.stderr.find("[vu1c] compiled=").and_then(|at| {
        let rest = &result.stderr[at..];
        digits_after(rest, " interpreted=")
    });
    let (Some(compiled), Some(interpreted)) = (compiled, interpreted) else {
        return Err(report);
    };
    if result.code != 0 || compiled == 0 || interpreted == 0 {
        return Err(report);
    }
    let exact = result.stdout.find("VU1 DIV: ").and_then(|at| {
        let rest = &result.stdout[at + "VU1 DIV: ".len()..];
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        rest[digits.len()..].starts_with(" exact cases").then(|| digits.parse::<u64>().ok()).flatten()
    });
    if exact.is_none_or(|n| n < 19072) {
        return Err(format!("The complete DIV check is missing: {report}"));
    }
    println!("{}", result.stdout.trim());
    println!("Real path: {compiled} compiled pairs and {interpreted} interpreted");
    Ok(())
}

// --- check-gs-replay ----------------------------------------------------------------------------------------------

struct Replay {
    executable: PathBuf,
    sample: PathBuf,
    output: PathBuf,
}

impl Replay {
    fn run(&self, arguments: &[&str], expected: i32, capture: Option<&Path>) -> Result<String, String> {
        let mut command = Command::new(&self.executable);
        command.arg(capture.unwrap_or(&self.sample)).arg("cpu").args(arguments).arg(&self.output);
        let result = run(command, 60)?;
        if result.code != expected {
            return Err(format!(
                "{arguments:?}: exit {} (expected {expected})\n{}\n{}",
                result.code, result.stdout, result.stderr
            ));
        }
        Ok(result.stdout + &result.stderr)
    }
}

fn expect(text: &str, needle: &str) -> Outcome {
    if text.contains(needle) {
        Ok(())
    } else {
        Err(format!("missing {needle:?} in:\n{text}"))
    }
}

fn expect_not(text: &str, needle: &str) -> Outcome {
    if text.contains(needle) {
        Err(format!("unexpected {needle:?} in:\n{text}"))
    } else {
        Ok(())
    }
}

/// CLI checks with the C++ test's synthetic capture; they use no game data.
pub fn gs_replay(executable: &str, sample: &str) -> Outcome {
    let output = std::env::temp_dir().join(format!("gow_gs_cli_{}", std::process::id()));
    std::fs::create_dir_all(&output).map_err(|e| e.to_string())?;
    let result = gs_replay_in(&Replay {
        executable: std::fs::canonicalize(executable).map_err(|e| format!("{executable}: {e}"))?,
        sample: std::fs::canonicalize(sample).map_err(|e| format!("{sample}: {e}"))?,
        output: output.clone(),
    });
    let _ = std::fs::remove_dir_all(&output);
    result?;
    println!("GS CLI: 11 invalid arguments, complete CPU controls/prefixes and limits before the replay: OK");
    Ok(())
}

fn gs_replay_in(replay: &Replay) -> Outcome {
    let invalid: [&[&str]; 11] = [
        &["--checkpoints-sync"],
        &["--checkpoints-sync", "--repetitions", "1"],
        &["--checkpoints-sync", "--checkpoints-sync", "--repetitions", "2"],
        &["--checkpoints-sync", "--repetitions", "0"],
        &["--checkpoints-sync", "--repetitions", "-2"],
        &["--until-record"],
        &["--until-record", "0"],
        &["--until-record", "-1"],
        &["--until-record", "abc"],
        &["--until-record", "4294967296"],
        &["--until-record", "1", "--until-record", "2"],
    ];
    for arguments in invalid {
        expect(&replay.run(arguments, 2, None)?, "Usage:")?;
    }
    for arguments in [
        &["--checkpoints-sync", "--repetitions", "3"][..],
        &["--repetitions", "2", "--checkpoints-sync", "--lockstep"][..],
    ] {
        let text = replay.run(arguments, 0, None)?;
        expect(&text, "Synchronized controls: previous=1 current=1 different=0")?;
        expect_not(&text, "First varying control:")?;
        expect(&text, "CPU vs final capture: bytes=0")?;
    }

    // Adding Flush does not modify the CPU End. The excess must be rejected BEFORE doing thousands of readbacks: also test
    // the limit on a valid input.
    let data = std::fs::read(&replay.sample).map_err(|e| e.to_string())?;
    if data.get(..8) != Some(b"GOWGSR1\0") {
        return Err("the sample is not a GOWGSR1 capture".to_owned());
    }
    let (mut position, mut end_offset, mut operations) = (24usize, None, Vec::<u8>::new());
    while position < data.len() {
        let start = position;
        let operation = data[position];
        let size =
            u32::from_le_bytes(data[position + 1..position + 5].try_into().map_err(|_| "truncated record")?) as usize;
        position += 5 + size;
        if position > data.len() {
            return Err("truncated record".to_owned());
        }
        operations.push(operation);
        if operation == 16 {
            end_offset = Some(start);
        }
    }
    let end_offset = end_offset.ok_or("the capture has no End")?;
    let record = |operation: u8, payload: &[u8]| {
        let mut bytes = vec![operation];
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(payload);
        bytes
    };
    let synchronized = replay.output.join("intermediate_controls.bin");
    // GSSyncReason has a uint8_t ABI; DebugReadback=3. Both commands are harmless for the CPU VRAM, but must be recorded
    // besides the End.
    let mut bytes = data[..end_offset].to_vec();
    bytes.extend(record(13, &[]));
    bytes.extend(record(14, &[3]));
    bytes.extend_from_slice(&data[end_offset..]);
    std::fs::write(&synchronized, &bytes).map_err(|e| e.to_string())?;
    let text = replay.run(&["--checkpoints-sync", "--repetitions", "2"], 0, Some(&synchronized))?;
    expect(&text, "Synchronized controls: previous=3 current=3 different=0")?;
    expect(&text, "CPU vs final capture: bytes=0")?;
    // Initial=0; the two added commands delimit a prefix that is compared with CPU, without presenting it as validation of
    // the original End.
    let before_end = operations.len() - 2;
    for stop in [before_end + 1, before_end + 2] {
        let text = replay.run(
            &["--until-record", &stop.to_string(), "--checkpoints-sync", "--repetitions", "2"],
            0,
            Some(&synchronized),
        )?;
        expect(&text, &format!("Prefix up to record={stop}"))?;
        expect(&text, "CPU vs cpu prefix: bytes=0")?;
        expect_not(&text, "CPU vs final capture:")?;
        expect(&text, "Synchronized controls: previous=")?;
        expect(&text, "different=0")?;
    }
    let text = replay.run(&["--until-record", &(before_end + 3).to_string()], 0, Some(&synchronized))?;
    expect(&text, "CPU vs final capture: bytes=0")?;
    let text = replay.run(&["--until-record", &(before_end + 4).to_string()], 2, Some(&synchronized))?;
    expect(&text, "exceeds the capture")?;
    for operation in [2u8, 6] {
        // Submit and TEXFLUSH are not supported boundaries.
        let stop =
            operations.iter().position(|o| *o == operation).ok_or_else(|| format!("operation {operation} missing"))?;
        let text = replay.run(&["--until-record", &stop.to_string()], 2, None)?;
        expect(&text, "requires Flush, Sync, Present or End")?;
    }

    let bad_end = replay.output.join("different_end.bin");
    let mut cut = bytes.clone();
    *cut.last_mut().ok_or("empty capture")? ^= 1;
    std::fs::write(&bad_end, &cut).map_err(|e| e.to_string())?;
    expect(&replay.run(&[], 3, Some(&bad_end))?, "CPU vs final capture: bytes=1")?;
    let text = replay.run(&["--until-record", &(before_end + 1).to_string()], 0, Some(&bad_end))?;
    expect_not(&text, "CPU vs final capture:")?;
    let oversized = replay.output.join("too_many_controls.bin");
    let mut many = data[..end_offset].to_vec();
    for _ in 0..4096 {
        many.extend(record(13, &[]));
    }
    many.extend_from_slice(&data[end_offset..]);
    std::fs::write(&oversized, &many).map_err(|e| e.to_string())?;
    let text = replay.run(&["--checkpoints-sync", "--repetitions", "2"], 2, Some(&oversized))?;
    expect(&text, "4096")?;
    expect_not(&text, "CPU vs final capture:")?;
    let text = replay.run(
        &["--until-record", &(before_end + 1).to_string(), "--checkpoints-sync", "--repetitions", "2"],
        0,
        Some(&oversized),
    )?;
    expect(&text, "CPU vs cpu prefix: bytes=0")?;
    expect(&text, "different=0")
}
