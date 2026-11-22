//! Read-only archive inspection. Does not extract or modify game assets.
use scs_codec::{Archive, archive::Key, cityhash::cityhash64};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("archive path required")?;
    let query = args.next().unwrap_or_else(|| "skill".into());
    let mut archive = Archive::open(path)?;
    if query.ends_with(".tobj")
        || query.ends_with(".mat")
        || query.ends_with(".sii")
        || query.ends_with(".sui")
    {
        let bytes = archive.read_path(&query)?;
        let header = archive.image_header(Key::Hash(cityhash64(query.as_bytes())));
        println!("{} bytes, image metadata: {:?}", bytes.len(), header);
        if let Some(header) = &header
            && let Some(mut meta) = scs_codec::tobj::parse_image_meta(header)
        {
            println!(
                "Detected format: {}",
                scs_codec::tobj::detect_format(&mut meta, bytes.len())
            );
            println!("{meta:?}");
        }
        if header.is_none() {
            println!(
                "{}",
                String::from_utf8_lossy(&bytes[..bytes.len().min(4096)])
            );
        }
    } else {
        let tree = archive.build_tree();
        for name in tree
            .files
            .iter()
            .filter(|name| name.contains(&query))
            .take(80)
        {
            println!("{name}");
        }
        println!(
            "{} named files, {} unresolved entries",
            tree.files.len(),
            tree.unresolved
        );
    }
    Ok(())
}
