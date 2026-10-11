import test from "node:test";
import assert from "node:assert/strict";
import { requireMirrorVisibilityApi } from "../../web/runtime-api.js";

test("runtime rejects stale generated WASM bindings without the mirror visibility export", () => {
  assert.doesNotThrow(() => requireMirrorVisibilityApi({ set_preview_mirror_visible() {} }));
  assert.throws(
    () => requireMirrorVisibilityApi({}),
    /Generated browser WASM bindings are stale.*missing\)\. Rebuild the wasm32 client.*wasm-bindgen/,
  );
});
