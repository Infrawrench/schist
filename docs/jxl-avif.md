# JPEG XL and AVIF

Both formats open and export like PNG or WebP: through the codec registry
(`plugins/codecs-common/src/jxl.rs`, `avif.rs`), so file pickers, drag and
drop, the gallery's thumbnailer and watched folders, export recipes and the
MCP server pick them up from the registered extensions.

## Crates and licences

| Job | Crate | Licence | Targets |
| --- | --- | --- | --- |
| JPEG XL decode | `jxl-oxide` 0.12 | MIT or Apache-2.0 | all |
| JPEG XL lossless encode | `zune-jpegxl` 0.5 | MIT, Apache-2.0 or Zlib | all |
| JPEG XL lossy encode | system libjxl ≥ 0.7, dlopen'd | BSD-3-Clause | Linux, macOS, Windows when installed |
| AV1 encode (AVIF export) | `rav1e` 0.8, no assembly | BSD-2-Clause | all |
| AV1 decode (AVIF import) | `rav1d` 1.1, no assembly | BSD-2-Clause | all but wasm32 |
| HEIF container | written here (`avif/container.rs`) | MIT | all |

No pure-Rust lossy JPEG XL encoder exists under a licence this repository
can take (`jxl-encoder` is AGPL), and building libjxl from source would put
a C++ toolchain in every build, so lossy JPEG XL follows the libheif
precedent: the system's copy is loaded at runtime (`libjxl.so.0.7`–`0.11`,
Homebrew's `libjxl.dylib`, `jxl.dll`), with `libjxl_threads` for
parallelism when it is there. `SCHIST_LIBJXL=/path/to/libjxl.so` points at
another copy and `SCHIST_LIBJXL=none` disables it. Only Schist's own pixels
are handed to it; it never parses a file from elsewhere. Nothing is
downloaded.

rav1d and rav1e are built without their assembly, which would add `nasm` to
every build; both are noticeably slower than with it. rav1d does not
compile for `wasm32-unknown-unknown` (it uses C integer types the `libc`
crate does not define there), so the browser build registers AVIF for export
only and explains why when asked to open one.

## JPEG XL

Import: lossy (VarDCT) and lossless (modular), 8–16-bit integer and float
samples (16-bit and 32-bit documents), alpha (associated alpha is
unpremultiplied), orientation, the first frame of an animation. An embedded
ICC profile becomes the document's profile; an enumerated colour space other
than sRGB (Display P3, Rec. 2020, custom primaries…) gets the profile
jxl-oxide synthesises for it; grey files render to sRGB; PQ and HLG are baked
to sRGB with the same policy as HDR PNG and HEIC. CMYK files are refused.

Export with libjxl: quality 1–100 maps to a Butteraugli distance the way
`cjxl` does (90 is distance 1, 100 is lossless), effort 1–9 (the dialog's 10
is clamped), 8/16-bit integer or 32-bit float samples, alpha, and the
document's ICC profile. libjxl may store a profile it can describe exactly as
an enumerated colour space instead of ICC bytes, so a re-import carries an
equivalent profile rather than the identical one. Documents with a profile
are coded in their own colour space rather than XYB: with libjxl 0.7 a
Display P3 profile through XYB shifted saturated colours visibly, in libjxl's
own decoder too. That costs some compression on profiled documents.

Export without libjxl (browser, iOS, Android, desktops without it):
lossless only, 8 or 16 bits, sRGB. A profiled document is converted to sRGB
first. zune-jpegxl declares a 16-bit file's alpha channel as 8-bit, so
Schist rewrites the image header of 16-bit RGBA output; the frame data is
untouched.

## AVIF

Import: 8/10/12-bit, 4:0:0/4:2:0/4:2:2/4:4:4 (chroma upsampled bilinearly),
full and limited range, BT.601/709/2020/FCC/240M/YCgCo/identity matrices,
alpha (straight or premultiplied), `grid` images, and the `clap`, `irot` and
`imir` transforms. An ICC `colr` box becomes the document's profile; an nclx
colour space other than sRGB gets a profile built from its code points; PQ
and HLG are baked to sRGB. 10- and 12-bit files open as 16-bit documents.
Image sequences (`avis`) open their primary still only.

Export: 4:4:4, BT.601 full range, 8/10/12 bits, quality 1–100 mapped to
rav1e's quantizer, effort 1–10 mapped to rav1e speed 10–1, alpha as a
monochrome auxiliary image, and the document's ICC profile in a `colr` box
alongside the nclx one. Quality 100 codes RGB through the identity matrix at
quantizer 0, which is lossless at 8 and 10 bits; at 12 bits rav1e is not
bit-exact and comes back within about two code values.

## DNG 1.7

`schist-codec-raw` decodes DNG tiles compressed with JPEG XL (compression
52546) through jxl-oxide, integer samples back on their own scale and float
samples as floats. This is tested on synthetic DNGs whose tiles are encoded
by zune-jpegxl; no camera or Adobe DNG Converter file has been tried yet.

## Limits

- Lossy JPEG XL export depends on libjxl being installed.
- AVIF cannot be opened in the browser build.
- Encoding large AVIFs is slow without rav1e's assembly.
- Animated JPEG XL and AVIF sequences import their first frame only.
- JPEG XL JPEG reconstruction and gain maps are not used.
