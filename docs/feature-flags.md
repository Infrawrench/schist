# Feature flags

The central handler is `schist_app::feature_enabled(name: &str) -> bool`.
Inside the app, call it as:

```rust
if crate::feature_enabled("gpu-compositing") {
    // The feature is available; still check preferences and platform support.
}
```

Register flags and their shipping defaults in `DEFAULTS` in
[`crates/app-settings/src/feature_flags.rs`](../crates/app-settings/src/feature_flags.rs).
Names are case-sensitive. Unknown names always return `false`, even if an
override tries to enable them. Adding a flag means adding its default there
and checking it at the feature's entry point.

## Overrides

On native builds, set `SCHIST_FEATURE_FLAGS` to a JSON object for one run:

```sh
SCHIST_FEATURE_FLAGS='{"gpu-compositing":false}' ./target/debug/schist
```

In the browser, set the same JSON object in localStorage and reload:

```js
localStorage.setItem("schist.feature_flags", JSON.stringify({
  "gpu-compositing": false
}));
location.reload();
```

Only JSON booleans are accepted: `true` and `false`, without quotes. An
absent override uses the registered default. Invalid JSON, a non-object,
or any non-boolean value causes the entire override object to be ignored
with a warning. Unregistered names have no effect.

Overrides are read once on the first call and kept for the session. Restart
the app or reload the page after changing them. Unset the environment
variable or remove the localStorage key to restore the defaults. No network
service or new dependency is required.

## Current flags

| Name | Default | Effect |
| --- | --- | --- |
| `gpu-compositing` | `true` | Allows the GPU compositor and GPU filter/warp kernels. When false, they use the CPU backend. |
| `schist-cloud` | `true` (native only) | Enables Schist Cloud menus, sign-in, gallery, generation, uploads, collaborative editing, and camera backup settings and jobs. Unavailable on WASM. |

The GPU preference and existing `SCHIST_GPU=0|1` override still apply when
this flag is true. Disabling the flag takes precedence over both. The
browser continues to use the CPU backend regardless of the flag, since
its GPU compositor is not supported there.

Schist Cloud is enabled by default on native builds. Disable it with
`SCHIST_FEATURE_FLAGS='{"schist-cloud":false}'`. WASM builds do not register
this feature, so browser overrides cannot enable it. When disabled, Cloud UI
and settings are hidden, stored accounts are not loaded, sign-in callbacks
are ignored, and no Cloud
connection or camera backup starts. Existing account credentials and backup
preferences are kept for when the flag is enabled again.

Run `make check-feature-flags` for the resolver and environment integration
tests, and `make check-cloud-browser` to check the browser build.
