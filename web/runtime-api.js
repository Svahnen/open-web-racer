export function requireMirrorVisibilityApi(wasmModule) {
  if (typeof wasmModule?.set_preview_mirror_visible !== "function") {
    throw new Error(
      "Generated browser WASM bindings are stale (set_preview_mirror_visible is missing). Rebuild the wasm32 client and rerun wasm-bindgen before starting the app.",
    );
  }
}
