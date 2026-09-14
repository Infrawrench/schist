// Keep language negotiation consistent with schist_i18n::Locale::from_tag.
import { LOCALES } from "./i18n-data.js";

export function fromTag(tag) {
  const language = String(tag).trim().toLowerCase().split(/[-_.@]/)[0];
  const aliases = { iw: "he", in: "id", ji: "yi", jw: "jv", mo: "ro", fil: "tl" };
  const canonical = aliases[language] ?? language;
  return Object.keys(LOCALES).find((locale) => locale.split("-")[0] === canonical);
}

export function negotiate(preferred) {
  for (const tag of preferred) {
    const locale = fromTag(tag);
    if (locale) return locale;
  }
  return "en";
}

export function fontMatches(font, locale) {
  if (font.locales) return font.locales.includes(locale);
  // Older manifests tag the Chinese subset by its language alone.
  return !font.lang || fromTag(font.lang) === locale;
}
