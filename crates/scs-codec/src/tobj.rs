//! Texture reconstruction for `HashFS` v2.
//!
//! In `HashFS` v2 a texture is one "tobj" entry: the `kind == 1` metadata part
//! describes the image and the data part holds texels in **D3D12 upload
//! layout** (GPU-ready), GDeflate-compressed.
//!
//! Image metadata part (`kind == 1`, 8 bytes):
//!
//! ```text
//! u16  width  - 1
//! u16  height - 1
//! u8   byte4 : low nibble = mipCount-1, high nibble = flags
//! u8   byte5 : pixel-format selector
//! u8   byte6 : 0x80 for 2D, 0x81 for cubemaps
//! u8   byte7 : 0x09 (constant)
//! ```
//!
//! Texel layout (D3D12 `GetCopyableFootprints`): row pitch is the tight row
//! aligned to 256, a subresource is `pitch * (rows - 1) + tight_row`, and every
//! subresource is aligned to 512 **except the very last**. Cubemaps store six
//! faces, each a full mip chain, in +X −X +Y −Y +Z −Z order.
//!
//! Producing a standard `.dds` means un-padding that back to tight rows and
//! writing `DDS_HEADER` + `DDS_HEADER_DXT10`. Geometry and texel bytes are
//! exact. The DXGI sub-variant among formats sharing a block size is
//! SCS-internal and not fully derivable from the metadata, so [`format_code`]
//! carries what has been identified and the block size decides the rest.

use std::fmt::Write as _;

const fn align(x: usize, a: usize) -> usize {
    x.div_ceil(a) * a
}

/// (block, bytes per block or pixel) candidates probed during detection.
const CANDIDATES: [(u32, u32); 7] = [
    (1, 1),
    (1, 2),
    (1, 4),
    (1, 8),
    (1, 16),
    (4, 8),  // BC1 / BC4
    (4, 16), // BC2 / BC3 / BC5 / BC6H / BC7
];

// DXGI_FORMAT constants.
pub const BC1_UNORM: u32 = 71;
pub const BC3_UNORM: u32 = 77;
pub const BC4_UNORM: u32 = 80;
pub const BC5_UNORM: u32 = 83;
pub const BC6H_UF16: u32 = 95;
pub const BC7_UNORM: u32 = 98;
pub const R8_UNORM: u32 = 61;
pub const R8G8_UNORM: u32 = 49;
pub const R8G8B8A8_UNORM: u32 = 28;
pub const B8G8R8A8_UNORM: u32 = 87;
pub const R16G16B16A16_UNORM: u32 = 11;
pub const R32G32B32A32_FLOAT: u32 = 2;

/// Fallback DXGI for a detected block class.
const fn default_dxgi(block: u32, bpp: u32) -> u32 {
    match (block, bpp) {
        (1, 1) => R8_UNORM,
        (1, 2) => R8G8_UNORM,
        (1, 4) => R8G8B8A8_UNORM,
        (1, 8) => R16G16B16A16_UNORM,
        (1, 16) => R32G32B32A32_FLOAT,
        (4, 8) => BC1_UNORM,
        (4, 16) => BC7_UNORM,
        _ => R8G8B8A8_UNORM,
    }
}

/// The SCS pixel-format enum, keyed by `(byte4 high nibble, byte5)`.
///
/// Recovered from the footprint block size plus a decode-coherence test:
/// `(3,5)` decodes cleanly only as BC5 (mean RGB (127,127,0), a normal map);
/// `(0,5)` carries single-channel `_mask`/`_ao` data, so BC4; `(13,4)`/`(14,4)`
/// carry `_c` colour, so BC7; `(7,4)`/`(8,4)` are diffuse, so BC1.
pub const fn format_code(flags: u8, byte5: u8) -> Option<u32> {
    Some(match (flags, byte5) {
        (0x0, 5) => BC4_UNORM,
        (0x7, 4) => BC1_UNORM,
        (0x8, 4) => BC1_UNORM,
        (0x3, 5) => BC5_UNORM,
        (0xD, 4) => BC7_UNORM,
        (0xE, 4) => BC7_UNORM,
        (0xB, 5) => B8G8R8A8_UNORM,
        (0xD, 5) => R8G8B8A8_UNORM,
        (0x8, 5) => R8G8B8A8_UNORM,
        (0x7, 5) => R8G8B8A8_UNORM,
        (0xD, 3) => R8_UNORM,
        _ => return None,
    })
}

/// Expected `(block, bpp)` for a DXGI format, used to validate a code against
/// the footprint that was actually measured.
const fn dxgi_block(dxgi: u32) -> Option<(u32, u32)> {
    Some(match dxgi {
        BC1_UNORM | BC4_UNORM => (4, 8),
        BC3_UNORM | BC5_UNORM | BC6H_UF16 | BC7_UNORM => (4, 16),
        R8_UNORM => (1, 1),
        R8G8_UNORM => (1, 2),
        R8G8B8A8_UNORM | B8G8R8A8_UNORM => (1, 4),
        R16G16B16A16_UNORM => (1, 8),
        R32G32B32A32_FLOAT => (1, 16),
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageMeta {
    pub width: u32,
    pub height: u32,
    pub mips: u32,
    pub is_cube: bool,
    pub faces: u32,
    pub flags: u8,
    pub byte5: u8,
    /// Filled by [`detect_format`].
    pub block: u32,
    pub bpp: u32,
    pub dxgi: u32,
}

/// Parse the 8-byte `kind == 1` image part. Format fields stay zero until
/// [`detect_format`] measures the payload.
pub fn parse_image_meta(img: &[u8]) -> Option<ImageMeta> {
    if img.len() < 8 {
        return None;
    }
    let width = u32::from(u16::from_le_bytes(img[0..2].try_into().expect("2 bytes"))) + 1;
    let height = u32::from(u16::from_le_bytes(img[2..4].try_into().expect("2 bytes"))) + 1;
    if width > 16384 || height > 16384 || u64::from(width) * u64::from(height) > 16 * 1024 * 1024 {
        return None;
    }
    let is_cube = img[6] & 0x01 != 0;
    Some(ImageMeta {
        width,
        height,
        mips: u32::from(img[4] & 0x0F) + 1,
        is_cube,
        faces: if is_cube { 6 } else { 1 },
        flags: img[4] >> 4,
        byte5: img[5],
        block: 0,
        bpp: 0,
        dxgi: 0,
    })
}

/// `(pitch, tight_row, rows, subresource_size)` for one mip.
const fn subres(mw: u32, mh: u32, block: u32, bpp: u32) -> (usize, usize, usize, usize) {
    let (trow, rows) = if block == 1 {
        ((mw * bpp) as usize, mh as usize)
    } else {
        ((mw.div_ceil(4) * bpp) as usize, mh.div_ceil(4) as usize)
    };
    let pitch = align(trow, 256);
    (pitch, trow, rows, pitch * (rows - 1) + trow)
}

/// Total D3D12 upload-layout size for the whole resource.
pub fn footprint(w: u32, h: u32, mips: u32, faces: u32, block: u32, bpp: u32) -> usize {
    if w == 0
        || h == 0
        || w > 16384
        || h > 16384
        || u64::from(w) * u64::from(h) > 16 * 1024 * 1024
        || mips == 0
        || mips > 16
        || !matches!(faces, 1 | 6)
        || !matches!(block, 1 | 4)
        || bpp == 0
        || bpp > 16
    {
        return usize::MAX;
    }
    let mut sizes = Vec::with_capacity((faces * mips) as usize);
    for _ in 0..faces {
        for i in 0..mips {
            let mw = (w >> i).max(1);
            let mh = (h >> i).max(1);
            sizes.push(subres(mw, mh, block, bpp).3);
        }
    }
    let last = sizes.len() - 1;
    sizes
        .iter()
        .enumerate()
        .map(|(k, &s)| if k == last { s } else { align(s, 512) })
        .sum()
}

/// Find the `(block, bpp)` whose footprint equals the payload size, then pick
/// the DXGI format. Returns false if no candidate fits.
pub fn detect_format(meta: &mut ImageMeta, usize_bytes: usize) -> bool {
    for (block, bpp) in CANDIDATES {
        if footprint(meta.width, meta.height, meta.mips, meta.faces, block, bpp) == usize_bytes {
            meta.block = block;
            meta.bpp = bpp;
            // Prefer the exact per-code format when it agrees with the measured
            // block size; otherwise the block size is the more trustworthy of
            // the two.
            meta.dxgi = match format_code(meta.flags, meta.byte5) {
                Some(d) if dxgi_block(d) == Some((block, bpp)) => d,
                _ => default_dxgi(block, bpp),
            };
            return true;
        }
    }
    false
}

/// Un-pad the D3D12 upload layout into the tightly packed faces and mips a
/// standard `.dds` expects.
pub fn untile(payload: &[u8], meta: &ImageMeta) -> Vec<u8> {
    if payload.len() > crate::MAX_ARCHIVE_ENTRY
        || footprint(
            meta.width,
            meta.height,
            meta.mips,
            meta.faces,
            meta.block,
            meta.bpp,
        ) == usize::MAX
    {
        return Vec::new();
    }
    let nsub = (meta.faces * meta.mips) as usize;
    let mut out = Vec::with_capacity(payload.len());
    let mut off = 0usize;
    let mut k = 0usize;
    for _ in 0..meta.faces {
        for i in 0..meta.mips {
            let mw = (meta.width >> i).max(1);
            let mh = (meta.height >> i).max(1);
            let (pitch, trow, rows, size) = subres(mw, mh, meta.block, meta.bpp);
            for r in 0..rows {
                let start = off + r * pitch;
                let end = (start + trow).min(payload.len());
                if start >= payload.len() {
                    break;
                }
                out.extend_from_slice(&payload[start..end]);
            }
            off += size;
            k += 1;
            if k != nsub {
                off = align(off, 512);
            }
        }
    }
    out
}

// --- DDS writing ---------------------------------------------------------

const DDSD_BASE: u32 = 0x1 | 0x2 | 0x4 | 0x1000 | 0x20000; // CAPS|HEIGHT|WIDTH|PIXELFORMAT|MIPMAP
const DDSD_PITCH: u32 = 0x8;
const DDSD_LINEARSIZE: u32 = 0x8_0000;
const DDSCAPS_COMPLEX: u32 = 0x8;
const DDSCAPS_TEXTURE: u32 = 0x1000;
const DDSCAPS_MIPMAP: u32 = 0x40_0000;
const DDSCAPS2_CUBEMAP: u32 = 0x200;
const DDSCAPS2_CUBEMAP_ALLFACES: u32 = 0xFC00;

pub fn write_dds(meta: &ImageMeta, tight: &[u8]) -> Vec<u8> {
    if tight.len() > crate::MAX_ARCHIVE_ENTRY
        || footprint(
            meta.width,
            meta.height,
            meta.mips,
            meta.faces,
            meta.block,
            meta.bpp,
        ) == usize::MAX
    {
        return Vec::new();
    }
    let (pitch_or_linear, flags) = if meta.block == 1 {
        (meta.width * meta.bpp, DDSD_BASE | DDSD_PITCH)
    } else {
        (
            meta.width.div_ceil(4) * meta.height.div_ceil(4) * meta.bpp,
            DDSD_BASE | DDSD_LINEARSIZE,
        )
    };
    let mut caps = DDSCAPS_TEXTURE;
    if meta.mips > 1 {
        caps |= DDSCAPS_MIPMAP | DDSCAPS_COMPLEX;
    }
    let caps2 = if meta.is_cube {
        caps |= DDSCAPS_COMPLEX;
        DDSCAPS2_CUBEMAP | DDSCAPS2_CUBEMAP_ALLFACES
    } else {
        0
    };

    let mut out = Vec::with_capacity(148 + tight.len());
    out.extend_from_slice(b"DDS ");
    let mut push = |v: u32| out.extend_from_slice(&v.to_le_bytes());
    push(124); // dwSize
    push(flags);
    push(meta.height);
    push(meta.width);
    push(pitch_or_linear);
    push(0); // depth
    push(meta.mips);
    for _ in 0..11 {
        push(0); // dwReserved1
    }
    // DDS_PIXELFORMAT with the DX10 FourCC
    push(32); // size
    push(0x4); // DDPF_FOURCC
    out.extend_from_slice(b"DX10");
    let mut push = |v: u32| out.extend_from_slice(&v.to_le_bytes());
    for _ in 0..5 {
        push(0); // bit counts and masks, unused with DX10
    }
    push(caps);
    push(caps2);
    push(0);
    push(0);
    push(0);
    // DDS_HEADER_DXT10
    push(meta.dxgi);
    push(3); // D3D12_RESOURCE_DIMENSION_TEXTURE2D
    push(if meta.is_cube { 0x4 } else { 0 }); // TEXTURECUBE
    push(if meta.is_cube { 1 } else { meta.faces });
    push(0);
    out.extend_from_slice(tight);
    out
}

/// Full pipeline: decompressed tobj payload plus its `kind == 1` bytes to a
/// `.dds`, or `None` if the layout cannot be identified.
pub fn tobj_to_dds(payload: &[u8], image_part: &[u8]) -> Option<Vec<u8>> {
    let mut meta = parse_image_meta(image_part)?;
    if !detect_format(&mut meta, payload.len()) {
        return None;
    }
    Some(write_dds(&meta, &untile(payload, &meta)))
}

// --- .tobj descriptor reconstruction -------------------------------------
// Binary .tobj header (40 bytes) + one texture entry (8 bytes) + path.
// Layout and enums from ConverterPIX (`src/structs/tobj.h`).

const TOBJ_MAGIC: u32 = 1_890_650_625;
const TOBJ_TYPE_2D: u8 = 2;
const TOBJ_TYPE_CUBE: u8 = 5;
const ADDR_REPEAT: u8 = 0;
const ADDR_CLAMP_TO_EDGE: u8 = 2;
const FILTER_DEFAULT: u8 = 3;
const MIP_NOMIPS: u8 = 2;

/// Build a `.tobj` referencing `dds_path` (archive-absolute, e.g.
/// `/vehicle/truck/foo.dds`).
///
/// Texture type and mip policy come from the image metadata. Sampler filtering
/// and addressing use engine defaults: the per-texture sampler state is not
/// stored in a form that round-trips.
pub fn write_tobj(meta: &ImageMeta, dds_path: &str) -> Vec<u8> {
    let mut hdr = [0u8; 40];
    hdr[0..4].copy_from_slice(&TOBJ_MAGIC.to_le_bytes());
    hdr[24] = if meta.is_cube {
        TOBJ_TYPE_CUBE
    } else {
        TOBJ_TYPE_2D
    };
    hdr[26] = FILTER_DEFAULT; // mag
    hdr[27] = FILTER_DEFAULT; // min
    hdr[28] = if meta.mips <= 1 {
        MIP_NOMIPS
    } else {
        FILTER_DEFAULT
    };
    let addr = if meta.is_cube {
        ADDR_CLAMP_TO_EDGE
    } else {
        ADDR_REPEAT
    };
    hdr[30] = addr;
    hdr[31] = addr;
    hdr[32] = addr;
    // hdr[33] compress, hdr[35] anisotropic, hdr[38] colour space: all 0.

    let path = dds_path.as_bytes();
    let mut out = Vec::with_capacity(48 + path.len());
    out.extend_from_slice(&hdr);
    out.extend_from_slice(&(path.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(path);
    out
}

/// The `.dds` path a reconstructed `.tobj` should point at.
pub fn dds_path_for(rel: &str) -> String {
    let mut out = String::with_capacity(rel.len() + 4);
    let _ = write!(out, "{}.dds", rel.strip_suffix(".tobj").unwrap_or(rel));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(w: u32, h: u32, mips: u32, cube: bool) -> ImageMeta {
        ImageMeta {
            width: w,
            height: h,
            mips,
            is_cube: cube,
            faces: if cube { 6 } else { 1 },
            flags: 0,
            byte5: 0,
            block: 0,
            bpp: 0,
            dxgi: 0,
        }
    }

    #[test]
    fn parses_the_image_part() {
        // 256x128, 9 mips, flags 3, byte5 5, 2D
        let img = [0xFF, 0x00, 0x7F, 0x00, 0x38, 0x05, 0x80, 0x09];
        let m = parse_image_meta(&img).unwrap();
        assert_eq!((m.width, m.height), (256, 128));
        assert_eq!(m.mips, 9);
        assert_eq!(m.flags, 0x3);
        assert_eq!(m.byte5, 5);
        assert!(!m.is_cube);
        assert_eq!(m.faces, 1);
        assert!(parse_image_meta(&img[..4]).is_none());
    }

    #[test]
    fn recognises_a_cubemap() {
        let img = [0x3F, 0x00, 0x3F, 0x00, 0x00, 0x04, 0x81, 0x09];
        let m = parse_image_meta(&img).unwrap();
        assert!(m.is_cube);
        assert_eq!(m.faces, 6);
        assert_eq!(m.mips, 1);
    }

    #[test]
    fn footprint_pads_rows_to_256_and_mips_to_512() {
        // One 4x4 BC1 block row: tight row 8 bytes, one row, so no padding
        // between rows and nothing after the only subresource.
        assert_eq!(footprint(4, 4, 1, 1, 4, 8), 8);
        // 8x8 BC1: 2 block columns -> 16 byte rows, 2 rows, pitch 256.
        assert_eq!(footprint(8, 8, 1, 1, 4, 8), 256 + 16);
        // Two mips: the first is padded to 512, the last is not padded.
        assert_eq!(footprint(8, 8, 2, 1, 4, 8), align(256 + 16, 512) + 8);
    }

    #[test]
    fn detects_the_format_from_the_payload_size() {
        let mut m = meta(8, 8, 1, false);
        m.flags = 0x3;
        m.byte5 = 5;
        // BC5 is 16 bytes per block: 2 columns -> 32 byte rows, 2 rows.
        let size = footprint(8, 8, 1, 1, 4, 16);
        assert!(detect_format(&mut m, size));
        assert_eq!((m.block, m.bpp), (4, 16));
        assert_eq!(m.dxgi, BC5_UNORM, "code (3,5) is a normal map");

        // A code that disagrees with the measured block size loses to it.
        let mut m2 = meta(8, 8, 1, false);
        m2.flags = 0x3;
        m2.byte5 = 5; // says BC5 (16-byte) ...
        assert!(detect_format(&mut m2, footprint(8, 8, 1, 1, 4, 8))); // ... but 8-byte
        assert_eq!(m2.dxgi, BC1_UNORM);

        assert!(!detect_format(&mut meta(8, 8, 1, false), 7));
    }

    #[test]
    fn untile_strips_the_row_padding() {
        let mut m = meta(8, 8, 1, false);
        assert!(detect_format(&mut m, footprint(8, 8, 1, 1, 4, 8)));
        // Two 16-byte rows at pitch 256; only the tight bytes come back.
        let mut payload = vec![0u8; 256 + 16];
        payload[..16].copy_from_slice(&[1u8; 16]);
        payload[256..256 + 16].copy_from_slice(&[2u8; 16]);
        let tight = untile(&payload, &m);
        assert_eq!(tight.len(), 32);
        assert_eq!(&tight[..16], &[1u8; 16]);
        assert_eq!(&tight[16..], &[2u8; 16]);
    }

    #[test]
    fn writes_a_dds_a_reader_can_parse() {
        let mut m = meta(8, 8, 1, false);
        assert!(detect_format(&mut m, footprint(8, 8, 1, 1, 4, 8)));
        let dds = write_dds(&m, &[0xAAu8; 32]);
        assert_eq!(&dds[..4], b"DDS ");
        assert_eq!(u32::from_le_bytes(dds[4..8].try_into().unwrap()), 124);
        assert_eq!(u32::from_le_bytes(dds[12..16].try_into().unwrap()), 8); // height
        assert_eq!(u32::from_le_bytes(dds[16..20].try_into().unwrap()), 8); // width
        assert_eq!(&dds[84..88], b"DX10");
        // Header is 4 + 124 + 20 bytes, then the texels.
        assert_eq!(dds.len(), 4 + 124 + 20 + 32);
        assert_eq!(
            u32::from_le_bytes(dds[128..132].try_into().unwrap()),
            BC1_UNORM
        );
    }

    #[test]
    fn writes_a_tobj_pointing_at_the_dds() {
        let mut m = meta(8, 8, 1, false);
        detect_format(&mut m, footprint(8, 8, 1, 1, 4, 8));
        let t = write_tobj(&m, "/material/foo.dds");
        assert_eq!(u32::from_le_bytes(t[..4].try_into().unwrap()), TOBJ_MAGIC);
        assert_eq!(t[24], TOBJ_TYPE_2D);
        assert_eq!(t[28], MIP_NOMIPS, "single mip");
        assert_eq!(&t[48..], b"/material/foo.dds");
        assert_eq!(dds_path_for("material/foo.tobj"), "material/foo.dds");
        assert_eq!(dds_path_for("material/foo"), "material/foo.dds");
    }
}
