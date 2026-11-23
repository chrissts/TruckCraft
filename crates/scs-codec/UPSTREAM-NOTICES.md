# Third-party notices & attribution

`scs_tools` is an independent, clean-room-ish reimplementation for reading SCS
Software `.scs` archives. The format understanding and some algorithms are
derived from, or ported from, the following projects. Grateful thanks to their
authors.

| Component in this repo | Derived from | Upstream | License |
|------------------------|--------------|----------|---------|
| `src/gdeflate.rs` (GDeflate/DEFLATE64 decoder) | NVIDIA fork of **libdeflate** (`gdeflate` branch) | github.com/NVIDIA/libdeflate | Apache-2.0 (GDeflate additions) / MIT (base libdeflate, © Eric Biggers) |
| GDeflate tile-stream header layout | Microsoft **DirectStorage** (`GDeflate/TileStream.h`) | github.com/microsoft/DirectStorage | Apache-2.0 |
| `src/cityhash.rs` (CityHash64, SCS variant) | Google **CityHash** (as bundled in `String::CityHash` ≤ 0.10) | github.com/google/cityhash | MIT |
| HashFS v1/v2 format & reference reader | **Archive::SCS** by *nautofon* | metacpan.org/dist/Archive-SCS | same terms as Perl 5 (Artistic 1.0 / GPL 1.0+) |
| `.tobj` header/enum layout | **ConverterPIX** by *mwl4* | github.com/mwl4/ConverterPIX | GPL-3.0 (referenced for the struct layout only) |
| `src/sii/nk3.rs` (3nK decode algorithm) | **3nK_Transcode** by *TheLazyTomcat* | github.com/TheLazyTomcat/3nK_Transcode | (algorithm reference) |
| `src/sii/bsii/`, `src/sii/scsc.rs` (BSII layout & type table, ScsC container, text output rules) | **SII_Decrypt** by *František Milt (TheLazyTomcat)* | github.com/TheLazyTomcat/SII_Decrypt | MPL-2.0 (referenced for the format documentation and algorithms) |
| `src/sii/bsii/` (cross-checked against it during development) | **sii-decode-rs** by *fangyi-zhou* | github.com/fangyi-zhou/sii-decode-rs | MIT |
| `src/ets2.rs` (delivery-log `params` slot numbers) | SCS forum community reverse engineering | forum.scssoft.com, topic 317674 | (findings, not code) |

Notes:
* `src/gdeflate.rs` is a faithful port of NVIDIA's libdeflate GDeflate
  decoder; its output is byte-identical to that reference. Retain the upstream
  copyright/license notices when redistributing.
* The BSII and ScsC code was written from the published format documentation
  (`Documents/Binary SII - {Format,Types}.txt`) rather than translated from the
  Pascal sources; the text-output rules were matched against `SII_Decode_*` so
  that the result is comparable with SII_Decrypt's.
* The `ScsC` AES key in `src/sii/scsc.rs` has been publicly documented since
  SII_Decrypt. It is included so that files you own can be read; nothing here
  encrypts.
* SCS Software, Euro Truck Simulator 2 and American Truck Simulator are
  trademarks of SCS Software. This project is not affiliated with or endorsed by
  SCS Software; it is an interoperability tool for reading files you own.
