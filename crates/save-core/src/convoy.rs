use crate::{
    Error, Result,
    document::UnitSpec,
    session::{Game, SaveData},
    value,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
const MAX_JSON: usize = 512 * 1024;
const ROUTES: [&str; 3] = [
    "stored_gps_behind_waypoints",
    "stored_gps_ahead_waypoints",
    "stored_gps_avoid_waypoints",
];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub struct Position {
    pub name: String,
    pub placement: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trailer_placements: Option<Vec<String>>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub struct Waypoint {
    pub position: String,
    pub direction: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub struct Pack {
    pub version: u32,
    pub game: Game,
    pub positions: Vec<Position>,
    pub behind: Vec<Waypoint>,
    pub ahead: Vec<Waypoint>,
    pub avoid: Vec<Waypoint>,
}

pub fn active_owner(data: &SaveData) -> Result<Option<String>> {
    let doc = &data.game;
    let player = data.player_id()?;
    if let Some(owner) = doc.value(player, "assigned_vehicles")
        && doc
            .value(owner, "vehicle")
            .is_some_and(|id| !value::is_null(id) && doc.unit(id).is_some())
        && doc.value(owner, "stored_vehicle_placement").is_some()
    {
        return Ok(Some(owner.into()));
    }
    if doc
        .value(player, "assigned_truck")
        .is_some_and(|id| !value::is_null(id) && doc.unit(id).is_some())
        && doc.value(player, "truck_placement").is_some()
    {
        return Ok(Some(player.into()));
    }
    Ok(None)
}
fn placement_field(data: &SaveData, owner: &str) -> Result<&'static str> {
    Ok(if owner == data.player_id()? {
        "truck_placement"
    } else {
        "stored_vehicle_placement"
    })
}
fn trailers(data: &SaveData, owner: &str) -> Result<Vec<String>> {
    let doc = &data.game;
    if owner == data.player_id()? {
        if doc
            .value(owner, "assigned_trailer")
            .is_some_and(|id| !value::is_null(id))
        {
            return Err(Error::InvalidEdit(
                "Detach trailers in-game before sharing positions from older save formats.".into(),
            ));
        }
        return Ok(Vec::new());
    }
    if doc.value(owner, "stored_trailer_attached") != Some("true") {
        return Ok(Vec::new());
    }
    let placements: Vec<_> = doc
        .array(owner, "stored_trailer_placements")?
        .into_iter()
        .map(str::to_owned)
        .collect();
    if placements.is_empty() {
        return Err(Error::InvalidEdit(
            "The attached trailer has no saved placement. Save in-game first.".into(),
        ));
    }
    Ok(placements)
}
fn capture_route(data: &SaveData, field: &str) -> Result<Vec<Waypoint>> {
    data.game
        .array_or_empty(data.economy_id()?, field)?
        .into_iter()
        .map(|id| {
            Ok(Waypoint {
                position: data.game.required(id, "nav_node_position")?.into(),
                direction: data.game.required(id, "direction")?.into(),
            })
        })
        .collect()
}
pub fn capture(data: &SaveData, name: &str) -> Result<Pack> {
    let owner = active_owner(data)?.ok_or_else(|| {
        Error::InvalidEdit("Enter a vehicle and save in-game before sharing positions.".into())
    })?;
    let pack = Pack {
        version: 1,
        game: data.simulator,
        positions: vec![Position {
            name: if name.trim().is_empty() {
                "Current position".into()
            } else {
                name.into()
            },
            placement: data
                .game
                .required(&owner, placement_field(data, &owner)?)?
                .into(),
            trailer_placements: Some(trailers(data, &owner)?),
        }],
        behind: capture_route(data, ROUTES[0])?,
        ahead: capture_route(data, ROUTES[1])?,
        avoid: capture_route(data, ROUTES[2])?,
    };
    validate(&pack, data.simulator)?;
    Ok(pack)
}
pub fn validate_placement(raw: &str) -> Result<()> {
    if raw.len() > 256 {
        return Err(Error::InvalidEdit("Vehicle placement is too large.".into()));
    }
    let (coordinates, orientation) = raw.split_once(") (").ok_or_else(|| {
        Error::InvalidEdit("Placement must contain coordinates and an orientation.".into())
    })?;
    let coordinates = coordinates
        .strip_prefix('(')
        .ok_or_else(|| Error::InvalidEdit("Invalid placement.".into()))?;
    let orientation = orientation
        .strip_suffix(')')
        .ok_or_else(|| Error::InvalidEdit("Invalid placement.".into()))?;
    let coordinates = coordinates
        .split(',')
        .map(value::float)
        .collect::<Result<Vec<_>>>()?;
    let orientation = orientation
        .split([',', ';'])
        .map(value::float)
        .collect::<Result<Vec<_>>>()?;
    if coordinates.len() != 3 || orientation.len() != 4 {
        return Err(Error::InvalidEdit(
            "Placement needs three coordinates and four orientation components.".into(),
        ));
    }
    let norm: f64 = orientation
        .into_iter()
        .map(|component| f64::from(component).powi(2))
        .sum();
    if !(0.5..=1.5).contains(&norm) {
        return Err(Error::InvalidEdit("Invalid orientation quaternion.".into()));
    }
    Ok(())
}
fn validate_waypoint(waypoint: &Waypoint) -> Result<()> {
    let position = waypoint
        .position
        .strip_prefix('(')
        .and_then(|text| text.strip_suffix(')'))
        .ok_or_else(|| Error::InvalidEdit("Invalid GPS coordinate.".into()))?;
    let components = position
        .split(',')
        .map(|component| component.trim().parse::<i32>())
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| Error::InvalidEdit("GPS coordinates must be 32-bit integers.".into()))?;
    if components.len() != 3 {
        return Err(Error::InvalidEdit(
            "GPS positions need three coordinates.".into(),
        ));
    }
    let direction = value::unquote(&waypoint.direction)?;
    if !matches!(direction.as_str(), "forward" | "backward" | "any") {
        return Err(Error::InvalidEdit("Invalid GPS direction.".into()));
    }
    Ok(())
}
pub fn validate(pack: &Pack, expected: Game) -> Result<()> {
    if pack.version != 1 || pack.game != expected {
        return Err(Error::InvalidEdit(
            "Use a version-1 convoy pack for the same game.".into(),
        ));
    }
    if pack.positions.len() > 128 || pack.behind.len() + pack.ahead.len() + pack.avoid.len() > 512 {
        return Err(Error::InvalidEdit(
            "Convoy pack exceeds its position/waypoint limits.".into(),
        ));
    }
    for position in &pack.positions {
        if position.name.chars().count() > 100 {
            return Err(Error::InvalidEdit("Position name is too long.".into()));
        }
        validate_placement(&position.placement)?;
        if let Some(trailers) = &position.trailer_placements {
            if trailers.len() > 16 {
                return Err(Error::InvalidEdit("Too many trailers.".into()));
            }
            for trailer in trailers {
                validate_placement(trailer)?;
            }
        }
    }
    for waypoint in pack.behind.iter().chain(&pack.ahead).chain(&pack.avoid) {
        validate_waypoint(waypoint)?;
    }
    Ok(())
}
pub fn import(json: &str, expected: Game) -> Result<Pack> {
    if json.len() > MAX_JSON {
        return Err(Error::InvalidEdit("Convoy file exceeds 512 KiB.".into()));
    }
    let pack: Pack = serde_json::from_str(json)
        .map_err(|_| Error::InvalidEdit("Invalid convoy JSON pack.".into()))?;
    validate(&pack, expected)?;
    Ok(pack)
}
pub fn export(pack: &Pack) -> Result<String> {
    validate(pack, pack.game)?;
    let json = serde_json::to_string_pretty(pack)
        .map_err(|_| Error::InvalidEdit("Could not serialize convoy pack.".into()))?;
    if json.len() > MAX_JSON {
        return Err(Error::InvalidEdit("Convoy pack exceeds 512 KiB.".into()));
    }
    Ok(json)
}
pub fn apply_position(data: &mut SaveData, pack: &Pack, index: usize) -> Result<()> {
    validate(pack, data.simulator)?;
    let position = pack
        .positions
        .get(index)
        .ok_or_else(|| Error::InvalidEdit("Select a position.".into()))?;
    let owner = active_owner(data)?
        .ok_or_else(|| Error::InvalidEdit("No active vehicle is saved.".into()))?;
    let field = placement_field(data, &owner)?;
    let source_trailers = position.trailer_placements.as_deref().unwrap_or_default();
    let current = trailers(data, &owner)?;
    if source_trailers.len() != current.len() {
        return Err(Error::InvalidEdit(
            "Captured and loaded vehicles must have the same number of attached trailers.".into(),
        ));
    }
    data.game.transaction(|doc| {
        if !source_trailers.is_empty() {
            doc.set_array(
                &owner,
                "stored_trailer_placements",
                source_trailers.to_vec(),
            )?;
        }
        doc.set(&owner, field, position.placement.clone())
    })
}
pub fn apply_route(data: &mut SaveData, pack: &Pack) -> Result<()> {
    validate(pack, data.simulator)?;
    let economy = data.economy_id()?.to_owned();
    for field in ROUTES {
        let _ = data.game.array(&economy, field)?;
    }
    data.game.transaction(|doc| {
        let mut reserved = BTreeSet::new();
        let mut units = Vec::new();
        let mut groups = Vec::new();
        for route in [&pack.behind, &pack.ahead, &pack.avoid] {
            let mut ids = Vec::new();
            for waypoint in route {
                let id = doc.fresh_id(&reserved);
                reserved.insert(id.clone());
                ids.push(id.clone());
                units.push(UnitSpec {
                    kind: "gps_waypoint_storage".into(),
                    id,
                    scalars: BTreeMap::from([
                        ("nav_node_position".into(), waypoint.position.clone()),
                        (
                            "direction".into(),
                            value::quote(&value::unquote(&waypoint.direction)?),
                        ),
                    ]),
                    ..Default::default()
                });
            }
            groups.push(ids);
        }
        doc.insert_units(&units)?;
        for (field, ids) in ROUTES.into_iter().zip(groups) {
            doc.set_array(&economy, field, ids)?;
        }
        Ok(())
    })
}

/// Clear route references while retaining waypoint units that other game data
/// may still reference. The caller stages this through the save session.
pub fn clear_route(data: &mut SaveData) -> Result<()> {
    let economy = data.economy_id()?.to_owned();
    for field in ROUTES {
        let _ = data.game.array(&economy, field)?;
    }
    data.game.transaction(|doc| {
        for field in ROUTES {
            doc.set_array(&economy, field, Vec::new())?;
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_untrusted_positions_and_coordinates() {
        for raw in [
            "(NaN, 0, 0) (1; 0, 0, 0)",
            "(0,0,0) (0;0,0,0)",
            "(0,0,0) (1;0,0,0)\n injected: 1",
        ] {
            assert!(validate_placement(raw).is_err());
        }
        assert!(validate_placement("(0, 0, 0) (&3f800000; 0, 0, 0)").is_ok());
        assert!(
            validate_waypoint(&Waypoint {
                position: "(9999999999999, 0, 0)".into(),
                direction: "\"any\"".into()
            })
            .is_err()
        );
    }
    #[test]
    fn pack_roundtrip_is_compatible_with_existing_pascal_case_json() {
        let json = r#"{"Version":1,"Game":"ETS2","Positions":[{"Name":"Test","Placement":"(0, 0, 0) (1; 0, 0, 0)","TrailerPlacements":[]}],"Behind":[],"Ahead":[],"Avoid":[]}"#;
        let pack = import(json, Game::Ets2).unwrap();
        assert_eq!(import(&export(&pack).unwrap(), Game::Ets2).unwrap(), pack);
        assert!(import(json, Game::Ats).is_err());
        assert!(import("{invalid}", Game::Ats).is_err());
    }
}
