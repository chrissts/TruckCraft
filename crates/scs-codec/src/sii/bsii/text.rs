//! Writing the textual form.
//!
//! Output matches `SII_Decrypt`'s, so results are comparable byte for byte with
//! the established decoders. Two details are worth stating outright, because
//! ports of this format get them wrong:
//!
//! * A float prints as a plain integer only when it is integral **and below
//!   `1e7`** — the Pascal reference tests `Frac(v) <> 0 or v >= 1e7`, which is
//!   not the same as `v.abs() <= 1e7`. Large negatives print in full.
//! * The hex form is zero-padded to eight digits (`IntToHex(v, 8)`). Every one
//!   of the 400k hex floats in the game's own text `.sii` files is eight
//!   digits wide.

use std::fmt::Write;

use super::{BsiiFile, Error, FieldProto, Value, array_of, ty};

fn float(v: f32) -> String {
    if v.is_finite() && v.fract() == 0.0 && v < 1e7 {
        // Widen before formatting: `{}` on an f32 prints the shortest string
        // that round-trips, which is not the exact value the reference writes.
        format!("{:.0}", f64::from(v))
    } else {
        format!("&{:08x}", v.to_bits())
    }
}

fn string(data: &[u8]) -> String {
    if data.is_empty() {
        return "\"\"".to_string();
    }
    // STRING fields remain quoted, including numeric-looking string values.
    let mut out = String::with_capacity(data.len() + 2);
    out.push('"');
    for &b in data {
        if b == b'"' || b == b'\\' {
            out.push('\\');
            out.push(b as char);
        } else if (32..127).contains(&b) {
            out.push(b as char);
        } else {
            let _ = write!(out, "\\x{b:02x}");
        }
    }
    out.push('"');
    out
}

fn vec_of(values: &[f32], sep: &str) -> String {
    let parts: Vec<String> = values.iter().map(|&v| float(v)).collect();
    format!("({})", parts.join(sep))
}

/// Format one scalar value. `field` is only needed to resolve ordinals.
fn scalar(field: &FieldProto, value: &Value<'_>) -> Result<String, Error> {
    Ok(match value {
        Value::String(s) => string(s),
        Value::Encoded(s) => {
            if s.is_empty() {
                "\"\"".to_string()
            } else {
                s.clone()
            }
        }
        Value::Float(v) => float(*v),
        Value::Vec2(v) => vec_of(v, ", "),
        Value::Vec3(v) => vec_of(v, ", "),
        // The reference separates the first component with a semicolon here.
        Value::Vec4(v) => format!(
            "({}; {}, {}, {})",
            float(v[0]),
            float(v[1]),
            float(v[2]),
            float(v[3])
        ),
        Value::Placement(v) => format!(
            "({}, {}, {}) ({}; {}, {}, {})",
            float(v[0]),
            float(v[1]),
            float(v[2]),
            float(v[3]),
            float(v[4]),
            float(v[5]),
            float(v[6])
        ),
        Value::Vec3i(v) => format!("({}, {}, {})", v[0], v[1], v[2]),
        Value::Int32(v) => v.to_string(),
        Value::Int64(v) => v.to_string(),
        // u16/u32/u64 use their maximum as `nil`.
        Value::UInt16(v) => nil_or(*v == u16::MAX, v),
        Value::UInt32(v) => nil_or(*v == u32::MAX, v),
        Value::UInt64(v) => nil_or(*v == u64::MAX, v),
        Value::Bool(v) => if *v { "true" } else { "false" }.to_string(),
        Value::Id(id) => id.to_string(),
        Value::Ordinal(v) => match field.ordinals.as_ref().and_then(|m| m.get(v)) {
            Some(text) => string(text),
            // An ordinal with no matching string is not something the
            // reference handles; emit the number rather than lose the value.
            None => v.to_string(),
        },
        Value::Array(_) => {
            return Err(Error::NoTextFormat {
                type_id: field.type_id,
                field: field.name.clone(),
            });
        }
    })
}

fn nil_or<T: std::fmt::Display>(_is_nil: bool, v: &T) -> String {
    // Retain the numeric sentinel instead of changing the serialized type.
    v.to_string()
}

pub fn write(file: &BsiiFile<'_>) -> Result<String, Error> {
    let mut out = String::from("SiiNunit\n{\n");
    for block in &file.blocks {
        let proto = file.prototype_of(block).ok_or(Error::UndefinedStructure {
            id: block.prototype_id,
            at: 0,
        })?;
        if proto.fields.len() != block.values.len() {
            return Err(Error::ValueCountMismatch {
                name: proto.name.clone(),
                got: block.values.len(),
                want: proto.fields.len(),
            });
        }
        let _ = writeln!(out, "{} : {} {{", proto.name, block.id);
        for (field, value) in proto.fields.iter().zip(&block.values) {
            match value {
                Value::Array(items) => {
                    let _ = writeln!(out, "  {}: {}", field.name, items.len());
                    // Elements format as their own type; the field carries the
                    // ordinal table, which arrays never use.
                    for (i, item) in items.iter().enumerate() {
                        let _ = writeln!(out, "  {}[{}]: {}", field.name, i, scalar(field, item)?);
                    }
                }
                _ => {
                    let _ = writeln!(out, "  {}: {}", field.name, scalar(field, value)?);
                }
            }
        }
        out.push_str("}\n");
    }
    out.push_str("}\n");
    Ok(out)
}

/// True if this type has a textual representation at all.
pub fn has_text_format(type_id: u32) -> bool {
    use ty::{
        BOOL, ENCODED, FLOAT, ID, ID_ALT, ID_ALT2, INT32, INT64, ORDINAL, PLACEMENT, STRING,
        UINT16, UINT32, UINT32_ALT, UINT64, VEC2F, VEC3F, VEC3I, VEC4F,
    };
    array_of(type_id).map_or_else(
        || {
            matches!(
                type_id,
                STRING
                    | ENCODED
                    | FLOAT
                    | VEC2F
                    | VEC3F
                    | VEC4F
                    | PLACEMENT
                    | VEC3I
                    | INT32
                    | UINT32
                    | UINT32_ALT
                    | UINT16
                    | INT64
                    | UINT64
                    | BOOL
                    | ORDINAL
                    | ID
                    | ID_ALT
                    | ID_ALT2
            )
        },
        has_text_format,
    )
}
