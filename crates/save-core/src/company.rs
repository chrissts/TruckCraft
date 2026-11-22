//! Garage, city and driver operations with explicit ownership/array invariants.
use crate::{Error, Result, document::Document, editor, events, session::SaveData, value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub struct Garage {
    pub id: String,
    pub city: String,
    pub status: u32,
    pub drivers: Vec<String>,
    pub vehicles: Vec<String>,
    pub trailers: Vec<String>,
}
impl Garage {
    pub fn capacity(&self) -> Result<usize> {
        capacity(self.status)
    }
    pub fn label(&self) -> &'static str {
        match self.status {
            0 => "Not owned",
            2 => "Small",
            3 => "Large",
            6 => "Starting garage",
            _ => "Unknown garage type",
        }
    }
}
fn capacity(status: u32) -> Result<usize> {
    match status {
        0 => Ok(0),
        2 => Ok(3),
        3 => Ok(5),
        6 => Ok(1),
        _ => Err(Error::InvalidEdit(
            "This garage type is not supported for resizing.".into(),
        )),
    }
}
fn array(doc: &Document, id: &str, field: &str) -> Result<Vec<String>> {
    Ok(doc
        .array(id, field)?
        .into_iter()
        .map(str::to_owned)
        .collect())
}
pub fn garages(data: &SaveData) -> Result<Vec<Garage>> {
    let mut result = data
        .game
        .units_of_kind("garage")
        .map(|unit| {
            Ok(Garage {
                id: unit.id.to_string(),
                city: unit
                    .id
                    .strip_prefix("garage.")
                    .ok_or(Error::InvalidSii("invalid garage identifier"))?
                    .into(),
                status: data.game.integer(&unit.id, "status")?,
                drivers: array(&data.game, &unit.id, "drivers")?,
                vehicles: array(&data.game, &unit.id, "vehicles")?,
                trailers: array(&data.game, &unit.id, "trailers")?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    result.sort_by(|a, b| a.city.cmp(&b.city));
    Ok(result)
}
fn validate_garages(doc: &Document, garages: &[Garage]) -> Result<()> {
    let mut drivers = BTreeSet::new();
    let mut vehicles = BTreeSet::new();
    let mut trailers = BTreeSet::new();
    for garage in garages {
        if garage.drivers.len() != garage.vehicles.len()
            || garage.drivers.len() != garage.capacity()?
        {
            return Err(Error::InvalidEdit(
                "Garage slot counts do not match its size. Load a fresh in-game save.".into(),
            ));
        }
        for (list, seen, kinds) in [
            (
                &garage.drivers,
                &mut drivers,
                &["driver_ai", "driver_player"][..],
            ),
            (&garage.vehicles, &mut vehicles, &["vehicle"][..]),
            (&garage.trailers, &mut trailers, &["trailer"][..]),
        ] {
            for id in list.iter().filter(|id| !value::is_null(id)) {
                if !seen.insert(id.clone())
                    || !doc
                        .unit(id)
                        .is_some_and(|unit| kinds.contains(&unit.kind.as_ref()))
                {
                    return Err(Error::InvalidEdit(
                        "A garage has a duplicate or missing occupant.".into(),
                    ));
                }
            }
        }
    }
    Ok(())
}
fn write_garages(doc: &mut Document, garages: &[Garage]) -> Result<()> {
    for garage in garages {
        let previous_drivers = array(doc, &garage.id, "drivers")?;
        doc.set(&garage.id, "status", garage.status.to_string())?;
        doc.set_array(&garage.id, "drivers", garage.drivers.clone())?;
        doc.set_array(&garage.id, "vehicles", garage.vehicles.clone())?;
        doc.set_array(&garage.id, "trailers", garage.trailers.clone())?;
        for driver in &garage.drivers {
            if !previous_drivers.contains(driver)
                && doc
                    .unit(driver)
                    .is_some_and(|unit| unit.kind.as_ref() == "driver_ai")
            {
                doc.set_string(driver, "hometown", &garage.city)?;
            }
        }
    }
    Ok(())
}

/// Occupants that no longer fit move together to free slots in other garages.
pub fn resize_garages(data: &mut SaveData, ids: &[String], status: u32) -> Result<()> {
    let new_capacity = capacity(status)?;
    let player = data.player_id()?.to_owned();
    let hq = data.game.string(&player, "hq_city")?;
    let player_driver = data
        .game
        .array(&player, "drivers")?
        .first()
        .ok_or_else(|| Error::Missing("player driver".into()))?
        .to_string();
    let mut all = garages(data)?;
    validate_garages(&data.game, &all)?;
    let selected = ids.iter().map(String::as_str).collect::<BTreeSet<_>>();
    if selected.is_empty()
        || selected
            .iter()
            .any(|id| !all.iter().any(|garage| garage.id == *id))
    {
        return Err(Error::InvalidEdit(
            "Select at least one known garage.".into(),
        ));
    }
    if all.iter().any(|garage| {
        selected.contains(garage.id.as_str())
            && ((garage.city == hq && status == 0) || (garage.city != hq && status == 6))
    }) {
        return Err(Error::InvalidEdit("Move headquarters before selling its garage. Only headquarters can use the starting garage size.".into()));
    }
    let mut overflow = Vec::new();
    let mut moved_trailers = Vec::new();
    for garage in all
        .iter_mut()
        .filter(|garage| selected.contains(garage.id.as_str()))
    {
        let mut slots = garage
            .drivers
            .iter()
            .cloned()
            .zip(garage.vehicles.iter().cloned())
            .filter(|(driver, vehicle)| !value::is_null(driver) || !value::is_null(vehicle))
            .collect::<Vec<_>>();
        if garage.city == hq {
            if let Some(index) = slots
                .iter()
                .position(|(driver, _)| driver == &player_driver)
            {
                slots.swap(0, index);
            } else {
                return Err(Error::Missing("player in headquarters garage".into()));
            }
        }
        garage.drivers = vec!["null".into(); new_capacity];
        garage.vehicles = vec!["null".into(); new_capacity];
        garage.status = status;
        for (index, (driver, vehicle)) in slots.into_iter().enumerate() {
            if index < new_capacity {
                garage.drivers[index] = driver;
                garage.vehicles[index] = vehicle;
            } else {
                overflow.push((driver, vehicle));
            }
        }
        if status == 0 {
            moved_trailers.append(&mut garage.trailers);
        }
    }
    for (driver, vehicle) in overflow {
        let destination = all
            .iter_mut()
            .filter(|garage| garage.status != 0)
            .find_map(|garage| {
                (0..garage.drivers.len())
                    .find(|slot| {
                        value::is_null(&garage.drivers[*slot])
                            && value::is_null(&garage.vehicles[*slot])
                    })
                    .map(|slot| (garage, slot))
            });
        let Some((garage, slot)) = destination else {
            return Err(Error::InvalidEdit("There are not enough free garage slots to retain all occupants. Buy or upgrade another garage first.".into()));
        };
        garage.drivers[slot] = driver;
        garage.vehicles[slot] = vehicle;
    }
    if !moved_trailers.is_empty() {
        let destination = all
            .iter_mut()
            .find(|garage| garage.city == hq && garage.status != 0)
            .ok_or_else(|| Error::Missing("owned headquarters garage".into()))?;
        destination.trailers.extend(moved_trailers);
    }
    validate_garages(&data.game, &all)?;
    data.game.transaction(|doc| write_garages(doc, &all))
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MoveKind {
    Pair,
    Driver,
    Vehicle,
}
pub fn move_slot(
    data: &mut SaveData,
    source: &str,
    slot: usize,
    target: &str,
    target_slot: usize,
    kind: MoveKind,
) -> Result<()> {
    let mut all = garages(data)?;
    validate_garages(&data.game, &all)?;
    let a = all
        .iter()
        .position(|garage| garage.id == source)
        .ok_or_else(|| Error::Missing("source garage".into()))?;
    let b = all
        .iter()
        .position(|garage| garage.id == target && garage.status != 0)
        .ok_or_else(|| Error::Missing("owned destination garage".into()))?;
    if slot >= all[a].drivers.len() || target_slot >= all[b].drivers.len() {
        return Err(Error::InvalidEdit(
            "Choose valid source and destination slots.".into(),
        ));
    }
    if a == b && slot == target_slot {
        return Ok(());
    }
    let player = data.player_id()?;
    let hq = data.game.string(player, "hq_city")?;
    let player_driver = data
        .game
        .array(player, "drivers")?
        .first()
        .map(|id| id.to_string())
        .unwrap_or_default();
    if kind != MoveKind::Vehicle
        && a != b
        && (all[a].drivers[slot] == player_driver || all[b].drivers[target_slot] == player_driver)
    {
        return Err(Error::InvalidEdit(format!(
            "Use Move headquarters to move your player from {hq}."
        )));
    }
    if kind != MoveKind::Vehicle {
        let av = all[a].drivers[slot].clone();
        let bv = all[b].drivers[target_slot].clone();
        all[a].drivers[slot] = bv;
        all[b].drivers[target_slot] = av;
    }
    if kind != MoveKind::Driver {
        let av = all[a].vehicles[slot].clone();
        let bv = all[b].vehicles[target_slot].clone();
        all[a].vehicles[slot] = bv;
        all[b].vehicles[target_slot] = av;
    }
    data.game.transaction(|doc| write_garages(doc, &all))
}
pub fn move_trailers(data: &mut SaveData, source: &str, target: &str) -> Result<usize> {
    if source == target {
        return Ok(0);
    }
    let mut all = garages(data)?;
    validate_garages(&data.game, &all)?;
    let a = all
        .iter()
        .position(|garage| garage.id == source)
        .ok_or_else(|| Error::Missing("source garage".into()))?;
    let b = all
        .iter()
        .position(|garage| garage.id == target && garage.status != 0)
        .ok_or_else(|| Error::Missing("owned destination garage".into()))?;
    let trailers = std::mem::take(&mut all[a].trailers);
    let count = trailers.len();
    all[b].trailers.extend(trailers);
    data.game.transaction(|doc| write_garages(doc, &all))?;
    Ok(count)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DriverStatus {
    Player,
    Hired,
    Dismissed,
    Available,
}
#[derive(Clone, Debug)]
pub struct Driver {
    pub id: String,
    pub status: DriverStatus,
    pub hometown: String,
    pub xp: u32,
    pub adr: u8,
    pub skills: [u8; 5],
    pub training: u8,
}
pub fn drivers(data: &SaveData) -> Result<Vec<Driver>> {
    let player = data.player_id()?;
    let economy = data.economy_id()?;
    let hired = array(&data.game, player, "drivers")?;
    if !hired.first().is_some_and(|id| {
        data.game
            .unit(id)
            .is_some_and(|unit| unit.kind.as_ref() == "driver_player")
    }) {
        return Err(Error::InvalidSii(
            "player driver is missing from the staff list",
        ));
    }
    let dismissed = array(&data.game, player, "dismissed_drivers")?;
    let pool = array(&data.game, economy, "driver_pool")?;
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for id in hired.iter().chain(&dismissed).chain(&pool) {
        if !seen.insert(id.clone()) {
            continue;
        }
        let is_player = hired.first() == Some(id);
        if !is_player
            && !data
                .game
                .unit(id)
                .is_some_and(|unit| unit.kind.as_ref() == "driver_ai")
        {
            return Err(Error::InvalidSii("invalid AI driver reference"));
        }
        let status = if is_player {
            DriverStatus::Player
        } else if dismissed.contains(id) {
            DriverStatus::Dismissed
        } else if hired.contains(id) {
            DriverStatus::Hired
        } else {
            DriverStatus::Available
        };
        result.push(Driver {
            id: id.clone(),
            status,
            hometown: data.game.string(id, "hometown").unwrap_or_default(),
            xp: data.game.integer(id, "experience_points").unwrap_or(0),
            adr: data.game.integer(id, "adr").unwrap_or(0),
            skills: editor::TRUCK_SKILLS.map(|field| data.game.integer(id, field).unwrap_or(0)),
            training: data.game.integer(id, "training_policy").unwrap_or(0),
        });
    }
    Ok(result)
}
fn require_ai(data: &SaveData, id: &str) -> Result<Driver> {
    drivers(data)?
        .into_iter()
        .find(|driver| driver.id == id && driver.status != DriverStatus::Player)
        .ok_or_else(|| Error::InvalidEdit("Select a known AI driver.".into()))
}
struct Staff {
    drivers: Vec<String>,
    arrays: BTreeMap<&'static str, (Vec<String>, &'static str)>,
}
impl Staff {
    fn push(&mut self, id: &str) {
        self.drivers.push(id.into());
        for (values, default) in self.arrays.values_mut() {
            values.push((*default).into());
        }
    }
    fn remove(&mut self, index: usize) {
        self.drivers.remove(index);
        for (values, _) in self.arrays.values_mut() {
            values.remove(index);
        }
    }
    fn write(self, doc: &mut Document, player: &str) -> Result<()> {
        doc.set_array(player, "drivers", self.drivers)?;
        for (field, (values, _)) in self.arrays {
            doc.set_array(player, field, values)?;
        }
        Ok(())
    }
}
fn staff(doc: &Document, player: &str) -> Result<Staff> {
    let drivers = array(doc, player, "drivers")?;
    let modern = doc.value(player, "driver_flags").is_some();
    let mut arrays = BTreeMap::new();
    for (field, default, required) in [
        ("driver_readiness_timer", "0", true),
        ("driver_flags", "0", false),
        ("driver_undrivable_truck_timers", "0", false),
        ("driver_quit_warned", "false", !modern),
    ] {
        if doc.value(player, field).is_none() {
            if required {
                return Err(Error::Missing(field.into()));
            }
            continue;
        }
        let values = array(doc, player, field)?;
        // Current saves retain an empty deprecated quit-warning array. Flags
        // and undrivable timers replace it; do not start populating it again.
        if field == "driver_quit_warned" && modern && values.is_empty() {
            continue;
        }
        if values.len() != drivers.len() {
            return Err(Error::InvalidSii(
                "driver readiness arrays are inconsistent",
            ));
        }
        arrays.insert(field, (values, default));
    }
    Ok(Staff { drivers, arrays })
}
fn idle(doc: &Document, id: &str) -> Result<bool> {
    let job = doc.required(id, "driver_job")?;
    if value::is_null(job) {
        return Ok(true);
    }
    Ok(value::is_null(doc.required(job, "cargo")?))
}

pub fn hire(data: &mut SaveData, id: &str, destination: &str) -> Result<()> {
    let driver = require_ai(data, id)?;
    if driver.status == DriverStatus::Hired {
        return Err(Error::InvalidEdit("This driver is already hired.".into()));
    }
    let mut all = garages(data)?;
    validate_garages(&data.game, &all)?;
    let target = all
        .iter_mut()
        .find(|garage| garage.id == destination && garage.status != 0)
        .ok_or_else(|| Error::Missing("owned garage".into()))?;
    let slot = target
        .drivers
        .iter()
        .position(|id| value::is_null(id))
        .ok_or_else(|| {
            Error::InvalidEdit("The destination garage has no free driver slot.".into())
        })?;
    target.drivers[slot] = id.into();
    let city = target.city.clone();
    let player = data.player_id()?.to_owned();
    let economy = data.economy_id()?.to_owned();
    let mut staff = staff(&data.game, &player)?;
    if !staff.drivers.iter().any(|driver| driver == id) {
        staff.push(id);
    }
    let pool = array(&data.game, &economy, "driver_pool")?
        .into_iter()
        .filter(|driver| driver != id)
        .collect();
    let dismissed = array(&data.game, &player, "dismissed_drivers")?
        .into_iter()
        .filter(|driver| driver != id)
        .collect();
    let new_driver = driver.status == DriverStatus::Available;
    let clock = data.game.integer::<u32>(&economy, "game_time")?;
    data.game.transaction(|doc| {
        write_garages(doc, &all)?;
        staff.write(doc, &player)?;
        doc.set_array(&player, "dismissed_drivers", dismissed)?;
        doc.set_array(&economy, "driver_pool", pool)?;
        if new_driver {
            doc.set_string(id, "current_city", &city)?;
            doc.set(id, "training_policy", "1")?;
            events::rewrite(
                doc,
                &economy,
                |event| event.unit_link == id,
                &[(id.into(), 3, clock)],
            )?;
        }
        Ok(())
    })
}
pub fn fire(data: &mut SaveData, id: &str) -> Result<()> {
    if require_ai(data, id)?.status != DriverStatus::Hired {
        return Err(Error::InvalidEdit(
            "Select a currently hired AI driver.".into(),
        ));
    }
    let mut all = garages(data)?;
    validate_garages(&data.game, &all)?;
    for garage in &mut all {
        for driver in &mut garage.drivers {
            if driver == id {
                *driver = "null".into();
            }
        }
    }
    let player = data.player_id()?.to_owned();
    let economy = data.economy_id()?.to_owned();
    let is_idle = idle(&data.game, id)?;
    let mut staff = staff(&data.game, &player)?;
    let index = staff
        .drivers
        .iter()
        .position(|driver| driver == id)
        .ok_or_else(|| Error::Missing("hired driver".into()))?;
    let mut pool = array(&data.game, &economy, "driver_pool")?;
    let mut dismissed = array(&data.game, &player, "dismissed_drivers")?;
    if is_idle {
        staff.remove(index);
        if !pool.iter().any(|driver| driver == id) {
            pool.push(id.into());
        }
        dismissed.retain(|driver| driver != id);
    } else if !dismissed.iter().any(|driver| driver == id) {
        dismissed.push(id.into());
    }
    data.game.transaction(|doc| {
        write_garages(doc, &all)?;
        staff.write(doc, &player)?;
        doc.set_array(&player, "dismissed_drivers", dismissed)?;
        doc.set_array(&economy, "driver_pool", pool)?;
        if is_idle {
            for field in ["hometown", "current_city", "old_hometown"] {
                if doc.value(id, field).is_some() {
                    doc.set_string(id, field, "")?;
                }
            }
            for (field, raw) in [
                ("state", "1"),
                ("on_duty_timer", "0"),
                ("extra_maintenance", "0"),
                ("training_policy", "0"),
                ("adopted_truck", "null"),
                ("assigned_truck", "null"),
                ("adopted_trailer", "null"),
                ("assigned_trailer", "null"),
                ("assigned_truck_efficiency", "1"),
                ("assigned_truck_axle_count", "2"),
                ("assigned_truck_mass", "10000"),
                ("slot_truck_efficiency", "1"),
                ("slot_truck_axle_count", "2"),
                ("slot_truck_mass", "10000"),
            ] {
                if doc.value(id, field).is_some() {
                    doc.set(id, field, raw)?;
                }
            }
            events::rewrite(doc, &economy, |event| event.unit_link == id, &[])?;
        }
        Ok(())
    })
}
pub fn set_driver(
    data: &mut SaveData,
    id: &str,
    xp: u32,
    adr: u8,
    skills: [u8; 5],
    training: Option<u8>,
) -> Result<()> {
    require_ai(data, id)?;
    if xp > i32::MAX as u32
        || adr > 63
        || skills.iter().any(|rank| *rank > 6)
        || training.is_some_and(|training| training > 7)
    {
        return Err(Error::InvalidEdit(
            "Driver XP, skills or training are outside the supported range.".into(),
        ));
    }
    data.game.transaction(|doc| {
        doc.set(id, "experience_points", xp.to_string())?;
        doc.set(id, "adr", adr.to_string())?;
        for (field, rank) in editor::TRUCK_SKILLS.iter().zip(skills) {
            doc.set(id, field, rank.to_string())?;
        }
        if let Some(training) = training {
            doc.set(id, "training_policy", training.to_string())?;
        }
        Ok(())
    })
}

/// Raise currently hired employees to the legacy tool's level-150 preset and
/// unlock truck skills/ADR. Existing higher XP, training and job state survive.
pub fn maximise_hired_drivers(data: &mut SaveData) -> Result<usize> {
    let target = crate::profile::xp_for_level(data.simulator, 150)?;
    maximise_hired_drivers_to(data, target)
}

pub fn maximise_hired_drivers_to(data: &mut SaveData, target: u32) -> Result<usize> {
    if target > i32::MAX as u32 {
        return Err(Error::InvalidEdit(
            "Driver XP must fit the game's signed integer.".into(),
        ));
    }
    let hired: Vec<_> = drivers(data)?
        .into_iter()
        .filter(|driver| driver.status == DriverStatus::Hired)
        .collect();
    data.game.transaction(|doc| {
        for driver in &hired {
            doc.set(
                &driver.id,
                "experience_points",
                driver.xp.max(target).to_string(),
            )?;
            doc.set(&driver.id, "adr", "63")?;
            for field in editor::TRUCK_SKILLS {
                doc.set(&driver.id, field, "6")?;
            }
        }
        Ok(hired.len())
    })
}

pub fn cities(data: &SaveData) -> Result<Vec<(String, u32)>> {
    let economy = data.economy_id()?;
    let visited = data.game.array(economy, "visited_cities")?;
    let counts = data.game.array(economy, "visited_cities_count")?;
    if visited.len() != counts.len() {
        return Err(Error::InvalidSii(
            "visited city arrays have different lengths",
        ));
    }
    let mut cities = BTreeMap::new();
    for (city, count) in visited.into_iter().zip(counts) {
        if cities
            .insert(
                value::unquote(city)?,
                count
                    .parse::<u32>()
                    .map_err(|_| Error::InvalidSii("invalid city visit count"))?,
            )
            .is_some()
        {
            return Err(Error::InvalidSii("duplicate visited city"));
        }
    }
    for unit in data.game.units() {
        let city = match unit.kind.as_ref() {
            "garage" => unit.id.strip_prefix("garage."),
            "company" => unit
                .id
                .strip_prefix("company.volatile.")
                .and_then(|id| id.rsplit_once('.').map(|(_, city)| city)),
            "bus_stop" => unit.id.strip_prefix("bus_stop.city."),
            _ => None,
        };
        if let Some(city) = city {
            cities.entry(city.into()).or_insert(0);
        }
    }
    Ok(cities.into_iter().collect())
}
pub fn set_visited(data: &mut SaveData, selected: &[String], visited: bool) -> Result<()> {
    let all = cities(data)?;
    if selected.is_empty()
        || selected
            .iter()
            .any(|city| !all.iter().any(|(known, _)| known == city))
    {
        return Err(Error::InvalidEdit(
            "Select cities listed in this save.".into(),
        ));
    }
    let economy = data.economy_id()?.to_owned();
    let wanted = selected.iter().map(String::as_str).collect::<BTreeSet<_>>();
    let mut names = Vec::new();
    let mut counts = Vec::new();
    let mut retained = BTreeSet::new();
    for (raw, count) in data
        .game
        .array(&economy, "visited_cities")?
        .into_iter()
        .zip(data.game.array(&economy, "visited_cities_count")?)
    {
        let city = value::unquote(raw)?;
        if wanted.contains(city.as_str()) && !visited {
            continue;
        }
        let count = if wanted.contains(city.as_str()) {
            count
                .parse::<u32>()
                .map_err(|_| Error::InvalidSii("invalid city visit count"))?
                .max(1)
                .to_string()
        } else {
            count.to_owned()
        };
        retained.insert(city);
        names.push(raw.to_owned());
        counts.push(count);
    }
    if visited {
        for city in selected {
            if retained.insert(city.clone()) {
                names.push(value::quote(city));
                counts.push("1".into());
            }
        }
    }
    data.game.transaction(|doc| {
        doc.set_array(&economy, "visited_cities", names)?;
        doc.set_array(&economy, "visited_cities_count", counts)
    })
}
