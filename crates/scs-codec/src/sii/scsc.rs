//! `ScsC` — the AES-encrypted, zlib-compressed wrapper used for save games
//! and a few configuration files.
//!
//! ```text
//! char[4]  magic     "ScsC"
//! u8[32]   hmac      not verified; neither reference decoder checks it
//! u8[16]   iv        AES-CBC initialisation vector
//! u32      size      payload size after decompression
//! u8[]     data      AES-256-CBC ciphertext, whole blocks, no padding
//! ```
//!
//! The plaintext is a zlib stream holding either textual `SiiNunit` or binary
//! `BSII`; [`crate::sii::decode`] carries on from there.
//!
//! The key has been public since František Milt's `SII_Decrypt`. Nothing here
//! encrypts.

use std::fmt;
use std::io::Read;

use aes::cipher::{BlockDecryptMut, KeyIvInit, block_padding::NoPadding};
use flate2::read::ZlibDecoder;

pub const MAGIC: &[u8; 4] = b"ScsC";
pub const HEADER_LEN: usize = 4 + 32 + 16 + 4;
pub const BLOCK: usize = 16;
pub const MAX_DECODED_SIZE: usize = 256 * 1024 * 1024;

/// AES-256 key used by ETS2 / ATS for `ScsC` payloads.
pub const KEY: [u8; 32] = [
    0x2a, 0x5f, 0xcb, 0x17, 0x91, 0xd2, 0x2f, 0xb6, 0x02, 0x45, 0xb3, 0xd8, 0x36, 0x9e, 0xd0, 0xb2,
    0xc2, 0x73, 0x71, 0x56, 0x3f, 0xbf, 0x1f, 0x3c, 0x9e, 0xdf, 0x6b, 0x11, 0x82, 0x5a, 0x5d, 0x0a,
];

type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    TooShort { len: usize },
    BadSignature([u8; 4]),
    RaggedCiphertext { len: usize },
    Empty,
    Decrypt,
    Decompress(String),
    SizeMismatch { got: usize, declared: usize },
    TooLarge,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => write!(f, "decoded payload exceeds the supported size limit"),
            Self::TooShort { len } => {
                write!(
                    f,
                    "too short for an ScsC header ({len} < {HEADER_LEN} bytes)"
                )
            }
            Self::BadSignature(got) => write!(
                f,
                "bad signature {:?}, expected {:?}",
                String::from_utf8_lossy(got),
                String::from_utf8_lossy(MAGIC)
            ),
            Self::RaggedCiphertext { len } => {
                write!(f, "ciphertext is {len} bytes, not a multiple of {BLOCK}")
            }
            Self::Empty => write!(f, "ScsC file has no payload"),
            Self::Decrypt => write!(f, "payload did not decrypt"),
            Self::Decompress(why) => write!(
                f,
                "payload did not decompress ({why}) -- wrong key, or not an ScsC file"
            ),
            Self::SizeMismatch { got, declared } => write!(
                f,
                "payload decompressed to {got} bytes, header declares {declared}"
            ),
        }
    }
}

impl std::error::Error for Error {}

/// A parsed header, without decrypting anything.
#[derive(Debug, Clone)]
pub struct Header<'a> {
    pub hmac: &'a [u8],
    pub iv: &'a [u8],
    pub size: u32,
    pub ciphertext: &'a [u8],
}

pub fn is_scsc(data: &[u8]) -> bool {
    data.len() >= HEADER_LEN && &data[..4] == MAGIC
}

/// Split a blob into its header fields and ciphertext.
pub fn parse(data: &[u8]) -> Result<Header<'_>, Error> {
    if data.len() < HEADER_LEN {
        return Err(Error::TooShort { len: data.len() });
    }
    if &data[..4] != MAGIC {
        let mut got = [0u8; 4];
        got.copy_from_slice(&data[..4]);
        return Err(Error::BadSignature(got));
    }
    let ciphertext = &data[HEADER_LEN..];
    if !ciphertext.len().is_multiple_of(BLOCK) {
        return Err(Error::RaggedCiphertext {
            len: ciphertext.len(),
        });
    }
    Ok(Header {
        hmac: &data[4..36],
        iv: &data[36..52],
        size: u32::from_le_bytes(data[52..56].try_into().expect("4 bytes")),
        ciphertext,
    })
}

/// Decrypt and decompress an `ScsC` blob into its payload.
pub fn decode(data: &[u8]) -> Result<Vec<u8>, Error> {
    decode_with_key(data, &KEY)
}

pub fn decode_with_key(data: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, Error> {
    let header = parse(data)?;
    if header.ciphertext.is_empty() {
        return Err(Error::Empty);
    }
    let mut buf = header.ciphertext.to_vec();
    // Key and IV are fixed size, so construction cannot fail.
    let cipher = Aes256CbcDec::new_from_slices(key, header.iv).expect("fixed key/iv sizes");
    cipher
        .decrypt_padded_mut::<NoPadding>(&mut buf)
        .map_err(|_| Error::Decrypt)?;

    // The last cipher block is padded with whatever followed the zlib stream,
    // so read up to the declared size and ignore the tail rather than
    // insisting on a clean end of stream.
    let size = header.size as usize;
    if size > MAX_DECODED_SIZE {
        return Err(Error::TooLarge);
    }
    let decoder = ZlibDecoder::new(buf.as_slice());
    let mut out = Vec::with_capacity(size.min(1 << 20));
    decoder
        .take((MAX_DECODED_SIZE + 1) as u64)
        .read_to_end(&mut out)
        .map_err(|e| Error::Decompress(e.to_string()))?;
    if out.len() > MAX_DECODED_SIZE {
        return Err(Error::TooLarge);
    }
    if size != 0 && out.len() != size {
        return Err(Error::SizeMismatch {
            got: out.len(),
            declared: size,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Built by compressing and encrypting known plaintext with the game key.
    const SCSC_TEXT: &str = "53637343000102030405060708090a0b0c0d0e0f101112131415161718191a1b\
        1c1d1e1f0f1e2d3c4b5a69788796a5b4c3d2e1f03b0000003fe3dcc02f8d92ff76dcc6cac34f52f5feae5cfc\
        503256c20a0a6d27ea50aecde9977f7eadfa751932bb9048413b157646f4c37196edc2576946af755d5e1eb1";

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn decodes_a_known_payload() {
        let plain = b"SiiNunit\n{\nuser_profile : _nameless.1 {\n  name: \"Test\"\n}\n}\n";
        assert_eq!(decode(&hex(SCSC_TEXT)).unwrap(), plain);
    }

    #[test]
    fn reads_the_header_fields() {
        let data = hex(SCSC_TEXT);
        let h = parse(&data).unwrap();
        assert_eq!(h.hmac, (0..32u8).collect::<Vec<_>>().as_slice());
        assert_eq!(h.iv, hex("0f1e2d3c4b5a69788796a5b4c3d2e1f0"));
        assert_eq!(h.size, 59);
        assert_eq!(h.ciphertext.len() % BLOCK, 0);
    }

    #[test]
    fn rejects_malformed_input() {
        assert!(matches!(parse(b"ScsC"), Err(Error::TooShort { .. })));
        let mut bad = vec![b'X'; HEADER_LEN];
        assert!(matches!(parse(&bad), Err(Error::BadSignature(_))));
        bad[..4].copy_from_slice(MAGIC);
        bad.extend_from_slice(&[0u8; 5]);
        assert!(matches!(parse(&bad), Err(Error::RaggedCiphertext { .. })));
        assert!(decode_with_key(&hex(SCSC_TEXT), &[0u8; 32]).is_err());
    }

    #[test]
    fn recognises_its_own_signature() {
        assert!(is_scsc(&hex(SCSC_TEXT)));
        assert!(!is_scsc(b"SiiNunit\n{}"));
    }
}
