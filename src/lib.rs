#[cfg(test)]
#[macro_use]
extern crate assert_matches;
extern crate config;
#[macro_use(select)]
extern crate crossbeam_channel;
#[cfg(test)]
extern crate goldenfile;
extern crate httpbis;
#[macro_use]
extern crate log;
extern crate rmp_serde as rmps;
extern crate rustyline;
extern crate serde;

mod client;
mod error;
mod handlers;
mod proto;
mod raft;
mod serializer;
mod sql;
mod store;

pub use client::Client;
pub use error::Error;
pub use handlers::Node;
