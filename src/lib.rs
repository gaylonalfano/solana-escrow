// NOTE MUST register our mods/crates here or won't compile!
pub mod error;
pub mod instruction;
pub mod processor;
pub mod state;


// Allow other programs to import this program via Cargo Features
// NOTE All programs have their own entrypoint. If we want to use
// other programs inside our current program, we need to turn off
// its entrypoint via Cargo Features.
#[cfg(not(feature = "no-entrypoint"))]
pub mod entrypoint;
