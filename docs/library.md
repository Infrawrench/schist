# Headless shared library

`make library` builds the editor's first-party tools, commands, filters,
adjustments and codecs, plus face detection and recognition, as:

- `dist/library/libschist.so` (Linux; `.dylib` on macOS, `.dll` on Windows).
- `dist/library/schist.h`, the versioned C ABI.
- `dist/library/wasm/schist.js`, `schist.d.ts`, and `schist_bg.wasm` for browsers.
- `dist/library/node/`, equivalent CommonJS bindings for Node.

`make library-native` and `make library-wasm` build either distribution.
`PROFILE=debug` selects development builds. WASM needs the Rust
`wasm32-unknown-unknown` target and `wasm-bindgen-cli` matching `Cargo.lock`.
No UI is included; an embeddable UI library is deferred.

Use these Make targets, including for downstream packaging. They compile with
`schist_library` in a separate `target/library` directory. This configuration
removes the desktop's mutable locale, ID counters, compositor/effect backends,
font caches and neural-filter cache from the library's execution paths. English
protocol labels are immutable; rendering and effects use the CPU implementations.
Font/codec resources are local to calls, and People models belong to the instance.
The browser build carries a fallback IBM Plex Sans face and built-in filter models.
Native text also sees installed system fonts. Immutable data such as model weights
and lookup tables may be shared. The desktop build keeps its existing configuration.

Each `SchistApp *` or JavaScript `Schist` owns its documents, histories,
registries, gestures, clipboard and People models. Destroying it releases those
resources. There is no global handle table or last-error slot. Calls on one handle
must be serialized; independent handles can be used concurrently. The library
does not install a logger/panic hook, start services, scan third-party plugin
directories, or download models. GPU selection, desktop plugin hosts, filesystem
gallery services, and platform integrations belong to the desktop host.

The C API catches Rust unwinds and marks that handle unusable; status 2 means
destroy it. Invalid memory supplied through the C ABI is outside this contract.
WASM traps must be handled by the host (cloud uses a bounded worker thread).

## C

Include `schist.h`, link `-lschist`, and check `schist_abi_version() == 1`.
All inputs are borrowed byte spans; outputs are owned, length-delimited buffers,
including error messages. Call `schist_buffer_free()` on every output. Never use
`free()` from the host allocator on library buffers. `schist_destroy(NULL)` and
freeing an already-cleared buffer are harmless.

```c
SchistApp *app = schist_create();
SchistBuffer out = {0};
const char request[] = "{\"op\":\"create\",\"width\":64,\"height\":64}";
int status = schist_request(app, (const uint8_t *)request,
                           sizeof(request) - 1, &out);
/* status 0: out contains {"session":1}; status 1/2: a UTF-8 error */
schist_buffer_free(&out);
schist_destroy(app);
```

See `examples/library/smoke.c` for a runnable example. Binary file and ONNX
inputs can bypass JSON/base64 using `schist_import` and `schist_load_model`.

## JavaScript

```js
import init, { Schist } from './wasm/schist.js';
await init();
const app = new Schist();
try {
  const { session } = JSON.parse(app.request(JSON.stringify({
    op: 'create', width: 64, height: 64, depth: 8,
  })));
  app.request(JSON.stringify({
    op: 'call', session, name: 'adjust_invert', args: {},
  }));
  const png = app.exportFile(session, 'png', 8); // Uint8Array
} finally {
  app.free();
}
```

Node uses `const { Schist } = require('./node/schist.js')`; initialization is
synchronous and needs no DOM or canvas. `importFile(name, Uint8Array)` returns
a session ID. `loadModel(id, Uint8Array)` loads one instance's ONNX model.

## JSON operations

| `op` | Other fields | Result |
| --- | --- | --- |
| `catalog` | none | Action schemas and import/export formats |
| `create` | `width`, `height`, optional `title`, `depth` (8/16/32) | `{session}` |
| `sessions` | none | This instance's session IDs |
| `close` | `session` | `null`; discards that document |
| `import` | `name`, base64 `data` | `{session}` |
| `export` | `session`, `extension`, optional `quality`, `bit_depth`, `dither` | Base64 `{data}` |
| `call` | `session`, `name`, optional `args` object | MCP content array |
| `load_model` | `id`, base64 `data` | `null` |
| `unload_model` | `id` | `null` |
| `detect_faces` | `width`, `height`, byte-array `rgb` | Normalized rectangles |
| `people` | `width`, `height`, byte-array `rgb`, optional `boxes` | `{rect, embedding}[]` |

`call` shares the app's MCP catalog and dispatch: brush/type/vector/selection
tools, strokes and keyboard input, layer properties, undo/redo, commands,
filters, adjustments, state inspection, and rendering. Rendering returns PNG
content; disk-write arguments are rejected. File import/export uses byte buffers
on both platforms. A library JSON request is limited to 24 MiB. Blank documents
are limited to 64 megapixels and 30,000 pixels per side.
JSON export defaults to the document's bit depth, where the codec supports it.

## Replacing schist-people-worker

The separate `schist-people-worker` crate, binary and Make target are removed.
Load the existing `face` (UltraFace) and `face-embed` (SFace) ONNX files with
`loadModel`/`schist_load_model`, then call `people`. The previous worker's
`{width,height,rgb,boxes?}` input and `{rect,embedding}[]` output are preserved,
with `op: "people"` added to the request. `detect_faces` needs only the detector.
Supplying boxes needs only the recognizer; an empty list returns an empty result.

Inputs are RGB8, 1–1024 pixels per side, at most 100 boxes. Rectangles are normalized
and clamped. The shared desktop crop enlarges boxes by 1.1, makes them square,
resizes to 112×112, and produces 128 finite, normalized embedding values.
Invalid replacement models leave the previously loaded model intact. No models
are loaded from an environment variable by the People API.

`make check-library` tests editing isolation, codec round trips, C ownership,
and the actual ONNX pipeline with small synthetic fixtures.
`make check-library-wasm` checks the portable build. Native and Node smoke tests
in `examples/library` exercise the built distributions.
