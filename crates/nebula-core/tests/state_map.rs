//! Runs `src/state_map.rs`'s own tests until `lib.rs` declares the module
//! (K10's pending hook). Delete this file once `pub mod state_map;` lands.
#![allow(dead_code)]

#[path = "../src/state_map.rs"]
mod state_map;
