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
use std::{io, iter};

/// Register a GATT service.
pub fn register_gatt() -> Result<(), io::Error> {
    let connection = dbus::Connection::get_private(dbus::BusType::System).unwrap(); // TODO: don't unwrap
    let msg = dbus::Message::new_method_call("org.bluez", "/org/bluez/hci0", "org.bluez.GattManager1", "RegisterApplication")
        .map_err(|s| io::Error::new(io::ErrorKind::Other, s))?
        .append2(dbus::Path::from_slice(b"/io/libp2p").unwrap(), dbus::arg::Dict::<String, dbus::arg::Variant<String>, _>::new(iter::empty()));

    connection.send(msg).unwrap();      // TODO: don't unwrap
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::register_gatt;
    use std::{thread, time::Duration};

    #[test]
    fn register_gatt_working() {
        let _reg = register_gatt().unwrap();
        thread::sleep(Duration::from_millis(500000));
    }
}
