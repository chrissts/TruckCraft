use eframe::egui::{self, ColorImage, TextureHandle};

use ts_save_core::session::Game;

const MARK: &[u8] = include_bytes!("../assets/app-mark-v2.png");
const TRUCK: &[u8] = include_bytes!("../assets/truck.png");
const COACH: &[u8] = include_bytes!("../assets/coach.png");
const CAR: &[u8] = include_bytes!("../assets/car.png");
const TRAILER: &[u8] = include_bytes!("../assets/trailer.png");
const PROFILE: &[u8] = include_bytes!("../assets/profile.png");
const COMPANY: &[u8] = include_bytes!("../assets/company.png");
const FREIGHT: &[u8] = include_bytes!("../assets/freight.png");
const CARGO: &[u8] = include_bytes!("../assets/cargo.png");
const CONVOY: &[u8] = include_bytes!("../assets/convoy.png");
#[cfg(truckcraft_local_game_art)]
const ETS2_LOGO: &[u8] = include_bytes!("../assets/ets2-logo.png");
#[cfg(truckcraft_local_game_art)]
const ATS_LOGO: &[u8] = include_bytes!("../assets/ats-logo.png");

pub struct Artwork {
    pub game: crate::game_artwork::GameArtwork,
    pub mark: TextureHandle,
    pub truck: TextureHandle,
    pub coach: TextureHandle,
    pub car: TextureHandle,
    pub trailer: TextureHandle,
    pub ets2_logo: Option<TextureHandle>,
    pub ats_logo: Option<TextureHandle>,
    profile: TextureHandle,
    company: TextureHandle,
    freight: TextureHandle,
    cargo: TextureHandle,
    convoy: TextureHandle,
}

fn pixels(bytes: &[u8], size: u32) -> image::RgbaImage {
    image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .expect("embedded application artwork must be a valid PNG")
        .thumbnail(size, size)
        .to_rgba8()
}

fn texture(context: &egui::Context, name: &str, bytes: &[u8]) -> TextureHandle {
    let rgba = pixels(bytes, 192);
    let size = [rgba.width() as usize, rgba.height() as usize];
    context.load_texture(
        name,
        ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()),
        egui::TextureOptions::LINEAR,
    )
}

impl Artwork {
    pub fn load(context: &egui::Context) -> Self {
        Self {
            game: crate::game_artwork::GameArtwork::load(context),
            mark: texture(context, "app-mark", MARK),
            truck: texture(context, "truck", TRUCK),
            coach: texture(context, "coach", COACH),
            car: texture(context, "car", CAR),
            trailer: texture(context, "trailer", TRAILER),
            #[cfg(truckcraft_local_game_art)]
            ets2_logo: Some(texture(context, "ets2-logo", ETS2_LOGO)),
            #[cfg(not(truckcraft_local_game_art))]
            ets2_logo: None,
            #[cfg(truckcraft_local_game_art)]
            ats_logo: Some(texture(context, "ats-logo", ATS_LOGO)),
            #[cfg(not(truckcraft_local_game_art))]
            ats_logo: None,
            profile: texture(context, "profile", PROFILE),
            company: texture(context, "company", COMPANY),
            freight: texture(context, "freight", FREIGHT),
            cargo: texture(context, "cargo", CARGO),
            convoy: texture(context, "convoy", CONVOY),
        }
    }

    pub fn tabs(&self, game: Game) -> [(&'static str, &TextureHandle); 8] {
        [
            ("Profile", &self.profile),
            ("Company", &self.company),
            ("Truck", &self.truck),
            match game {
                Game::Ets2 => ("Coach", &self.coach),
                Game::Ats => ("Car", &self.car),
            },
            ("Trailer", &self.trailer),
            ("Freight Market", &self.freight),
            ("Cargo Market", &self.cargo),
            ("Convoy Tools", &self.convoy),
        ]
    }
}

pub fn window_icon() -> egui::IconData {
    let rgba = pixels(MARK, 64);
    egui::IconData {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_artwork_has_real_transparency_and_visible_content() {
        for bytes in [
            MARK, TRUCK, COACH, TRAILER, PROFILE, COMPANY, FREIGHT, CARGO, CONVOY, CAR,
        ] {
            let rgba = pixels(bytes, 128);
            assert!(rgba.pixels().any(|pixel| pixel[3] == 0));
            assert!(rgba.pixels().any(|pixel| pixel[3] >= 240));
        }
        assert_eq!(window_icon().rgba.len(), 64 * 64 * 4);
    }
}
