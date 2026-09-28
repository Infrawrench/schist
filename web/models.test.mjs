import test from "node:test";
import assert from "node:assert/strict";
import { fetchModel, installModelLoader } from "./models.mjs";

const url = "https://example.test/editor/assets/models/model.json";
function server(manifest, parts) {
  const requested = [];
  const fetcher = async path => {
    requested.push(String(path));
    if (String(path) === url) return Response.json(manifest);
    const value = parts[new URL(path).pathname.split("/").pop()];
    return value === undefined ? new Response(null, { status: 404 }) : new Response(value);
  };
  return { fetcher, requested };
}

test("models are not fetched when the loader is installed", () => {
  const host = { location: { href: "https://example.test/editor/" } };
  installModelLoader(host);
  assert.equal(typeof host.__schistFetchModel, "function");
});

test("chunks reconstruct exact bytes with progress and subdirectory URLs", async () => {
  const { fetcher, requested } = server({ bytes: 5, chunks: [
    { file: "model.000", bytes: 3 }, { file: "model.001", bytes: 2 },
  ] }, { "model.000": new Uint8Array([1, 2, 3]), "model.001": new Uint8Array([4, 5]) });
  const progress = [];
  assert.deepEqual(await fetchModel(url, n => progress.push(n), fetcher), new Uint8Array([1, 2, 3, 4, 5]));
  assert.deepEqual(progress, [3, 5]);
  assert.deepEqual(requested, [url, new URL("model.000", url).href, new URL("model.001", url).href]);
});

test("missing, short and oversized chunks reject instead of installing partial models", async () => {
  for (const [part, expected] of [[undefined, /404/], [new Uint8Array(1), /truncated/], [new Uint8Array(3), /oversized/]]) {
    const { fetcher } = server({ bytes: 2, chunks: [{ file: "model.000", bytes: 2 }] }, { "model.000": part });
    await assert.rejects(fetchModel(url, () => {}, fetcher), expected);
  }
});

test("invalid lengths and paths fail before allocating or requesting chunks", async () => {
  for (const manifest of [
    { bytes: 0, chunks: [] }, { bytes: 2 ** 40, chunks: [] },
    { bytes: 1, chunks: [{ file: "../secret", bytes: 1 }] },
    { bytes: 1, chunks: [{ file: "model.000", bytes: 2 }] },
    { bytes: 1, chunks: [{ file: "model.000", bytes: -1 }] },
  ]) {
    const { fetcher, requested } = server(manifest, {});
    await assert.rejects(fetchModel(url, () => {}, fetcher), /invalid model manifest/);
    assert.deepEqual(requested, [url]);
  }
});
