# Third-party dependency inventory

Generated from the working `Cargo.lock` using `cargo metadata --locked` on 2026-10-07 and the resolved npm `package-lock.json` / installed package manifests. The Rust inventory includes all 207 registry packages in the workspace dependency graph, including the new server SHA-256 verifier dependency; the npm inventory includes all three transitive Playwright packages. Sources are crates.io and npmjs.org as listed. SPDX expressions are taken from package metadata and checked with `cargo deny check licenses`; see repository lockfiles for exact source checksums.

Project code remains Apache-2.0. Third-party license terms govern each dependency. No third-party source code is vendored. License and notice texts are distributed by their respective package sources; this table identifies the exact resolved packages.

## Rust crates

| Package | Version | SPDX license expression | Source | Repository |
|---|---:|---|---|---|
| `addr2line` | `0.25.1` | `Apache-2.0 OR MIT` | crates.io | https://github.com/gimli-rs/addr2line |
| `adler2` | `2.0.1` | `0BSD OR MIT OR Apache-2.0` | crates.io | https://github.com/oyvindln/adler2 |
| `ahash` | `0.8.12` | `MIT OR Apache-2.0` | crates.io | https://github.com/tkaitchuck/ahash |
| `allocator-api2` | `0.2.21` | `MIT OR Apache-2.0` | crates.io | https://github.com/zakarumych/allocator-api2 |
| `android_system_properties` | `0.1.6` | `MIT OR Apache-2.0` | crates.io | https://github.com/nical/android_system_properties |
| `arrayvec` | `0.7.8` | `MIT OR Apache-2.0` | crates.io | https://github.com/bluss/arrayvec |
| `ash` | `0.37.3+1.3.251` | `MIT OR Apache-2.0` | crates.io | https://github.com/MaikKlein/ash |
| `atomic-waker` | `1.1.2` | `Apache-2.0 OR MIT` | crates.io | https://github.com/smol-rs/atomic-waker |
| `autocfg` | `1.5.1` | `Apache-2.0 OR MIT` | crates.io | https://github.com/cuviper/autocfg |
| `axum` | `0.8.4` | `MIT` | crates.io | https://github.com/tokio-rs/axum |
| `axum-core` | `0.5.6` | `MIT` | crates.io | https://github.com/tokio-rs/axum |
| `backtrace` | `0.3.76` | `MIT OR Apache-2.0` | crates.io | https://github.com/rust-lang/backtrace-rs |
| `base64` | `0.22.1` | `MIT OR Apache-2.0` | crates.io | https://github.com/marshallpierce/rust-base64 |
| `bit-set` | `0.5.3` | `MIT/Apache-2.0` | crates.io | https://github.com/contain-rs/bit-set |
| `bit-vec` | `0.6.3` | `MIT/Apache-2.0` | crates.io | https://github.com/contain-rs/bit-vec |
| `bitflags` | `1.3.2` | `MIT/Apache-2.0` | crates.io | https://github.com/bitflags/bitflags |
| `bitflags` | `2.13.2` | `MIT OR Apache-2.0` | crates.io | https://github.com/bitflags/bitflags |
| `block` | `0.1.6` | `MIT` | crates.io | http://github.com/SSheldon/rust-block |
| `block-buffer` | `0.10.4` | `MIT OR Apache-2.0` | crates.io | https://github.com/RustCrypto/utils |
| `bumpalo` | `3.20.3` | `MIT OR Apache-2.0` | crates.io | https://github.com/fitzgen/bumpalo |
| `bytemuck` | `1.25.2` | `Zlib OR Apache-2.0 OR MIT` | crates.io | https://github.com/Lokathor/bytemuck |
| `bytemuck_derive` | `1.12.1` | `Zlib OR Apache-2.0 OR MIT` | crates.io | https://github.com/Lokathor/bytemuck |
| `bytes` | `1.12.1` | `MIT` | crates.io | https://github.com/tokio-rs/bytes |
| `cc` | `1.6.0` | `MIT OR Apache-2.0` | crates.io | https://github.com/rust-lang/cc-rs |
| `cfg_aliases` | `0.1.1` | `MIT` | crates.io | https://github.com/katharostech/cfg_aliases |
| `cfg-if` | `1.0.5` | `MIT OR Apache-2.0` | crates.io | https://github.com/rust-lang/cfg-if |
| `cobs` | `0.3.0` | `MIT OR Apache-2.0` | crates.io | https://github.com/jamesmunns/cobs.rs |
| `codespan-reporting` | `0.11.1` | `Apache-2.0` | crates.io | https://github.com/brendanzab/codespan |
| `com` | `0.6.0` | `MIT` | crates.io | https://github.com/microsoft/com-rs |
| `com_macros` | `0.6.0` | `MIT` | crates.io | https://github.com/microsoft/com-rs |
| `com_macros_support` | `0.6.0` | `MIT` | crates.io | https://github.com/microsoft/com-rs |
| `core-foundation` | `0.9.4` | `MIT OR Apache-2.0` | crates.io | https://github.com/servo/core-foundation-rs |
| `core-foundation-sys` | `0.8.7` | `MIT OR Apache-2.0` | crates.io | https://github.com/servo/core-foundation-rs |
| `core-graphics-types` | `0.1.3` | `MIT OR Apache-2.0` | crates.io | https://github.com/servo/core-foundation-rs |
| `cpufeatures` | `0.2.17` | `MIT OR Apache-2.0` | crates.io | https://github.com/RustCrypto/utils |
| `crypto-common` | `0.1.7` | `MIT OR Apache-2.0` | crates.io | https://github.com/RustCrypto/traits |
| `d3d12` | `0.19.0` | `MIT OR Apache-2.0` | crates.io | https://github.com/gfx-rs/wgpu/tree/trunk/d3d12 |
| `data-encoding` | `2.11.1` | `MIT` | crates.io | https://github.com/ia0/data-encoding |
| `digest` | `0.10.7` | `MIT OR Apache-2.0` | crates.io | https://github.com/RustCrypto/traits |
| `embedded-io` | `0.4.0` | `MIT OR Apache-2.0` | crates.io | https://github.com/embassy-rs/embedded-io |
| `embedded-io` | `0.6.1` | `MIT OR Apache-2.0` | crates.io | https://github.com/rust-embedded/embedded-hal |
| `equivalent` | `1.0.2` | `Apache-2.0 OR MIT` | crates.io | https://github.com/indexmap-rs/equivalent |
| `errno` | `0.3.14` | `MIT OR Apache-2.0` | crates.io | https://github.com/lambda-fairy/rust-errno |
| `find-msvc-tools` | `0.1.14` | `MIT OR Apache-2.0` | crates.io | https://github.com/rust-lang/cc-rs |
| `foreign-types` | `0.5.0` | `MIT/Apache-2.0` | crates.io | https://github.com/sfackler/foreign-types |
| `foreign-types-macros` | `0.2.4` | `MIT/Apache-2.0` | crates.io | https://github.com/sfackler/foreign-types |
| `foreign-types-shared` | `0.3.1` | `MIT/Apache-2.0` | crates.io | https://github.com/sfackler/foreign-types |
| `form_urlencoded` | `1.2.2` | `MIT OR Apache-2.0` | crates.io | https://github.com/servo/rust-url |
| `futures-core` | `0.3.34` | `MIT OR Apache-2.0` | crates.io | https://github.com/rust-lang/futures-rs |
| `futures-macro` | `0.3.31` | `MIT OR Apache-2.0` | crates.io | https://github.com/rust-lang/futures-rs |
| `futures-sink` | `0.3.34` | `MIT OR Apache-2.0` | crates.io | https://github.com/rust-lang/futures-rs |
| `futures-task` | `0.3.34` | `MIT OR Apache-2.0` | crates.io | https://github.com/rust-lang/futures-rs |
| `futures-util` | `0.3.31` | `MIT OR Apache-2.0` | crates.io | https://github.com/rust-lang/futures-rs |
| `generic-array` | `0.14.7` | `MIT` | crates.io | https://github.com/fizyk20/generic-array.git |
| `getrandom` | `0.3.4` | `MIT OR Apache-2.0` | crates.io | https://github.com/rust-random/getrandom |
| `gimli` | `0.32.3` | `MIT OR Apache-2.0` | crates.io | https://github.com/gimli-rs/gimli |
| `gl_generator` | `0.14.0` | `Apache-2.0` | crates.io | https://github.com/brendanzab/gl-rs/ |
| `glow` | `0.13.1` | `MIT OR Apache-2.0 OR Zlib` | crates.io | https://github.com/grovesNL/glow |
| `glutin_wgl_sys` | `0.5.0` | `Apache-2.0` | crates.io | https://github.com/rust-windowing/glutin |
| `gpu-alloc` | `0.6.2` | `MIT OR Apache-2.0` | crates.io | https://github.com/zakarumych/gpu-alloc |
| `gpu-alloc-types` | `0.3.1` | `MIT OR Apache-2.0` | crates.io | https://github.com/zakarumych/gpu-alloc |
| `gpu-allocator` | `0.25.0` | `MIT OR Apache-2.0` | crates.io | https://github.com/Traverse-Research/gpu-allocator |
| `gpu-descriptor` | `0.2.4` | `MIT OR Apache-2.0` | crates.io | https://github.com/zakarumych/gpu-descriptor |
| `gpu-descriptor-types` | `0.1.2` | `MIT OR Apache-2.0` | crates.io | https://github.com/zakarumych/gpu-descriptor |
| `hashbrown` | `0.14.5` | `MIT OR Apache-2.0` | crates.io | https://github.com/rust-lang/hashbrown |
| `hashbrown` | `0.17.1` | `MIT OR Apache-2.0` | crates.io | https://github.com/rust-lang/hashbrown |
| `hassle-rs` | `0.11.0` | `MIT` | crates.io | https://github.com/Traverse-Research/hassle-rs |
| `hexf-parse` | `0.2.1` | `CC0-1.0` | crates.io | https://github.com/lifthrasiir/hexf |
| `http` | `1.5.0` | `MIT OR Apache-2.0` | crates.io | https://github.com/hyperium/http |
| `http-body` | `1.1.0` | `MIT` | crates.io | https://github.com/hyperium/http-body |
| `http-body-util` | `0.1.5` | `MIT` | crates.io | https://github.com/hyperium/http-body |
| `http-range-header` | `0.4.2` | `MIT` | crates.io | https://github.com/MarcusGrass/parse-range-headers |
| `httparse` | `1.10.1` | `MIT OR Apache-2.0` | crates.io | https://github.com/seanmonstar/httparse |
| `httpdate` | `1.0.3` | `MIT OR Apache-2.0` | crates.io | https://github.com/pyfisch/httpdate |
| `hyper` | `1.12.0` | `MIT` | crates.io | https://github.com/hyperium/hyper |
| `hyper-util` | `0.1.21` | `MIT` | crates.io | https://github.com/hyperium/hyper-util |
| `indexmap` | `2.14.2` | `Apache-2.0 OR MIT` | crates.io | https://github.com/indexmap-rs/indexmap |
| `itoa` | `1.0.18` | `MIT OR Apache-2.0` | crates.io | https://github.com/dtolnay/itoa |
| `jni-sys` | `0.3.1` | `MIT OR Apache-2.0` | crates.io | https://github.com/jni-rs/jni-sys |
| `jni-sys` | `0.4.1` | `MIT OR Apache-2.0` | crates.io | https://github.com/jni-rs/jni-sys |
| `jni-sys-macros` | `0.4.1` | `MIT OR Apache-2.0` | crates.io | https://github.com/jni-rs/jni-sys |
| `js-sys` | `0.3.106` | `MIT OR Apache-2.0` | crates.io | https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/js-sys |
| `khronos_api` | `3.1.0` | `Apache-2.0` | crates.io | https://github.com/brendanzab/gl-rs/ |
| `khronos-egl` | `6.0.0` | `MIT/Apache-2.0` | crates.io | https://github.com/timothee-haudebourg/khronos-egl |
| `libc` | `0.2.190` | `MIT OR Apache-2.0` | crates.io | https://github.com/rust-lang/libc |
| `libloading` | `0.7.4` | `ISC` | crates.io | https://github.com/nagisa/rust_libloading/ |
| `libloading` | `0.8.9` | `ISC` | crates.io | https://github.com/nagisa/rust_libloading/ |
| `lock_api` | `0.4.14` | `MIT OR Apache-2.0` | crates.io | https://github.com/Amanieu/parking_lot |
| `log` | `0.4.34` | `MIT OR Apache-2.0` | crates.io | https://github.com/rust-lang/log |
| `malloc_buf` | `0.0.6` | `MIT` | crates.io | https://github.com/SSheldon/malloc_buf |
| `matchit` | `0.8.4` | `MIT AND BSD-3-Clause` | crates.io | https://github.com/ibraheemdev/matchit |
| `memchr` | `2.8.3` | `Unlicense OR MIT` | crates.io | https://github.com/BurntSushi/memchr |
| `metal` | `0.27.0` | `MIT OR Apache-2.0` | crates.io | https://github.com/gfx-rs/metal-rs |
| `mime` | `0.3.17` | `MIT OR Apache-2.0` | crates.io | https://github.com/hyperium/mime |
| `mime_guess` | `2.0.5` | `MIT` | crates.io | https://github.com/abonander/mime_guess |
| `miniz_oxide` | `0.8.9` | `MIT OR Zlib OR Apache-2.0` | crates.io | https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide |
| `mio` | `1.2.4` | `MIT` | crates.io | https://github.com/tokio-rs/mio |
| `naga` | `0.19.2` | `MIT OR Apache-2.0` | crates.io | https://github.com/gfx-rs/wgpu/tree/trunk/naga |
| `ndk-sys` | `0.5.0+25.2.9519653` | `MIT OR Apache-2.0` | crates.io | https://github.com/rust-mobile/ndk |
| `num-traits` | `0.2.19` | `MIT OR Apache-2.0` | crates.io | https://github.com/rust-num/num-traits |
| `objc` | `0.2.7` | `MIT` | crates.io | http://github.com/SSheldon/rust-objc |
| `objc_exception` | `0.1.2` | `MIT` | crates.io | http://github.com/SSheldon/rust-objc-exception |
| `object` | `0.37.3` | `Apache-2.0 OR MIT` | crates.io | https://github.com/gimli-rs/object |
| `once_cell` | `1.21.4` | `MIT OR Apache-2.0` | crates.io | https://github.com/matklad/once_cell |
| `parking_lot` | `0.12.5` | `MIT OR Apache-2.0` | crates.io | https://github.com/Amanieu/parking_lot |
| `parking_lot_core` | `0.9.12` | `MIT OR Apache-2.0` | crates.io | https://github.com/Amanieu/parking_lot |
| `paste` | `1.0.15` | `MIT OR Apache-2.0` | crates.io | https://github.com/dtolnay/paste |
| `percent-encoding` | `2.3.2` | `MIT OR Apache-2.0` | crates.io | https://github.com/servo/rust-url/ |
| `pin-project-lite` | `0.2.17` | `Apache-2.0 OR MIT` | crates.io | https://github.com/taiki-e/pin-project-lite |
| `pin-utils` | `0.1.1` | `MIT OR Apache-2.0` | crates.io | https://github.com/rust-lang/pin-utils |
| `pkg-config` | `0.3.34` | `MIT OR Apache-2.0` | crates.io | https://github.com/rust-lang/pkg-config-rs |
| `postcard` | `1.1.3` | `MIT OR Apache-2.0` | crates.io | https://github.com/jamesmunns/postcard |
| `ppv-lite86` | `0.2.21` | `MIT OR Apache-2.0` | crates.io | https://github.com/cryptocorrosion/cryptocorrosion |
| `presser` | `0.3.1` | `MIT OR Apache-2.0` | crates.io | https://github.com/EmbarkStudios/presser |
| `proc-macro2` | `1.0.107` | `MIT OR Apache-2.0` | crates.io | https://github.com/dtolnay/proc-macro2 |
| `profiling` | `1.0.18` | `MIT OR Apache-2.0` | crates.io | https://github.com/aclysma/profiling |
| `quote` | `1.0.47` | `MIT OR Apache-2.0` | crates.io | https://github.com/dtolnay/quote |
| `r-efi` | `5.3.0` | `MIT OR Apache-2.0 OR LGPL-2.1-or-later` | crates.io | https://github.com/r-efi/r-efi |
| `rand` | `0.9.5` | `MIT OR Apache-2.0` | crates.io | https://github.com/rust-random/rand |
| `rand_chacha` | `0.9.0` | `MIT OR Apache-2.0` | crates.io | https://github.com/rust-random/rand |
| `rand_core` | `0.9.5` | `MIT OR Apache-2.0` | crates.io | https://github.com/rust-random/rand |
| `range-alloc` | `0.1.5` | `MIT OR Apache-2.0` | crates.io | https://github.com/gfx-rs/range-alloc |
| `raw-window-handle` | `0.6.2` | `MIT OR Apache-2.0 OR Zlib` | crates.io | https://github.com/rust-windowing/raw-window-handle |
| `redox_syscall` | `0.5.18` | `MIT` | crates.io | https://gitlab.redox-os.org/redox-os/syscall |
| `renderdoc-sys` | `1.1.0` | `MIT OR Apache-2.0` | crates.io | https://github.com/ebkalderon/renderdoc-rs |
| `rustc-demangle` | `0.1.28` | `MIT/Apache-2.0` | crates.io | https://github.com/rust-lang/rustc-demangle |
| `rustc-hash` | `1.1.0` | `Apache-2.0/MIT` | crates.io | https://github.com/rust-lang-nursery/rustc-hash |
| `rustversion` | `1.0.23` | `MIT OR Apache-2.0` | crates.io | https://github.com/dtolnay/rustversion |
| `ryu` | `1.0.23` | `Apache-2.0 OR BSL-1.0` | crates.io | https://github.com/dtolnay/ryu |
| `scopeguard` | `1.2.0` | `MIT OR Apache-2.0` | crates.io | https://github.com/bluss/scopeguard |
| `serde` | `1.0.219` | `MIT OR Apache-2.0` | crates.io | https://github.com/serde-rs/serde |
| `serde_derive` | `1.0.219` | `MIT OR Apache-2.0` | crates.io | https://github.com/serde-rs/serde |
| `serde_json` | `1.0.140` | `MIT OR Apache-2.0` | crates.io | https://github.com/serde-rs/json |
| `serde_path_to_error` | `0.1.17` | `MIT OR Apache-2.0` | crates.io | https://github.com/dtolnay/path-to-error |
| `serde_urlencoded` | `0.7.1` | `MIT/Apache-2.0` | crates.io | https://github.com/nox/serde_urlencoded |
| `sha1` | `0.10.7` | `MIT OR Apache-2.0` | crates.io | https://github.com/RustCrypto/hashes |
| `sha2` | `0.10.9` | `MIT OR Apache-2.0` | crates.io | https://github.com/RustCrypto/hashes |
| `shlex` | `2.0.1` | `MIT OR Apache-2.0` | crates.io | https://github.com/comex/rust-shlex |
| `signal-hook-registry` | `1.4.8` | `MIT OR Apache-2.0` | crates.io | https://github.com/vorner/signal-hook |
| `slab` | `0.4.12` | `MIT` | crates.io | https://github.com/tokio-rs/slab |
| `slotmap` | `1.1.1` | `Zlib` | crates.io | https://github.com/orlp/slotmap |
| `smallvec` | `1.16.2` | `MIT OR Apache-2.0` | crates.io | https://github.com/servo/rust-smallvec |
| `socket2` | `0.5.10` | `MIT OR Apache-2.0` | crates.io | https://github.com/rust-lang/socket2 |
| `spirv` | `0.3.0+sdk-1.3.268.0` | `Apache-2.0` | crates.io | https://github.com/gfx-rs/rspirv |
| `static_assertions` | `1.1.0` | `MIT OR Apache-2.0` | crates.io | https://github.com/nvzqz/static-assertions-rs |
| `syn` | `1.0.109` | `MIT OR Apache-2.0` | crates.io | https://github.com/dtolnay/syn |
| `syn` | `2.0.119` | `MIT OR Apache-2.0` | crates.io | https://github.com/dtolnay/syn |
| `syn` | `3.0.6` | `MIT OR Apache-2.0` | crates.io | https://github.com/dtolnay/syn |
| `sync_wrapper` | `1.0.2` | `Apache-2.0` | crates.io | https://github.com/Actyx/sync_wrapper |
| `termcolor` | `1.4.1` | `Unlicense OR MIT` | crates.io | https://github.com/BurntSushi/termcolor |
| `thiserror` | `1.0.69` | `MIT OR Apache-2.0` | crates.io | https://github.com/dtolnay/thiserror |
| `thiserror` | `2.0.21` | `MIT OR Apache-2.0` | crates.io | https://github.com/dtolnay/thiserror |
| `thiserror-impl` | `1.0.69` | `MIT OR Apache-2.0` | crates.io | https://github.com/dtolnay/thiserror |
| `thiserror-impl` | `2.0.21` | `MIT OR Apache-2.0` | crates.io | https://github.com/dtolnay/thiserror |
| `tokio` | `1.45.1` | `MIT` | crates.io | https://github.com/tokio-rs/tokio |
| `tokio-macros` | `2.5.0` | `MIT` | crates.io | https://github.com/tokio-rs/tokio |
| `tokio-tungstenite` | `0.26.2` | `MIT` | crates.io | https://github.com/snapview/tokio-tungstenite |
| `tokio-util` | `0.7.18` | `MIT` | crates.io | https://github.com/tokio-rs/tokio |
| `tower` | `0.5.3` | `MIT` | crates.io | https://github.com/tower-rs/tower |
| `tower-http` | `0.6.6` | `MIT` | crates.io | https://github.com/tower-rs/tower-http |
| `tower-layer` | `0.3.3` | `MIT` | crates.io | https://github.com/tower-rs/tower |
| `tower-service` | `0.3.3` | `MIT` | crates.io | https://github.com/tower-rs/tower |
| `tracing` | `0.1.44` | `MIT` | crates.io | https://github.com/tokio-rs/tracing |
| `tracing-core` | `0.1.36` | `MIT` | crates.io | https://github.com/tokio-rs/tracing |
| `tungstenite` | `0.26.2` | `MIT OR Apache-2.0` | crates.io | https://github.com/snapview/tungstenite-rs |
| `typenum` | `1.20.1` | `MIT OR Apache-2.0` | crates.io | https://github.com/paholg/typenum |
| `unicase` | `2.10.0` | `MIT OR Apache-2.0` | crates.io | https://github.com/seanmonstar/unicase |
| `unicode-ident` | `1.0.26` | `(MIT OR Apache-2.0) AND Unicode-3.0` | crates.io | https://github.com/dtolnay/unicode-ident |
| `unicode-width` | `0.1.14` | `MIT OR Apache-2.0` | crates.io | https://github.com/unicode-rs/unicode-width |
| `unicode-xid` | `0.2.6` | `MIT OR Apache-2.0` | crates.io | https://github.com/unicode-rs/unicode-xid |
| `utf-8` | `0.7.6` | `MIT OR Apache-2.0` | crates.io | https://github.com/SimonSapin/rust-utf8 |
| `version_check` | `0.9.5` | `MIT/Apache-2.0` | crates.io | https://github.com/SergioBenitez/version_check |
| `wasi` | `0.11.1+wasi-snapshot-preview1` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | crates.io | https://github.com/bytecodealliance/wasi |
| `wasip2` | `1.0.4+wasi-0.2.12` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | crates.io | https://github.com/bytecodealliance/wasi-rs |
| `wasm-bindgen` | `0.2.129` | `MIT OR Apache-2.0` | crates.io | https://github.com/wasm-bindgen/wasm-bindgen |
| `wasm-bindgen-futures` | `0.4.79` | `MIT OR Apache-2.0` | crates.io | https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/futures |
| `wasm-bindgen-macro` | `0.2.129` | `MIT OR Apache-2.0` | crates.io | https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/macro |
| `wasm-bindgen-macro-support` | `0.2.129` | `MIT OR Apache-2.0` | crates.io | https://github.com/wasm-bindgen/wasm-bindgen/tree/main/crates/macro-support |
| `wasm-bindgen-shared` | `0.2.129` | `MIT OR Apache-2.0` | crates.io | https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/shared |
| `web-sys` | `0.3.106` | `MIT OR Apache-2.0` | crates.io | https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/web-sys |
| `wgpu` | `0.19.4` | `MIT OR Apache-2.0` | crates.io | https://github.com/gfx-rs/wgpu |
| `wgpu-core` | `0.19.4` | `MIT OR Apache-2.0` | crates.io | https://github.com/gfx-rs/wgpu |
| `wgpu-hal` | `0.19.5` | `MIT OR Apache-2.0` | crates.io | https://github.com/gfx-rs/wgpu |
| `wgpu-types` | `0.19.2` | `MIT OR Apache-2.0` | crates.io | https://github.com/gfx-rs/wgpu |
| `widestring` | `1.2.1` | `MIT OR Apache-2.0` | crates.io | https://github.com/VoidStarKat/widestring-rs |
| `winapi` | `0.3.9` | `MIT/Apache-2.0` | crates.io | https://github.com/retep998/winapi-rs |
| `winapi-i686-pc-windows-gnu` | `0.4.0` | `MIT/Apache-2.0` | crates.io | https://github.com/retep998/winapi-rs |
| `winapi-util` | `0.1.11` | `Unlicense OR MIT` | crates.io | https://github.com/BurntSushi/winapi-util |
| `winapi-x86_64-pc-windows-gnu` | `0.4.0` | `MIT/Apache-2.0` | crates.io | https://github.com/retep998/winapi-rs |
| `windows` | `0.52.0` | `MIT OR Apache-2.0` | crates.io | https://github.com/microsoft/windows-rs |
| `windows_aarch64_gnullvm` | `0.52.6` | `MIT OR Apache-2.0` | crates.io | https://github.com/microsoft/windows-rs |
| `windows_aarch64_msvc` | `0.52.6` | `MIT OR Apache-2.0` | crates.io | https://github.com/microsoft/windows-rs |
| `windows_i686_gnu` | `0.52.6` | `MIT OR Apache-2.0` | crates.io | https://github.com/microsoft/windows-rs |
| `windows_i686_gnullvm` | `0.52.6` | `MIT OR Apache-2.0` | crates.io | https://github.com/microsoft/windows-rs |
| `windows_i686_msvc` | `0.52.6` | `MIT OR Apache-2.0` | crates.io | https://github.com/microsoft/windows-rs |
| `windows_x86_64_gnu` | `0.52.6` | `MIT OR Apache-2.0` | crates.io | https://github.com/microsoft/windows-rs |
| `windows_x86_64_gnullvm` | `0.52.6` | `MIT OR Apache-2.0` | crates.io | https://github.com/microsoft/windows-rs |
| `windows_x86_64_msvc` | `0.52.6` | `MIT OR Apache-2.0` | crates.io | https://github.com/microsoft/windows-rs |
| `windows-core` | `0.52.0` | `MIT OR Apache-2.0` | crates.io | https://github.com/microsoft/windows-rs |
| `windows-link` | `0.2.1` | `MIT OR Apache-2.0` | crates.io | https://github.com/microsoft/windows-rs |
| `windows-sys` | `0.52.0` | `MIT OR Apache-2.0` | crates.io | https://github.com/microsoft/windows-rs |
| `windows-sys` | `0.61.2` | `MIT OR Apache-2.0` | crates.io | https://github.com/microsoft/windows-rs |
| `windows-targets` | `0.52.6` | `MIT OR Apache-2.0` | crates.io | https://github.com/microsoft/windows-rs |
| `wit-bindgen` | `0.57.1` | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | crates.io | https://github.com/bytecodealliance/wit-bindgen |
| `xml-rs` | `0.8.29` | `MIT` | crates.io | https://github.com/kornelski/xml-rs |
| `zerocopy` | `0.8.60` | `BSD-2-Clause OR Apache-2.0 OR MIT` | crates.io | https://github.com/google/zerocopy |
| `zerocopy-derive` | `0.8.60` | `BSD-2-Clause OR Apache-2.0 OR MIT` | crates.io | https://github.com/google/zerocopy |

## Browser test packages

| Package | Version | SPDX license | Source |
|---|---:|---|---|
| `@playwright/test` | `1.63.0` | `Apache-2.0` | npmjs.org |
| `playwright` | `1.63.0` | `Apache-2.0` | npmjs.org |
| `playwright-core` | `1.63.0` | `Apache-2.0` | npmjs.org |

## Audit notes

- `cargo deny check` passes advisories, bans, licenses, and sources. It reports duplicate cross-platform versions (including `bitflags`, `hashbrown`, `libloading`, `syn`, `thiserror`, and `windows-sys`) as warnings from target-specific and transitive dependencies; several appear in the selected wgpu 0.19 backend graph.
- `cargo audit` reports no vulnerabilities. It retains one unmaintained warning, `paste 1.0.15` (RUSTSEC-2024-0436), through wgpu 0.19.4 → wgpu-hal 0.19.5 → Metal 0.27.0. This is target-specific to macOS. The exact advisory is narrowly listed in `deny.toml`; reconsider when changing the wgpu baseline or adding a macOS target/build.
- The alloc-only postcard configuration removes `atomic-polyfill`/heapless from the resolved graph because the protocol uses alloc-backed postcard serialization and does not use heapless buffers.
