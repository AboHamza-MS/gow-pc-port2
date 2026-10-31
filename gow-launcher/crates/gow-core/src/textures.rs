//! Automatic download and installation of the community texture pack.
//!
//! The pack is a ZIP of PCSX2-format replacement textures (`<name>.png` / `<name>.dds`). It is downloaded with a
//! streaming progress callback and extracted next to `<installation>/textures/replacements`; only when every file is
//! written does the new folder take the old one's place (so the pack replaces what the user had, and an install that
//! fails or is interrupted leaves the old textures alone), and the downloaded ZIP is deleted afterwards. The prefix the
//! pack uses inside the archive (`replacements/`, `textures/SCUS-97399/replacements/` or a single wrapper folder) is
//! detected and stripped, and entries that try to escape the destination are refused.

use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// ZIP of the pack.
pub const PACK_URL: &str = "https://gow-textures.kyantech.cloud/gow-textures.zip";
/// Metadata of the pack: size, ETag and upload date.
pub const PACK_META_URL: &str = "https://gow-textures.kyantech.cloud/_meta";
/// Name of the marker written next to `replacements/` once a pack is installed.
const MARKER: &str = "pack.json";
/// Downloaded archives are written here (inside `textures/`) and deleted after a successful extraction.
const ARCHIVE: &str = ".gow-textures.zip";
/// Room the extraction is assumed to need on top of the archive itself.
const EXTRACT_FACTOR: f64 = 2.0;
/// The new folder is written here, and only renamed to `replacements` once it is complete.
const STAGING: &str = ".replacements-new";
/// The old folder waits here during the swap, so it can be put back if the swap fails.
const BACKUP: &str = ".replacements-old";
/// Share of the overall progress given to the download; the extraction takes the rest. The pack is mostly PNG and DDS,
/// which barely compress, so both phases move about the same number of bytes, but a download crosses the network and
/// the extraction only the disk: this keeps the bar moving at a steady pace instead of racing through its second half.
const DOWNLOAD_SHARE: f64 = 0.8;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackMeta {
    pub size: u64,
    pub etag: String,
    pub uploaded: String,
}

/// What the caller shows while the pack is handled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PackStage {
    Downloading,
    Extracting,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackProgress {
    pub stage: PackStage,
    /// Bytes downloaded, or files extracted.
    pub done: u64,
    /// Content length, or number of files to extract (0 when unknown).
    pub total: u64,
    /// Progress of the whole installation, 0 to 1: the download first, then the extraction, one bar that never goes back.
    pub overall: f64,
}

/// Progress of the download: `done` of `total` bytes (`total` 0 when the server does not say).
fn downloading(done: u64, total: u64) -> PackProgress {
    let ratio = if total > 0 { (done as f64 / total as f64).min(1.0) } else { 0.0 };
    PackProgress { stage: PackStage::Downloading, done, total, overall: DOWNLOAD_SHARE * ratio }
}

/// Progress of the extraction: `files` of `files_total` written, `bytes` of `bytes_total` (the bar follows the bytes,
/// because textures differ a lot in size).
fn extracting(files: u64, files_total: u64, bytes: u64, bytes_total: u64) -> PackProgress {
    let ratio = if bytes_total > 0 {
        (bytes as f64 / bytes_total as f64).min(1.0)
    } else if files_total > 0 {
        (files as f64 / files_total as f64).min(1.0)
    } else {
        0.0
    };
    PackProgress {
        stage: PackStage::Extracting,
        done: files,
        total: files_total,
        overall: DOWNLOAD_SHARE + (1.0 - DOWNLOAD_SHARE) * ratio,
    }
}

/// State of the installation, shown by the launcher.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackStatus {
    pub installed: bool,
    pub files: u64,
    pub bytes: u64,
    pub etag: String,
    /// Upload date of the pack, as the server reports it.
    #[serde(default)]
    pub uploaded: String,
}

#[derive(Debug, thiserror::Error)]
pub enum PackError {
    #[error("this installation has no game folder, so the textures have nowhere to go")]
    NoGameFolder,
    #[error("the game installation folder does not exist: {0}")]
    MissingFolder(String),
    #[error("not enough free space in {0}: the pack needs about {1:.1} GB")]
    NoSpace(String, f64),
    #[error("could not reach the texture server: {0}")]
    Network(String),
    #[error("the texture server answered with HTTP {0}")]
    Http(u16),
    #[error("the downloaded archive is not a valid ZIP: {0}")]
    Archive(String),
    #[error("writing the textures failed: {0}")]
    Io(String),
}

impl From<std::io::Error> for PackError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

impl PackError {
    /// Stable code the interface translates: the launcher must not show these English messages to the player.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NoGameFolder => "noGame",
            Self::MissingFolder(_) => "noFolder",
            Self::NoSpace(_, _) => "noSpace",
            Self::Network(_) | Self::Http(_) => "network",
            Self::Archive(_) => "archive",
            Self::Io(_) => "io",
        }
    }
}

/// Where the pack comes from; the launcher uses [`PackSource::default`].
#[derive(Clone, Debug)]
pub struct PackSource {
    pub pack_url: String,
    pub meta_url: String,
}

impl Default for PackSource {
    fn default() -> Self {
        Self { pack_url: PACK_URL.to_string(), meta_url: PACK_META_URL.to_string() }
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_global(None)
        .user_agent(concat!("gow-launcher/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// Size, ETag and upload date of the pack. Cheap: it is what the page shows before downloading.
pub fn meta(source: &PackSource) -> Result<PackMeta, PackError> {
    let mut response = agent().get(&source.meta_url).call().map_err(|error| PackError::Network(error.to_string()))?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(PackError::Http(status));
    }
    let body = response.body_mut().read_to_string().map_err(|error| PackError::Network(error.to_string()))?;
    serde_json::from_str::<PackMeta>(&body).map_err(|error| PackError::Network(format!("unexpected metadata: {error}")))
}

/// What is installed now, read from the marker (or counted when there is no marker, an older manual copy).
pub fn status(elf: &str) -> PackStatus {
    let Some(folder) = replacement_folder(elf) else { return PackStatus::default() };
    let marker = folder.parent().map(|parent| parent.join(MARKER));
    if let Some(marker) = marker {
        if let Ok(text) = fs::read_to_string(&marker) {
            if let Ok(status) = serde_json::from_str::<PackStatus>(&text) {
                if status.installed && folder.is_dir() {
                    return status;
                }
            }
        }
    }
    let (files, bytes) = count(&folder);
    PackStatus { installed: files > 0, files, bytes, etag: String::new(), uploaded: String::new() }
}

/// Downloads the pack, empties `textures/replacements` into the new files and deletes the archive.
pub fn install(elf: &str, source: &PackSource, progress: &dyn Fn(PackProgress)) -> Result<PackStatus, PackError> {
    let Some(replacements) = replacement_folder(elf) else { return Err(PackError::NoGameFolder) };
    let textures = replacements.parent().map(Path::to_path_buf).ok_or(PackError::NoGameFolder)?;
    if !textures.is_dir() {
        // Older installations may not have the folder yet: `textures` sits next to `game/`.
        if textures.parent().is_some_and(|parent| parent.is_dir()) {
            fs::create_dir_all(&textures)?;
        } else {
            return Err(PackError::MissingFolder(textures.display().to_string()));
        }
    }
    let meta = meta(source).ok();
    let size = meta.as_ref().map_or(0, |meta| meta.size);
    // The archive plus the extracted files live side by side while the swap happens.
    let needed = size as f64 * EXTRACT_FACTOR / (1024.0 * 1024.0 * 1024.0);
    let free = crate::system::free_space_gb(&textures);
    if needed > 0.0 && free < needed {
        return Err(PackError::NoSpace(textures.display().to_string(), needed));
    }

    recover(&textures);
    let archive = textures.join(ARCHIVE);
    let downloaded = download(source, &archive, size, progress)?;
    let extracted = extract(&archive, &textures, progress);
    let _ = fs::remove_file(&archive);
    let (files, bytes) = extracted?;

    let status = PackStatus {
        installed: true,
        files,
        bytes,
        etag: meta.as_ref().map_or_else(String::new, |meta| meta.etag.clone()),
        uploaded: meta.map_or_else(String::new, |meta| meta.uploaded),
    };
    let marker = serde_json::to_string_pretty(&status).unwrap_or_default();
    fs::write(textures.join(MARKER), marker)?;
    let _ = downloaded;
    Ok(status)
}

/// Cleans up after an install that was interrupted (the launcher closed, the PC turned off): leftovers of the staging
/// folder and of the archive go away, and if the old folder was moved aside but never replaced, it comes back.
fn recover(textures: &Path) {
    let replacements = textures.join("replacements");
    let backup = textures.join(BACKUP);
    if !replacements.exists() && backup.is_dir() {
        let _ = fs::rename(&backup, &replacements);
    }
    let _ = fs::remove_dir_all(textures.join(STAGING));
    let _ = fs::remove_dir_all(&backup);
    let _ = fs::remove_file(textures.join(ARCHIVE));
}

fn replacement_folder(elf: &str) -> Option<PathBuf> {
    crate::game::texture_replacements_folder(elf)
}

fn count(folder: &Path) -> (u64, u64) {
    let mut files = 0;
    let mut bytes = 0;
    let mut stack = vec![folder.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let Ok(entries) = fs::read_dir(&directory) else { continue };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                stack.push(entry.path());
            } else if kind.is_file() && is_texture(&entry.path()) {
                files += 1;
                bytes += entry.metadata().map(|meta| meta.len()).unwrap_or(0);
            }
        }
    }
    (files, bytes)
}

/// Downloads `source` into `archive`, reporting bytes as they arrive. Returns the bytes written.
fn download(
    source: &PackSource,
    archive: &Path,
    expected: u64,
    progress: &dyn Fn(PackProgress),
) -> Result<u64, PackError> {
    let mut response = agent().get(&source.pack_url).call().map_err(|error| PackError::Network(error.to_string()))?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(PackError::Http(status));
    }
    let total = response.body().content_length().unwrap_or(expected);
    let mut reader = response.body_mut().as_reader();
    progress(downloading(0, total));

    let mut writer = BufWriter::new(File::create(archive)?);
    let mut buffer = vec![0u8; 1 << 20];
    let mut done = 0u64;
    let mut reported = Instant::now();
    loop {
        let read = reader.read(&mut buffer).map_err(|error| PackError::Network(error.to_string()))?;
        if read == 0 {
            break;
        }
        writer.write_all(&buffer[..read])?;
        done += read as u64;
        if reported.elapsed() >= Duration::from_millis(200) {
            progress(downloading(done, total));
            reported = Instant::now();
        }
    }
    writer.flush()?;
    drop(writer);
    if total > 0 && done != total {
        return Err(PackError::Network(format!("the download ended at {done} of {total} bytes")));
    }
    progress(downloading(done, total));
    Ok(done)
}

/// Extracts the pack next to `textures/replacements` and then puts it in its place: the pack replaces whatever the
/// user had, but only once it is complete. If anything fails, the old folder is untouched. Returns the number of files
/// and their total size.
fn extract(archive_path: &Path, textures: &Path, progress: &dyn Fn(PackProgress)) -> Result<(u64, u64), PackError> {
    let (entries, bytes_total) = candidates(archive_path)?;
    let files_total = entries.len() as u64;
    progress(extracting(0, files_total, 0, bytes_total));

    let staging = textures.join(STAGING);
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging)?;
    let written = write_entries(archive_path, &entries, &staging, bytes_total, progress);
    let (files, bytes) = match written {
        Ok(counts) => counts,
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
    };
    swap(textures, &staging)?;
    progress(extracting(files, files_total, bytes, bytes_total));
    Ok((files, bytes))
}

fn write_entries(
    archive_path: &Path,
    entries: &[(usize, PathBuf)],
    staging: &Path,
    bytes_total: u64,
    progress: &dyn Fn(PackProgress),
) -> Result<(u64, u64), PackError> {
    let files_total = entries.len() as u64;
    let mut archive = open(archive_path)?;
    let mut files = 0u64;
    let mut bytes = 0u64;
    let mut reported = Instant::now();
    for (index, relative) in entries {
        let mut entry = archive.by_index(*index).map_err(|error| PackError::Archive(error.to_string()))?;
        let target = staging.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut out = BufWriter::new(File::create(&target)?);
        let written = std::io::copy(&mut entry, &mut out).map_err(|error| PackError::Archive(error.to_string()))?;
        out.flush()?;
        files += 1;
        bytes += written;
        if reported.elapsed() >= Duration::from_millis(200) {
            progress(extracting(files, files_total, bytes, bytes_total));
            reported = Instant::now();
        }
    }
    Ok((files, bytes))
}

/// Puts the finished `staging` folder where `replacements` is. The old folder moves aside first and comes back if the
/// rename fails (for example because something still has a file open), so there is always a complete folder.
fn swap(textures: &Path, staging: &Path) -> Result<(), PackError> {
    let replacements = textures.join("replacements");
    let backup = textures.join(BACKUP);
    let _ = fs::remove_dir_all(&backup);
    let had_old = replacements.exists();
    if had_old {
        if let Err(error) = fs::rename(&replacements, &backup) {
            let _ = fs::remove_dir_all(staging);
            return Err(error.into());
        }
    }
    if let Err(error) = fs::rename(staging, &replacements) {
        if had_old {
            let _ = fs::rename(&backup, &replacements);
        }
        let _ = fs::remove_dir_all(staging);
        return Err(error.into());
    }
    let _ = fs::remove_dir_all(&backup);
    Ok(())
}

fn open(archive_path: &Path) -> Result<zip::ZipArchive<BufReader<File>>, PackError> {
    let file = File::open(archive_path)?;
    zip::ZipArchive::new(BufReader::new(file)).map_err(|error| PackError::Archive(error.to_string()))
}

/// Archive index and destination-relative path of every texture in the pack, with the pack's own prefix removed, and
/// the size of those files once extracted.
fn candidates(archive_path: &Path) -> Result<(Vec<(usize, PathBuf)>, u64), PackError> {
    let mut archive = open(archive_path)?;
    let mut paths = Vec::new();
    let mut sizes = Vec::new();
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|error| PackError::Archive(error.to_string()))?;
        if entry.is_dir() {
            continue;
        }
        let Some(path) = entry.enclosed_name() else { continue };
        if !is_texture(&path) || is_junk(&path) {
            continue;
        }
        paths.push((index, path));
        sizes.push(entry.size());
    }
    let common = common_root(&paths);
    let mut entries = Vec::with_capacity(paths.len());
    let mut bytes = 0u64;
    for ((index, path), size) in paths.into_iter().zip(sizes) {
        let relative = strip_prefix(&path, common);
        if let Some(relative) = relative.filter(|path| !path.as_os_str().is_empty()) {
            entries.push((index, relative));
            bytes += size;
        }
    }
    if entries.is_empty() {
        return Err(PackError::Archive("no PNG or DDS files found inside".into()));
    }
    Ok((entries, bytes))
}

/// The pack is PCSX2-shaped: the useful files live below a `replacements` folder whenever the archive has one.
fn strip_prefix(path: &Path, common: Option<usize>) -> Option<PathBuf> {
    let parts: Vec<&std::ffi::OsStr> = path.iter().collect();
    let after = match parts.iter().position(|part| part.to_string_lossy().eq_ignore_ascii_case("replacements")) {
        Some(index) => index + 1,
        None => match common {
            Some(depth) if parts.len() > depth => depth,
            _ => 0,
        },
    };
    if after >= parts.len() {
        return None;
    }
    let relative: PathBuf = parts[after..].iter().collect();
    // Never write outside the destination, whatever the archive says.
    if relative.is_absolute() || relative.components().any(|component| !matches!(component, Component::Normal(_))) {
        return None;
    }
    Some(relative)
}

/// Number of leading folders every entry shares, when they all share at least one (a wrapper folder).
fn common_root(paths: &[(usize, PathBuf)]) -> Option<usize> {
    let first = paths.first()?.1.iter().next()?;
    let shared = paths.iter().all(|(_, path)| {
        let mut parts = path.iter();
        parts.next() == Some(first) && parts.next().is_some()
    });
    shared.then_some(1)
}

fn is_texture(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("png") || extension.eq_ignore_ascii_case("dds"))
}

fn is_junk(path: &Path) -> bool {
    path.iter().any(|part| {
        let part = part.to_string_lossy();
        part.eq_ignore_ascii_case("__MACOSX") || part.eq_ignore_ascii_case(".DS_Store")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// Small ZIP with the given files, written by hand through the `zip` writer.
    fn zip_with(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buffer = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buffer);
            let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            for (name, data) in files {
                writer.start_file(*name, options).unwrap();
                writer.write_all(data).unwrap();
            }
            writer.finish().unwrap();
        }
        buffer.into_inner()
    }

    /// Game folder with an ELF placeholder, so the helpers point at `<root>/textures`.
    fn installation() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(directory.path().join("game")).unwrap();
        fs::write(directory.path().join("game/SCUS_973.99"), b"elf").unwrap();
        fs::create_dir_all(directory.path().join("textures")).unwrap();
        directory
    }

    fn elf(directory: &tempfile::TempDir) -> String {
        directory.path().join("game/SCUS_973.99").display().to_string()
    }

    /// Loopback HTTP server for one request path map, so the download path is exercised without the internet.
    fn serve(responses: Vec<(String, Vec<u8>)>, port: u16) {
        let listener = TcpListener::bind(("127.0.0.1", port)).unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut request = [0u8; 2048];
                let read = std::io::Read::read(&mut stream, &mut request).unwrap_or(0);
                let text = String::from_utf8_lossy(&request[..read]).to_string();
                let path = text.split_whitespace().nth(1).unwrap_or("").to_string();
                let Some((_, body)) = responses.iter().find(|(name, _)| *name == path) else {
                    let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
                    continue;
                };
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/zip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(body);
            }
        });
    }

    #[test]
    fn installs_the_pack_stripping_the_pcsx2_prefix() {
        let directory = installation();
        let pack = zip_with(&[
            ("textures/SCUS-97399/replacements/a-1.png", b"png-a"),
            ("textures/SCUS-97399/replacements/inner/b-2.dds", b"dds-b"),
            ("textures/SCUS-97399/replacements/readme.txt", b"ignore me"),
            ("__MACOSX/textures/SCUS-97399/replacements/._a-1.png", b"junk"),
        ]);
        let meta = br#"{"key":"k","size":1000,"etag":"abc","uploaded":"2026-10-29T02:58:32.356Z"}"#.to_vec();
        serve(vec![("/pack.zip".into(), pack), ("/_meta".into(), meta)], 45871);
        std::thread::sleep(Duration::from_millis(200));
        let source = PackSource {
            pack_url: "http://127.0.0.1:45871/pack.zip".into(),
            meta_url: "http://127.0.0.1:45871/_meta".into(),
        };

        let reports = std::cell::RefCell::new(Vec::new());
        let installed = install(&elf(&directory), &source, &|progress| reports.borrow_mut().push(progress)).unwrap();
        let stages: Vec<PackStage> = reports.borrow().iter().map(|progress| progress.stage).collect();

        let replacements = directory.path().join("textures/replacements");
        assert_eq!(installed.files, 2, "only the PNG and the DDS are installed");
        assert_eq!(fs::read(replacements.join("a-1.png")).unwrap(), b"png-a");
        assert_eq!(fs::read(replacements.join("inner/b-2.dds")).unwrap(), b"dds-b");
        assert!(!replacements.join("readme.txt").exists());
        assert!(!directory.path().join("textures/.gow-textures.zip").exists(), "the archive is deleted");
        assert!(stages.contains(&PackStage::Downloading) && stages.contains(&PackStage::Extracting));
        assert_eq!(installed.etag, "abc");

        // One bar for the whole installation: it never goes back, the download stays within its share, and it ends full.
        let overall: Vec<f64> = reports.borrow().iter().map(|progress| progress.overall).collect();
        assert!(overall.windows(2).all(|pair| pair[0] <= pair[1]), "progress went back: {overall:?}");
        assert!(reports
            .borrow()
            .iter()
            .filter(|progress| progress.stage == PackStage::Downloading)
            .all(|progress| progress.overall <= DOWNLOAD_SHARE));
        assert!((overall.last().copied().unwrap() - 1.0).abs() < 1e-9, "the last report is not 100%: {overall:?}");
        assert!(!directory.path().join("textures").join(STAGING).exists());
        assert!(!directory.path().join("textures").join(BACKUP).exists());

        let marker: PackStatus =
            serde_json::from_str(&fs::read_to_string(directory.path().join("textures/pack.json")).unwrap()).unwrap();
        assert_eq!(marker.files, 2);
        assert_eq!(status(&elf(&directory)), marker, "the marker is what the page reads afterwards");
    }

    #[test]
    fn refuses_entries_that_escape_the_destination() {
        let directory = installation();
        let pack = zip_with(&[("../evil.png", b"nope"), ("replacements/ok.png", b"ok")]);
        let meta = br#"{"size":1,"etag":"e","uploaded":""}"#.to_vec();
        serve(vec![("/pack.zip".into(), pack), ("/_meta".into(), meta)], 45872);
        std::thread::sleep(Duration::from_millis(200));
        let source = PackSource {
            pack_url: "http://127.0.0.1:45872/pack.zip".into(),
            meta_url: "http://127.0.0.1:45872/_meta".into(),
        };

        install(&elf(&directory), &source, &|_| {}).unwrap();

        let replacements = directory.path().join("textures/replacements");
        assert!(replacements.join("ok.png").exists());
        assert!(!directory.path().join("evil.png").exists());
        assert!(!directory.path().join("textures/evil.png").exists());
    }

    #[test]
    fn keeps_a_flat_pack_as_it_is() {
        let directory = installation();
        let pack = zip_with(&[("flat.png", b"x")]);
        let source =
            PackSource { pack_url: "http://127.0.0.1:1/none".into(), meta_url: "http://127.0.0.1:1/none".into() };
        let archive = directory.path().join("flat.zip");
        fs::write(&archive, pack).unwrap();
        let textures = directory.path().join("textures");
        let (files, _) = extract(&archive, &textures, &|_| {}).unwrap();
        assert_eq!(files, 1);
        assert!(textures.join("replacements/flat.png").exists());
        let _ = source;
    }

    #[test]
    fn replaces_the_textures_the_user_had() {
        let directory = installation();
        let textures = directory.path().join("textures");
        fs::create_dir_all(textures.join("replacements/inner")).unwrap();
        fs::write(textures.join("replacements/old.png"), b"old").unwrap();
        fs::write(textures.join("replacements/inner/old.dds"), b"old").unwrap();
        let archive = directory.path().join("pack.zip");
        fs::write(&archive, zip_with(&[("replacements/new.png", b"new")])).unwrap();

        let (files, _) = extract(&archive, &textures, &|_| {}).unwrap();

        assert_eq!(files, 1);
        assert!(textures.join("replacements/new.png").exists());
        assert!(!textures.join("replacements/old.png").exists(), "the user's textures are removed");
        assert!(!textures.join("replacements/inner").exists(), "old sub-folders go away too");
    }

    /// Like `zip_with`, but stored without compression, so a byte of a file's payload can be corrupted.
    fn zip_stored(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buffer = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buffer);
            let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            for (name, data) in files {
                writer.start_file(*name, options).unwrap();
                writer.write_all(data).unwrap();
            }
            writer.finish().unwrap();
        }
        buffer.into_inner()
    }

    #[test]
    fn a_failed_extraction_keeps_the_textures_the_user_had() {
        let directory = installation();
        let textures = directory.path().join("textures");
        fs::create_dir_all(textures.join("replacements")).unwrap();
        fs::write(textures.join("replacements/old.png"), b"old").unwrap();
        let mut pack =
            zip_stored(&[("replacements/a.png", b"first-file"), ("replacements/b.png", b"CORRUPT-ME-PLEASE")]);
        let at = pack.windows(17).position(|window| window == b"CORRUPT-ME-PLEASE").unwrap();
        pack[at + 3] ^= 0xff; // the stored bytes no longer match their checksum
        let archive = directory.path().join("pack.zip");
        fs::write(&archive, pack).unwrap();

        let error = extract(&archive, &textures, &|_| {}).unwrap_err();

        assert!(matches!(error, PackError::Archive(_)), "{error}");
        assert_eq!(fs::read(textures.join("replacements/old.png")).unwrap(), b"old", "the old folder is untouched");
        assert!(!textures.join("replacements/a.png").exists(), "nothing of the new pack leaks into it");
        assert!(!textures.join(STAGING).exists(), "the half-written folder is removed");
    }

    #[test]
    fn an_interrupted_install_is_cleaned_up_and_the_old_folder_comes_back() {
        let directory = installation();
        let textures = directory.path().join("textures");
        // The launcher closed after the old folder moved aside but before the new one took its place.
        fs::create_dir_all(textures.join(BACKUP)).unwrap();
        fs::write(textures.join(BACKUP).join("old.png"), b"old").unwrap();
        fs::create_dir_all(textures.join(STAGING)).unwrap();
        fs::write(textures.join(STAGING).join("half.png"), b"half").unwrap();
        fs::write(textures.join(ARCHIVE), b"half a download").unwrap();

        recover(&textures);

        assert_eq!(fs::read(textures.join("replacements/old.png")).unwrap(), b"old");
        assert!(
            !textures.join(BACKUP).exists() && !textures.join(STAGING).exists() && !textures.join(ARCHIVE).exists()
        );
        // With a folder in place, a stale backup is only discarded.
        fs::create_dir_all(textures.join(BACKUP)).unwrap();
        fs::write(textures.join(BACKUP).join("older.png"), b"older").unwrap();
        recover(&textures);
        assert!(textures.join("replacements/old.png").exists() && !textures.join(BACKUP).exists());
    }

    #[test]
    fn the_extraction_bar_follows_the_bytes_and_reaches_the_end() {
        let directory = installation();
        let textures = directory.path().join("textures");
        let big = vec![7u8; 4000];
        let archive = directory.path().join("pack.zip");
        fs::write(&archive, zip_with(&[("replacements/tiny.png", b"x"), ("replacements/big.png", &big)])).unwrap();
        let reports = std::cell::RefCell::new(Vec::new());

        extract(&archive, &textures, &|progress| reports.borrow_mut().push(progress)).unwrap();

        let reports = reports.borrow();
        assert_eq!(reports.first().unwrap().overall, DOWNLOAD_SHARE, "the extraction starts where the download ended");
        let last = reports.last().unwrap();
        assert_eq!((last.done, last.total), (2, 2));
        assert!((last.overall - 1.0).abs() < 1e-9);
    }

    #[test]
    fn progress_without_a_known_size_stays_at_the_start() {
        assert_eq!(downloading(500, 0).overall, 0.0);
        assert_eq!(downloading(1000, 1000).overall, DOWNLOAD_SHARE);
        assert_eq!(
            downloading(2000, 1000).overall,
            DOWNLOAD_SHARE,
            "a server that sends more than it promised does not overflow"
        );
        assert_eq!(extracting(0, 0, 0, 0).overall, DOWNLOAD_SHARE);
    }

    #[test]
    fn reports_progress_and_status_without_a_pack() {
        let directory = installation();
        assert_eq!(status(&elf(&directory)), PackStatus::default());
    }
}
