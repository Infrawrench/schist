# OAC Creator 2027 research evidence

The [OAC September 7, 2026 submission announcement](https://www.oac.or.jp/news/5532/)
links the public [InDesign A/B template archive](https://www.oac.or.jp/wp/wp-content/uploads/2026/09/750d314f3f933ac82832971308532050.zip).
Downloaded September 30, 2026. Only the four INDD/IDML/PDF pairs were extracted;
fonts, caches, resource forks and other archive members were skipped.

Despite the release date, **both INDD and IDML metadata identify InDesign 19.5
(Macintosh)**. These are not v21 specimens. All four read-only probes find one
XMP contiguous object following an undecoded database of 576–659 pages.

Redistribution terms for the documents remain unverified, so only provenance and
probe metadata are committed here. `source.json` records the archive digest,
original Unicode and ZIP-decoded member names, local names, sizes and hashes.
Research copies are under `/tmp/schist-external-corpus/oac2027/`. Reproduce by
fetching the linked archive, extracting the listed members, verifying hashes and
running `tools/research/indd_probe.py` on the four INDD paths.

No Adobe SDK headers or executable code were read. This acquisition expands
the comparison corpus; it does not establish database semantics, controlled
one-property changes, or the missing v18/v21 coverage.
