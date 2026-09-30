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

## Observed specimen

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

One older template is insufficient for the requested 6–10 paired examples,
v18–v21 coverage, controlled one-property changes, or an INDD writer.
The pair's semantic equivalence also remains unverified. Public acquisition
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
