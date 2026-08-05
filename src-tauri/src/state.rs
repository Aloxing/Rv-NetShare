//! Application-wide mutable state shared across Tauri commands.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// One shareable resource. Identified by a random opaque token that lives in
/// the URL path (e.g. /s/<id>). Only the `id` is exposed externally; the
/// `path` stays server-side.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareSession {
    pub id: String,
    pub name: String,
    pub path: String,
    pub kind: ShareKind,
    /// bytes for a file, recursive file count for a folder
    pub size: u64,
    /// sum of all file sizes (only meaningful for folder shares)
    pub total_bytes: u64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ShareKind {
    File,
    Folder,
}

pub struct AppState {
    /// Stable application data directory holding config.json and shares.json.
    pub base_dir: Mutex<PathBuf>,
    /// Download-history directory (created if missing); user-configurable.
    pub data_dir: Mutex<PathBuf>,
    /// TCP port that the embedded receiver listens on (mutable so the
    /// runtime can reflect the actually-bound port after fallback).
    pub port: Mutex<u16>,
    /// Active shares, keyed by their random token.
    pub shares: Mutex<HashMap<String, ShareSession>>,
}

impl AppState {
    pub fn new(base_dir: PathBuf, data_dir: PathBuf, port: u16) -> Self {
        Self {
            base_dir: Mutex::new(base_dir),
            data_dir: Mutex::new(data_dir),
            port: Mutex::new(port),
            shares: Mutex::new(HashMap::new()),
        }
    }
}
