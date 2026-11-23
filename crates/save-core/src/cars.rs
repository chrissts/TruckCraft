//! ATS saved car mode. Vehicle edits use the shared vehicle fields; car jobs
//! remain separate from truck and coach jobs.
use crate::{
    Error, Result,
    session::{Game, SaveData},
    value,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CarState {
    WrongGame,
    MissingDlc,
    MissingData,
    NoActiveCar,
    NoJourney,
    Ready { vehicle: String, job: String },
}
impl CarState {
    pub fn message(&self) -> &'static str {
        match self {
            Self::WrongGame => "Car mode is available for ATS only.",
            Self::MissingDlc => {
                "Road Trip DLC was not found in this save. Car editing is unavailable."
            }
            Self::MissingData => {
                "No car-mode data was found in this save. Enter car mode in ATS, save in-game, then reload."
            }
            Self::NoActiveCar => {
                "Car data is available. Enter a car in ATS, save in-game, then reload to edit it."
            }
            Self::NoJourney => {
                "Road Trip DLC and a car were detected. Start a Road Trip journey, save in-game, then reload before editing Car."
            }
            Self::Ready { .. } => {
                "Road Trip DLC and an active car journey detected. Changes apply to this car; truck and coach jobs are retained."
            }
        }
    }
    pub fn ready(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }
}

pub fn detect(data: &SaveData) -> Result<CarState> {
    if data.simulator != Game::Ats {
        return Ok(CarState::WrongGame);
    }
    let player = data.player_id()?;
    let doc = &data.game;
    let info = data.info.first_id("save_container")?;
    let has_dlc = data
        .info
        .array_or_empty(info, "dependencies")?
        .into_iter()
        .any(|raw| {
            value::unquote(raw).is_ok_and(|dependency| {
                dependency.split('|').any(|part| part.starts_with("ats_r_"))
            })
        });
    if !has_dlc {
        return Ok(CarState::MissingDlc);
    }
    let mode = doc
        .value(player, "assigned_vehicles_mode")
        .map(value::unquote)
        .transpose()?;
    if mode.as_deref() != Some("car") {
        return Ok(
            if doc.value(player, "cars").is_some()
                || doc.value(player, "current_car_job").is_some()
                || doc.value(data.economy_id()?, "car_job_generator").is_some()
            {
                CarState::NoActiveCar
            } else {
                CarState::MissingData
            },
        );
    }
    let Some(owner) = doc
        .value(player, "assigned_vehicles")
        .filter(|id| !value::is_null(id))
    else {
        return Ok(CarState::NoActiveCar);
    };
    if !doc
        .unit(owner)
        .is_some_and(|unit| unit.kind.as_ref() == "player_vehicles")
    {
        return Err(Error::InvalidSii("invalid saved car vehicle container"));
    }
    let vehicle = doc.required(owner, "vehicle")?;
    if !doc
        .unit(vehicle)
        .is_some_and(|unit| unit.kind.as_ref() == "vehicle")
        || doc.value(vehicle, "fuel_relative").is_none()
    {
        return Err(Error::InvalidSii("missing active car vehicle data"));
    }
    let job = doc
        .value(player, "current_car_job")
        .filter(|id| !value::is_null(id))
        .map(str::to_owned);
    let Some(job) = job else {
        return Ok(CarState::NoJourney);
    };
    {
        if !doc
            .unit(&job)
            .is_some_and(|unit| unit.kind.as_ref() == "player_car_job")
        {
            return Err(Error::InvalidSii("invalid saved car job reference"));
        }
        if let Some(company_car) = doc
            .value(&job, "company_car")
            .filter(|id| !value::is_null(id))
            && company_car != vehicle
        {
            return Err(Error::InvalidSii("active car and car job vehicle disagree"));
        }
    }
    Ok(CarState::Ready {
        vehicle: vehicle.into(),
        job,
    })
}

pub fn require_active(data: &SaveData) -> Result<CarState> {
    let state = detect(data)?;
    if state.ready() {
        Ok(state)
    } else {
        Err(Error::InvalidEdit(state.message().into()))
    }
}
