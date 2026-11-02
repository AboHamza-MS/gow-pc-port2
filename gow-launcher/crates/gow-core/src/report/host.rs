//! Facts about the PC a report states (operating system, processor, memory, graphics adapters) and the names the report
//! must hide (user, computer, folders). Detected once; tests build their own.

use std::path::PathBuf;

use serde::Serialize;

/// A graphics adapter as the system describes it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Adapter {
    pub name: String,
    pub driver_version: String,
    pub driver_date: String,
    pub provider: String,
}

#[derive(Debug, Clone, Default)]
pub struct HostFacts {
    pub os: Option<String>,
    pub cpu: Option<String>,
    pub logical_cores: Option<usize>,
    /// Physical memory, in MiB.
    pub ram_mb: Option<u64>,
    /// Windows: the display adapters of the registry. Linux: empty (the card list of the launcher has them).
    pub adapters: Vec<Adapter>,
    pub user_name: Option<String>,
    pub computer_name: Option<String>,
    pub home: Option<PathBuf>,
    pub temp: PathBuf,
    /// The folder of the launcher's own executable.
    pub launcher_dir: Option<PathBuf>,
}

impl HostFacts {
    pub fn detect() -> Self {
        let mut facts = Self {
            logical_cores: std::thread::available_parallelism().ok().map(|n| n.get()),
            user_name: first_env(&["USERNAME", "USER", "LOGNAME"]),
            computer_name: first_env(&["COMPUTERNAME", "HOSTNAME"]).or_else(hostname_file),
            home: dirs::home_dir(),
            temp: std::env::temp_dir(),
            launcher_dir: std::env::current_exe().ok().and_then(|exe| exe.parent().map(PathBuf::from)),
            ..Self::default()
        };
        #[cfg(windows)]
        windows::fill(&mut facts);
        #[cfg(not(windows))]
        linux_fill(&mut facts);
        facts
    }
}

fn first_env(names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| std::env::var(name).ok().filter(|value| !value.trim().is_empty()))
}

/// Linux keeps the host name in a file when the shell does not export `HOSTNAME`.
fn hostname_file() -> Option<String> {
    std::fs::read_to_string("/etc/hostname").ok().map(|text| text.trim().to_string()).filter(|text| !text.is_empty())
}

#[cfg(not(windows))]
fn linux_fill(facts: &mut HostFacts) {
    let read = |path: &str| std::fs::read_to_string(path).unwrap_or_default();
    facts.os = pretty_name(&read("/etc/os-release"));
    facts.cpu = cpu_model(&read("/proc/cpuinfo"));
    facts.ram_mb = mem_total_mb(&read("/proc/meminfo"));
}

/// `PRETTY_NAME` of an `os-release` file.
pub fn pretty_name(os_release: &str) -> Option<String> {
    os_release.lines().find_map(|line| {
        let value = line.strip_prefix("PRETTY_NAME=")?;
        Some(value.trim().trim_matches('"').trim_matches('\'').to_string()).filter(|text| !text.is_empty())
    })
}

/// First `model name` of `/proc/cpuinfo`.
pub fn cpu_model(cpuinfo: &str) -> Option<String> {
    cpuinfo.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        (key.trim() == "model name").then(|| value.trim().to_string()).filter(|text| !text.is_empty())
    })
}

/// `MemTotal` of `/proc/meminfo`, in MiB.
pub fn mem_total_mb(meminfo: &str) -> Option<u64> {
    let line = meminfo.lines().find(|line| line.starts_with("MemTotal:"))?;
    let kib: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kib / 1024)
}

/// Windows 11 still reports `ProductName` "Windows 10 ..." in the registry; from build 22000 on it is Windows 11.
pub fn windows_product(name: &str, display_version: &str, build: &str) -> String {
    let number: u32 = build.trim().parse().unwrap_or(0);
    let name = if number >= 22000 { name.replacen("Windows 10", "Windows 11", 1) } else { name.to_string() };
    let mut text = name.trim().to_string();
    if !display_version.trim().is_empty() {
        text.push_str(&format!(" {}", display_version.trim()));
    }
    if number > 0 {
        text.push_str(&format!(" (build {number})"));
    }
    text
}

#[cfg(windows)]
mod windows {
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ};
    use winreg::RegKey;

    use super::{windows_product, Adapter, HostFacts};

    const DISPLAY_CLASS: &str = r"SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}";

    fn machine() -> RegKey {
        RegKey::predef(HKEY_LOCAL_MACHINE)
    }

    fn text(key: &RegKey, name: &str) -> String {
        key.get_value::<String, _>(name).unwrap_or_default()
    }

    pub fn fill(facts: &mut HostFacts) {
        if let Ok(key) = machine().open_subkey_with_flags(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion", KEY_READ) {
            let name = text(&key, "ProductName");
            if !name.is_empty() {
                facts.os =
                    Some(windows_product(&name, &text(&key, "DisplayVersion"), &text(&key, "CurrentBuildNumber")));
            }
        }
        if let Ok(key) = machine().open_subkey_with_flags(r"HARDWARE\DESCRIPTION\System\CentralProcessor\0", KEY_READ) {
            let name = text(&key, "ProcessorNameString");
            facts.cpu = Some(name.trim().to_string()).filter(|name| !name.is_empty());
        }
        facts.ram_mb = total_memory_mb();
        facts.adapters = adapters();
    }

    fn total_memory_mb() -> Option<u64> {
        use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        // SAFETY: an all-zero MEMORYSTATUSEX is valid once `dwLength` is set, which is what the call requires.
        let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
        status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        // SAFETY: `status` is a live, correctly sized structure.
        let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
        (ok != 0).then_some(status.ullTotalPhys / (1024 * 1024))
    }

    /// The display adapters under the display class key: numbered subkeys (`0000`, `0001`...), each with the driver's own
    /// description, version and date. Read-only, no elevation needed.
    fn adapters() -> Vec<Adapter> {
        let Ok(class) = machine().open_subkey_with_flags(DISPLAY_CLASS, KEY_READ) else { return Vec::new() };
        class
            .enum_keys()
            .flatten()
            .filter(|name| name.len() == 4 && name.chars().all(|c| c.is_ascii_digit()))
            .filter_map(|name| class.open_subkey_with_flags(&name, KEY_READ).ok())
            .map(|key| Adapter {
                name: text(&key, "DriverDesc"),
                driver_version: text(&key, "DriverVersion"),
                driver_date: text(&key, "DriverDate"),
                provider: text(&key, "ProviderName"),
            })
            .filter(|adapter| !adapter.name.is_empty())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_release_cpuinfo_and_meminfo_are_read() {
        assert_eq!(
            pretty_name("NAME=\"Ubuntu\"\nPRETTY_NAME=\"Ubuntu 24.04.1 LTS\"\nID=ubuntu\n").as_deref(),
            Some("Ubuntu 24.04.1 LTS")
        );
        assert_eq!(pretty_name("NAME=x\n"), None);
        assert_eq!(
            cpu_model("processor\t: 0\nmodel name\t: AMD Ryzen 5 5600\ncpu MHz\t: 3500\n").as_deref(),
            Some("AMD Ryzen 5 5600")
        );
        assert_eq!(cpu_model("processor : 0\n"), None);
        assert_eq!(mem_total_mb("MemTotal:       16384000 kB\nMemFree: 1 kB\n"), Some(16000));
        assert_eq!(mem_total_mb(""), None);
    }

    #[test]
    fn windows_eleven_is_named_by_its_build_number() {
        assert_eq!(windows_product("Windows 10 Pro", "23H2", "22631"), "Windows 11 Pro 23H2 (build 22631)");
        assert_eq!(windows_product("Windows 10 Home", "22H2", "19045"), "Windows 10 Home 22H2 (build 19045)");
        assert_eq!(windows_product("Windows 7", "", ""), "Windows 7");
    }

    #[test]
    fn detect_finds_the_basics_on_this_pc() {
        let facts = HostFacts::detect();
        assert!(facts.logical_cores.is_some_and(|n| n >= 1));
        assert!(!facts.temp.as_os_str().is_empty());
    }
}
