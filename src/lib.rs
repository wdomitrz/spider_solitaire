//! Spider Solitaire: a small static progressive web app.
//!
//! The split this crate exists to keep:
//!
//! * [`card`] and [`game`] — the deck, the rules, the score and every word the
//!   page shows. No `web-sys`, no `js-sys`, no `wasm-bindgen`, so plain
//!   `cargo test` runs them natively.
//! * `ui` — the browser layer, compiled only for `wasm32-unknown-unknown`.
//!   Every `document`, `localStorage` and service-worker call lives there, and
//!   nothing else does.
//!
//! `cargo build --release` writes the whole site to `dist/` through `build.rs`.
//! There is no server, no binary and no run step: the build is the product.

#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

pub mod board;
#[cfg(target_arch = "wasm32")]
pub mod board_ui;
pub mod card;
pub mod game;
pub mod scene;
#[cfg(target_arch = "wasm32")]
pub mod shell;

#[cfg(target_arch = "wasm32")]
pub mod ui;

pub use card::{Card, Rank, Suit};
pub use game::{Difficulty, Game, Hint, Options, Pile, Report, Scoring, Selection, HISTORY_LIMIT};
