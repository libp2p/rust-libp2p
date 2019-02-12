// This file is generated by rust-protobuf 2.3.0. Do not edit
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

use protobuf::Message as Message_imported_for_functions;
use protobuf::ProtobufEnum as ProtobufEnum_imported_for_functions;

#[derive(PartialEq,Clone,Default)]
pub struct Identify {
    // message fields
    protocolVersion: ::protobuf::SingularField<::std::string::String>,
    agentVersion: ::protobuf::SingularField<::std::string::String>,
    publicKey: ::protobuf::SingularField<::std::vec::Vec<u8>>,
    listenAddrs: ::protobuf::RepeatedField<::std::vec::Vec<u8>>,
    observedAddr: ::protobuf::SingularField<::std::vec::Vec<u8>>,
    protocols: ::protobuf::RepeatedField<::std::string::String>,
    // special fields
    pub unknown_fields: ::protobuf::UnknownFields,
    pub cached_size: ::protobuf::CachedSize,
}

impl Identify {
    pub fn new() -> Identify {
        ::std::default::Default::default()
    }

    // optional string protocolVersion = 5;

    pub fn clear_protocolVersion(&mut self) {
        self.protocolVersion.clear();
    }

    pub fn has_protocolVersion(&self) -> bool {
        self.protocolVersion.is_some()
    }

    // Param is passed by value, moved
    pub fn set_protocolVersion(&mut self, v: ::std::string::String) {
        self.protocolVersion = ::protobuf::SingularField::some(v);
    }

    // Mutable pointer to the field.
    // If field is not initialized, it is initialized with default value first.
    pub fn mut_protocolVersion(&mut self) -> &mut ::std::string::String {
        if self.protocolVersion.is_none() {
            self.protocolVersion.set_default();
        }
        self.protocolVersion.as_mut().unwrap()
    }

    // Take field
    pub fn take_protocolVersion(&mut self) -> ::std::string::String {
        self.protocolVersion.take().unwrap_or_else(|| ::std::string::String::new())
    }

    pub fn get_protocolVersion(&self) -> &str {
        match self.protocolVersion.as_ref() {
            Some(v) => &v,
            None => "",
        }
    }

    // optional string agentVersion = 6;

    pub fn clear_agentVersion(&mut self) {
        self.agentVersion.clear();
    }

    pub fn has_agentVersion(&self) -> bool {
        self.agentVersion.is_some()
    }

    // Param is passed by value, moved
    pub fn set_agentVersion(&mut self, v: ::std::string::String) {
        self.agentVersion = ::protobuf::SingularField::some(v);
    }

    // Mutable pointer to the field.
    // If field is not initialized, it is initialized with default value first.
    pub fn mut_agentVersion(&mut self) -> &mut ::std::string::String {
        if self.agentVersion.is_none() {
            self.agentVersion.set_default();
        }
        self.agentVersion.as_mut().unwrap()
    }

    // Take field
    pub fn take_agentVersion(&mut self) -> ::std::string::String {
        self.agentVersion.take().unwrap_or_else(|| ::std::string::String::new())
    }

    pub fn get_agentVersion(&self) -> &str {
        match self.agentVersion.as_ref() {
            Some(v) => &v,
            None => "",
        }
    }

    // optional bytes publicKey = 1;

    pub fn clear_publicKey(&mut self) {
        self.publicKey.clear();
    }

    pub fn has_publicKey(&self) -> bool {
        self.publicKey.is_some()
    }

    // Param is passed by value, moved
    pub fn set_publicKey(&mut self, v: ::std::vec::Vec<u8>) {
        self.publicKey = ::protobuf::SingularField::some(v);
    }

    // Mutable pointer to the field.
    // If field is not initialized, it is initialized with default value first.
    pub fn mut_publicKey(&mut self) -> &mut ::std::vec::Vec<u8> {
        if self.publicKey.is_none() {
            self.publicKey.set_default();
        }
        self.publicKey.as_mut().unwrap()
    }

    // Take field
    pub fn take_publicKey(&mut self) -> ::std::vec::Vec<u8> {
        self.publicKey.take().unwrap_or_else(|| ::std::vec::Vec::new())
    }

    pub fn get_publicKey(&self) -> &[u8] {
        match self.publicKey.as_ref() {
            Some(v) => &v,
            None => &[],
        }
    }

    // repeated bytes listenAddrs = 2;

    pub fn clear_listenAddrs(&mut self) {
        self.listenAddrs.clear();
    }

    // Param is passed by value, moved
    pub fn set_listenAddrs(&mut self, v: ::protobuf::RepeatedField<::std::vec::Vec<u8>>) {
        self.listenAddrs = v;
    }

    // Mutable pointer to the field.
    pub fn mut_listenAddrs(&mut self) -> &mut ::protobuf::RepeatedField<::std::vec::Vec<u8>> {
        &mut self.listenAddrs
    }

    // Take field
    pub fn take_listenAddrs(&mut self) -> ::protobuf::RepeatedField<::std::vec::Vec<u8>> {
        ::std::mem::replace(&mut self.listenAddrs, ::protobuf::RepeatedField::new())
    }

    pub fn get_listenAddrs(&self) -> &[::std::vec::Vec<u8>] {
        &self.listenAddrs
    }

    // optional bytes observedAddr = 4;

    pub fn clear_observedAddr(&mut self) {
        self.observedAddr.clear();
    }

    pub fn has_observedAddr(&self) -> bool {
        self.observedAddr.is_some()
    }

    // Param is passed by value, moved
    pub fn set_observedAddr(&mut self, v: ::std::vec::Vec<u8>) {
        self.observedAddr = ::protobuf::SingularField::some(v);
    }

    // Mutable pointer to the field.
    // If field is not initialized, it is initialized with default value first.
    pub fn mut_observedAddr(&mut self) -> &mut ::std::vec::Vec<u8> {
        if self.observedAddr.is_none() {
            self.observedAddr.set_default();
        }
        self.observedAddr.as_mut().unwrap()
    }

    // Take field
    pub fn take_observedAddr(&mut self) -> ::std::vec::Vec<u8> {
        self.observedAddr.take().unwrap_or_else(|| ::std::vec::Vec::new())
    }

    pub fn get_observedAddr(&self) -> &[u8] {
        match self.observedAddr.as_ref() {
            Some(v) => &v,
            None => &[],
        }
    }

    // repeated string protocols = 3;

    pub fn clear_protocols(&mut self) {
        self.protocols.clear();
    }

    // Param is passed by value, moved
    pub fn set_protocols(&mut self, v: ::protobuf::RepeatedField<::std::string::String>) {
        self.protocols = v;
    }

    // Mutable pointer to the field.
    pub fn mut_protocols(&mut self) -> &mut ::protobuf::RepeatedField<::std::string::String> {
        &mut self.protocols
    }

    // Take field
    pub fn take_protocols(&mut self) -> ::protobuf::RepeatedField<::std::string::String> {
        ::std::mem::replace(&mut self.protocols, ::protobuf::RepeatedField::new())
    }

    pub fn get_protocols(&self) -> &[::std::string::String] {
        &self.protocols
    }
}

impl ::protobuf::Message for Identify {
    fn is_initialized(&self) -> bool {
        true
    }

    fn merge_from(&mut self, is: &mut ::protobuf::CodedInputStream<'_>) -> ::protobuf::ProtobufResult<()> {
        while !is.eof()? {
            let (field_number, wire_type) = is.read_tag_unpack()?;
            match field_number {
                5 => {
                    ::protobuf::rt::read_singular_string_into(wire_type, is, &mut self.protocolVersion)?;
                },
                6 => {
                    ::protobuf::rt::read_singular_string_into(wire_type, is, &mut self.agentVersion)?;
                },
                1 => {
                    ::protobuf::rt::read_singular_bytes_into(wire_type, is, &mut self.publicKey)?;
                },
                2 => {
                    ::protobuf::rt::read_repeated_bytes_into(wire_type, is, &mut self.listenAddrs)?;
                },
                4 => {
                    ::protobuf::rt::read_singular_bytes_into(wire_type, is, &mut self.observedAddr)?;
                },
                3 => {
                    ::protobuf::rt::read_repeated_string_into(wire_type, is, &mut self.protocols)?;
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
        if let Some(ref v) = self.protocolVersion.as_ref() {
            my_size += ::protobuf::rt::string_size(5, &v);
        }
        if let Some(ref v) = self.agentVersion.as_ref() {
            my_size += ::protobuf::rt::string_size(6, &v);
        }
        if let Some(ref v) = self.publicKey.as_ref() {
            my_size += ::protobuf::rt::bytes_size(1, &v);
        }
        for value in &self.listenAddrs {
            my_size += ::protobuf::rt::bytes_size(2, &value);
        };
        if let Some(ref v) = self.observedAddr.as_ref() {
            my_size += ::protobuf::rt::bytes_size(4, &v);
        }
        for value in &self.protocols {
            my_size += ::protobuf::rt::string_size(3, &value);
        };
        my_size += ::protobuf::rt::unknown_fields_size(self.get_unknown_fields());
        self.cached_size.set(my_size);
        my_size
    }

    fn write_to_with_cached_sizes(&self, os: &mut ::protobuf::CodedOutputStream<'_>) -> ::protobuf::ProtobufResult<()> {
        if let Some(ref v) = self.protocolVersion.as_ref() {
            os.write_string(5, &v)?;
        }
        if let Some(ref v) = self.agentVersion.as_ref() {
            os.write_string(6, &v)?;
        }
        if let Some(ref v) = self.publicKey.as_ref() {
            os.write_bytes(1, &v)?;
        }
        for v in &self.listenAddrs {
            os.write_bytes(2, &v)?;
        };
        if let Some(ref v) = self.observedAddr.as_ref() {
            os.write_bytes(4, &v)?;
        }
        for v in &self.protocols {
            os.write_string(3, &v)?;
        };
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

    fn new() -> Identify {
        Identify::new()
    }

    fn descriptor_static() -> &'static ::protobuf::reflect::MessageDescriptor {
        static mut descriptor: ::protobuf::lazy::Lazy<::protobuf::reflect::MessageDescriptor> = ::protobuf::lazy::Lazy {
            lock: ::protobuf::lazy::ONCE_INIT,
            ptr: 0 as *const ::protobuf::reflect::MessageDescriptor,
        };
        unsafe {
            descriptor.get(|| {
                let mut fields = ::std::vec::Vec::new();
                fields.push(::protobuf::reflect::accessor::make_singular_field_accessor::<_, ::protobuf::types::ProtobufTypeString>(
                    "protocolVersion",
                    |m: &Identify| { &m.protocolVersion },
                    |m: &mut Identify| { &mut m.protocolVersion },
                ));
                fields.push(::protobuf::reflect::accessor::make_singular_field_accessor::<_, ::protobuf::types::ProtobufTypeString>(
                    "agentVersion",
                    |m: &Identify| { &m.agentVersion },
                    |m: &mut Identify| { &mut m.agentVersion },
                ));
                fields.push(::protobuf::reflect::accessor::make_singular_field_accessor::<_, ::protobuf::types::ProtobufTypeBytes>(
                    "publicKey",
                    |m: &Identify| { &m.publicKey },
                    |m: &mut Identify| { &mut m.publicKey },
                ));
                fields.push(::protobuf::reflect::accessor::make_repeated_field_accessor::<_, ::protobuf::types::ProtobufTypeBytes>(
                    "listenAddrs",
                    |m: &Identify| { &m.listenAddrs },
                    |m: &mut Identify| { &mut m.listenAddrs },
                ));
                fields.push(::protobuf::reflect::accessor::make_singular_field_accessor::<_, ::protobuf::types::ProtobufTypeBytes>(
                    "observedAddr",
                    |m: &Identify| { &m.observedAddr },
                    |m: &mut Identify| { &mut m.observedAddr },
                ));
                fields.push(::protobuf::reflect::accessor::make_repeated_field_accessor::<_, ::protobuf::types::ProtobufTypeString>(
                    "protocols",
                    |m: &Identify| { &m.protocols },
                    |m: &mut Identify| { &mut m.protocols },
                ));
                ::protobuf::reflect::MessageDescriptor::new::<Identify>(
                    "Identify",
                    fields,
                    file_descriptor_proto()
                )
            })
        }
    }

    fn default_instance() -> &'static Identify {
        static mut instance: ::protobuf::lazy::Lazy<Identify> = ::protobuf::lazy::Lazy {
            lock: ::protobuf::lazy::ONCE_INIT,
            ptr: 0 as *const Identify,
        };
        unsafe {
            instance.get(Identify::new)
        }
    }
}

impl ::protobuf::Clear for Identify {
    fn clear(&mut self) {
        self.clear_protocolVersion();
        self.clear_agentVersion();
        self.clear_publicKey();
        self.clear_listenAddrs();
        self.clear_observedAddr();
        self.clear_protocols();
        self.unknown_fields.clear();
    }
}

impl ::std::fmt::Debug for Identify {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        ::protobuf::text_format::fmt(self, f)
    }
}

impl ::protobuf::reflect::ProtobufValue for Identify {
    fn as_ref(&self) -> ::protobuf::reflect::ProtobufValueRef<'_> {
        ::protobuf::reflect::ProtobufValueRef::Message(self)
    }
}

static file_descriptor_proto_data: &'static [u8] = b"\
    \n\rstructs.proto\"\xda\x01\n\x08Identify\x12(\n\x0fprotocolVersion\x18\
    \x05\x20\x01(\tR\x0fprotocolVersion\x12\"\n\x0cagentVersion\x18\x06\x20\
    \x01(\tR\x0cagentVersion\x12\x1c\n\tpublicKey\x18\x01\x20\x01(\x0cR\tpub\
    licKey\x12\x20\n\x0blistenAddrs\x18\x02\x20\x03(\x0cR\x0blistenAddrs\x12\
    \"\n\x0cobservedAddr\x18\x04\x20\x01(\x0cR\x0cobservedAddr\x12\x1c\n\tpr\
    otocols\x18\x03\x20\x03(\tR\tprotocolsJ\xc2\t\n\x06\x12\x04\0\0\x16\x01\
    \n\n\n\x02\x04\0\x12\x04\0\0\x16\x01\n\n\n\x03\x04\0\x01\x12\x03\0\x08\
    \x10\nX\n\x04\x04\0\x02\0\x12\x03\x02\x02&\x1a8\x20protocolVersion\x20de\
    termines\x20compatibility\x20between\x20peers\n\"\x11\x20e.g.\x20ipfs/1.\
    0.0\n\n\x0c\n\x05\x04\0\x02\0\x04\x12\x03\x02\x02\n\n\x0c\n\x05\x04\0\
    \x02\0\x05\x12\x03\x02\x0b\x11\n\x0c\n\x05\x04\0\x02\0\x01\x12\x03\x02\
    \x12!\n\x0c\n\x05\x04\0\x02\0\x03\x12\x03\x02$%\n\x9f\x01\n\x04\x04\0\
    \x02\x01\x12\x03\x06\x02#\x1a|\x20agentVersion\x20is\x20like\x20a\x20Use\
    rAgent\x20string\x20in\x20browsers,\x20or\x20client\x20version\x20in\x20\
    bittorrent\n\x20includes\x20the\x20client\x20name\x20and\x20client.\n\"\
    \x14\x20e.g.\x20go-ipfs/0.1.0\n\n\x0c\n\x05\x04\0\x02\x01\x04\x12\x03\
    \x06\x02\n\n\x0c\n\x05\x04\0\x02\x01\x05\x12\x03\x06\x0b\x11\n\x0c\n\x05\
    \x04\0\x02\x01\x01\x12\x03\x06\x12\x1e\n\x0c\n\x05\x04\0\x02\x01\x03\x12\
    \x03\x06!\"\n\xe3\x01\n\x04\x04\0\x02\x02\x12\x03\x0b\x02\x1f\x1a\xd5\
    \x01\x20publicKey\x20is\x20this\x20node's\x20public\x20key\x20(which\x20\
    also\x20gives\x20its\x20node.ID)\n\x20-\x20may\x20not\x20need\x20to\x20b\
    e\x20sent,\x20as\x20secure\x20channel\x20implies\x20it\x20has\x20been\
    \x20sent.\n\x20-\x20then\x20again,\x20if\x20we\x20change\x20/\x20disable\
    \x20secure\x20channel,\x20may\x20still\x20want\x20it.\n\n\x0c\n\x05\x04\
    \0\x02\x02\x04\x12\x03\x0b\x02\n\n\x0c\n\x05\x04\0\x02\x02\x05\x12\x03\
    \x0b\x0b\x10\n\x0c\n\x05\x04\0\x02\x02\x01\x12\x03\x0b\x11\x1a\n\x0c\n\
    \x05\x04\0\x02\x02\x03\x12\x03\x0b\x1d\x1e\n]\n\x04\x04\0\x02\x03\x12\
    \x03\x0e\x02!\x1aP\x20listenAddrs\x20are\x20the\x20multiaddrs\x20the\x20\
    sender\x20node\x20listens\x20for\x20open\x20connections\x20on\n\n\x0c\n\
    \x05\x04\0\x02\x03\x04\x12\x03\x0e\x02\n\n\x0c\n\x05\x04\0\x02\x03\x05\
    \x12\x03\x0e\x0b\x10\n\x0c\n\x05\x04\0\x02\x03\x01\x12\x03\x0e\x11\x1c\n\
    \x0c\n\x05\x04\0\x02\x03\x03\x12\x03\x0e\x1f\x20\n\x81\x02\n\x04\x04\0\
    \x02\x04\x12\x03\x13\x02\"\x1a\xf3\x01\x20oservedAddr\x20is\x20the\x20mu\
    ltiaddr\x20of\x20the\x20remote\x20endpoint\x20that\x20the\x20sender\x20n\
    ode\x20perceives\n\x20this\x20is\x20useful\x20information\x20to\x20conve\
    y\x20to\x20the\x20other\x20side,\x20as\x20it\x20helps\x20the\x20remote\
    \x20endpoint\n\x20determine\x20whether\x20its\x20connection\x20to\x20the\
    \x20local\x20peer\x20goes\x20through\x20NAT.\n\n\x0c\n\x05\x04\0\x02\x04\
    \x04\x12\x03\x13\x02\n\n\x0c\n\x05\x04\0\x02\x04\x05\x12\x03\x13\x0b\x10\
    \n\x0c\n\x05\x04\0\x02\x04\x01\x12\x03\x13\x11\x1d\n\x0c\n\x05\x04\0\x02\
    \x04\x03\x12\x03\x13\x20!\n\x0b\n\x04\x04\0\x02\x05\x12\x03\x15\x02\x20\
    \n\x0c\n\x05\x04\0\x02\x05\x04\x12\x03\x15\x02\n\n\x0c\n\x05\x04\0\x02\
    \x05\x05\x12\x03\x15\x0b\x11\n\x0c\n\x05\x04\0\x02\x05\x01\x12\x03\x15\
    \x12\x1b\n\x0c\n\x05\x04\0\x02\x05\x03\x12\x03\x15\x1e\x1f\
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
