//! Bounded, read-only profile and save discovery. No game commands are executed.
use crate::{Error, Result, decode_document, session::Game};
use std::{
    collections::{HashSet, VecDeque},
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone, Debug)]
pub struct SaveEntry {
    pub path: PathBuf,
    pub name: String,
    pub modified: Option<SystemTime>,
}
#[derive(Clone, Debug)]
pub struct Profile {
    pub path: PathBuf,
    pub name: String,
    pub source: String,
    pub avatar: String,
    pub saves: Vec<SaveEntry>,
}
#[derive(Default)]
pub struct Catalog {
    pub profiles: Vec<Profile>,
    pub unreadable: usize,
    pub truncated: bool,
}

pub fn read_small(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let file = File::open(path)?;
    if file.metadata()?.len() > limit as u64 {
        return Err(Error::InvalidSii("metadata exceeds the size limit"));
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(Error::InvalidSii("metadata exceeds the size limit"));
    }
    Ok(bytes)
}

fn command_executable(log: &str) -> Option<PathBuf> {
    let command = log.lines().find_map(|line| {
        line.split_once("Command line:")
            .map(|(_, command)| command.trim())
    })?;
    let end = command.to_ascii_lowercase().find(".exe")? + 4;
    let path = command[..end].trim_matches(['"', ' ']);
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}

fn defaults(game: Game) -> Vec<(PathBuf, String)> {
    let mut roots = default_roots(
        game,
        dirs::document_dir().as_deref(),
        std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .as_deref(),
        std::env::var_os("OneDrive").map(PathBuf::from).as_deref(),
        std::env::var_os("ProgramFiles(x86)")
            .map(PathBuf::from)
            .as_deref(),
    );
    if cfg!(any(target_os = "linux", target_os = "macos")) {
        roots.extend(unix_roots(
            game,
            dirs::home_dir().as_deref(),
            dirs::data_dir().as_deref(),
            cfg!(target_os = "linux"),
        ));
    }
    roots
}

fn steam_app_id(game: Game) -> &'static str {
    match game {
        Game::Ets2 => "227300",
        Game::Ats => "270880",
    }
}

fn steam_roots(game: Game, steam: &Path, roots: &mut Vec<(PathBuf, String)>) {
    roots.push((
        steam.join("steamapps/common").join(game.folder()),
        "Game-local".into(),
    ));
    if let Ok(entries) = fs::read_dir(steam.join("userdata")) {
        for entry in entries.flatten().take(128) {
            roots.push((
                entry.path().join(steam_app_id(game)).join("remote"),
                "Steam Cloud".into(),
            ));
        }
    }
}

fn unix_roots(
    game: Game,
    home: Option<&Path>,
    data: Option<&Path>,
    linux: bool,
) -> Vec<(PathBuf, String)> {
    let mut roots = Vec::new();
    let mut steam = Vec::new();
    if let Some(data) = data {
        roots.push((data.join(game.folder()), "Game data".into()));
        steam.push(data.join("Steam"));
    }
    if linux && let Some(home) = home {
        let flatpak = home.join(".var/app/com.valvesoftware.Steam");
        roots.push((
            home.join(".local/share").join(game.folder()),
            "Game data".into(),
        ));
        roots.push((
            flatpak.join("data").join(game.folder()),
            "Flatpak Steam".into(),
        ));
        steam.extend([
            home.join(".local/share/Steam"),
            home.join(".steam/steam"),
            flatpak.join("data/Steam"),
            flatpak.join(".local/share/Steam"),
        ]);
    }
    for path in steam {
        steam_roots(game, &path, &mut roots);
        if linux {
            roots.push((
                path.join("steamapps/compatdata")
                    .join(steam_app_id(game))
                    .join("pfx/drive_c/users/steamuser/Documents")
                    .join(game.folder()),
                "Steam Proton".into(),
            ));
        }
    }
    roots
}

fn default_roots(
    game: Game,
    documents: Option<&Path>,
    user: Option<&Path>,
    onedrive: Option<&Path>,
    programs: Option<&Path>,
) -> Vec<(PathBuf, String)> {
    let mut roots = Vec::new();
    let mut homes = Vec::new();
    if let Some(documents) = documents {
        homes.push((documents.join(game.folder()), "Documents"));
    }
    if let Some(user) = user {
        homes.push((user.join("Documents").join(game.folder()), "Documents"));
        homes.push((
            user.join("OneDrive/Documents").join(game.folder()),
            "OneDrive Documents",
        ));
    }
    if let Some(onedrive) = onedrive {
        homes.push((
            onedrive.join("Documents").join(game.folder()),
            "OneDrive Documents",
        ));
    }
    let mut seen = HashSet::new();
    homes.retain(|(path, _)| seen.insert(path.clone()));
    for (home, source) in homes {
        roots.push((home.clone(), source.into()));
        if let Ok(log) = read_small(&home.join("game.log.txt"), 4 * 1024 * 1024)
            && let Some(executable) = command_executable(&String::from_utf8_lossy(&log))
            && executable.is_file()
            && let Some(directory) = executable.parent()
        {
            roots.push((directory.to_path_buf(), "Game-local".into()));
            let name = directory
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_ascii_lowercase();
            let levels = if name == "win_x64" || name == "win_x86" {
                2
            } else if name == "bin" {
                1
            } else {
                0
            };
            for parent in directory.ancestors().skip(1).take(levels) {
                roots.push((parent.to_path_buf(), "Game-local".into()));
            }
        }
    }
    if let Some(programs) = programs {
        steam_roots(game, &programs.join("Steam"), &mut roots);
    }
    roots
}

pub fn discover(game: Game, custom: &[PathBuf], include_backups: bool) -> Catalog {
    let mut roots = defaults(game);
    roots.extend(custom.iter().cloned().map(|path| (path, "Custom".into())));
    discover_roots(&roots, include_backups)
}

pub fn discover_roots(roots: &[(PathBuf, String)], include_backups: bool) -> Catalog {
    let mut catalog = Catalog::default();
    let mut queue: VecDeque<_> = roots
        .iter()
        .map(|(path, label)| (path.clone(), label.clone(), 0usize))
        .collect();
    let mut seen = HashSet::new();
    let mut directories = 0;
    while let Some((path, source, depth)) = queue.pop_front() {
        if directories >= 6000 || catalog.profiles.len() >= 512 {
            catalog.truncated = true;
            break;
        }
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            continue;
        }
        let Ok(canonical) = path.canonicalize() else {
            continue;
        };
        if !seen.insert(canonical) {
            continue;
        }
        directories += 1;
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase();
        if name.starts_with(".truckcraft-") {
            continue;
        }
        if !include_backups && (name.ends_with(".bak") || name == "tsset-backups") {
            continue;
        }
        if path.join("profile.sii").is_file() && path.join("save").is_dir() {
            match read_profile(&path, &source) {
                Ok(profile) => catalog.profiles.push(profile),
                Err(_) => catalog.unreadable += 1,
            }
            continue;
        }
        if depth >= 4 {
            continue;
        }
        let Ok(entries) = fs::read_dir(&path) else {
            catalog.unreadable += 1;
            continue;
        };
        for entry in entries.flatten().take(2048) {
            if queue.len() >= 12000 {
                catalog.truncated = true;
                break;
            }
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if name == "tsset-backups" || name == "node_modules" || name == "target" {
                continue;
            }
            queue.push_back((entry.path(), source.clone(), depth + 1));
        }
    }
    catalog.profiles.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.path.cmp(&b.path))
    });
    catalog
}

fn read_profile(path: &Path, source: &str) -> Result<Profile> {
    let doc = decode_document(&read_small(&path.join("profile.sii"), 4 * 1024 * 1024)?)?;
    let id = doc.first_id("user_profile")?;
    let name = doc.string(id, "profile_name")?;
    let avatar = doc
        .string(id, "face")
        .or_else(|_| doc.string(id, "avatar"))
        .or_else(|_| doc.string(id, "avatar_path"))
        .unwrap_or_default();
    let mut saves = Vec::new();
    for entry in fs::read_dir(path.join("save"))?.flatten().take(2048) {
        let folder = entry.path();
        if !folder.join("game.sii").is_file() {
            continue;
        }
        let info = read_small(&folder.join("info.sii"), 4 * 1024 * 1024)
            .and_then(|bytes| decode_document(&bytes));
        let name = info
            .as_ref()
            .ok()
            .and_then(|doc| {
                doc.first_id("save_container")
                    .ok()
                    .and_then(|id| doc.string(id, "name").ok())
            })
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| {
                folder
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .replace('_', " ")
            });
        let modified = fs::metadata(folder.join("game.sii"))
            .ok()
            .and_then(|metadata| metadata.modified().ok());
        saves.push(SaveEntry {
            path: folder.join("game.sii"),
            name,
            modified,
        });
    }
    saves.sort_by(|a, b| {
        b.modified
            .cmp(&a.modified)
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(Profile {
        path: path.to_path_buf(),
        name,
        source: source.into(),
        avatar,
        saves,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn linux_and_macos_find_native_and_steam_game_homes() {
        let home = tempfile::tempdir().unwrap();
        let home = home.path();
        let linux = home.join(".local/share");
        let mac = home.join("Library/Application Support");
        for (game, id) in [(Game::Ets2, "227300"), (Game::Ats, "270880")] {
            let custom_xdg = home.join("redirected-data");
            for (data, is_linux) in [(&linux, true), (&custom_xdg, true), (&mac, false)] {
                let steam = data.join("Steam");
                let cloud = steam.join("userdata/sample").join(id).join("remote");
                fs::create_dir_all(&cloud).unwrap();
                let roots = unix_roots(game, Some(home), Some(data), is_linux);
                assert!(
                    roots
                        .iter()
                        .any(|(path, _)| path == &data.join(game.folder()))
                );
                assert!(
                    roots
                        .iter()
                        .any(|(path, source)| path == &cloud && source == "Steam Cloud")
                );
                let steam_install = steam.join("steamapps/common").join(game.folder());
                assert!(roots.iter().any(|(path, _)| path == &steam_install));
                let proton = steam
                    .join("steamapps/compatdata")
                    .join(id)
                    .join("pfx/drive_c/users/steamuser/Documents")
                    .join(game.folder());
                assert_eq!(roots.iter().any(|(path, _)| path == &proton), is_linux);
                if is_linux {
                    assert!(
                        roots
                            .iter()
                            .any(|(path, _)| path == &linux.join(game.folder()))
                    );
                    let flatpak_proton = home
                        .join(".var/app/com.valvesoftware.Steam/.local/share/Steam/steamapps/compatdata")
                        .join(id)
                        .join("pfx/drive_c/users/steamuser/Documents")
                        .join(game.folder());
                    assert!(roots.iter().any(|(path, _)| path == &flatpak_proton));
                }
            }
        }
    }
    #[test]
    fn default_locations_cover_redirected_local_onedrive_and_steam_homes() {
        let temporary = tempfile::tempdir().unwrap();
        let user = temporary.path().join("user");
        let redirected = temporary.path().join("redirected/Documents");
        let onedrive = temporary.path().join("custom-cloud");
        let programs = temporary.path().join("programs");
        for (game, appid) in [(Game::Ets2, "227300"), (Game::Ats, "270880")] {
            let cloud = programs
                .join("Steam/userdata/123")
                .join(appid)
                .join("remote");
            fs::create_dir_all(&cloud).unwrap();
            let profile = cloud.join("profiles/ABC");
            fs::create_dir_all(profile.join("save/1")).unwrap();
            fs::write(profile.join("profile.sii"), "SiiNunit\n{\nuser_profile : profile.test {\n profile_name: \"Cloud sample\"\n}\n}\n").unwrap();
            fs::write(
                profile.join("save/1/game.sii"),
                b"not read during discovery",
            )
            .unwrap();
            let roots = default_roots(
                game,
                Some(&redirected),
                Some(&user),
                Some(&onedrive),
                Some(&programs),
            );
            for home in [
                redirected.join(game.folder()),
                user.join("Documents").join(game.folder()),
                user.join("OneDrive/Documents").join(game.folder()),
                onedrive.join("Documents").join(game.folder()),
                programs.join("Steam/steamapps/common").join(game.folder()),
            ] {
                assert!(roots.iter().any(|(path, _)| path == &home));
            }
            let catalog = discover_roots(&roots, false);
            assert_eq!(catalog.profiles.len(), 1);
            assert_eq!(catalog.profiles[0].source, "Steam Cloud");
            assert_eq!(catalog.profiles[0].saves.len(), 1);
            assert_eq!(
                fs::read(profile.join("save/1/game.sii")).unwrap(),
                b"not read during discovery"
            );
        }
        let roots = default_roots(
            Game::Ets2,
            Some(&user.join("Documents")),
            Some(&user),
            None,
            None,
        );
        assert_eq!(
            roots
                .iter()
                .filter(|(path, _)| path == &user.join("Documents").join(Game::Ets2.folder()))
                .count(),
            1
        );
    }
    #[test]
    fn game_log_command_is_parsed_as_data_without_running_it() {
        assert_eq!(
            command_executable("00:00 Command line: \"D:\\Game Folder\\bin\\game.exe\" -silent"),
            Some(PathBuf::from("D:\\Game Folder\\bin\\game.exe"))
        );
        assert!(command_executable("arbitrary data").is_none());
    }
    #[test]
    fn bounded_discovery_uses_metadata_names_and_filters_backups() {
        let temporary = tempfile::tempdir().unwrap();
        for base in ["profiles", "profiles(1.62).bak"] {
            let path = temporary.path().join(base).join("ABC");
            fs::create_dir_all(path.join("save/1")).unwrap();
            fs::write(path.join("profile.sii"),"SiiNunit\n{\nuser_profile : profile.test {\n profile_name: \"Test profile\"\n face: \"/material/ui/driver/1.mat\"\n}\n}\n").unwrap();
            fs::write(
                path.join("save/1/info.sii"),
                "SiiNunit\n{\nsave_container : info.test {\n name: \"My journey\"\n}\n}\n",
            )
            .unwrap();
            fs::write(path.join("save/1/game.sii"), b"not read during discovery").unwrap();
        }
        let roots = [(temporary.path().to_path_buf(), "Test".into())];
        let catalog = discover_roots(&roots, false);
        assert_eq!(catalog.profiles.len(), 1);
        assert_eq!(catalog.profiles[0].name, "Test profile");
        assert_eq!(catalog.profiles[0].avatar, "/material/ui/driver/1.mat");
        assert_eq!(catalog.profiles[0].saves[0].name, "My journey");
        assert_eq!(discover_roots(&roots, true).profiles.len(), 2);
    }
}
