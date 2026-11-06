//! GOW-Port (issue #32, "Rebuild transparency"): what a build in progress is doing, shared by the installation wizard
//! (`installer`) and the rebuilds (`optimizer`): the installer's step, ninja's count, the last lines and, read once a second,
//! how much processor the build's processes use (see `proctree`). The interface shows it in the details window.

use std::time::{Duration, Instant};

use serde::Serialize;

/// GOW-Port (issue #32, "Rebuild transparency"): what a build in progress is doing, published about once a second. The
/// progress bar only moves when ninja finishes a file, and some steps (the native VU1 code, the link, the configure) are one
/// step of many minutes that prints nothing until it ends, so the bar alone looks stuck. The details window shows the step,
/// the last lines, the time since anything was printed and, the real sign of life, how much processor the build uses.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildActivity {
    /// The installer's current step (its `==> ...` line without the arrow), empty before the first one.
    pub step: String,
    /// Ninja's count from its last `[done/total]` line (0/0 before the first).
    pub files_done: u32,
    pub files_total: u32,
    /// What that line said ninja had just finished, without the count.
    pub last_finished: String,
    /// The last lines of the installer's output, oldest first.
    pub recent_lines: Vec<String>,
    /// When the build started and when it last printed a line (milliseconds since the Unix epoch, 0 = not yet).
    pub started_at_ms: u64,
    pub last_output_ms: u64,
    /// Processor used by the build's processes over the last second, in % of one core (200 = two cores busy); -1 unknown.
    pub cpu_percent: f64,
    /// When they last used a noticeable share of the processor (milliseconds since the Unix epoch, 0 = not yet).
    pub last_busy_ms: u64,
}

/// How many lines the details window keeps, and how long a line may be there.
const RECENT_LINES: usize = 8;
const LINE_CHARACTERS: usize = 240;
/// Below this share of one core the build counts as idle (the installer's own PowerShell waiting uses a little).
const BUSY_PERCENT: f64 = 5.0;

/// Milliseconds since the Unix epoch, for the interface (it compares them with its own clock on the same PC).
pub fn unix_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

impl BuildActivity {
    pub fn started(now_ms: u64) -> Self {
        Self { started_at_ms: now_ms, cpu_percent: -1.0, ..Self::default() }
    }

    /// One line of the installer's output.
    pub fn after_line(&mut self, line: &str, now_ms: u64) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        self.last_output_ms = now_ms;
        if let Some(step) = line.strip_prefix("==> ") {
            self.step = step.trim().to_string();
        }
        if let Some((done, total, rest)) = ninja_count(line) {
            self.files_done = done;
            self.files_total = total;
            self.last_finished = rest.to_string();
        }
        let mut kept: String = line.chars().take(LINE_CHARACTERS).collect();
        if kept.len() < line.len() {
            kept.push('…');
        }
        self.recent_lines.push(kept);
        if self.recent_lines.len() > RECENT_LINES {
            let extra = self.recent_lines.len() - RECENT_LINES;
            self.recent_lines.drain(..extra);
        }
    }

    /// One reading of the processor the build used.
    pub fn after_sample(&mut self, cpu_percent: f64, now_ms: u64) {
        self.cpu_percent = cpu_percent;
        if cpu_percent >= BUSY_PERCENT {
            self.last_busy_ms = now_ms;
        }
    }
}

/// `[512/1051] Building CXX object ...` -> (512, 1051, "Building CXX object ...").
fn ninja_count(line: &str) -> Option<(u32, u32, &str)> {
    let rest = line.strip_prefix('[')?;
    let (count, rest) = rest.split_once(']')?;
    let (done, total) = count.split_once('/')?;
    Some((done.trim().parse().ok()?, total.trim().parse().ok()?, rest.trim()))
}

/// While `keep_going` says so, reads once a second how much processor the tree of `pid` used and hands it over (in % of one
/// core, -1 when it cannot be read). Runs on its own thread and ends when `keep_going` returns `false`.
pub fn spawn_tree_sampler(pid: u32, mut keep_going: impl FnMut(f64) -> bool + Send + 'static) {
    std::thread::spawn(move || {
        let mut before = crate::proctree::tree_cpu_time(pid);
        let mut at = Instant::now();
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let now = crate::proctree::tree_cpu_time(pid);
            let wall = at.elapsed();
            at = Instant::now();
            let percent = match (before, now) {
                (Some(before), Some(now)) => crate::proctree::busy_percent(before, now, wall),
                _ => -1.0,
            };
            before = now;
            if !keep_going(percent) {
                return;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_activity_follows_the_installer_and_ninja() {
        let mut activity = BuildActivity::started(1_000);
        assert_eq!(activity.cpu_percent, -1.0);
        activity.after_line("==> Configuring the build (the first time it downloads raylib)", 2_000);
        activity.after_line("   ", 2_500);
        assert_eq!(activity.step, "Configuring the build (the first time it downloads raylib)");
        assert_eq!(activity.last_output_ms, 2_000);
        activity.after_line("==> Building the game (the first time takes a while: ~6,400 files)", 3_000);
        activity.after_line(
            "[512/1051] Building CXX object ps2xRuntime/CMakeFiles/ps2EntryRunner.dir/runner/a.cpp.obj",
            4_000,
        );
        assert_eq!((activity.files_done, activity.files_total), (512, 1051));
        assert_eq!(
            activity.last_finished,
            "Building CXX object ps2xRuntime/CMakeFiles/ps2EntryRunner.dir/runner/a.cpp.obj"
        );
        // A line that only looks like a count leaves the count alone.
        activity.after_line("[generate_vu1] using 4096 captured micro-memories", 5_000);
        assert_eq!((activity.files_done, activity.files_total), (512, 1051));
        assert_eq!(activity.step, "Building the game (the first time takes a while: ~6,400 files)");
        // Only the last lines stay, each one cut to a sane length.
        for index in 0..20 {
            activity.after_line(&format!("line {index} {}", "x".repeat(500)), 6_000 + index);
        }
        assert_eq!(activity.recent_lines.len(), RECENT_LINES);
        assert!(activity.recent_lines[0].starts_with("line 12 "));
        assert!(activity.recent_lines[0].ends_with('…'));
        assert!(activity.recent_lines[0].chars().count() <= LINE_CHARACTERS + 1);
    }

    #[test]
    fn only_a_noticeable_share_of_the_processor_counts_as_busy() {
        let mut activity = BuildActivity::started(0);
        activity.after_sample(2.0, 1_000);
        assert_eq!((activity.cpu_percent, activity.last_busy_ms), (2.0, 0));
        activity.after_sample(380.0, 2_000);
        activity.after_sample(0.5, 3_000);
        assert_eq!((activity.cpu_percent, activity.last_busy_ms), (0.5, 2_000));
    }
}
