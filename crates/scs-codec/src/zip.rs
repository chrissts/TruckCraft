//! Minimal ZIP reader.
//!
//! Some `.scs` files are plain zip archives rather than `HashFS`. Only what that
//! needs is implemented: the central directory, stored and deflated members.
//! Encryption, multi-disk archives and Zip64 are reported as errors rather
//! than guessed at.

use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const EOCD_SIG: u32 = 0x0605_4b50;
const CD_SIG: u32 = 0x0201_4b50;
const LOCAL_SIG: u32 = 0x0403_4b50;
const EOCD_MIN: usize = 22;
const MAX_COMMENT: usize = 0xFFFF;

pub const METHOD_STORE: u16 = 0;
pub const METHOD_DEFLATE: u16 = 8;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    NotAZip,
    UnsupportedMethod(u16),
    Encrypted(String),
    Zip64,
    MultiDisk,
    Corrupt(String),
    Inflate(String),
    NotFound(String),
    SizeMismatch { name: String, got: usize, want: u32 },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::NotAZip => write!(f, "no end-of-central-directory record found"),
            Self::UnsupportedMethod(m) => write!(f, "unsupported compression method {m}"),
            Self::Encrypted(n) => write!(f, "member {n:?} is encrypted"),
            Self::Zip64 => write!(f, "Zip64 archives are not supported"),
            Self::MultiDisk => write!(f, "multi-disk archives are not supported"),
            Self::Corrupt(why) => write!(f, "corrupt zip: {why}"),
            Self::Inflate(why) => write!(f, "member did not inflate: {why}"),
            Self::NotFound(n) => write!(f, "not found: {n}"),
            Self::SizeMismatch { name, got, want } => {
                write!(f, "member {name:?} inflated to {got}, expected {want}")
            }
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

#[derive(Debug, Clone)]
pub struct Member {
    pub name: String,
    pub method: u16,
    pub crc: u32,
    pub compressed_size: u32,
    pub size: u32,
    pub local_offset: u32,
    pub is_dir: bool,
}

pub struct ZipArchive {
    pub path: PathBuf,
    file: File,
    pub members: Vec<Member>,
    by_name: HashMap<String, usize>,
}

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes(b[o..o + 2].try_into().expect("2 bytes"))
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().expect("4 bytes"))
}

impl ZipArchive {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref().to_path_buf();
        let mut file = File::open(&path)?;
        let len = file.seek(SeekFrom::End(0))? as usize;
        if len < EOCD_MIN {
            return Err(Error::NotAZip);
        }

        // The EOCD sits at the end, behind an optional comment of up to 64 KiB.
        let tail_len = len.min(EOCD_MIN + MAX_COMMENT);
        file.seek(SeekFrom::Start((len - tail_len) as u64))?;
        let mut tail = vec![0u8; tail_len];
        file.read_exact(&mut tail)?;
        let eocd = (0..=tail_len.saturating_sub(EOCD_MIN))
            .rev()
            .find(|&i| {
                u32_at(&tail, i) == EOCD_SIG
                    && i + EOCD_MIN + usize::from(u16_at(&tail, i + 20)) == tail.len()
            })
            .ok_or(Error::NotAZip)?;

        let entry_count = u16_at(&tail, eocd + 10) as usize;
        let cd_size = u32_at(&tail, eocd + 12) as usize;
        let cd_offset = u32_at(&tail, eocd + 16);
        if u16_at(&tail, eocd + 4) != 0 || u16_at(&tail, eocd + 6) != 0 {
            return Err(Error::MultiDisk);
        }
        if usize::from(u16_at(&tail, eocd + 8)) != entry_count {
            return Err(Error::Corrupt(
                "central directory entry counts disagree".into(),
            ));
        }
        if cd_offset == u32::MAX || cd_size == u32::MAX as usize {
            return Err(Error::Zip64);
        }
        if cd_size > crate::MAX_ARCHIVE_INDEX
            || u64::from(cd_offset) + cd_size as u64 > (len - tail_len + eocd) as u64
        {
            return Err(Error::Corrupt("central directory exceeds bounds".into()));
        }

        file.seek(SeekFrom::Start(u64::from(cd_offset)))?;
        let mut cd = vec![0u8; cd_size];
        file.read_exact(&mut cd)?;

        let mut members = Vec::with_capacity(entry_count);
        let mut by_name = HashMap::with_capacity(entry_count);
        let mut at = 0usize;
        for _ in 0..entry_count {
            if at + 46 > cd.len() || u32_at(&cd, at) != CD_SIG {
                return Err(Error::Corrupt("invalid central directory entry".into()));
            }
            let flags = u16_at(&cd, at + 8);
            let method = u16_at(&cd, at + 10);
            let crc = u32_at(&cd, at + 16);
            let compressed_size = u32_at(&cd, at + 20);
            let size = u32_at(&cd, at + 24);
            let name_len = u16_at(&cd, at + 28) as usize;
            let extra_len = u16_at(&cd, at + 30) as usize;
            let comment_len = u16_at(&cd, at + 32) as usize;
            let local_offset = u32_at(&cd, at + 42);
            if u16_at(&cd, at + 34) != 0 {
                return Err(Error::MultiDisk);
            }
            if compressed_size == u32::MAX || size == u32::MAX || local_offset == u32::MAX {
                return Err(Error::Zip64);
            }
            let name_at = at + 46;
            if name_at + name_len + extra_len + comment_len > cd.len() {
                return Err(Error::Corrupt("central directory name overruns".into()));
            }
            let name = String::from_utf8_lossy(&cd[name_at..name_at + name_len]).into_owned();
            if flags & 1 != 0 {
                return Err(Error::Encrypted(name));
            }
            let is_dir = name.ends_with('/');
            if by_name.insert(name.clone(), members.len()).is_some() {
                return Err(Error::Corrupt("duplicate member name".into()));
            }
            members.push(Member {
                name,
                method,
                crc,
                compressed_size,
                size,
                local_offset,
                is_dir,
            });
            at = name_at + name_len + extra_len + comment_len;
        }
        if at != cd.len() {
            return Err(Error::Corrupt("unexpected central directory data".into()));
        }

        Ok(Self {
            path,
            file,
            members,
            by_name,
        })
    }

    pub fn read_index(&self, index: usize) -> Result<Vec<u8>, Error> {
        self.read_index_limited(index, crate::MAX_ARCHIVE_ENTRY)
    }

    pub fn read_index_limited(&self, index: usize, limit: usize) -> Result<Vec<u8>, Error> {
        let limit = limit.min(crate::MAX_ARCHIVE_ENTRY);
        let m = self
            .members
            .get(index)
            .ok_or_else(|| Error::NotFound(format!("index {index}")))?
            .clone();
        if m.is_dir {
            return Ok(Vec::new());
        }
        if m.compressed_size as usize > limit || m.size as usize > limit {
            return Err(Error::Corrupt("member exceeds size limit".into()));
        }
        // The local header repeats the lengths, and its are the ones that
        // locate the data.
        let mut local = [0u8; 30];
        let got = crate::hashfs2::read_at(&self.file, &mut local, u64::from(m.local_offset))?;
        if got != local.len() || u32_at(&local, 0) != LOCAL_SIG {
            return Err(Error::Corrupt(format!(
                "member {:?} has no local header",
                m.name
            )));
        }
        if u16_at(&local, 6) & 1 != 0 {
            return Err(Error::Encrypted(m.name));
        }
        if u16_at(&local, 8) != m.method {
            return Err(Error::Corrupt("local compression method disagrees".into()));
        }
        if u16_at(&local, 6) & 8 == 0
            && (u32_at(&local, 14) != m.crc
                || u32_at(&local, 18) != m.compressed_size
                || u32_at(&local, 22) != m.size)
        {
            return Err(Error::Corrupt("local size or checksum disagrees".into()));
        }
        let mut local_name = vec![0u8; usize::from(u16_at(&local, 26))];
        if crate::hashfs2::read_at(&self.file, &mut local_name, u64::from(m.local_offset) + 30)?
            != local_name.len()
            || local_name != m.name.as_bytes()
        {
            return Err(Error::Corrupt("local member name disagrees".into()));
        }
        let data_at = u64::from(m.local_offset)
            + 30
            + u64::from(u16_at(&local, 26))
            + u64::from(u16_at(&local, 28));

        let mut raw = vec![0u8; m.compressed_size as usize];
        if data_at
            .checked_add(raw.len() as u64)
            .is_none_or(|end| end > self.file.metadata().map(|m| m.len()).unwrap_or(0))
        {
            return Err(Error::Corrupt(
                "member offset exceeds archive bounds".into(),
            ));
        }
        if crate::hashfs2::read_at(&self.file, &mut raw, data_at)? != raw.len() {
            return Err(Error::Corrupt("truncated member".into()));
        }

        let out = match m.method {
            METHOD_STORE => raw,
            METHOD_DEFLATE => {
                use flate2::read::DeflateDecoder;
                let mut out = Vec::with_capacity(m.size as usize);
                DeflateDecoder::new(raw.as_slice())
                    .take(limit as u64 + 1)
                    .read_to_end(&mut out)
                    .map_err(|e| Error::Inflate(e.to_string()))?;
                if out.len() > limit {
                    return Err(Error::Corrupt(
                        "decompressed member exceeds size limit".into(),
                    ));
                }
                out
            }
            other => return Err(Error::UnsupportedMethod(other)),
        };
        if out.len() != m.size as usize {
            return Err(Error::SizeMismatch {
                name: m.name,
                got: out.len(),
                want: m.size,
            });
        }
        let mut crc = flate2::Crc::new();
        crc.update(&out);
        if crc.sum() != m.crc {
            return Err(Error::Corrupt("member checksum mismatch".into()));
        }
        Ok(out)
    }

    pub fn read_path(&self, path: &str) -> Result<Vec<u8>, Error> {
        self.read_path_limited(path, crate::MAX_ARCHIVE_ENTRY)
    }

    pub fn read_path_limited(&self, path: &str, limit: usize) -> Result<Vec<u8>, Error> {
        let index = *self
            .by_name
            .get(path)
            .ok_or_else(|| Error::NotFound(path.to_string()))?;
        self.read_index_limited(index, limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A one-member stored zip, written by hand so the reader is exercised
    /// against bytes this crate did not produce with the same code path.
    fn stored_zip(name: &str, body: &[u8]) -> Vec<u8> {
        let crc = crc32(body);
        let mut out = Vec::new();
        let local_offset = 0u32;
        // local header
        out.extend_from_slice(&LOCAL_SIG.to_le_bytes());
        out.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]); // ver, flags, method, time, date
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(body);
        let cd_offset = out.len() as u32;
        // central directory
        out.extend_from_slice(&CD_SIG.to_le_bytes());
        out.extend_from_slice(&[20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra
        out.extend_from_slice(&0u16.to_le_bytes()); // comment
        out.extend_from_slice(&0u16.to_le_bytes()); // disk
        out.extend_from_slice(&0u16.to_le_bytes()); // int attrs
        out.extend_from_slice(&0u32.to_le_bytes()); // ext attrs
        out.extend_from_slice(&local_offset.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        let cd_size = out.len() as u32 - cd_offset;
        // EOCD
        out.extend_from_slice(&EOCD_SIG.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&cd_size.to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &b in data {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    #[test]
    fn reads_a_stored_member() {
        let dir = std::env::temp_dir().join("scs_tool_zip_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.zip");
        let body = b"SiiNunit\n{\n}\n";
        File::create(&path)
            .unwrap()
            .write_all(&stored_zip("manifest.sii", body))
            .unwrap();

        let z = ZipArchive::open(&path).unwrap();
        assert_eq!(z.members.len(), 1);
        assert_eq!(z.members[0].name, "manifest.sii");
        assert!(!z.members[0].is_dir);
        assert_eq!(z.read_path("manifest.sii").unwrap(), body);
        assert!(z.read_path_limited("manifest.sii", body.len() - 1).is_err());
        assert_eq!(
            z.read_path_limited("manifest.sii", body.len()).unwrap(),
            body
        );
        assert!(z.read_path("nope.sii").is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_a_file_that_is_not_a_zip() {
        let dir = std::env::temp_dir().join("scs_tool_zip_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("notzip.bin");
        File::create(&path)
            .unwrap()
            .write_all(b"nothing here")
            .unwrap();
        match ZipArchive::open(&path) {
            Err(Error::NotAZip) => {}
            Err(other) => panic!("expected NotAZip, got {other}"),
            Ok(_) => panic!("expected NotAZip, got a readable archive"),
        }
        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn rejects_corrupt_payload_and_inconsistent_directory_metadata() {
        let original = stored_zip("manifest.sii", b"SiiNunit\n{\n}\n");
        let path = std::env::temp_dir().join(format!("truckcraft-zip-{}.zip", std::process::id()));
        let mut corrupt = original.clone();
        corrupt[30 + "manifest.sii".len()] ^= 1;
        std::fs::write(&path, corrupt).unwrap();
        assert!(ZipArchive::open(&path).unwrap().read_index(0).is_err());
        let mut corrupt = original.clone();
        corrupt[30] ^= 1;
        std::fs::write(&path, corrupt).unwrap();
        assert!(ZipArchive::open(&path).unwrap().read_index(0).is_err());
        let mut corrupt = original.clone();
        let eocd = corrupt.len() - EOCD_MIN;
        corrupt[eocd + 8..eocd + 10].copy_from_slice(&2u16.to_le_bytes());
        std::fs::write(&path, corrupt).unwrap();
        assert!(ZipArchive::open(&path).is_err());
        let mut corrupt = original.clone();
        corrupt[eocd + 8..eocd + 12].copy_from_slice(&[2, 0, 2, 0]);
        std::fs::write(&path, corrupt).unwrap();
        assert!(ZipArchive::open(&path).is_err());
        let mut corrupt = original;
        let cd = u32_at(&corrupt, eocd + 16) as usize;
        corrupt[cd + 30..cd + 32].copy_from_slice(&u16::MAX.to_le_bytes());
        std::fs::write(&path, corrupt).unwrap();
        assert!(ZipArchive::open(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
