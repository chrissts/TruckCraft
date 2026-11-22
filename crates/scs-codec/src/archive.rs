//! Unified opener for the three container formats that use the `.scs`
//! extension: `HashFS` v2, `HashFS` v1 and plain ZIP.
//!
//! [`Archive::open`] sniffs the header and returns a reader with one interface
//! over all three, so callers do not care which they got.

use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::hashfs1::HashFsV1;
use crate::hashfs2::{self, COMPRESSION_NONE, COMPRESSION_ZLIB, HashFsV2, Tree};
use crate::zip::{self, METHOD_DEFLATE, ZipArchive};

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    HashFs(hashfs2::Error),
    Zip(zip::Error),
    Unrecognised,
    UnsupportedVersion(u16),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::HashFs(e) => write!(f, "{e}"),
            Self::Zip(e) => write!(f, "{e}"),
            Self::Unrecognised => {
                write!(f, "unrecognised .scs container (not HashFS or ZIP)")
            }
            Self::UnsupportedVersion(v) => write!(f, "unsupported HashFS version {v}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<hashfs2::Error> for Error {
    fn from(e: hashfs2::Error) -> Self {
        Self::HashFs(e)
    }
}

impl From<zip::Error> for Error {
    fn from(e: zip::Error) -> Self {
        Self::Zip(e)
    }
}

/// How to address one entry, whichever container it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Key {
    /// `HashFS`: the `CityHash64` of the path.
    Hash(u64),
    /// ZIP: the member's position in the central directory.
    Index(usize),
}

/// A container-independent view of one entry.
#[derive(Debug, Clone)]
pub struct EntryInfo {
    pub key: Key,
    pub name: Option<String>,
    pub is_dir: bool,
    pub is_tobj: bool,
    pub size: u32,
    pub compression: u8,
}

pub enum Archive {
    V2(HashFsV2),
    V1(HashFsV1),
    Zip(ZipArchive),
}

impl Archive {
    /// Sniff the header and open the right reader.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref();
        let mut magic = [0u8; 8];
        let read = File::open(path)?.read(&mut magic)?;
        if read >= 6 && &magic[..4] == b"SCS#" {
            let version = u16::from_le_bytes(magic[4..6].try_into().expect("2 bytes"));
            return match version {
                2 => Ok(Self::V2(HashFsV2::open(path)?)),
                1 => Ok(Self::V1(HashFsV1::open(path)?)),
                other => Err(Error::UnsupportedVersion(other)),
            };
        }
        if read >= 2 && &magic[..2] == b"PK" {
            return Ok(Self::Zip(ZipArchive::open(path)?));
        }
        Err(Error::Unrecognised)
    }

    pub const fn format_name(&self) -> &'static str {
        match self {
            Self::V2(_) => "HashFS v2",
            Self::V1(_) => "HashFS v1",
            Self::Zip(_) => "ZIP",
        }
    }

    /// Walk the directory tree, resolving names. ZIP already carries them.
    pub fn build_tree(&mut self) -> Tree {
        match self {
            Self::V2(a) => a.build_tree(),
            Self::V1(a) => a.build_tree(),
            Self::Zip(a) => {
                let mut tree = Tree::default();
                for m in &a.members {
                    if m.is_dir {
                        tree.dirs.push(m.name.clone());
                    } else {
                        tree.files.push(m.name.clone());
                    }
                }
                tree
            }
        }
    }

    /// Every entry, in a stable order: by hash for `HashFS`, by position for ZIP.
    pub fn entries(&self) -> Vec<EntryInfo> {
        match self {
            Self::V2(a) => {
                let mut keys: Vec<u64> = a.entries.keys().copied().collect();
                keys.sort_unstable();
                keys.into_iter()
                    .map(|h| {
                        let e = &a.entries[&h];
                        EntryInfo {
                            key: Key::Hash(h),
                            name: e.name.clone(),
                            is_dir: e.is_dir,
                            is_tobj: e.is_tobj,
                            size: e.size,
                            compression: e.compression,
                        }
                    })
                    .collect()
            }
            Self::V1(a) => {
                let mut keys: Vec<u64> = a.entries.keys().copied().collect();
                keys.sort_unstable();
                keys.into_iter()
                    .map(|h| {
                        let e = &a.entries[&h];
                        EntryInfo {
                            key: Key::Hash(h),
                            name: e.name.clone(),
                            is_dir: e.is_dir,
                            is_tobj: false,
                            size: e.size,
                            compression: e.compression(),
                        }
                    })
                    .collect()
            }
            Self::Zip(a) => a
                .members
                .iter()
                .enumerate()
                .map(|(i, m)| EntryInfo {
                    key: Key::Index(i),
                    name: Some(m.name.clone()),
                    is_dir: m.is_dir,
                    is_tobj: false,
                    size: m.size,
                    compression: if m.method == METHOD_DEFLATE {
                        COMPRESSION_ZLIB
                    } else {
                        COMPRESSION_NONE
                    },
                })
                .collect(),
        }
    }

    pub fn read(&self, key: Key) -> Result<Vec<u8>, Error> {
        Ok(match (self, key) {
            (Self::V2(a), Key::Hash(h)) => a.read_entry(h)?,
            (Self::V1(a), Key::Hash(h)) => a.read_entry(h)?,
            (Self::Zip(a), Key::Index(i)) => a.read_index(i)?,
            _ => return Err(Error::Unrecognised),
        })
    }

    pub fn read_path(&self, path: &str) -> Result<Vec<u8>, Error> {
        self.read_path_limited(path, crate::MAX_ARCHIVE_ENTRY)
    }

    /// Apply a caller's smaller allocation/inflation bound to every format.
    pub fn read_path_limited(&self, path: &str, limit: usize) -> Result<Vec<u8>, Error> {
        let hash = crate::cityhash::cityhash64(path.as_bytes());
        Ok(match self {
            Self::V2(a) => a.read_entry_limited(hash, limit)?,
            Self::V1(a) => a.read_entry_limited(hash, limit)?,
            Self::Zip(a) => a.read_path_limited(path, limit)?,
        })
    }

    /// Inspect one directory without walking unrelated multi-GB archives.
    pub fn list_directory(&self, path: &str) -> Result<(Vec<String>, Vec<String>), Error> {
        let hash = crate::cityhash::cityhash64(path.as_bytes());
        match self {
            Self::V2(archive) => {
                if !archive.entries.get(&hash).is_some_and(|entry| entry.is_dir) {
                    return Err(hashfs2::Error::NotFound(path.into()).into());
                }
                Ok(HashFsV2::parse_dir_listing(
                    &archive.read_entry_limited(hash, 512 * 1024)?,
                )?)
            }
            Self::V1(archive) => {
                if !archive.entries.get(&hash).is_some_and(|entry| entry.is_dir) {
                    return Err(hashfs2::Error::NotFound(path.into()).into());
                }
                Ok(HashFsV1::parse_dir_listing(
                    &archive.read_entry_limited(hash, 512 * 1024)?,
                ))
            }
            Self::Zip(archive) => {
                let prefix = format!("{}/", path.trim_end_matches('/'));
                let mut dirs = std::collections::BTreeSet::new();
                let mut files = std::collections::BTreeSet::new();
                for entry in &archive.members {
                    if let Some(tail) = entry.name.strip_prefix(&prefix) {
                        if let Some((child, _)) = tail.split_once('/') {
                            if !child.is_empty() {
                                dirs.insert(child.to_owned());
                            }
                        } else if !tail.is_empty() {
                            files.insert(tail.to_owned());
                        }
                        if dirs.len() + files.len() > 4096 {
                            return Err(hashfs2::Error::Limit("directory listing").into());
                        }
                    }
                }
                Ok((dirs.into_iter().collect(), files.into_iter().collect()))
            }
        }
    }

    /// The tobj image part header for an entry, when the container has one.
    pub fn image_header(&self, key: Key) -> Option<Vec<u8>> {
        match (self, key) {
            (Self::V2(a), Key::Hash(h)) => a.entries.get(&h)?.image_header().map(<[u8]>::to_vec),
            _ => None,
        }
    }

    /// Label / value pairs for an `info` listing.
    pub fn describe(&mut self) -> Vec<(String, String)> {
        let format = self.format_name().to_string();
        let tree = self.build_tree();
        let entries = self.entries();
        let dirs = entries.iter().filter(|e| e.is_dir).count();
        let tobj = entries.iter().filter(|e| e.is_tobj).count();
        let mut comp = std::collections::BTreeMap::new();
        for e in &entries {
            *comp.entry(e.compression).or_insert(0usize) += 1;
        }
        let names = |c: u8| match c {
            COMPRESSION_NONE => "store".to_string(),
            COMPRESSION_ZLIB => "zlib".to_string(),
            hashfs2::COMPRESSION_GDEFLATE => "gdeflate".to_string(),
            other => format!("{other:#x}"),
        };
        vec![
            ("format".into(), format),
            ("entries".into(), format!("{}", entries.len())),
            ("directories".into(), format!("{dirs}")),
            ("files".into(), format!("{}", entries.len() - dirs)),
            ("tobj textures".into(), format!("{tobj}")),
            (
                "resolved names".into(),
                format!(
                    "{}/{} (orphans: {})",
                    entries.len() - tree.unresolved,
                    entries.len(),
                    tree.unresolved
                ),
            ),
            (
                "compression".into(),
                comp.iter()
                    .map(|(k, v)| format!("{}={v}", names(*k)))
                    .collect::<Vec<_>>()
                    .join("  "),
            ),
        ]
    }
}
