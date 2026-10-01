# INDD Phase 0 research

Status: **spike in progress; no production reader/writer go-ahead**.
The user permitted public binary document specimens on 2026-09-29. Adobe
SDK headers and proprietary executable decompilation remain excluded.

## Public container evidence

[ExifTool's independently published InDesign metadata reader](https://github.com/exiftool/exiftool/blob/master/lib/Image/ExifTool/InDesign.pm)
reads two 4096-byte master pages and selects the greater little-endian
64-bit sequence at offset 264. Byte 24 specifies stream byte order; the
little-endian 32-bit count at 280 sets the database extent in 4096-byte
pages. It scans subsequent framed contiguous objects for XMP, and expressly
limits support to metadata. This does not decode the layout database.

[TestDisk's document carver](https://github.com/cgsecurity/testdisk/blob/master/src/file_indd.c)
independently describes the container markers and lengths. Both projects
reference the public XMP Part 3 specification. No implementation code was
copied into Schist; `tools/research/indd_probe.py` is a bounded, read-only
research script using those field descriptions.

## Initial observed specimen

The MIT-licensed [Proof template](https://github.com/kennethormandy/proof)
is recorded with its matching IDML, license, pinned source URLs and hashes
in `fixtures/indd/`. Running:

```sh
python3 tools/research/indd_probe.py fixtures/indd/proof/proof.indd
```

produces the retained `fixtures/indd/proof/probe.json`:

- File size: 1,212,416 bytes.
- Master sequences: 7 and 6; first master selected; little-endian streams.
- Database extent: 287 pages, ending at byte 1,175,552.
- One contiguous object, UID `0x80000001`, class `0xc0000000`, 36,643 bytes.
- Its payload is a four-byte length followed by 36,639 bytes of XML/XMP.
- Trailer identity matches; 157 trailing bytes are all zero.

**The contiguous object is metadata, not the page-layout object stream.**
Most of this sample is inside the preceding database pages, whose layout
semantics this probe does not decode. No inference about database
compression follows from the absence of a zlib stream in the XMP object.
Checksums are recorded but not verified; the algorithm is not established.

## Gate still to meet

Eleven paired templates now span an older release, v19.5 and v20.2. They exceed
the original corpus count but lack v18/v21 and controlled one-property changes.
Database layout semantics, checksums and pairwise semantic equivalence remain
unverified; these are not grounds for an INDD writer. Public acquisition
can continue without proprietary executable analysis. A production codec
must wait for a documented go/no-go supported by those results. IDML is
still the write target; recognizing a container must never be presented as
opening an editable INDD layout.

## Session evidence, 2026-09-29–30

This is an evidence log, **not a verbatim exported conversation transcript**.
The conversation itself must accompany any contribution requiring complete
AI-session transcripts.

1. Read local AGENTS.md, git status and the full roadmap.
2. Searched public web results for INDD/IDML fixture pairs.
3. Read Kenneth Ormandy's repository listing and MIT license through
   GitHub's public API/raw endpoints; pinned commit
   `d08584512fc76a8582cf8633f654414e30ba8d9a`.
4. Downloaded only `proof.indd`, `proof.idml` and `LICENSE.md`; no fonts,
   Adobe SDK headers or executables were acquired.
5. Read ExifTool's `InDesign.pm` and TestDisk's `file_indd.c` as public
   container-format evidence; neither is an Adobe header.
6. Ran the read-only container probe on that explicit document path.
   No decompiler, Adobe executable, INDD writer or production codec ran.
7. Further public searches on 2026-09-30 found commercial templates and
   public artwork guides, but no additional downloaded, version-verified
   paired specimen. A direct GitHub API request from the build sandbox
   failed DNS resolution. No headers or proprietary executables were read.

8. A further primary-source lead is the [OAC Creator 2025 submission
   page](https://www.oac.or.jp/creator2025/detail.html). Its public
   [submission specification](https://www.oac.or.jp/creator2025/dwld/Creator2025_shiyousho.pdf)
   identifies InDesign CC2024 and accompanying IDML. The page links
   `dwld/Creator2025_OACkaiin_format_indd.zip` and
   `dwld/Creator2025_Creators%27sindex_format.zip`. The web tool could not
   fetch these ZIPs; a direct HTTPS download failed DNS resolution in this
   sandbox. Neither ZIP was acquired, inspected or counted in the corpus.
   Redistribution terms and actual file versions remain unverified.


## Additional specimens, September 30, 2026

The OAC download described above succeeded from the unrestricted environment.
Four INDD/IDML/PDF pairs identify **InDesign 19.5 (Macintosh)** in both formats.
Their two/four-page layouts contain populated facing masters and Japanese prose.
Redistribution terms remain unverified, so only exact provenance and probe output
are checked in under `fixtures/indd/oac2025/`.

Two public-domain templates from Penn State Libraries Open Publishing identify
**InDesign 20.2 (Windows)**. Their CC0 dedication, seven/eight-page IDML layouts,
INDD and reference PDFs are included under `fixtures/indd/psu-*`. Public native
IDML now corroborates populated facing masters and a spot Color resource.
Repeated-save tests establish the imported supported subset, not full native
rendering: tables, footnotes, math and anchored objects remain unsupported.

All six new INDD probes show exactly one framed contiguous XMP object, class
`0xc0000000`, followed by zero padding. Database extents range from 434 to 548
4096-byte pages. This corroborates the container map across two modern versions;
it supplies no evidence of the database's page-layout semantics or compression.
No production codec or speculative object-stream decoder has been added.

The public specimen PDFs and the named-tint proof were inspected with Poppler.
Only document data and public XML/API documentation were read; no Adobe SDK
headers, font-cache files, proprietary executable code or decompilation were used.
This remains an evidence log, not an exported AI conversation transcript.


## Further version leads, September 30, 2026

The public [Seiyo Shobo novel template](https://booth.pm/ja/items/751606)
explicitly lists an August 2026 update for InDesign 2026 and accompanying IDML.
Its free download endpoint redirects to BOOTH sign-in. No authenticated download
was available, so it is a lead only, not an acquired or version-verified specimen.
The [OAC Creator 2024 formats](https://www.oac.or.jp/creator2024/detail.html)
are labelled CC2022 (v17), and [NanoLund's poster ZIP](https://www.nano.lu.se/nanolundians/templates-downloads) has a
November 2020 Last-Modified response. Neither supplies verified v18/v21 evidence;
neither archive was downloaded. No corpus count or production gate changed.


The [OAC Creator 2027 release](https://www.oac.or.jp/news/5532/) is dated September 7,
2026. Its public InDesign archive was acquired September 30; four INDD/IDML/PDF
pairs were extracted, skipping fonts, caches and resource forks. Both formats in all four pairs'
XMP CreatorTool values identify **19.5 (Macintosh)**, not v21. Probe and acquisition
metadata are in `fixtures/indd/oac2027/`; document bytes are research-only because
redistribution terms are unverified. Each probe again finds a single XMP object,
following 576–659 undecoded database pages. There are now eleven acquired pairs,
three redistributed, but no new covered version and no change to the production gate.

The Oxford stationery lead now redirects to a page placing staff communication
resources behind SSO. No files were acquired there. Public 2026 product listings
also led to paid/subscription downloads; no purchase, account creation or access
bypass was attempted. No InDesign app bundle was found in the two standard local
Applications directories, so generating controlled paired samples locally is not
available in this environment. No Adobe headers or executable contents were read.

The 2025/2027 A-2 and B-2 IDML comparisons preserve many part IDs and include a
year-text edit, but also color, style and structural changes. The four-page variants
replace most part IDs. These naturally revised templates are not controlled
one-property samples, so their binary differences cannot be attributed to the
year change alone. No inferred object mapping is treated as verified.

On October 1, 2026, a further [public export bug report](https://indesign.uservoice.com/forums/601180-adobe-indesign-bugs/suggestions/50666021-id2026-export-issues)
described a 2026 INDD and a 2025 INDD recreated through IDML. The accessible page
does not expose document download links or the matching IDML. This is another
lead only; no document was acquired or version-verified, and the paired corpus
count remains eleven. Other current search results led to paid templates or the
already recorded Penn State samples. No production gate changed.
