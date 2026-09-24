// Usage: node tools/bench-paint.mjs path/to/paintbench.js [path/to/baseline.js]
import { createRequire } from "node:module";
import { resolve } from "node:path";
const require = createRequire(import.meta.url);
const engines = [require(resolve(process.argv[2])).paint_benchmark];
if (process.argv[3]) engines.push(require(resolve(process.argv[3])).paint_benchmark);
for (const diameter of [32, 128, 300]) {
  // Warm both engines, then alternate their order to limit timing bias.
  for (let i = 0; i < 3; i++) for (const paint of engines) paint(diameter);
  const samples = engines.map(() => ({ wall: [], cpu: [] }));
  let checksum;
  for (let i = 0; i < 7; i++) {
    const order = engines.map((_, j) => j);
    if (i % 2) order.reverse();
    for (const j of order) {
      const start = performance.now();
      const cpuStart = process.cpuUsage();
      const actual = engines[j](diameter);
      const cpu = process.cpuUsage(cpuStart);
      samples[j].wall.push(performance.now() - start);
      samples[j].cpu.push((cpu.user + cpu.system) / 1000);
      if (checksum !== undefined && actual !== checksum) {
        throw new Error(`Pixel checksum mismatch at ${diameter}px: ${actual} != ${checksum}`);
      }
      checksum = actual;
    }
  }
  const median = (values) => values.sort((a, b) => a - b)[3].toFixed(1);
  for (const [j, sample] of samples.entries()) {
    console.log(`${diameter}px ${j ? "baseline" : "current"}: ${median(sample.wall)} ms wall, ${median(sample.cpu)} ms CPU, checksum ${checksum}`);
  }
}
