# Original application artwork

`car.svg` is an original hand-authored vector drawing of a generic sedan for
ATS Car mode. `car.png` is its transparent raster rendering using resvg 0.50.0.
It uses no game or manufacturer artwork and was not generated with ImageGen.

Eight desktop illustrations were generated with the built-in image generation
tool using original prompts. The desktop embeds their transparent PNGs.

- `truck.png`: generic European tractor unit.
- `coach.png`: generic touring coach.
- `trailer.png`: standalone box semi-trailer.
- `profile.png`: anonymous account bust and identification card.
- `company.png`: transport depot.
- `freight.png`: delivery crate and forwarding arrow.
- `cargo.png`: stacked cargo containers.
- `convoy.png`: two trucks following a route.

The prompts for these original illustrations are recorded in [prompts.json](prompts.json).

The application mark is an original vector drawing of a truck on a teal tile.
Storytold's [PhotoCraft](https://github.com/storytold/photocraft),
[LightCraft](https://github.com/storytold/lightcraft),
[FilmCraft](https://github.com/storytold/filmcraft), and
[AniCraft](https://github.com/storytold/anicraft) informed the visual direction.
The SVG and renderer are in [`docs/brand`](../../../docs/brand). Run
`./docs/brand/render-brand.ps1 -ResvgPath '<path-to-resvg.exe>'` with resvg
0.50.0 to regenerate the README image, desktop PNG, and Windows ICO.

## Official game logos

Local builds with the optional game artwork use unmodified SCS Software logos
from the official press kits. Public builds identify each simulator by name
and retain TruckCraft’s original illustrations.

- ETS2 source: https://www.scssoft.com/press/euro_truck_simulator_2/images/assets/logo.png
- ETS2 press kit: https://www.scssoft.com/press/sheet?p=euro_truck_simulator_2
- ATS source: https://www.scssoft.com/press/american_truck_simulator/images/assets/logo.png
- ATS press kit: https://www.scssoft.com/press/sheet?p=american_truck_simulator

Copyright SCS Software. The press pages describe their asset permission as
editorial use. Preserve this provenance and assess applicable permission before
any different use. No endorsement of TruckCraft is implied.

## Private local game artwork

Local builds can use skill, rank, portrait and company artwork from local
ETS2 and ATS archives. `game/ASSET-SOURCES.json` records the 770 PNGs,
virtual source paths, dimensions and hashes.

`crates/desktop/examples/import_game_art.rs` is the one-time import tool. It
unpads texture rows and converts BGRA pixels to RGBA without inventing image
content. Rank distance-field contours are materialized as an alpha mask.
Unsupported shader-based company logos are skipped, not shown as raw channels.
The import manifests and `src/game_artwork/catalog.rs` describe the selected set.

The source checkout includes the provenance record. To bundle the artwork in
a local build, supply both logos and the complete ETS2 and ATS imports.
Set `TRUCKCRAFT_BUNDLE_LOCAL_ART=0` to build with TruckCraft artwork only.

## Buy Me a Coffee icon

`buy-me-a-coffee.png` identifies the support link in the app and README. The
icon belongs to Buy Me a Coffee. [Official PNG](https://cdn.buymeacoffee.com/assets/homepage/meta/favicon-144x144.png) · [Brand kit](https://buymeacoffee.com/brand)
