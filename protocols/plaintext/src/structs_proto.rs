// This file is generated by rust-protobuf 2.8.1. Do not edit
// @generated

// https://github.com/Manishearth/rust-clippy/issues/702
#![allow(unknown_lints)]
#![allow(clippy::all)]

#![cfg_attr(rustfmt, rustfmt_skip)]

#![allow(box_pointers)]
#![allow(dead_code)]
#![allow(missing_docs)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(non_upper_case_globals)]
#![allow(trivial_casts)]
#![allow(unsafe_code)]
#![allow(unused_imports)]
#![allow(unused_results)]
//! Generated file from `src/structs.proto`

use protobuf::Message as Message_imported_for_functions;
use protobuf::ProtobufEnum as ProtobufEnum_imported_for_functions;

/// Generated files are compatible only with the same version
/// of protobuf runtime.
const _PROTOBUF_VERSION_CHECK: () = ::protobuf::VERSION_2_8_1;

#[derive(PartialEq,Clone,Default)]
pub struct Exchange {
    // message fields
    id: ::protobuf::SingularField<::std::vec::Vec<u8>>,
    pubkey: ::protobuf::SingularField<::std::vec::Vec<u8>>,
    // special fields
    pub unknown_fields: ::protobuf::UnknownFields,
    pub cached_size: ::protobuf::CachedSize,
}

impl<'a> ::std::default::Default for &'a Exchange {
    fn default() -> &'a Exchange {
        <Exchange as ::protobuf::Message>::default_instance()
    }
}

impl Exchange {
    pub fn new() -> Exchange {
        ::std::default::Default::default()
    }

    // optional bytes id = 1;


    pub fn get_id(&self) -> &[u8] {
        match self.id.as_ref() {
            Some(v) => &v,
            None => &[],
        }
    }
    pub fn clear_id(&mut self) {
        self.id.clear();
    }

    pub fn has_id(&self) -> bool {
        self.id.is_some()
    }

    // Param is passed by value, moved
    pub fn set_id(&mut self, v: ::std::vec::Vec<u8>) {
        self.id = ::protobuf::SingularField::some(v);
    }

    // Mutable pointer to the field.
    // If field is not initialized, it is initialized with default value first.
    pub fn mut_id(&mut self) -> &mut ::std::vec::Vec<u8> {
        if self.id.is_none() {
            self.id.set_default();
        }
        self.id.as_mut().unwrap()
    }

    // Take field
    pub fn take_id(&mut self) -> ::std::vec::Vec<u8> {
        self.id.take().unwrap_or_else(|| ::std::vec::Vec::new())
    }

    // optional bytes pubkey = 2;


    pub fn get_pubkey(&self) -> &[u8] {
        match self.pubkey.as_ref() {
            Some(v) => &v,
            None => &[],
        }
    }
    pub fn clear_pubkey(&mut self) {
        self.pubkey.clear();
    }

    pub fn has_pubkey(&self) -> bool {
        self.pubkey.is_some()
    }

    // Param is passed by value, moved
    pub fn set_pubkey(&mut self, v: ::std::vec::Vec<u8>) {
        self.pubkey = ::protobuf::SingularField::some(v);
    }

    // Mutable pointer to the field.
    // If field is not initialized, it is initialized with default value first.
    pub fn mut_pubkey(&mut self) -> &mut ::std::vec::Vec<u8> {
        if self.pubkey.is_none() {
            self.pubkey.set_default();
        }
        self.pubkey.as_mut().unwrap()
    }

    // Take field
    pub fn take_pubkey(&mut self) -> ::std::vec::Vec<u8> {
        self.pubkey.take().unwrap_or_else(|| ::std::vec::Vec::new())
    }
}

impl ::protobuf::Message for Exchange {
    fn is_initialized(&self) -> bool {
        true
    }

    fn merge_from(&mut self, is: &mut ::protobuf::CodedInputStream<'_>) -> ::protobuf::ProtobufResult<()> {
        while !is.eof()? {
            let (field_number, wire_type) = is.read_tag_unpack()?;
            match field_number {
                1 => {
                    ::protobuf::rt::read_singular_bytes_into(wire_type, is, &mut self.id)?;
                },
                2 => {
                    ::protobuf::rt::read_singular_bytes_into(wire_type, is, &mut self.pubkey)?;
                },
                _ => {
                    ::protobuf::rt::read_unknown_or_skip_group(field_number, wire_type, is, self.mut_unknown_fields())?;
                },
            };
        }
        ::std::result::Result::Ok(())
    }

    // Compute sizes of nested messages
    #[allow(unused_variables)]
    fn compute_size(&self) -> u32 {
        let mut my_size = 0;
        if let Some(ref v) = self.id.as_ref() {
            my_size += ::protobuf::rt::bytes_size(1, &v);
        }
        if let Some(ref v) = self.pubkey.as_ref() {
            my_size += ::protobuf::rt::bytes_size(2, &v);
        }
        my_size += ::protobuf::rt::unknown_fields_size(self.get_unknown_fields());
        self.cached_size.set(my_size);
        my_size
    }

    fn write_to_with_cached_sizes(&self, os: &mut ::protobuf::CodedOutputStream<'_>) -> ::protobuf::ProtobufResult<()> {
        if let Some(ref v) = self.id.as_ref() {
            os.write_bytes(1, &v)?;
        }
        if let Some(ref v) = self.pubkey.as_ref() {
            os.write_bytes(2, &v)?;
        }
        os.write_unknown_fields(self.get_unknown_fields())?;
        ::std::result::Result::Ok(())
    }

    fn get_cached_size(&self) -> u32 {
        self.cached_size.get()
    }

    fn get_unknown_fields(&self) -> &::protobuf::UnknownFields {
        &self.unknown_fields
    }

    fn mut_unknown_fields(&mut self) -> &mut ::protobuf::UnknownFields {
        &mut self.unknown_fields
    }

    fn as_any(&self) -> &dyn (::std::any::Any) {
        self as &dyn (::std::any::Any)
    }
    fn as_any_mut(&mut self) -> &mut dyn (::std::any::Any) {
        self as &mut dyn (::std::any::Any)
    }
    fn into_any(self: Box<Self>) -> ::std::boxed::Box<dyn (::std::any::Any)> {
        self
    }

    fn descriptor(&self) -> &'static ::protobuf::reflect::MessageDescriptor {
        Self::descriptor_static()
    }

    fn new() -> Exchange {
        Exchange::new()
    }

    fn descriptor_static() -> &'static ::protobuf::reflect::MessageDescriptor {
        static mut descriptor: ::protobuf::lazy::Lazy<::protobuf::reflect::MessageDescriptor> = ::protobuf::lazy::Lazy {
            lock: ::protobuf::lazy::ONCE_INIT,
            ptr: 0 as *const ::protobuf::reflect::MessageDescriptor,
        };
        unsafe {
            descriptor.get(|| {
                let mut fields = ::std::vec::Vec::new();
                fields.push(::protobuf::reflect::accessor::make_singular_field_accessor::<_, ::protobuf::types::ProtobufTypeBytes>(
                    "id",
                    |m: &Exchange| { &m.id },
                    |m: &mut Exchange| { &mut m.id },
                ));
                fields.push(::protobuf::reflect::accessor::make_singular_field_accessor::<_, ::protobuf::types::ProtobufTypeBytes>(
                    "pubkey",
                    |m: &Exchange| { &m.pubkey },
                    |m: &mut Exchange| { &mut m.pubkey },
                ));
                ::protobuf::reflect::MessageDescriptor::new::<Exchange>(
                    "Exchange",
                    fields,
                    file_descriptor_proto()
                )
            })
        }
    }

    fn default_instance() -> &'static Exchange {
        static mut instance: ::protobuf::lazy::Lazy<Exchange> = ::protobuf::lazy::Lazy {
            lock: ::protobuf::lazy::ONCE_INIT,
            ptr: 0 as *const Exchange,
        };
        unsafe {
            instance.get(Exchange::new)
        }
    }
}

impl ::protobuf::Clear for Exchange {
    fn clear(&mut self) {
        self.id.clear();
        self.pubkey.clear();
        self.unknown_fields.clear();
    }
}

impl ::std::fmt::Debug for Exchange {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        ::protobuf::text_format::fmt(self, f)
    }
}

impl ::protobuf::reflect::ProtobufValue for Exchange {
    fn as_ref(&self) -> ::protobuf::reflect::ProtobufValueRef {
        ::protobuf::reflect::ProtobufValueRef::Message(self)
    }
}

static file_descriptor_proto_data: &'static [u8] = b"\
    \n\x11src/structs.proto\"2\n\x08Exchange\x12\x0e\n\x02id\x18\x01\x20\x01\
    (\x0cR\x02id\x12\x16\n\x06pubkey\x18\x02\x20\x01(\x0cR\x06pubkeyJ\xb4\
    \x01\n\x06\x12\x04\0\0\x05\x01\n\x08\n\x01\x0c\x12\x03\0\0\x12\n\n\n\x02\
    \x04\0\x12\x04\x02\0\x05\x01\n\n\n\x03\x04\0\x01\x12\x03\x02\x08\x10\n\
    \x0b\n\x04\x04\0\x02\0\x12\x03\x03\x02\x18\n\x0c\n\x05\x04\0\x02\0\x04\
    \x12\x03\x03\x02\n\n\x0c\n\x05\x04\0\x02\0\x05\x12\x03\x03\x0b\x10\n\x0c\
    \n\x05\x04\0\x02\0\x01\x12\x03\x03\x11\x13\n\x0c\n\x05\x04\0\x02\0\x03\
    \x12\x03\x03\x16\x17\n\x0b\n\x04\x04\0\x02\x01\x12\x03\x04\x02\x1c\n\x0c\
    \n\x05\x04\0\x02\x01\x04\x12\x03\x04\x02\n\n\x0c\n\x05\x04\0\x02\x01\x05\
    \x12\x03\x04\x0b\x10\n\x0c\n\x05\x04\0\x02\x01\x01\x12\x03\x04\x11\x17\n\
    \x0c\n\x05\x04\0\x02\x01\x03\x12\x03\x04\x1a\x1b\
";

static mut file_descriptor_proto_lazy: ::protobuf::lazy::Lazy<::protobuf::descriptor::FileDescriptorProto> = ::protobuf::lazy::Lazy {
    lock: ::protobuf::lazy::ONCE_INIT,
    ptr: 0 as *const ::protobuf::descriptor::FileDescriptorProto,
};

fn parse_descriptor_proto() -> ::protobuf::descriptor::FileDescriptorProto {
    ::protobuf::parse_from_bytes(file_descriptor_proto_data).unwrap()
}

pub fn file_descriptor_proto() -> &'static ::protobuf::descriptor::FileDescriptorProto {
    unsafe {
        file_descriptor_proto_lazy.get(|| {
            parse_descriptor_proto()
        })
    }
}
