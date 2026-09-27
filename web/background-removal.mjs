// One worker per action: termination cancels even a synchronous tract kernel
// and releases its WASM memory. Only model downloads use the network; image
// buffers are transferred locally. No SharedArrayBuffer/COOP/COEP required.
export function startBackgroundRemoval(module, js, models, rgb, width, height,
                                       progress, WorkerClass = Worker) {
  const worker = new WorkerClass(new URL("./background-removal-worker.mjs", import.meta.url),
                                 { type: "module", name: "background-removal" });
  let settled = false, resolve, reject;
  const result = new Promise((ok, fail) => { resolve = ok; reject = fail; });
  const finish = (error, value) => {
    if (settled) return;
    settled = true;
    worker.onmessage = worker.onerror = worker.onmessageerror = null;
    worker.terminate();
    if (error) reject(error); else resolve(value);
  };
  worker.onmessage = ({ data }) => {
    if (settled) return;
    if (data.kind === "progress") progress(data.index);
    else if (data.kind === "done") finish(null, data);
    else if (data.kind === "error") finish(new Error(data.error));
    else finish(new Error("invalid background-removal worker reply"));
  };
  worker.onerror = event => finish(new Error(event.message || "background-removal worker failed"));
  worker.onmessageerror = () => finish(new Error("background-removal worker message failed"));
  try {
    worker.postMessage({ module, js, models, rgb, width, height }, [rgb.buffer]);
  } catch (error) {
    finish(error);
  }
  return { result, cancel: () => finish(new Error("cancelled")) };
}

export function installBackgroundRemoval(module, js, host = window) {
  host.__schistBackgroundRemoval = (models, rgb, width, height, progress) =>
    startBackgroundRemoval(module, new URL(js, host.location.href).href,
      JSON.parse(models).map(m => ({ ...m, url: new URL(m.url, host.location.href).href })),
      rgb, width, height, progress);
}
