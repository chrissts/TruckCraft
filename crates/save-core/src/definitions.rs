//! Read-only SCS definitions. This grammar is deliberately separate from saves:
//! definitions have include fragments, implicit arrays and multiline strings.
use crate::{Error, Result, value};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_FILE: usize = 8 * 1024 * 1024;
const MAX_TOTAL: usize = 64 * 1024 * 1024;
const MAX_FILES: usize = 4096;
const MAX_DEPTH: usize = 32;
const MAX_RECORD: usize = 128 * 1024;
const MAX_UNITS: usize = 100_000;
const MAX_FIELDS: usize = 1_000_000;
const MAX_ARRAY: usize = 65_536;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Field {
    pub scalar: Option<String>,
    pub elements: BTreeMap<usize, String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unit {
    pub kind: String,
    pub id: String,
    pub fields: BTreeMap<String, Field>,
}

impl Unit {
    pub fn raw(&self, field: &str) -> Result<&str> {
        self.fields
            .get(field)
            .and_then(|field| field.scalar.as_deref())
            .ok_or_else(|| Error::Missing(format!("definition {}.{field}", self.id)))
    }
    pub fn integer<T: std::str::FromStr>(&self, field: &str) -> Result<T> {
        self.raw(field)?
            .parse()
            .map_err(|_| invalid("invalid definition integer"))
    }
    pub fn array(&self, field: &str) -> Result<Vec<&str>> {
        let field = self
            .fields
            .get(field)
            .ok_or_else(|| Error::Missing("definition array".into()))?;
        if let Some(count) = &field.scalar {
            let count: usize = count
                .parse()
                .map_err(|_| invalid("definition array count is not an integer"))?;
            if count != field.elements.len() {
                return Err(invalid("definition array count disagrees"));
            }
        }
        (0..field.elements.len())
            .map(|index| {
                field
                    .elements
                    .get(&index)
                    .map(String::as_str)
                    .ok_or_else(|| invalid("definition array has a missing index"))
            })
            .collect()
    }
}

#[derive(Clone, Debug, Default)]
pub struct Catalog {
    pub units: BTreeMap<String, Unit>,
    pub files: BTreeSet<String>,
}

fn invalid(message: &str) -> Error {
    Error::InvalidDefinition(message.into())
}

/// Resolve only virtual, slash-separated paths; never interpret an include as an
/// OS path, URL, drive, UNC share or command. Parent traversal stays inside root.
pub fn virtual_path(base: &str, requested: &str) -> Result<String> {
    if requested.is_empty()
        || requested.starts_with("//")
        || requested.len() > 1024
        || requested
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '\\' | ':' | '"'))
    {
        return Err(invalid("invalid definition include path"));
    }
    let mut parts: Vec<&str> = if requested.starts_with('/') {
        Vec::new()
    } else {
        base.rsplit_once('/')
            .map_or(Vec::new(), |(parent, _)| parent.split('/').collect())
    };
    for part in requested.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(invalid("definition include escapes its virtual root"));
                }
            }
            part => parts.push(part),
        }
    }
    if parts.is_empty() {
        return Err(invalid("definition include has no file name"));
    }
    let path = parts.join("/");
    if path.len() > 1024 {
        return Err(invalid("definition include path exceeds its limit"));
    }
    Ok(path)
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Frame {
    Document,
    Unit(String),
}
#[derive(Debug)]
enum Pending {
    Document,
    Unit(String, String),
}
#[derive(Debug)]
enum Record {
    Text(String),
    Open,
    Close,
}

fn token(raw: &str) -> bool {
    !raw.is_empty()
        && raw.len() <= 512
        && raw
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

// Stream records rather than allocating a second expanded copy of the tree.
fn records(text: &str, mut emit: impl FnMut(Record, usize) -> Result<()>) -> Result<()> {
    let bytes = text.as_bytes();
    let mut at = 0;
    let mut start = 0;
    let mut line = 1;
    let mut record_line = 1;
    let mut buffer = String::new();
    let mut quoted = false;
    let mut escaped = false;
    let mut block = false;
    while at < bytes.len() {
        let byte = bytes[at];
        if block {
            if byte == b'*' && bytes.get(at + 1) == Some(&b'/') {
                block = false;
                at += 2;
                start = at;
                continue;
            }
            if matches!(byte, b'\n' | b'\r') {
                if !buffer.trim().is_empty() {
                    emit(Record::Text(buffer.trim().to_owned()), record_line)?;
                }
                buffer.clear();
                if byte == b'\n' || bytes.get(at + 1) != Some(&b'\n') {
                    line += 1;
                }
                record_line = line;
            }
            at += 1;
            start = at;
            continue;
        }
        if quoted {
            if byte == b'\n' {
                line += 1;
            }
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
            at += 1;
        } else if byte == b'"' {
            quoted = true;
            at += 1;
        } else if byte == b'#' || (byte == b'/' && bytes.get(at + 1) == Some(&b'/')) {
            buffer.push_str(&text[start..at]);
            while at < bytes.len() && !matches!(bytes[at], b'\r' | b'\n') {
                at += 1;
            }
            start = at;
        } else if byte == b'/' && bytes.get(at + 1) == Some(&b'*') {
            buffer.push_str(&text[start..at]);
            buffer.push(' ');
            block = true;
            at += 2;
            start = at;
        } else if matches!(byte, b'\n' | b'\r' | b'{' | b'}') {
            buffer.push_str(&text[start..at]);
            if !buffer.trim().is_empty() {
                emit(Record::Text(buffer.trim().to_owned()), record_line)?;
            }
            buffer.clear();
            if byte == b'{' {
                emit(Record::Open, line)?;
            } else if byte == b'}' {
                emit(Record::Close, line)?;
            }
            if byte == b'\n' || (byte == b'\r' && bytes.get(at + 1) != Some(&b'\n')) {
                line += 1;
            }
            at += 1;
            start = at;
            record_line = line;
        } else {
            at += 1;
        }
        if buffer.len() + at - start > MAX_RECORD {
            return Err(invalid("definition record exceeds its size limit"));
        }
    }
    if quoted || block {
        return Err(invalid("unterminated definition string or comment"));
    }
    buffer.push_str(&text[start..]);
    if !buffer.trim().is_empty() {
        emit(Record::Text(buffer.trim().to_owned()), record_line)?;
    }
    Ok(())
}

struct Parser<'a, F> {
    read: &'a mut F,
    catalog: Catalog,
    frames: Vec<Frame>,
    pending: Option<Pending>,
    active: BTreeSet<String>,
    bytes: usize,
    reads: usize,
    fields: usize,
}

impl<F: FnMut(&str, usize) -> Result<Vec<u8>>> Parser<'_, F> {
    fn file(&mut self, path: &str) -> Result<()> {
        if self.active.len() >= MAX_DEPTH
            || self.reads >= MAX_FILES
            || !self.active.insert(path.into())
        {
            return Err(invalid("definition include cycle or traversal limit"));
        }
        self.reads += 1;
        let limit = MAX_FILE.min(MAX_TOTAL.saturating_sub(self.bytes));
        let bytes = (self.read)(path, limit)?;
        if bytes.len() > limit {
            return Err(invalid("definition input exceeds its size limit"));
        }
        self.bytes += bytes.len();
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| invalid("definition is not UTF-8"))?
            .trim_start_matches('\u{feff}');
        let frames = self.frames.clone();
        records(text, |record, line| {
            self.record(path, record)
                .map_err(|error| Error::InvalidDefinition(format!("{path}:{line}: {error}")))
        })?;
        if self.frames != frames || self.pending.is_some() {
            return Err(invalid("unbalanced definition include or unit"));
        }
        self.catalog.files.insert(path.into());
        self.active.remove(path);
        Ok(())
    }
    fn record(&mut self, path: &str, record: Record) -> Result<()> {
        match record {
            Record::Open => {
                if self.frames.len() >= MAX_DEPTH {
                    return Err(invalid("definition nesting limit"));
                }
                match self
                    .pending
                    .take()
                    .ok_or_else(|| invalid("unexpected definition opening brace"))?
                {
                    Pending::Document => self.frames.push(Frame::Document),
                    Pending::Unit(kind, id) => {
                        if self.catalog.units.len() >= MAX_UNITS
                            || self.catalog.units.contains_key(&id)
                        {
                            return Err(invalid("duplicate definition unit or unit limit"));
                        }
                        self.catalog.units.insert(
                            id.clone(),
                            Unit {
                                kind,
                                id: id.clone(),
                                fields: BTreeMap::new(),
                            },
                        );
                        self.frames.push(Frame::Unit(id));
                    }
                }
            }
            Record::Close => {
                if self.pending.is_some() || self.frames.pop().is_none() {
                    return Err(invalid("unexpected definition closing brace"));
                }
            }
            Record::Text(text) => {
                if self.pending.is_some() {
                    return Err(invalid("expected definition opening brace"));
                }
                if let Some(include) = text.strip_prefix("@include") {
                    if !include.starts_with(char::is_whitespace) {
                        return Err(invalid("invalid include directive"));
                    }
                    let include = include.trim();
                    if !include.starts_with('"') || !include.ends_with('"') {
                        return Err(invalid("include file name must be quoted"));
                    }
                    let include = value::unquote(include)?;
                    return self.file(&virtual_path(path, &include)?);
                }
                if text == "SiiNunit" {
                    if !self.frames.is_empty() {
                        return Err(invalid("nested SiiNunit document"));
                    }
                    self.pending = Some(Pending::Document);
                } else if let Some(Frame::Unit(id)) = self.frames.last() {
                    self.fields += 1;
                    if self.fields > MAX_FIELDS {
                        return Err(invalid("definition field limit"));
                    }
                    let (key, raw) = text
                        .split_once(':')
                        .ok_or_else(|| invalid("definition field has no value separator"))?;
                    let (key, raw) = (key.trim(), raw.trim());
                    if raw.is_empty() {
                        return Err(invalid("definition field is empty"));
                    }
                    let (name, index) = if let Some((name, tail)) = key.split_once('[') {
                        (
                            name,
                            Some(
                                tail.strip_suffix(']')
                                    .ok_or_else(|| invalid("invalid definition array key"))?,
                            ),
                        )
                    } else {
                        (key, None)
                    };
                    if !token(name) {
                        return Err(invalid("invalid definition field name"));
                    }
                    let field = self
                        .catalog
                        .units
                        .get_mut(id)
                        .expect("active unit exists")
                        .fields
                        .entry(name.into())
                        .or_default();
                    if let Some(index) = index {
                        let index = if index.is_empty() {
                            field
                                .elements
                                .last_key_value()
                                .map_or(0, |(last, _)| last + 1)
                        } else {
                            index
                                .parse::<usize>()
                                .map_err(|_| invalid("invalid definition array index"))?
                        };
                        if index >= MAX_ARRAY {
                            return Err(invalid("definition array limit"));
                        }
                        if field.scalar.as_deref() == Some("0") {
                            field.scalar = None;
                        }
                        field.elements.insert(index, raw.into());
                    } else {
                        // Definition fragments may override defaults. A zero
                        // count resets an inherited array before appending.
                        field.scalar = Some(raw.into());
                        if raw == "0" {
                            field.elements.clear();
                        }
                    }
                } else {
                    let (kind, id) = text
                        .split_once(':')
                        .ok_or_else(|| invalid("invalid definition unit header"))?;
                    let (kind, id) = (kind.trim(), id.trim());
                    if !token(kind) || !token(id) {
                        return Err(invalid("invalid definition unit type or identifier"));
                    }
                    self.pending = Some(Pending::Unit(kind.into(), id.into()));
                }
            }
        }
        Ok(())
    }
}

impl Catalog {
    pub fn from_archive(archive: &scs_codec::Archive, paths: &[&str]) -> Result<Self> {
        Self::load(paths, &mut |path, limit| {
            archive.read_path_limited(path, limit).map_err(|error| {
                Error::InvalidDefinition(format!("could not read {path}: {error}"))
            })
        })
    }
    /// The reader must honor `limit` before allocating; all paths are virtual.
    /// A failure discards the entire load rather than publishing a partial tree.
    pub fn load(
        paths: &[&str],
        read: &mut impl FnMut(&str, usize) -> Result<Vec<u8>>,
    ) -> Result<Self> {
        let mut parser = Parser {
            read,
            catalog: Self::default(),
            frames: Vec::new(),
            pending: None,
            active: BTreeSet::new(),
            bytes: 0,
            reads: 0,
            fields: 0,
        };
        for path in paths {
            parser.file(&virtual_path("", path)?)?;
        }
        Ok(parser.catalog)
    }
    pub fn unit(&self, id: &str) -> Result<&Unit> {
        self.units
            .get(id)
            .ok_or_else(|| Error::Missing(format!("definition {id}")))
    }
}
