# Translation refresh status

## September 25, 2026: missing feature translations

Ten GPT-6 Sol agents translated **5,067 previously English-identical values
across all 149 non-English registered locales**. This includes **4,343 values**
in the lens-profile, virtual-copy, and advanced-photo-merge catalogs, plus
724 older labels and messages. Counts compare catalog values with the repository
state before this pass; subsequent corrections to already translated values
are not counted again.

Those three feature catalogs now have no unresolved English fallback. Malay
`lens_profiles.import = Import Lensfun XML…` intentionally matches English:
`Import` is also the existing Malay import-command label. The `no` and `sh`
catalogs were regenerated from `nb` and `hr`. All web loading-page catalogs
already had their prose translated, so their text needed no changes. The
150-locale registry is unchanged; the 34 unregistered ISO languages remain
outside this refresh.

The review corrected image cropping translated as harvesting, lens profiles
translated as personal accounts, moving-subject ghosting, color-profile
conversion versus assignment, and references to translated menu labels.
Already-correct shared terms, product names, paper formats, and notation were
retained. Existing translations of identical labels elsewhere in each language
were reused only after checking their context.

`make check-i18n` now runs `tools/check-i18n.py --strict-audit`, which fails on
unchanged English prose containing at least seven words. The ordinary `--audit`
option remains a nonfatal review report. Placeholders do not count as words;
short shared terms and notation remain allowed. The validator also protects
the Lensfun product name. These checks prevent copied English passages from
silently passing validation, but do not establish fluency or detect every
untranslated short label.

Validation completed on the final catalogs:

- `make check-i18n`: 27 Rust tests, one documentation test, six Python audit
  regression tests, all 150 strict catalog audits, generated-file synchronization,
  and four web tests passed. Font character coverage passed with existing assets.
- `git diff --check`: passed.

### Remaining language-review work

Older short-label gaps remain, especially in Akan, Bislama, Bambara, Chamorro,
Ewe, and Marshallese. Some specialized terms in Kinyarwanda, Northern Sámi,
Samoan, Shona, Tswana, and Tongan still need fluent review. English-identical
counts also include valid technical loanwords, units, placeholders, and proper
names, so they are not counts of missing translations.

Specific unresolved examples include Guarani `export_finishing.watermark`
and the Cornish shear, ramp, stamp, and liquify labels (`filter.shear.name`,
`filter.shear.choice.ramp`, `filter.stamp.name`, `menu.filter.liquify`,
`tool.liquify.name`, and `tool.liquify.history.liquify`). Uncertain replacements
were left out. The new specialist prose, particularly in less widely supported
languages, also needs native-speaker review; no fluency certification is claimed.

Agents composed text directly and used existing catalog vocabulary. Google
Translate supplied initial drafts for part of the pass; MyMemory supplied
candidate wording for Russian, Sindhi, Sinhala, Slovak, and Slovenian. Retained
drafts were reviewed and corrected, and unreliable bulk output was reverted.
Later handoffs used direct composition and independent review. Local per-agent
reports, candidate ledgers, and verification logs are under
`target/i18n-missing-refresh/`; those working artifacts are not shipped.

## September 23, 2026: printing refresh (historical)

Updated 2026-09-23. The initial pass used ten GPT-6 Sol subagents, each assigned
14–15 non-English registered locales. A focused completion pass used ten GPT-6 Sol
subagents, each assigned 2–3 of the 27 locales with remaining printing entries.
English remained the source catalog. The existing 150-locale registry is unchanged; the 34
unregistered ISO languages remain outside this pass (see [ISO coverage](i18n-coverage.md)).

The two passes localized **6,586 previously English-identical values across all 149
non-English locales**, mainly the PDF printing and editable print-layout workflows,
plus older controls and labels. The `no` and `sh` aliases were regenerated from
`nb` and `hr`. Web loader prose was already translated and needed no changes.

The printing catalog now has **no unresolved English fallback in all 149
non-English locales**. The completion pass translated all **433 deferred entries
across 27 locales** and corrected 33 additional printing labels or instructions.
Standard paper names, unit-only formats, and valid shared terms such as the Romansh
label `Margins` remain unchanged where appropriate.

## Validation

- `make check-i18n`: 27 Rust tests, one documentation test, catalog validation,
  generated-file synchronization, and four web tests passed. Font character
  coverage passed with the existing assets.
- `python3 tools/check-i18n.py --audit`: all 150 catalogs passed structural checks;
  no unchanged long English sentences were reported.
- `git diff --check`: passed.

The integration review corrected mistranslated cropping and resizing instructions,
fit-to-page wording, source CMYK/Lab profile requirements, protected product names,
and menu breadcrumbs. The completion review also corrected photo-thumbnail sheet
labels, Cornish fitting terminology, Chamorro size/preset wording, and Marshallese
page-label consistency. These checks do not certify fluency or verify rendered
layout. No native-speaker certification is claimed.

## Other terminology needing review

The baseline also contained short English labels that can be valid technical
terms, cognates, style names, units, or product names. They were not replaced
solely to make an English-match counter smaller. The agents identified further
unresolved terminology outside printing, particularly in these catalogs:

| Locales | Outstanding examples |
| --- | --- |
| `bi`, `ch`, `ee`, `mh` | Older common, tool, filter, panel, and dialog labels. |
| `ig` | Blur, noise, pixelation, rendering, sharpening, and artistic-filter names and parameters. |
| `id`, `jv` | Dodge/burn, puppet warp, vanishing point, and some blend-mode names. |
| `ny`, `pi`, `rn`, `rw` | Layer/channel/mask/path/font and navigator labels; noise, texture, plugins, and filter parameters. |
| `tl`, `tn`, `to` | Older filter and dialog terminology. |
| `tw` | Specialized filter/tool names, clipping mask, model dialog, and rendering intents. |
| `ve`, `wa`, `wo`, `yo` | Specialized artistic filters and parameters, such as high-pass, bas-relief, craquelure, and anisotropic modes. |

Some new prose also needs fluent review even when no English remains, particularly
in Akan, Breton, Igbo, Interlingue, Ido, Khmer, Māori, Mongolian, and the smaller
language catalogs. A successful structural audit cannot establish idiomatic wording.

## Translation method and assignments

During the initial pass, agents composed and reviewed the translations. MyMemory supplied candidate
drafts for Afrikaans and parts of Māori, Macedonian, Malayalam, Mongolian, and
Marathi. Google Translate supplied drafts for the 13 non-Sardinian, non-alias
languages in chunk 8. Retained drafts were reviewed and corrected; technical
sentences whose meaning could not be established were restored to English.
Other initial chunks retained no external translation-service output.

The completion pass composed the deferred entries directly using the existing
catalogs and focused lexical references. It retained no external machine-translation
drafts. Review references included the [Cornish SWF dictionary](https://www.cornishdictionary.org.uk/sites/default/files/SWF_dictionary_20190530_final.pdf),
the [revised Chamorro dictionary finder list](https://natibunmarianas.org/wp-content/uploads/2025/01/English-Chamorro-Finders-List.pdf),
and the [Marshallese-English dictionary](https://marshallese.org/dictionary/).
These references support vocabulary choices, not certification of complete sentences.

Initial assignments:

| Chunk | Locales |
| --- | --- |
| 1 | `af`, `ak`, `am`, `an`, `ar`, `as`, `az`, `ba`, `be`, `bg`, `bi`, `bm`, `bn`, `bo`, `br` |
| 2 | `bs`, `ca`, `ch`, `co`, `cs`, `cy`, `da`, `de`, `dv`, `dz`, `ee`, `el`, `eo`, `es`, `et` |
| 3 | `eu`, `fa`, `fi`, `fo`, `fr`, `fy`, `ga`, `gd`, `gl`, `gn`, `gu`, `ha`, `he`, `hi`, `hr` |
| 4 | `ht`, `hu`, `hy`, `ia`, `id`, `ie`, `ig`, `io`, `is`, `it`, `ja`, `jv`, `ka`, `kk`, `km` |
| 5 | `kn`, `ko`, `ku`, `kw`, `ky`, `la`, `lb`, `lg`, `li`, `ln`, `lo`, `lt`, `lv`, `mg`, `mh` |
| 6 | `mi`, `mk`, `ml`, `mn`, `mr`, `ms`, `mt`, `my`, `nb`, `nd`, `ne`, `nl`, `nn`, `no`, `nr` |
| 7 | `ny`, `oc`, `om`, `or`, `pa`, `pi`, `pl`, `ps`, `pt`, `qu`, `rm`, `rn`, `ro`, `ru`, `rw` |
| 8 | `sa`, `sc`, `sd`, `se`, `sh`, `si`, `sk`, `sl`, `sm`, `sn`, `so`, `sq`, `sr`, `ss`, `st` |
| 9 | `su`, `sv`, `sw`, `ta`, `te`, `tg`, `th`, `ti`, `tk`, `tl`, `tn`, `to`, `tr`, `ts`, `tt` |
| 10 | `tw`, `ug`, `uk`, `ur`, `uz`, `ve`, `vi`, `wa`, `wo`, `xh`, `yi`, `yo`, `zh-Hans`, `zu` |

Completion assignments:

| Chunk | Locales |
| --- | --- |
| 1 | `mh`, `om` |
| 2 | `dv`, `bm`, `gn` |
| 3 | `dz`, `bo` |
| 4 | `ee`, `so` |
| 5 | `kw`, `to`, `ny` |
| 6 | `ch`, `ti`, `ss` |
| 7 | `nd`, `qu`, `se` |
| 8 | `nr`, `st`, `sa` |
| 9 | `sc`, `sm`, `si` |
| 10 | `sn`, `pi`, `sd` |

The local working directories `target/i18n-translation-pass/` and
`target/i18n-completion/` contain the baseline inventories, per-chunk reports,
and final validation logs. They are ignored working artifacts; this report
preserves the completion status and material review limitations in the repository.

## Tethered capture — 2026-09-24

Ten GPT-6 Sol subagents translated the 16-entry tethered-capture catalog across
147 independent non-English locales. The `no` and `sh` catalogs were regenerated
from `nb` and `hr`, covering all 149 non-English shipped locales without changing
the English source or language registry. This pass changed 2,312 values, including
all 2,308 that previously matched English. No tethered values now match English.

The review checked placeholders, the 1–120-byte filename limit, the literal
`make` command, and references to each locale's Capture photo and Save As labels.
It corrected the French macOS Camera settings label and the Māori, Malayalam,
and Uyghur Save As references. `make check-i18n` and the targeted tethered audit
passed for all 150 catalogs; existing font assets cover the new text.

These are AI-assisted translations, not native-speaker-certified translations.
Specialized camera-runtime and permission wording in lower-resource languages
still needs fluent review, particularly Cornish, Dzongkha, Ewe, Igbo, Interlingue,
Ido, Marshallese, Northern Ndebele, Pāḷi, Tibetan, and Tongan. Passing structural
and font checks does not establish idiomatic phrasing or verify rendered layout.
The baseline inventory and validation logs are in the ignored
`target/tethered-i18n/` working directory.

The same ten GPT-6 Sol subagents translated four additional strings for the
separate local/cloud save modes and retained-upload recovery path. The catalog
now contains 20 entries in all 150 locales, with `no` and `sh` regenerated from
their canonical catalogs. The `{path}` placeholder and Schist Cloud product name
are preserved. The native-speaker review limitations above also apply to these
new strings.
