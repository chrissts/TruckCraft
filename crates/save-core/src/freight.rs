//! Freight offers and their indexed expiration events are edited atomically.
use crate::{Error, Result, events, session::SaveData, value};
use std::collections::{BTreeMap, BTreeSet};

const MAX_OFFERS: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Terms {
    pub target: String,
    pub expiration: u32,
    pub urgency: u8,
    pub distance: u32,
    pub ferry_time: u32,
    pub ferry_price: u32,
}

#[derive(Clone, Debug)]
pub struct Offer {
    pub id: String,
    pub source: String,
    pub cargo: String,
    pub trailer: String,
    pub variant: String,
    pub truck: String,
    pub units: u32,
    pub terms: Terms,
}

pub fn companies(data: &SaveData) -> Result<Vec<String>> {
    let mut ids = Vec::new();
    for id in data.game.array(data.economy_id()?, "companies")? {
        if data
            .game
            .unit(id)
            .is_some_and(|unit| unit.kind.as_ref() == "company")
            && data.game.value(id, "job_offer").is_some()
        {
            ids.push(id.to_owned());
        }
    }
    ids.sort();
    ids.dedup();
    Ok(ids)
}

fn slots(data: &SaveData, source: &str) -> Result<Vec<String>> {
    if !companies(data)?.iter().any(|id| id == source) {
        return Err(Error::InvalidEdit(
            "Select a Freight Market company in this save.".into(),
        ));
    }
    let ids = data.game.array(source, "job_offer")?;
    if ids.len() > MAX_OFFERS {
        return Err(Error::InvalidEdit(
            "Company exceeds the supported offer limit.".into(),
        ));
    }
    let mut seen = BTreeSet::new();
    for id in &ids {
        if !seen.insert(*id)
            || !data
                .game
                .unit(id)
                .is_some_and(|unit| unit.kind.as_ref() == "job_offer_data")
        {
            return Err(Error::InvalidSii(
                "invalid or duplicate freight offer reference",
            ));
        }
    }
    Ok(ids.into_iter().map(str::to_owned).collect())
}

pub fn offers(data: &SaveData, source: &str) -> Result<Vec<Offer>> {
    slots(data, source)?
        .into_iter()
        .filter(|id| {
            data.game
                .value(id, "cargo")
                .is_some_and(|raw| raw.starts_with("cargo."))
        })
        .map(|id| {
            let doc = &data.game;
            Ok(Offer {
                source: source.into(),
                cargo: doc.required(&id, "cargo")?.into(),
                trailer: doc.required(&id, "trailer_definition")?.into(),
                variant: doc.required(&id, "trailer_variant")?.into(),
                truck: doc.string(&id, "company_truck")?,
                units: doc.integer(&id, "units_count")?,
                terms: Terms {
                    target: doc.string(&id, "target")?,
                    expiration: doc.integer(&id, "expiration_time")?,
                    urgency: doc.integer(&id, "urgency")?,
                    distance: doc.integer(&id, "shortest_distance_km")?,
                    ferry_time: doc.integer(&id, "ferry_time")?,
                    ferry_price: doc.integer(&id, "ferry_price")?,
                },
                id,
            })
        })
        .collect()
}

fn validate_terms(data: &SaveData, source: &str, terms: &Terms) -> Result<()> {
    let destination = format!("company.volatile.{}", terms.target);
    if destination == source || !companies(data)?.contains(&destination) {
        return Err(Error::InvalidEdit(
            "Choose a different destination company in this save.".into(),
        ));
    }
    let now: u32 = data.game.integer(data.economy_id()?, "game_time")?;
    if terms.expiration <= now
        || terms.urgency > 2
        || terms.distance > 100_000
        || terms.ferry_time > 100_800
        || terms.ferry_price > 10_000_000
    {
        return Err(Error::InvalidEdit(
            "Use a future expiry, urgency 0–2, and supported distance/ferry values.".into(),
        ));
    }
    Ok(())
}

fn write_terms(data: &mut SaveData, id: &str, terms: &Terms) -> Result<()> {
    data.game.set_string(id, "target", &terms.target)?;
    for (field, number) in [
        ("expiration_time", terms.expiration),
        ("urgency", u32::from(terms.urgency)),
        ("shortest_distance_km", terms.distance),
        ("ferry_time", terms.ferry_time),
        ("ferry_price", terms.ferry_price),
    ] {
        data.game.set(id, field, number.to_string())?;
    }
    Ok(())
}

/// Preserve event units and unknown fields; only known slot indices/times change.
fn remap_events(data: &mut SaveData, source: &str, old: &[String], new: &[String]) -> Result<()> {
    let economy = data.economy_id()?.to_owned();
    let (queue, mut entries) = events::entries(&data.game, &economy)?;
    let mut matched = BTreeSet::new();
    let mut retained = Vec::new();
    let indices: BTreeMap<_, _> = new
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), i))
        .collect();
    for mut event in entries.drain(..) {
        if event.unit_link == source && event.param >= 0 && (event.param as u64) < old.len() as u64
        {
            let old_index = event.param as usize;
            if !matched.insert(old_index) {
                return Err(Error::InvalidSii("duplicate freight expiration event"));
            }
            let Some(&index) = indices.get(old[old_index].as_str()) else {
                continue;
            };
            event.param = index as i64;
            event.time = data.game.integer(&new[index], "expiration_time")?;
            data.game.set(&event.id, "param", event.param.to_string())?;
            data.game.set(&event.id, "time", event.time.to_string())?;
        }
        retained.push(event);
    }
    if old.iter().enumerate().any(|(index, id)| {
        data.game
            .value(id, "cargo")
            .is_some_and(|raw| raw.starts_with("cargo."))
            && !matched.contains(&index)
    }) {
        return Err(Error::InvalidSii("missing freight expiration event"));
    }
    retained.sort_by_key(|event| event.time);
    data.game.set_array(
        &queue,
        "data",
        retained.into_iter().map(|event| event.id).collect(),
    )
}

pub fn update(data: &mut SaveData, source: &str, id: &str, terms: &Terms) -> Result<()> {
    let old = slots(data, source)?;
    if !old.iter().any(|offer| offer == id) {
        return Err(Error::InvalidEdit(
            "Select an offer from this company.".into(),
        ));
    }
    for company in companies(data)?
        .iter()
        .filter(|company| company.as_str() != source)
    {
        if slots(data, company)?.iter().any(|offer| offer == id) {
            return Err(Error::InvalidEdit("This offer is shared by multiple companies. Reload a game-generated save before editing it.".into()));
        }
    }
    validate_terms(data, source, terms)?;
    let mut staged = data.clone();
    write_terms(&mut staged, id, terms)?;
    remap_events(&mut staged, source, &old, &old)?;
    *data = staged;
    Ok(())
}

pub fn configure(
    data: &mut SaveData,
    source: &str,
    id: &str,
    template: &str,
    terms: &Terms,
) -> Result<()> {
    let mut known = false;
    for company in companies(data)? {
        if slots(data, &company)?.iter().any(|offer| offer == template) {
            known = true;
            break;
        }
    }
    if !known
        || !data.game.required(template, "cargo")?.starts_with("cargo.")
        || !data.game.array(template, "trailer_place")?.is_empty()
        || !data.game.array(id, "trailer_place")?.is_empty()
    {
        return Err(Error::InvalidEdit(
            "Use standard offers without special trailer placements.".into(),
        ));
    }
    let mut staged = data.clone();
    for field in [
        "cargo",
        "company_truck",
        "trailer_variant",
        "trailer_definition",
        "units_count",
        "fill_ratio",
    ] {
        let raw = data.game.required(template, field)?.to_owned();
        staged.game.set(id, field, raw)?;
    }
    update(&mut staged, source, id, terms)?;
    *data = staged;
    Ok(())
}

/// Copy a saved cargo/trailer/variant combination without inventing definitions.
pub fn create(data: &mut SaveData, source: &str, template: &str, terms: &Terms) -> Result<String> {
    let old = slots(data, source)?;
    if old.len() >= MAX_OFFERS {
        return Err(Error::InvalidEdit(
            "A company can contain at most 256 offers.".into(),
        ));
    }
    validate_terms(data, source, terms)?;
    let mut found = false;
    for company in companies(data)? {
        if slots(data, &company)?.iter().any(|id| id == template) {
            found = true;
            break;
        }
    }
    if !found
        || !data.game.required(template, "cargo")?.starts_with("cargo.")
        || !data.game.array(template, "trailer_place")?.is_empty()
    {
        return Err(Error::InvalidEdit(
            "Choose a standard saved cargo offer without special trailer placements.".into(),
        ));
    }
    let mut staged = data.clone();
    // Validate the original queue before appending a new indexed event.
    remap_events(&mut staged, source, &old, &old)?;
    let id = staged.game.fresh_id(&BTreeSet::new());
    staged.game.duplicate_unit(template, &id)?;
    write_terms(&mut staged, &id, terms)?;
    let mut new = old.clone();
    new.push(id.clone());
    staged.game.set_array(source, "job_offer", new)?;
    let economy = staged.economy_id()?.to_owned();
    events::rewrite(
        &mut staged.game,
        &economy,
        |_| false,
        &[(source.into(), old.len() as i64, terms.expiration)],
    )?;
    *data = staged;
    Ok(id)
}

pub fn remove(data: &mut SaveData, source: &str, ids: &[String]) -> Result<()> {
    let old = slots(data, source)?;
    if ids.iter().any(|id| !old.contains(id)) {
        return Err(Error::InvalidEdit(
            "Select offers from this company.".into(),
        ));
    }
    let new: Vec<_> = old.iter().filter(|id| !ids.contains(id)).cloned().collect();
    let mut staged = data.clone();
    remap_events(&mut staged, source, &old, &new)?;
    staged.game.set_array(source, "job_offer", new)?;
    // Retain detached units: unknown game data may still refer to them.
    *data = staged;
    Ok(())
}

pub fn clear(data: &mut SaveData, source: &str) -> Result<()> {
    let ids = slots(data, source)?;
    remove(data, source, &ids)
}

pub fn target(company: &str) -> Result<&str> {
    company
        .strip_prefix("company.volatile.")
        .ok_or_else(|| Error::InvalidEdit("Unsupported company identifier.".into()))
}

pub fn cargo_name(raw: &str) -> String {
    value::unquote(raw)
        .unwrap_or_else(|_| raw.into())
        .trim_start_matches("cargo.")
        .replace('_', " ")
}
