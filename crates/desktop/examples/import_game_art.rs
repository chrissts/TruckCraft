//! One-time asset import from a locally installed game. Never reads a profile.
//! Output names are fixed; no game archive paths become host filesystem paths.
use scs_codec::{Archive, archive::Key, cityhash::cityhash64, tobj};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let root = PathBuf::from(args.next().ok_or("game installation folder required")?);
    let output = PathBuf::from(args.next().ok_or("output folder required")?);
    let mut archive = Archive::open(root.join("base.scs"))?;
    std::fs::create_dir_all(&output)?;
    let mut names = [
        "skill_adr",
        "skill_distance",
        "skill_heavy",
        "skill_fragile",
        "skill_jit",
        "skill_mechanical",
    ]
    .into_iter()
    .map(|name| (format!("material/ui/icon/{name}.tobj"), name.to_owned()))
    .collect::<Vec<_>>();
    for code in [1, 2, 3, 4, 6, 8] {
        for suffix in ["", "_grey"] {
            let name = format!("adr_{code}{suffix}");
            names.push((format!("material/ui/icon/{name}.tobj"), name));
        }
    }
    for rank in 1..=6 {
        let name = format!("linear_rank_{rank}");
        names.push((format!("material/ui/skills/{name}.tobj"), name));
    }
    let tree = archive.build_tree();
    for (prefix, label, limit) in [
        ("material/ui/driver/", "portrait", 1024),
        ("material/ui/player_logo/", "company_logo", 64),
    ] {
        let mut found = 0;
        for path in tree
            .files
            .iter()
            .filter(|path| path.starts_with(prefix) && path.ends_with(".tobj"))
        {
            let token = path
                .strip_prefix(prefix)
                .unwrap()
                .strip_suffix(".tobj")
                .unwrap();
            if token.contains('/') {
                continue;
            } // use full-resolution portraits, not nested thumbnails
            if token.is_empty()
                || token.len() > 32
                || !token
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
            {
                return Err(format!("unsupported artwork token: {token}").into());
            }
            found += 1;
            if found > limit {
                return Err("artwork catalog exceeds import limit".into());
            }
            names.push((path.clone(), format!("{label}_{token}")));
        }
    }
    let mut imported = Vec::new();
    for (path, name) in names {
        let header = archive
            .image_header(Key::Hash(cityhash64(path.as_bytes())))
            .ok_or("texture metadata absent")?;
        let mut meta = tobj::parse_image_meta(&header).ok_or("invalid texture dimensions")?;
        if meta.is_cube || meta.width > 1024 || meta.height > 1024 {
            return Err("UI asset exceeds import limits".into());
        }
        let payload = archive.read_path(&path)?;
        if !tobj::detect_format(&mut meta, payload.len())
            || meta.block != 1
            || !matches!(meta.bpp, 1 | 4)
        {
            return Err(format!("unsupported UI texture format: {path}").into());
        }
        if (name.starts_with("portrait_") || name.starts_with("company_logo_"))
            && meta.dxgi != tobj::B8G8R8A8_UNORM
        {
            println!("Skipped shader-based artwork: {name}");
            continue;
        }
        let tight = tobj::untile(&payload, &meta);
        let count = meta.width as usize * meta.height as usize;
        let bytes = tight
            .get(..count * meta.bpp as usize)
            .ok_or("truncated UI texture")?;
        let rgba = if meta.dxgi == tobj::B8G8R8A8_UNORM {
            // Legacy UI assets store BGRA, confirmed against their DDS channel masks.
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|[b, g, r, a]| [*r, *g, *b, *a])
                .collect()
        } else if meta.bpp == 4 {
            bytes.to_vec()
        } else {
            // The game's rank glyph is a signed distance field. Materialize its
            // contour once so the embedded PNG needs no game shader at runtime.
            bytes
                .iter()
                .flat_map(|sample| {
                    [
                        255,
                        255,
                        255,
                        (((*sample as f32 - 120.0) / 15.0).clamp(0.0, 1.0) * 255.0).round() as u8,
                    ]
                })
                .collect()
        };
        let image = image::RgbaImage::from_raw(meta.width, meta.height, rgba)
            .ok_or("invalid decoded pixels")?;
        image.save(output.join(format!("{name}.png")))?;
        imported.push(serde_json::json!({"file": format!("{name}.png"), "source": path, "width": meta.width, "height": meta.height}));
        println!("Imported {name}: {} × {}", meta.width, meta.height);
    }
    std::fs::write(
        output.join("import-manifest.json"),
        serde_json::to_vec_pretty(&imported)?,
    )?;
    Ok(())
}
