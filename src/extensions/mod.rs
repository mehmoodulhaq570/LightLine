//! Consumes extensions from the Zed extension ecosystem
//! (<https://github.com/zed-industries/extensions>) through adapters into
//! LightLine's own internal types, instead of LightLine maintaining its own
//! marketplace/catalog. See `docs/extension_implementation_plan.md`.

pub mod installer;
pub mod zed_manifest;
pub mod zed_registry;
