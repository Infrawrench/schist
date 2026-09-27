// Model bytes stay outside the WASM and startup manifest. Small chunks also
// fit static hosts' file limits. Rust verifies the complete model's SHA-256.
export async function fetchModel(url, progress = () => {}, fetcher = fetch) {
  const response = await fetcher(url);
  if (!response.ok) throw new Error(`model manifest: HTTP ${response.status}`);
  const manifest = await response.json();
  if (!manifest || !Number.isSafeInteger(manifest.bytes) || manifest.bytes <= 0 ||
      manifest.bytes > 512 * 1024 * 1024 || !Array.isArray(manifest.chunks) ||
      !manifest.chunks.length || manifest.chunks.some(c =>
        !c || !/^[a-zA-Z0-9][a-zA-Z0-9_.-]*\.[0-9]{3,}$/.test(c.file) || !Number.isSafeInteger(c.bytes) ||
        c.bytes <= 0 || c.bytes > 16 * 1024 * 1024) ||
      manifest.chunks.reduce((n, c) => n + c.bytes, 0) !== manifest.bytes) {
    throw new Error("invalid model manifest");
  }
  const bytes = new Uint8Array(manifest.bytes);
  let offset = 0;
  for (const chunk of manifest.chunks) {
    const part = await fetcher(new URL(chunk.file, url));
    if (!part.ok) throw new Error(`model chunk: HTTP ${part.status}`);
    if (!part.body) throw new Error("model chunk has no body");
    const reader = part.body.getReader();
    const end = offset + chunk.bytes;
    try {
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        if (offset + value.length > end) throw new Error("oversized model chunk");
        bytes.set(value, offset);
        offset += value.length;
        progress(offset);
      }
      if (offset !== end) throw new Error("truncated model chunk");
    } catch (error) {
      await reader.cancel().catch(() => {});
      throw error;
    } finally {
      reader.releaseLock();
    }
  }
  return bytes;
}

export function installModelLoader(host = window) {
  host.__schistFetchModel = (url, progress) =>
    fetchModel(new URL(url, host.location.href), progress);
}
