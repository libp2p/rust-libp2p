// Copyright 2019 Parity Technologies (UK) Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use crate::Addr;
// TODO: put that in sys, or move the ffi, or something, I don't know
use crate::sys::platform::ffi;
use std::{io, ptr};

/// Makes the given local interface discoverable from the outside.
///
/// Without doing that, you will not receive any incoming connection.
pub fn enable_discoverable(addr: &Addr) -> Result<(), io::Error> {
    // TODO: quite bad, as we should revert to non-discoverable

    let connection = dbus::Connection::get_private(dbus::BusType::System).unwrap(); // TODO: don't unwrap
    let msg = dbus::Message::new_method_call("org.bluez", "/org/bluez/hci0", "org.freedesktop.DBus.Properties", "Set")
        .map_err(|s| io::Error::new(io::ErrorKind::Other, s))?
        .append3("org.bluez.Adapter1", "Discoverable", dbus::arg::Variant(true));

    // TODO: don't block
    let reply = connection.send_with_reply_and_block(msg, 5000).unwrap();
    Ok(())
}

/*
let c = Connection::get_private(BusType::Session).unwrap();
    c.add_match("interface='com.canonical.Unity.WindowStack',member='FocusedWindowChanged'").unwrap();

    for i in c.iter(1000) {
        if let Some(app) = focus_msg(&i) { println!("{} has now focus.", app) };
}
*/
