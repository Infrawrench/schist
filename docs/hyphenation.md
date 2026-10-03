# Dictionary hyphenation

Design typography keeps three separate things: native paragraph policy, optional
dictionary break positions, and the glyph selected when a line wraps. Source
stories are never rewritten to insert generated hyphens. Manual U+00AD continues
to work independently of the paragraph's automatic-hyphenation switch.

## Pattern sources and coverage

`schist-layout` pins [Hypher 0.1.8](https://github.com/typst/hypher/tree/v0.1.8),
with default features disabled. Only `alloc`, `english`, `french` and `german`
are enabled. `alloc` is needed for words longer than the dependency's inline
buffer. These tables are public TeX patterns, not Adobe/Hunspell/Proximity vendor
dictionaries; saved native vendor identities remain unchanged.

| Pattern | Accepted identities | Terms |
| --- | --- | --- |
| [hyph-en-us.tex](https://github.com/typst/hypher/blob/v0.1.8/patterns/hyph-en-us.tex) | `en-US`, `en-Latn-US`, native English: USA | Copy/distribute with copyright and permission notice |
| [hyph-fr.tex](https://github.com/typst/hypher/blob/v0.1.8/patterns/hyph-fr.tex) | `fr`, `fr-FR`, with optional Latin script; native French | MIT |
| [hyph-de-1996.tex](https://github.com/typst/hypher/blob/v0.1.8/patterns/hyph-de-1996.tex) | `de-1996`, with optional Latin script/Germany region; native German: Reformed | MIT |

German patterns describe reformed 2006 spelling under the `de-1996` tag. Generic
English/German, British/Canadian English, traditional/Swiss German, other regions,
non-Latin scripts and private/unknown variants do not silently select one of the
above. Declared native resource IDs are opaque and take priority over a matching
tag-shaped spelling. Explicit authored tags bypass the native-ID namespace. A
resource's generic primary name cannot override its unknown or unsupported full
name. Shaping's deliberately broader language resolver remains separate.

`THIRD-PARTY-NOTICES.txt` retains the selected pattern headers and Hypher's MIT
license. Packaging includes it in macOS/iOS app resources, Android assets, the
Windows installer and loose release files, Linux payloads/Flatpak, Web output,
and native/browser/Node library distributions. macOS MCP archives include the
notice alongside the executable. Released AUR recipes still target their pinned
older source archives; they are not changed to require a file absent there.

## Source boundaries and word policies

The word selector inspects complete source words, including when asked for a
slice starting halfway through one. It lowercases and NFC-normalizes a disposable
lookup string per grapheme, then maps dictionary boundaries back to original
UTF-8 grapheme edges. Length-changing casing such as capital sharp S and decomposed
accent sequences keep their source coordinates. Minimum word length is inclusive;
before/after limits count source letter graphemes. Missing word settings use the
public fixture policy: five letters, two on each side, capitalized and final words
allowed. These defaults are composition decisions; they are not serialized into
unset native attributes.

Disabled paragraphs have no automatic opportunities. A language change inside a
word to a different or unavailable dictionary leaves that word alone. Character
ranges use the same first-match precedence as the painter, including an explicit
No Break reset. Final-word exclusion uses the complete paragraph, not the end of
a requested frame slice. Words containing manual SHY, joiners, numbers or foreign
scripts are not automatically rehyphenated. A leading source SHY protects the following
word even though Unicode segmentation excludes that prefix from the word token.
Dictionary-specific spelling changes,
apostrophe rules, ligature decomposition and replacement/multiple hyphen glyphs
are not implemented.

## Integration status

The word selector and generated-glyph renderer are independently tested. The
selector is not yet enabled in ordinary composition: line preference/zone,
consecutive-line limits across threads and column-end restrictions must be wired
before the native automatic-hyphenation switch can honestly claim those policies.
There is no new control or change to the default feature flag in this checkpoint.
External application placement agreement remains unverified.
