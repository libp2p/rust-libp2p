# libp2p-dns

This crate provides the type `DnsConfig` that allows one to resolve the `/dns4/` and `/dns6/`
components of multiaddresses.

## Usage

In order to use this crate, create a `DnsConfig` with one of its constructors and pass it an
implementation of the `Transport` trait.

Whenever we want to dial an address through the `DnsConfig` and that address contains a
`/dns4/` or `/dns6/` component, a DNS resolve will be performed and the component will be
replaced with respectively an `/ip4/` or an `/ip6/` component.
