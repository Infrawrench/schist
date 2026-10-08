# INDD format research

Status: **Phase 0 decided, 2026-10-07: go for a reader of the recovered
subset, no-go for a writer.** The database layer is mapped and verified on
every page of three specimens from InDesign CC 2014 (Macintosh) and 20.2
(Windows). It is uncompressed and indexed. Every object reassembles to its
indexed length, and every IDML `Self="u…"` id is the UID of the same object.
`schist-codec-indd` reads the subset described below. IDML stays the
write target.

The user permitted public binary document specimens on 2026-09-29. Adobe
SDK headers and proprietary executable decompilation remain excluded. The
2026-10-07 work used only the bytes of the specimens in `fixtures/indd/`,
the IDML InDesign exported beside each, and the public container
descriptions cited below.

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

## The database

Everything in this section was established from the specimens' bytes. It
holds for all three local specimens, `proof` (InDesign CC 2014, Macintosh),
`psu-academic-2` and `psu-literary` (both InDesign 20.2, Windows).
`crates/codec-indd/src/database.rs` implements it, and its tests read every
object of all three.

**Pages.** The file is 4096-byte pages. Every page ends in a twelve-byte
trailer: a 32-bit page type, a 32-bit field whose meaning depends on the
type, and a checksum. The types seen are:

| Type | Role | Second trailer word |
|---|---|---|
| 0 | master page | 0 |
| 2, 3 | allocation pair (free-page bitmaps) | the partner page |
| 4 | page-map index pair | the partner page |
| 5 | page map pair | the partner page |
| 6 | B+tree leaf | its logical page number |
| 7 | B+tree branch | its logical page number |
| 8 | data page | 0 |
| 9 | slotted record page | its logical page number |

**Checksum.** The low sixteen bits are the sum of the first 4092 bytes
modulo 65521, Adler-32's first sum without its initial 1. This holds on all
1,382 pages of the three specimens. The high sixteen bits are a second sum
over the same bytes that is neither Adler-32's nor a linear position
weighting; identical pages in different files carry identical checksums,
so it depends on content alone. It remains unidentified, and a writer would
need it.

**Master page.** Besides the fields above, byte `0x25` selects which copy
of each A/B pair is current (0 the lower-numbered page, 1 the other).
`0xb7c` and `0xb80` hold the logical pages of the location tree's and the
class tree's roots. `0xb88` and `0xb90` hold how many location entries and
objects there are. The UTF-8 document id `xmp.did:…` follows at `0xbac`.
The reader walks the selected page map. If that walk does not hold together
and account for both counts, it tries the other copy.

**Page map.** The current page-map index lists, from offset `0x80`, the
physical pages holding the page map. Each map page lists, from `0x80`, the
physical page of 989 consecutive logical pages. A logically addressed
page's trailer repeats its logical number. That is how a live copy is told
from the stale copies an incremental save leaves behind, and the reader
checks it on every page it visits.

**The two trees.** Both are B+trees keyed by UID. A branch is a count and
then twelve-byte entries: child logical page, then the greatest key under
it as sequence and UID, with `0xffffffff` closing the last. A leaf is a
count and sixteen-byte entries. In the class tree a leaf entry is zero,
UID, class, zero. In the location tree it is sequence, UID, then a word
whose high half is a slot and low half a length, then a page. An object
has one location entry per segment, in sequence order:

- slot 0: `length` bytes from the start of the data page `page`, physical;
- otherwise: record `slot` of the slotted page whose logical number is
  `page`.

**Slotted pages.** A directory sits below the trailer. At `0xfdc` is the
slot capacity, then counts of used slots, the first free slot, the free
offset and the free size. Slot *n*'s offset is at `0xfdc - 4n`. Free
slots chain through small numbers there. A record starts with a word whose
low half is its allocated size and high half its slot. When the slot's
high bit (`0x8000`) is set, the record continues: its next two words hold
the slot (in the high half) and the logical page of the record holding the
rest. Records continue across pages this way when a page fills.

**Nothing is compressed.** All 2,423 objects reassemble to exactly the
length their location entries give. Text appears as plain bytes. All but
raw images and the XMP packet parse as the chunks below. One Proof object
has a class and no location entries; it is empty.

## Objects

**Identity.** Every `Self="u…"` id in the three IDML packages is the
hexadecimal UID of an object in its INDD twin, and every element maps to
one class. Examples: `Spread` 0x501, `MasterSpread` 0x1401, `Page` 0x50f,
`TextFrame` and `Rectangle` 0x6201, `Group` 0x401, `Story` 0x201, `Layer`
0x302, `Guide` 0x3301 and `Section` 0x4c01. The document is UID 1, class
0xe01.

**Chunks.** An object is a run of chunks, each a 32-bit tag, a 32-bit
length and the bytes. A string is a byte belonging to the object, the word
2, a 16-bit character count, then segments. A segment is a 16-bit head
whose low fourteen bits count what follows. A head of `0x4000` is followed
by single bytes, all ASCII in the specimens. A head of `0x8000` is followed
by UTF-16 code units, which carry everything else (U+FEFF markers, U+FFFC
anchors). The same segments make up story text.

**Recovered fields.** Each was checked against every specimen object of
its class and its IDML twin:

| Object | Chunk | Holds |
|---|---|---|
| document | 0x501, 0x1401 | spread and master spread UIDs, in order |
| document | 0x301 | layer UIDs in IDML order, after InDesign's internal pages layer |
| layer | 0x304 | byte 0: internal; byte 2: visible; name string at 18 |
| spread | 0x503 | the spread's per-layer containers |
| spread | 0x56e | `ItemTransform` |
| master spread | 0x1402 | prefix and base name strings |
| layer container (0x301) | 0x302, 0x303 | the document layer; the items in paint order |
| page | 0x5dd | bounds: left, top, right, bottom |
| page | 0x5cc | `ItemTransform` |
| page | 0x140f | applied master UID, a word, `MasterPageTransform` |
| page | 0x51a | margins: left, top, right, bottom; a page without it takes its parent page's |
| any item | 0x15b | spread, parent, children |
| spline item | 0x151 | `ItemTransform` |
| spline item | 0x162b, or on the 0x104 child | subpaths of points; type 2 is a corner point |
| spline item | 0x6e03 | attributes; 0x6e68 with a UID value is the fill swatch |
| item | 0x2c32 | visible |
| group | 0x40d | `ItemTransform` |
| swatch (0x1f05) | 0x1f10, 0x1f01, 0x1f09 | name; space (6 CMYK, 5 RGB, 14 HSB), components as fractions; model (0 process, 1 spot, 2 registration) |
| document setup (0x2202) | 0x533 | page size; at 58, 1 for single and 2 for facing pages; at 70, four bleed sides |
| section (0x4c01) | 0x4c02 | two strings, start page (0 for the first), first number, style (0x4c15 Arabic), whether numbering continues |

**Stories.** A story's 0x223 chunk lists its strands. The 0x234 strand's
0x261 chunk lists blocks with their character counts. Each block (0xca18)
holds runs, and each run is a byte size, a character count and segments.
Paragraphs end in `\r`. U+0018 is the page number, U+0016/U+0017 a table
and U+0004 a footnote. The 0x2a3 strand's blocks (0x27e) give each range of
characters an owner: the story, a table (0xb608) or a footnote (0x24f).
Table cells and footnote text follow the story's own characters in the same
strand, so the owner ranges are how they are left out. A story's frames
come from its frame list (0x228). Its 0x205 chunk names the story and the
columns (0x227) in thread order, and each column's parent is the frame's
text side (0x263), whose parent is the frame.

**Recovered text.** With those rules, every story the three IDML packages
place in a frame matches its IDML text. The one difference is a page break,
which INDD stores as a paragraph return with a break-kind attribute; the
subset reads it as an empty paragraph.

## The reader

`crates/codec-indd` reads the subset above into a `LayoutDocument`, behind
the Design Mode flag. It writes the recovered objects as the IDML parts
InDesign would export for them, under the same ids, and reads those with
`schist-codec-idml`. Spreads, parent pages, threads and inks therefore take
the path the IDML importer's tests already cover.

`crates/codec-indd/tests/specimens.rs` compares the result with the IDML
importer's reading of each twin:

- page sizes, names, margins, bleed, facing pages, parent pages, spreads
  and layers agree;
- every object agrees in page, bounds, transform, visibility, kind, path
  and fill;
- every story placed in a frame matches, except for break kinds;
- 504 damaged variants of the Proof (truncations, byte damage and every
  page zeroed in turn) return errors without panicking.

Pages are named by their number within their sections, as InDesign names
them, and the bleed and facing pages come from the document setup. The
specimens' four bleed sides are all equal, so which side is which is
unverified. Only an equal bleed is read.

The reader reports, rather than drops, what the subset does not read:
styles, strokes, text formatting and frame options; tables and footnotes;
guides; frame contents such as placed images; curved paths; page items of
other classes; Lab and other swatch spaces; unexpected control characters;
sections with prefixes, markers or other number styles; and an unequal
bleed. Big-endian (PowerPC-era Macintosh) files are declined because no
specimen has one.

## Go/no-go, 2026-10-07

The roadmap's Phase 0 questions are answered for the specimens: the master
pages, the object walk, decompression (there is none) and indexing (two
B+trees). The layout held unchanged across eleven years of InDesign and
both platforms. **Go for Phase 5's reader**, which is implemented for the
subset above and grows a field at a time against paired IDML.

**No-go for a writer.** Half of each page checksum is unidentified. The
object payloads hold far more than the subset reads, including styles,
attributes, text attributes and the free-space and allocation bitmaps an
incremental save maintains. A writer would also need specimens from
controlled one-property edits, which the corpus still lacks. IDML remains
the write target, and an INDD document opened in Schist is saved as IDML.

The corpus limits from the earlier sessions still apply. Only three pairs
are on disk; the eight OAC pairs are recorded by provenance only, and no
v18 or v21 specimen has been acquired. Each new specimen should be run
through the specimen tests before the reader is trusted with it.

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

The contiguous object is the XMP packet, which the database also holds
under the same UID. The layout itself is in the database pages described
above.

## Session evidence, 2026-10-07

This is an evidence log, **not a verbatim exported conversation
transcript**. The full Claude Code session (id `ce924eef`) is retained in
the project owner's session history, and must accompany any contribution
that requires complete AI-session transcripts.

1. Read `AGENTS.md`, the roadmap's Phase 0/5 text and this document.
2. Classified every page of the three local specimens by trailer, then
   followed the A/B pairs, the page map and the two trees using bounded,
   read-only Python scripts over the fixture bytes. Located the roots and
   counts in the master page by searching it for the root numbers the walk
   implied.
3. Reassembled every object, finding slotted records and their
   continuations by matching duplicated content across records.
4. Tested standard checksums against the trailers; only the low half
   matched.
5. Matched IDML `Self` ids, element attributes, `PathPointArray` anchors,
   `MarginPreference`, swatch values and `<Content>` text against chunks to
   identify each field in the table above.
6. Implemented `schist-codec-indd` and its specimen tests.

No Adobe SDK headers, plug-in SDK, InDesign executable, decompiler or
disassembler was used. The research scripts were scratch work; the crate
and its tests supersede them as the reproducible record.

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
