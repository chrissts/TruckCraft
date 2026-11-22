//! Reading the binary form.

use std::collections::HashMap;
use std::fmt;

use super::{Block, BsiiFile, CHAR_TABLE, FieldProto, Id, MAGIC, Prototype, Value, array_of, ty};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    LimitExceeded,
    TooShort,
    BadSignature([u8; 4]),
    UnsupportedVersion(u32),
    Truncated {
        at: usize,
        want: usize,
        left: usize,
    },
    LengthOverruns {
        at: usize,
        len: u32,
        left: usize,
    },
    BadCharIndex(u64),
    UnsupportedType {
        type_id: u32,
        at: usize,
    },
    InvalidStructureBlock,
    ReservedStructureId,
    DuplicateStructureId(u32),
    UndefinedStructure {
        id: u32,
        at: usize,
    },
    NoTerminator,
    ValueCountMismatch {
        name: String,
        got: usize,
        want: usize,
    },
    NoTextFormat {
        type_id: u32,
        field: String,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LimitExceeded => write!(
                f,
                "binary save exceeds the supported structure/value limits"
            ),
            Self::TooShort => write!(f, "too short to be a BSII file"),
            Self::BadSignature(got) => write!(
                f,
                "bad signature {:?}, expected {:?}",
                String::from_utf8_lossy(got),
                String::from_utf8_lossy(MAGIC)
            ),
            Self::UnsupportedVersion(v) => {
                write!(
                    f,
                    "unsupported BSII format version {v} (supported: 1, 2, 3)"
                )
            }
            Self::Truncated { at, want, left } => {
                write!(
                    f,
                    "truncated at offset {at} (want {want} bytes, {left} left)"
                )
            }
            Self::LengthOverruns { at, len, left } => write!(
                f,
                "length {len} exceeds remaining {left} bytes at offset {at}"
            ),
            Self::BadCharIndex(i) => write!(f, "bad encoded-string char index {i}"),
            Self::UnsupportedType { type_id, at } => {
                write!(f, "unsupported value type 0x{type_id:08x} at offset {at}")
            }
            Self::InvalidStructureBlock => write!(f, "structure block marked invalid"),
            Self::ReservedStructureId => write!(f, "structure ID 0 is reserved"),
            Self::DuplicateStructureId(id) => write!(f, "duplicate structure ID {id}"),
            Self::UndefinedStructure { id, at } => write!(
                f,
                "data block references undefined structure ID {id} at offset {at}"
            ),
            Self::NoTerminator => write!(f, "file ends without a terminating structure block"),
            Self::ValueCountMismatch { name, got, want } => write!(
                f,
                "block of {name:?} has {got} values but the structure defines {want}"
            ),
            Self::NoTextFormat { type_id, field } => write!(
                f,
                "no text format for value type 0x{type_id:02x} (field {field:?})"
            ),
        }
    }
}

impl std::error::Error for Error {}

/// A little-endian cursor with bounds-checked primitives.
struct Reader<'a> {
    data: &'a [u8],
    off: usize,
    remaining_values: usize,
}

macro_rules! read_scalar {
    ($name:ident, $ty:ty) => {
        fn $name(&mut self) -> Result<$ty, Error> {
            const N: usize = std::mem::size_of::<$ty>();
            let bytes = self.raw(N)?;
            Ok(<$ty>::from_le_bytes(
                bytes.try_into().expect("checked length"),
            ))
        }
    };
}

impl<'a> Reader<'a> {
    const fn new(data: &'a [u8]) -> Self {
        Reader {
            data,
            off: 0,
            remaining_values: 4_000_000,
        }
    }

    const fn left(&self) -> usize {
        self.data.len() - self.off
    }

    fn raw(&mut self, n: usize) -> Result<&'a [u8], Error> {
        if n > self.left() {
            return Err(Error::Truncated {
                at: self.off,
                want: n,
                left: self.left(),
            });
        }
        let out = &self.data[self.off..self.off + n];
        self.off += n;
        Ok(out)
    }

    read_scalar!(u8_, u8);
    read_scalar!(u16_, u16);
    read_scalar!(u32_, u32);
    read_scalar!(i32_, i32);
    read_scalar!(u64_, u64);
    read_scalar!(i64_, i64);
    read_scalar!(f32_, f32);

    fn peek_u32(&self) -> Result<u32, Error> {
        if self.left() < 4 {
            return Err(Error::Truncated {
                at: self.off,
                want: 4,
                left: self.left(),
            });
        }
        Ok(u32::from_le_bytes(
            self.data[self.off..self.off + 4]
                .try_into()
                .expect("4 bytes"),
        ))
    }

    fn floats<const N: usize>(&mut self) -> Result<[f32; N], Error> {
        let mut out = [0f32; N];
        for slot in &mut out {
            *slot = self.f32_()?;
        }
        Ok(out)
    }

    /// A count prefix, sanity-checked against the bytes that remain.
    fn count(&mut self) -> Result<usize, Error> {
        let at = self.off;
        let n = self.u32_()?;
        if n as usize > self.left() {
            return Err(Error::LengthOverruns {
                at,
                len: n,
                left: self.left(),
            });
        }
        Ok(n as usize)
    }

    /// u32 length + that many raw bytes, not null-terminated.
    fn string(&mut self) -> Result<&'a [u8], Error> {
        let n = self.count()?;
        self.raw(n)
    }

    fn name(&mut self) -> Result<String, Error> {
        Ok(String::from_utf8_lossy(self.string()?).into_owned())
    }

    /// A base-38 packed identifier stored in 63 bits of a u64.
    fn encoded(&mut self) -> Result<String, Error> {
        let mut value = self.u64_()? & !(1u64 << 63);
        let mut out = String::new();
        while value > 0 {
            let index = value % 38;
            value /= 38;
            if !(1..=37).contains(&index) {
                return Err(Error::BadCharIndex(index));
            }
            out.push(CHAR_TABLE[index as usize - 1] as char);
        }
        Ok(out)
    }

    fn id(&mut self) -> Result<Id, Error> {
        let n = self.u8_()?;
        if n == 0xff {
            return Ok(Id::Nameless(self.u64_()?));
        }
        let mut parts = Vec::with_capacity(n as usize);
        for _ in 0..n {
            parts.push(self.encoded()?);
        }
        Ok(Id::Named(parts))
    }
}

/// Type 0x19: 7 floats (v1) or 8 with two biases packed into the 4th (v2+).
fn placement(r: &mut Reader<'_>, version: u32) -> Result<[f32; 7], Error> {
    if version == 1 {
        return r.floats::<7>();
    }
    let v = r.floats::<8>()?;
    // Truncated to Int64 like the reference, and the biased components are
    // stored back into a Single, so both additions round to 32 bits.
    let coef = if v[3].is_finite() { v[3] as i64 } else { 0 };
    Ok([
        v[0] + (((coef & 0xfff) - 2048) << 9) as f32,
        v[1],
        v[2] + ((((coef >> 12) & 0xfff) - 2048) << 9) as f32,
        v[4],
        v[5],
        v[6],
        v[7],
    ])
}

fn read_value<'a>(r: &mut Reader<'a>, type_id: u32, version: u32) -> Result<Value<'a>, Error> {
    r.remaining_values = r
        .remaining_values
        .checked_sub(1)
        .ok_or(Error::LimitExceeded)?;
    if let Some(elem) = array_of(type_id) {
        let n = r.count()?;
        if n > r.remaining_values {
            return Err(Error::LimitExceeded);
        }
        let mut items = Vec::with_capacity(n.min(4096));
        for _ in 0..n {
            items.push(read_value(r, elem, version)?);
        }
        return Ok(Value::Array(items));
    }
    Ok(match type_id {
        ty::STRING => Value::String(r.string()?),
        ty::ENCODED => Value::Encoded(r.encoded()?),
        ty::FLOAT => Value::Float(r.f32_()?),
        ty::VEC2F => Value::Vec2(r.floats::<2>()?),
        ty::VEC3F => Value::Vec3(r.floats::<3>()?),
        ty::VEC4F => Value::Vec4(r.floats::<4>()?),
        ty::PLACEMENT => Value::Placement(placement(r, version)?),
        ty::VEC3I => Value::Vec3i([r.i32_()?, r.i32_()?, r.i32_()?]),
        ty::INT32 => Value::Int32(r.i32_()?),
        ty::UINT32 | ty::UINT32_ALT => Value::UInt32(r.u32_()?),
        ty::ORDINAL => Value::Ordinal(r.u32_()?),
        ty::UINT16 => Value::UInt16(r.u16_()?),
        ty::INT64 => Value::Int64(r.i64_()?),
        ty::UINT64 => Value::UInt64(r.u64_()?),
        ty::BOOL => Value::Bool(r.u8_()? != 0),
        ty::ID | ty::ID_ALT | ty::ID_ALT2 => Value::Id(r.id()?),
        _ => return Err(Error::UnsupportedType { type_id, at: r.off }),
    })
}

fn read_prototype(r: &mut Reader<'_>) -> Result<Prototype, Error> {
    r.u32_()?; // block type, already known to be 0
    if r.u8_()? != 1 {
        return Err(Error::InvalidStructureBlock);
    }
    let id = r.u32_()?;
    if id == 0 {
        return Err(Error::ReservedStructureId);
    }
    let name = r.name()?;
    let mut fields = Vec::new();
    loop {
        let type_id = r.u32_()?;
        if type_id == 0 {
            break;
        }
        let field_name = r.name()?;
        let ordinals = if type_id == ty::ORDINAL {
            let n = r.count()?;
            let mut map = HashMap::with_capacity(n.min(4096));
            for _ in 0..n {
                let key = r.u32_()?;
                map.insert(key, r.string()?.to_vec());
            }
            Some(map)
        } else {
            None
        };
        fields.push(FieldProto {
            type_id,
            name: field_name,
            ordinals,
        });
        if fields.len() > 100_000 {
            return Err(Error::LimitExceeded);
        }
    }
    Ok(Prototype { id, name, fields })
}

/// Parse binary SII data.
pub fn parse(data: &[u8]) -> Result<BsiiFile<'_>, Error> {
    if data.len() < 8 {
        return Err(Error::TooShort);
    }
    if &data[..4] != MAGIC {
        let mut got = [0u8; 4];
        got.copy_from_slice(&data[..4]);
        return Err(Error::BadSignature(got));
    }
    let version = u32::from_le_bytes(data[4..8].try_into().expect("4 bytes"));
    if !super::SUPPORTED_VERSIONS.contains(&version) {
        return Err(Error::UnsupportedVersion(version));
    }

    let mut r = Reader::new(data);
    r.off = 8;
    let mut prototypes: HashMap<u32, Prototype> = HashMap::new();
    let mut blocks = Vec::new();
    loop {
        let block_type = r.peek_u32().map_err(|_| Error::NoTerminator)?;
        if block_type == 0 {
            if r.left() < 5 {
                return Err(Error::Truncated {
                    at: r.off,
                    want: 5,
                    left: r.left(),
                });
            }
            if r.data[r.off + 4] == 0 {
                // validity 0 marks the end of the stream
                return Ok(BsiiFile {
                    version,
                    prototypes,
                    blocks,
                });
            }
            let proto = read_prototype(&mut r)?;
            if prototypes.contains_key(&proto.id) {
                return Err(Error::DuplicateStructureId(proto.id));
            }
            prototypes.insert(proto.id, proto);
            if prototypes.len() > 100_000 {
                return Err(Error::LimitExceeded);
            }
        } else {
            let field_types: Vec<u32> = match prototypes.get(&block_type) {
                Some(p) => p.fields.iter().map(|f| f.type_id).collect(),
                None => {
                    return Err(Error::UndefinedStructure {
                        id: block_type,
                        at: r.off,
                    });
                }
            };
            r.off += 4;
            let id = r.id()?;
            let mut values = Vec::with_capacity(field_types.len());
            for type_id in field_types {
                values.push(read_value(&mut r, type_id, version)?);
            }
            blocks.push(Block {
                prototype_id: block_type,
                id,
                values,
            });
            if blocks.len() > 500_000 {
                return Err(Error::LimitExceeded);
            }
        }
    }
}
