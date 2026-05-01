// lib.rs — Library crate root
//
// WHY have both lib.rs and main.rs?
// In Rust, a project can have both a library crate (lib.rs) and a binary crate (main.rs).
// The library crate is what tests import. The binary is the runnable app.
// This is the standard pattern for any Rust app that needs to be tested —
// same as exporting functions from a module so your test files can import them.
//
// Tests in `tests/` import this as `use git_mirror::git;` etc.

pub mod app;
pub mod config;
pub mod git;
pub mod sync;
