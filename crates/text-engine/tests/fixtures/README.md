# Font fixture

`IBMPlexSans-Light.ttf` is an unmodified IBM Plex Sans Light static face,
redistributed under the SIL Open Font License 1.1 in [OFL.txt](OFL.txt).

Source: [IBM/plex font blob](https://api.github.com/repos/IBM/plex/git/blobs/62292926854d1a963e8351eef8e39222e5752646),
`packages/plex-sans/fonts/complete/ttf/IBMPlexSans-Light.ttf`, retrieved 2026-09-30.
SHA-256: `2218b5f3f1fc9d3a793343f45c3be5ee7eae9584a7a52391bb8cf2d37622b3e0`.
License source: [IBM/plex license blob](https://api.github.com/repos/IBM/plex/git/blobs/c35c4c618fab33da8695177b3a6cefe0810b7b28).

It deliberately has legacy subfamily Regular (name ID 2) and typographic
subfamily Light (ID 17). Tests must select Light by ID 17 and never match its
legacy Regular label as a regular member of the IBM Plex Sans family. The
regular counterpart already ships in `web/fonts/IBMPlexSans-Regular.ttf`.
See the [OpenType naming specification](https://learn.microsoft.com/en-us/typography/opentype/spec/name).

This is font data from IBM's public OFL repository, not an Adobe header or
proprietary application binary. It is a test fixture, not a new bundled UI font.
