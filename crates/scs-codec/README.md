# SCS codec

TruckCraft adapts the SII, archive, HashFS, ZIP, CityHash, GDeflate and texture
modules from [Wilps93/scs_tools](https://github.com/Wilps93/scs_tools) at commit
`e147e755778579e78f80a406a53c64515113d7a1`. The codec runs in Rust and retains
the upstream [MIT license](LICENSE) and [attribution](UPSTREAM-NOTICES.md).

Archive readers open installed files read-only and return bounded in-memory
byte buffers. The GDeflate port retains the NVIDIA and libdeflate attribution.
The [licenses directory](licenses/) contains the applicable Apache-2.0 and MIT
texts and the NVIDIA and Microsoft copyright notices.

TruckCraft adds limits for inputs, decompression and binary parsing, as well as
string escaping, unsigned sentinel handling and regression tests. Upstream
tests remain alongside the adapted code.
