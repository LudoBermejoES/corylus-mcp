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
    let dir = module_dir(app_data);
    std::fs::create_dir_all(&dir)?;
    let target = endpoint_file(app_data);
    let tmp = dir.join(format!(".endpoint.json.{}.tmp", std::process::id()));
    std::fs::write(&tmp, serde_json::to_vec(info)?)?;
    restrict_to_user(&tmp)?;
    std::fs::rename(&tmp, &target)
}

pub fn remove_endpoint(app_data: &Path) {
    let _ = std::fs::remove_file(endpoint_file(app_data));
}

#[cfg(unix)]
fn restrict_to_user(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

// On Windows the file lives under the user's own %APPDATA%, whose ACL already
// grants access to that user (plus SYSTEM and administrators) only.
#[cfg(not(unix))]
fn restrict_to_user(_path: &Path) -> io::Result<()> {
    Ok(())
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
    }

    #[test]
    fn tokens_are_long_and_distinct() {
        let a = new_token();
        assert_eq!(a.len(), 64);
        assert_ne!(a, new_token());
    }
}
