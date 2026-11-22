use crate::{Error, Result, session::SaveData};
#[derive(Clone, Copy, Debug)]
pub struct Parameters {
    pub count: u32,
    pub valid_minutes: u32,
    pub spacing_minutes: u32,
}
pub fn companies(data: &SaveData) -> Vec<String> {
    let mut ids: Vec<_> = data
        .game
        .units_of_kind("company")
        .filter(|unit| data.game.value(&unit.id, "cargo_offer_seeds").is_some())
        .map(|unit| unit.id.to_string())
        .collect();
    ids.sort_unstable();
    ids
}
fn require(data: &SaveData, id: &str) -> Result<()> {
    if data
        .game
        .unit(id)
        .is_some_and(|unit| unit.kind.as_ref() == "company")
        && data.game.value(id, "cargo_offer_seeds").is_some()
    {
        Ok(())
    } else {
        Err(Error::InvalidEdit(
            "Select a company with Cargo Market seed data.".into(),
        ))
    }
}
pub fn replace(data: &mut SaveData, id: &str, seeds: &[u32]) -> Result<()> {
    require(data, id)?;
    if seeds.len() > 64 {
        return Err(Error::InvalidEdit("Use at most 64 offer seeds.".into()));
    }
    data.game.set_array(
        id,
        "cargo_offer_seeds",
        seeds.iter().map(u32::to_string).collect(),
    )
}
pub fn regenerate(data: &mut SaveData, id: &str, parameters: Parameters) -> Result<()> {
    require(data, id)?;
    let Parameters {
        count,
        valid_minutes,
        spacing_minutes,
    } = parameters;
    if !(1..=64).contains(&count)
        || !(1..=10080).contains(&valid_minutes)
        || !(1..=1440).contains(&spacing_minutes)
    {
        return Err(Error::InvalidEdit(
            "Cargo generation values are outside their supported ranges.".into(),
        ));
    }
    let time = data.game.integer::<u32>(data.economy_id()?, "game_time")?;
    let mut seeds = Vec::with_capacity(count as usize);
    for index in 0..count {
        let offset = index
            .checked_mul(spacing_minutes)
            .and_then(|value| value.checked_add(valid_minutes))
            .and_then(|value| value.checked_add(rand::random_range(0..=spacing_minutes)))
            .and_then(|value| value.checked_add(time))
            .ok_or_else(|| {
                Error::InvalidEdit("Offer expiry would overflow the game clock.".into())
            })?;
        seeds.push(offset);
    }
    replace(data, id, &seeds)
}
pub fn city(data: &mut SaveData, id: &str, parameters: Option<Parameters>) -> Result<usize> {
    require(data, id)?;
    let city = id
        .rsplit('.')
        .next()
        .ok_or_else(|| Error::InvalidEdit("Company has no city.".into()))?;
    let suffix = format!(".{city}");
    let ids: Vec<_> = companies(data)
        .into_iter()
        .filter(|company| company.ends_with(&suffix))
        .collect();
    let mut staged = data.clone();
    for company in &ids {
        if let Some(parameters) = parameters {
            regenerate(&mut staged, company, parameters)?;
        } else {
            replace(&mut staged, company, &[])?;
        }
    }
    *data = staged;
    Ok(ids.len())
}
