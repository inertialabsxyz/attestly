//! HTTP route handlers. One submodule per resource family. The locked HTTP
//! contract for the entire surface lives in `services/awp-cloud/API.md`.

pub mod account;
pub mod attestations;
pub mod billing;
pub mod export;
pub mod health;
pub mod share_links;
