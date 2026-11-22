//! SCS `HashFS` version 1 (`.scs`) reader.
//!
//! The pre-1.50 archive format, still accepted by the games for mods. Magic
//! `SCS#`, version 1, hash method `CITY` (the same `CityHash64` variant as v2).
//!
//! ```text
//! Header (20 bytes, little-endian):
//!   0x00 char[4] magic        "SCS#"
//!   0x04 u16     version       1
//!   0x06 u16     salt          0
//!   0x08 char[4] hash_method  "CITY"
//!   0x0C u32     entry_count
//!   0x10 u32     entry_offset  file offset of the entry table
//! ```
//!
//! The entry table is `entry_count * 32` bytes of `u64 hash`, `u64 offset`,
//! `u32 flags` (bit0 directory, bit1 compressed), `u32 crc`, `u32 size`,
//! `u32 zsize`. Payloads are raw or a zlib stream.
//!
//! Directory listings are UTF-8 text, one item per line, a leading `*` marking
//! a subdirectory — unlike v2's binary listing.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::cityhash::cityhash64;
use crate::hashfs2::{COMPRESSION_NONE, COMPRESSION_ZLIB, DEFAULT_ROOTS, Error, KNOWN_FILES, Tree};

pub const MAGIC: &[u8; 4] = b"SCS#";
pub const HEADER_LEN: usize = 0x14;
pub const FLAG_DIRECTORY: u32 = 0x1;
pub const FLAG_COMPRESSED: u32 = 0x2;

#[derive(Debug, Clone)]
pub struct Entry {
    pub hash: u64,
    pub offset: u64,
    pub flags: u32,
    pub crc: u32,
    pub size: u32,
    pub zsize: u32,
    pub is_dir: bool,
    pub is_compressed: bool,
    pub name: Option<String>,
}

impl Entry {
    pub const fn compression(&self) -> u8 {
        if self.is_compressed {
            COMPRESSION_ZLIB
        } else {
            COMPRESSION_NONE
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Header {
    pub version: u16,
    pub salt: u16,
    pub entry_count: u32,
    pub entry_offset: u32,
}

pub struct HashFsV1 {
    pub path: PathBuf,
    file: File,
    pub header: Header,
    pub entries: HashMap<u64, Entry>,
    pub root_hash: Option<u64>,
}

impl HashFsV1 {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref().to_path_buf();
        let mut file = File::open(&path)?;
        let mut hdr = [0u8; HEADER_LEN];
        file.read_exact(&mut hdr).map_err(|_| Error::TooSmall)?;
        if &hdr[..4] != MAGIC {
            let mut m = [0u8; 4];
            m.copy_from_slice(&hdr[..4]);
            return Err(Error::BadMagic(m));
        }
        let version = u16::from_le_bytes(hdr[4..6].try_into().expect("2 bytes"));
        if version != 1 {
            return Err(Error::UnsupportedVersion(version));
        }
        let salt = u16::from_le_bytes(hdr[6..8].try_into().expect("2 bytes"));
        let hash_method: [u8; 4] = hdr[8..12].try_into().expect("4 bytes");
        if &hash_method != b"CITY" {
            return Err(Error::UnsupportedHashMethod(hash_method));
        }
        let entry_count = u32::from_le_bytes(hdr[12..16].try_into().expect("4 bytes"));
        let entry_offset = u32::from_le_bytes(hdr[16..20].try_into().expect("4 bytes"));
        if entry_count as usize > crate::MAX_ARCHIVE_ENTRIES
            || u64::from(entry_offset) + u64::from(entry_count) * 32 > file.metadata()?.len()
        {
            return Err(Error::Limit("v1 index size or offset"));
        }

        file.seek(SeekFrom::Start(u64::from(entry_offset)))?;
        let mut table = vec![0u8; entry_count as usize * 32];
        file.read_exact(&mut table).map_err(|_| Error::ShortRead {
            at: u64::from(entry_offset),
            got: 0,
            want: table.len(),
        })?;

        let mut entries = HashMap::with_capacity(entry_count as usize);
        for i in 0..entry_count as usize {
            let at = i * 32;
            let u64_at = |o: usize| {
                u64::from_le_bytes(table[at + o..at + o + 8].try_into().expect("8 bytes"))
            };
            let u32_at = |o: usize| {
                u32::from_le_bytes(table[at + o..at + o + 4].try_into().expect("4 bytes"))
            };
            let hash = u64_at(0);
            let flags = u32_at(16);
            entries.insert(
                hash,
                Entry {
                    hash,
                    offset: u64_at(8),
                    flags,
                    crc: u32_at(20),
                    size: u32_at(24),
                    zsize: u32_at(28),
                    is_dir: flags & FLAG_DIRECTORY != 0,
                    is_compressed: flags & FLAG_COMPRESSED != 0,
                    name: None,
                },
            );
        }

        let mut fs = Self {
            path,
            file,
            header: Header {
                version,
                salt,
                entry_count,
                entry_offset,
            },
            entries,
            root_hash: None,
        };
        let root = cityhash64(b"");
        if fs.entries.get(&root).is_some_and(|e| e.is_dir) {
            fs.root_hash = Some(root);
            if let Some(e) = fs.entries.get_mut(&root) {
                e.name = Some(String::new());
            }
        }
        Ok(fs)
    }

    pub fn read_entry(&self, hash: u64) -> Result<Vec<u8>, Error> {
        self.read_entry_limited(hash, crate::MAX_ARCHIVE_ENTRY)
    }

    pub fn read_entry_limited(&self, hash: u64, limit: usize) -> Result<Vec<u8>, Error> {
        let limit = limit.min(crate::MAX_ARCHIVE_ENTRY);
        let (offset, zsize, size, compressed) = match self.entries.get(&hash) {
            Some(e) => (e.offset, e.zsize, e.size, e.is_compressed),
            None => return Err(Error::NotFound(format!("{hash:#018x}"))),
        };
        if zsize as usize > limit
            || size as usize > limit
            || offset
                .checked_add(u64::from(zsize))
                .is_none_or(|end| end > self.file.metadata().map(|m| m.len()).unwrap_or(0))
        {
            return Err(Error::Limit("v1 entry size or offset"));
        }
        let mut raw = vec![0u8; zsize as usize];
        let got = crate::hashfs2::read_at(&self.file, &mut raw, offset)?;
        if got != raw.len() {
            return Err(Error::ShortRead {
                at: offset,
                got,
                want: zsize as usize,
            });
        }
        if !compressed {
            return Ok(raw);
        }
        use flate2::read::ZlibDecoder;
        let mut out = Vec::with_capacity(size as usize);
        ZlibDecoder::new(raw.as_slice())
            .take(limit as u64 + 1)
            .read_to_end(&mut out)
            .map_err(|e| Error::Inflate(e.to_string()))?;
        if out.len() > limit {
            return Err(Error::Limit("v1 decompressed entry"));
        }
        if size != 0 && out.len() != size as usize {
            return Err(Error::SizeMismatch {
                hash,
                got: out.len(),
                want: size,
            });
        }
        Ok(out)
    }

    pub fn read_path(&self, path: &str) -> Result<Vec<u8>, Error> {
        let hash = cityhash64(path.as_bytes());
        if !self.entries.contains_key(&hash) {
            return Err(Error::NotFound(path.to_string()));
        }
        self.read_entry(hash)
    }

    /// Text listing: one name per line, `*` prefix marking a subdirectory.
    pub fn parse_dir_listing(data: &[u8]) -> (Vec<String>, Vec<String>) {
        let text = String::from_utf8_lossy(data);
        let (mut subdirs, mut files) = (Vec::new(), Vec::new());
        for line in text.split('\n') {
            if line.is_empty() {
                continue;
            }
            match line.strip_prefix('*') {
                Some(rest) => subdirs.push(rest.to_string()),
                None => files.push(line.to_string()),
            }
        }
        (subdirs, files)
    }

    pub fn build_tree(&mut self) -> Tree {
        let mut roots: Vec<String> = DEFAULT_ROOTS
            .iter()
            .map(std::string::ToString::to_string)
            .collect();
        if let Some(stem) = self.path.file_stem().and_then(|s| s.to_str()) {
            for cand in [
                stem.to_string(),
                stem.rsplit('_').next().unwrap_or(stem).to_string(),
            ] {
                if !cand.is_empty() && !roots.contains(&cand) {
                    roots.push(cand);
                }
            }
        }

        let mut tree = Tree::default();
        let mut seen: HashSet<u64> = HashSet::new();
        let mut queue: Vec<String> = Vec::new();
        for r in &roots {
            self.enqueue(&mut seen, &mut queue, r);
        }

        let mut i = 0;
        while i < queue.len() {
            let dpath = queue[i].clone();
            i += 1;
            tree.dirs.push(dpath.clone());
            let Ok(data) = self.read_entry(cityhash64(dpath.as_bytes())) else {
                continue;
            };
            let (subdirs, dfiles) = Self::parse_dir_listing(&data);
            let prefix = if dpath.is_empty() {
                String::new()
            } else {
                format!("{dpath}/")
            };
            for sd in subdirs {
                let child = format!("{prefix}{sd}");
                self.enqueue(&mut seen, &mut queue, &child);
            }
            for f in dfiles {
                let fpath = format!("{prefix}{f}");
                if let Some(fe) = self.entries.get_mut(&cityhash64(fpath.as_bytes())) {
                    fe.name = Some(fpath.clone());
                }
                tree.files.push(fpath);
            }
        }

        for kf in KNOWN_FILES {
            if let Some(fe) = self.entries.get_mut(&cityhash64(kf.as_bytes()))
                && fe.name.is_none()
            {
                fe.name = Some((*kf).to_string());
                tree.files.push((*kf).to_string());
            }
        }

        tree.unresolved = self.entries.values().filter(|e| e.name.is_none()).count();
        tree
    }

    fn enqueue(&mut self, seen: &mut HashSet<u64>, queue: &mut Vec<String>, path: &str) {
        let hash = cityhash64(path.as_bytes());
        if seen.contains(&hash) {
            return;
        }
        if let Some(e) = self.entries.get_mut(&hash)
            && e.is_dir
        {
            seen.insert(hash);
            e.name = Some(path.to_string());
            queue.push(path.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_text_listing() {
        let (subdirs, files) = HashFsV1::parse_dir_listing(b"*def\n*material\nversion.txt\n");
        assert_eq!(subdirs, vec!["def", "material"]);
        assert_eq!(files, vec!["version.txt"]);
    }

    #[test]
    fn ignores_blank_lines() {
        let (subdirs, files) = HashFsV1::parse_dir_listing(b"\n\na.sii\n\n");
        assert!(subdirs.is_empty());
        assert_eq!(files, vec!["a.sii"]);
    }

    #[test]
    fn caller_limit_bounds_inflation_even_when_declared_size_is_unknown() {
        use std::io::Write;
        let body = vec![0u8; 64 * 1024];
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
        encoder.write_all(&body).unwrap();
        let compressed = encoder.finish().unwrap();
        assert!(compressed.len() < 128);
        let hash = cityhash64(b"def/sample.sii");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"SCS#\x01\x00\x00\x00CITY");
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&20u32.to_le_bytes());
        bytes.extend_from_slice(&hash.to_le_bytes());
        bytes.extend_from_slice(&52u64.to_le_bytes());
        bytes.extend_from_slice(&FLAG_COMPRESSED.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&compressed);
        let path = std::env::temp_dir().join(format!(
            "truckcraft-hashfs-limit-{}.scs",
            std::process::id()
        ));
        std::fs::write(&path, bytes).unwrap();
        let archive = crate::Archive::open(&path).unwrap();
        assert_eq!(archive.read_path("def/sample.sii").unwrap(), body);
        assert!(archive.read_path_limited("def/sample.sii", 128).is_err());
        drop(archive);
        std::fs::remove_file(path).unwrap();
    }
}
