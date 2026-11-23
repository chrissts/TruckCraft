//! Three-file save sessions, bounded undo and recoverable atomic replacements.
use crate::{Error, Result, decode_document, document::Document};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_FILE: usize = 256 * 1024 * 1024;
const HISTORY_BUDGET: usize = 128 * 1024 * 1024;
const PENDING_BACKUP: &str = ".pending-";
pub const SAVE_FORMAT_TARGET: u32 = 105;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Game {
    #[serde(rename = "ETS2")]
    Ets2,
    #[serde(rename = "ATS")]
    Ats,
}
impl Game {
    pub fn name(self) -> &'static str {
        match self {
            Self::Ets2 => "ETS2",
            Self::Ats => "ATS",
        }
    }
    pub fn folder(self) -> &'static str {
        match self {
            Self::Ets2 => "Euro Truck Simulator 2",
            Self::Ats => "American Truck Simulator",
        }
    }
}

#[derive(Clone, Debug)]
pub struct SaveData {
    pub profile: Document,
    pub info: Document,
    pub game: Document,
    pub simulator: Game,
}
impl SaveData {
    pub fn economy_id(&self) -> Result<&str> {
        self.game.first_id("economy")
    }
    pub fn player_id(&self) -> Result<&str> {
        self.game.required(self.economy_id()?, "player")
    }
    pub fn bank_id(&self) -> Result<&str> {
        self.game.required(self.economy_id()?, "bank")
    }
    pub fn format_version(&self) -> Result<u32> {
        self.info
            .integer(self.info.first_id("save_container")?, "version")
    }
    pub fn profile_id(&self) -> Result<&str> {
        self.profile.first_id("user_profile")
    }
    pub fn profile_name(&self) -> String {
        self.profile_id()
            .and_then(|id| self.profile.string(id, "profile_name"))
            .unwrap_or_else(|_| "Profile".into())
    }
    pub fn save_name(&self) -> String {
        self.info
            .first_id("save_container")
            .and_then(|id| self.info.string(id, "name"))
            .unwrap_or_default()
    }
    pub fn is_dirty(&self) -> bool {
        self.profile.is_dirty() || self.info.is_dirty() || self.game.is_dirty()
    }
    fn same_revision(&self, other: &Self) -> bool {
        self.profile.same_revision(&other.profile)
            && self.info.same_revision(&other.info)
            && self.game.same_revision(&other.game)
    }
    fn documents(&self) -> [&Document; 3] {
        [&self.profile, &self.info, &self.game]
    }
}

#[derive(Clone, Debug)]
struct FileImage {
    path: PathBuf,
    bytes: Arc<[u8]>,
    hash: [u8; 32],
    modified: Option<SystemTime>,
    limit: usize,
}
impl FileImage {
    fn read(path: PathBuf) -> Result<Self> {
        Self::read_limited(path, MAX_FILE)
    }
    fn read_limited(path: PathBuf, limit: usize) -> Result<Self> {
        let file = File::open(&path)?;
        let metadata = file.metadata()?;
        if metadata.len() > limit as u64 {
            return Err(Error::InvalidSii("file exceeds the size limit"));
        }
        let before = metadata.modified().ok();
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        file.take((limit + 1) as u64).read_to_end(&mut bytes)?;
        if bytes.len() > limit {
            return Err(Error::InvalidSii("file exceeds the size limit"));
        }
        let after = fs::metadata(&path)?;
        if before != after.modified().ok() || after.len() != bytes.len() as u64 {
            return Err(Error::FileChanged);
        }
        let hash = Sha256::digest(&bytes).into();
        Ok(Self {
            path,
            bytes: bytes.into(),
            hash,
            modified: before,
            limit,
        })
    }
    fn verify(&self) -> Result<()> {
        let current = match Self::read_limited(self.path.clone(), self.limit) {
            Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(Error::FileChanged);
            }
            result => result?,
        };
        if current.hash != self.hash || current.modified != self.modified {
            return Err(Error::FileChanged);
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum SaveOutcome {
    Unchanged,
    Saved {
        backup: PathBuf,
        changed_files: usize,
    },
}

pub struct SaveSession {
    pub data: SaveData,
    images: [FileImage; 3],
    save_folder: PathBuf,
    settings_originals: BTreeMap<String, Option<FileImage>>,
    settings_edits: BTreeMap<String, Option<Arc<[u8]>>>,
    replacements: [Option<Arc<[u8]>>; 3],
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    reload_required: bool,
}

#[derive(Clone)]
struct Snapshot {
    data: SaveData,
    settings: BTreeMap<String, Option<Arc<[u8]>>>,
    replacements: [Option<Arc<[u8]>>; 3],
}

struct PreparedWrite {
    path: PathBuf,
    original: Option<Arc<[u8]>>,
    expected: Option<Arc<[u8]>>,
    temporary: Option<tempfile::NamedTempFile>,
}

pub fn recovery_snapshots(slot: &Path) -> Result<Vec<PathBuf>> {
    let root = slot.join("tsset-backups");
    match fs::symlink_metadata(&root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
        Ok(_) => {}
    }
    crate::settings::ordinary(&root, true)?;
    let mut snapshots = Vec::new();
    for entry in fs::read_dir(&root)?.take(1025) {
        let path = entry?.path();
        if !pending_backup(&path)
            && path.join("game.sii").is_file()
            && path.join("info.sii").is_file()
            && path.join("profile.sii").is_file()
        {
            snapshots.push(path);
        }
        if snapshots.len() > 1024 {
            return Err(Error::InvalidEdit(
                "Too many recovery snapshots. Choose a snapshot folder directly.".into(),
            ));
        }
    }
    snapshots.sort_unstable();
    snapshots.reverse();
    Ok(snapshots)
}

pub(crate) fn checked_snapshot(slot: &Path, folder: &Path) -> Result<PathBuf> {
    let root = slot.join("tsset-backups");
    crate::settings::ordinary(&root, true)?;
    crate::settings::ordinary(folder, true)?;
    let folder = folder.canonicalize()?;
    if pending_backup(&folder) || folder.parent() != Some(root.canonicalize()?.as_path()) {
        return Err(Error::InvalidEdit(
            "Choose a recovery snapshot from this save's tsset-backups folder.".into(),
        ));
    }
    Ok(folder)
}

type RecoverySettings = BTreeMap<String, Option<Arc<[u8]>>>;

pub(crate) fn snapshot_settings(folder: &Path) -> Result<RecoverySettings> {
    let mut files = BTreeMap::new();
    let settings = folder.join("settings");
    let manifest = folder.join("settings-absent.json");
    if fs::symlink_metadata(&settings).is_ok() || fs::symlink_metadata(&manifest).is_ok() {
        crate::settings::ordinary(&settings, true)?;
        crate::settings::ordinary(&manifest, false)?;
        let absent: Vec<String> =
            serde_json::from_slice(&crate::discovery::read_small(&manifest, 8192)?)
                .map_err(|_| Error::InvalidSii("invalid settings recovery manifest"))?;
        let names = crate::settings::names(&settings)?;
        if names.len() + absent.len() > crate::settings::MAX_FILES {
            return Err(Error::InvalidSii("too many recovery settings files"));
        }
        let mut total = 0usize;
        for name in names {
            let image = FileImage::read_limited(settings.join(&name), crate::settings::MAX_FILE)?;
            total += image.bytes.len();
            if total > crate::settings::MAX_TOTAL {
                return Err(Error::InvalidSii("settings recovery snapshot is too large"));
            }
            files.insert(name, Some(image.bytes));
        }
        let mut seen = std::collections::BTreeSet::new();
        for name in absent {
            if !crate::settings::valid_name(&name)
                || !seen.insert(name.clone())
                || settings.join(&name).exists()
            {
                return Err(Error::InvalidSii("invalid absent settings recovery entry"));
            }
            files.insert(name, None);
        }
    }
    Ok(files)
}

impl SaveSession {
    pub fn load(game_file: impl AsRef<Path>, simulator: Game) -> Result<Self> {
        let game_file = game_file.as_ref().to_path_buf();
        if !game_file
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case("game.sii"))
        {
            return Err(Error::InvalidEdit("Choose a game.sii save file.".into()));
        }
        let save_folder = game_file
            .parent()
            .ok_or_else(|| Error::Missing("save folder".into()))?
            .to_path_buf();
        let save_root = save_folder
            .parent()
            .ok_or_else(|| Error::Missing("profile save root".into()))?;
        if !save_root
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case("save"))
        {
            return Err(Error::InvalidEdit(
                "Choose game.sii inside a profile's save folder.".into(),
            ));
        }
        let profile_folder = save_root
            .parent()
            .ok_or_else(|| Error::Missing("profile folder".into()))?;
        let images = [
            FileImage::read(profile_folder.join("profile.sii"))?,
            FileImage::read(save_folder.join("info.sii"))?,
            FileImage::read(game_file)?,
        ];
        let data = SaveData {
            profile: decode_document(&images[0].bytes)?,
            info: decode_document(&images[1].bytes)?,
            game: decode_document(&images[2].bytes)?,
            simulator,
        };
        data.profile_id()?;
        let player = data.player_id()?;
        let bank = data.bank_id()?;
        if data.game.unit(player).is_none() || data.game.unit(bank).is_none() {
            return Err(Error::InvalidSii(
                "economy references missing player/bank units",
            ));
        }
        let _ = data.format_version()?;
        Ok(Self {
            data,
            images,
            save_folder,
            settings_originals: BTreeMap::new(),
            settings_edits: BTreeMap::new(),
            replacements: Default::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            reload_required: false,
        })
    }
    pub fn save_folder(&self) -> &Path {
        &self.save_folder
    }
    pub fn staged_settings(&self) -> impl Iterator<Item = (&str, bool)> {
        self.settings_edits
            .iter()
            .map(|(name, data)| (name.as_str(), data.is_some()))
    }

    pub fn replaced_files(&self) -> impl Iterator<Item = &'static str> + '_ {
        ["profile.sii", "info.sii", "game.sii"]
            .into_iter()
            .zip(&self.replacements)
            .filter_map(|(name, bytes)| bytes.is_some().then_some(name))
    }
    pub fn can_save(&self) -> bool {
        !self.reload_required && self.is_dirty()
    }
    pub fn is_dirty(&self) -> bool {
        self.data.is_dirty()
            || !self.settings_edits.is_empty()
            || self.replacements.iter().any(Option::is_some)
    }
    pub fn profile_folder(&self) -> &Path {
        self.images[0]
            .path
            .parent()
            .expect("loaded profile has a parent")
    }
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            data: self.data.clone(),
            settings: self.settings_edits.clone(),
            replacements: self.replacements.clone(),
        }
    }
    fn restore_snapshot(&mut self, state: Snapshot) {
        self.data = state.data;
        self.settings_edits = state.settings;
        self.replacements = state.replacements;
    }
    fn setting_original(&mut self, name: &str) -> Result<Option<FileImage>> {
        if let Some(image) = self.settings_originals.get(name) {
            return Ok(image.clone());
        }
        crate::settings::ordinary(self.profile_folder(), true)?;
        let path = self.profile_folder().join(name);
        let image = match fs::symlink_metadata(&path) {
            Ok(meta) => {
                if meta.len() > crate::settings::MAX_FILE as u64 {
                    return Err(Error::InvalidEdit(
                        "Profile settings file is too large.".into(),
                    ));
                }
                crate::settings::ordinary(&path, false)?;
                Some(FileImage::read_limited(path, crate::settings::MAX_FILE)?)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        let total: usize = self
            .settings_originals
            .values()
            .flatten()
            .map(|image| image.bytes.len())
            .sum();
        if self.settings_originals.len() >= crate::settings::MAX_FILES
            || total + image.as_ref().map_or(0, |image| image.bytes.len())
                > crate::settings::MAX_TOTAL
        {
            return Err(Error::InvalidEdit(
                "Too many loaded profile settings files. Reload before choosing other files."
                    .into(),
            ));
        }
        self.settings_originals.insert(name.into(), image.clone());
        Ok(image)
    }
    pub fn settings_files(&mut self) -> Result<Vec<crate::settings::SettingsFile>> {
        let mut names = crate::settings::names(self.profile_folder())?;
        names.extend(self.settings_edits.keys().cloned());
        names.sort_unstable();
        names.dedup();
        let mut files = Vec::new();
        for name in names {
            let original = self.setting_original(&name)?;
            let bytes = self
                .settings_edits
                .get(&name)
                .cloned()
                .unwrap_or_else(|| original.map(|image| image.bytes));
            if let Some(bytes) = bytes {
                files.push(crate::settings::SettingsFile {
                    name,
                    bytes: bytes.to_vec(),
                });
            }
        }
        if !files.is_empty() {
            crate::settings::validate(&files)?;
        }
        Ok(files)
    }
    pub fn import_settings(&mut self, files: &[crate::settings::SettingsFile]) -> Result<()> {
        if self.reload_required {
            return Err(Error::ReloadRequired);
        }
        crate::settings::validate(files)?;
        let mut edits = self.settings_edits.clone();
        for file in files {
            let original = self.setting_original(&file.name)?;
            if original
                .as_ref()
                .is_some_and(|old| old.bytes.as_ref() == file.bytes)
            {
                edits.remove(&file.name);
            } else {
                edits.insert(file.name.clone(), Some(file.bytes.clone().into()));
            }
        }
        let total: usize = edits.values().flatten().map(|bytes| bytes.len()).sum();
        if edits.len() > crate::settings::MAX_FILES || total > crate::settings::MAX_TOTAL {
            return Err(Error::InvalidEdit("Too many staged settings files.".into()));
        }
        if edits != self.settings_edits {
            self.undo.push(self.snapshot());
            self.settings_edits = edits;
            self.redo.clear();
            self.bound_history();
        }
        Ok(())
    }
    pub fn recovery_snapshots(&self) -> Result<Vec<PathBuf>> {
        recovery_snapshots(&self.save_folder)
    }
    /// Stage a complete tool snapshot from this save's recovery directory. The
    /// current files are backed up again when the staged restoration is saved.
    pub fn restore_recovery(&mut self, folder: &Path) -> Result<usize> {
        if self.reload_required {
            return Err(Error::ReloadRequired);
        }
        if self.is_dirty() {
            return Err(Error::InvalidEdit(
                "Save or undo staged changes before restoring a recovery snapshot.".into(),
            ));
        }
        let folder = checked_snapshot(&self.save_folder, folder)?;
        let mut restored = Vec::new();
        for name in ["profile.sii", "info.sii", "game.sii"] {
            let path = folder.join(name);
            crate::settings::ordinary(&path, false)?;
            restored.push(FileImage::read(path)?);
        }
        let data = SaveData {
            profile: decode_document(&restored[0].bytes)?,
            info: decode_document(&restored[1].bytes)?,
            game: decode_document(&restored[2].bytes)?,
            simulator: self.data.simulator,
        };
        data.profile_id()?;
        data.format_version()?;
        if data.game.unit(data.player_id()?).is_none() || data.game.unit(data.bank_id()?).is_none()
        {
            return Err(Error::InvalidSii(
                "recovery snapshot references missing player/bank units",
            ));
        }
        let mut edits = BTreeMap::new();
        for (name, bytes) in snapshot_settings(&folder)? {
            let original = self.setting_original(&name)?;
            if original.as_ref().map(|file| &file.bytes) != bytes.as_ref() {
                edits.insert(name, bytes);
            }
        }
        let replacements = std::array::from_fn(|index| {
            if restored[index].bytes != self.images[index].bytes {
                Some(restored[index].bytes.clone())
            } else {
                None
            }
        });
        let count = replacements.iter().filter(|bytes| bytes.is_some()).count() + edits.len();
        if count > 0 {
            self.undo.push(self.snapshot());
            self.data = data;
            self.settings_edits = edits;
            self.replacements = replacements;
            self.redo.clear();
            self.bound_history();
        }
        Ok(count)
    }
    pub fn require_reload(&mut self) {
        self.reload_required = true;
    }
    pub fn reload_required(&self) -> bool {
        self.reload_required
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn edit<T>(&mut self, operation: impl FnOnce(&mut SaveData) -> Result<T>) -> Result<T> {
        if self.reload_required {
            return Err(Error::ReloadRequired);
        }
        let mut staged = self.data.clone();
        let result = operation(&mut staged)?;
        if !staged.same_revision(&self.data) {
            self.undo.push(self.snapshot());
            self.data = staged;
            self.redo.clear();
            self.bound_history();
        }
        Ok(result)
    }
    pub fn undo(&mut self) -> bool {
        if let Some(previous) = self.undo.pop() {
            self.redo.push(self.snapshot());
            self.restore_snapshot(previous);
            true
        } else {
            false
        }
    }
    pub fn redo(&mut self) -> bool {
        if let Some(next) = self.redo.pop() {
            self.undo.push(self.snapshot());
            self.restore_snapshot(next);
            self.bound_history();
            true
        } else {
            false
        }
    }
    fn bound_history(&mut self) {
        loop {
            let mut indices = HashMap::new();
            let current = self.snapshot();
            for state in self.undo.iter().chain(std::iter::once(&current)) {
                for doc in state.data.documents() {
                    indices
                        .entry(doc.index_identity())
                        .or_insert_with(|| doc.estimated_index_bytes());
                }
                for bytes in state
                    .settings
                    .values()
                    .flatten()
                    .chain(state.replacements.iter().flatten())
                {
                    indices
                        .entry(Arc::as_ptr(bytes) as *const u8 as usize)
                        .or_insert(bytes.len());
                }
            }
            if self.undo.is_empty()
                || (self.undo.len() <= 64 && indices.values().sum::<usize>() <= HISTORY_BUDGET)
            {
                break;
            }
            self.undo.remove(0);
        }
    }
    fn verify_originals(&self) -> Result<()> {
        for image in &self.images {
            image.verify()?;
        }
        for name in self.settings_edits.keys() {
            crate::settings::ordinary(self.profile_folder(), true)?;
            match self
                .settings_originals
                .get(name)
                .ok_or(Error::FileChanged)?
            {
                Some(image) => {
                    image.verify()?;
                    crate::settings::ordinary(&image.path, false)?;
                }
                None => match fs::symlink_metadata(self.profile_folder().join(name)) {
                    Ok(_) => return Err(Error::FileChanged),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                },
            }
        }
        Ok(())
    }
    fn check_originals_before_write(&mut self) -> Result<()> {
        let result = self.verify_originals();
        if matches!(result, Err(Error::FileChanged)) {
            self.require_reload();
        }
        result
    }
    fn backup(&self) -> Result<PathBuf> {
        let root = self.save_folder.join("tsset-backups");
        fs::create_dir_all(&root)?;
        crate::settings::ordinary(&root, true)?;
        let pending = tempfile::Builder::new()
            .prefix(PENDING_BACKUP)
            .tempdir_in(&root)?;
        let path = pending.path();
        for (name, image) in ["profile.sii", "info.sii", "game.sii"]
            .iter()
            .zip(&self.images)
        {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(path.join(name))?;
            file.write_all(&image.bytes)?;
            file.sync_all()?;
        }
        if !self.settings_edits.is_empty() {
            let settings = path.join("settings");
            fs::create_dir(&settings)?;
            let mut absent = Vec::new();
            for name in self.settings_edits.keys() {
                if let Some(image) = self.settings_originals.get(name).and_then(Option::as_ref) {
                    let mut file = OpenOptions::new()
                        .create_new(true)
                        .write(true)
                        .open(settings.join(name))?;
                    file.write_all(&image.bytes)?;
                    file.sync_all()?;
                } else {
                    absent.push(name);
                }
            }
            let mut manifest = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(path.join("settings-absent.json"))?;
            manifest.write_all(
                serde_json::to_string(&absent)
                    .map_err(|_| Error::InvalidSii("settings recovery manifest"))?
                    .as_bytes(),
            )?;
            manifest.sync_all()?;
        }
        let instant = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let suffix = path.file_name().unwrap().to_string_lossy();
        let destination = root.join(format!("{instant:x}-{}", &suffix[PENDING_BACKUP.len()..]));
        fs::rename(path, &destination)?;
        Ok(destination)
    }
    pub fn save(&mut self) -> Result<SaveOutcome> {
        if self.reload_required {
            return Err(Error::ReloadRequired);
        }
        if !self.is_dirty() {
            return Ok(SaveOutcome::Unchanged);
        }
        let output: Vec<Option<Arc<[u8]>>> = self
            .data
            .documents()
            .into_iter()
            .enumerate()
            .map(|(index, doc)| {
                if doc.is_dirty() {
                    doc.render().map(|text| Some(Arc::from(text.as_bytes())))
                } else {
                    Ok(self.replacements[index].clone())
                }
            })
            .collect::<Result<_>>()?;
        // Parse every prospective replacement before touching any original file.
        let clean: Vec<Document> = self
            .data
            .documents()
            .into_iter()
            .zip(&output)
            .map(|(doc, bytes)| match bytes {
                Some(bytes) => decode_document(bytes),
                None => Ok(doc.clone()),
            })
            .collect::<Result<_>>()?;
        self.check_originals_before_write()?;
        let mut prepared = Vec::new();
        for (index, bytes) in output.iter().enumerate() {
            if let Some(bytes) = bytes {
                prepared.push(PreparedWrite {
                    path: self.images[index].path.clone(),
                    original: Some(self.images[index].bytes.clone()),
                    expected: Some(bytes.clone()),
                    temporary: Some(prepare(&self.images[index].path, bytes)?),
                });
            }
        }
        for (name, bytes) in &self.settings_edits {
            let path = self.profile_folder().join(name);
            prepared.push(PreparedWrite {
                path: path.clone(),
                original: self
                    .settings_originals
                    .get(name)
                    .and_then(Option::as_ref)
                    .map(|image| image.bytes.clone()),
                expected: bytes.clone(),
                temporary: bytes
                    .as_ref()
                    .map(|bytes| prepare(&path, bytes))
                    .transpose()?,
            });
        }
        let backup = self.backup()?;
        self.check_originals_before_write()?;
        self.reload_required = true;
        let mut written: Vec<PreparedWrite> = Vec::new();
        for mut write in prepared {
            let published = if let Some(temporary) = write.temporary.take() {
                (if write.original.is_some() {
                    temporary.persist(&write.path)
                } else {
                    temporary.persist_noclobber(&write.path)
                })
                .map(|_| ())
                .map_err(|error| error.error)
            } else {
                fs::remove_file(&write.path)
            };
            if let Err(error) = published {
                let reason = error.to_string();
                let mut restored = true;
                for previous in written.iter().rev() {
                    let result = if let Some(bytes) = &previous.original {
                        replace(&previous.path, bytes)
                    } else {
                        fs::remove_file(&previous.path).map_err(Error::from)
                    };
                    if result.is_err() {
                        restored = false;
                    }
                }
                self.reload_required = true;
                return Err(Error::Commit {
                    backup: backup
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    restored,
                    reason,
                });
            }
            written.push(write);
        }
        for write in &written {
            if let Some(expected) = &write.expected {
                if FileImage::read(write.path.clone())?.bytes != *expected {
                    return Err(Error::FileChanged);
                }
            } else {
                match fs::symlink_metadata(&write.path) {
                    Ok(_) => return Err(Error::FileChanged),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            }
        }
        let refreshed = [
            FileImage::read(self.images[0].path.clone())?,
            FileImage::read(self.images[1].path.clone())?,
            FileImage::read(self.images[2].path.clone())?,
        ];
        for (index, image) in refreshed.iter().enumerate() {
            let expected = output[index]
                .as_deref()
                .unwrap_or(&self.images[index].bytes);
            if image.bytes.as_ref() != expected {
                self.reload_required = true;
                return Err(Error::FileChanged);
            }
        }
        let mut clean = clean.into_iter();
        self.data.profile = clean.next().unwrap();
        self.data.info = clean.next().unwrap();
        self.data.game = clean.next().unwrap();
        self.images = refreshed;
        self.settings_originals.clear();
        self.settings_edits.clear();
        self.replacements = Default::default();
        self.undo.clear();
        self.redo.clear();
        self.reload_required = false;
        Ok(SaveOutcome::Saved {
            backup,
            changed_files: written.len(),
        })
    }
}

fn pending_backup(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name.to_string_lossy().starts_with(PENDING_BACKUP))
}

fn prepare(path: &Path, bytes: &[u8]) -> Result<tempfile::NamedTempFile> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Missing("destination folder".into()))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    Ok(temporary)
}
fn replace(path: &Path, bytes: &[u8]) -> Result<()> {
    prepare(path, bytes)?
        .persist(path)
        .map_err(|error| Error::Io(error.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const PROFILE: &str =
        "SiiNunit\n{\nuser_profile : profile.test {\n profile_name: \"Example\"\n}\n}\n";
    const INFO: &str =
        "SiiNunit\n{\nsave_container : info.test {\n version: 105\n name: \"Example save\"\n}\n}\n";
    const GAME: &str = "SiiNunit\n{\neconomy : economy.test {\n player: player.test\n bank: bank.test\n experience_points: 42\n}\nplayer : player.test {\n hq_city: \"berlin\"\n}\nbank : bank.test {\n money_account: 100\n}\n}\n";
    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("profile/save/1");
        fs::create_dir_all(&folder).unwrap();
        fs::write(root.path().join("profile/profile.sii"), PROFILE).unwrap();
        fs::write(folder.join("info.sii"), INFO).unwrap();
        fs::write(folder.join("game.sii"), GAME).unwrap();
        (root, folder.join("game.sii"))
    }
    fn stage(session: &mut SaveSession) {
        session
            .edit(|state| state.game.set("bank.test", "money_account", "200"))
            .unwrap();
    }
    #[test]
    fn recover_broken_slot_to_separate_profile_with_original_settings() {
        let (root, path) = fixture();
        let source = root.path().join("profile");
        fs::write(source.join("controls.sii"), b"original controls").unwrap();
        fs::write(source.join("unknown.data"), b"extension data").unwrap();
        let mut session = SaveSession::load(&path, Game::Ets2).unwrap();
        stage(&mut session);
        session
            .import_settings(&[
                crate::settings::SettingsFile {
                    name: "controls.sii".into(),
                    bytes: b"later controls".to_vec(),
                },
                crate::settings::SettingsFile {
                    name: "config.cfg".into(),
                    bytes: b"new".to_vec(),
                },
            ])
            .unwrap();
        let SaveOutcome::Saved { backup, .. } = session.save().unwrap() else {
            panic!()
        };
        fs::write(&path, b"damaged").unwrap();
        let slot = path.parent().unwrap();
        assert_eq!(
            recovery_snapshots(slot).unwrap(),
            std::slice::from_ref(&backup)
        );
        let recovered =
            crate::profile_files::recover_profile(slot, &backup, "Recovered", Game::Ets2).unwrap();
        let result = SaveSession::load(recovered, Game::Ets2).unwrap();
        assert_eq!(result.data.profile_name(), "Recovered");
        assert_eq!(
            result.data.game.value("bank.test", "money_account"),
            Some("100")
        );
        assert_eq!(
            fs::read(result.profile_folder().join("unknown.data")).unwrap(),
            b"extension data"
        );
        assert_eq!(
            fs::read(result.profile_folder().join("controls.sii")).unwrap(),
            b"original controls"
        );
        assert!(!result.profile_folder().join("config.cfg").exists());
        assert!(
            crate::profile_files::recover_profile(slot, &backup, "Recovered", Game::Ets2).is_err()
        );
        assert!(
            crate::profile_files::recover_profile(slot, slot, "Wrong snapshot", Game::Ets2)
                .is_err()
        );
        assert_eq!(fs::read(path).unwrap(), b"damaged");
        assert_eq!(
            fs::read(source.join("controls.sii")).unwrap(),
            b"later controls"
        );
    }

    #[test]
    fn unchanged_save_does_not_rewrite_or_create_backups() {
        let (_root, path) = fixture();
        let mut session = SaveSession::load(&path, Game::Ets2).unwrap();
        assert!(matches!(session.save().unwrap(), SaveOutcome::Unchanged));
        assert!(!path.parent().unwrap().join("tsset-backups").exists());
    }
    #[test]
    fn incomplete_recovery_snapshot_is_neither_listed_nor_restorable() {
        let (_root, path) = fixture();
        let mut session = SaveSession::load(&path, Game::Ets2).unwrap();
        let pending = session.save_folder().join("tsset-backups/.pending-test");
        fs::create_dir_all(&pending).unwrap();
        for (name, contents) in [
            ("profile.sii", PROFILE),
            ("info.sii", INFO),
            ("game.sii", GAME),
        ] {
            fs::write(pending.join(name), contents).unwrap();
        }
        assert!(session.recovery_snapshots().unwrap().is_empty());
        assert!(session.restore_recovery(&pending).is_err());
        assert!(!session.is_dirty());
    }
    #[test]
    fn complete_raw_backup_and_targeted_write() {
        let (_root, path) = fixture();
        let mut session = SaveSession::load(&path, Game::Ets2).unwrap();
        stage(&mut session);
        let SaveOutcome::Saved {
            backup,
            changed_files,
        } = session.save().unwrap()
        else {
            panic!("not saved")
        };
        assert_eq!(changed_files, 1);
        assert_eq!(
            fs::read(backup.join("profile.sii")).unwrap(),
            PROFILE.as_bytes()
        );
        assert_eq!(fs::read(backup.join("info.sii")).unwrap(), INFO.as_bytes());
        assert_eq!(fs::read(backup.join("game.sii")).unwrap(), GAME.as_bytes());
        assert_eq!(
            fs::read_to_string(path).unwrap(),
            GAME.replace("money_account: 100", "money_account: 200")
        );
        assert!(!session.data.is_dirty());
        assert!(!session.can_undo());
    }
    #[test]
    fn undo_redo_and_rejected_cross_document_edit() {
        let (_root, path) = fixture();
        let mut session = SaveSession::load(&path, Game::Ats).unwrap();
        stage(&mut session);
        assert!(session.undo());
        assert!(!session.data.is_dirty());
        assert_eq!(session.data.game.changes().count(), 0);
        assert!(session.redo());
        assert!(matches!(
            session.data.game.changes().collect::<Vec<_>>().as_slice(),
            [crate::document::FieldChange::Value {
                unit: "bank.test",
                field: "money_account",
                item: None,
                before: "100",
                after: "200",
            }]
        ));
        assert_eq!(
            session.data.game.value("bank.test", "money_account"),
            Some("200")
        );
        let old = session.data.profile.render().unwrap().into_owned();
        assert!(
            session
                .edit(|state| {
                    state
                        .profile
                        .set_string("profile.test", "profile_name", "Changed")?;
                    state.game.set("bank.test", "missing", "1")
                })
                .is_err()
        );
        assert_eq!(session.data.profile.render().unwrap(), old);
    }
    #[test]
    fn convoy_route_clear_preserves_shared_units_and_supports_undo_redo() {
        for game in [Game::Ets2, Game::Ats] {
            let (_root, path) = fixture();
            let source = GAME.replace("experience_points: 42", "experience_points: 42\n stored_gps_behind_waypoints: 1\n stored_gps_behind_waypoints[0]: waypoint.test\n stored_gps_ahead_waypoints: 1\n stored_gps_ahead_waypoints[0]: waypoint.test\n stored_gps_avoid_waypoints: 1\n stored_gps_avoid_waypoints[0]: waypoint.test\n future_route_link: waypoint.test")
                .replace("player : player.test", "gps_waypoint_storage : waypoint.test {\n nav_node_position: (1, 2, 3)\n direction: \"any\"\n future_field: 42\n}\nplayer : player.test");
            fs::write(&path, &source).unwrap();
            let mut session = SaveSession::load(&path, game).unwrap();
            session.edit(crate::convoy::clear_route).unwrap();
            for field in [
                "stored_gps_behind_waypoints",
                "stored_gps_ahead_waypoints",
                "stored_gps_avoid_waypoints",
            ] {
                assert!(
                    session
                        .data
                        .game
                        .array("economy.test", field)
                        .unwrap()
                        .is_empty()
                );
            }
            assert_eq!(
                session.data.game.value("waypoint.test", "future_field"),
                Some("42")
            );
            assert_eq!(
                session.data.game.value("economy.test", "future_route_link"),
                Some("waypoint.test")
            );
            let cleared = session.data.game.render().unwrap().into_owned();
            assert!(session.undo());
            assert_eq!(session.data.game.render().unwrap(), source);
            assert!(session.redo());
            assert_eq!(session.data.game.render().unwrap(), cleared);
            assert_eq!(fs::read_to_string(&path).unwrap(), source);
        }
    }
    #[test]
    fn convoy_route_clear_rejects_partial_route_data_without_staging_changes() {
        let (_root, path) = fixture();
        let source = GAME.replace("experience_points: 42", "experience_points: 42\n stored_gps_behind_waypoints: 0\n stored_gps_ahead_waypoints: 0");
        fs::write(&path, &source).unwrap();
        let mut session = SaveSession::load(&path, Game::Ets2).unwrap();
        assert!(session.edit(crate::convoy::clear_route).is_err());
        assert!(!session.can_undo());
        assert!(!session.data.is_dirty());
        assert_eq!(session.data.game.render().unwrap(), source);
    }
    #[test]
    fn changed_content_and_backwards_timestamp_are_rejected() {
        let (_root, path) = fixture();
        let mut session = SaveSession::load(&path, Game::Ets2).unwrap();
        stage(&mut session);
        let before = fs::metadata(&path).unwrap().modified().unwrap();
        fs::write(
            &path,
            GAME.replace("money_account: 100", "money_account: 999"),
        )
        .unwrap();
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(before))
            .unwrap();
        assert!(matches!(session.save(), Err(Error::FileChanged)));
        assert!(session.reload_required());
        assert!(!session.can_save());
        assert!(session.is_dirty());
        assert!(
            fs::read_to_string(&path)
                .unwrap()
                .contains("money_account: 999")
        );
        let mut session = SaveSession::load(&path, Game::Ets2).unwrap();
        stage(&mut session);
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(
                fs::FileTimes::new().set_modified(before - std::time::Duration::from_secs(3600)),
            )
            .unwrap();
        assert!(matches!(session.save(), Err(Error::FileChanged)));
    }
    #[test]
    fn deleted_save_requires_reload_and_preserves_staged_changes() {
        let (_root, path) = fixture();
        let mut session = SaveSession::load(&path, Game::Ets2).unwrap();
        stage(&mut session);
        fs::remove_file(&path).unwrap();
        assert!(matches!(session.save(), Err(Error::FileChanged)));
        assert!(session.reload_required());
        assert!(session.is_dirty());
        assert!(!path.exists());
        assert!(!session.save_folder().join("tsset-backups").exists());
    }
    #[cfg(windows)]
    #[test]
    fn locked_destination_rolls_back_earlier_file_and_keeps_full_recovery() {
        use std::os::windows::fs::OpenOptionsExt;
        let (root, path) = fixture();
        let mut session = SaveSession::load(&path, Game::Ets2).unwrap();
        session
            .edit(|state| {
                state
                    .profile
                    .set_string("profile.test", "profile_name", "Changed")?;
                state.game.set("bank.test", "money_account", "200")
            })
            .unwrap();
        let _lock = OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .unwrap();
        assert!(matches!(
            session.save(),
            Err(Error::Commit { restored: true, .. })
        ));
        assert_eq!(
            fs::read_to_string(root.path().join("profile/profile.sii")).unwrap(),
            PROFILE
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), GAME);
        assert!(!session.can_save());
        let snapshots: Vec<_> = fs::read_dir(path.parent().unwrap().join("tsset-backups"))
            .unwrap()
            .collect();
        assert_eq!(snapshots.len(), 1);
    }
}
