//! Coeur du runtime natif de RUSTFORGE-X.
//!
//! Composants : C-27 (Rust Runtime Core, module [`runtime`]) et C-45 (Hardware
//! Probe, module [`hw`]). Machine a etats : SM-06 (module [`state`]).
//! Codes d'erreur : annexe A.2 (module [`error`]).
//!
//! Ce crate ne connait ni Java, ni la JVM, ni Forge : la frontiere FFI est
//! entierement contenue dans `rfx-ffi`, qui depend de ce crate. Le sens des
//! dependances est donc `rfx-ffi` vers `rfx-core` vers `rfx-model`, jamais l'inverse
//! (INV-13).

pub mod error;
pub mod hw;
pub mod runtime;
pub mod state;

pub use error::{ErrorCode, Severity, OK};
pub use runtime::{Runtime, ABI_VERSION};
pub use state::RuntimeState;
