# libp2p over Tor - Ping-Pong

Basic TCP peer to peer connectivity using libp2p over the Tor network.

## Usage

0. Install Tor
1. Configure an onion service using the Tor run file. You can use
`./torrc` and run `tor` using `sudo /usr/bin/tor --defaults-torrc tor-service-defaults-torrc -f torrc --RunAsDaemon 0`
2. Once you have run `tor` for the first time get the onion address
   from the `hostname` file (if you used the `tor` invocation above
   this will be in `/var/lib/tor/hidden_service/hostname`). Set the
   onion address `const ONION` in `main.rs`.
3. Set the log level in `main.rs` if you wish.

Now to run this demo application run tor in one terminal, the listener
in another terminal, and the dialer in a third terminal.

- Run the listener with: `ping-pong --listener`.
- Run the dialer with: `ping-pong --dialer`.

See `ping-pong --help` for more information.


Tested on Ubuntu LTS 18 using Tor version 0.4.3.5