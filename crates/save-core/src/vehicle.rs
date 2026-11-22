//! Typed vehicle details. Edit only existing fields and retain unknown accessories.
use crate::{
    Error, Result,
    editor::{self, VehicleKind},
    session::SaveData,
    value,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const WEAR: &[&str] = &[
    "engine_wear",
    "transmission_wear",
    "cabin_wear",
    "chassis_wear",
    "body_wear",
    "trailer_body_wear",
    "cargo_damage",
    "engine_wear_unfixable",
    "transmission_wear_unfixable",
    "cabin_wear_unfixable",
    "chassis_wear_unfixable",
    "body_wear_unfixable",
    "trailer_body_wear_unfixable",
];
pub const PAINT_FIELDS: &[&str] = &[
    "base_color",
    "mask_r_color",
    "mask_g_color",
    "mask_b_color",
    "flake_color",
    "flip_color",
];

#[derive(Clone, Debug, PartialEq)]
pub struct Wear {
    pub field: String,
    pub wheel: Option<usize>,
    pub fraction: f32,
}

fn fraction(raw: &str) -> Result<f32> {
    let value = value::float(raw)?;
    if !(0.0..=1.0).contains(&value) {
        return Err(Error::InvalidEdit(
            "Saved wear or colour is outside the editable range.".into(),
        ));
    }
    Ok(value)
}

pub fn condition(data: &SaveData, kind: VehicleKind, id: &str) -> Result<Vec<Wear>> {
    editor::require_vehicle(data, kind, id)?;
    let mut result = Vec::new();
    for &field in WEAR {
        if let Some(raw) = data.game.value(id, field) {
            result.push(Wear {
                field: field.into(),
                wheel: None,
                fraction: fraction(raw)?,
            });
        }
    }
    for field in ["wheels_wear", "wheels_wear_unfixable"] {
        let wheels = data.game.array_or_empty(id, field)?;
        if wheels.len() > 64 {
            return Err(Error::InvalidEdit("Too many saved wheels.".into()));
        }
        for (wheel, raw) in wheels.into_iter().enumerate() {
            result.push(Wear {
                field: field.into(),
                wheel: Some(wheel),
                fraction: fraction(raw)?,
            });
        }
    }
    Ok(result)
}

pub fn set_condition(
    data: &mut SaveData,
    kind: VehicleKind,
    id: &str,
    edits: &[Wear],
) -> Result<()> {
    let current = condition(data, kind, id)?;
    if edits.len() != current.len()
        || edits.iter().zip(&current).any(|(a, b)| {
            a.field != b.field
                || a.wheel != b.wheel
                || !a.fraction.is_finite()
                || !(0.0..=1.0).contains(&a.fraction)
        })
    {
        return Err(Error::InvalidEdit(
            "Condition fields changed or contain an invalid percentage.".into(),
        ));
    }
    data.game.transaction(|doc| {
        for (edit, old) in edits.iter().zip(&current).filter(|(a, b)| a != b) {
            let key = edit.wheel.map_or_else(
                || edit.field.clone(),
                |wheel| format!("{}[{wheel}]", edit.field),
            );
            debug_assert_eq!(edit.field, old.field);
            doc.set(id, &key, value::float_text(edit.fraction)?)?;
        }
        Ok(())
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct PaintColor {
    pub accessory: String,
    pub field: String,
    pub linear: [f32; 3],
}
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub id: String,
    pub kind: String,
    pub path: String,
}

pub fn parts(data: &SaveData, kind: VehicleKind, id: &str) -> Result<Vec<Part>> {
    editor::require_vehicle(data, kind, id)?;
    let accessories = data.game.array_or_empty(id, "accessories")?;
    if accessories.len() > 4096 {
        return Err(Error::InvalidEdit("Too many saved accessories.".into()));
    }
    let mut seen = BTreeSet::new();
    accessories
        .into_iter()
        .map(|accessory| {
            if !seen.insert(accessory) {
                return Err(Error::InvalidSii("duplicate vehicle accessory"));
            }
            let unit = data
                .game
                .unit(accessory)
                .ok_or(Error::InvalidSii("missing vehicle accessory"))?;
            Ok(Part {
                id: accessory.into(),
                kind: unit.kind.to_string(),
                path: data.game.string(accessory, "data_path").unwrap_or_default(),
            })
        })
        .collect()
}

fn exclusive(data: &SaveData, owner: &str, accessory: &str) -> Result<()> {
    for unit in data.game.units().filter(|unit| {
        unit.id.as_ref() != owner && matches!(unit.kind.as_ref(), "vehicle" | "trailer")
    }) {
        if data
            .game
            .array_or_empty(&unit.id, "accessories")?
            .contains(&accessory)
        {
            return Err(Error::InvalidEdit(
                "This accessory is shared by another vehicle. Its values were retained.".into(),
            ));
        }
    }
    Ok(())
}

fn rgb(raw: &str) -> Result<[f32; 3]> {
    let raw = raw
        .trim()
        .strip_prefix('(')
        .and_then(|raw| raw.strip_suffix(')'))
        .ok_or(Error::InvalidSii("invalid paint colour"))?;
    let mut values = raw.split(',');
    let mut rgb = [0.0; 3];
    for channel in &mut rgb {
        *channel = fraction(
            values
                .next()
                .ok_or(Error::InvalidSii("paint colour must have three channels"))?,
        )?;
    }
    if values.next().is_some() {
        return Err(Error::InvalidSii("paint colour must have three channels"));
    }
    Ok(rgb)
}

pub fn paint(data: &SaveData, kind: VehicleKind, id: &str) -> Result<Vec<PaintColor>> {
    let mut result = Vec::new();
    for part in parts(data, kind, id)?
        .into_iter()
        .filter(|part| part.kind == "vehicle_paint_job_accessory")
    {
        for &field in PAINT_FIELDS {
            if let Some(raw) = data.game.value(&part.id, field) {
                result.push(PaintColor {
                    accessory: part.id.clone(),
                    field: field.into(),
                    linear: rgb(raw)?,
                });
            }
        }
    }
    Ok(result)
}

pub fn set_paint(
    data: &mut SaveData,
    kind: VehicleKind,
    id: &str,
    edits: &[PaintColor],
) -> Result<()> {
    let current = paint(data, kind, id)?;
    if edits.len() != current.len()
        || edits.iter().zip(&current).any(|(a, b)| {
            a.accessory != b.accessory
                || a.field != b.field
                || a.linear
                    .iter()
                    .any(|n| !n.is_finite() || !(0.0..=1.0).contains(n))
        })
    {
        return Err(Error::InvalidEdit(
            "Paint fields changed or contain an invalid colour.".into(),
        ));
    }
    for (edit, _) in edits.iter().zip(&current).filter(|(a, b)| a != b) {
        exclusive(data, id, &edit.accessory)?;
    }
    data.game.transaction(|doc| {
        for (edit, _) in edits.iter().zip(&current).filter(|(a, b)| a != b) {
            let [r, g, b] = edit.linear.map(value::float_text);
            doc.set(
                &edit.accessory,
                &edit.field,
                format!("({}, {}, {})", r?, g?, b?),
            )?;
        }
        Ok(())
    })
}

fn family(path: &str) -> Option<&str> {
    if path.len() > 512
        || !path.starts_with("/def/vehicle/")
        || !path.ends_with(".sii")
        || path.contains("..")
        || path.contains('\\')
        || path.chars().any(|c| c.is_control())
    {
        return None;
    }
    let tail = path.strip_prefix("/def/vehicle/")?;
    let (vehicle_type, tail) = tail.split_once('/')?;
    if !matches!(
        vehicle_type,
        "truck" | "trailer" | "trailer_owned" | "bus" | "car"
    ) || !tail.contains('/')
    {
        return None;
    }
    path.rsplit_once('/').map(|(prefix, _)| prefix)
}

pub fn part_choices(data: &SaveData, part: &Part) -> Vec<String> {
    let Some(prefix) = family(&part.path) else {
        return Vec::new();
    };
    data.game
        .units_of_kind(&part.kind)
        .filter_map(|unit| data.game.string(&unit.id, "data_path").ok())
        .filter(|path| family(path) == Some(prefix))
        .take(4096)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub fn set_parts(data: &mut SaveData, kind: VehicleKind, id: &str, edits: &[Part]) -> Result<()> {
    let current = parts(data, kind, id)?;
    if current.len() != edits.len() {
        return Err(Error::InvalidEdit("The accessory list changed.".into()));
    }
    for (edit, old) in edits.iter().zip(&current) {
        if edit.id != old.id || edit.kind != old.kind {
            return Err(Error::InvalidEdit(
                "Accessory references cannot be replaced here.".into(),
            ));
        }
        if edit.path != old.path {
            // Structural components need coordinated vehicle-definition validation.
            if old.kind != "vehicle_addon_accessory"
                || !part_choices(data, old).contains(&edit.path)
            {
                return Err(Error::InvalidEdit(
                    "Choose a saved addon from the same vehicle and slot.".into(),
                ));
            }
            exclusive(data, id, &edit.id)?;
        }
    }
    data.game.transaction(|doc| {
        for (edit, _) in edits.iter().zip(&current).filter(|(a, b)| a != b) {
            doc.set_string(&edit.id, "data_path", &edit.path)?;
        }
        Ok(())
    })
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Palette {
    truckcraft_paint: u32,
    colors: Vec<PaletteColor>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PaletteColor {
    field: String,
    linear: [f32; 3],
}

pub fn export_paint(colors: &[PaintColor], accessory: &str) -> Result<String> {
    let colors: Vec<_> = colors
        .iter()
        .filter(|c| c.accessory == accessory)
        .map(|c| PaletteColor {
            field: c.field.clone(),
            linear: c.linear,
        })
        .collect();
    if colors.is_empty() || colors.len() > 6 {
        return Err(Error::InvalidEdit("Select one saved paint job.".into()));
    }
    serde_json::to_string_pretty(&Palette {
        truckcraft_paint: 1,
        colors,
    })
    .map_err(|_| Error::InvalidEdit("Could not export paint colours.".into()))
}

pub fn import_paint(text: &str, colors: &mut [PaintColor], accessory: &str) -> Result<()> {
    if text.len() > 4096 {
        return Err(Error::InvalidEdit(
            "Paint sharing data is too large.".into(),
        ));
    }
    let palette: Palette = serde_json::from_str(text)
        .map_err(|_| Error::InvalidEdit("Expected TruckCraft paint colour sharing data.".into()))?;
    let mut seen = BTreeSet::new();
    if palette.truckcraft_paint != 1
        || palette.colors.is_empty()
        || palette.colors.len() > 6
        || palette.colors.iter().any(|c| {
            !seen.insert(c.field.as_str())
                || !PAINT_FIELDS.contains(&c.field.as_str())
                || c.linear
                    .iter()
                    .any(|n| !n.is_finite() || !(0.0..=1.0).contains(n))
                || !colors
                    .iter()
                    .any(|old| old.accessory == accessory && old.field == c.field)
        })
    {
        return Err(Error::InvalidEdit(
            "The shared paint colours do not match this paint job.".into(),
        ));
    }
    for color in palette.colors {
        if let Some(target) = colors
            .iter_mut()
            .find(|c| c.accessory == accessory && c.field == color.field)
        {
            target.linear = color.linear;
        }
    }
    Ok(())
}
