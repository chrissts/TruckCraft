//! Structured plate text and SCS formatting. Unknown tags and attributes survive
//! edits verbatim; the desktop never needs to expose markup as an input field.
use crate::{Error, Result};
use std::ops::Range;

const MAX_PLATE: usize = 4096;
pub const MAX_PARTS: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tag {
    raw: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Part {
    Text(String),
    Tag(Tag),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plate {
    pub parts: Vec<Part>,
    pub country: Option<String>,
}

fn invalid(message: &str) -> Error {
    Error::InvalidEdit(message.into())
}

impl Tag {
    pub fn name(&self) -> &str {
        self.raw[1..self.raw.len() - 1]
            .split_whitespace()
            .next()
            .unwrap_or("")
    }
    fn attribute(&self, wanted: &str) -> Option<Range<usize>> {
        let bytes = self.raw.as_bytes();
        let mut at = 1;
        while at < bytes.len() && !bytes[at].is_ascii_whitespace() && bytes[at] != b'>' {
            at += 1;
        }
        while at + 1 < bytes.len() {
            while at < bytes.len() && bytes[at].is_ascii_whitespace() {
                at += 1;
            }
            let key_start = at;
            while at < bytes.len()
                && (bytes[at].is_ascii_alphanumeric() || matches!(bytes[at], b'_' | b'-'))
            {
                at += 1;
            }
            let key_end = at;
            while at < bytes.len() && bytes[at].is_ascii_whitespace() {
                at += 1;
            }
            if key_start == key_end || bytes.get(at) != Some(&b'=') {
                return None;
            }
            at += 1;
            while at < bytes.len() && bytes[at].is_ascii_whitespace() {
                at += 1;
            }
            let quoted = bytes
                .get(at)
                .copied()
                .filter(|byte| matches!(byte, b'\'' | b'"'));
            if quoted.is_some() {
                at += 1;
            }
            let start = at;
            while at < bytes.len()
                && if let Some(quote) = quoted {
                    bytes[at] != quote
                } else {
                    !bytes[at].is_ascii_whitespace() && bytes[at] != b'>'
                }
            {
                at += 1;
            }
            let end = at;
            if &self.raw[key_start..key_end] == wanted {
                return Some(start..end);
            }
            if quoted.is_some() {
                at += 1;
            }
        }
        None
    }
    pub fn get(&self, name: &str) -> Option<&str> {
        self.attribute(name).map(|range| &self.raw[range])
    }
    pub fn number(&self, name: &str, default: f32) -> f32 {
        self.get(name)
            .and_then(|raw| raw.parse::<f32>().ok())
            .filter(|value| value.is_finite())
            .unwrap_or(default)
    }
    pub fn color(&self) -> Option<[u8; 4]> {
        let raw = self.get("value")?;
        if raw.len() != 8 {
            return None;
        }
        u32::from_str_radix(raw, 16).ok().map(u32::to_le_bytes)
    }
    pub fn set(&mut self, name: &str, value: &str) -> Result<()> {
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            || value.is_empty()
            || value.chars().any(char::is_whitespace)
            || value
                .chars()
                .any(|ch| ch.is_control() || matches!(ch, '<' | '>' | '\'' | '"' | '|'))
        {
            return Err(invalid("Invalid plate formatting value."));
        }
        match name {
            "xscale" | "yscale" | "width" | "height" | "hshift" | "vshift" | "left" | "right" => {
                let number = value
                    .parse::<f32>()
                    .map_err(|_| invalid("Enter a valid plate size or position."))?;
                let positive = matches!(name, "xscale" | "yscale" | "width" | "height");
                if !number.is_finite() || number.abs() > 4096.0 || (positive && number <= 0.0) {
                    return Err(invalid(
                        "Plate sizes must be positive and positions must be finite values within range.",
                    ));
                }
            }
            "value" if self.name() == "color" => {
                if value.len() != 8 || u32::from_str_radix(value, 16).is_err() {
                    return Err(invalid("Choose a valid plate colour."));
                }
            }
            "src"
                if !value.starts_with('/')
                    || !value.ends_with(".mat")
                    || value.contains(['\\', ':'])
                    || value.split('/').any(|part| matches!(part, "." | "..")) =>
            {
                return Err(invalid("Choose a plate image from the game artwork list."));
            }
            _ => {}
        }
        let mut raw = self.raw.clone();
        if let Some(range) = self.attribute(name) {
            raw.replace_range(range, value);
        } else {
            raw.insert_str(raw.len() - 1, &format!(" {name}={value}"));
        }
        if raw.len() > MAX_PLATE {
            return Err(invalid("Plate formatting is too long."));
        }
        self.raw = raw;
        Ok(())
    }
    pub fn create(name: &str) -> Result<Self> {
        if !matches!(
            name,
            "color"
                | "font"
                | "/font"
                | "offset"
                | "img"
                | "align"
                | "/align"
                | "sup"
                | "/sup"
                | "sub"
                | "/sub"
                | "ret"
        ) {
            return Err(invalid("Unsupported new plate formatting control."));
        }
        Ok(Self {
            raw: format!("<{name}>"),
        })
    }
}

impl Plate {
    pub fn parse(raw: &str) -> Result<Self> {
        if raw.len() > MAX_PLATE || raw.chars().any(char::is_control) {
            return Err(invalid(
                "This plate contains unsupported text or exceeds its size limit.",
            ));
        }
        let (body, country) = raw.rsplit_once('|').map_or((raw, None), |(body, country)| {
            (body, Some(country.to_owned()))
        });
        let mut parts = Vec::new();
        let mut at = 0;
        while at < body.len() {
            if parts.len() >= MAX_PARTS {
                return Err(invalid("This plate has too many formatting sections."));
            }
            if body.as_bytes()[at] == b'<' {
                let mut end = at + 1;
                let mut quote = None;
                while end < body.len() {
                    let byte = body.as_bytes()[end];
                    if let Some(current) = quote {
                        if byte == current {
                            quote = None;
                        }
                    } else if matches!(byte, b'\'' | b'"') {
                        quote = Some(byte);
                    } else if byte == b'>' {
                        break;
                    }
                    end += 1;
                }
                if end == body.len() {
                    return Err(invalid(
                        "The saved plate has incomplete formatting. Resave it in-game before editing.",
                    ));
                }
                parts.push(Part::Tag(Tag {
                    raw: body[at..=end].into(),
                }));
                at = end + 1;
            } else {
                let end = body[at..]
                    .find('<')
                    .map_or(body.len(), |offset| at + offset);
                parts.push(Part::Text(body[at..end].into()));
                at = end;
            }
        }
        if parts.is_empty() {
            parts.push(Part::Text(String::new()));
        }
        Ok(Self { parts, country })
    }
    pub fn text(&self) -> String {
        self.parts
            .iter()
            .filter_map(|part| {
                if let Part::Text(text) = part {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect()
    }
    pub fn first_color(&self) -> [u8; 4] {
        self.parts
            .iter()
            .find_map(|part| {
                if let Part::Tag(tag) = part
                    && tag.name() == "color"
                {
                    tag.color()
                } else {
                    None
                }
            })
            .unwrap_or([0, 0, 0, 255])
    }
    pub fn set_text(&mut self, text: &str) -> Result<()> {
        validate_text(text)?;
        if self
            .parts
            .iter()
            .filter(|part| matches!(part, Part::Text(_)))
            .count()
            > 1
        {
            return Err(invalid(
                "Use Layout and badges to edit this plate's separate text sections.",
            ));
        }
        if let Some(Part::Text(current)) = self
            .parts
            .iter_mut()
            .find(|part| matches!(part, Part::Text(_)))
        {
            *current = text.into();
        } else {
            self.parts.push(Part::Text(text.into()));
        }
        Ok(())
    }
    pub fn set_color(&mut self, color: [u8; 4]) -> Result<()> {
        let value = format!("{:08X}", u32::from_le_bytes(color));
        let mut changed = false;
        for part in &mut self.parts {
            if let Part::Tag(tag) = part
                && tag.name() == "color"
            {
                tag.set("value", &value)?;
                changed = true;
            }
        }
        if !changed {
            let mut tag = Tag::create("color")?;
            tag.set("value", &value)?;
            self.parts.insert(0, Part::Tag(tag));
        }
        Ok(())
    }
    pub fn render(&self) -> Result<String> {
        if self.parts.len() > MAX_PARTS {
            return Err(invalid("A plate can contain at most 128 sections."));
        }
        let mut raw = String::new();
        for part in &self.parts {
            match part {
                Part::Text(text) => {
                    validate_text(text)?;
                    raw.push_str(text);
                }
                Part::Tag(tag) => raw.push_str(&tag.raw),
            }
        }
        if let Some(country) = &self.country {
            if country
                .chars()
                .any(|ch| ch.is_control() || matches!(ch, '|' | '<' | '>'))
            {
                return Err(invalid("Invalid plate country or state."));
            }
            raw.push('|');
            raw.push_str(country);
        }
        if raw.len() > MAX_PLATE {
            return Err(invalid("The plate exceeds its size limit."));
        }
        Ok(raw)
    }
}

pub fn validate_text(text: &str) -> Result<()> {
    if text.chars().count() > 128
        || text
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '<' | '>' | '|'))
    {
        return Err(invalid(
            "Enter plate text without formatting symbols, using at most 128 characters.",
        ));
    }
    Ok(())
}
