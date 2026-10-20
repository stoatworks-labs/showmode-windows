//! Where Show Mode keeps its journal, settings and log: %LOCALAPPDATA%\ShowMode, which stays on
//! this machine (a show's settings belong to the PC, not to a roaming profile).
//! SHOWMODE_DATA_DIR overrides it for tests.

use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
    let dir = std::env::var_os("SHOWMODE_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("ShowMode")))
        .unwrap_or_else(|| std::env::temp_dir().join("ShowMode"));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn log_file() -> PathBuf {
    data_dir().join("ShowMode.log")
}
