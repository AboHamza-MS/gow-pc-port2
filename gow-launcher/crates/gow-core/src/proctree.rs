//! GOW-Port: the CPU time a process and all its descendants have used, so the interface can tell a build that is working
//! from one that is stuck. Ninja prints a step only when it finishes, so a step that takes half an hour (the native VU1
//! code, the link) writes nothing meanwhile: the only honest sign of life is the processor time the build's processes use.
//!
//! Only the tree of a PID this launcher started is read, and nothing is changed in it.

use std::time::Duration;

/// The CPU time (user plus kernel) used so far by `root` and every process below it, or `None` when it cannot be read
/// (`root` already gone, or no way to list processes here). A process of the tree that exits takes its time with it, so the
/// total can go down between two readings: compare readings a short time apart and ignore a negative difference.
pub fn tree_cpu_time(root: u32) -> Option<Duration> {
    let processes = list()?;
    let mut tree = vec![root];
    let mut index = 0;
    while index < tree.len() {
        let parent = tree[index];
        let children: Vec<u32> = processes
            .iter()
            .filter(|p| p.parent == parent && p.pid != parent && !tree.contains(&p.pid))
            .map(|p| p.pid)
            .collect();
        tree.extend(children);
        index += 1;
    }
    let mut total = Duration::ZERO;
    let mut found = false;
    for process in processes.iter().filter(|p| tree.contains(&p.pid)) {
        if let Some(time) = process.cpu() {
            total += time;
            found = true;
        }
    }
    found.then_some(total)
}

/// The share of one core the tree used between two readings `wall` apart (100 = one core busy), never negative.
pub fn busy_percent(before: Duration, after: Duration, wall: Duration) -> f64 {
    if wall.is_zero() || after < before {
        return 0.0;
    }
    (after - before).as_secs_f64() / wall.as_secs_f64() * 100.0
}

struct Process {
    pid: u32,
    parent: u32,
    #[cfg(unix)]
    ticks: u64,
}

#[cfg(windows)]
fn list() -> Option<Vec<Process>> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    // SAFETY: a snapshot handle is created, read with a correctly sized entry and closed.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut processes = Vec::new();
        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                processes.push(Process { pid: entry.th32ProcessID, parent: entry.th32ParentProcessID });
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);
        Some(processes)
    }
}

#[cfg(windows)]
impl Process {
    fn cpu(&self) -> Option<Duration> {
        use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
        use windows_sys::Win32::System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        // SAFETY: the handle is opened for querying only and closed; the four FILETIMEs are written by the call.
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, self.pid);
            if handle.is_null() {
                return None;
            }
            let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
            let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
            let ok = GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) != 0;
            CloseHandle(handle);
            let hundreds = |time: FILETIME| (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);
            ok.then(|| Duration::from_nanos((hundreds(kernel) + hundreds(user)).saturating_mul(100)))
        }
    }
}

#[cfg(unix)]
fn list() -> Option<Vec<Process>> {
    let entries = std::fs::read_dir("/proc").ok()?;
    Some(
        entries
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().to_str()?.parse::<u32>().ok())
            .filter_map(|pid| parse_stat(pid, &std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?))
            .collect(),
    )
}

/// `/proc/<pid>/stat`: the name in parentheses may hold spaces, so the fields are counted after the last `)`: the parent is
/// the 2nd of them, user and system ticks the 12th and 13th.
#[cfg(unix)]
fn parse_stat(pid: u32, stat: &str) -> Option<Process> {
    let fields: Vec<&str> = stat.rsplit_once(')')?.1.split_whitespace().collect();
    let parent = fields.get(1)?.parse().ok()?;
    let ticks = fields.get(11)?.parse::<u64>().ok()? + fields.get(12)?.parse::<u64>().ok()?;
    Some(Process { pid, parent, ticks })
}

#[cfg(unix)]
impl Process {
    fn cpu(&self) -> Option<Duration> {
        // SAFETY: sysconf only reads a configuration value.
        let per_second = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
        (per_second > 0).then(|| Duration::from_secs_f64(self.ticks as f64 / per_second as f64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_share_of_a_core_is_never_negative() {
        let second = Duration::from_secs(1);
        assert_eq!(busy_percent(Duration::from_millis(500), Duration::from_millis(2500), second), 200.0);
        // A child that exited took its time with it: no activity is invented, none is negative.
        assert_eq!(busy_percent(Duration::from_secs(5), Duration::from_secs(3), second), 0.0);
        assert_eq!(busy_percent(Duration::ZERO, Duration::from_secs(1), Duration::ZERO), 0.0);
    }

    #[test]
    fn this_process_is_its_own_tree_and_uses_cpu() {
        // Burn a little CPU so the reading cannot be zero.
        let start = std::time::Instant::now();
        let mut value = 0u64;
        while start.elapsed() < Duration::from_millis(60) {
            value = value.wrapping_mul(6364136223846793005).wrapping_add(1);
        }
        assert_ne!(value, 42);
        let time = tree_cpu_time(std::process::id()).expect("the test process can read its own times");
        assert!(time > Duration::ZERO);
        assert!(tree_cpu_time(u32::MAX - 7).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn a_process_name_with_spaces_and_parentheses_is_parsed() {
        let stat = "4242 (my (odd) name) S 17 4242 4242 0 -1 4194560 100 0 0 0 250 50 0 0 20 0 1 0 100 0 0";
        let process = parse_stat(4242, stat).unwrap();
        assert_eq!((process.pid, process.parent, process.ticks), (4242, 17, 300));
    }
}
