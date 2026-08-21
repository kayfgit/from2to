//! Fast `<from>2<to>` conversion for the terminal.
//!
//! Today every conversion is a number base or a byte encoding, but nothing in
//! the plumbing assumes that: [`format::Format`] is just "a way of writing
//! bytes or numbers", so other transforms (percent-encoding, rot13) can join
//! the same command-name scheme without disturbing it.
//!
//! The library half is pure: no I/O, no argument parsing. `main.rs` is the
//! shell around it.

pub mod convert;
pub mod detect;
pub mod endian;
pub mod format;
pub mod install;
pub mod names;
pub mod opts;

pub use convert::{ConvError, Converter, Options};
pub use detect::Candidate;
pub use endian::{Endian, Endpoint};
pub use format::Format;
