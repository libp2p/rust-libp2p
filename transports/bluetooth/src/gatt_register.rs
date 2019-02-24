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

use crate::{Addr, ffi};
use std::{io, iter, mem};

/// Register a GATT service.
pub fn register_gatt() -> Result<Registration, io::Error> {
    let interface_path = b"/io/libp2p".to_vec();

    let connection = dbus::Connection::get_private(dbus::BusType::System).unwrap(); // TODO: don't unwrap
    let msg = dbus::Message::new_method_call("org.bluez", "/org/bluez/hci0", "org.bluez.GattManager1", "RegisterApplication")
        .map_err(|s| io::Error::new(io::ErrorKind::Other, s))?
        .append2(dbus::Path::from_slice(&interface_path[..]).unwrap(), dbus::arg::Dict::<String, dbus::arg::Variant<String>, _>::new(iter::empty()));

    connection.send(msg).unwrap();      // TODO: don't unwrap

    Ok(Registration {
        connection,
        interface_path,
    })
}

pub struct Registration {
    connection: dbus::Connection,
    interface_path: Vec<u8>,
}

impl Drop for Registration {
    fn drop(&mut self) {
        // TODO: are we supposed to watch for AddMatch/RemoveMatch messages and only send to the
        //       appropriate recipients?
        // Send an `InterfacesRemoved` signal to unregister the GATT.
        let msg = dbus::Message::new_signal(
            mem::replace(&mut self.interface_path, Vec::new()),
            "org.freedesktop.DBus.ObjectManager",
            "InterfacesRemoved"
        ).expect("message is always valid; QED");
        let _ = self.connection.send(msg).unwrap();
    }
}

/*
method return time=1551029658.159717 sender=:1.1149 -> destination=:1.9 serial=8 reply_serial=1145
   array [
      dict entry(
         object path "/org/bluez/example/service0"
         array [
            dict entry(
               string "org.bluez.GattService1"
               array [
                  dict entry(
                     string "UUID"
                     variant                         string "0000180d-0000-1000-8000-00805f9b34fb"
                  )
                  dict entry(
                     string "Primary"
                     variant                         boolean true
                  )
                  dict entry(
                     string "Characteristics"
                     variant                         array [
                           object path "/org/bluez/example/service0/char0"
                           object path "/org/bluez/example/service0/char1"
                           object path "/org/bluez/example/service0/char2"
                        ]
                  )
               ]
            )
         ]
      )
      dict entry(
         object path "/org/bluez/example/service0/char0"
         array [
            dict entry(
               string "org.bluez.GattCharacteristic1"
               array [
                  dict entry(
                     string "Service"
                     variant                         object path "/org/bluez/example/service0"
                  )
                  dict entry(
                     string "UUID"
                     variant                         string "00002a37-0000-1000-8000-00805f9b34fb"
                  )
                  dict entry(
                     string "Flags"
                     variant                         array [
                           string "notify"
                        ]
                  )
                  dict entry(
                     string "Descriptors"
                     variant                         array [
                        ]
                  )
               ]
            )
         ]
      )
      dict entry(
         object path "/org/bluez/example/service0/char1"
         array [
            dict entry(
               string "org.bluez.GattCharacteristic1"
               array [
                  dict entry(
                     string "Service"
                     variant                         object path "/org/bluez/example/service0"
                  )
                  dict entry(
                     string "UUID"
                     variant                         string "00002a38-0000-1000-8000-00805f9b34fb"
                  )
                  dict entry(
                     string "Flags"
                     variant                         array [
                           string "read"
                        ]
                  )
                  dict entry(
                     string "Descriptors"
                     variant                         array [
                        ]
                  )
               ]
            )
         ]
      )
      */

#[cfg(test)]
mod tests {
    use super::register_gatt;
    use std::{thread, time::Duration};

    #[test]
    fn register_gatt_working() {
        let _reg = register_gatt().unwrap();
        thread::sleep(Duration::from_millis(5000));     // TODO: smaller
    }
}
