//! The Tauri commands, one module per feature; `lib.rs` registers them in `run()`.

pub(crate) mod engines;
pub(crate) mod files;
pub(crate) mod pipelines;
pub(crate) mod plugins;
pub(crate) mod screen;
pub(crate) mod shortcut;
pub(crate) mod tools;
