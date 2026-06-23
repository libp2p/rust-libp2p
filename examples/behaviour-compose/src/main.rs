#![no_main]
//! # Manually composing `NetworkBehaviour`s
//!
//! In most cases, `NetworkBehaviour` derive macro should be able to
//! do the heavy lifting of composing behaviours. But manually composing
//! behaviours can bring about some degree of freedom.  
//!
//! This example not only shows how to compose `NetworkBehaviour`s,
//! but also some core concepts of `rust-libp2p`.
//!
//!  To fully understand how to compose behaviours, the core concepts cannot be forgotten:
//! - [`Swarm`] only holds **one `NetworkBehaviour` instance**: All composed behaviour instances
//! should live under one composed instance, typically a struct.
//! - [`NetworkBehaviour`] only requires **one `ConnectionHandler` type**
//! and only assign one instance of it per connection: All `ConnectionHandler`s should also reside
//! in the same instance, also a struct.
//! The "illusion" of multiple behaviour acting together comes from properly **delegating** calls
//! and **aggregating** events.  
//! This tutorial is broken down into two parts: composing `NetworkBehaviour` and composing
//! `ConnectionHandler`. The former is easier to understand and implement, while the latter is more
//! complex because it involves how peers negotiate protocols. But they need to work together.
//!
//! Proceeding to their dedicated modules for more information.

pub mod behaviour;
pub mod connection_handler;
