#![doc = include_str!("../README.md")]

pub mod browser;
mod connection;
mod error;
mod sdp;
mod stream;
mod transport;
mod upgrade;


pub use self::{
    connection::Connection,
    error::Error,
    stream::Stream,
    transport::{Config, Transport},
    browser::{Transport as BrowserTransport, Config as BrowserConfig}
};
