//! Definition-derived progression data, loaded without extracting game archives.
use crate::{
    Error, Result, definitions::Catalog, editor::VehicleKind, profile::XpCurve, session::Game,
};
use scs_codec::{Archive, archive::Key, cityhash::cityhash64, tobj};
use std::{collections::BTreeMap, path::Path};

const TRUCK_LINKS: [&str; 5] = [
    "long_distance_skill_data",
    "valuable_cargo_skill_data",
    "fragile_cargo_skill_data",
    "urgent_job_skill_data",
    "ecodriving_skill_data",
];
const COACH_LINKS: [&str; 6] = [
    "long_duty_skill_data",
    "tight_schedule_skill_data",
    "satisfaction_decrease_skill_data",
    "low_acceleration_skill_data",
    "satisfaction_increase_skill_data",
    "bus_ecodriving_skill_data",
];

#[derive(Clone, Debug)]
pub struct Progression {
    pub plate_assets: PlateAssets,
    pub truck_xp: XpCurve,
    pub coach_xp: Option<XpCurve>,
    pub truck_ranks: [u8; 5],
    pub adr_ranks: u8,
    pub coach_ranks: Option<[u8; 6]>,
    pub definition_files: usize,
}

fn rank(catalog: &Catalog, link: &str) -> Result<u8> {
    let id = catalog.unit("skills.data")?.raw(link)?;
    let unit = catalog.unit(id)?;
    let rank = unit.integer("max_rank")?;
    if unit.kind != "skill_data" || rank == 0 || rank > 32 {
        return Err(Error::InvalidDefinition(
            "Invalid skill definition type or rank count.".into(),
        ));
    }
    Ok(rank)
}

impl Progression {
    pub fn from_catalog(game: Game, catalog: &Catalog) -> Result<Self> {
        let mut truck_ranks = [0; 5];
        for (rank_value, link) in truck_ranks.iter_mut().zip(TRUCK_LINKS) {
            *rank_value = rank(catalog, link)?;
        }
        let coach_ranks = if game == Game::Ets2 {
            let mut ranks = [0; 6];
            for (rank_value, link) in ranks.iter_mut().zip(COACH_LINKS) {
                *rank_value = rank(catalog, link)?;
            }
            Some(ranks)
        } else {
            None
        };
        Ok(Self {
            plate_assets: PlateAssets::default(),
            truck_xp: XpCurve::from_definitions(catalog, "economy.data.xp_levels")?,
            coach_xp: if game == Game::Ets2 {
                Some(XpCurve::from_definitions(
                    catalog,
                    "bus_economy.data.xp_levels",
                )?)
            } else {
                None
            },
            truck_ranks,
            adr_ranks: rank(catalog, "adr_skill_data")?,
            coach_ranks,
            definition_files: catalog.files.len(),
        })
    }
    pub fn load(game: Game, folder: &Path) -> Result<Self> {
        let (windows_executable, linux_executable) = match game {
            Game::Ets2 => ("eurotrucks2.exe", "eurotrucks2"),
            Game::Ats => ("amtrucks.exe", "amtrucks"),
        };
        let executable_paths = [
            ("bin/win_x64", windows_executable),
            ("bin/win_x86", windows_executable),
            ("bin/linux_x64", linux_executable),
        ];
        if !executable_paths
            .iter()
            .any(|(directory, executable)| folder.join(directory).join(executable).is_file())
        {
            return Err(Error::InvalidDefinition("Choose the selected game's installation folder (the folder containing def.scs and bin).".into()));
        }
        let archive = scs_codec::Archive::open(folder.join("def.scs")).map_err(|error| {
            Error::InvalidDefinition(format!("could not open game definition archive: {error}"))
        })?;
        let mut files = vec!["def/economy_data_xp.sii", "def/skill_data.sii"];
        if game == Game::Ets2 {
            files.push("def/bus_economy_data_xp.sii");
        }
        let mut data = Self::from_catalog(game, &Catalog::from_archive(&archive, &files)?)?;
        data.plate_assets = PlateAssets::load(folder)?;
        Ok(data)
    }
}

#[derive(Clone, Debug, Default)]
pub struct PlateAssets {
    pub regions: Vec<String>,
    pub badges: Vec<String>,
    backgrounds: BTreeMap<String, PlateBackground>,
}

#[derive(Clone, Debug)]
pub struct PlateBackground {
    pub size: [usize; 2],
    pub rgba: Vec<u8>,
}

const PLATE_ROOT: &str = "material/ui/lp";
const MAX_PLATE_ART_BYTES: usize = 16 * 1024 * 1024;

impl PlateAssets {
    pub fn background(&self, region: &str, kind: VehicleKind) -> Option<&PlateBackground> {
        let front = match kind {
            VehicleKind::Truck => "truck_front.mat",
            VehicleKind::Trailer => "trailer.mat",
            VehicleKind::Coach | VehicleKind::Car => "front.mat",
        };
        self.backgrounds
            .get(&format!("{PLATE_ROOT}/{region}/{front}"))
            .or_else(|| {
                self.backgrounds
                    .get(&format!("{PLATE_ROOT}/{region}/front.mat"))
            })
    }

    pub fn load(folder: &Path) -> Result<Self> {
        let archive = scs_codec::Archive::open(folder.join("base.scs")).map_err(|error| {
            Error::InvalidDefinition(format!("could not open plate artwork archive: {error}"))
        })?;
        let mut queue = vec![(PLATE_ROOT.to_owned(), 0usize)];
        let mut seen = std::collections::BTreeSet::new();
        let mut output = Self::default();
        let mut art_bytes = 0;
        while let Some((path, depth)) = queue.pop() {
            if seen.len() >= 4096 {
                return Err(Error::InvalidDefinition(
                    "Plate artwork exceeds its directory limit.".into(),
                ));
            }
            if !seen.insert(path.clone()) {
                continue;
            }
            let (dirs, files) = archive.list_directory(&path).map_err(|error| {
                Error::InvalidDefinition(format!("could not list plate artwork: {error}"))
            })?;
            if depth < 4 {
                for directory in dirs {
                    if directory.is_empty()
                        || directory.len() > 255
                        || queue.len() >= 4096
                        || directory.contains(['/', '\\'])
                        || directory == "."
                        || directory == ".."
                        || directory.chars().any(char::is_control)
                    {
                        return Err(Error::InvalidDefinition(
                            "Invalid plate artwork directory.".into(),
                        ));
                    }
                    if depth == 0 {
                        output.regions.push(directory.clone());
                    }
                    if path.len() + directory.len() + 1 > 1024 {
                        return Err(Error::InvalidDefinition(
                            "Plate artwork path exceeds its limit.".into(),
                        ));
                    }
                    queue.push((format!("{path}/{directory}"), depth + 1));
                }
            }
            for file in files {
                if file.contains(['/', '\\']) || file.chars().any(char::is_control) {
                    return Err(Error::InvalidDefinition(
                        "Invalid plate artwork file name.".into(),
                    ));
                }
                if file.ends_with(".mat") {
                    output.badges.push(format!("/{path}/{file}"));
                }
                if depth == 1
                    && matches!(
                        file.as_str(),
                        "front.mat" | "truck_front.mat" | "trailer.mat"
                    )
                    && art_bytes < MAX_PLATE_ART_BYTES
                {
                    let material = format!("{path}/{file}");
                    if let Some(background) = plate_background(&archive, &material)
                        && art_bytes + background.rgba.len() <= MAX_PLATE_ART_BYTES
                    {
                        art_bytes += background.rgba.len();
                        output.backgrounds.insert(material, background);
                    }
                }
                if output.badges.len() > 8192 {
                    return Err(Error::InvalidDefinition(
                        "Plate artwork exceeds its image limit.".into(),
                    ));
                }
            }
        }
        output.regions.sort();
        output.regions.dedup();
        output.badges.sort();
        output.badges.dedup();
        Ok(output)
    }
}

fn plate_background(archive: &Archive, material: &str) -> Option<PlateBackground> {
    let bytes = archive.read_path_limited(material, 4096).ok()?;
    let source = std::str::from_utf8(&bytes).ok()?.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        if key.trim() != "source" {
            return None;
        }
        value.trim().strip_prefix('"')?.strip_suffix('"')
    })?;
    let texture = if let Some(absolute) = source.strip_prefix('/') {
        absolute.to_owned()
    } else {
        format!("{}/{source}", material.rsplit_once('/')?.0)
    };
    if texture.len() > 1024
        || !texture.starts_with("material/ui/lp/")
        || !texture.ends_with(".tobj")
        || texture.contains(['\\', ':'])
        || texture
            .split('/')
            .any(|part| matches!(part, "" | "." | ".."))
    {
        return None;
    }
    let header = archive.image_header(Key::Hash(cityhash64(texture.as_bytes())))?;
    let mut meta = tobj::parse_image_meta(&header)?;
    if meta.is_cube || meta.width > 512 || meta.height > 256 {
        return None;
    }
    let payload = archive.read_path_limited(&texture, 1024 * 1024).ok()?;
    if !tobj::detect_format(&mut meta, payload.len())
        || !matches!(meta.dxgi, tobj::B8G8R8A8_UNORM | tobj::R8G8B8A8_UNORM)
    {
        return None;
    }
    let size = [meta.width as usize, meta.height as usize];
    let mut rgba = tobj::untile(&payload, &meta);
    let bytes = size[0] * size[1] * 4;
    if rgba.len() < bytes {
        return None;
    }
    rgba.truncate(bytes);
    if meta.dxgi == tobj::B8G8R8A8_UNORM {
        for pixel in rgba.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
    }
    Some(PlateBackground { size, rgba })
}
