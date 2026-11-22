use crate::{
    Error, Result,
    session::{Game, SaveData},
    value,
};
pub const SKILLS: [&str; 6] = [
    "long_duty",
    "tight_schedule",
    "satisfaction_decrease",
    "low_acceleration",
    "satisfaction_increase",
    "ecodriving",
];
pub const OFFER_FIELDS: [&str; 5] = [
    "departure_time_of_day",
    "expiration_time",
    "tight_schedule_rank",
    "satisfaction_decrease_rank",
    "low_acceleration_rank",
];
#[derive(Clone, Debug)]
pub enum CoachState {
    WrongGame,
    MissingDlc,
    MissingData,
    NoJourney,
    Ready {
        job: String,
        stages: Vec<String>,
        passengers: Vec<String>,
    },
}
impl CoachState {
    pub fn message(&self) -> &'static str {
        match self {
            Self::WrongGame => "Coach editing is available for ETS2 only.",
            Self::MissingDlc => {
                "Coaches DLC was not found in this save. Coach editing is unavailable."
            }
            Self::MissingData => {
                "Coaches DLC detected, but coach data is absent. Start a coach journey, save in-game, then reload that save."
            }
            Self::NoJourney => {
                "Coaches DLC detected. Start a coach journey and save in-game, then load that save before editing Coach."
            }
            Self::Ready { .. } => {
                "Coaches DLC and an active journey detected. Start a journey and save in-game before editing; keep an independent backup."
            }
        }
    }
    pub fn ready(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }
}
pub fn detect(data: &SaveData) -> Result<CoachState> {
    if data.simulator != Game::Ets2 {
        return Ok(CoachState::WrongGame);
    }
    let container = data.info.first_id("save_container")?;
    let has_dlc = data
        .info
        .array_or_empty(container, "dependencies")?
        .into_iter()
        .any(|raw| {
            value::unquote(raw).is_ok_and(|text| text.split('|').any(|part| part == "eut2_coaches"))
        });
    if !has_dlc {
        return Ok(CoachState::MissingDlc);
    }
    let doc = &data.game;
    let economy = data.economy_id()?;
    if doc.value(economy, "bus_experience_points").is_none()
        || doc.units_of_kind("bus_stop").next().is_none()
    {
        return Ok(CoachState::MissingData);
    }
    let player = data.player_id()?;
    let Some(job) = doc.value(player, "current_bus_job") else {
        return Ok(CoachState::NoJourney);
    };
    if !doc
        .unit(job)
        .is_some_and(|unit| unit.kind.as_ref() == "player_bus_job")
    {
        return Ok(CoachState::NoJourney);
    }
    let stages: Vec<_> = doc
        .array_or_empty(job, "stages")?
        .into_iter()
        .map(str::to_owned)
        .collect();
    let passengers: Vec<_> = doc
        .array_or_empty(job, "passengers")?
        .into_iter()
        .map(str::to_owned)
        .collect();
    let valid_stage = doc
        .integer::<usize>(job, "stage_idx")
        .is_ok_and(|index| index < stages.len());
    let references = stages.iter().all(|id| {
        doc.unit(id)
            .is_some_and(|unit| unit.kind.as_ref() == "player_bus_job_stage")
    }) && passengers.iter().all(|id| {
        doc.unit(id)
            .is_some_and(|unit| unit.kind.as_ref() == "player_bus_passenger")
    });
    let has_coach = !crate::editor::fleet(data, crate::editor::VehicleKind::Coach)?.is_empty();
    if !valid_stage || !references || !has_coach {
        return Ok(CoachState::NoJourney);
    }
    Ok(CoachState::Ready {
        job: job.into(),
        stages,
        passengers,
    })
}
pub fn require_journey(data: &SaveData) -> Result<CoachState> {
    let state = detect(data)?;
    if state.ready() {
        Ok(state)
    } else {
        Err(Error::InvalidEdit(state.message().into()))
    }
}
pub fn set_progress(data: &mut SaveData, xp: u32, ranks: [u8; 6]) -> Result<()> {
    require_journey(data)?;
    if xp > i32::MAX as u32 || ranks.iter().any(|rank| *rank > 3) {
        return Err(Error::InvalidEdit(
            "Coach XP must fit a signed integer; coach ranks must be 0–3.".into(),
        ));
    }
    let economy = data.economy_id()?.to_owned();
    data.game.transaction(|doc| {
        doc.set(&economy, "bus_experience_points", xp.to_string())?;
        for (key, rank) in SKILLS.iter().zip(ranks) {
            doc.set(&economy, key, rank.to_string())?;
        }
        Ok(())
    })
}
pub fn discover_stations(data: &mut SaveData) -> Result<usize> {
    require_journey(data)?;
    let ids: Vec<_> = data
        .game
        .units_of_kind("bus_stop")
        .map(|unit| unit.id.to_string())
        .collect();
    data.game.transaction(|doc| {
        for id in &ids {
            doc.set(id, "discovered", "true")?;
        }
        Ok(ids.len())
    })
}
pub fn set_satisfaction(data: &mut SaveData, percent: f32) -> Result<usize> {
    let state = require_journey(data)?;
    if !percent.is_finite() || !(0.0..=100.0).contains(&percent) {
        return Err(Error::InvalidEdit("Satisfaction must be 0%–100%.".into()));
    }
    let CoachState::Ready { passengers, .. } = state else {
        unreachable!()
    };
    let raw = value::float_text(percent / 100.0)?;
    data.game.transaction(|doc| {
        for id in &passengers {
            doc.set(id, "satisfaction", raw.clone())?;
        }
        Ok(passengers.len())
    })
}
pub fn extend_trip(data: &mut SaveData, minutes: u32) -> Result<()> {
    let state = require_journey(data)?;
    if !(1..=1440).contains(&minutes) {
        return Err(Error::InvalidEdit("Add between 1 and 1440 minutes.".into()));
    }
    let CoachState::Ready { job, stages, .. } = state else {
        unreachable!()
    };
    let index = data.game.integer::<usize>(&job, "stage_idx")?;
    data.game.transaction(|doc| {
        for id in stages.iter().skip(index) {
            for field in ["planned_stage_time", "planned_arrival_time"] {
                if let Some(raw) = doc.value(id, field) {
                    let number = raw
                        .parse::<u32>()
                        .map_err(|_| Error::InvalidSii("invalid stage time"))?;
                    if number != u32::MAX {
                        doc.set(
                            id,
                            field,
                            number
                                .checked_add(minutes)
                                .ok_or_else(|| {
                                    Error::InvalidEdit("Stage time would overflow.".into())
                                })?
                                .to_string(),
                        )?;
                    }
                }
            }
        }
        Ok(())
    })
}
pub fn set_offer(data: &mut SaveData, id: &str, field: &str, value: u32) -> Result<()> {
    require_journey(data)?;
    let limit = match field {
        "departure_time_of_day" => 1439,
        "expiration_time" => i32::MAX as u32,
        "tight_schedule_rank" | "satisfaction_decrease_rank" | "low_acceleration_rank" => 3,
        _ => {
            return Err(Error::InvalidEdit(
                "Unsupported passenger offer field.".into(),
            ));
        }
    };
    if value > limit
        || !data
            .game
            .unit(id)
            .is_some_and(|unit| unit.kind.as_ref() == "line_offer_data")
    {
        return Err(Error::InvalidEdit(
            "Select a passenger offer and a value in range.".into(),
        ));
    }
    data.game.set(id, field, value.to_string())
}
