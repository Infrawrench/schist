# Photo metadata and XMP sidecars

In the local gallery, select one or more photos, right-click, and choose
**Photo metadata**. A single photo opens with its current sidecar text and
effective capture time/location. A batch starts with blank, unchecked fields.
Check only the fields to replace; typing also selects the field. An unchecked
field stays unchanged, and a checked empty field clears its value.

- **Keywords** are separated by semicolons. Saving replaces the keyword list;
  duplicate entries and empty entries are removed.
- **Caption** and **Copyright** change the default language value. Existing
  translations in the same XMP language alternative remain intact. Captions
  accept pasted paragraphs; **Shift+Enter** inserts a line break.
- **Date** accepts a complete ISO date/time, such as
  `2026-09-20T14:30:00`, `2026-09-20T14:30:00Z`, or
  `2026-09-20T14:30:00+02:00`. Calendar dates and timezone offsets are validated.
  A time without an offset stays a local camera time; Schist does not invent a
  timezone. Fractional seconds are supported.
- **Offset (s)** shifts each photo's own capture time by a signed number of
  seconds. Use `3600` to add an hour or `-86400` to subtract a day. This and
  **Date** are mutually exclusive. A photo without an XMP or EXIF capture time
  fails individually; its filesystem modification time is never substituted.
- **GPS** accepts `latitude, longitude` in decimal degrees, with `.` as the
  decimal separator. Negative latitude means south; negative longitude means
  west. Values must be finite and within ±90° / ±180°.

Save runs in the background. A partial failure lists the failed paths and leaves
only those photos in the dialog, so retrying a time shift cannot shift successful
photos twice. Detailed filesystem/XML errors are recorded in the application log.
Close dismisses the dialog; a save already running continues.

The gallery immediately refreshes dates, map positions, nearest-city grouping,
and textual metadata search after a save. Searches match all whitespace-separated
query words in keywords, captions, or copyright, even without the semantic search
models. Metadata also participates in smart buckets and headless gallery search.
Use **Refresh**, or reopen the gallery, to pick up edits or sidecar deletions made
by another application. Original EXIF cache entries are kept separately, so
removing a sidecar restores the camera's values. An explicitly empty date/GPS
property suppresses the EXIF value in Schist. Other readers may fall back to
the embedded camera values, which remain in the original. Date grouping uses the ordinary file
clock fallback for undated photos.

## Files and compatibility

Schist never changes the original photo bytes. It writes XML in an adjacent XMP
sidecar using the public [Dublin Core XMP schema](https://developer.adobe.com/xmp/docs/xmp-namespaces/dc/)
and [EXIF XMP schema](https://developer.adobe.com/xmp/docs/xmp-namespaces/exif/):

| Field | XMP property and representation |
| --- | --- |
| Keywords | `dc:subject`, an RDF Bag |
| Caption | `dc:description`, RDF Alt with `xml:lang="x-default"` |
| Copyright | `dc:rights`, RDF Alt with `xml:lang="x-default"` |
| Capture time | `exif:DateTimeOriginal`, ISO 8601 |
| GPS | `exif:GPSLatitude` / `exif:GPSLongitude`, degrees and fractional minutes followed by hemisphere |

An existing `photo.jpg.xmp` takes precedence. Otherwise Schist uses `photo.xmp`
when the filename stem is unique, matching common RAW workflows. If two originals
share a stem, new writes use the unambiguous full filename (`photo.jpg.xmp` and
`photo.raw.xmp`). An existing shared `photo.xmp` is refused in that situation:
Schist cannot safely determine which original it belongs to. Resolve that
ambiguity by assigning it an unambiguous full filename before editing. Both
sidecar conventions are recognized when reading, but other applications vary in
which names and image formats they discover automatically. Non-RAW applications
may require explicit XMP import; metadata is not embedded into exported images.

Namespaces are resolved by URI, so packets written with different namespace
prefixes work. Both attribute and element representations of scalar properties
are supported. Unedited XML, unknown schemas, comments, other RDF subjects and
processing instructions are retained. Localized alternatives remain byte-for-byte
intact when editing the default caption or copyright. Only explicitly selected
properties are changed. Unsupported, malformed, oversized (>8 MiB), ambiguous,
and symlink sidecars are refused rather than rewritten.

Writes use a temporary file in the same directory and atomic publication. An
exclusive lock coordinates Schist saves and gallery moves, and a final content
comparison detects changes made during preparation. Every previous packet is
retained in `.schist/metadata/<original filename>/previous-*.xmp` before replacing
it. External applications do not necessarily honor Schist's advisory lock; avoid
simultaneous edits to the same sidecar in two applications. A process killed
while holding a lock can leave `<sidecar>.schist-lock`; after confirming no save
is running, removing that stale lock permits another attempt.

Moving photos through the gallery also moves their XMP packets and metadata
backups. All destinations are checked before moving anything, publishing files
never replaces an existing destination, and completed transfers are rolled back
on a later failure. Filesystem failures can still require manual cleanup; the
original is moved last and errors are reported.

## Validation

`make check-metadata-xmp` exercises namespace/attribute interoperability,
escaping, translated alternatives and unknown XML, timezone/calendar/GPS
validation, malformed-packet refusal, non-destructive atomic writes and backups,
partial batches, symlink/lock/stem conflicts, warm EXIF cache overlays after
external edits/deletions, gallery search cache invalidation, and sidecar moves
with destination collision preflight. It also compiles all native editor targets.
`make check-i18n` validates all 150 shipped catalogs, aliases, browser font
coverage, and the web language loader. The optional
`make check-metadata-xmp-exiftool` uses ExifTool as an independent reader and
writer to check coordinates, dates, keywords, captions, copyright, and preservation
of foreign rating metadata. No Adobe SDK/header files are used.
