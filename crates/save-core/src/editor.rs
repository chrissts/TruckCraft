//! Typed edits against preserved fields rather than obsolete save models.
use crate::{Error, Result, document::Document, session::SaveData, value};
use std::collections::BTreeSet;
pub const TRUCK_SKILLS: [&str; 5] = ["long_dist", "heavy", "fragile", "urgent", "mechanical"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VehicleKind {
    Truck,
    Coach,
    Car,
    Trailer,
}
#[derive(Clone, Debug)]
pub struct Entity {
    pub id: String,
    pub name: String,
}

pub fn set_progress(data: &mut SaveData, xp: u32, adr: u8, skills: [u8; 5]) -> Result<()> {
    if xp > i32::MAX as u32 || adr > 63 || skills.iter().any(|rank| *rank > 6) {
        return Err(Error::InvalidEdit("XP must fit a signed integer, truck skills must be 0–6 and ADR must be a six-bit mask.".into()));
    }
    let economy = data.economy_id()?.to_owned();
    data.game.transaction(|doc| {
        doc.set(&economy, "experience_points", xp.to_string())?;
        doc.set(&economy, "adr", adr.to_string())?;
        for (key, rank) in TRUCK_SKILLS.iter().zip(skills) {
            doc.set(&economy, key, rank.to_string())?;
        }
        Ok(())
    })
}
fn validate_company_name(name: &str) -> Result<()> {
    if name.chars().count() > 20 {
        return Err(Error::InvalidEdit(
            "Company name can contain at most 20 characters.".into(),
        ));
    }
    Ok(())
}

pub fn set_company_name(data: &mut SaveData, name: &str) -> Result<()> {
    validate_company_name(name)?;
    let profile = data.profile_id()?.to_owned();
    data.profile.set_string(&profile, "company_name", name)
}

pub fn set_balance(data: &mut SaveData, money: i64) -> Result<()> {
    let bank = data.bank_id()?.to_owned();
    data.game.set(&bank, "money_account", money.to_string())
}

pub fn set_company(data: &mut SaveData, money: i64, name: &str) -> Result<()> {
    validate_company_name(name)?;
    let bank = data.bank_id()?.to_owned();
    let profile = data.profile_id()?.to_owned();
    let mut staged_profile = data.profile.clone();
    let mut staged_game = data.game.clone();
    staged_profile.set_string(&profile, "company_name", name)?;
    staged_game.set(&bank, "money_account", money.to_string())?;
    data.profile = staged_profile;
    data.game = staged_game;
    Ok(())
}
pub fn fleet(data: &SaveData, kind: VehicleKind) -> Result<Vec<Entity>> {
    if kind == VehicleKind::Car && data.simulator != crate::session::Game::Ats {
        return Ok(Vec::new());
    }
    let doc = &data.game;
    let player = data.player_id()?;
    let field = match kind {
        VehicleKind::Truck => "trucks",
        VehicleKind::Coach => "buses",
        VehicleKind::Car => "cars",
        VehicleKind::Trailer => "trailers",
    };
    let mut ids: BTreeSet<String> = doc
        .array_or_empty(player, field)?
        .into_iter()
        .map(str::to_owned)
        .collect();
    let job_field = match kind {
        VehicleKind::Coach => "current_bus_job",
        VehicleKind::Car => "current_car_job",
        _ => "current_job",
    };
    if let Some(job) = doc.value(player, job_field) {
        let field = match kind {
            VehicleKind::Truck => "company_truck",
            VehicleKind::Coach => "company_bus",
            VehicleKind::Car => "company_car",
            VehicleKind::Trailer => "company_trailer",
        };
        if let Some(id) = doc.value(job, field) {
            ids.insert(id.into());
        }
    }
    if let Some(container) = doc.value(player, "assigned_vehicles") {
        let mode = doc
            .value(player, "assigned_vehicles_mode")
            .and_then(|mode| value::unquote(mode).ok());
        let field = if kind == VehicleKind::Trailer {
            "trailer"
        } else {
            "vehicle"
        };
        let matching = match kind {
            VehicleKind::Truck => mode.as_deref() == Some("truck"),
            VehicleKind::Coach => mode.as_deref() == Some("bus"),
            VehicleKind::Car => mode.as_deref() == Some("car"),
            VehicleKind::Trailer => true,
        };
        if matching && let Some(id) = doc.value(container, field) {
            ids.insert(id.into());
        }
    }
    let legacy = if kind == VehicleKind::Trailer {
        "assigned_trailer"
    } else {
        "assigned_truck"
    };
    if matches!(kind, VehicleKind::Truck | VehicleKind::Trailer)
        && let Some(id) = doc.value(player, legacy)
    {
        ids.insert(id.into());
    }
    Ok(ids
        .into_iter()
        .filter(|id| {
            doc.unit(id).is_some()
                && (kind == VehicleKind::Trailer || doc.value(id, "fuel_relative").is_some())
        })
        .map(|id| Entity {
            name: vehicle_name(doc, &id, kind),
            id,
        })
        .collect())
}
pub fn vehicle_name(doc: &Document, id: &str, kind: VehicleKind) -> String {
    if let Ok(accessories) = doc.array_or_empty(id, "accessories") {
        let segment = match kind {
            VehicleKind::Truck => "/truck/",
            VehicleKind::Coach => "/bus/",
            VehicleKind::Car => "/car/",
            VehicleKind::Trailer => "/trailer/",
        };
        for accessory in accessories {
            if let Some(path) = doc
                .value(accessory, "data_path")
                .and_then(|raw| value::unquote(raw).ok())
                && let Some((_, tail)) = path.split_once(segment)
                && let Some(name) = tail.split('/').next()
            {
                return name.replace(['.', '_'], " ");
            }
        }
    }
    match kind {
        VehicleKind::Truck => "Truck",
        VehicleKind::Coach => "Coach",
        VehicleKind::Car => "Car",
        VehicleKind::Trailer => "Trailer",
    }
    .into()
}
pub(crate) fn require_vehicle(data: &SaveData, kind: VehicleKind, id: &str) -> Result<()> {
    if !fleet(data, kind)?.iter().any(|entity| entity.id == id) {
        return Err(Error::InvalidEdit(
            "Select an owned or active-job vehicle.".into(),
        ));
    }
    if kind == VehicleKind::Coach {
        crate::coaches::require_journey(data)?;
    }
    if kind == VehicleKind::Car {
        crate::cars::require_active(data)?;
    }
    Ok(())
}
pub fn fuel(data: &SaveData, id: &str) -> Result<f32> {
    Ok(value::float(data.game.required(id, "fuel_relative")?)? * 100.0)
}
pub fn set_fuel(data: &mut SaveData, kind: VehicleKind, id: &str, percent: f32) -> Result<()> {
    require_vehicle(data, kind, id)?;
    if !percent.is_finite() || !(0.0..=100.0).contains(&percent) {
        return Err(Error::InvalidEdit(
            "Fuel must be between 0% and 100%.".into(),
        ));
    }
    data.game
        .set(id, "fuel_relative", value::float_text(percent / 100.0)?)
}
pub fn repair(data: &mut SaveData, kind: VehicleKind, id: &str, permanent: bool) -> Result<usize> {
    require_vehicle(data, kind, id)?;
    let mut members = vec![id.to_owned()];
    if kind == VehicleKind::Trailer {
        let mut seen = BTreeSet::from([id.to_owned()]);
        let mut current = id.to_owned();
        while let Some(next) = data
            .game
            .value(&current, "slave_trailer")
            .filter(|raw| !value::is_null(raw))
        {
            if members.len() >= 64
                || !seen.insert(next.to_owned())
                || !data
                    .game
                    .unit(next)
                    .is_some_and(|unit| unit.kind.as_ref() == "trailer")
            {
                return Err(Error::InvalidEdit("The trailer chain is missing data, contains a cycle or exceeds its supported length.".into()));
            }
            members.push(next.to_owned());
            current = next.to_owned();
        }
    }
    data.game.transaction(|doc| {
        let mut changed = 0;
        for id in &members {
            for part in [
                "engine",
                "transmission",
                "cabin",
                "chassis",
                "body",
                "trailer_body",
            ] {
                for suffix in if permanent {
                    &["_wear", "_wear_unfixable"][..]
                } else {
                    &["_wear"][..]
                } {
                    let key = format!("{part}{suffix}");
                    if doc.value(id, &key).is_some() {
                        doc.set(id, &key, "0")?;
                        changed += 1;
                    }
                }
            }
            if doc.value(id, "cargo_damage").is_some() {
                doc.set(id, "cargo_damage", "0")?;
                changed += 1;
            }
            if permanent {
                for field in ["integrity_odometer", "integrity_odometer_float_part"] {
                    if doc.value(id, field).is_some() {
                        doc.set(id, field, "0")?;
                        changed += 1;
                    }
                }
            }
            for field in if permanent {
                &["wheels_wear", "wheels_wear_unfixable"][..]
            } else {
                &["wheels_wear"][..]
            } {
                if doc.value(id, field).is_some() {
                    let count = doc.array(id, field)?.len();
                    if count > 64 {
                        return Err(Error::InvalidEdit(
                            "The saved wheel array exceeds its supported size.".into(),
                        ));
                    }
                    doc.set_array(id, field, vec!["0".into(); count])?;
                    changed += count;
                }
            }
        }
        Ok(changed)
    })
}
pub fn set_license_plate(
    data: &mut SaveData,
    kind: VehicleKind,
    id: &str,
    plate: &str,
) -> Result<()> {
    require_vehicle(data, kind, id)?;
    if plate.len() > 4096 {
        return Err(Error::InvalidEdit("License plate text is too long.".into()));
    }
    let _ = crate::plates::Plate::parse(plate)?.render()?;
    data.game.set_string(id, "license_plate", plate)
}
pub fn move_headquarters(data: &mut SaveData, destination: &str) -> Result<()> {
    let player = data.player_id()?.to_owned();
    let previous_city = data.game.string(&player, "hq_city")?;
    if previous_city == destination {
        return Ok(());
    }
    let previous = format!("garage.{previous_city}");
    let next = format!("garage.{destination}");
    if data.game.integer::<u32>(&next, "status")? == 0 {
        return Err(Error::InvalidEdit(
            "The destination garage must be owned.".into(),
        ));
    }
    let driver = data
        .game
        .array(&player, "drivers")?
        .first()
        .ok_or_else(|| Error::Missing("player driver".into()))?
        .to_string();
    let mut old_drivers: Vec<_> = data
        .game
        .array(&previous, "drivers")?
        .into_iter()
        .map(str::to_owned)
        .collect();
    let mut old_vehicles: Vec<_> = data
        .game
        .array(&previous, "vehicles")?
        .into_iter()
        .map(str::to_owned)
        .collect();
    let mut drivers: Vec<_> = data
        .game
        .array(&next, "drivers")?
        .into_iter()
        .map(str::to_owned)
        .collect();
    let mut vehicles: Vec<_> = data
        .game
        .array(&next, "vehicles")?
        .into_iter()
        .map(str::to_owned)
        .collect();
    if old_drivers.len() != old_vehicles.len()
        || drivers.len() != vehicles.len()
        || drivers.is_empty()
    {
        return Err(Error::InvalidEdit(
            "The garage slots are missing or inconsistent.".into(),
        ));
    }
    let old_slot = old_drivers
        .iter()
        .position(|id| id == &driver)
        .ok_or_else(|| Error::Missing("player in current HQ garage".into()))?;
    let displaced_driver = drivers[0].clone();
    let displaced_vehicle = vehicles[0].clone();
    drivers[0] = driver;
    vehicles[0] = old_vehicles[old_slot].clone();
    if let Some(spare) = (1..drivers.len())
        .find(|slot| value::is_null(&drivers[*slot]) && value::is_null(&vehicles[*slot]))
    {
        drivers[spare] = displaced_driver;
        vehicles[spare] = displaced_vehicle;
        old_drivers[old_slot] = "null".into();
        old_vehicles[old_slot] = "null".into();
    } else {
        old_drivers[old_slot] = displaced_driver;
        old_vehicles[old_slot] = displaced_vehicle;
    }
    data.game.transaction(|doc| {
        for (city, drivers) in [
            (&previous_city, &old_drivers),
            (&destination.to_owned(), &drivers),
        ] {
            for driver in drivers {
                if doc
                    .unit(driver)
                    .is_some_and(|unit| unit.kind.as_ref() == "driver_ai")
                {
                    doc.set_string(driver, "hometown", city)?;
                }
            }
        }
        doc.set_array(&previous, "drivers", old_drivers)?;
        doc.set_array(&previous, "vehicles", old_vehicles)?;
        doc.set_array(&next, "drivers", drivers)?;
        doc.set_array(&next, "vehicles", vehicles)?;
        doc.set_string(&player, "hq_city", destination)
    })
}
