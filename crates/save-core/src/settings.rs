//! Portable profile settings. Names are allowlisted; bundle data is never executed.
use crate::{Error, Result, session::Game};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fs, path::Path};

pub const MAX_FILE: usize = 2 * 1024 * 1024;
pub const MAX_TOTAL: usize = 8 * 1024 * 1024;
pub const MAX_BUNDLE: usize = MAX_TOTAL * 2 + 64 * 1024;
pub const MAX_FILES: usize = 32;

pub fn valid_name(name: &str) -> bool {
    matches!(name, "config.cfg" | "config_local.cfg" | "controls.sii")
        || (name.len() <= 80
            && name.starts_with("gearbox_")
            && name.ends_with(".sii")
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'.'))
            && !name.contains(".."))
}

pub(crate) fn ordinary(path: &Path, directory: bool) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // OneDrive cloud placeholders also use REPARSE_POINT. read_link
        // distinguishes actual links/junctions without rejecting cloud files.
        if metadata.file_attributes() & 0x400 != 0 && fs::read_link(path).is_ok() {
            return Err(Error::InvalidEdit(
                "Linked profile files and folders are not supported for this operation.".into(),
            ));
        }
    }
    if metadata.file_type().is_symlink()
        || (if directory {
            !metadata.is_dir()
        } else {
            !metadata.is_file()
        })
    {
        return Err(Error::InvalidEdit(
            "Select ordinary profile files and folders.".into(),
        ));
    }
    Ok(())
}

pub fn names(folder: &Path) -> Result<Vec<String>> {
    ordinary(folder, true)?;
    let mut names = Vec::new();
    for entry in fs::read_dir(folder)? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if valid_name(&name) {
            ordinary(&entry.path(), false)?;
            names.push(name);
            if names.len() > MAX_FILES {
                return Err(Error::InvalidEdit(
                    "Too many profile settings files.".into(),
                ));
            }
        }
    }
    names.sort_unstable();
    Ok(names)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsFile {
    pub name: String,
    pub bytes: Vec<u8>,
}

pub fn validate(files: &[SettingsFile]) -> Result<()> {
    let mut seen = BTreeSet::new();
    let mut total = 0usize;
    if files.is_empty() || files.len() > MAX_FILES {
        return Err(Error::InvalidEdit(
            "Choose between 1 and 32 settings files.".into(),
        ));
    }
    for file in files {
        total = total.saturating_add(file.bytes.len());
        if !valid_name(&file.name)
            || !seen.insert(&file.name)
            || file.bytes.len() > MAX_FILE
            || total > MAX_TOTAL
        {
            return Err(Error::InvalidEdit(
                "Invalid, duplicate or oversized profile settings file.".into(),
            ));
        }
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Bundle {
    truckcraft_settings: u32,
    game: Game,
    files: Vec<Entry>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    name: String,
    hex: String,
}

pub fn export(game: Game, files: &[SettingsFile]) -> Result<String> {
    validate(files)?;
    let files = files
        .iter()
        .map(|file| {
            let mut hex = String::with_capacity(file.bytes.len() * 2);
            for byte in &file.bytes {
                use std::fmt::Write;
                let _ = write!(hex, "{byte:02x}");
            }
            Entry {
                name: file.name.clone(),
                hex,
            }
        })
        .collect();
    serde_json::to_string(&Bundle {
        truckcraft_settings: 1,
        game,
        files,
    })
    .map_err(|_| Error::InvalidEdit("Could not export settings.".into()))
}

pub fn import(game: Game, text: &str) -> Result<Vec<SettingsFile>> {
    if text.len() > MAX_BUNDLE {
        return Err(Error::InvalidEdit("Settings bundle is too large.".into()));
    }
    let bundle: Bundle = serde_json::from_str(text)
        .map_err(|_| Error::InvalidEdit("Expected a TruckCraft settings bundle.".into()))?;
    if bundle.truckcraft_settings != 1
        || bundle.game != game
        || bundle.files.is_empty()
        || bundle.files.len() > MAX_FILES
    {
        return Err(Error::InvalidEdit(
            "Choose a settings bundle for the selected game.".into(),
        ));
    }
    let mut total = 0usize;
    let files = bundle
        .files
        .into_iter()
        .map(|file| {
            total = total.saturating_add(file.hex.len() / 2);
            if !valid_name(&file.name)
                || !file.hex.len().is_multiple_of(2)
                || file.hex.len() > MAX_FILE * 2
                || total > MAX_TOTAL
                || !file.hex.is_ascii()
            {
                return Err(Error::InvalidEdit(
                    "Invalid or oversized settings entry.".into(),
                ));
            }
            let bytes = file
                .hex
                .as_bytes()
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| {
                    let a = char::from(pair[0]).to_digit(16);
                    let b = char::from(pair[1]).to_digit(16);
                    a.zip(b)
                        .map(|(a, b)| ((a << 4) | b) as u8)
                        .ok_or_else(|| Error::InvalidEdit("Invalid settings data.".into()))
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(SettingsFile {
                name: file.name,
                bytes,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    validate(&files)?;
    Ok(files)
}
