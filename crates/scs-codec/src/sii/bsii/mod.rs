//! Binary SII (`BSII`) — the binary unit serialisation, and its conversion
//! back to textual `SiiNunit`.
//!
//! Format versions 1, 2 and 3 are supported. A file is a header followed by a
//! stream of blocks:
//!
//! * a **structure block** (block type 0) defines a named struct: an ID, a
//!   name and a list of `(value type, value name, type-specific data)` fields
//!   terminated by a value type of 0. A structure block whose validity byte is
//!   0 terminates the file.
//! * a **data block** (block type = some structure ID) is one instance: a
//!   block ID followed by the field values back to back, no separators.
//!
//! Structure blocks always precede the data blocks that use them.
//!
//! Version 1 differs from 2/3 only in value types `0x19` / `0x1A`, which hold
//! 7 plain floats there and 8 with two packed bias values here.
//!
//! Spec: `TheLazyTomcat/SII_Decrypt`, `Documents/Binary SII - {Format,Types}.txt`.

mod parse;
mod text;

pub use parse::{Error, parse};
pub use text::has_text_format;

use std::collections::HashMap;
use std::fmt;

pub const MAGIC: &[u8; 4] = b"BSII";
pub const SUPPORTED_VERSIONS: [u32; 3] = [1, 2, 3];

/// Encoded strings pack characters into a base-38 number; index 0 is unused.
pub const CHAR_TABLE: &[u8; 37] = b"0123456789abcdefghijklmnopqrstuvwxyz_";

pub mod ty {
    pub const STRING: u32 = 0x01;
    pub const STRING_ARRAY: u32 = 0x02;
    pub const ENCODED: u32 = 0x03;
    pub const ENCODED_ARRAY: u32 = 0x04;
    pub const FLOAT: u32 = 0x05;
    pub const FLOAT_ARRAY: u32 = 0x06;
    pub const VEC2F: u32 = 0x07;
    pub const VEC3F: u32 = 0x09;
    pub const VEC3F_ARRAY: u32 = 0x0a;
    pub const VEC3I: u32 = 0x11;
    pub const VEC3I_ARRAY: u32 = 0x12;
    pub const VEC4F: u32 = 0x17;
    pub const VEC4F_ARRAY: u32 = 0x18;
    /// vec7s in version 1, vec8s in 2+.
    pub const PLACEMENT: u32 = 0x19;
    pub const PLACEMENT_ARRAY: u32 = 0x1a;
    pub const INT32: u32 = 0x25;
    pub const INT32_ARRAY: u32 = 0x26;
    pub const UINT32: u32 = 0x27;
    pub const UINT32_ARRAY: u32 = 0x28;
    pub const UINT16: u32 = 0x2b;
    pub const UINT16_ARRAY: u32 = 0x2c;
    pub const UINT32_ALT: u32 = 0x2f;
    pub const INT64: u32 = 0x31;
    pub const INT64_ARRAY: u32 = 0x32;
    pub const UINT64: u32 = 0x33;
    pub const UINT64_ARRAY: u32 = 0x34;
    pub const BOOL: u32 = 0x35;
    pub const BOOL_ARRAY: u32 = 0x36;
    /// "Ordinal string": a u32 index into a per-field string table.
    pub const ORDINAL: u32 = 0x37;
    pub const ID: u32 = 0x39;
    pub const ID_ARRAY: u32 = 0x3a;
    pub const ID_ALT: u32 = 0x3b;
    pub const ID_ARRAY_ALT: u32 = 0x3c;
    pub const ID_ALT2: u32 = 0x3d;
}

/// For an array type, the element type it is an array of.
pub const fn array_of(type_id: u32) -> Option<u32> {
    use ty::{
        BOOL, BOOL_ARRAY, ENCODED, ENCODED_ARRAY, FLOAT, FLOAT_ARRAY, ID, ID_ARRAY, ID_ARRAY_ALT,
        INT32, INT32_ARRAY, INT64, INT64_ARRAY, PLACEMENT, PLACEMENT_ARRAY, STRING, STRING_ARRAY,
        UINT16, UINT16_ARRAY, UINT32, UINT32_ARRAY, UINT64, UINT64_ARRAY, VEC3F, VEC3F_ARRAY,
        VEC3I, VEC3I_ARRAY, VEC4F, VEC4F_ARRAY,
    };
    Some(match type_id {
        STRING_ARRAY => STRING,
        ENCODED_ARRAY => ENCODED,
        FLOAT_ARRAY => FLOAT,
        VEC3F_ARRAY => VEC3F,
        VEC3I_ARRAY => VEC3I,
        VEC4F_ARRAY => VEC4F,
        PLACEMENT_ARRAY => PLACEMENT,
        INT32_ARRAY => INT32,
        UINT32_ARRAY => UINT32,
        UINT16_ARRAY => UINT16,
        INT64_ARRAY => INT64,
        UINT64_ARRAY => UINT64,
        BOOL_ARRAY => BOOL,
        ID_ARRAY | ID_ARRAY_ALT => ID,
        _ => return None,
    })
}

/// A unit ID: either a dotted name or an opaque 64-bit "nameless" value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Id {
    Nameless(u64),
    Named(Vec<String>),
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Named(parts) if parts.is_empty() => f.write_str("null"),
            Self::Named(parts) => f.write_str(&parts.join(".")),
            Self::Nameless(0) => f.write_str("_nameless.0"),
            Self::Nameless(value) => {
                // Most significant non-zero 16-bit group unpadded, rest padded.
                let mut groups = Vec::new();
                let mut v = *value;
                while v != 0 {
                    groups.push((v & 0xffff) as u16);
                    v >>= 16;
                }
                write!(f, "_nameless.{:x}", groups[groups.len() - 1])?;
                for g in groups.iter().rev().skip(1) {
                    write!(f, ".{g:04x}")?;
                }
                Ok(())
            }
        }
    }
}

/// One field value. Arrays hold elements of a single type by construction.
#[derive(Debug, Clone, PartialEq)]
pub enum Value<'a> {
    /// Nominally UTF-8, but kept as bytes: the text writer escapes per byte,
    /// and real files are not guaranteed to be valid UTF-8.
    String(&'a [u8]),
    Encoded(String),
    Float(f32),
    Vec2([f32; 2]),
    Vec3([f32; 3]),
    Vec4([f32; 4]),
    /// Position then rotation, with the hidden bias component folded in and
    /// dropped, so versions 1 and 2/3 look the same here.
    Placement([f32; 7]),
    Vec3i([i32; 3]),
    Int32(i32),
    UInt32(u32),
    UInt16(u16),
    Int64(i64),
    UInt64(u64),
    Bool(bool),
    Ordinal(u32),
    Id(Id),
    Array(Vec<Self>),
}

impl Value<'_> {
    pub fn as_array(&self) -> Option<&[Value<'_>]> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }
}

/// One field definition inside a structure block.
#[derive(Debug, Clone)]
pub struct FieldProto {
    pub type_id: u32,
    pub name: String,
    /// Present only for [`ty::ORDINAL`].
    pub ordinals: Option<HashMap<u32, Vec<u8>>>,
}

/// A structure block: a named struct definition with an ID.
#[derive(Debug, Clone)]
pub struct Prototype {
    pub id: u32,
    pub name: String,
    pub fields: Vec<FieldProto>,
}

impl Prototype {
    pub fn field_index(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|f| f.name == name)
    }
}

/// One instance of a [`Prototype`].
#[derive(Debug, Clone)]
pub struct Block<'a> {
    pub prototype_id: u32,
    pub id: Id,
    pub values: Vec<Value<'a>>,
}

/// A parsed binary SII file.
#[derive(Debug, Clone)]
pub struct BsiiFile<'a> {
    pub version: u32,
    pub prototypes: HashMap<u32, Prototype>,
    pub blocks: Vec<Block<'a>>,
}

impl<'a> BsiiFile<'a> {
    pub fn prototype_of(&self, block: &Block<'a>) -> Option<&Prototype> {
        self.prototypes.get(&block.prototype_id)
    }

    /// Field definitions paired with this block's values.
    pub fn fields<'b>(
        &'b self,
        block: &'b Block<'a>,
    ) -> Option<impl Iterator<Item = (&'b FieldProto, &'b Value<'a>)>> {
        let proto = self.prototype_of(block)?;
        if proto.fields.len() != block.values.len() {
            return None;
        }
        Some(proto.fields.iter().zip(block.values.iter()))
    }

    /// Look up one value in a block by field name.
    pub fn field<'b>(&'b self, block: &'b Block<'a>, name: &str) -> Option<&'b Value<'a>> {
        block
            .values
            .get(self.prototype_of(block)?.field_index(name)?)
    }

    /// Blocks whose structure is called `name`.
    pub fn blocks_named<'b>(&'b self, name: &'b str) -> impl Iterator<Item = &'b Block<'a>> + 'b {
        self.blocks.iter().filter(move |b| {
            self.prototypes
                .get(&b.prototype_id)
                .is_some_and(|p| p.name == name)
        })
    }

    /// Serialise back to textual `SiiNunit`.
    pub fn to_text(&self) -> Result<String, Error> {
        text::write(self)
    }
}

/// Binary SII bytes to textual `SiiNunit`.
pub fn to_text(data: &[u8]) -> Result<String, Error> {
    parse(data)?.to_text()
}

pub fn is_bsii(data: &[u8]) -> bool {
    data.len() >= 8 && &data[..4] == MAGIC
}
