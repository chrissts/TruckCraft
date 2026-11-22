//! Recoverable profile rename/clone operations. No source file is rewritten.
use crate::{Error, Result, decode_document, discovery::read_small};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_FILE: u64 = 256 * 1024 * 1024;
const MAX_TOTAL: u64 = 16 * 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 20_000;
const RECOVERY: &str = ".truckcraft-profile-backups";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Rename,
    Clone { autosave_only: bool },
}
#[derive(Debug)]
pub struct Outcome {
    pub path: PathBuf,
    pub backup: Option<PathBuf>,
}

/// SCS identifies profile directories using the uppercase hex of UTF-8 bytes.
pub fn folder_name(name: &str) -> Result<String> {
    if name.trim() != name
        || name.is_empty()
        || name.chars().count() > 30
        || name.chars().any(char::is_control)
    {
        return Err(Error::InvalidEdit("Enter a profile name of 1–30 characters without control characters or surrounding spaces.".into()));
    }
    Ok(name
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect())
}

#[derive(Debug)]
struct SourceFile {
    relative: PathBuf,
    len: u64,
    modified: Option<SystemTime>,
    hash: [u8; 32],
}
struct Plan {
    files: Vec<SourceFile>,
    directories: Vec<PathBuf>,
}

fn ordinary(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    metadata.is_dir() || metadata.is_file()
}
fn same(file: &SourceFile, metadata: &fs::Metadata) -> bool {
    ordinary(metadata)
        && metadata.is_file()
        && metadata.len() == file.len
        && metadata.modified().ok() == file.modified
}
fn selected(relative: &Path, operation: Operation) -> bool {
    let parts: Vec<_> = relative.components().map(|part| part.as_os_str()).collect();
    if parts
        .iter()
        .any(|part| part.to_string_lossy().starts_with(".truckcraft-"))
    {
        return false;
    }
    if let Operation::Clone { autosave_only } = operation {
        if parts
            .iter()
            .any(|part| part.eq_ignore_ascii_case("tsset-backups"))
        {
            return false;
        }
        if autosave_only
            && parts
                .first()
                .is_some_and(|part| part.eq_ignore_ascii_case("save"))
            && parts.len() > 1
        {
            return parts[1].eq_ignore_ascii_case("autosave");
        }
    }
    true
}
fn collect_plan(source: &Path, operation: Operation) -> Result<Plan> {
    let mut queue = vec![(PathBuf::new(), 0)];
    let mut result = Plan {
        files: Vec::new(),
        directories: Vec::new(),
    };
    let mut total = 0_u64;
    while let Some((relative, depth)) = queue.pop() {
        if depth > 32 {
            return Err(Error::InvalidEdit(
                "Profile folder nesting exceeds the safety limit.".into(),
            ));
        }
        for entry in fs::read_dir(source.join(&relative))? {
            let entry = entry?;
            let child = relative.join(entry.file_name());
            if !selected(&child, operation) {
                continue;
            }
            let metadata = fs::symlink_metadata(entry.path())?;
            if !ordinary(&metadata) {
                return Err(Error::InvalidEdit(
                    "Profile copying does not follow symbolic links, junctions or special files."
                        .into(),
                ));
            }
            if result.files.len() + result.directories.len() >= MAX_ENTRIES {
                return Err(Error::InvalidEdit(
                    "Profile contains too many files or folders to copy safely.".into(),
                ));
            }
            if metadata.is_dir() {
                result.directories.push(child.clone());
                queue.push((child, depth + 1));
            } else {
                total = total
                    .checked_add(metadata.len())
                    .ok_or(Error::InvalidSii("profile byte count overflow"))?;
                if metadata.len() > MAX_FILE || total > MAX_TOTAL {
                    return Err(Error::InvalidEdit(
                        "Profile exceeds the supported copy size limits.".into(),
                    ));
                }
                result.files.push(SourceFile {
                    relative: child,
                    len: metadata.len(),
                    modified: metadata.modified().ok(),
                    hash: [0; 32],
                });
            }
        }
    }
    result
        .directories
        .sort_by_key(|path| path.components().count());
    result.files.sort_by(|a, b| a.relative.cmp(&b.relative));
    Ok(result)
}
fn copy_file(source: &Path, file: &SourceFile, destination: Option<&Path>) -> Result<[u8; 32]> {
    let source = source.join(&file.relative);
    if !same(file, &fs::symlink_metadata(&source)?) {
        return Err(Error::FileChanged);
    }
    let mut input = File::open(&source)?.take(file.len + 1);
    let mut output = destination
        .map(|path| {
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path.join(&file.relative))
        })
        .transpose()?;
    let mut digest = Sha256::new();
    let mut count = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        count += read as u64;
        if count > file.len {
            return Err(Error::FileChanged);
        }
        digest.update(&buffer[..read]);
        if let Some(output) = &mut output {
            output.write_all(&buffer[..read])?;
        }
    }
    if count != file.len || !same(file, &fs::symlink_metadata(&source)?) {
        return Err(Error::FileChanged);
    }
    if let Some(output) = output {
        output.sync_all()?;
    }
    Ok(digest.finalize().into())
}
fn preview(bytes: &[u8], new_folder: &str) -> Result<Vec<u8>> {
    if bytes.len() < 48 || bytes[..4] != [1, 10, 177, 112] {
        return Err(Error::InvalidEdit(
            "A save preview texture object has an unsupported header.".into(),
        ));
    }
    let length = u32::from_le_bytes(bytes[40..44].try_into().unwrap()) as usize;
    let old = bytes
        .get(
            48..48_usize
                .checked_add(length)
                .ok_or(Error::InvalidSii("preview path overflow"))?,
        )
        .ok_or(Error::InvalidSii("truncated preview path"))?;
    let old =
        std::str::from_utf8(old).map_err(|_| Error::InvalidSii("preview path is not UTF-8"))?;
    let (prefix, suffix) = old.split_once("/save/").ok_or_else(|| {
        Error::InvalidEdit("A save preview has an unsupported virtual path.".into())
    })?;
    let (root, _) = prefix
        .rsplit_once('/')
        .ok_or(Error::InvalidSii("invalid preview profile path"))?;
    if !root.starts_with("/home/") || !suffix.ends_with("/preview.tga") {
        return Err(Error::InvalidEdit(
            "A save preview has an unsupported virtual path.".into(),
        ));
    }
    let replacement = format!("{root}/{new_folder}/save/{suffix}");
    let mut output = bytes[..40].to_vec();
    output.extend_from_slice(&(replacement.len() as u32).to_le_bytes());
    output.extend_from_slice(&bytes[44..48]);
    output.extend_from_slice(replacement.as_bytes());
    output.extend_from_slice(&bytes[48 + length..]);
    Ok(output)
}

/// Rebuild one saved journey in a separate profile even when the current SII files cannot load.
pub fn recover_profile(
    slot: &Path,
    snapshot: &Path,
    name: &str,
    game: crate::session::Game,
) -> Result<PathBuf> {
    folder_name(name)?;
    let snapshot = crate::session::checked_snapshot(slot, snapshot)?;
    let slot = slot.canonicalize()?;
    let save_root = slot
        .parent()
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.eq_ignore_ascii_case("save"))
        })
        .ok_or(Error::InvalidSii(
            "choose a slot inside the profile save directory",
        ))?;
    let profile = save_root
        .parent()
        .ok_or(Error::InvalidSii("missing profile directory"))?;
    let parent = profile
        .parent()
        .ok_or(Error::InvalidSii("missing profiles directory"))?;
    let slot_name = slot
        .file_name()
        .ok_or(Error::InvalidSii("missing save slot name"))?;
    let relative_slot = Path::new("save").join(slot_name);
    let stage = tempfile::Builder::new()
        .prefix(".truckcraft-recover-")
        .tempdir_in(parent)?;
    let staged_slot = stage.path().join(&relative_slot);
    fs::create_dir_all(&staged_slot)?;
    let op = Operation::Clone {
        autosave_only: false,
    };
    let plan = collect_plan(profile, op)?;
    let settings = crate::session::snapshot_settings(&snapshot)?;
    for file in plan.files {
        let path = &file.relative;
        if path == Path::new("profile.sii")
            || (path.starts_with("save") && !path.starts_with(&relative_slot))
            || path == &relative_slot.join("game.sii")
            || path == &relative_slot.join("info.sii")
            || path
                .to_str()
                .is_some_and(|name| settings.contains_key(name))
        {
            continue;
        }
        let destination = stage.path().join(path);
        fs::create_dir_all(destination.parent().unwrap())?;
        copy_file(profile, &file, Some(stage.path()))?;
    }
    for (name, bytes) in settings {
        if let Some(bytes) = bytes {
            fs::write(stage.path().join(name), bytes)?;
        }
    }
    for (file, target) in [
        ("profile.sii", stage.path()),
        ("game.sii", staged_slot.as_path()),
        ("info.sii", staged_slot.as_path()),
    ] {
        let source = snapshot.join(file);
        crate::settings::ordinary(&source, false)?;
        fs::write(target.join(file), read_small(&source, MAX_FILE as usize)?)?;
    }
    crate::session::SaveSession::load(staged_slot.join("game.sii"), game)?;
    let outcome = copy_profile(stage.path(), name, op)?;
    Ok(outcome.path.join(relative_slot).join("game.sii"))
}

/// The destination exists only as an incomplete copy until profile.sii is
/// atomically published last. Existing destinations are never overwritten.
pub fn copy_profile(source: impl AsRef<Path>, name: &str, operation: Operation) -> Result<Outcome> {
    copy_profile_with_hook(source.as_ref(), name, operation, |_, _, _| {})
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Verify,
    Publish,
}
fn copy_profile_with_hook(
    source_input: &Path,
    name: &str,
    operation: Operation,
    mut hook: impl FnMut(Phase, &Path, &Path),
) -> Result<Outcome> {
    let new_folder = folder_name(name)?;
    if !ordinary(&fs::symlink_metadata(source_input)?) {
        return Err(Error::InvalidEdit(
            "Select an ordinary profile folder.".into(),
        ));
    }
    let source = source_input.canonicalize()?;
    let parent = source
        .parent()
        .ok_or(Error::InvalidSii("profile folder has no parent"))?;
    let destination = parent.join(&new_folder);
    if source == destination || destination.exists() {
        return Err(Error::InvalidEdit(
            "A profile with this name already exists.".into(),
        ));
    }
    let bytes = read_small(&source.join("profile.sii"), MAX_FILE as usize)?;
    let mut profile = decode_document(&bytes)?;
    let id = profile.first_id("user_profile")?.to_owned();
    profile.set_string(&id, "profile_name", name)?;
    if matches!(operation, Operation::Clone { .. }) && profile.value(&id, "creation_time").is_some()
    {
        profile.set(
            &id,
            "creation_time",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                .to_string(),
        )?;
    }
    let rendered = profile.render()?.as_bytes().to_vec();
    decode_document(&rendered)?;
    let mut plan = collect_plan(&source, operation)?;
    if !plan
        .files
        .iter()
        .any(|file| file.relative == Path::new("profile.sii"))
        || !plan.directories.iter().any(|dir| dir == Path::new("save"))
    {
        return Err(Error::InvalidEdit(
            "Select a profile containing profile.sii and a save folder.".into(),
        ));
    }
    if operation
        == (Operation::Clone {
            autosave_only: true,
        })
        && !source.join("save/autosave/game.sii").is_file()
    {
        return Err(Error::InvalidEdit(
            "This profile has no autosave to clone.".into(),
        ));
    }
    fs::create_dir(&destination)?;
    let mut cleanup = Incomplete {
        path: destination.clone(),
        committed: false,
    };
    for dir in &plan.directories {
        fs::create_dir(destination.join(dir))?;
    }
    for file in &mut plan.files {
        if file.relative == Path::new("profile.sii") {
            file.hash = Sha256::digest(&bytes).into();
            continue;
        }
        file.hash = copy_file(&source, file, Some(&destination))?;
        if file.relative.components().count() == 3
            && file.relative.starts_with("save")
            && file
                .relative
                .file_name()
                .is_some_and(|name| name == "preview.tobj")
        {
            let target = destination.join(&file.relative);
            let data = read_small(&target, 1024 * 1024)?;
            let patched = preview(&data, &new_folder)?;
            let mut output = OpenOptions::new().write(true).truncate(true).open(target)?;
            output.write_all(&patched)?;
            output.sync_all()?;
        }
    }
    hook(Phase::Verify, &source, &destination);
    let current = collect_plan(&source, operation)?;
    if current.directories != plan.directories
        || current.files.len() != plan.files.len()
        || current
            .files
            .iter()
            .zip(&plan.files)
            .any(|(a, b)| a.relative != b.relative || a.len != b.len || a.modified != b.modified)
    {
        return Err(Error::FileChanged);
    }
    for file in &plan.files {
        if copy_file(&source, file, None)? != file.hash {
            return Err(Error::FileChanged);
        }
    }
    let mut pending = tempfile::NamedTempFile::new_in(&destination)?;
    pending.write_all(&rendered)?;
    pending.as_file().sync_all()?;
    let backup = if operation == Operation::Rename {
        let root = parent.join(RECOVERY);
        fs::create_dir_all(&root)?;
        if !ordinary(&fs::symlink_metadata(&root)?) {
            return Err(Error::InvalidEdit(
                "Profile recovery folder is not an ordinary directory.".into(),
            ));
        }
        let container = tempfile::Builder::new()
            .prefix("rename-")
            .tempdir_in(&root)?
            .keep();
        let backup = container.join(source.file_name().unwrap());
        fs::rename(&source, &backup)?;
        Some(backup)
    } else {
        None
    };
    let mut restore_on_panic = RestoreOnPanic {
        source: source.clone(),
        backup: backup.clone(),
        active: true,
    };
    hook(Phase::Publish, &source, &destination);
    if let Err(error) = pending.persist_noclobber(destination.join("profile.sii")) {
        if let Some(backup) = &backup
            && let Err(restore) = fs::rename(backup, &source)
        {
            restore_on_panic.active = false;
            return Err(Error::ProfileRecovery {
                backup: backup.clone(),
                reason: format!(
                    "Publishing failed: {}. Restoring the original location failed: {restore}",
                    error.error
                ),
            });
        }
        restore_on_panic.active = false;
        return Err(Error::Io(error.error));
    }
    restore_on_panic.active = false;
    cleanup.committed = true;
    Ok(Outcome {
        path: destination,
        backup,
    })
}
struct Incomplete {
    path: PathBuf,
    committed: bool,
}
impl Drop for Incomplete {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
struct RestoreOnPanic {
    source: PathBuf,
    backup: Option<PathBuf>,
    active: bool,
}
impl Drop for RestoreOnPanic {
    fn drop(&mut self) {
        if self.active
            && !self.source.exists()
            && let Some(backup) = &self.backup
        {
            let _ = fs::rename(backup, &self.source);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> (tempfile::TempDir, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join(folder_name("Original").unwrap());
        fs::create_dir_all(source.join("save/autosave")).unwrap();
        fs::create_dir_all(source.join("save/2")).unwrap();
        fs::create_dir_all(source.join("album")).unwrap();
        fs::write(source.join("profile.sii"),b"SiiNunit\n{\nuser_profile : profile.test {\n profile_name: \"Original\"\n creation_time: 123\n future_data: \"keep me\"\n}\n}\n").unwrap();
        for slot in ["autosave", "2"] {
            let save = source.join("save").join(slot);
            fs::write(save.join("game.sii"), b"unchanged game data").unwrap();
            fs::write(save.join("info.sii"), b"unchanged save metadata").unwrap();
            let path = format!(
                "/home/profiles/{}/save/{slot}/preview.tga",
                folder_name("Original").unwrap()
            );
            let mut header = [0_u8; 40];
            header[..4].copy_from_slice(&[1, 10, 177, 112]);
            header[20] = 0x5a;
            let mut tobj = header.to_vec();
            tobj.extend_from_slice(&(path.len() as u32).to_le_bytes());
            tobj.extend_from_slice(&[0; 4]);
            tobj.extend_from_slice(path.as_bytes());
            fs::write(save.join("preview.tobj"), tobj).unwrap();
        }
        fs::write(source.join("controls.sii"), b"controller data").unwrap();
        fs::write(source.join("config.cfg"), b"config data").unwrap();
        fs::write(source.join("album/custom.data"), b"unknown album data").unwrap();
        (root, source)
    }
    #[test]
    fn cloning_preserves_unknown_files_and_repairs_preview_references() {
        let (_root, source) = sample();
        let original = fs::read(source.join("profile.sii")).unwrap();
        let outcome = copy_profile(
            &source,
            "New Å",
            Operation::Clone {
                autosave_only: false,
            },
        )
        .unwrap();
        assert!(outcome.backup.is_none());
        assert_eq!(fs::read(source.join("profile.sii")).unwrap(), original);
        let profile =
            decode_document(&fs::read(outcome.path.join("profile.sii")).unwrap()).unwrap();
        assert_eq!(
            profile.string("profile.test", "profile_name").unwrap(),
            "New Å"
        );
        assert_eq!(
            profile.value("profile.test", "future_data"),
            Some("\"keep me\"")
        );
        assert!(
            profile
                .integer::<u64>("profile.test", "creation_time")
                .unwrap()
                > 123
        );
        assert_eq!(
            fs::read(outcome.path.join("album/custom.data")).unwrap(),
            b"unknown album data"
        );
        assert_eq!(
            fs::read(outcome.path.join("save/2/game.sii")).unwrap(),
            b"unchanged game data"
        );
        let preview = fs::read(outcome.path.join("save/2/preview.tobj")).unwrap();
        assert_eq!(preview[20], 0x5a);
        assert_eq!(
            &preview[48..],
            format!(
                "/home/profiles/{}/save/2/preview.tga",
                folder_name("New Å").unwrap()
            )
            .as_bytes()
        );
    }
    #[test]
    fn renaming_retains_complete_original_recovery_and_hides_it_from_discovery() {
        let (root, source) = sample();
        let original = fs::read(source.join("profile.sii")).unwrap();
        let result = copy_profile(&source, "Renamed", Operation::Rename).unwrap();
        assert!(!source.exists());
        let backup = result.backup.unwrap();
        assert_eq!(fs::read(backup.join("profile.sii")).unwrap(), original);
        assert_eq!(
            fs::read(backup.join("controls.sii")).unwrap(),
            b"controller data"
        );
        let profile = decode_document(&fs::read(result.path.join("profile.sii")).unwrap()).unwrap();
        assert_eq!(
            profile
                .integer::<u64>("profile.test", "creation_time")
                .unwrap(),
            123
        );
        let catalog =
            crate::discovery::discover_roots(&[(root.path().to_path_buf(), "Test".into())], true);
        assert_eq!(catalog.profiles.len(), 1);
        assert_eq!(catalog.profiles[0].name, "Renamed");
    }
    #[test]
    fn autosave_clone_and_destination_collision_are_explicit() {
        let (_root, source) = sample();
        let result = copy_profile(
            &source,
            "One save",
            Operation::Clone {
                autosave_only: true,
            },
        )
        .unwrap();
        assert!(result.path.join("save/autosave/game.sii").is_file());
        assert!(!result.path.join("save/2").exists());
        assert!(copy_profile(&source, "One save", Operation::Rename).is_err());
        assert!(source.join("profile.sii").is_file());
        assert!(
            copy_profile(
                &source,
                "Original",
                Operation::Clone {
                    autosave_only: false
                }
            )
            .is_err()
        );
        for name in ["", " trailing ", "bad\nname", &"a".repeat(31)] {
            assert!(folder_name(name).is_err());
        }
        assert_eq!(folder_name("Å").unwrap(), "C385");
    }
    #[test]
    fn source_changes_abort_without_publishing_a_partial_profile() {
        for added in [false, true] {
            let (root, source) = sample();
            let result = copy_profile_with_hook(
                &source,
                "Concurrent",
                Operation::Rename,
                |phase, source, _| {
                    if phase == Phase::Verify {
                        fs::write(
                            source.join(if added { "new.file" } else { "controls.sii" }),
                            b"external change",
                        )
                        .unwrap();
                    }
                },
            );
            assert!(matches!(result, Err(Error::FileChanged)));
            assert!(source.join("profile.sii").is_file());
            assert!(
                !root
                    .path()
                    .join(folder_name("Concurrent").unwrap())
                    .exists()
            );
        }
    }
    #[test]
    fn failed_publication_restores_original_location() {
        let (root, source) = sample();
        let original = fs::read(source.join("profile.sii")).unwrap();
        let result = copy_profile_with_hook(
            &source,
            "Blocked publish",
            Operation::Rename,
            |phase, _, destination| {
                if phase == Phase::Publish {
                    fs::write(destination.join("profile.sii"), b"injected collision").unwrap();
                }
            },
        );
        assert!(result.is_err());
        assert_eq!(fs::read(source.join("profile.sii")).unwrap(), original);
        assert!(
            !root
                .path()
                .join(folder_name("Blocked publish").unwrap())
                .exists()
        );
    }
    #[test]
    fn malformed_preview_aborts_and_never_moves_the_original() {
        let (root, source) = sample();
        fs::write(source.join("save/2/preview.tobj"), b"bad header").unwrap();
        assert!(copy_profile(&source, "Malformed", Operation::Rename).is_err());
        assert!(source.join("profile.sii").is_file());
        assert!(!root.path().join(folder_name("Malformed").unwrap()).exists());
    }
    #[test]
    fn panic_after_archiving_restores_original_and_removes_incomplete_copy() {
        let (root, source) = sample();
        let result = std::panic::catch_unwind(|| {
            copy_profile_with_hook(&source, "Panicked", Operation::Rename, |phase, _, _| {
                if phase == Phase::Publish {
                    panic!("injected publication panic");
                }
            })
        });
        assert!(result.is_err());
        assert!(source.join("profile.sii").is_file());
        assert!(!root.path().join(folder_name("Panicked").unwrap()).exists());
    }
}
