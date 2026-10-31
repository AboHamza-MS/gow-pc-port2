//! Small operating-system helpers shared by the installer and the optimizer.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// Free space, in GiB, of the volume that holds `path` (or of its closest existing parent).
pub fn free_space_gb(path: &Path) -> f64 {
    let mut probe = path;
    while !probe.exists() {
        match probe.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => probe = parent,
            _ => break,
        }
    }
    free_bytes(probe) as f64 / (1024.0 * 1024.0 * 1024.0)
}

#[cfg(windows)]
fn free_bytes(path: &Path) -> u64 {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut available = 0u64;
    // SAFETY: `wide` is NUL-terminated and `available` outlives the call; the other out-pointers may be null.
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut available, std::ptr::null_mut(), std::ptr::null_mut()) };
    if ok == 0 {
        0
    } else {
        available
    }
}

#[cfg(unix)]
fn free_bytes(path: &Path) -> u64 {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let Ok(c_path) = CString::new(path.as_os_str().as_bytes()) else {
        return 0;
    };
    // SAFETY: zeroed statvfs is a valid out-parameter and the path is NUL-terminated.
    unsafe {
        let mut stats: libc::statvfs = std::mem::zeroed();
        if libc::statvfs(c_path.as_ptr(), &mut stats) != 0 {
            return 0;
        }
        (stats.f_bavail as u64).saturating_mul(stats.f_frsize as u64)
    }
}

/// Looks `name` up in `PATH` (with the executable extensions on Windows).
pub fn find_in_path(name: &str) -> Option<PathBuf> {
    let extensions: &[&str] = if cfg!(windows) { &["", ".exe", ".cmd", ".bat"] } else { &[""] };
    std::env::split_paths(&std::env::var_os("PATH")?).find_map(|directory| {
        extensions.iter().map(|ext| directory.join(format!("{name}{ext}"))).find(|candidate| candidate.is_file())
    })
}

/// Runs a command and returns its stdout, or `None` if it fails or takes longer than `timeout`.
pub fn output_with_timeout(command: Command, timeout: Duration) -> Option<String> {
    run_with_timeout(command, timeout).filter(|(success, _)| *success).map(|(_, output)| output)
}

/// Runs a command and returns whether it succeeded and its stdout, or `None` if it could not run or took longer than
/// `timeout`. For commands whose exit code carries an answer, like `install.sh --check-deps`.
pub fn run_with_timeout(mut command: Command, timeout: Duration) -> Option<(bool, String)> {
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(command.output());
    });
    match receiver.recv_timeout(timeout) {
        Ok(Ok(output)) => Some((output.status.success(), String::from_utf8_lossy(&output.stdout).into_owned())),
        _ => None,
    }
}

/// `true` if every character is printable ASCII. ps2_recomp and MSVC do not handle accents in the generated paths.
pub fn is_ascii_path(text: &str) -> bool {
    text.chars().all(|c| ('\u{20}'..='\u{7e}').contains(&c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_check_rejects_accents_and_controls() {
        assert!(is_ascii_path(r"C:\GodOfWar-PC"));
        assert!(!is_ascii_path(r"C:\Usuários\x"));
        assert!(!is_ascii_path("C:\\a\tb"));
    }

    #[test]
    fn free_space_of_a_missing_folder_uses_its_parent() {
        let directory = tempfile::tempdir().unwrap();
        let missing = directory.path().join("a/b/c");
        assert!(free_space_gb(&missing) > 0.0);
        assert!((free_space_gb(&missing) - free_space_gb(directory.path())).abs() < 1.0);
    }

    #[test]
    fn path_lookup_finds_something_that_exists() {
        let shell = if cfg!(windows) { "cmd" } else { "sh" };
        assert!(find_in_path(shell).is_some());
        assert!(find_in_path("definitely-not-a-real-program-gow").is_none());
    }
}
