//! Runs `install.ps1` (Windows) or `install.sh` (Linux), the installer of the port, as a child, line by line. Shared by
//! the installation wizard and the background optimizer.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

/// `'text'` with embedded quotes doubled: a PowerShell literal string.
pub fn quote_for_powershell(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// A running script. Dropping it leaves the script running; call [`ScriptRun::kill_tree`] to stop it.
#[derive(Debug, Clone, Copy)]
pub struct ScriptRun {
    pid: u32,
}

impl ScriptRun {
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Kills the whole tree: PowerShell, ninja, the compilers and the game of the optimization stage. Only the tree
    /// of the PID this launcher opened is touched.
    pub fn kill_tree(&self) {
        kill_tree(self.pid);
    }
}

/// Splits a byte stream into lines on `\n`, `\r` or `\r\n`. Empty lines are dropped.
#[derive(Default)]
pub struct LineSplitter {
    buffer: Vec<u8>,
}

impl LineSplitter {
    pub fn push(&mut self, bytes: &[u8], mut on_line: impl FnMut(&str)) {
        self.buffer.extend_from_slice(bytes);
        while let Some(end) = self.buffer.iter().position(|&b| b == b'\n' || b == b'\r') {
            let line: Vec<u8> = self.buffer.drain(..=end).collect();
            let text = String::from_utf8_lossy(&line[..line.len() - 1]);
            let text = text.trim();
            if !text.is_empty() {
                on_line(text);
            }
        }
    }

    pub fn finish(&mut self, mut on_line: impl FnMut(&str)) {
        let rest = std::mem::take(&mut self.buffer);
        let text = String::from_utf8_lossy(&rest);
        if !text.trim().is_empty() {
            on_line(text.trim());
        }
    }
}

pub struct ScriptOptions {
    pub script: std::path::PathBuf,
    /// Arguments already formatted for the platform: PowerShell source on Windows (see [`quote_for_powershell`]), e.g.
    /// `-Destination 'C:\x'`, and plain words for `bash` on Linux, e.g. `--destination /home/me/gow`.
    pub arguments: Vec<String>,
    /// Lower the priority so the build does not compete with the game (Windows `BELOW_NORMAL_PRIORITY_CLASS`).
    pub below_normal_priority: bool,
}

/// The command line PowerShell is given: UTF-8 output so the titles read correctly, and exit code 0 only when the
/// script ends without an error.
pub fn powershell_command(script: &Path, arguments: &[String]) -> String {
    format!(
        "[Console]::OutputEncoding=[System.Text.Encoding]::UTF8; $global:LASTEXITCODE=0; & {} {}; if (-not $?) {{ exit 1 }}; exit $LASTEXITCODE",
        quote_for_powershell(&script.to_string_lossy()),
        arguments.join(" ")
    )
}

/// The program and arguments that run `script`: PowerShell on Windows, `bash` on Linux.
pub fn command_for(script: &Path, arguments: &[String], windows: bool) -> (&'static str, Vec<String>) {
    if windows {
        (
            "powershell.exe",
            vec![
                "-NoProfile".to_string(),
                "-ExecutionPolicy".to_string(),
                "Bypass".to_string(),
                "-Command".to_string(),
                powershell_command(script, arguments),
            ],
        )
    } else {
        let mut all = vec![script.to_string_lossy().into_owned()];
        all.extend(arguments.iter().cloned());
        ("bash", all)
    }
}

/// Starts the script. `on_line` receives stdout and stderr merged, `on_exit` the exit code (`None` if killed).
pub fn run(
    options: ScriptOptions,
    on_line: impl FnMut(&str) + Send + 'static,
    on_exit: impl FnOnce(Option<i32>) + Send + 'static,
) -> std::io::Result<ScriptRun> {
    let working_directory = options.script.parent().map(Path::to_path_buf).unwrap_or_default();
    let (program, program_arguments) = command_for(&options.script, &options.arguments, cfg!(windows));
    let mut command = Command::new(program);
    command
        .args(&program_arguments)
        .current_dir(working_directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
        command.creation_flags(
            CREATE_NO_WINDOW | if options.below_normal_priority { BELOW_NORMAL_PRIORITY_CLASS } else { 0 },
        );
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Its own process group, so the whole tree can be signalled.
        command.process_group(0);
    }
    let mut child = command.spawn()?;
    let pid = child.id();
    let on_line = Arc::new(Mutex::new(on_line));
    let mut readers = Vec::new();
    for stream in [
        child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>),
        child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let on_line = Arc::clone(&on_line);
        readers.push(std::thread::spawn(move || {
            let mut stream = stream;
            let mut splitter = LineSplitter::default();
            let mut chunk = [0u8; 4096];
            loop {
                match stream.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => splitter.push(&chunk[..count], |line| (on_line.lock().unwrap())(line)),
                }
            }
            splitter.finish(|line| (on_line.lock().unwrap())(line));
        }));
    }
    std::thread::spawn(move || {
        let status = child.wait();
        for reader in readers {
            let _ = reader.join();
        }
        on_exit(status.ok().and_then(|s| s.code()));
    });
    Ok(ScriptRun { pid })
}

#[cfg(windows)]
fn kill_tree(pid: u32) {
    use std::os::windows::process::CommandExt;
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .creation_flags(0x0800_0000)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(unix)]
fn kill_tree(pid: u32) {
    // SAFETY: signals the process group this launcher created for the script.
    unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_doubles_single_quotes() {
        assert_eq!(quote_for_powershell(r"C:\it's here"), r"'C:\it''s here'");
    }

    #[test]
    fn the_command_wraps_the_script_and_its_arguments() {
        let text = powershell_command(Path::new(r"C:\kit\install.ps1"), &["-Iso".into(), "'a.iso'".into()]);
        assert!(text.contains(r"& 'C:\kit\install.ps1' -Iso 'a.iso';"));
        assert!(text.ends_with("exit $LASTEXITCODE"));
    }

    #[test]
    fn windows_runs_powershell_and_linux_runs_bash() {
        let (program, arguments) =
            command_for(Path::new(r"C:\kit\install.ps1"), &["-Iso".into(), "'a.iso'".into()], true);
        assert_eq!(program, "powershell.exe");
        assert_eq!(&arguments[..4], ["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command"]);
        assert!(arguments[4].contains(r"& 'C:\kit\install.ps1' -Iso 'a.iso';"));

        let (program, arguments) = command_for(
            Path::new("/kit/install.sh"),
            &["/home/me/God of War.iso".into(), "--no-optimize".into()],
            false,
        );
        assert_eq!(program, "bash");
        assert_eq!(arguments, ["/kit/install.sh", "/home/me/God of War.iso", "--no-optimize"]);
    }

    #[test]
    fn lines_are_split_on_cr_lf_and_crlf() {
        let mut lines = Vec::new();
        let mut splitter = LineSplitter::default();
        splitter.push(b"one\r\ntw", |l| lines.push(l.to_string()));
        splitter.push(b"o\r[3/9] x\n\n  \nrest", |l| lines.push(l.to_string()));
        splitter.finish(|l| lines.push(l.to_string()));
        assert_eq!(lines, ["one", "two", "[3/9] x", "rest"]);
    }

    #[cfg(windows)]
    #[test]
    fn a_real_powershell_script_streams_lines_and_an_exit_code() {
        let directory = tempfile::tempdir().unwrap();
        let script = directory.path().join("hello.ps1");
        std::fs::write(
            &script,
            "param([string]$Name, [int]$Code = 0)\nWrite-Output \"==> hello $Name\"\n[Console]::Error.WriteLine('oops')\nexit $Code\n",
        )
        .unwrap();
        let run_once = |code: &str| {
            let (sender, receiver) = std::sync::mpsc::channel();
            let lines = Arc::new(Mutex::new(Vec::<String>::new()));
            let sink = Arc::clone(&lines);
            run(
                ScriptOptions {
                    script: script.clone(),
                    arguments: vec!["-Name".into(), "'it''s'".into(), "-Code".into(), code.into()],
                    below_normal_priority: false,
                },
                move |line| sink.lock().unwrap().push(line.to_string()),
                move |code| sender.send(code).unwrap(),
            )
            .unwrap();
            let code = receiver.recv_timeout(std::time::Duration::from_secs(60)).unwrap();
            let collected = lines.lock().unwrap().clone();
            (code, collected)
        };
        let (code, lines) = run_once("0");
        assert_eq!(code, Some(0));
        assert!(lines.iter().any(|l| l == "==> hello it's"), "{lines:?}");
        assert!(lines.iter().any(|l| l.contains("oops")), "{lines:?}");
        // The wrapper turns any failure of the script into exit code 1, as the former launcher did.
        assert_eq!(run_once("3").0, Some(1));
    }
}
