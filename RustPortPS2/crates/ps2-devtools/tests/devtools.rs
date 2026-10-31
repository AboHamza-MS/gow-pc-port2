//! Tests of the developer tools. No game files: every input is built in memory.

use std::path::PathBuf;

use ps2_devtools::captures::{flatten, inspect, select_chain, walk};
use ps2_devtools::perf::{select_windows, summarize};
use ps2_devtools::validate::{check_suite, results};

fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

// --- perf-summary ------------------------------------------------------------------------------------------------

fn perf(t: f64, window: f64) -> String {
    format!("[gow-perf] t={t:.2} window={window:.2} guest_flip_hz=10.00 host_hz=60.00 ee_ms=500.00")
}

fn state(value: u32, pending: u32, ready: u32) -> String {
    // The pad clock intentionally differs from the profile clock.
    format!("[gow-pad2:state] seconds=700.00 state={value} pending={pending} levelReady={ready}")
}

fn loaded() -> String {
    state(11, 0, 1)
}

fn sample() -> String {
    [
        perf(5.0, 5.0),
        state(3, 1, 0),
        perf(10.0, 5.0),
        loaded(),
        perf(15.0, 5.0),
        loaded(),
        perf(20.0, 5.0),
        loaded(),
        perf(25.0, 5.0),
        state(4, 1, 0),
    ]
    .join("\n")
}

fn times(text: &str, from: f64, to: f64, game: bool) -> Vec<f64> {
    select_windows(text, from, to, game).unwrap().iter().map(|row| row["t"]).collect()
}

#[test]
fn game_excludes_transition_windows_with_different_clocks() {
    assert_eq!(times(&sample(), 0.0, 100.0, true), [20.0]);
}

#[test]
fn loading_is_rejected() {
    let text = [state(3, 1, 0), perf(5.0, 5.0), perf(10.0, 5.0), state(3, 1, 0)].join("\n");
    assert!(times(&text, 0.0, 100.0, true).is_empty());
}

#[test]
fn pending_and_unready_split_loaded_runs() {
    for invalid in [state(11, 1, 1), state(11, 0, 0)] {
        let text = [loaded(), perf(5.0, 5.0), invalid, perf(10.0, 5.0), loaded(), perf(15.0, 5.0), loaded()].join("\n");
        assert!(times(&text, 0.0, 100.0, true).is_empty());
    }
}

#[test]
fn needs_both_state_bounds() {
    let (a, b) = (perf(5.0, 5.0), perf(10.0, 5.0));
    for text in
        [[loaded(), a.clone(), b.clone()].join("\n"), [a.clone(), b.clone(), loaded()].join("\n"), [a, b].join("\n")]
    {
        assert!(times(&text, 0.0, 100.0, true).is_empty());
    }
}

#[test]
fn interval_and_normal_mode_are_preserved() {
    assert_eq!(times(&sample(), 10.0, 20.0, false), [15.0, 20.0]);
    assert!(times(&sample(), 20.0, 30.0, true).is_empty());
}

#[test]
fn invalidated_run_is_rejected_even_outside_selected_interval() {
    let marker = "[gow-perf:invalid] Profile invalidated by external load: cl.";
    for game in [false, true] {
        for text in [format!("{marker}\n{}", sample()), format!("{}\n{marker}", sample())] {
            let error = select_windows(&text, 10.0, 20.0, game).unwrap_err();
            assert!(error.contains("profile invalidated"), "{error}");
        }
    }
}

#[test]
fn separately_rounded_times_do_not_drop_full_window() {
    let text = [loaded(), perf(5.01, 5.0), loaded(), perf(10.01, 5.01), loaded()].join("\n");
    assert_eq!(times(&text, 0.0, 20.0, true), [10.01]);
}

#[test]
fn summary_reports_the_loaded_interval() {
    let rows = select_windows(&sample(), 0.0, 100.0, true).unwrap();
    let summary = summarize(&rows, true);
    assert_eq!(summary["windows"], 1);
    assert_eq!(summary["from"], 15.0);
    assert_eq!(summary["to"], 20.0);
    assert_eq!(summary["guest_flip_hz"], 10.0);
    assert_eq!(summary["selection"], "observed game: state=11, pending=0, levelReady=1");
}

// --- vif-extract ---------------------------------------------------------------------------------------------------

struct Memory(Vec<u8>);

impl Memory {
    fn new() -> Self {
        Self(vec![0; 0x400])
    }

    fn tag(&mut self, address: usize, kind: u64, target: u64, qwc: u64, tte: u64) {
        let low = qwc | (kind << 28) | (target << 32);
        self.0[address..address + 8].copy_from_slice(&low.to_le_bytes());
        self.0[address + 8..address + 16].copy_from_slice(&tte.to_le_bytes());
    }

    fn end(&mut self, address: usize) {
        self.tag(address, 7, 0, 0, 0);
    }
}

const HEADERS: [i64; 2] = [0x100, 0x200];

#[test]
fn single_valid_header_even_if_not_the_first() {
    let mut m = Memory::new();
    m.end(0x200);
    assert_eq!(select_chain(&m.0, None, None, &HEADERS), Ok((0x200, 0x200, 1)));
}

#[test]
fn ambiguity_reports_both_candidates_and_explicit_option() {
    let mut m = Memory::new();
    m.end(0x100);
    m.end(0x200);
    let error = select_chain(&m.0, None, None, &HEADERS).unwrap_err();
    for needle in ["0x100 -> END 0x100", "0x200 -> END 0x200", "--start", "does not prove"] {
        assert!(error.contains(needle), "{error}");
    }
    assert_eq!(select_chain(&m.0, Some(0x200), None, &HEADERS), Ok((0x200, 0x200, 1)));
}

#[test]
fn explicit_choice_outside_known_headers() {
    let mut m = Memory::new();
    m.end(0x300);
    assert_eq!(select_chain(&m.0, Some(0x300), None, &HEADERS), Ok((0x300, 0x300, 1)));
}

#[test]
fn no_valid_header_and_invalid_explicit_header() {
    let mut m = Memory::new();
    assert!(select_chain(&m.0, None, None, &HEADERS).unwrap_err().contains("No frame header"));
    m.end(0x100);
    assert!(select_chain(&m.0, Some(0x200), None, &HEADERS).unwrap_err().contains("--start 0x200"));
}

#[test]
fn savestate_end_selects_a_single_chain() {
    let mut m = Memory::new();
    m.end(0x100);
    m.end(0x200);
    assert_eq!(select_chain(&m.0, None, Some(0x200), &HEADERS), Ok((0x200, 0x200, 1)));
    assert!(select_chain(&m.0, None, Some(0x300), &HEADERS).unwrap_err().contains("END 0x300"));
    assert!(select_chain(&m.0, Some(0x100), Some(0x200), &HEADERS).unwrap_err().contains("END 0x200"));
}

#[test]
fn savestate_end_can_also_be_ambiguous() {
    let mut m = Memory::new();
    m.tag(0x100, 2, 0x300, 0, 0);
    m.tag(0x200, 2, 0x300, 0, 0);
    m.end(0x300);
    assert!(select_chain(&m.0, None, Some(0x300), &HEADERS).unwrap_err().contains("Ambiguous selection"));
    assert_eq!(select_chain(&m.0, Some(0x200), Some(0x300), &HEADERS), Ok((0x200, 0x300, 2)));
}

#[test]
fn invalid_tags_payloads_and_cycles_are_rejected() {
    let mut m = Memory::new();
    m.tag(0x3F0, 0, 0, 1, 0);
    m.tag(0x100, 2, 0x100, 0, 0);
    for start in [-16, 0x401, 0x3F8, 0x3F0] {
        assert_eq!(walk(&m.0, start, None, 500_000).0, None, "start {start:#x}");
        assert!(flatten(&m.0, start).is_err());
    }
    assert_eq!(walk(&m.0, 0x100, None, 3), (None, 3));
}

#[test]
fn flatten_keeps_tte_ref_call_and_ret() {
    // CALL -> REF -> RET -> END, with different loads and TTE on each tag.
    let mut m = Memory::new();
    m.tag(0x100, 5, 0x200, 1, 0x1111);
    m.0[0x110..0x120].fill(b'A');
    m.tag(0x120, 7, 0, 0, 0x4444);
    m.tag(0x200, 3, 0x300, 1, 0x2222);
    m.0[0x300..0x310].fill(b'B');
    m.tag(0x210, 6, 0, 1, 0x3333);
    m.0[0x220..0x230].fill(b'C');
    assert_eq!(select_chain(&m.0, Some(0x100), None, &HEADERS), Ok((0x100, 0x120, 4)));
    let mut expected = Vec::new();
    for (tte, fill) in [(0x1111u64, Some(b'A')), (0x2222, Some(b'B')), (0x3333, Some(b'C')), (0x4444, None)] {
        expected.extend_from_slice(&tte.to_le_bytes());
        if let Some(fill) = fill {
            expected.extend_from_slice(&[fill; 16]);
        }
    }
    assert_eq!(flatten(&m.0, 0x100).unwrap(), expected);
}

// --- gif-inspect -----------------------------------------------------------------------------------------------------

fn gif_tag(registers: &[u64], nloop: u64, format: u64) -> Vec<u8> {
    let nreg = registers.len() as u64 & 15;
    let low = nloop | (1 << 15) | (format << 58) | (nreg << 60);
    let high: u64 = registers.iter().enumerate().map(|(i, r)| r << (4 * i)).sum();
    [low.to_le_bytes(), high.to_le_bytes()].concat()
}

fn quad(low: u64, high: u64) -> Vec<u8> {
    [low.to_le_bytes(), high.to_le_bytes()].concat()
}

#[test]
fn packed_adc_and_xyz3() {
    let mut data = gif_tag(&[5, 13], 2, 0);
    for adc in [0u64, 0, 1, 1] {
        data.extend(quad(160 | (320 << 32), 12345 | (adc << 47)));
    }
    let r = inspect(&data).unwrap();
    assert_eq!(
        (r["vertices"].as_u64(), r["kick_vertices"].as_u64(), r["no_kick_vertices"].as_u64()),
        (Some(4), Some(1), Some(3))
    );
    assert_eq!(r["xyz_bounds"], serde_json::json!([[160, 160], [320, 320], [12345, 12345]]));
}

#[test]
fn xyzf_fog_is_not_adc() {
    let mut data = gif_tag(&[4], 1, 0);
    data.extend(quad(16 | (32 << 32), (0xabcdef << 4) | (255 << 36)));
    let r = inspect(&data).unwrap();
    assert_eq!(r["kick_vertices"], 1);
    assert_eq!(r["xyz_bounds"][2], serde_json::json!([0xabcdef, 0xabcdef]));
}

#[test]
fn reglist_padding_and_next_tag() {
    let value = 16 | (32 << 16) | (123 << 32);
    let mut data = gif_tag(&[5], 1, 1);
    data.extend(quad(value, u64::MAX));
    data.extend(gif_tag(&[13], 1, 1));
    data.extend(quad(value, 0));
    let r = inspect(&data).unwrap();
    assert_eq!((r["vertices"].as_u64(), r["kick_vertices"].as_u64()), (Some(2), Some(1)));
    assert_eq!(r["xyz_bounds"], serde_json::json!([[16, 16], [32, 32], [123, 123]]));
}

#[test]
fn ad_uses_register_coordinates() {
    let mut data = gif_tag(&[14], 1, 0);
    data.extend(quad(16 | (32 << 16) | (456 << 32), 5));
    assert_eq!(inspect(&data).unwrap()["xyz_bounds"], serde_json::json!([[16, 16], [32, 32], [456, 456]]));
}

#[test]
fn nreg_zero_means_sixteen() {
    let mut data = gif_tag(&[15; 16], 1, 0);
    data.extend(vec![0; 16 * 16]);
    let r = inspect(&data).unwrap();
    assert_eq!(r["tags"][0]["nreg"], 16);
    assert_eq!(r["vertices"], 0);
}

#[test]
fn image_and_empty_tag() {
    let mut data = gif_tag(&[15], 2, 2);
    data.extend(vec![0; 32]);
    data.extend(gif_tag(&[5], 0, 0));
    let r = inspect(&data).unwrap();
    assert_eq!(r["tags"].as_array().unwrap().len(), 2);
    assert_eq!(r["vertices"], 0);
}

#[test]
fn pre_only_applies_in_packed() {
    for format in 0..3u64 {
        let mut data = gif_tag(&[15], 0, format);
        let low = u64::from_le_bytes(data[..8].try_into().unwrap()) | (1 << 46) | (4 << 47);
        data[..8].copy_from_slice(&low.to_le_bytes());
        let expected = if format == 0 { serde_json::json!({"4": 1}) } else { serde_json::json!({}) };
        assert_eq!(inspect(&data).unwrap()["prim_types_with_pre"], expected, "format {format}");
    }
}

#[test]
fn incomplete_payloads_and_unsupported_format() {
    let cases: Vec<Vec<u8>> = vec![
        vec![0; 15],
        gif_tag(&[5], 1, 0),
        [gif_tag(&[5], 1, 1), vec![0; 8]].concat(),
        gif_tag(&[15], 1, 2),
        gif_tag(&[15], 0, 3),
    ];
    for data in cases {
        assert!(inspect(&data).is_err());
    }
}

// --- validators ----------------------------------------------------------------------------------------------------------

#[test]
fn minitest_output_is_split_into_results() {
    // A name ends at the first line break or color code, so the statuses are colored like MiniTest prints them.
    let output = "[Suite]: Alpha\n  [Run]: first \u{1b}[32m[Passed]\u{1b}[0m\n  [Run]: second\nmore\u{1b}[31m [Failed]\u{1b}[0m\n[Suite]: Beta\n  [Run]: third \u{1b}[32m[Passed]\u{1b}[0m\nTotal Tests: 3\n";
    let found = results(output);
    assert_eq!(found["Alpha :: first"], "Passed");
    assert_eq!(found["Alpha :: second"], "Failed");
    assert_eq!(found["Beta :: third"], "Passed");
}

#[test]
fn only_new_failures_fail_the_suite() {
    let (green, red, reset) = ("\u{1b}[32m", "\u{1b}[31m", "\u{1b}[0m");
    let output = format!(
        "[Suite]: A\n  [Run]: one {green}[Passed]{reset}\n  [Run]: two {red}[Failed]{reset}\n  [Run]: three {red}[Failed]{reset}\nTotal Tests: 3\n"
    );
    let known_two = "# known\nA :: two\nA :: three\n";
    assert_eq!(check_suite(&output, known_two).code, 0);
    let check = check_suite(&output, "A :: two\n");
    assert_eq!(check.code, 1);
    assert!(check.lines.iter().any(|l| l.contains("New failure: A :: three")));
    assert_eq!(check_suite(&output.replace("Total Tests:", "Tot"), known_two).code, 1);
    assert_eq!(check_suite("nothing\n", "").code, 1);
}

#[test]
fn the_committed_config_is_valid() {
    let code =
        ps2_devtools::validate::validate_config(&["--root".to_owned(), repository().to_string_lossy().into_owned()]);
    assert_eq!(code, 0);
}

#[test]
fn the_recompilers_call_lists_match_the_runtime_header() {
    let root = repository();
    let header = std::fs::read_to_string(root.join("RustPortPS2/native/ps2xRuntime/include/ps2_call_list.h")).unwrap();
    let generated = ps2_devtools::calls::generate(&header).unwrap();
    let committed = std::fs::read_to_string(root.join("RustPortPS2/crates/ps2-recomp/src/calls_generated.rs"))
        .unwrap()
        .replace("\r\n", "\n");
    assert_eq!(committed, generated, "calls_generated.rs is out of date: run `ps2-dev gen-call-list`");
}
