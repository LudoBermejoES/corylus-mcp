//! Where the bridge finds a running Corylus: `<app-data>/mcp/endpoint.json`,
//! written by Corylus while its endpoint listens and deleted when it stops.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Corylus's bundle identifier; its app-data directory is named after it.
pub const APP_IDENTIFIER: &str = "com.corylus.app";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointInfo {
    pub port: u16,
    pub token: String,
    pub protocol: u32,
}

/// Corylus's app-data directory on this OS — the same one Tauri resolves:
/// `%APPDATA%\com.corylus.app`, `~/Library/Application Support/com.corylus.app`,
/// `~/.local/share/com.corylus.app`.
pub fn default_app_data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join(APP_IDENTIFIER))
}

pub fn module_dir(app_data: &Path) -> PathBuf {
    app_data.join("mcp")
}

pub fn endpoint_file(app_data: &Path) -> PathBuf {
    module_dir(app_data).join("endpoint.json")
}

pub fn read_endpoint(app_data: &Path) -> io::Result<EndpointInfo> {
    let bytes = std::fs::read(endpoint_file(app_data))?;
    serde_json::from_slice(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Write the file atomically and readable by the current user only.
pub fn write_endpoint(app_data: &Path, info: &EndpointInfo) -> io::Result<()> {
    write_private(&endpoint_file(app_data), &serde_json::to_vec(info)?)
}

/// Atomically write `bytes` to `path`, readable by the current user only from
/// the moment the file exists (no window where another account could open it),
/// in a directory only the user can list on Unix.
pub fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no parent directory"))?;
    create_private_dir(dir)?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    {
        let mut f = open_private(&tmp)?;
        std::io::Write::write_all(&mut f, bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

#[cfg(unix)]
fn open_private(path: &Path) -> io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)
}

// On Windows the file lives under the user's own %APPDATA%, whose ACL already
// grants access to that user (plus SYSTEM and administrators) only.
#[cfg(not(unix))]
fn open_private(path: &Path) -> io::Result<std::fs::File> {
    std::fs::OpenOptions::new().write(true).create_new(true).open(path)
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> io::Result<()> {
    std::fs::create_dir_all(dir)
}

pub fn remove_endpoint(app_data: &Path) {
    let _ = std::fs::remove_file(endpoint_file(app_data));
}

/// 32 random bytes, hex-encoded.
pub fn new_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("OS random source unavailable");
    hex::encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_round_trips_and_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let info = EndpointInfo { port: 4321, token: new_token(), protocol: 1 };
        write_endpoint(dir.path(), &info).unwrap();
        assert_eq!(read_endpoint(dir.path()).unwrap(), info);
        remove_endpoint(dir.path());
        assert!(read_endpoint(dir.path()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn endpoint_file_is_user_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        write_endpoint(dir.path(), &EndpointInfo { port: 1, token: "t".into(), protocol: 1 }).unwrap();
        let mode = std::fs::metadata(endpoint_file(dir.path())).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let dir_mode = std::fs::metadata(module_dir(dir.path())).unwrap().permissions().mode();
        assert_eq!(dir_mode & 0o777, 0o700);
    }

    #[test]
    fn tokens_are_long_and_distinct() {
        let a = new_token();
        assert_eq!(a.len(), 64);
        assert_ne!(a, new_token());
    }
}
