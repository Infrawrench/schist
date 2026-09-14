# ISO language coverage

The translation expansion targets the 184 entries in
[`iso-639-1.tsv`](../crates/i18n/data/iso-639-1.tsv). This inventory includes the
legacy `sh` macrolanguage and excludes retired `bh`. The authoritative shipped
list is [`locales.tsv`](../crates/i18n/locales.tsv).

The translation expansion has 150 registered locales: the five original
languages plus 145 additions. Translation workers have finished their current
assignments. The 34 languages below remain unregistered, so the all-ISO target
is incomplete.

GPT-5.6 Sol workers composed the translations, with separate Sol reviews and
GPT-6 review and integration. Catalog validation checks completeness, plural
categories, placeholders, product names, navigation labels, command literals,
and font coverage. Those checks cannot establish fluency. The translations
have not received human native-speaker certification.

## Languages requiring further translation work

These drafts failed semantic review and require fluent recomposition or
substantial review. Some passed every structural check but still changed basic
meanings or used unreliable grammar. They are excluded from the shipped registry.

| Code | Language | Unresolved issue from the translation attempt |
| --- | --- | --- |
| `aa` | Afar | Basic word meanings and sentence grammar remained unreliable after limited corrections. |
| `ab` | Abkhaz | The author could not reliably compose the required tool descriptions and loader prose. |
| `ce` | Chechen | Repeated layer/room confusion and dependent case and agreement errors persisted in a fresh rewrite. |
| `cr` | Cree | The draft needs dependable, consistent Cree grammar and technical prose. |
| `gv` | Manx | Wrong verb forms, clause meanings, and unsupported terminology remained throughout the draft. |
| `hz` | Herero | Basic meanings, noun-class agreement, and diagnostic and synchronization prose remained unreliable. |
| `kg` | Kongo | The draft used Kituba rather than a language variety covered by the intended Kongo catalog. |
| `ki` | Kikuyu | Recurring wrong senses and agreement errors persisted after review. |
| `kj` | Kuanyama | Basic tool-description meanings and productive grammar could not be corrected reliably. |
| `kv` | Komi | Basic labels and long technical clauses still require fluent recomposition. |
| `ng` | Ndonga | Basic colors, opacity, diagnostic meanings, and sentence grammar were unreliable. |
| `os` | Ossetian | Basic terminology and all long tool descriptions remained linguistically untrustworthy. |
| `sg` | Sango | Repeated lexical collisions and sentence-level problems remained after independent review. |
| `ty` | Tahitian | Basic meanings such as blur, sponge, paper, and empty states were repeatedly wrong. |
| `vo` | Volapük | Layer, texture, application, and other basic meanings were wrong, with dependent prose needing recomposition. |

For these additional languages, workers assessed the actual source prose but
could not produce dependable grammar. No usable catalog was generated:

| Code | Language | Code | Language |
| --- | --- | --- | --- |
| `ae` | Avestan | `av` | Avar |
| `ay` | Aymara | `cu` | Church Slavic |
| `cv` | Chuvash | `ff` | Fulah |
| `fj` | Fijian | `ho` | Hiri Motu |
| `ii` | Sichuan Yi | `ik` | Iñupiaq |
| `iu` | Inuktitut | `kl` | Kalaallisut |
| `kr` | Kanuri | `ks` | Kashmiri |
| `lu` | Luba-Katanga | `na` | Nauruan |
| `nv` | Navajo | `oj` | Ojibwe |
| `za` | Zhuang | | |

These limitations concern the attempted translations, not whether the languages
can express software concepts. Related languages must not be substituted. In
particular, Luba-Kasai is not Luba-Katanga, and Tok Pisin is not Hiri Motu.

Rejected and incomplete drafts are preserved locally under
`target/i18n-unverified/`, outside the catalog tree. That directory is a local
working artifact, not part of the shipped application. The translation workflow
and requirements for registering a replacement are in [i18n.md](i18n.md).

## Verification and limits

Final validation of the 150 registered locales passed:

| Check | Result |
| --- | --- |
| `make check-i18n` | 23 Rust unit tests and one documentation test passed, including catalog-directory and font character coverage checks; catalog validation, generated platform/loader synchronization, and all four JavaScript tests passed. No tests were skipped. |
| `python3 tools/check-i18n.py --audit` | All 150 registered catalogs passed; no unchanged long English sentences were reported. Short technical names can remain in English. |
| `make app PROFILE=debug` | Native application build passed. |
| `python3 tools/check-i18n.py --all-iso` | Failed with exactly the 34 missing languages listed above. |

There are no unregistered draft directories in the active catalog tree.
The stricter all-ISO check requires every inventory entry; the unresolved
languages must not be replaced with English scaffolds to make it pass.

Browser visual verification was blocked by the in-app browser tool failing
before script execution with a missing `sandboxPolicy` error. Font character
coverage checks are separate from visual verification. The change does not
localize numeric input formats or mirror the native panel layout for RTL
languages; those existing limits are documented in [i18n.md](i18n.md).
