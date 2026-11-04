//! `ps2-dev <command> ...`: see the usage text below.

use std::process::ExitCode;

use ps2_devtools::{calls, captures, checks, perf, rate, sampling, validate};

const USAGE: &str = "usage:
  ps2-dev validate-config [--root DIR] [--runtime native/ DIR]   funcmap.csv and gow-port.toml checks (CI)
  ps2-dev validate-tests KNOWN_FAILURES.txt                      reads ps2x_tests' output on stdin (CI)
  ps2-dev perf-summary LOG [--from N] [--to N] [--game] [--json FILE]
  ps2-dev sampling SAMPLING.txt EXECUTABLE [N]                   GOW_SAMPLING summary with nm (Linux)
  ps2-dev sampling-win SAMPLING.txt MAP [--top N] [--undname EXE]  GOW_SAMPLING summary with the linker map (Windows)
  ps2-dev cancel-line SEGMENT.bin [T0 T1]                        movement controller timeline
  ps2-dev gif-inspect CAPTURE [--json FILE]                      positions and ADC of local GIF captures
  ps2-dev vif-extract --dump FOLDER [--start ADDR] OUTPUT        input of replay_vif_chain
  ps2-dev gen-call-list HEADER OUT                               ps2_call_list.h -> calls_generated.rs
  ps2-dev rate-readers RUNNER_DIR [--rate A,B] [--delta A,B] [--watch A,B]  logic-rate readers in the generated code
  ps2-dev check-vu1-compiled EXECUTABLE                          driver of gow_vu1_compiled_test
  ps2-dev check-gs-replay EXECUTABLE CAPTURE.bin                 driver of gow_gs_replay's CLI test";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((command, rest)) = args.split_first() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let outcome = |result: Result<(), String>| match result {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("{message}");
            1
        }
    };
    let code = match (command.as_str(), rest) {
        ("validate-config", rest) => validate::validate_config(rest),
        ("validate-tests", rest) => validate::validate_tests(rest),
        ("perf-summary", rest) => perf::run(rest),
        ("sampling", rest) => sampling::linux(rest),
        ("sampling-win", rest) => sampling::windows(rest),
        ("cancel-line", rest) => captures::cancel_line(rest),
        ("gif-inspect", rest) => captures::gif_inspect(rest),
        ("vif-extract", rest) => captures::vif_extract(rest),
        ("gen-call-list", rest) => calls::run(rest),
        ("rate-readers", rest) => rate::run(rest),
        ("check-vu1-compiled", [executable]) => outcome(checks::vu1_compiled(executable)),
        ("check-gs-replay", [executable, capture]) => outcome(checks::gs_replay(executable, capture)),
        _ => {
            eprintln!("{USAGE}");
            2
        }
    };
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}
