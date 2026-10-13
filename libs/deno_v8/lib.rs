// Copyright 2018-2026 the Deno authors. MIT license.

#[cfg(not(feature = "v8"))]
compile_error!("feature `v8` must be enabled");

#[cfg(feature = "v8")]
pub use rusty_v8::*;
