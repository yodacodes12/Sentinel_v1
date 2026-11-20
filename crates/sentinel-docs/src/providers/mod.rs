//! Built-in providers.
//!
//! | Provider | Online | Source category |
//! |---|---|---|
//! | `local_library` | no | documents the user put in Sentinel's library folder |
//! | `catalogue` | yes | JSON catalogues at HTTPS addresses the user configures (for example a manufacturer's or a community's documentation index) |
//! | `internet_archive` | yes | the Internet Archive's public search API; results are kept as references only |

pub mod archive;
pub mod catalogue;
pub mod local;
