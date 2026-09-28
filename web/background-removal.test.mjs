import test from "node:test";
import assert from "node:assert/strict";
import { installBackgroundRemoval, startBackgroundRemoval } from "./background-removal.mjs";

class Worker {
  static instances = [];
  constructor(url, options) { this.url = url; this.options = options; this.stops = 0; Worker.instances.push(this); }
  postMessage(data, transfer) { this.input = structuredClone(data, { transfer }); }
  terminate() { this.stops++; }
}
function start() {
  const rgb = new Float32Array([0.2, 0.4, 0.6]), progress = [];
  const job = startBackgroundRemoval(null, "https://example.test/editor/pkg/schist.js", [], rgb, 1, 1, n => progress.push(n), Worker);
  return { job, rgb, progress, worker: Worker.instances.at(-1) };
}

test("installation is lazy; no worker or model download at boot", () => {
  const count = Worker.instances.length, host = {};
  installBackgroundRemoval(null, "pkg/schist.js", host);
  assert.equal(typeof host.__schistBackgroundRemoval, "function");
  assert.equal(Worker.instances.length, count);
});

test("input transfers to a module worker; success releases memory exactly once", async () => {
  const { job, rgb, worker, progress } = start();
  assert.equal(rgb.byteLength, 0);
  assert.equal(worker.input.rgb.length, 3);
  assert.equal(worker.options.type, "module");
  worker.onmessage({ data: { kind: "progress", index: 2 } });
  assert.deepEqual(progress, [2]);
  const output = { kind: "done", alpha: new Float32Array([1]), foreground: worker.input.rgb };
  worker.onmessage({ data: output });
  assert.equal(await job.result, output);
  job.cancel();
  assert.equal(worker.stops, 1);
  assert.equal(worker.onmessage, null);
});

test("cancel stops blocking inference and late replies cannot succeed", async () => {
  const { job, worker } = start(), reply = worker.onmessage;
  const rejected = assert.rejects(job.result, /cancelled/);
  job.cancel(); job.cancel();
  reply({ data: { kind: "done" } });
  await rejected;
  assert.equal(worker.stops, 1);
});

test("fetch/checksum/panic/transport failures terminate the worker and allow retry", async () => {
  for (const fail of [
    w => w.onmessage({ data: { kind: "error", error: "checksum mismatch" } }),
    w => w.onerror({ message: "unreachable" }),
    w => w.onmessageerror(),
    w => w.onmessage({ data: { kind: "unexpected" } }),
  ]) {
    const { job, worker } = start();
    fail(worker);
    await assert.rejects(job.result);
    assert.equal(worker.stops, 1);
  }
  const { job, worker } = start();
  worker.onmessage({ data: { kind: "done" } });
  await job.result;
});

test("postMessage failures reject and dispose instead of leaking a worker", async () => {
  class BrokenWorker extends Worker { postMessage() { throw new Error("clone failed"); } }
  const job = startBackgroundRemoval(null, "", [], new Float32Array(3), 1, 1, () => {}, BrokenWorker);
  await assert.rejects(job.result, /clone failed/);
  assert.equal(Worker.instances.at(-1).stops, 1);
});
