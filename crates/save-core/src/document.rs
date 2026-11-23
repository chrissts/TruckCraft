//! Lossless SII document and copy-on-write edits.
use crate::{Error, Result, value};
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet, HashMap},
    ops::Range,
    sync::Arc,
};

const MAX_TEXT: usize = 256 * 1024 * 1024;
const MAX_ARRAY: usize = 1_000_000;
type Symbol = usize;
type GroupKey = (usize, Symbol);

#[derive(Clone, Debug)]
struct FieldLine {
    span: Range<usize>,
    value: Range<usize>,
    key_start: usize,
}
#[derive(Debug, Default)]
struct Group {
    root: Option<FieldLine>,
    elements: BTreeMap<usize, FieldLine>,
}
#[derive(Debug)]
pub struct Unit {
    pub id: Arc<str>,
    pub kind: Arc<str>,
    span: Range<usize>,
    fields: HashMap<Symbol, Group>,
}
#[derive(Debug)]
struct Index {
    source: Arc<str>,
    units: Vec<Unit>,
    by_id: HashMap<Arc<str>, usize>,
    by_kind: HashMap<Arc<str>, Vec<usize>>,
    symbols: HashMap<Arc<str>, Symbol>,
    names: Vec<Arc<str>>,
    closing: usize,
    newline: &'static str,
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum Edit {
    Members(BTreeMap<Option<usize>, String>),
    Array { count: String, values: Vec<String> },
}
pub enum FieldChange<'a> {
    Value {
        unit: &'a str,
        field: &'a str,
        item: Option<usize>,
        before: &'a str,
        after: &'a str,
    },
    Array {
        unit: &'a str,
        field: &'a str,
        count: usize,
    },
    Removed(&'a str),
    Structure,
}

#[derive(Clone, Debug)]
pub struct Document {
    index: Arc<Index>,
    edits: BTreeMap<GroupKey, Edit>,
    removed: BTreeSet<usize>,
    structurally_changed: bool,
}
#[derive(Clone, Debug, Default)]
pub struct UnitSpec {
    pub kind: String,
    pub id: String,
    pub scalars: BTreeMap<String, String>,
    pub arrays: BTreeMap<String, Vec<String>>,
}

fn token(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 512
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.'))
}
fn field_key(key: &str) -> Option<(&str, Option<usize>)> {
    let (root, item) = if let Some((root, index)) = key.split_once('[') {
        (root, Some(index.strip_suffix(']')?.parse::<usize>().ok()?))
    } else {
        (key, None)
    };
    if root.is_empty() || !root.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return None;
    }
    Some((root, item))
}

impl Document {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let text =
            std::str::from_utf8(bytes).map_err(|_| Error::InvalidSii("document is not UTF-8"))?;
        Self::parse(Arc::<str>::from(text))
    }

    pub fn parse(text: impl Into<Arc<str>>) -> Result<Self> {
        let source = text.into();
        if source.len() > MAX_TEXT {
            return Err(Error::InvalidSii("document exceeds the size limit"));
        }
        let newline = if source.contains("\r\n") {
            "\r\n"
        } else if source.contains('\r') {
            "\r"
        } else {
            "\n"
        };
        let mut index = Index {
            source: source.clone(),
            units: Vec::new(),
            by_id: HashMap::new(),
            by_kind: HashMap::new(),
            symbols: HashMap::new(),
            names: Vec::new(),
            closing: 0,
            newline,
        };
        let mut position = 0;
        let mut header = false;
        let mut opened = false;
        let mut closed = false;
        let mut current = None;
        while position < source.len() {
            let start = position;
            let remaining = &source.as_bytes()[position..];
            let body_end = match memchr::memchr2(b'\r', b'\n', remaining) {
                Some(offset) => position + offset,
                None => source.len(),
            };
            position = body_end;
            if source.as_bytes().get(position) == Some(&b'\r') {
                position += 1;
            }
            if source.as_bytes().get(position) == Some(&b'\n') {
                position += 1;
            }
            let original_body = &source[start..body_end];
            let body = &original_body[..value::comment_start(original_body)];
            let trimmed = body.trim().trim_start_matches('\u{feff}');
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
                continue;
            }
            if !header {
                if trimmed != "SiiNunit" {
                    return Err(Error::InvalidSii("missing SiiNunit header"));
                }
                header = true;
                continue;
            }
            if !opened {
                if trimmed != "{" {
                    return Err(Error::InvalidSii("missing document opening brace"));
                }
                opened = true;
                continue;
            }
            if closed {
                return Err(Error::InvalidSii("data after the document closing brace"));
            }
            if let Some(unit_index) = current {
                let unit: &mut Unit = &mut index.units[unit_index];
                if trimmed == "}" {
                    unit.span.end = position;
                    current = None;
                    continue;
                }
                // Unknown non-field lines are retained verbatim. Only indexed
                // fields can be edited; nothing is silently reconstructed.
                let Some(colon) = body.find(':') else {
                    continue;
                };
                let key = body[..colon].trim();
                let Some((root, item)) = field_key(key) else {
                    continue;
                };
                let symbol = if let Some(symbol) = index.symbols.get(root) {
                    *symbol
                } else {
                    let symbol = index.names.len();
                    let name: Arc<str> = Arc::from(root);
                    index.names.push(name.clone());
                    index.symbols.insert(name, symbol);
                    symbol
                };
                let after = &body[colon + 1..];
                let value_start = start + colon + 1 + (after.len() - after.trim_start().len());
                let value_end = start + body.trim_end().len();
                if value_start >= value_end {
                    return Err(Error::InvalidSii("empty field value"));
                }
                let line = FieldLine {
                    span: start..position,
                    value: value_start..value_end,
                    key_start: start + body.len() - body.trim_start().len(),
                };
                let group = unit.fields.entry(symbol).or_default();
                if let Some(item) = item {
                    if group.elements.insert(item, line).is_some() {
                        return Err(Error::InvalidSii("duplicate array index"));
                    }
                } else if group.root.replace(line).is_some() {
                    return Err(Error::InvalidSii("duplicate field"));
                }
            } else if trimmed == "}" {
                index.closing = start;
                closed = true;
            } else {
                let declaration = trimmed
                    .strip_suffix('{')
                    .ok_or(Error::InvalidSii("invalid unit declaration"))?
                    .trim();
                let (kind, id) = declaration
                    .split_once(':')
                    .ok_or(Error::InvalidSii("unit declaration has no identifier"))?;
                let (kind, id) = (kind.trim(), id.trim());
                if !token(kind) || !token(id) {
                    return Err(Error::InvalidSii("invalid unit type or identifier"));
                }
                if index.by_id.contains_key(id) {
                    return Err(Error::InvalidSii("duplicate unit identifier"));
                }
                let number = index.units.len();
                let id: Arc<str> = Arc::from(id);
                let kind: Arc<str> = Arc::from(kind);
                index.by_id.insert(id.clone(), number);
                index.by_kind.entry(kind.clone()).or_default().push(number);
                index.units.push(Unit {
                    id,
                    kind,
                    span: start..0,
                    fields: HashMap::new(),
                });
                current = Some(number);
            }
        }
        if !closed || current.is_some() {
            return Err(Error::InvalidSii("document or unit is not closed"));
        }
        Ok(Self {
            index: Arc::new(index),
            edits: BTreeMap::new(),
            removed: BTreeSet::new(),
            structurally_changed: false,
        })
    }

    pub fn source_len(&self) -> usize {
        self.index.source.len()
    }
    pub fn unit_count(&self) -> usize {
        self.index.units.len() - self.removed.len()
    }
    pub fn field_count(&self) -> usize {
        self.units().map(|unit| unit.fields.len()).sum()
    }
    pub fn is_dirty(&self) -> bool {
        self.structurally_changed || !self.edits.is_empty() || !self.removed.is_empty()
    }
    pub fn changes(&self) -> impl Iterator<Item = FieldChange<'_>> {
        let fields = self.edits.iter().flat_map(move |(&(unit, symbol), edit)| {
            let unit = &self.index.units[unit];
            let field = &*self.index.names[symbol];
            let group = &unit.fields[&symbol];
            let (members, array) = match edit {
                Edit::Members(members) => (Some(members), None),
                Edit::Array { values, .. } => (
                    None,
                    Some(FieldChange::Array {
                        unit: &unit.id,
                        field,
                        count: values.len(),
                    }),
                ),
            };
            array
                .into_iter()
                .chain(members.into_iter().flatten().map(move |(&item, after)| {
                    let line = match item {
                        Some(index) => &group.elements[&index],
                        None => group.root.as_ref().expect("edited scalar has an original"),
                    };
                    FieldChange::Value {
                        unit: &unit.id,
                        field,
                        item,
                        before: &self.index.source[line.value.clone()],
                        after,
                    }
                }))
        });
        self.structurally_changed
            .then_some(FieldChange::Structure)
            .into_iter()
            .chain(
                self.removed
                    .iter()
                    .map(|i| FieldChange::Removed(&self.index.units[*i].id)),
            )
            .chain(fields)
    }
    pub fn change_count(&self) -> usize {
        self.edits.len() + self.removed.len() + usize::from(self.structurally_changed)
    }
    pub fn same_revision(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.index, &other.index)
            && self.edits == other.edits
            && self.removed == other.removed
            && self.structurally_changed == other.structurally_changed
    }
    pub(crate) fn index_identity(&self) -> usize {
        Arc::as_ptr(&self.index) as usize
    }
    pub(crate) fn estimated_index_bytes(&self) -> usize {
        self.source_len() + self.field_count() * 128 + self.unit_count() * 256
    }
    pub fn units(&self) -> impl Iterator<Item = &Unit> {
        self.index
            .units
            .iter()
            .enumerate()
            .filter(|(i, _)| !self.removed.contains(i))
            .map(|(_, u)| u)
    }
    pub fn units_of_kind<'a>(&'a self, kind: &str) -> impl Iterator<Item = &'a Unit> {
        self.index
            .by_kind
            .get(kind)
            .into_iter()
            .flatten()
            .filter(|i| !self.removed.contains(i))
            .map(|i| &self.index.units[*i])
    }
    pub fn unit(&self, id: &str) -> Option<&Unit> {
        let index = *self.index.by_id.get(id)?;
        if self.removed.contains(&index) {
            None
        } else {
            self.index.units.get(index)
        }
    }
    pub fn first_id(&self, kind: &str) -> Result<&str> {
        self.units_of_kind(kind)
            .next()
            .map(|unit| unit.id.as_ref())
            .ok_or_else(|| Error::Missing(kind.into()))
    }
    fn group_key(&self, id: &str, field: &str) -> Option<GroupKey> {
        let unit = *self.index.by_id.get(id)?;
        if self.removed.contains(&unit) {
            return None;
        }
        let symbol = *self.index.symbols.get(field)?;
        self.index.units[unit]
            .fields
            .contains_key(&symbol)
            .then_some((unit, symbol))
    }
    fn value_at(&self, key: GroupKey, item: Option<usize>) -> Option<&str> {
        if let Some(edit) = self.edits.get(&key) {
            match edit {
                Edit::Array { count, values } => {
                    return if let Some(item) = item {
                        values.get(item).map(String::as_str)
                    } else {
                        Some(count)
                    };
                }
                Edit::Members(members) => {
                    if let Some(value) = members.get(&item) {
                        return Some(value);
                    }
                }
            }
        }
        let group = &self.index.units[key.0].fields[&key.1];
        let line = if let Some(item) = item {
            group.elements.get(&item)?
        } else {
            group.root.as_ref()?
        };
        Some(&self.index.source[line.value.clone()])
    }
    pub fn value(&self, id: &str, field: &str) -> Option<&str> {
        let (root, item) = field_key(field)?;
        self.value_at(self.group_key(id, root)?, item)
    }
    pub fn required(&self, id: &str, field: &str) -> Result<&str> {
        self.value(id, field)
            .ok_or_else(|| Error::Missing(field.into()))
    }
    pub fn integer<T: std::str::FromStr>(&self, id: &str, field: &str) -> Result<T> {
        self.required(id, field)?
            .parse()
            .map_err(|_| Error::InvalidSii("invalid integer"))
    }
    pub fn string(&self, id: &str, field: &str) -> Result<String> {
        value::unquote(self.required(id, field)?)
    }
    pub fn array(&self, id: &str, field: &str) -> Result<Vec<&str>> {
        let key = self
            .group_key(id, field)
            .ok_or_else(|| Error::Missing(field.into()))?;
        if let Some(Edit::Array { values, .. }) = self.edits.get(&key) {
            return Ok(values.iter().map(String::as_str).collect());
        }
        let count = self
            .value_at(key, None)
            .ok_or(Error::InvalidSii("array has no count"))?
            .parse::<usize>()
            .map_err(|_| Error::InvalidSii("invalid array count"))?;
        if count > MAX_ARRAY {
            return Err(Error::InvalidSii("array exceeds the element limit"));
        }
        let group = &self.index.units[key.0].fields[&key.1];
        if group.elements.len() != count {
            return Err(Error::InvalidSii("array count does not match its elements"));
        }
        (0..count)
            .map(|item| {
                self.value_at(key, Some(item))
                    .ok_or(Error::InvalidSii("array index is missing"))
            })
            .collect()
    }
    pub fn array_or_empty(&self, id: &str, field: &str) -> Result<Vec<&str>> {
        if self.value(id, field).is_none() {
            Ok(Vec::new())
        } else {
            self.array(id, field)
        }
    }
    pub fn fields<'a>(&'a self, id: &str) -> Vec<(&'a str, &'a str)> {
        let Some(unit) = self.unit(id) else {
            return Vec::new();
        };
        let mut fields: Vec<_> = unit
            .fields
            .keys()
            .filter_map(|symbol| {
                let name = self.index.names[*symbol].as_ref();
                self.value(id, name).map(|value| (name, value))
            })
            .collect();
        fields.sort_unstable_by_key(|(name, _)| *name);
        fields
    }
    pub fn set(&mut self, id: &str, field: &str, value: impl Into<String>) -> Result<()> {
        let value = value.into();
        value::validate(&value)?;
        let (root, item) =
            field_key(field).ok_or_else(|| Error::InvalidEdit("Invalid field name.".into()))?;
        let key = self
            .group_key(id, root)
            .ok_or_else(|| Error::Missing(root.into()))?;
        if self.value_at(key, item).is_none() {
            return Err(Error::Missing(field.into()));
        }
        let group = &self.index.units[key.0].fields[&key.1];
        if item.is_none()
            && (!group.elements.is_empty()
                || matches!(self.edits.get(&key), Some(Edit::Array { .. })))
        {
            return Err(Error::InvalidEdit(
                "Replace the array and its count together.".into(),
            ));
        }
        if self.value_at(key, item) == Some(value.as_str()) {
            return Ok(());
        }
        if let Some(Edit::Array { values, .. }) = self.edits.get(&key) {
            let index =
                item.ok_or_else(|| Error::InvalidEdit("Use the array operation.".into()))?;
            let mut values = values.clone();
            values[index] = value;
            let root = self.index.names[key.1].to_string();
            return self.set_array(id, &root, values);
        }
        let line = if let Some(item) = item {
            group.elements.get(&item)
        } else {
            group.root.as_ref()
        }
        .ok_or_else(|| Error::Missing(field.into()))?;
        let same_original = self.index.source[line.value.clone()] == value;
        let edit = self
            .edits
            .entry(key)
            .or_insert_with(|| Edit::Members(BTreeMap::new()));
        if let Edit::Members(members) = edit {
            if same_original {
                members.remove(&item);
            } else {
                members.insert(item, value);
            }
            if members.is_empty() {
                self.edits.remove(&key);
            }
        }
        Ok(())
    }
    pub fn set_string(&mut self, id: &str, field: &str, text: &str) -> Result<()> {
        self.set(id, field, value::quote(text))
    }
    pub fn set_array(&mut self, id: &str, field: &str, values: Vec<String>) -> Result<()> {
        if values.len() > MAX_ARRAY {
            return Err(Error::InvalidEdit("Too many array elements.".into()));
        }
        for value in &values {
            value::validate(value)?;
        }
        let previous = self.array(id, field)?;
        if previous
            .iter()
            .copied()
            .eq(values.iter().map(String::as_str))
        {
            return Ok(());
        }
        let key = self
            .group_key(id, field)
            .ok_or_else(|| Error::Missing(field.into()))?;
        let group = &self.index.units[key.0].fields[&key.1];
        let original_matches = group.root.as_ref().is_some_and(|root| {
            self.index.source[root.value.clone()].parse::<usize>().ok() == Some(values.len())
        }) && group.elements.len() == values.len()
            && values.iter().enumerate().all(|(i, value)| {
                group
                    .elements
                    .get(&i)
                    .is_some_and(|line| &self.index.source[line.value.clone()] == value)
            });
        if original_matches {
            self.edits.remove(&key);
            return Ok(());
        }
        self.edits.insert(
            key,
            Edit::Array {
                count: values.len().to_string(),
                values,
            },
        );
        Ok(())
    }
    /// All-or-nothing changes. The immutable index is shared, not duplicated.
    pub fn transaction<T>(&mut self, operation: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        let mut staged = self.clone();
        let output = operation(&mut staged)?;
        *self = staged;
        Ok(output)
    }
    pub fn remove_units(&mut self, ids: &[String]) -> Result<()> {
        let indices: Vec<_> = ids
            .iter()
            .map(|id| {
                self.index
                    .by_id
                    .get(id.as_str())
                    .copied()
                    .ok_or_else(|| Error::Missing("unit to remove".into()))
            })
            .collect::<Result<_>>()?;
        self.removed.extend(indices);
        Ok(())
    }
    pub fn fresh_id(&self, reserved: &BTreeSet<String>) -> String {
        (1u64..)
            .map(|counter| format!("_nameless.ffff.fffe.{counter:x}"))
            .find(|id| !self.index.by_id.contains_key(id.as_str()) && !reserved.contains(id))
            .expect("identifier space cannot be exhausted by a bounded document")
    }
    pub fn insert_units(&mut self, units: &[UnitSpec]) -> Result<()> {
        if units.is_empty() {
            return Ok(());
        }
        let mut reserved = BTreeSet::new();
        let mut extra = String::new();
        let newline = self.index.newline;
        for unit in units {
            if !token(&unit.kind)
                || !token(&unit.id)
                || self.index.by_id.contains_key(unit.id.as_str())
                || !reserved.insert(unit.id.clone())
            {
                return Err(Error::InvalidEdit(
                    "Invalid or duplicate new unit identifier.".into(),
                ));
            }
            extra.push_str(&format!("{} : {} {{{newline}", unit.kind, unit.id));
            for (name, raw) in &unit.scalars {
                if field_key(name) != Some((name.as_str(), None)) || unit.arrays.contains_key(name)
                {
                    return Err(Error::InvalidEdit("Invalid or duplicate new field.".into()));
                }
                value::validate(raw)?;
                extra.push_str(&format!(" {name}: {raw}{newline}"));
            }
            for (name, values) in &unit.arrays {
                if field_key(name) != Some((name.as_str(), None)) || values.len() > MAX_ARRAY {
                    return Err(Error::InvalidEdit("Invalid new array.".into()));
                }
                extra.push_str(&format!(" {name}: {}{newline}", values.len()));
                for (i, raw) in values.iter().enumerate() {
                    value::validate(raw)?;
                    extra.push_str(&format!(" {name}[{i}]: {raw}{newline}"));
                }
            }
            extra.push_str(&format!("}}{newline}"));
        }
        let text = self.render_with_extra(&extra)?;
        let mut replacement = Self::parse(Arc::<str>::from(text))?;
        replacement.structurally_changed = true;
        *self = replacement;
        Ok(())
    }
    /// Duplicate a unit verbatim, including unknown fields, arrays and comments.
    /// References in its body retain their original targets.
    pub fn duplicate_unit(&mut self, source: &str, id: &str) -> Result<()> {
        if !token(id) || self.unit(id).is_some() {
            return Err(Error::InvalidEdit(
                "Invalid or duplicate new unit identifier.".into(),
            ));
        }
        let current = Self::parse(Arc::<str>::from(self.render()?.into_owned()))?;
        let unit = current
            .unit(source)
            .ok_or_else(|| Error::Missing("unit to duplicate".into()))?;
        let raw = &current.index.source[unit.span.clone()];
        let colon = raw
            .find(':')
            .ok_or(Error::InvalidSii("missing unit separator"))?;
        let after = &raw[colon + 1..];
        let start = colon + 1 + after.len() - after.trim_start().len();
        let mut extra = raw.to_owned();
        extra.replace_range(start..start + unit.id.len(), id);
        let text = current.render_with_extra(&extra)?;
        let mut replacement = Self::parse(Arc::<str>::from(text))?;
        replacement.structurally_changed = true;
        *self = replacement;
        Ok(())
    }
    pub fn render(&self) -> Result<Cow<'_, str>> {
        if self.edits.is_empty() && self.removed.is_empty() {
            return Ok(Cow::Borrowed(&self.index.source));
        }
        Ok(Cow::Owned(self.render_with_extra("")?))
    }
    fn render_with_extra(&self, extra: &str) -> Result<String> {
        let mut patches: Vec<(Range<usize>, String)> = Vec::new();
        let source = &self.index.source;
        for index in &self.removed {
            patches.push((self.index.units[*index].span.clone(), String::new()));
        }
        for (key, edit) in &self.edits {
            if self.removed.contains(&key.0) {
                continue;
            }
            let group = &self.index.units[key.0].fields[&key.1];
            match edit {
                Edit::Members(members) => {
                    for (item, value) in members {
                        let line = if let Some(item) = item {
                            &group.elements[item]
                        } else {
                            group
                                .root
                                .as_ref()
                                .ok_or(Error::InvalidSii("missing field root"))?
                        };
                        patches.push((line.value.clone(), value.clone()));
                    }
                }
                Edit::Array { count, values } => {
                    let root = group
                        .root
                        .as_ref()
                        .ok_or(Error::InvalidSii("missing array root"))?;
                    let indent = &source[root.span.start..root.key_start];
                    let name = &self.index.names[key.1];
                    patches.push((root.value.clone(), count.clone()));
                    for (index, line) in &group.elements {
                        if let Some(raw) = values.get(*index) {
                            patches.push((line.value.clone(), raw.clone()));
                        } else {
                            let suffix = &source[line.value.end..line.span.end];
                            let comment = suffix.trim_start_matches([' ', '\t']);
                            let replacement =
                                if comment.starts_with('#') || comment.starts_with("//") {
                                    format!("{}{comment}", &source[line.span.start..line.key_start])
                                } else {
                                    String::new()
                                };
                            patches.push((line.span.clone(), replacement));
                        }
                    }
                    if values.len() > group.elements.len() {
                        let at = group
                            .elements
                            .values()
                            .map(|line| line.span.end)
                            .max()
                            .unwrap_or(root.span.end);
                        let mut extra = String::new();
                        for (index, raw) in values.iter().enumerate().skip(group.elements.len()) {
                            extra.push_str(&format!(
                                "{indent}{name}[{index}]: {raw}{}",
                                self.index.newline
                            ));
                        }
                        patches.push((at..at, extra));
                    }
                }
            }
        }
        if !extra.is_empty() {
            patches.push((self.index.closing..self.index.closing, extra.into()));
        }
        patches.sort_unstable_by_key(|(range, _)| (range.start, range.end));
        let mut output = String::with_capacity(source.len() + extra.len());
        let mut cursor = 0;
        for (range, replacement) in patches {
            if range.start < cursor || range.end > source.len() {
                return Err(Error::InvalidSii("overlapping save edits"));
            }
            output.push_str(&source[cursor..range.start]);
            output.push_str(&replacement);
            cursor = range.end;
        }
        output.push_str(&source[cursor..]);
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const TEXT: &str = "SiiNunit\r\n{\r\neconomy : _nameless.1 {\r\n amount: 12\r\n unknown: \"retain: { }\"\r\n items: 2\r\n items[0]: one\r\n future_scalar: 123\r\n items[1]: two\r\n}\r\nfuture_type : future.unit {\r\n untouched: &00000001\r\n}\r\n}\r\n";
    #[test]
    fn unedited_roundtrip_is_byte_exact_for_all_newlines() {
        for newline in ["\r\n", "\n", "\r"] {
            let input = TEXT.replace("\r\n", newline);
            let doc = Document::parse(input.clone()).unwrap();
            assert_eq!(doc.render().unwrap(), input);
            assert!(!doc.is_dirty());
            assert_eq!(doc.unit_count(), 2);
        }
    }
    #[test]
    fn scalar_edits_preserve_every_other_byte_and_can_be_reverted() {
        let mut doc = Document::parse(TEXT).unwrap();
        doc.set("_nameless.1", "amount", "999").unwrap();
        assert_eq!(
            doc.render().unwrap(),
            TEXT.replace("amount: 12", "amount: 999")
        );
        doc.set("_nameless.1", "amount", "12").unwrap();
        assert!(!doc.is_dirty());
        assert_eq!(doc.render().unwrap(), TEXT);
    }
    #[test]
    fn array_replacement_retains_interleaved_unknown_fields() {
        let mut doc = Document::parse(TEXT).unwrap();
        doc.set_array("_nameless.1", "items", vec!["three".into()])
            .unwrap();
        let output = doc.render().unwrap();
        assert!(output.contains(" future_scalar: 123\r\n"));
        assert!(!output.contains("items[1]"));
        let reloaded = Document::parse(Arc::<str>::from(output.as_ref())).unwrap();
        assert_eq!(reloaded.array("_nameless.1", "items").unwrap(), ["three"]);
        assert_eq!(
            reloaded.value("future.unit", "untouched"),
            Some("&00000001")
        );
        doc.set_array("_nameless.1", "items", Vec::new()).unwrap();
        assert!(doc.array("_nameless.1", "items").unwrap().is_empty());
    }
    #[test]
    fn inline_comments_survive_scalar_and_array_edits_and_reverts() {
        for newline in ["\r\n", "\n", "\r"] {
            let input = "SiiNunit // header\n{ # outer\neconomy : test.unit { // declaration\n amount: 12 # exact comment\n quoted: \"https://example/#tag\\\"//still quoted\" // trailing\n items: 2 // array count\n items[0]: one # first\n unknown: \"retained\"\n items[1]: two // second\n} // unit end\n} # end\n".replace('\n', newline);
            let mut doc = Document::parse(input.clone()).unwrap();
            assert_eq!(doc.integer::<u32>("test.unit", "amount").unwrap(), 12);
            assert_eq!(
                doc.string("test.unit", "quoted").unwrap(),
                "https://example/#tag\"//still quoted"
            );
            assert_eq!(doc.render().unwrap(), input);
            doc.set("test.unit", "amount", "99").unwrap();
            doc.set_array("test.unit", "items", vec!["changed".into()])
                .unwrap();
            let output = doc.render().unwrap();
            assert!(output.contains("amount: 99 # exact comment"));
            assert!(output.contains("items: 1 // array count"));
            assert!(output.contains("items[0]: changed # first"));
            assert!(output.contains(&format!(" // second{newline}")));
            assert!(output.contains(&format!(" unknown: \"retained\"{newline}")));
            let reloaded = Document::parse(output.into_owned()).unwrap();
            assert_eq!(reloaded.array("test.unit", "items").unwrap(), ["changed"]);
            doc.set_array("test.unit", "items", vec!["one".into(), "two".into()])
                .unwrap();
            doc.set("test.unit", "amount", "12").unwrap();
            assert!(!doc.is_dirty());
            assert_eq!(doc.render().unwrap(), input);
        }
    }
    #[test]
    fn array_growth_and_member_reverts_restore_exact_original() {
        let mut doc = Document::parse(TEXT).unwrap();
        doc.set_array(
            "_nameless.1",
            "items",
            vec!["one".into(), "two".into(), "three".into()],
        )
        .unwrap();
        let reloaded = Document::parse(doc.render().unwrap().into_owned()).unwrap();
        assert_eq!(
            reloaded.array("_nameless.1", "items").unwrap(),
            ["one", "two", "three"]
        );
        doc.set_array("_nameless.1", "items", vec!["one".into(), "two".into()])
            .unwrap();
        assert!(!doc.is_dirty());
        assert_eq!(doc.render().unwrap(), TEXT);
        doc.set("_nameless.1", "items[1]", "changed").unwrap();
        doc.set_array("_nameless.1", "items", vec!["one".into(), "two".into()])
            .unwrap();
        assert!(!doc.is_dirty());
        doc.set_array("_nameless.1", "items", vec!["one".into(), "changed".into()])
            .unwrap();
        doc.set("_nameless.1", "items[1]", "two").unwrap();
        assert!(!doc.is_dirty());
    }
    #[test]
    fn rejected_transaction_rolls_back_all_staged_changes() {
        let mut doc = Document::parse(TEXT).unwrap();
        let result = doc.transaction(|draft| {
            draft.set("_nameless.1", "amount", "100")?;
            draft.set("_nameless.1", "unknown", "0 }\n new.unit {")
        });
        assert!(result.is_err());
        assert_eq!(doc.render().unwrap(), TEXT);
    }
    #[test]
    fn array_elements_and_inserted_units_are_addressable() {
        let mut doc = Document::parse(TEXT).unwrap();
        doc.set("_nameless.1", "items[1]", "changed").unwrap();
        let unit = UnitSpec {
            kind: "future_type".into(),
            id: "new.unit".into(),
            scalars: BTreeMap::from([("name".into(), value::quote("New"))]),
            ..Default::default()
        };
        doc.insert_units(&[unit]).unwrap();
        assert_eq!(doc.value("_nameless.1", "items[1]"), Some("changed"));
        assert_eq!(doc.string("new.unit", "name").unwrap(), "New");
        assert!(doc.is_dirty());
        doc.remove_units(&["new.unit".into()]).unwrap();
        let output = doc.render().unwrap();
        assert!(!output.contains("new.unit"));
        assert!(output.contains("future.unit"));
    }
    #[test]
    fn malformed_or_ambiguous_input_is_refused() {
        for input in [
            "SiiNunit\n{\n",
            "SiiNunit\n{\nx : x {\na: 1\na: 2\n}\n}\n",
            "SiiNunit\n{\nx : x {\n}\nx : x {\n}\n}\n",
        ] {
            assert!(Document::parse(input).is_err());
        }
        let mut doc = Document::parse(TEXT).unwrap();
        assert!(doc.set("_nameless.1", "items", "3").is_err());
        assert!(doc.set("_nameless.1", "absent", "1").is_err());
    }
}
