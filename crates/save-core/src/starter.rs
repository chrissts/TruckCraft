//! Create a local profile from the starter state in the user's installed simulator.
use crate::{
    Error, Result, decode_document, profile_files,
    session::{Game, SAVE_FORMAT_TARGET, SaveSession},
    settings::ordinary,
    value::quote,
};
use scs_codec::Archive;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_STARTER_FILE: usize = 64 * 1024;
const STARTER: &str = "def/initial_save/normal/";
const LOCKED_SCREENS: &str = "@include \"locked_screens.sui\"";

fn archive(path: &Path) -> Result<Archive> {
    Archive::open(path).map_err(|error| Error::InvalidDefinition(error.to_string()))
}

fn archive_text(archive: &Archive, path: &str) -> Result<String> {
    let bytes = archive
        .read_path_limited(path, MAX_STARTER_FILE)
        .map_err(|error| Error::InvalidDefinition(format!("{path}: {error}")))?;
    String::from_utf8(bytes).map_err(|_| Error::InvalidSii("starter data is not UTF-8"))
}

fn expand(starter: &str, screens: &str) -> Result<String> {
    let mut expanded = String::new();
    let mut included = false;
    let mut lines = starter.lines().peekable();
    while let Some(line) = lines.next() {
        if line.trim().starts_with('@') {
            if line.trim() != LOCKED_SCREENS
                || included
                || screens
                    .lines()
                    .any(|line| line.trim_start().starts_with('@'))
            {
                return Err(Error::InvalidSii("unsupported starter save directive"));
            }
            included = true;
            expanded.push_str(screens);
        } else {
            expanded.push_str(line);
            // The archive permits a unit's opening brace on the following line.
            if line.contains(':') && lines.peek().is_some_and(|line| line.trim() == "{") {
                lines.next();
                expanded.push_str(" {");
            }
        }
        expanded.push('\n');
    }
    if !included {
        return Err(Error::InvalidSii("starter data has no locked screen list"));
    }
    Ok(expanded)
}

fn starter_truck(game: Game, installation: &Path, defs: &Archive) -> Result<&'static str> {
    const EUROPEAN: &[(&str, &str)] = &[("def.scs", "scania_r_2016"), ("def.scs", "daf")];
    const AMERICAN: &[(&str, &str)] = &[
        ("dlc_kenworth_t680.scs", "kenworth_t680"),
        ("dlc_peterbilt_579.scs", "peterbilt_579"),
        ("dlc_kenworth_t680_2022.scs", "kenworth_t680_2022"),
        ("dlc_kenworth_w900.scs", "kenworth_w900"),
    ];
    let models = match game {
        Game::Ets2 => EUROPEAN,
        Game::Ats => AMERICAN,
    };
    for &(pack, model) in models {
        let file = format!("def/vehicle/truck_desktop/{model}.sii");
        let valid = |archive: &Archive| {
            archive
                .read_path_limited(&file, MAX_STARTER_FILE)
                .is_ok_and(|bytes| !bytes.is_empty())
        };
        let present = if pack == "def.scs" {
            valid(defs)
        } else {
            archive(&installation.join(pack)).is_ok_and(|archive| valid(&archive))
        };
        if present {
            return Ok(model);
        }
    }
    Err(Error::InvalidDefinition(
        "No supported starter truck model was found. Verify the installed game files and, for ATS, install one of the free Kenworth T680, Kenworth W900, or Peterbilt 579 truck packs.".into(),
    ))
}

pub fn create(game: Game, installation: &Path, game_home: &Path, name: &str) -> Result<PathBuf> {
    let folder = profile_files::folder_name(name)?;
    let (application, map) = match game {
        Game::Ets2 => ("eut2", "/map/europe.mbd"),
        Game::Ats => ("ats", "/map/usa.mbd"),
    };
    let version_file = archive_text(&archive(&installation.join("version.scs"))?, "version.sii")?;
    let version_doc = decode_document(version_file.as_bytes())?;
    let pack = version_doc.first_id("fs_pack_set")?;
    let version = version_doc.string(pack, "version")?;
    if version_doc.required(pack, "application")? != application || !version.starts_with("1.62.") {
        return Err(Error::InvalidDefinition(format!(
            "Select an installed copy of {} 1.62.x.",
            game.name()
        )));
    }
    let defs = archive(&installation.join("def.scs"))?;
    let brand = starter_truck(game, installation, &defs)?;
    let starter = archive_text(&defs, &format!("{STARTER}game.sii"))?;
    let screens = archive_text(&defs, &format!("{STARTER}locked_screens.sui"))?;
    let starter = expand(&starter, &screens)?;
    let game_doc = decode_document(starter.as_bytes())?;
    let economy = game_doc.first_id("economy")?;
    if game_doc.required(economy, "new_game")? != "true" {
        return Err(Error::InvalidSii(
            "the installed starter save is not a new game",
        ));
    }
    let bank = game_doc.required(economy, "bank")?;
    let balance: i64 = game_doc.integer(bank, "money_account")?;
    let time: u32 = game_doc.integer(economy, "game_time")?;

    ordinary(game_home, true)?;
    if !game_home
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case(game.folder()))
    {
        return Err(Error::InvalidEdit(format!(
            "Choose the local {} data folder.",
            game.folder()
        )));
    }
    let profiles = game_home.join("profiles");
    match fs::create_dir(&profiles) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            ordinary(&profiles, true)?
        }
        Err(error) => return Err(error.into()),
    }
    if profiles.join(folder).exists() {
        return Err(Error::InvalidEdit(
            "A profile with this name already exists.".into(),
        ));
    }

    let staging = tempfile::Builder::new()
        .prefix(".truckcraft-starter-")
        .tempdir_in(&profiles)?;
    let slot = staging.path().join("save/1");
    fs::create_dir_all(&slot)?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::InvalidEdit("The system clock is before 1970.".into()))?
        .as_secs();
    let profile_name = quote(name);
    fs::write(
        staging.path().join("profile.sii"),
        format!(
            "SiiNunit\n{{\nuser_profile : _nameless.1 {{\n version: 6\n creation_time: {timestamp}\n save_time: {timestamp}\n profile_name: {profile_name}\n company_name: {profile_name}\n map_path: \"{map}\"\n male: true\n face: 1\n logo: logo_0\n brand: {brand}\n active_mods: 0\n}}\n}}\n"
        ),
    )?;
    fs::write(
        slot.join("info.sii"),
        format!(
            "SiiNunit\n{{\nsave_container : _nameless.2 {{\n name: \"Fresh starter\"\n version: {SAVE_FORMAT_TARGET}\n time: {time}\n file_time: {timestamp}\n info_version: 1\n dependencies: 0\n info_players_experience: 0\n info_money_account: {balance}\n}}\n}}\n"
        ),
    )?;
    fs::write(slot.join("game.sii"), starter)?;
    SaveSession::load(slot.join("game.sii"), game)?;
    let published = profile_files::copy_profile(
        staging.path(),
        name,
        profile_files::Operation::Clone {
            autosave_only: false,
        },
    )?;
    Ok(published.path.join("save/1/game.sii"))
}
