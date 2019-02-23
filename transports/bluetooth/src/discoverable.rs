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
    /*let connection = ;

    let msg = dbus::Message::new_method_call(destination: D, path: P, "org.freedesktop.DBus.Properties", "Set")?
        .append3(proxy->interface, "Discoverable", true);

	if (g_dbus_send_message_with_reply(client->dbus_conn, msg,
							&call, -1) == FALSE) {
		dbus_message_unref(msg);
		g_free(data);
		return FALSE;
	}

	dbus_pending_call_set_notify(call, set_property_reply, data, g_free);
	dbus_pending_call_unref(call);

	dbus_message_unref(msg);

    unsafe {
        let ctl = libc::socket(libc::AF_BLUETOOTH, libc::SOCK_RAW, ffi::BTPROTO_HCI);
        if ctl == 0 {
            return Err(io::Error::last_os_error());
        }

        let dev_id = ffi::hci_get_route(ptr::null_mut());
        if dev_id == -1 {
            return Err(io::Error::last_os_error());
        }

        let dr = ffi::hci_dev_req {
            dev_id: dev_id as u16,
            dev_opt: ffi::SCAN_INQUIRY | ffi::SCAN_PAGE,
        };

        if libc::ioctl(ctl, ffi::HCISETSCAN, &dr as *const _) < 0 {
            return Err(io::Error::last_os_error());
        }

        // TODO: :-/
        libc::close(ctl);

        Ok(())
    }*/
    Ok(())      // TODO:
}
