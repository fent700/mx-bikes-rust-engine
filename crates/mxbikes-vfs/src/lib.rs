use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::{debug, info};

#[derive(Error, Debug)]
pub enum VfsError {
    #[error("Game installation not found at '{0}'")]
    GameNotFound(PathBuf),
    #[error("Asset not found: '{0}'")]
    FileNotFound(String),
    #[error("I/O error reading asset '{path}': {source}")]
    IoError {
        path: String,
        source: std::io::Error,
    },
    #[error("ZIP error in PKZ '{pkz}': {source}")]
    ZipError {
        pkz: String,
        source: zip::result::ZipError,
    },
}

#[derive(Debug, Clone)]
enum AssetSource {
    Loose(PathBuf),
    Archive {
        pkz_path: PathBuf,
        archive_name: String,
    },
}

/// Virtual File System for MX Bikes assets.
/// Seamlessly resolves files from loose files and `.pkz` ZIP archives.
pub struct MxBikesVfs {
    game_dir: PathBuf,
    /// Maps normalized relative paths (e.g. "bikes/250xf/250xf.cfg") to source
    index: HashMap<String, AssetSource>,
}

impl MxBikesVfs {
    /// Auto-detects the game directory from standard Steam install paths.
    pub fn auto_detect() -> Result<Self, VfsError> {
        let candidate_paths = [
            PathBuf::from(r"N:\SteamLibrary\steamapps\common\MX Bikes"),
            PathBuf::from(r"C:\Program Files (x86)\Steam\steamapps\common\MX Bikes"),
            PathBuf::from(r"D:\SteamLibrary\steamapps\common\MX Bikes"),
            PathBuf::from(r"E:\SteamLibrary\steamapps\common\MX Bikes"),
        ];

        for path in &candidate_paths {
            if path.exists() && path.join("mxbikes.exe").exists() {
                info!("Found MX Bikes installation at: {:?}", path);
                return Self::open(path);
            }
        }

        Err(VfsError::GameNotFound(PathBuf::from(
            "Steam library (checked default locations)",
        )))
    }

    /// Opens an MX Bikes installation at the specified path and indexes all PKZ archives.
    pub fn open<P: AsRef<Path>>(game_dir: P) -> Result<Self, VfsError> {
        let game_dir = game_dir.as_ref().to_path_buf();
        if !game_dir.exists() {
            return Err(VfsError::GameNotFound(game_dir));
        }

        let mut vfs = Self {
            game_dir: game_dir.clone(),
            index: HashMap::new(),
        };

        vfs.index_all()?;
        Ok(vfs)
    }

    /// Indexes all `.pkz` files and loose files.
    fn index_all(&mut self) -> Result<(), VfsError> {
        // Known PKZ packages in load order
        let pkz_names = [
            "ui.pkz",
            "series.pkz",
            "effects.pkz",
            "misc.pkz",
            "tyres.pkz",
            "rider.pkz",
            "bikes.pkz",
            "tracks.pkz",
        ];

        for pkz_name in &pkz_names {
            let pkz_path = self.game_dir.join(pkz_name);
            if pkz_path.exists() {
                self.index_pkz(&pkz_path, pkz_name)?;
            } else {
                debug!("PKZ package not found (optional): {:?}", pkz_path);
            }
        }

        // Also index any custom mods / loose files that override PKZ
        self.index_loose_files();

        info!("Indexed {} unique asset entries in VFS", self.index.len());
        Ok(())
    }

    fn index_pkz(&mut self, pkz_path: &Path, pkz_name: &str) -> Result<(), VfsError> {
        let file = File::open(pkz_path).map_err(|source| VfsError::IoError {
            path: pkz_path.display().to_string(),
            source,
        })?;

        let mut archive = zip::ZipArchive::new(file).map_err(|source| VfsError::ZipError {
            pkz: pkz_name.to_string(),
            source,
        })?;

        for i in 0..archive.len() {
            let file = archive.by_index(i).map_err(|source| VfsError::ZipError {
                pkz: pkz_name.to_string(),
                source,
            })?;

            if !file.is_dir() {
                let normalized = Self::normalize_path(file.name());
                self.index.insert(
                    normalized,
                    AssetSource::Archive {
                        pkz_path: pkz_path.to_path_buf(),
                        archive_name: file.name().to_string(),
                    },
                );
            }
        }

        Ok(())
    }

    fn index_loose_files(&mut self) {
        let dirs_to_check = ["mods", "tracks", "bikes", "rider", "tyres", "ui"];
        for dir in &dirs_to_check {
            let target_dir = self.game_dir.join(dir);
            if !target_dir.exists() {
                continue;
            }

            for entry in walkdir::WalkDir::new(&target_dir).into_iter().filter_map(|e| e.ok()) {
                if entry.file_type().is_file() {
                    if let Ok(rel) = entry.path().strip_prefix(&self.game_dir) {
                        let rel_str = rel.to_string_lossy().to_string();
                        let normalized = Self::normalize_path(&rel_str);
                        // Loose files take precedence over PKZ
                        self.index.insert(normalized, AssetSource::Loose(entry.path().to_path_buf()));
                    }
                }
            }
        }
    }

    /// Normalizes a path to lowercase with forward slashes.
    pub fn normalize_path(path: &str) -> String {
        path.trim_start_matches('/')
            .trim_start_matches('\\')
            .replace('\\', "/")
            .to_lowercase()
    }

    /// Checks if an asset exists in the VFS.
    pub fn exists(&self, path: &str) -> bool {
        let normalized = Self::normalize_path(path);
        self.index.contains_key(&normalized)
    }

    /// Reads an asset as the engine's file loader (`FUN_140157920`) returns it: files ending in
    /// a `PYRC` footer (sounds, rider parameters) are decrypted; others come back unchanged.
    pub fn read(&self, path: &str) -> Result<Vec<u8>, VfsError> {
        let mut data = self.read_raw(path)?;
        if let Some(plain) = pyrc_decrypt(&data) {
            data = plain;
        }
        Ok(data)
    }

    /// Reads raw asset bytes.
    pub fn read_raw(&self, path: &str) -> Result<Vec<u8>, VfsError> {
        let normalized = Self::normalize_path(path);
        let source = self
            .index
            .get(&normalized)
            .ok_or_else(|| VfsError::FileNotFound(path.to_string()))?;

        match source {
            AssetSource::Loose(loose_path) => {
                let mut buf = Vec::new();
                let mut f = File::open(loose_path).map_err(|source| VfsError::IoError {
                    path: loose_path.display().to_string(),
                    source,
                })?;
                f.read_to_end(&mut buf).map_err(|source| VfsError::IoError {
                    path: loose_path.display().to_string(),
                    source,
                })?;
                Ok(buf)
            }
            AssetSource::Archive {
                pkz_path,
                archive_name,
            } => {
                let f = File::open(pkz_path).map_err(|source| VfsError::IoError {
                    path: pkz_path.display().to_string(),
                    source,
                })?;
                let mut archive =
                    zip::ZipArchive::new(f).map_err(|source| VfsError::ZipError {
                        pkz: pkz_path.display().to_string(),
                        source,
                    })?;
                let mut entry = archive
                    .by_name(archive_name)
                    .map_err(|source| VfsError::ZipError {
                        pkz: pkz_path.display().to_string(),
                        source,
                    })?;

                let mut buf = Vec::with_capacity(entry.size() as usize);
                entry.read_to_end(&mut buf).map_err(|source| VfsError::IoError {
                    path: path.to_string(),
                    source,
                })?;
                Ok(buf)
            }
        }
    }

    /// Reads asset as UTF-8 string.
    pub fn read_to_string(&self, path: &str) -> Result<String, VfsError> {
        let bytes = self.read(path)?;
        String::from_utf8(bytes).map_err(|_| VfsError::IoError {
            path: path.to_string(),
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid UTF-8 in asset"),
        })
    }

    /// Returns all registered asset paths that start with the given prefix.
    pub fn list_files(&self, prefix: &str) -> Vec<String> {
        let norm_prefix = Self::normalize_path(prefix);
        self.index
            .keys()
            .filter(|k| k.starts_with(&norm_prefix))
            .cloned()
            .collect()
    }

    /// Returns the game installation directory.
    pub fn game_dir(&self) -> &Path {
        &self.game_dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_normalization() {
        assert_eq!(
            MxBikesVfs::normalize_path("Bikes\\250xf\\250xf.CFG"),
            "bikes/250xf/250xf.cfg"
        );
        assert_eq!(
            MxBikesVfs::normalize_path("/tracks/enduro/enduro.ini"),
            "tracks/enduro/enduro.ini"
        );
    }

    #[test]
    fn test_pyrc_decrypt_real_wav() {
        if let Ok(vfs) = MxBikesVfs::auto_detect() {
            let data = vfs.read("bikes/250xf/250cc_onverylow.wav").expect("read wav");
            assert!(data.len() >= 12);
            assert_eq!(&data[..4], b"RIFF");
            assert_eq!(&data[8..12], b"WAVE");
        }
    }
}

/// Decrypts a `PYRC` container: payload followed by a 12-byte footer (magic `PYRC`, seed,
/// payload length). The payload is ARC4 with the engine's 16-byte key, dropping the first
/// `seed + 256` keystream bytes (`FUN_14015af80` / `FUN_14015b070`). `None` when the data has
/// no such footer.
pub fn pyrc_decrypt(data: &[u8]) -> Option<Vec<u8>> {
    const KEY: &[u8; 16] = b"XL$S$udrla#!oaSW";
    let footer = data.len().checked_sub(12)?;
    if &data[footer..footer + 4] != b"PYRC" {
        return None;
    }
    let seed = u32::from_le_bytes(data[footer + 4..footer + 8].try_into().ok()?) as usize;
    let len = u32::from_le_bytes(data[footer + 8..footer + 12].try_into().ok()?) as usize;
    if len != footer {
        return None;
    }
    let mut s: [u8; 256] = std::array::from_fn(|i| i as u8);
    let mut j: u8 = 0;
    for i in 0..256 {
        j = j.wrapping_add(s[i]).wrapping_add(KEY[i % KEY.len()]);
        s.swap(i, j as usize);
    }
    let (mut i, mut j) = (0u8, 0u8);
    let mut next = || {
        i = i.wrapping_add(1);
        j = j.wrapping_add(s[i as usize]);
        s.swap(i as usize, j as usize);
        s[s[i as usize].wrapping_add(s[j as usize]) as usize]
    };
    for _ in 0..seed + 256 {
        next();
    }
    Some(data[..len].iter().map(|b| b ^ next()).collect())
}
