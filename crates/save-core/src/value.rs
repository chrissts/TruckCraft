//! Typed SII values. Reading never rewrites the stored representation.
use crate::{Error, Result};

/// Byte offset of a line comment outside a quoted value.
pub(crate) fn comment_start(line: &str) -> usize {
    let mut quoted = false;
    let mut escaped = false;
    let bytes = line.as_bytes();
    for (index, byte) in bytes.iter().copied().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        if quoted && byte == b'\\' {
            escaped = true;
            continue;
        }
        if byte == b'"' {
            quoted = !quoted;
        }
        if !quoted && (byte == b'#' || (byte == b'/' && bytes.get(index + 1) == Some(&b'/'))) {
            return index;
        }
    }
    line.len()
}

pub fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for byte in text.bytes() {
        match byte {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            32..=126 => out.push(char::from(byte)),
            _ => {
                use std::fmt::Write;
                let _ = write!(out, "\\x{byte:02x}");
            }
        }
    }
    out.push('"');
    out
}

pub fn unquote(raw: &str) -> Result<String> {
    let raw = raw.trim();
    if !raw.starts_with('"') {
        return Ok(raw.to_owned());
    }
    let inner = raw
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .ok_or(Error::InvalidSii("unterminated string"))?;
    let mut output = Vec::with_capacity(inner.len());
    let mut bytes = inner.bytes();
    while let Some(byte) = bytes.next() {
        if byte != b'\\' {
            output.push(byte);
            continue;
        }
        match bytes.next() {
            Some(b'x') => {
                let a = bytes.next().and_then(|b| char::from(b).to_digit(16));
                let b = bytes.next().and_then(|b| char::from(b).to_digit(16));
                match (a, b) {
                    (Some(a), Some(b)) => output.push((a * 16 + b) as u8),
                    _ => return Err(Error::InvalidSii("invalid hex escape")),
                }
            }
            Some(b'"') => output.push(b'"'),
            Some(b'\\') => output.push(b'\\'),
            Some(other) => {
                output.push(b'\\');
                output.push(other);
            }
            None => return Err(Error::InvalidSii("incomplete string escape")),
        }
    }
    String::from_utf8(output).map_err(|_| Error::InvalidSii("invalid UTF-8 string value"))
}

pub fn float(raw: &str) -> Result<f32> {
    let raw = raw.trim();
    let value = if let Some(hex) = raw.strip_prefix('&') {
        if hex.len() != 8 {
            return Err(Error::InvalidSii("float must have eight hex digits"));
        }
        f32::from_bits(
            u32::from_str_radix(hex, 16).map_err(|_| Error::InvalidSii("invalid float bits"))?,
        )
    } else {
        raw.parse::<f32>()
            .map_err(|_| Error::InvalidSii("invalid float"))?
    };
    if !value.is_finite() {
        return Err(Error::InvalidSii("nonfinite float"));
    }
    Ok(value)
}

pub fn float_text(value: f32) -> Result<String> {
    if !value.is_finite() {
        return Err(Error::InvalidEdit("Use a finite number.".into()));
    }
    Ok(format!("&{:08x}", value.to_bits()))
}

pub fn is_null(raw: &str) -> bool {
    matches!(raw.trim(), "null" | "nil" | "\"\"" | "")
}

pub(crate) fn validate(raw: &str) -> Result<()> {
    if raw.trim().is_empty() || raw.len() > 1024 * 1024 {
        return Err(Error::InvalidEdit("A value is empty or too large.".into()));
    }
    let mut quoted = false;
    let mut escaped = false;
    let mut previous = '\0';
    for character in raw.chars() {
        if character.is_control() {
            return Err(Error::InvalidEdit(
                "Values cannot contain control characters.".into(),
            ));
        }
        if escaped {
            escaped = false;
            previous = '\0';
            continue;
        }
        if quoted && character == '\\' {
            escaped = true;
            continue;
        }
        if character == '"' {
            quoted = !quoted;
        }
        if !quoted
            && (matches!(character, '{' | '}' | '#') || (previous == '/' && character == '/'))
        {
            return Err(Error::InvalidEdit(
                "Values cannot introduce units or comments.".into(),
            ));
        }
        previous = character;
    }
    if quoted || escaped {
        return Err(Error::InvalidEdit("The quoted value is incomplete.".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn string_roundtrip(text in ".{0,200}") {
            let encoded = quote(&text);
            prop_assert!(validate(&encoded).is_ok());
            prop_assert_eq!(unquote(&encoded).unwrap(),text);
        }
    }
    #[test]
    fn floats_preserve_bits() {
        for number in [0.0, -0.0, 0.5, -123.75, f32::MIN_POSITIVE, f32::MAX] {
            assert_eq!(
                float(&float_text(number).unwrap()).unwrap().to_bits(),
                number.to_bits()
            );
        }
        assert!(float_text(f32::NAN).is_err());
        assert!(float("&7f800000").is_err());
    }
    #[test]
    fn rejects_sii_injection_but_allows_quoted_text() {
        for text in ["1\n x: 2", "0 }", "value // hidden", "\"unterminated"] {
            assert!(validate(text).is_err());
        }
        assert!(validate(&quote("a } { // # \" \\ text")).is_ok());
    }
}
