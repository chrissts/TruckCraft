//! Player level conversion and bounded, legacy-compatible color sharing.
use crate::{
    Error, Result,
    session::{Game, SaveData},
};
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use std::io::{Read, Write};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub face: u16,
    pub logo: String,
    pub male: bool,
}

pub fn identity(data: &SaveData) -> Result<Identity> {
    let id = data.profile_id()?;
    let male = match data.profile.required(id, "male")? {
        "true" => true,
        "false" => false,
        _ => return Err(Error::InvalidSii("invalid saved profile gender")),
    };
    Ok(Identity {
        face: data.profile.integer(id, "face")?,
        logo: data.profile.string(id, "logo")?,
        male,
    })
}

/// Stock choices are provided by the game's artwork catalog. Unchanged custom
/// choices are retained; only an explicitly selected stock value is replaced.
pub fn set_identity(
    data: &mut SaveData,
    edit: &Identity,
    faces: &[u16],
    logos: &[u16],
) -> Result<()> {
    let current = identity(data)?;
    if (edit.face != current.face && !faces.contains(&edit.face))
        || (edit.logo != current.logo
            && !edit
                .logo
                .strip_prefix("logo_")
                .and_then(|id| id.parse::<u16>().ok())
                .is_some_and(|id| logos.contains(&id) && edit.logo == format!("logo_{id}")))
    {
        return Err(Error::InvalidEdit(
            "Choose a portrait and logo from the selected game's stock catalog.".into(),
        ));
    }
    let id = data.profile_id()?.to_owned();
    data.profile.transaction(|doc| {
        if edit.face != current.face {
            doc.set(&id, "face", edit.face.to_string())?;
        }
        if edit.logo != current.logo {
            doc.set(&id, "logo", edit.logo.clone())?;
        }
        if edit.male != current.male {
            doc.set(&id, "male", edit.male.to_string())?;
        }
        Ok(())
    })
}

const ETS_LEVELS: [u32; 30] = [
    200, 500, 700, 900, 1000, 1100, 1300, 1600, 1700, 2100, 2300, 2600, 2700, 2900, 3000, 3100,
    3400, 3700, 4000, 4300, 4600, 4700, 4900, 5200, 5700, 5900, 6000, 6200, 6600, 6800,
];
const ATS_LEVELS: [u32; 30] = [
    200, 500, 700, 900, 1100, 1300, 1500, 1700, 1900, 2100, 2300, 2500, 2700, 2900, 3100, 3300,
    3500, 3700, 4000, 4300, 4600, 4900, 5200, 5500, 5800, 6100, 6400, 6700, 7000, 7300,
];
fn steps(game: Game) -> &'static [u32; 30] {
    if game == Game::Ets2 {
        &ETS_LEVELS
    } else {
        &ATS_LEVELS
    }
}

/// A validated definition-driven XP curve. Beyond the listed levels, the game
/// repeats the final step. Presets select the last XP within the requested level.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XpCurve {
    steps: Vec<u32>,
}
impl XpCurve {
    pub fn from_steps(steps: Vec<u32>) -> Result<Self> {
        if steps.is_empty()
            || steps.len() > 256
            || steps.contains(&0)
            || steps
                .iter()
                .try_fold(0u32, |sum, step| sum.checked_add(*step))
                .is_none()
        {
            return Err(Error::InvalidDefinition(
                "XP steps must be positive bounded integers.".into(),
            ));
        }
        Ok(Self { steps })
    }
    pub fn standard(game: Game) -> Self {
        Self {
            steps: steps(game).to_vec(),
        }
    }
    pub fn from_definitions(catalog: &crate::definitions::Catalog, unit: &str) -> Result<Self> {
        let unit = catalog.unit(unit)?;
        if unit.kind != "xp_level_data" {
            return Err(Error::InvalidDefinition(
                "Unexpected XP definition type.".into(),
            ));
        }
        let steps = unit
            .array("level_xp")?
            .into_iter()
            .map(|raw| {
                raw.parse::<u32>()
                    .map_err(|_| Error::InvalidDefinition("XP step is not an integer.".into()))
            })
            .collect::<Result<Vec<_>>>()?;
        Self::from_steps(steps)
    }
    pub fn level_for_xp(&self, xp: u32) -> u32 {
        let mut remaining = xp;
        for (level, step) in self.steps.iter().enumerate() {
            if remaining < *step {
                return level as u32;
            }
            remaining -= step;
        }
        // At most xp + 1 levels exist because every validated step is positive.
        (self.steps.len() as u64 + u64::from(remaining / self.steps[self.steps.len() - 1]))
            .min(u64::from(u32::MAX)) as u32
    }
    pub fn xp_for_level(&self, level: u32) -> Result<u32> {
        if level > 150 {
            return Err(Error::InvalidEdit(
                "Choose a level between 0 and 150.".into(),
            ));
        }
        let end = (0..=level)
            .try_fold(0u32, |total, index| {
                total.checked_add(self.steps[(index as usize).min(self.steps.len() - 1)])
            })
            .and_then(|total| total.checked_sub(1))
            .filter(|xp| *xp <= i32::MAX as u32)
            .ok_or_else(|| Error::InvalidEdit("This level exceeds the game's XP range.".into()))?;
        Ok(end)
    }
}

pub fn level_for_xp(game: Game, xp: u32) -> u32 {
    let mut remaining = xp;
    for (level, step) in steps(game).iter().enumerate() {
        if remaining < *step {
            return level as u32;
        }
        remaining -= step;
    }
    30 + remaining / steps(game)[29]
}

pub fn xp_for_level(game: Game, level: u32) -> Result<u32> {
    if level > 150 {
        return Err(Error::InvalidEdit(
            "Choose a level between 0 and 150.".into(),
        ));
    }
    let steps = steps(game);
    // Match the original level convenience control: the last XP in that level.
    Ok((0..=level)
        .map(|index| steps[(index as usize).min(29)])
        .sum::<u32>()
        - 1)
}

pub fn user_colors(data: &SaveData) -> Result<Vec<u32>> {
    data.game
        .array(data.economy_id()?, "user_colors")?
        .into_iter()
        .map(|raw| {
            if raw == "nil" {
                Ok(u32::MAX)
            } else {
                raw.parse()
                    .map_err(|_| Error::InvalidSii("invalid user color"))
            }
        })
        .collect()
}

fn valid_colors(colors: &[u32]) -> Result<()> {
    if colors.len() > 160 || !colors.len().is_multiple_of(4) {
        return Err(Error::InvalidEdit(
            "Colors must contain complete four-color slots, up to 40 slots.".into(),
        ));
    }
    Ok(())
}

pub fn set_user_colors(data: &mut SaveData, colors: &[u32]) -> Result<()> {
    valid_colors(colors)?;
    let economy = data.economy_id()?.to_owned();
    data.game.set_array(
        &economy,
        "user_colors",
        colors.iter().map(u32::to_string).collect(),
    )
}

pub fn rgba(color: u32) -> [u8; 4] {
    color.to_le_bytes()
}
pub fn color(rgba: [u8; 4]) -> u32 {
    u32::from_le_bytes(rgba)
}

pub fn export_colors(colors: &[u32]) -> Result<String> {
    valid_colors(colors)?;
    let mut text = String::from("UserColors");
    for raw in colors {
        let [r, g, b, a] = rgba(*raw);
        let argb = u32::from_be_bytes([a, r, g, b]) as i32;
        text.push_str(&format!("\r\n{argb}"));
    }
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(text.as_bytes())?;
    Ok(encoder
        .finish()?
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect())
}

pub fn import_colors(text: &str) -> Result<Vec<u32>> {
    let text = text.trim();
    if text.len() > 64 * 1024 || !text.len().is_multiple_of(2) || !text.is_ascii() {
        return Err(Error::InvalidEdit("Invalid color sharing data.".into()));
    }
    let bytes: Vec<u8> = text
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let hex = |byte: u8| match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                b'A'..=b'F' => Some(byte - b'A' + 10),
                _ => None,
            };
            hex(pair[0])
                .zip(hex(pair[1]))
                .map(|(a, b)| a * 16 + b)
                .ok_or_else(|| {
                    Error::InvalidEdit("Color sharing data must contain hexadecimal bytes.".into())
                })
        })
        .collect::<Result<_>>()?;
    let mut decoded = String::new();
    GzDecoder::new(bytes.as_slice())
        .take(16 * 1024 + 1)
        .read_to_string(&mut decoded)?;
    if decoded.len() > 16 * 1024 {
        return Err(Error::InvalidEdit(
            "Color sharing data is too large.".into(),
        ));
    }
    let mut lines = decoded.lines();
    if lines.next() != Some("UserColors") {
        return Err(Error::InvalidEdit(
            "Expected UserColors sharing data.".into(),
        ));
    }
    let colors: Vec<u32> = lines
        .filter(|line| !line.is_empty())
        .map(|line| {
            let argb = line
                .parse::<i32>()
                .map_err(|_| Error::InvalidEdit("Invalid shared color value.".into()))?
                as u32;
            let [a, r, g, b] = argb.to_be_bytes();
            Ok(color([r, g, b, a]))
        })
        .collect::<Result<_>>()?;
    valid_colors(&colors)?;
    Ok(colors)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn level_convenience_matches_boundaries_for_both_games() {
        for game in [Game::Ets2, Game::Ats] {
            for level in 0..=150 {
                let xp = xp_for_level(game, level).unwrap();
                assert_eq!(level_for_xp(game, xp), level);
                assert_eq!(level_for_xp(game, xp + 1), level + 1);
            }
        }
    }
    #[test]
    fn legacy_color_sharing_preserves_channel_order_and_empty_slots() {
        let colors = [
            0,
            color([255, 0, 0, 255]),
            color([0, 19, 201, 255]),
            u32::MAX,
        ];
        let text = export_colors(&colors).unwrap();
        assert!(text.starts_with("1F8B"));
        assert_eq!(import_colors(&text).unwrap(), colors);
        assert!(import_colors("00é").is_err());
        assert!(import_colors("1F8B00").is_err());
        assert!(export_colors(&[0; 164]).is_err());
        assert!(export_colors(&[0; 3]).is_err());
    }
}
