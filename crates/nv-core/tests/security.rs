//! Security PoC tests.
//!
//! A test that demonstrates an open finding asserts the SECURE behaviour and is
//! `#[ignore = "..."]` until the finding is fixed (`cargo test -- --ignored` shows them
//! failing). Tests that confirm a property holds run normally.

#[path = "security/common.rs"]
mod common;
#[path = "security/tamper.rs"]
mod tamper;
#[path = "security/rollback.rs"]
mod rollback;
#[path = "security/slots.rs"]
mod slots;
#[path = "security/rotation.rs"]
mod rotation;
#[path = "security/fuzz.rs"]
mod fuzz;
