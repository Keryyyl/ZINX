pub mod attachment_cache;
pub mod attachments;
pub mod folders;
pub mod imap_real;
pub mod manager;
pub mod outbox;
pub mod parse;
pub mod remote;
pub mod send;
pub mod session;
pub mod sync;

#[cfg(test)]
pub mod mock;

#[cfg(test)]
mod sync_tests;
