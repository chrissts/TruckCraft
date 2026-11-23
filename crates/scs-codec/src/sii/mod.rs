//! One entry point for every shape a SII unit file comes in.
//!
//! A `.sii` may be plain text, obfuscated, encrypted, binary, or a combination
//! — an `ScsC` file decrypts to either text or `BSII`, and either may itself be
//! `3nK`-encoded. [`decode`] peels the layers until textual `SiiNunit` is left:
//!
//! ```text
//! ScsC  --(AES-256-CBC + zlib)-->  BSII  --(serialise)-->  SiiNunit
//! 3nK   --(XOR key table)------->  SiiNunit
//! BSII  --(serialise)-----------> SiiNunit
//! ```

pub mod bsii;
pub mod nk3;
pub mod scsc;

use std::fmt;

pub const TEXT_MAGIC: &[u8; 8] = b"SiiNunit";

/// How many wrappers to unwrap before assuming a decoding loop.
pub const MAX_DEPTH: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Text,
    Bsii,
    Scsc,
    Nk3,
    Unknown,
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Text => "text",
            Self::Bsii => "bsii",
            Self::Scsc => "scsc",
            Self::Nk3 => "3nk",
            Self::Unknown => "unknown",
        })
    }
}

#[derive(Debug, Clone)]
pub enum Error {
    Scsc(scsc::Error),
    Bsii(bsii::Error),
    TooDeep(Format),
    TooLarge,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => write!(f, "SII payload exceeds the supported size limit"),
            Self::Scsc(e) => write!(f, "scsc: {e}"),
            Self::Bsii(e) => write!(f, "bsii: {e}"),
            Self::TooDeep(fmt_) => {
                write!(f, "still {fmt_} after {MAX_DEPTH} decoding rounds")
            }
        }
    }
}

impl std::error::Error for Error {}

impl From<scsc::Error> for Error {
    fn from(e: scsc::Error) -> Self {
        Self::Scsc(e)
    }
}

impl From<bsii::Error> for Error {
    fn from(e: bsii::Error) -> Self {
        Self::Bsii(e)
    }
}

/// Sniff which encoding `data` is in.
pub fn detect(data: &[u8]) -> Format {
    if data.len() >= 8 && &data[..8] == TEXT_MAGIC {
        Format::Text
    } else if bsii::is_bsii(data) {
        Format::Bsii
    } else if scsc::is_scsc(data) {
        Format::Scsc
    } else if nk3::is_3nk(data) {
        Format::Nk3
    } else {
        Format::Unknown
    }
}

/// True if `data` looks like a SII unit file in any of its encodings.
pub fn is_sii(data: &[u8]) -> bool {
    detect(data) != Format::Unknown
}

/// Decode down to textual `SiiNunit`.
///
/// `keep_binary` stops before converting `BSII` to text, which is what you
/// want if you are going to walk the binary structure yourself. Data that is
/// not SII at all is returned unchanged.
pub fn decode(data: &[u8], keep_binary: bool) -> Result<Vec<u8>, Error> {
    if data.len() > scsc::MAX_DECODED_SIZE {
        return Err(Error::TooLarge);
    }
    let mut current = data.to_vec();
    for _ in 0..MAX_DEPTH {
        if current.len() > scsc::MAX_DECODED_SIZE {
            return Err(Error::TooLarge);
        }
        match detect(&current) {
            Format::Text | Format::Unknown => return Ok(current),
            Format::Bsii if keep_binary => return Ok(current),
            Format::Nk3 => current = nk3::decode(&current),
            Format::Scsc => current = scsc::decode(&current)?,
            Format::Bsii => return Ok(bsii::to_text(&current)?.into_bytes()),
        }
    }
    Err(Error::TooDeep(detect(&current)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The worked example from the format specification.
    fn spec_bsii() -> Vec<u8> {
        let hex = "425349490200000000000000010100000\
            00f0000006669727374\
            5f73747275637475726525000000\
            0b000000696e7433325f6669656c64\
            360000001400000062797465626f6f6c5f61727261795f6669656c64\
            34000000180000\
            00656d7074795f75696e7436345f61727261795f6669656c64\
            0000000000000000010200000004000000\
            6c617374050000000c00000073696e676c655f6669656c6400000000\
            01000000ff0102030405060708ffffffff03000000000100\
            0000000002000000fff8f9fafbfcfdfeff0000803f0000000000";
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }

    const SPEC_TEXT: &str = "SiiNunit\n{\nfirst_structure : _nameless.807.0605.0403.0201 {\n  \
        int32_field: -1\n  bytebool_array_field: 3\n  bytebool_array_field[0]: false\n  \
        bytebool_array_field[1]: true\n  bytebool_array_field[2]: false\n  \
        empty_uint64_array_field: 0\n}\nlast : _nameless.fffe.fdfc.fbfa.f9f8 {\n  \
        single_field: 1\n}\n}\n";

    #[test]
    fn parses_the_specification_example() {
        let data = spec_bsii();
        let file = bsii::parse(&data).unwrap();
        assert_eq!(file.version, 2);
        assert_eq!(file.prototypes.len(), 2);
        assert_eq!(file.blocks.len(), 2);
        assert_eq!(file.prototypes[&1].name, "first_structure");
        assert_eq!(file.prototypes[&1].fields.len(), 3);
        assert_eq!(file.blocks[0].values[0], bsii::Value::Int32(-1));
        assert_eq!(file.blocks[1].values[0], bsii::Value::Float(1.0));
        assert_eq!(file.to_text().unwrap(), SPEC_TEXT);
    }

    #[test]
    fn detects_every_format() {
        assert_eq!(detect(b"SiiNunit\n{}"), Format::Text);
        assert_eq!(detect(&spec_bsii()), Format::Bsii);
        assert_eq!(detect(b"3nK\x01\x00\x00rest"), Format::Nk3);
        assert_eq!(detect(b"DDS \x00\x00\x00\x00"), Format::Unknown);
        assert!(!is_sii(b"PK\x03\x04"));
    }

    #[test]
    fn unwraps_layers() {
        let data = spec_bsii();
        assert_eq!(decode(&data, false).unwrap(), SPEC_TEXT.as_bytes());
        assert_eq!(decode(&data, true).unwrap(), data);
        assert_eq!(decode(b"SiiNunit\n{}", false).unwrap(), b"SiiNunit\n{}");
        assert_eq!(decode(b"DDS abc", false).unwrap(), b"DDS abc");

        // 3nK wrapping BSII: both layers have to come off.
        let seed = 0x11u8;
        let mut nested = vec![b'3', b'n', b'K', 1, 0, seed];
        nested.extend(nk3::decode(
            &[vec![b'3', b'n', b'K', 1, 0, seed], data].concat(),
        ));
        // The helper above re-encodes by XORing, which is its own inverse.
        assert_eq!(decode(&nested, false).unwrap(), SPEC_TEXT.as_bytes());
    }

    #[test]
    fn identifiers_render_like_the_reference() {
        use bsii::Id;
        assert_eq!(
            Id::Nameless(0x0807060504030201).to_string(),
            "_nameless.807.0605.0403.0201"
        );
        assert_eq!(
            Id::Nameless(0x060504030201).to_string(),
            "_nameless.605.0403.0201"
        );
        assert_eq!(Id::Nameless(0).to_string(), "_nameless.0");
        assert_eq!(
            Id::Named(vec!["first".into(), "second".into()]).to_string(),
            "first.second"
        );
        assert_eq!(Id::Named(vec![]).to_string(), "null");
    }
}
