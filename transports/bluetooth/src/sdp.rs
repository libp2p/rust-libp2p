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

use super::ffi;
use std::{ffi::CStr, io, mem, os::raw::c_void, ptr};

/// Configuration for the registration.
pub struct RegisterConfig<'a> {
    /// UUID of the service to register.
    pub uuid: [u32; 4],
    /// RFCOMM channel that will serve the service.
    pub rfcomm_channel: u8,
    /// Name of the service to register.
    pub service_name: &'a CStr,
    /// Description of the service to register.
    pub service_desc: &'a CStr,
    /// Provider of the service to register.
    pub service_prov: &'a CStr,
}

/// Registers an RFCOMM service.
pub fn register(config: RegisterConfig) -> Result<SdpRegistration, io::Error> {
    unsafe {
        let record = ffi::sdp_record_alloc();

        // Set the UUID.
        let mut svc_uuid: ffi::uuid_t = mem::zeroed();
        ffi::sdp_uuid128_create(&mut svc_uuid, &config.uuid as *const _ as *const c_void);
        ffi::sdp_set_service_id(record, svc_uuid);

        // Make the service public.
        let mut root_uuid: ffi::uuid_t = mem::zeroed();
        ffi::sdp_uuid16_create(&mut root_uuid, ffi::PUBLIC_BROWSE_GROUP);
        let root_list = ffi::sdp_list_append(ptr::null_mut(), &mut root_uuid as *mut _ as *mut c_void);
        ffi::sdp_set_uuidseq_attr(record, ffi::SDP_ATTR_BROWSE_GRP_LIST, root_list);

        // Register the L2CAP information.
        let mut l2cap_uuid: ffi::uuid_t = mem::zeroed();
        ffi::sdp_uuid16_create(&mut l2cap_uuid, ffi::L2CAP_UUID);
        let l2cap_list = ffi::sdp_list_append(ptr::null_mut(), &mut l2cap_uuid as *mut _ as *mut c_void);
        let proto_list = ffi::sdp_list_append(ptr::null_mut(), l2cap_list as *mut c_void);

        // Register the RFCOMM information.
        let mut rfcomm_uuid: ffi::uuid_t = mem::zeroed();
        ffi::sdp_uuid16_create(&mut rfcomm_uuid, ffi::RFCOMM_UUID);
        let channel = ffi::sdp_data_alloc(ffi::SDP_UINT8, &config.rfcomm_channel as *const _ as *const c_void);
        let rfcomm_list = ffi::sdp_list_append(ptr::null_mut(), &mut rfcomm_uuid as *mut _ as *mut c_void);
        ffi::sdp_list_append(rfcomm_list, channel as *mut c_void);
        ffi::sdp_list_append(proto_list, rfcomm_list as *mut c_void);

        // Attach protocol information to service record.
        let access_proto_list = ffi::sdp_list_append(ptr::null_mut(), proto_list as *mut c_void);
        ffi::sdp_set_access_protos(record, access_proto_list);

        ffi::sdp_set_info_attr(record, config.service_name.as_ptr(), config.service_prov.as_ptr(), config.service_desc.as_ptr());

        let session = ffi::sdp_connect(&ffi::BDADDR_ANY, &ffi::BDADDR_LOCAL, ffi::SDP_RETRY_IF_BUSY);
        if session.is_null() {
            // TODO: must clean up
            println!("err session");
            return Err(io::Error::last_os_error());
        }

        let result = if ffi::sdp_record_register(session, record, 0) == 0 {
            Ok(SdpRegistration { session })
        } else {
            Err(io::Error::last_os_error())
        };

        ffi::sdp_data_free(channel);
        ffi::sdp_list_free(l2cap_list, None);
        ffi::sdp_list_free(rfcomm_list, None);
        ffi::sdp_list_free(root_list, None);
        ffi::sdp_list_free(access_proto_list, None);

        result
    }
}

/// Active SDP registration. Dropping this object will unregister.
pub struct SdpRegistration {
    session: *mut ffi::sdp_session_t,
}

unsafe impl Send for SdpRegistration {}
unsafe impl Sync for SdpRegistration {}

impl SdpRegistration {
    /// Closes the registration. Allows checking for errors.
    pub fn close(mut self) -> Result<(), io::Error> {
        unsafe {
            if ffi::sdp_close(self.session) == 0 {
                self.session = ptr::null_mut();
                Ok(())
            } else {
                Err(io::Error::last_os_error())
            }
        }
    }
}

impl Drop for SdpRegistration {
    fn drop(&mut self) {
        unsafe {
            if self.session != ptr::null_mut() {
                ffi::sdp_close(self.session);
            }
        }
    }
}
