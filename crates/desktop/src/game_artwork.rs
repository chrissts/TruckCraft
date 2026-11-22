//! Artwork available in local builds that bundle game assets.
use eframe::egui::{self, ColorImage, TextureHandle};
#[cfg(truckcraft_local_game_art)]
mod catalog;

#[cfg(not(truckcraft_local_game_art))]
mod catalog {
    pub const ETS2_PORTRAITS: &[(&str, &[u8])] = &[];
    pub const ATS_PORTRAITS: &[(&str, &[u8])] = &[];
    pub const ETS2_LOGOS: &[(&str, &[u8])] = &[];
    pub const ATS_LOGOS: &[(&str, &[u8])] = &[];
}

pub struct GameArtwork {
    ets2: Option<SkillArtwork>,
    ats: Option<SkillArtwork>,
    cached: std::collections::HashMap<String, TextureHandle>,
}
impl GameArtwork {
    pub fn stock_choices(game: ts_save_core::session::Game, logo: bool) -> Vec<u16> {
        let entries = match (game, logo) {
            (ts_save_core::session::Game::Ets2, false) => catalog::ETS2_PORTRAITS,
            (ts_save_core::session::Game::Ats, false) => catalog::ATS_PORTRAITS,
            (ts_save_core::session::Game::Ets2, true) => catalog::ETS2_LOGOS,
            (ts_save_core::session::Game::Ats, true) => catalog::ATS_LOGOS,
        };
        let mut choices: Vec<_> = entries
            .iter()
            .filter_map(|(id, _)| id.trim_start_matches("logo_").parse::<u16>().ok())
            .collect();
        choices.sort_unstable();
        choices
    }
    pub fn load(context: &egui::Context) -> Self {
        #[cfg(not(truckcraft_local_game_art))]
        let _ = context;
        Self {
            #[cfg(truckcraft_local_game_art)]
            ets2: Some(SkillArtwork::load(
                context,
                ts_save_core::session::Game::Ets2,
            )),
            #[cfg(not(truckcraft_local_game_art))]
            ets2: None,
            #[cfg(truckcraft_local_game_art)]
            ats: Some(SkillArtwork::load(
                context,
                ts_save_core::session::Game::Ats,
            )),
            #[cfg(not(truckcraft_local_game_art))]
            ats: None,
            cached: Default::default(),
        }
    }
    pub fn for_game(&self, game: ts_save_core::session::Game) -> Option<&SkillArtwork> {
        if game == ts_save_core::session::Game::Ats {
            self.ats.as_ref()
        } else {
            self.ets2.as_ref()
        }
    }
    pub fn portrait(
        &mut self,
        context: &egui::Context,
        game: ts_save_core::session::Game,
        face: &str,
    ) -> Option<TextureHandle> {
        self.catalog_image(context, game, face, false)
    }
    pub fn company_logo(
        &mut self,
        context: &egui::Context,
        game: ts_save_core::session::Game,
        logo: &str,
    ) -> Option<TextureHandle> {
        self.catalog_image(context, game, logo, true)
    }
    fn catalog_image(
        &mut self,
        context: &egui::Context,
        game: ts_save_core::session::Game,
        raw: &str,
        logo: bool,
    ) -> Option<TextureHandle> {
        let token = raw
            .rsplit('/')
            .next()?
            .strip_prefix("driver.")
            .unwrap_or(raw.rsplit('/').next()?);
        let token = [".mat", ".tobj", ".dds", ".png"]
            .iter()
            .find_map(|suffix| token.strip_suffix(suffix))
            .unwrap_or(token);
        if token.is_empty()
            || token.len() > 32
            || !token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        {
            return None;
        }
        let key = format!(
            "{}-{}-{token}",
            game.name(),
            if logo { "logo" } else { "portrait" }
        );
        if let Some(texture) = self.cached.get(&key) {
            return Some(texture.clone());
        }
        let entries = match (game, logo) {
            (ts_save_core::session::Game::Ets2, false) => catalog::ETS2_PORTRAITS,
            (ts_save_core::session::Game::Ats, false) => catalog::ATS_PORTRAITS,
            (ts_save_core::session::Game::Ets2, true) => catalog::ETS2_LOGOS,
            (ts_save_core::session::Game::Ats, true) => catalog::ATS_LOGOS,
        };
        let (_, bytes) = entries.iter().find(|(id, _)| *id == token)?;
        let texture = texture(context, key.clone(), bytes);
        self.cached.insert(key, texture.clone());
        Some(texture)
    }
}

#[cfg(truckcraft_local_game_art)]
const SKILLS: [&[u8]; 6] = [
    include_bytes!("../assets/game/skill_adr.png"),
    include_bytes!("../assets/game/skill_distance.png"),
    include_bytes!("../assets/game/skill_heavy.png"),
    include_bytes!("../assets/game/skill_fragile.png"),
    include_bytes!("../assets/game/skill_jit.png"),
    include_bytes!("../assets/game/skill_mechanical.png"),
];
#[cfg(truckcraft_local_game_art)]
const ADR: [&[u8]; 6] = [
    include_bytes!("../assets/game/adr_1.png"),
    include_bytes!("../assets/game/adr_2.png"),
    include_bytes!("../assets/game/adr_3.png"),
    include_bytes!("../assets/game/adr_4.png"),
    include_bytes!("../assets/game/adr_6.png"),
    include_bytes!("../assets/game/adr_8.png"),
];
#[cfg(truckcraft_local_game_art)]
const ADR_GREY: [&[u8]; 6] = [
    include_bytes!("../assets/game/adr_1_grey.png"),
    include_bytes!("../assets/game/adr_2_grey.png"),
    include_bytes!("../assets/game/adr_3_grey.png"),
    include_bytes!("../assets/game/adr_4_grey.png"),
    include_bytes!("../assets/game/adr_6_grey.png"),
    include_bytes!("../assets/game/adr_8_grey.png"),
];
#[cfg(truckcraft_local_game_art)]
const RANKS: [&[u8]; 6] = [
    include_bytes!("../assets/game/linear_rank_1.png"),
    include_bytes!("../assets/game/linear_rank_2.png"),
    include_bytes!("../assets/game/linear_rank_3.png"),
    include_bytes!("../assets/game/linear_rank_4.png"),
    include_bytes!("../assets/game/linear_rank_5.png"),
    include_bytes!("../assets/game/linear_rank_6.png"),
];

#[cfg(truckcraft_local_game_art)]
const ATS_SKILLS: [&[u8]; 6] = [
    include_bytes!("../assets/game/ats/skill_adr.png"),
    include_bytes!("../assets/game/ats/skill_distance.png"),
    include_bytes!("../assets/game/ats/skill_heavy.png"),
    include_bytes!("../assets/game/ats/skill_fragile.png"),
    include_bytes!("../assets/game/ats/skill_jit.png"),
    include_bytes!("../assets/game/ats/skill_mechanical.png"),
];
#[cfg(truckcraft_local_game_art)]
const ATS_ADR: [&[u8]; 6] = [
    include_bytes!("../assets/game/ats/adr_1.png"),
    include_bytes!("../assets/game/ats/adr_2.png"),
    include_bytes!("../assets/game/ats/adr_3.png"),
    include_bytes!("../assets/game/ats/adr_4.png"),
    include_bytes!("../assets/game/ats/adr_6.png"),
    include_bytes!("../assets/game/ats/adr_8.png"),
];
#[cfg(truckcraft_local_game_art)]
const ATS_ADR_GREY: [&[u8]; 6] = [
    include_bytes!("../assets/game/ats/adr_1_grey.png"),
    include_bytes!("../assets/game/ats/adr_2_grey.png"),
    include_bytes!("../assets/game/ats/adr_3_grey.png"),
    include_bytes!("../assets/game/ats/adr_4_grey.png"),
    include_bytes!("../assets/game/ats/adr_6_grey.png"),
    include_bytes!("../assets/game/ats/adr_8_grey.png"),
];
#[cfg(truckcraft_local_game_art)]
const ATS_RANKS: [&[u8]; 6] = [
    include_bytes!("../assets/game/ats/linear_rank_1.png"),
    include_bytes!("../assets/game/ats/linear_rank_2.png"),
    include_bytes!("../assets/game/ats/linear_rank_3.png"),
    include_bytes!("../assets/game/ats/linear_rank_4.png"),
    include_bytes!("../assets/game/ats/linear_rank_5.png"),
    include_bytes!("../assets/game/ats/linear_rank_6.png"),
];

pub struct SkillArtwork {
    pub skills: [TextureHandle; 6],
    pub adr: [TextureHandle; 6],
    pub adr_grey: [TextureHandle; 6],
    pub ranks: [TextureHandle; 6],
}
fn texture(context: &egui::Context, name: String, bytes: &[u8]) -> TextureHandle {
    let rgba = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .expect("embedded game artwork must decode")
        .to_rgba8();
    context.load_texture(
        name,
        ColorImage::from_rgba_unmultiplied(
            [rgba.width() as usize, rgba.height() as usize],
            rgba.as_raw(),
        ),
        egui::TextureOptions::LINEAR,
    )
}
#[cfg(truckcraft_local_game_art)]
impl SkillArtwork {
    fn load(context: &egui::Context, game: ts_save_core::session::Game) -> Self {
        let (skills, adr, adr_grey, ranks) = if game == ts_save_core::session::Game::Ats {
            (ATS_SKILLS, ATS_ADR, ATS_ADR_GREY, ATS_RANKS)
        } else {
            (SKILLS, ADR, ADR_GREY, RANKS)
        };
        Self {
            skills: std::array::from_fn(|i| {
                texture(context, format!("{}-skill-{i}", game.name()), skills[i])
            }),
            adr: std::array::from_fn(|i| {
                texture(context, format!("{}-adr-{i}", game.name()), adr[i])
            }),
            adr_grey: std::array::from_fn(|i| {
                texture(
                    context,
                    format!("{}-adr-grey-{i}", game.name()),
                    adr_grey[i],
                )
            }),
            ranks: std::array::from_fn(|i| {
                texture(context, format!("{}-rank-{i}", game.name()), ranks[i])
            }),
        }
    }
}

#[cfg(all(test, truckcraft_local_game_art))]
mod tests {
    use super::*;
    #[test]
    fn bundled_game_art_has_visible_content_and_correct_explosive_colour() {
        for bytes in ATS_SKILLS
            .into_iter()
            .chain(ATS_ADR)
            .chain(ATS_ADR_GREY)
            .chain(ATS_RANKS)
            .chain(SKILLS)
            .chain(ADR)
            .chain(ADR_GREY)
            .chain(RANKS)
        {
            let rgba = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
                .unwrap()
                .to_rgba8();
            assert!(rgba.pixels().any(|pixel| pixel[3] == 0));
            assert!(rgba.pixels().any(|pixel| pixel[3] >= 240));
        }
        let explosive = image::load_from_memory_with_format(ADR[0], image::ImageFormat::Png)
            .unwrap()
            .to_rgba8();
        // The original DDS uses BGRA masks; swapped red/blue turns this orange badge blue.
        assert!(
            explosive
                .pixels()
                .filter(|p| p[0] > 150 && p[0] > p[2].saturating_add(80) && p[3] > 200)
                .count()
                > 100
        );
    }
}
