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
engine also enforces transient generated-hyphen line policy. Zero consecutive
limit means unlimited; a caller can carry preceding hyphenated lines into a
continuation. Explicit newlines reset that history. Manual source SHY remains an
explicit request and is not disabled by automatic limits. The non-justified zone
counts trailing separators as whitespace, measuring from the preceding word.
For weight, the single-line composer compares squared unused measure, adding
`weight / 100 * measure²` to generated-hyphen candidates. Larger weights prefer
whole-word breaks; a necessary hyphen can still fit a word longer than the line.
This is Schist's documented greedy preference, not an Adobe paragraph-composer
algorithm or placement-parity claim. Every candidate still needs room for its
visible glyph. The policy and generated positions are excluded from serialization.

Ordinary Design composition now selects dictionary opportunities once per source
story, then maps them into disposable inline-reference projections. Generated
footnote digits, affixes and styles cannot change source-word length, language,
No Break or final-word eligibility. A candidate coinciding exactly with a reference
anchor is withheld until reference/hyphen glyph ownership has an explicit policy.
Neither reference numbers nor selected hyphens enter editable source or undo data.

Every frame, column, balance search and footer trial owns its consecutive-line
history. Only accepted lines advance it; split notes carry independent histories.
Missing options resolve to a three-line limit, a 36 pt ragged zone and weight 5;
justified paragraphs use zero zone. Unset native values remain unset on save.
When across-column breaks are prohibited, a column or final frame tail is checked
after paragraph keeps. The complete offending word is excluded from that trial's
automatic candidates and the container is recomposed. Each retry excludes a new
word. Word ownership survives even beyond the last dictionary candidate, so an
unfit continuation suffix also stays overset in a narrower frame. A selected hyphen is
never merely hidden. Bounded path frames apply the same policy to their one line.
Manual source discretionary breaks remain independent explicit requests.

Paragraph has a closed Hyphenation section with icon toggles for automatic
hyphenation, capitalized words, final words and column boundaries. Numeric fields
appear only inside that section while enabled. Blank values inherit; reset restores
inheritance in one undo step. Disabling retains the dormant settings. Edits capture
the named paragraph style, and unchanged formatted zones retain imported precision.
The available pattern languages are shown beside these controls. Design Mode stays
disabled by default; external application placement agreement remains unverified.
