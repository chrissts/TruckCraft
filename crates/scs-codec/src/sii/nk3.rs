//! `3nK`-obfuscated SII.
//!
//! A six byte header — `u32` signature `0x014B6E33` (`"3nK\x01"`), an unknown
//! byte and a seed — followed by a payload XOR-ed against a fixed 256 entry key
//! table indexed by `(seed + position) & 0xff`.
//!
//! Key schedule from `TheLazyTomcat/3nK_Transcode`.

pub const MAGIC: &[u8; 4] = b"3nK\x01";
pub const HEADER_LEN: usize = 6;

/// `KEY[i] = (((i << 2) ^ !i) << 3) ^ i`, truncated to a byte.
static KEY: [u8; 256] = build_key();

const fn build_key() -> [u8; 256] {
    let mut key = [0u8; 256];
    let mut i = 0usize;
    while i < 256 {
        let b = i as u8;
        key[i] = (((b << 2) ^ !b) << 3) ^ b;
        i += 1;
    }
    key
}

/// True if `data` carries the 3nK signature and a complete header.
pub fn is_3nk(data: &[u8]) -> bool {
    data.len() >= HEADER_LEN && &data[..4] == MAGIC
}

/// Decode a 3nK blob. Returns `data` unchanged if it is not 3nK-encoded.
pub fn decode(data: &[u8]) -> Vec<u8> {
    if !is_3nk(data) {
        return data.to_vec();
    }
    let seed = data[5];
    data[HEADER_LEN..]
        .iter()
        .enumerate()
        .map(|(j, b)| b ^ KEY[(seed as usize + j) & 0xff])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The table the reference builds, recomputed the long way round.
    #[test]
    fn key_table_matches_the_reference_schedule() {
        // Recomputed the long way, in usize, so a byte-width mistake in the
        // const version would show up rather than being reproduced here.
        for (i, &got) in KEY.iter().enumerate() {
            let expected = ((((i << 2) ^ (i ^ 0xff)) << 3) ^ i) & 0xff;
            assert_eq!(got as usize, expected, "key[{i}]");
        }
    }

    #[test]
    fn round_trips_every_seed() {
        let plain: Vec<u8> = b"SiiNunit\n{\n"
            .iter()
            .copied()
            .chain((0..=255u8).cycle().take(768))
            .chain(b"\n}\n".iter().copied())
            .collect();
        for seed in [0u8, 1, 7, 128, 255] {
            let mut blob = vec![b'3', b'n', b'K', 1, 0, seed];
            blob.extend(
                plain
                    .iter()
                    .enumerate()
                    .map(|(j, b)| b ^ KEY[(seed as usize + j) & 0xff]),
            );
            assert_eq!(decode(&blob), plain, "seed {seed}");
        }
    }

    #[test]
    fn passes_through_what_is_not_3nk() {
        assert_eq!(decode(b"SiiNunit"), b"SiiNunit");
        assert!(!is_3nk(b"3nK"));
        assert_eq!(decode(b"3nK\x01\x00\x2a"), b"");
    }
}
