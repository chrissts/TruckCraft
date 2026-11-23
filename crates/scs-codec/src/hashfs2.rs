//! SCS `HashFS` version 2 (`.scs`) reader.
//!
//! Used by Euro Truck Simulator 2 / American Truck Simulator 1.50+. Magic
//! `SCS#`, version 2, hash method `CITY` (`CityHash64`, SCS variant).
//!
//! ```text
//! Header (0x34 bytes, little-endian):
//!   0x00  char[4]  magic        "SCS#"
//!   0x04  u16      version       2
//!   0x06  u16      salt          0
//!   0x08  char[4]  hash_method   "CITY"
//!   0x0C  u32      entry_count           entries in index 1
//!   0x10  u32      entry_table_size      compressed length of index 1
//!   0x14  u32      meta_word_count       u32 words in index 2
//!   0x18  u32      meta_table_size       compressed length of index 2
//!   0x1C  u64      entry_table_offset
//!   0x24  u64      meta_table_offset
//!   0x2C  u64      security_offset       cert region, unused here
//! ```
//!
//! Index 1 is zlib-compressed and inflates to `entry_count * 0x10` bytes, each
//! entry being `u64 hash`, `u32 meta_index`, `u16 part_count`, `u16 flags1`
//! (bit0 = directory, bit2 = preload).
//!
//! Index 2 inflates to `meta_word_count * 4` bytes of u32 words. Starting at
//! word `meta_index`, an entry has `part_count` four-byte part descriptors:
//! `u16 off_low`, `u8 off_high`, `u8 kind`, where the part header sits at
//! `(off_low | off_high << 16) * 4`. `kind & 0x80` marks the 16-byte data part
//! that describes the payload; kind 1 is a tobj/image part, kind 2 a sampler.
//!
//! Directory listings are `u32 count`, `u8 name_len[count]`, then the names;
//! a name starting with `/` is a subdirectory.
//!
//! References: `Archive::SCS::HashFS2` (nautofon), SCS Modding Wiki.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use std::path::{Path, PathBuf};

use crate::cityhash::cityhash64;
use crate::gdeflate;

pub const MAGIC: &[u8; 4] = b"SCS#";
pub const HEADER_LEN: usize = 0x34;

pub const COMPRESSION_NONE: u8 = 0x00;
pub const COMPRESSION_ZLIB: u8 = 0x10;
pub const COMPRESSION_GDEFLATE: u8 = 0x30;

/// Known top-level names in ETS2 / ATS archives.
///
/// `HashFS` stores only hashes plus per-directory listings, so recovering names
/// needs at least one known node per connected subtree. The root lists most of
/// them, but some archives are subtrees with no root (`locale.scs`) and some
/// keep unlinked subtrees under a present-but-empty root (`core.scs`).
pub const DEFAULT_ROOTS: &[&str] = &[
    "",
    "asset",
    "automat",
    "contentbrowser",
    "custom",
    "def",
    "dlc",
    "effect",
    "font",
    "locale",
    "map",
    "material",
    "matrix",
    "model",
    "model2",
    "offline_radio",
    "prefab",
    "prefab2",
    "preview_config",
    "replay",
    "road_template",
    "sound",
    "system",
    "truck",
    "ui",
    "umatlib",
    "unit",
    "vehicle",
    "video",
    "world",
    "sky",
    "autoexec.cfg",
    "autoexec_ds.cfg",
    "manifest.sii",
    "version.txt",
    "mod_workshop_package.sii",
    "mod_description.txt",
    "mod_icon.jpg",
    "locale.sii",
];

/// Files the engine loads by a hard-coded path that appears in no directory
/// listing, so the tree walk cannot discover them.
pub const KNOWN_FILES: &[&str] = &[
    "def/online_data.sii",
    "def/mod_manager_data.sii",
    "def/online_economy_data.sii",
];

#[derive(Debug)]
pub enum Error {
    Limit(&'static str),
    Io(std::io::Error),
    TooSmall,
    BadMagic([u8; 4]),
    UnsupportedVersion(u16),
    UnsupportedHashMethod([u8; 4]),
    UnsupportedSalt(u16),
    ShortRead { at: u64, got: usize, want: usize },
    IndexSize { at: u64, got: usize, want: usize },
    Inflate(String),
    UnknownCompression { compression: u8, hash: u64 },
    GDeflate { hash: u64, source: gdeflate::Error },
    SizeMismatch { hash: u64, got: usize, want: u32 },
    NotFound(String),
    MalformedListing,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Limit(reason) => write!(f, "archive limit exceeded: {reason}"),
            Self::Io(e) => write!(f, "{e}"),
            Self::TooSmall => write!(f, "file too small to be a HashFS archive"),
            Self::BadMagic(m) => write!(f, "not an SCS archive (magic {m:?})"),
            Self::UnsupportedVersion(v) => {
                write!(f, "unsupported HashFS version {v} (this reader handles v2)")
            }
            Self::UnsupportedHashMethod(m) => write!(f, "unsupported hash method {m:?}"),
            Self::UnsupportedSalt(s) => write!(f, "unsupported salt {s}"),
            Self::ShortRead { at, got, want } => {
                write!(f, "short read at {at:#x}: got {got} of {want}")
            }
            Self::IndexSize { at, got, want } => {
                write!(f, "index at {at:#x} inflated to {got}, expected {want}")
            }
            Self::Inflate(why) => write!(f, "index did not inflate: {why}"),
            Self::UnknownCompression { compression, hash } => write!(
                f,
                "unknown compression {compression:#x} for entry {hash:#018x}"
            ),
            Self::GDeflate { hash, source } => {
                write!(f, "GDeflate error for entry {hash:#018x}: {source}")
            }
            Self::SizeMismatch { hash, got, want } => write!(
                f,
                "entry {hash:#018x} decompressed to {got}, expected {want} (mod 2**24)"
            ),
            Self::NotFound(p) => write!(f, "not found: {p}"),
            Self::MalformedListing => write!(f, "malformed directory listing"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// One part descriptor and its header bytes.
#[derive(Debug, Clone)]
pub struct Part {
    pub kind: u8,
    pub word_offset: u32,
    pub header: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub hash: u64,
    pub flags1: u16,
    pub parts: Vec<Part>,
    pub offset: u64,
    pub zsize: u32,
    pub size: u32,
    pub compression: u8,
    pub flags2: u8,
    pub flags3: u8,
    pub is_dir: bool,
    pub is_tobj: bool,
    pub preload: bool,
    /// Resolved path, once the directory tree has been walked.
    pub name: Option<String>,
}

impl Entry {
    pub const fn is_compressed(&self) -> bool {
        self.compression == COMPRESSION_ZLIB
    }

    /// The image part of a tobj entry, if it has one.
    pub fn image_header(&self) -> Option<&[u8]> {
        self.parts
            .iter()
            .find(|p| p.kind == 1)
            .map(|p| p.header.as_slice())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Header {
    pub version: u16,
    pub salt: u16,
    pub entry_count: u32,
    pub entry_table_size: u32,
    pub meta_word_count: u32,
    pub meta_table_size: u32,
    pub entry_table_offset: u64,
    pub meta_table_offset: u64,
    pub security_offset: u64,
}

pub struct HashFsV2 {
    pub path: PathBuf,
    file: File,
    pub header: Header,
    pub entries: HashMap<u64, Entry>,
    pub root_hash: Option<u64>,
}

/// Result of walking the directory tree.
#[derive(Debug, Default)]
pub struct Tree {
    pub dirs: Vec<String>,
    pub files: Vec<String>,
    pub unresolved: usize,
}

fn inflate(raw: &[u8], expected: Option<usize>, limit: usize) -> Result<Vec<u8>, Error> {
    use flate2::read::ZlibDecoder;
    let mut out = Vec::with_capacity(expected.unwrap_or(raw.len()).min(limit).min(1024 * 1024));
    ZlibDecoder::new(raw)
        .take(limit as u64 + 1)
        .read_to_end(&mut out)
        .map_err(|e| Error::Inflate(e.to_string()))?;
    if out.len() > limit {
        return Err(Error::Limit("decompressed data"));
    }
    Ok(out)
}

impl HashFsV2 {
    /// Open and index an archive.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref().to_path_buf();
        let mut file = File::open(&path)?;
        let header = Self::read_header(&mut file)?;
        let mut fs = Self {
            path,
            file,
            header,
            entries: HashMap::new(),
            root_hash: None,
        };
        fs.read_entries()?;
        let root = cityhash64(b"");
        if fs.entries.get(&root).is_some_and(|e| e.is_dir) {
            fs.root_hash = Some(root);
            if let Some(e) = fs.entries.get_mut(&root) {
                e.name = Some(String::new());
            }
        }
        Ok(fs)
    }

    fn read_header(file: &mut File) -> Result<Header, Error> {
        let mut buf = [0u8; HEADER_LEN];
        file.seek(SeekFrom::Start(0))?;
        if file.read(&mut buf)? < HEADER_LEN {
            return Err(Error::TooSmall);
        }
        let magic: [u8; 4] = buf[0..4].try_into().expect("4 bytes");
        if &magic != MAGIC {
            return Err(Error::BadMagic(magic));
        }
        let version = u16::from_le_bytes(buf[4..6].try_into().expect("2 bytes"));
        if version != 2 {
            return Err(Error::UnsupportedVersion(version));
        }
        let salt = u16::from_le_bytes(buf[6..8].try_into().expect("2 bytes"));
        if salt != 0 {
            return Err(Error::UnsupportedSalt(salt));
        }
        let hash_method: [u8; 4] = buf[8..12].try_into().expect("4 bytes");
        if &hash_method != b"CITY" {
            return Err(Error::UnsupportedHashMethod(hash_method));
        }
        let u32_at = |o: usize| u32::from_le_bytes(buf[o..o + 4].try_into().expect("4 bytes"));
        let u64_at = |o: usize| u64::from_le_bytes(buf[o..o + 8].try_into().expect("8 bytes"));
        Ok(Header {
            version,
            salt,
            entry_count: u32_at(0x0C),
            entry_table_size: u32_at(0x10),
            meta_word_count: u32_at(0x14),
            meta_table_size: u32_at(0x18),
            entry_table_offset: u64_at(0x1C),
            meta_table_offset: u64_at(0x24),
            security_offset: u64_at(0x2C),
        })
    }

    fn read_index(&self, offset: u64, comp_size: u32, expected: usize) -> Result<Vec<u8>, Error> {
        if comp_size as usize > crate::MAX_ARCHIVE_INDEX
            || expected > crate::MAX_ARCHIVE_INDEX
            || offset
                .checked_add(u64::from(comp_size))
                .is_none_or(|end| end > self.file.metadata().map(|m| m.len()).unwrap_or(0))
        {
            return Err(Error::Limit("index size or offset"));
        }
        let mut raw = vec![0u8; comp_size as usize];
        let got = read_at(&self.file, &mut raw, offset)?;
        if got != raw.len() {
            return Err(Error::ShortRead {
                at: offset,
                got,
                want: raw.len(),
            });
        }
        let data = inflate(&raw, Some(expected), expected)?;
        if data.len() != expected {
            return Err(Error::IndexSize {
                at: offset,
                got: data.len(),
                want: expected,
            });
        }
        Ok(data)
    }

    fn read_entries(&mut self) -> Result<(), Error> {
        let h = self.header;
        if h.entry_count as usize > crate::MAX_ARCHIVE_ENTRIES
            || h.meta_word_count as usize > crate::MAX_ARCHIVE_INDEX / 4
        {
            return Err(Error::Limit("index entry count"));
        }
        let index1 = self.read_index(
            h.entry_table_offset,
            h.entry_table_size,
            h.entry_count as usize * 0x10,
        )?;
        let index2 = self.read_index(
            h.meta_table_offset,
            h.meta_table_size,
            h.meta_word_count as usize * 4,
        )?;

        let mut part_budget = 0usize;
        for j in 0..h.entry_count as usize {
            let at = j * 0x10;
            let hash = u64::from_le_bytes(index1[at..at + 8].try_into().expect("8 bytes"));
            let meta_index =
                u32::from_le_bytes(index1[at + 8..at + 12].try_into().expect("4 bytes")) as usize;
            let part_count =
                u16::from_le_bytes(index1[at + 12..at + 14].try_into().expect("2 bytes")) as usize;
            part_budget = part_budget
                .checked_add(part_count)
                .ok_or(Error::Limit("metadata parts"))?;
            if part_count > 16 || part_budget > crate::MAX_ARCHIVE_ENTRIES * 4 {
                return Err(Error::Limit("metadata parts"));
            }
            let flags1 = u16::from_le_bytes(index1[at + 14..at + 16].try_into().expect("2 bytes"));

            let mut parts = Vec::with_capacity(part_count);
            let mut data_header: Option<&[u8]> = None;
            for k in 0..part_count {
                let d = (meta_index + k) * 4;
                if d + 4 > index2.len() {
                    break;
                }
                let off_low = u16::from_le_bytes(index2[d..d + 2].try_into().expect("2 bytes"));
                let off_high = index2[d + 2];
                let kind = index2[d + 3];
                let word_offset = u32::from(off_low) | (u32::from(off_high) << 16);
                let hsize = if kind & 0x80 != 0 {
                    16
                } else if kind == 1 {
                    8
                } else if kind == 2 {
                    4
                } else {
                    0
                };
                let start = word_offset as usize * 4;
                let end = (start + hsize).min(index2.len());
                let header = index2.get(start..end).unwrap_or(&[]).to_vec();
                if kind & 0x80 != 0 && header.len() == 16 {
                    data_header = Some(&index2[start..start + 16]);
                }
                parts.push(Part {
                    kind,
                    word_offset,
                    header,
                });
            }

            let (offset, zsize, size, compression, flags2, flags3) = match data_header {
                None => (0, 0, 0, COMPRESSION_NONE, 0, 0),
                Some(d) => {
                    let zsize = u32::from(u16::from_le_bytes(d[0..2].try_into().expect("2 bytes")))
                        | (u32::from(d[2]) << 16);
                    let flags2 = d[3];
                    let size = u32::from(u16::from_le_bytes(d[4..6].try_into().expect("2 bytes")))
                        | (u32::from(d[6]) << 16);
                    let flags3 = d[7];
                    let block_offset =
                        u64::from(u32::from_le_bytes(d[12..16].try_into().expect("4 bytes")));
                    (
                        block_offset * 0x10,
                        zsize,
                        size,
                        flags2 & 0xF0,
                        flags2,
                        flags3,
                    )
                }
            };

            let is_tobj = parts.first().is_some_and(|p| p.kind == 1);
            self.entries.insert(
                hash,
                Entry {
                    hash,
                    flags1,
                    parts,
                    offset,
                    zsize,
                    size,
                    compression,
                    flags2,
                    flags3,
                    is_dir: flags1 & 1 != 0,
                    is_tobj,
                    preload: flags1 & 4 != 0,
                    name: None,
                },
            );
        }
        Ok(())
    }

    /// Both size fields are 24-bit and wrap for payloads over 16 MiB. The
    /// `GDeflate` tile stream is self-describing, so recover the true compressed
    /// size from its header and tile-offset table.
    fn gdeflate_true_zsize(&self, entry_offset: u64, stored: u32) -> Result<u32, Error> {
        let mut head = [0u8; 8];
        if read_at(&self.file, &mut head, entry_offset)? < 8 || head[0] != 0x04 || head[1] != 0xFB {
            return Ok(stored); // not a tile stream; trust the field
        }
        let num_tiles = u16::from_le_bytes(head[2..4].try_into().expect("2 bytes")) as usize;
        let mut table = vec![0u8; num_tiles * 4];
        if read_at(&self.file, &mut table, entry_offset + 8)? < table.len() {
            return Ok(stored);
        }
        let word = |i: usize| {
            u64::from(u32::from_le_bytes(
                table[i * 4..i * 4 + 4].try_into().expect("4 bytes"),
            ))
        };
        if num_tiles == 0 {
            return Ok(stored);
        }
        // Compressed data spans the header, the table, the last tile's offset
        // and its size (which entry [0] holds).
        let true_size = 8 + num_tiles as u64 * 4 + word(num_tiles - 1) + word(0);
        // Trust it only if the low 24 bits match the stored, possibly wrapped, field.
        Ok(if (true_size & 0xFF_FFFF) as u32 == stored {
            true_size as u32
        } else {
            stored
        })
    }

    /// Decompressed bytes of an entry's data payload.
    pub fn read_entry(&self, hash: u64) -> Result<Vec<u8>, Error> {
        self.read_entry_limited(hash, crate::MAX_ARCHIVE_ENTRY)
    }

    pub fn read_entry_limited(&self, hash: u64, limit: usize) -> Result<Vec<u8>, Error> {
        let limit = limit.min(crate::MAX_ARCHIVE_ENTRY);
        let entry = match self.entries.get(&hash) {
            Some(e) => (e.zsize, e.compression, e.offset, e.size, e.hash),
            None => return Err(Error::NotFound(format!("{hash:#018x}"))),
        };
        let (zsize, compression, offset, size, hash) = entry;
        if zsize == 0 {
            return Ok(Vec::new()); // empty entry, e.g. a runtime placeholder
        }
        let read_size = if compression == COMPRESSION_GDEFLATE {
            self.gdeflate_true_zsize(offset, zsize)?
        } else {
            zsize
        } as usize;
        if read_size > limit
            || size as usize > limit
            || offset
                .checked_add(read_size as u64)
                .is_none_or(|end| end > self.file.metadata().map(|m| m.len()).unwrap_or(0))
        {
            return Err(Error::Limit("entry size or offset"));
        }
        let mut raw = vec![0u8; read_size];
        let got = read_at(&self.file, &mut raw, offset)?;
        if got != read_size {
            return Err(Error::ShortRead {
                at: offset,
                got,
                want: read_size,
            });
        }

        let data = match compression {
            COMPRESSION_NONE => raw,
            COMPRESSION_ZLIB => inflate(&raw, Some(size as usize), limit)?,
            COMPRESSION_GDEFLATE => {
                // The uncompressed size wraps at 24 bits too, so decode without
                // a size argument and check modulo 2**24 below.
                gdeflate::decompress_limited(&raw, None, limit)
                    .map_err(|source| Error::GDeflate { hash, source })?
            }
            other => {
                return Err(Error::UnknownCompression {
                    compression: other,
                    hash,
                });
            }
        };

        if size != 0 && (data.len() as u32 & 0xFF_FFFF) != (size & 0xFF_FFFF) {
            return Err(Error::SizeMismatch {
                hash,
                got: data.len(),
                want: size,
            });
        }
        Ok(data)
    }

    /// Read a file by its archive path.
    pub fn read_path(&self, path: &str) -> Result<Vec<u8>, Error> {
        let hash = cityhash64(path.as_bytes());
        if !self.entries.contains_key(&hash) {
            return Err(Error::NotFound(path.to_string()));
        }
        self.read_entry(hash)
    }

    /// Parse a directory-index payload into `(subdirs, files)`.
    pub fn parse_dir_listing(data: &[u8]) -> Result<(Vec<String>, Vec<String>), Error> {
        if data.len() < 4 {
            return Err(Error::MalformedListing);
        }
        let count = u32::from_le_bytes(data[0..4].try_into().expect("4 bytes")) as usize;
        if 4 + count > data.len() {
            return Err(Error::MalformedListing);
        }
        let sizes = &data[4..4 + count];
        let mut off = 4 + count;
        let (mut subdirs, mut files) = (Vec::new(), Vec::new());
        for &n in sizes {
            let n = n as usize;
            if off + n > data.len() {
                return Err(Error::MalformedListing);
            }
            let item = &data[off..off + n];
            off += n;
            if item.first() == Some(&b'/') {
                subdirs.push(String::from_utf8_lossy(&item[1..]).into_owned());
            } else {
                files.push(String::from_utf8_lossy(item).into_owned());
            }
        }
        Ok((subdirs, files))
    }

    /// Walk directory listings from every known root, assigning names.
    ///
    /// Seeds the walk with [`DEFAULT_ROOTS`] plus a candidate derived from the
    /// archive filename (`locale.scs` -> `locale`), since an archive may be a
    /// subtree with no root, or may hide unlinked subtrees under an empty one.
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
            enqueue_dir(&mut self.entries, &mut seen, &mut queue, r);
        }

        let mut i = 0;
        while i < queue.len() {
            let dpath = queue[i].clone();
            i += 1;
            tree.dirs.push(dpath.clone());
            let hash = cityhash64(dpath.as_bytes());
            let Ok(data) = self.read_entry(hash) else {
                continue;
            };
            let Ok((subdirs, dfiles)) = Self::parse_dir_listing(&data) else {
                continue;
            };
            let prefix = if dpath.is_empty() {
                String::new()
            } else {
                format!("{dpath}/")
            };
            for sd in subdirs {
                let child = format!("{prefix}{sd}");
                enqueue_dir(&mut self.entries, &mut seen, &mut queue, &child);
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
}

fn enqueue_dir(
    entries: &mut HashMap<u64, Entry>,
    seen: &mut HashSet<u64>,
    queue: &mut Vec<String>,
    path: &str,
) {
    if path.len() > 4096 || queue.len() >= crate::MAX_ARCHIVE_ENTRIES {
        return;
    }
    let hash = cityhash64(path.as_bytes());
    if seen.contains(&hash) {
        return;
    }
    match entries.get_mut(&hash) {
        Some(e) if e.is_dir => {
            seen.insert(hash);
            e.name = Some(path.to_string());
            queue.push(path.to_string());
        }
        _ => {}
    }
}

/// Read at an absolute offset without touching a shared seek cursor, so that
/// reads take `&File` and any number of threads can share one open archive.
pub(crate) fn read_at(file: &File, buf: &mut [u8], offset: u64) -> std::io::Result<usize> {
    if offset.checked_add(buf.len() as u64).is_none() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "archive offset overflow",
        ));
    }
    #[cfg(unix)]
    use std::os::unix::fs::FileExt;
    #[cfg(windows)]
    use std::os::windows::fs::FileExt;

    let mut total = 0;
    while total < buf.len() {
        #[cfg(unix)]
        let n = file.read_at(&mut buf[total..], offset + total as u64)?;
        #[cfg(windows)]
        let n = file.seek_read(&mut buf[total..], offset + total as u64)?;
        if n == 0 {
            break;
        }
        total += n;
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_directory_listing() {
        // count=3, lengths, then "/sub", "a.sii", "b.txt"
        let mut data = 3u32.to_le_bytes().to_vec();
        data.extend_from_slice(&[4, 5, 5]);
        data.extend_from_slice(b"/sub");
        data.extend_from_slice(b"a.sii");
        data.extend_from_slice(b"b.txt");
        let (subdirs, files) = HashFsV2::parse_dir_listing(&data).unwrap();
        assert_eq!(subdirs, vec!["sub"]);
        assert_eq!(files, vec!["a.sii", "b.txt"]);
    }

    #[test]
    fn rejects_a_truncated_listing() {
        let mut data = 2u32.to_le_bytes().to_vec();
        data.extend_from_slice(&[4, 40]);
        data.extend_from_slice(b"/sub");
        assert!(HashFsV2::parse_dir_listing(&data).is_err());
        assert!(HashFsV2::parse_dir_listing(&[0u8; 2]).is_err());
    }

    #[test]
    fn empty_listing_is_valid() {
        let (s, f) = HashFsV2::parse_dir_listing(&0u32.to_le_bytes()).unwrap();
        assert!(s.is_empty() && f.is_empty());
    }
}
