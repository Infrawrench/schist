import assert from "node:assert/strict";
import test from "node:test";
import { LOCALES, STRINGS } from "./i18n-data.js";
import { fromTag, negotiate, fontMatches } from "./i18n.js";

test("all shipped locales have loader strings and round-trip through negotiation", () => {
  for (const [tag, metadata] of Object.entries(LOCALES)) {
    assert.equal(fromTag(tag), tag);
    assert.equal(fromTag(tag.toUpperCase()), tag);
    assert.equal(negotiate(["zz-ZZ", tag, "en"]), tag);
    assert.deepEqual(Object.keys(STRINGS[tag]).sort(), Object.keys(STRINGS.en).sort());
    assert.ok(Object.values(STRINGS[tag]).every((value) => typeof value === "string" && value.length));
    assert.ok(["ltr", "rtl"].includes(metadata.direction));
  }
});

test("POSIX tags, preferred-language ordering, and English fallback", () => {
  assert.equal(fromTag("sv_FI.UTF-8"), "sv");
  assert.equal(fromTag("de_DE@euro"), "de");
  assert.equal(fromTag("zh-Hans-CN"), "zh-Hans");
  assert.equal(fromTag("zh-TW"), "zh-Hans");
  assert.equal(negotiate(["zz", "de", "sv"]), "de");
  assert.equal(negotiate(["C", "POSIX", ""]), "en");
  assert.equal(negotiate([]), "en");
});

test("font selection includes common faces and only the selected locale's extra faces", () => {
  assert.equal(fontMatches({}, "en"), true);
  assert.equal(fontMatches({ lang: "zh" }, "zh-Hans"), true);
  assert.equal(fontMatches({ lang: "zh" }, "ja"), false);
  assert.equal(fontMatches({ locales: ["sv", "de"] }, "de"), true);
  assert.equal(fontMatches({ locales: ["sv", "de"] }, "ja"), false);
});

test("Arabic preferences retain right-to-left metadata and select the script font", () => {
  const selected = negotiate(["ar-SA", "en"]);
  assert.equal(selected, "ar");
  assert.equal(LOCALES[selected].direction, "rtl");
  assert.equal(LOCALES[selected].script, "Arab");
  assert.equal(fontMatches({ locales: ["ar", "fa"] }, selected), true);
  assert.equal(fontMatches({ locales: ["ko"] }, selected), false);
});
