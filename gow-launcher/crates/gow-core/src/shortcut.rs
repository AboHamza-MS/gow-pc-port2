//! Application-menu entry and desktop shortcut.

use std::path::{Path, PathBuf};
use std::process::Command;

pub const SHORTCUT_NAME: &str = "God Of War - PC Launcher";

/// Contents of the Linux `.desktop` file, with the "Play now" action (`--play`).
pub fn desktop_entry(executable: &Path, icon: &Path) -> String {
    let executable = executable.display();
    format!(
        "[Desktop Entry]\nType=Application\nName=God Of War: PC Launcher\nGenericName=Game Launcher\n\
         Comment=God of War (PS2) PC port\nExec=\"{executable}\"\nIcon={}\nTerminal=false\nCategories=Game;\n\
         StartupWMClass=PC-launcher\nActions=play;\n\n[Desktop Action play]\nName=Play now\n\
         Name[pt_BR]=Jogar agora\nExec=\"{executable}\" --play\n",
        icon.display()
    )
}

/// Creates the shortcut. `desktop = false` puts it in the application menu (Start menu on Windows).
pub fn create(executable: &Path, icon: &Path, desktop: bool) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        let _ = icon;
        let folder = if desktop {
            dirs::desktop_dir()
        } else {
            dirs::data_dir().map(|roaming| roaming.join(r"Microsoft\Windows\Start Menu\Programs"))
        }
        .ok_or_else(|| std::io::Error::other("shortcut folder not found"))?;
        std::fs::create_dir_all(&folder)?;
        let link = folder.join(format!("{SHORTCUT_NAME}.lnk"));
        let quote = |path: &Path| path.to_string_lossy().replace('\'', "''");
        let script = format!(
            "$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{}'); $s.TargetPath='{}'; $s.WorkingDirectory='{}'; $s.Save()",
            quote(&link),
            quote(executable),
            quote(executable.parent().unwrap_or_else(|| Path::new("."))),
        );
        let mut command = Command::new("powershell.exe");
        command.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let status = command.status()?;
        if status.success() {
            Ok(())
        } else {
            Err(std::io::Error::other("could not create the shortcut"))
        }
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("true");
        let content = desktop_entry(executable, icon);
        let write = |path: PathBuf, executable_bit: bool| -> std::io::Result<()> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, &content)?;
            if executable_bit {
                use std::os::unix::fs::PermissionsExt;
                let mut permissions = std::fs::metadata(&path)?.permissions();
                permissions.set_mode(permissions.mode() | 0o100);
                std::fs::set_permissions(&path, permissions)?;
            }
            Ok(())
        };
        let applications = dirs::data_dir().map(|d| d.join("applications")).unwrap_or_default();
        write(applications.join("PC-launcher.desktop"), false)?;
        if desktop {
            if let Some(folder) = dirs::desktop_dir() {
                write(folder.join("PC-launcher.desktop"), true)?;
            }
        }
        Ok(())
    }
}

/// Opens a file or folder with the system's default program.
pub fn open_path(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // `explorer` handles both folders and files; it returns non-zero even on success, so the status is ignored.
        Command::new("explorer.exe").arg(path).creation_flags(0x0800_0000).spawn().map(|_| ())
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("open").arg(path).spawn().map(|_| ())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let mut command = Command::new("xdg-open");
        command.arg(path);
        crate::system::sanitize_child_environment(&mut command);
        command.spawn().map(|_| ())
    }
}

/// The folder to open for `path`: itself if it is a folder, else its parent.
pub fn folder_of(path: &Path) -> PathBuf {
    if path.is_file() {
        path.parent().map(Path::to_path_buf).unwrap_or_default()
    } else {
        path.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_desktop_entry_has_the_play_action() {
        let text = desktop_entry(Path::new("/opt/gow/gow-launcher"), Path::new("/home/u/icon.png"));
        assert!(text.contains("Exec=\"/opt/gow/gow-launcher\"\n"));
        assert!(text.contains("Exec=\"/opt/gow/gow-launcher\" --play"));
        assert!(text.contains("Icon=/home/u/icon.png"));
        assert!(text.contains("Name[pt_BR]=Jogar agora"));
    }

    #[test]
    fn a_file_opens_its_folder() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("a.log");
        std::fs::write(&file, "x").unwrap();
        assert_eq!(folder_of(&file), directory.path());
        assert_eq!(folder_of(directory.path()), directory.path());
    }
}
