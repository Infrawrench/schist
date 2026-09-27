import { fetchModel } from "./models.mjs";

self.onmessage = async ({ data: { module, js, models, rgb, width, height } }) => {
  // No overlapping jobs or model stores within this WASM instance.
  self.onmessage = null;
  try {
    const api = await import(js);
    await api.default({ module_or_path: module });
    for (const [index, model] of models.entries()) {
      self.postMessage({ kind: "progress", index });
      const bytes = await fetchModel(model.url);
      api.background_removal_install(model.id, bytes);
    }
    self.postMessage({ kind: "progress", index: models.length });
    const output = api.background_removal_infer(rgb, width, height);
    try {
      const alpha = output.take_alpha(), foreground = output.take_foreground();
      self.postMessage({ kind: "done", alpha, foreground }, [alpha.buffer, foreground.buffer]);
    } finally {
      output.free();
    }
  } catch (error) {
    self.postMessage({ kind: "error", error: String(error?.stack || error) });
  }
};
