# OAC Creator 2025 research evidence

The [OAC submission page](https://www.oac.or.jp/creator2025/detail.html)
links the public [InDesign template archive](https://www.oac.or.jp/creator2025/dwld/Creator2025_OACkaiin_format_indd.zip).
Its [specification](https://www.oac.or.jp/creator2025/dwld/Creator2025_shiyousho.pdf)
requests InDesign CC2024 and IDML. The archive was acquired September 30, 2026.
Only its four INDD/IDML/PDF pairs were extracted; fonts and macOS resource forks
were skipped. Both IDML and INDD XMP identify InDesign 19.5 on Macintosh.

This directory retains metadata and read-only container observations only.
Redistribution terms for the documents have not been established, so their bytes
are not included in the repository. `source.json` records the archive URL/hash,
original Japanese member names, shortened local names and file hashes. To reproduce,
fetch that archive, extract the explicitly listed members, verify their SHA-256
hashes and run `tools/research/indd_probe.py` on the four INDD paths. The research
copies are currently under `/tmp/schist-external-corpus/oac2025/`.

The IDML contains two-page spreads, a populated facing master and Japanese text.
The A-4 sample imports with four pages, 130 page objects and 54 stories. The
reference PDF was visually inspected; matching native application rendering is
not established. No page OverrideList was populated in these four samples.
