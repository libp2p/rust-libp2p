// This file is generated. Do not edit
// @generated

// https://github.com/Manishearth/rust-clippy/issues/702
#![allow(unknown_lints)]
#![allow(clippy)]

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
pub struct Message {
    // message fields
    field_type: ::std::option::Option<Message_MessageType>,
    clusterLevelRaw: ::std::option::Option<i32>,
    key: ::protobuf::SingularField<::std::vec::Vec<u8>>,
    record: ::protobuf::SingularPtrField<super::record::Record>,
    closerPeers: ::protobuf::RepeatedField<Message_Peer>,
    providerPeers: ::protobuf::RepeatedField<Message_Peer>,
    // special fields
    unknown_fields: ::protobuf::UnknownFields,
    cached_size: ::protobuf::CachedSize,
}

// see codegen.rs for the explanation why impl Sync explicitly
unsafe impl ::std::marker::Sync for Message {}

impl Message {
    pub fn new() -> Message {
        ::std::default::Default::default()
    }

    pub fn default_instance() -> &'static Message {
        static mut instance: ::protobuf::lazy::Lazy<Message> = ::protobuf::lazy::Lazy {
            lock: ::protobuf::lazy::ONCE_INIT,
            ptr: 0 as *const Message,
        };
        unsafe {
            instance.get(Message::new)
        }
    }

    // optional .dht.pb.Message.MessageType type = 1;

    pub fn clear_field_type(&mut self) {
        self.field_type = ::std::option::Option::None;
    }

    pub fn has_field_type(&self) -> bool {
        self.field_type.is_some()
    }

    // Param is passed by value, moved
    pub fn set_field_type(&mut self, v: Message_MessageType) {
        self.field_type = ::std::option::Option::Some(v);
    }

    pub fn get_field_type(&self) -> Message_MessageType {
        self.field_type.unwrap_or(Message_MessageType::PUT_VALUE)
    }

    fn get_field_type_for_reflect(&self) -> &::std::option::Option<Message_MessageType> {
        &self.field_type
    }

    fn mut_field_type_for_reflect(&mut self) -> &mut ::std::option::Option<Message_MessageType> {
        &mut self.field_type
    }

    // optional int32 clusterLevelRaw = 10;

    pub fn clear_clusterLevelRaw(&mut self) {
        self.clusterLevelRaw = ::std::option::Option::None;
    }

    pub fn has_clusterLevelRaw(&self) -> bool {
        self.clusterLevelRaw.is_some()
    }

    // Param is passed by value, moved
    pub fn set_clusterLevelRaw(&mut self, v: i32) {
        self.clusterLevelRaw = ::std::option::Option::Some(v);
    }

    pub fn get_clusterLevelRaw(&self) -> i32 {
        self.clusterLevelRaw.unwrap_or(0)
    }

    fn get_clusterLevelRaw_for_reflect(&self) -> &::std::option::Option<i32> {
        &self.clusterLevelRaw
    }

    fn mut_clusterLevelRaw_for_reflect(&mut self) -> &mut ::std::option::Option<i32> {
        &mut self.clusterLevelRaw
    }

    // optional bytes key = 2;

    pub fn clear_key(&mut self) {
        self.key.clear();
    }

    pub fn has_key(&self) -> bool {
        self.key.is_some()
    }

    // Param is passed by value, moved
    pub fn set_key(&mut self, v: ::std::vec::Vec<u8>) {
        self.key = ::protobuf::SingularField::some(v);
    }

    // Mutable pointer to the field.
    // If field is not initialized, it is initialized with default value first.
    pub fn mut_key(&mut self) -> &mut ::std::vec::Vec<u8> {
        if self.key.is_none() {
            self.key.set_default();
        }
        self.key.as_mut().unwrap()
    }

    // Take field
    pub fn take_key(&mut self) -> ::std::vec::Vec<u8> {
        self.key.take().unwrap_or_else(|| ::std::vec::Vec::new())
    }

    pub fn get_key(&self) -> &[u8] {
        match self.key.as_ref() {
            Some(v) => &v,
            None => &[],
        }
    }

    fn get_key_for_reflect(&self) -> &::protobuf::SingularField<::std::vec::Vec<u8>> {
        &self.key
    }

    fn mut_key_for_reflect(&mut self) -> &mut ::protobuf::SingularField<::std::vec::Vec<u8>> {
        &mut self.key
    }

    // optional .record.pb.Record record = 3;

    pub fn clear_record(&mut self) {
        self.record.clear();
    }

    pub fn has_record(&self) -> bool {
        self.record.is_some()
    }

    // Param is passed by value, moved
    pub fn set_record(&mut self, v: super::record::Record) {
        self.record = ::protobuf::SingularPtrField::some(v);
    }

    // Mutable pointer to the field.
    // If field is not initialized, it is initialized with default value first.
    pub fn mut_record(&mut self) -> &mut super::record::Record {
        if self.record.is_none() {
            self.record.set_default();
        }
        self.record.as_mut().unwrap()
    }

    // Take field
    pub fn take_record(&mut self) -> super::record::Record {
        self.record.take().unwrap_or_else(|| super::record::Record::new())
    }

    pub fn get_record(&self) -> &super::record::Record {
        self.record.as_ref().unwrap_or_else(|| super::record::Record::default_instance())
    }

    fn get_record_for_reflect(&self) -> &::protobuf::SingularPtrField<super::record::Record> {
        &self.record
    }

    fn mut_record_for_reflect(&mut self) -> &mut ::protobuf::SingularPtrField<super::record::Record> {
        &mut self.record
    }

    // repeated .dht.pb.Message.Peer closerPeers = 8;

    pub fn clear_closerPeers(&mut self) {
        self.closerPeers.clear();
    }

    // Param is passed by value, moved
    pub fn set_closerPeers(&mut self, v: ::protobuf::RepeatedField<Message_Peer>) {
        self.closerPeers = v;
    }

    // Mutable pointer to the field.
    pub fn mut_closerPeers(&mut self) -> &mut ::protobuf::RepeatedField<Message_Peer> {
        &mut self.closerPeers
    }

    // Take field
    pub fn take_closerPeers(&mut self) -> ::protobuf::RepeatedField<Message_Peer> {
        ::std::mem::replace(&mut self.closerPeers, ::protobuf::RepeatedField::new())
    }

    pub fn get_closerPeers(&self) -> &[Message_Peer] {
        &self.closerPeers
    }

    fn get_closerPeers_for_reflect(&self) -> &::protobuf::RepeatedField<Message_Peer> {
        &self.closerPeers
    }

    fn mut_closerPeers_for_reflect(&mut self) -> &mut ::protobuf::RepeatedField<Message_Peer> {
        &mut self.closerPeers
    }

    // repeated .dht.pb.Message.Peer providerPeers = 9;

    pub fn clear_providerPeers(&mut self) {
        self.providerPeers.clear();
    }

    // Param is passed by value, moved
    pub fn set_providerPeers(&mut self, v: ::protobuf::RepeatedField<Message_Peer>) {
        self.providerPeers = v;
    }

    // Mutable pointer to the field.
    pub fn mut_providerPeers(&mut self) -> &mut ::protobuf::RepeatedField<Message_Peer> {
        &mut self.providerPeers
    }

    // Take field
    pub fn take_providerPeers(&mut self) -> ::protobuf::RepeatedField<Message_Peer> {
        ::std::mem::replace(&mut self.providerPeers, ::protobuf::RepeatedField::new())
    }

    pub fn get_providerPeers(&self) -> &[Message_Peer] {
        &self.providerPeers
    }

    fn get_providerPeers_for_reflect(&self) -> &::protobuf::RepeatedField<Message_Peer> {
        &self.providerPeers
    }

    fn mut_providerPeers_for_reflect(&mut self) -> &mut ::protobuf::RepeatedField<Message_Peer> {
        &mut self.providerPeers
    }
}

impl ::protobuf::Message for Message {
    fn is_initialized(&self) -> bool {
        for v in &self.record {
            if !v.is_initialized() {
                return false;
            }
        };
        for v in &self.closerPeers {
            if !v.is_initialized() {
                return false;
            }
        };
        for v in &self.providerPeers {
            if !v.is_initialized() {
                return false;
            }
        };
        true
    }

    fn merge_from(&mut self, is: &mut ::protobuf::CodedInputStream) -> ::protobuf::ProtobufResult<()> {
        while !is.eof()? {
            let (field_number, wire_type) = is.read_tag_unpack()?;
            match field_number {
                1 => {
                    ::protobuf::rt::read_proto2_enum_with_unknown_fields_into(wire_type, is, &mut self.field_type, 1, &mut self.unknown_fields)?
                },
                10 => {
                    if wire_type != ::protobuf::wire_format::WireTypeVarint {
                        return ::std::result::Result::Err(::protobuf::rt::unexpected_wire_type(wire_type));
                    }
                    let tmp = is.read_int32()?;
                    self.clusterLevelRaw = ::std::option::Option::Some(tmp);
                },
                2 => {
                    ::protobuf::rt::read_singular_bytes_into(wire_type, is, &mut self.key)?;
                },
                3 => {
                    ::protobuf::rt::read_singular_message_into(wire_type, is, &mut self.record)?;
                },
                8 => {
                    ::protobuf::rt::read_repeated_message_into(wire_type, is, &mut self.closerPeers)?;
                },
                9 => {
                    ::protobuf::rt::read_repeated_message_into(wire_type, is, &mut self.providerPeers)?;
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
        if let Some(v) = self.field_type {
            my_size += ::protobuf::rt::enum_size(1, v);
        }
        if let Some(v) = self.clusterLevelRaw {
            my_size += ::protobuf::rt::value_size(10, v, ::protobuf::wire_format::WireTypeVarint);
        }
        if let Some(ref v) = self.key.as_ref() {
            my_size += ::protobuf::rt::bytes_size(2, &v);
        }
        if let Some(ref v) = self.record.as_ref() {
            let len = v.compute_size();
            my_size += 1 + ::protobuf::rt::compute_raw_varint32_size(len) + len;
        }
        for value in &self.closerPeers {
            let len = value.compute_size();
            my_size += 1 + ::protobuf::rt::compute_raw_varint32_size(len) + len;
        };
        for value in &self.providerPeers {
            let len = value.compute_size();
            my_size += 1 + ::protobuf::rt::compute_raw_varint32_size(len) + len;
        };
        my_size += ::protobuf::rt::unknown_fields_size(self.get_unknown_fields());
        self.cached_size.set(my_size);
        my_size
    }

    fn write_to_with_cached_sizes(&self, os: &mut ::protobuf::CodedOutputStream) -> ::protobuf::ProtobufResult<()> {
        if let Some(v) = self.field_type {
            os.write_enum(1, v.value())?;
        }
        if let Some(v) = self.clusterLevelRaw {
            os.write_int32(10, v)?;
        }
        if let Some(ref v) = self.key.as_ref() {
            os.write_bytes(2, &v)?;
        }
        if let Some(ref v) = self.record.as_ref() {
            os.write_tag(3, ::protobuf::wire_format::WireTypeLengthDelimited)?;
            os.write_raw_varint32(v.get_cached_size())?;
            v.write_to_with_cached_sizes(os)?;
        }
        for v in &self.closerPeers {
            os.write_tag(8, ::protobuf::wire_format::WireTypeLengthDelimited)?;
            os.write_raw_varint32(v.get_cached_size())?;
            v.write_to_with_cached_sizes(os)?;
        };
        for v in &self.providerPeers {
            os.write_tag(9, ::protobuf::wire_format::WireTypeLengthDelimited)?;
            os.write_raw_varint32(v.get_cached_size())?;
            v.write_to_with_cached_sizes(os)?;
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

    fn as_any(&self) -> &::std::any::Any {
        self as &::std::any::Any
    }
    fn as_any_mut(&mut self) -> &mut ::std::any::Any {
        self as &mut ::std::any::Any
    }
    fn into_any(self: Box<Self>) -> ::std::boxed::Box<::std::any::Any> {
        self
    }

    fn descriptor(&self) -> &'static ::protobuf::reflect::MessageDescriptor {
        ::protobuf::MessageStatic::descriptor_static(None::<Self>)
    }
}

impl ::protobuf::MessageStatic for Message {
    fn new() -> Message {
        Message::new()
    }

    fn descriptor_static(_: ::std::option::Option<Message>) -> &'static ::protobuf::reflect::MessageDescriptor {
        static mut descriptor: ::protobuf::lazy::Lazy<::protobuf::reflect::MessageDescriptor> = ::protobuf::lazy::Lazy {
            lock: ::protobuf::lazy::ONCE_INIT,
            ptr: 0 as *const ::protobuf::reflect::MessageDescriptor,
        };
        unsafe {
            descriptor.get(|| {
                let mut fields = ::std::vec::Vec::new();
                fields.push(::protobuf::reflect::accessor::make_option_accessor::<_, ::protobuf::types::ProtobufTypeEnum<Message_MessageType>>(
                    "type",
                    Message::get_field_type_for_reflect,
                    Message::mut_field_type_for_reflect,
                ));
                fields.push(::protobuf::reflect::accessor::make_option_accessor::<_, ::protobuf::types::ProtobufTypeInt32>(
                    "clusterLevelRaw",
                    Message::get_clusterLevelRaw_for_reflect,
                    Message::mut_clusterLevelRaw_for_reflect,
                ));
                fields.push(::protobuf::reflect::accessor::make_singular_field_accessor::<_, ::protobuf::types::ProtobufTypeBytes>(
                    "key",
                    Message::get_key_for_reflect,
                    Message::mut_key_for_reflect,
                ));
                fields.push(::protobuf::reflect::accessor::make_singular_ptr_field_accessor::<_, ::protobuf::types::ProtobufTypeMessage<super::record::Record>>(
                    "record",
                    Message::get_record_for_reflect,
                    Message::mut_record_for_reflect,
                ));
                fields.push(::protobuf::reflect::accessor::make_repeated_field_accessor::<_, ::protobuf::types::ProtobufTypeMessage<Message_Peer>>(
                    "closerPeers",
                    Message::get_closerPeers_for_reflect,
                    Message::mut_closerPeers_for_reflect,
                ));
                fields.push(::protobuf::reflect::accessor::make_repeated_field_accessor::<_, ::protobuf::types::ProtobufTypeMessage<Message_Peer>>(
                    "providerPeers",
                    Message::get_providerPeers_for_reflect,
                    Message::mut_providerPeers_for_reflect,
                ));
                ::protobuf::reflect::MessageDescriptor::new::<Message>(
                    "Message",
                    fields,
                    file_descriptor_proto()
                )
            })
        }
    }
}

impl ::protobuf::Clear for Message {
    fn clear(&mut self) {
        self.clear_field_type();
        self.clear_clusterLevelRaw();
        self.clear_key();
        self.clear_record();
        self.clear_closerPeers();
        self.clear_providerPeers();
        self.unknown_fields.clear();
    }
}

impl ::std::fmt::Debug for Message {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        ::protobuf::text_format::fmt(self, f)
    }
}

impl ::protobuf::reflect::ProtobufValue for Message {
    fn as_ref(&self) -> ::protobuf::reflect::ProtobufValueRef {
        ::protobuf::reflect::ProtobufValueRef::Message(self)
    }
}

#[derive(PartialEq,Clone,Default)]
pub struct Message_Peer {
    // message fields
    id: ::protobuf::SingularField<::std::vec::Vec<u8>>,
    addrs: ::protobuf::RepeatedField<::std::vec::Vec<u8>>,
    connection: ::std::option::Option<Message_ConnectionType>,
    // special fields
    unknown_fields: ::protobuf::UnknownFields,
    cached_size: ::protobuf::CachedSize,
}

// see codegen.rs for the explanation why impl Sync explicitly
unsafe impl ::std::marker::Sync for Message_Peer {}

impl Message_Peer {
    pub fn new() -> Message_Peer {
        ::std::default::Default::default()
    }

    pub fn default_instance() -> &'static Message_Peer {
        static mut instance: ::protobuf::lazy::Lazy<Message_Peer> = ::protobuf::lazy::Lazy {
            lock: ::protobuf::lazy::ONCE_INIT,
            ptr: 0 as *const Message_Peer,
        };
        unsafe {
            instance.get(Message_Peer::new)
        }
    }

    // optional bytes id = 1;

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

    pub fn get_id(&self) -> &[u8] {
        match self.id.as_ref() {
            Some(v) => &v,
            None => &[],
        }
    }

    fn get_id_for_reflect(&self) -> &::protobuf::SingularField<::std::vec::Vec<u8>> {
        &self.id
    }

    fn mut_id_for_reflect(&mut self) -> &mut ::protobuf::SingularField<::std::vec::Vec<u8>> {
        &mut self.id
    }

    // repeated bytes addrs = 2;

    pub fn clear_addrs(&mut self) {
        self.addrs.clear();
    }

    // Param is passed by value, moved
    pub fn set_addrs(&mut self, v: ::protobuf::RepeatedField<::std::vec::Vec<u8>>) {
        self.addrs = v;
    }

    // Mutable pointer to the field.
    pub fn mut_addrs(&mut self) -> &mut ::protobuf::RepeatedField<::std::vec::Vec<u8>> {
        &mut self.addrs
    }

    // Take field
    pub fn take_addrs(&mut self) -> ::protobuf::RepeatedField<::std::vec::Vec<u8>> {
        ::std::mem::replace(&mut self.addrs, ::protobuf::RepeatedField::new())
    }

    pub fn get_addrs(&self) -> &[::std::vec::Vec<u8>] {
        &self.addrs
    }

    fn get_addrs_for_reflect(&self) -> &::protobuf::RepeatedField<::std::vec::Vec<u8>> {
        &self.addrs
    }

    fn mut_addrs_for_reflect(&mut self) -> &mut ::protobuf::RepeatedField<::std::vec::Vec<u8>> {
        &mut self.addrs
    }

    // optional .dht.pb.Message.ConnectionType connection = 3;

    pub fn clear_connection(&mut self) {
        self.connection = ::std::option::Option::None;
    }

    pub fn has_connection(&self) -> bool {
        self.connection.is_some()
    }

    // Param is passed by value, moved
    pub fn set_connection(&mut self, v: Message_ConnectionType) {
        self.connection = ::std::option::Option::Some(v);
    }

    pub fn get_connection(&self) -> Message_ConnectionType {
        self.connection.unwrap_or(Message_ConnectionType::NOT_CONNECTED)
    }

    fn get_connection_for_reflect(&self) -> &::std::option::Option<Message_ConnectionType> {
        &self.connection
    }

    fn mut_connection_for_reflect(&mut self) -> &mut ::std::option::Option<Message_ConnectionType> {
        &mut self.connection
    }
}

impl ::protobuf::Message for Message_Peer {
    fn is_initialized(&self) -> bool {
        true
    }

    fn merge_from(&mut self, is: &mut ::protobuf::CodedInputStream) -> ::protobuf::ProtobufResult<()> {
        while !is.eof()? {
            let (field_number, wire_type) = is.read_tag_unpack()?;
            match field_number {
                1 => {
                    ::protobuf::rt::read_singular_bytes_into(wire_type, is, &mut self.id)?;
                },
                2 => {
                    ::protobuf::rt::read_repeated_bytes_into(wire_type, is, &mut self.addrs)?;
                },
                3 => {
                    ::protobuf::rt::read_proto2_enum_with_unknown_fields_into(wire_type, is, &mut self.connection, 3, &mut self.unknown_fields)?
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
        for value in &self.addrs {
            my_size += ::protobuf::rt::bytes_size(2, &value);
        };
        if let Some(v) = self.connection {
            my_size += ::protobuf::rt::enum_size(3, v);
        }
        my_size += ::protobuf::rt::unknown_fields_size(self.get_unknown_fields());
        self.cached_size.set(my_size);
        my_size
    }

    fn write_to_with_cached_sizes(&self, os: &mut ::protobuf::CodedOutputStream) -> ::protobuf::ProtobufResult<()> {
        if let Some(ref v) = self.id.as_ref() {
            os.write_bytes(1, &v)?;
        }
        for v in &self.addrs {
            os.write_bytes(2, &v)?;
        };
        if let Some(v) = self.connection {
            os.write_enum(3, v.value())?;
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

    fn as_any(&self) -> &::std::any::Any {
        self as &::std::any::Any
    }
    fn as_any_mut(&mut self) -> &mut ::std::any::Any {
        self as &mut ::std::any::Any
    }
    fn into_any(self: Box<Self>) -> ::std::boxed::Box<::std::any::Any> {
        self
    }

    fn descriptor(&self) -> &'static ::protobuf::reflect::MessageDescriptor {
        ::protobuf::MessageStatic::descriptor_static(None::<Self>)
    }
}

impl ::protobuf::MessageStatic for Message_Peer {
    fn new() -> Message_Peer {
        Message_Peer::new()
    }

    fn descriptor_static(_: ::std::option::Option<Message_Peer>) -> &'static ::protobuf::reflect::MessageDescriptor {
        static mut descriptor: ::protobuf::lazy::Lazy<::protobuf::reflect::MessageDescriptor> = ::protobuf::lazy::Lazy {
            lock: ::protobuf::lazy::ONCE_INIT,
            ptr: 0 as *const ::protobuf::reflect::MessageDescriptor,
        };
        unsafe {
            descriptor.get(|| {
                let mut fields = ::std::vec::Vec::new();
                fields.push(::protobuf::reflect::accessor::make_singular_field_accessor::<_, ::protobuf::types::ProtobufTypeBytes>(
                    "id",
                    Message_Peer::get_id_for_reflect,
                    Message_Peer::mut_id_for_reflect,
                ));
                fields.push(::protobuf::reflect::accessor::make_repeated_field_accessor::<_, ::protobuf::types::ProtobufTypeBytes>(
                    "addrs",
                    Message_Peer::get_addrs_for_reflect,
                    Message_Peer::mut_addrs_for_reflect,
                ));
                fields.push(::protobuf::reflect::accessor::make_option_accessor::<_, ::protobuf::types::ProtobufTypeEnum<Message_ConnectionType>>(
                    "connection",
                    Message_Peer::get_connection_for_reflect,
                    Message_Peer::mut_connection_for_reflect,
                ));
                ::protobuf::reflect::MessageDescriptor::new::<Message_Peer>(
                    "Message_Peer",
                    fields,
                    file_descriptor_proto()
                )
            })
        }
    }
}

impl ::protobuf::Clear for Message_Peer {
    fn clear(&mut self) {
        self.clear_id();
        self.clear_addrs();
        self.clear_connection();
        self.unknown_fields.clear();
    }
}

impl ::std::fmt::Debug for Message_Peer {
    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
        ::protobuf::text_format::fmt(self, f)
    }
}

impl ::protobuf::reflect::ProtobufValue for Message_Peer {
    fn as_ref(&self) -> ::protobuf::reflect::ProtobufValueRef {
        ::protobuf::reflect::ProtobufValueRef::Message(self)
    }
}

#[derive(Clone,PartialEq,Eq,Debug,Hash)]
pub enum Message_MessageType {
    PUT_VALUE = 0,
    GET_VALUE = 1,
    ADD_PROVIDER = 2,
    GET_PROVIDERS = 3,
    FIND_NODE = 4,
    PING = 5,
}

impl ::protobuf::ProtobufEnum for Message_MessageType {
    fn value(&self) -> i32 {
        *self as i32
    }

    fn from_i32(value: i32) -> ::std::option::Option<Message_MessageType> {
        match value {
            0 => ::std::option::Option::Some(Message_MessageType::PUT_VALUE),
            1 => ::std::option::Option::Some(Message_MessageType::GET_VALUE),
            2 => ::std::option::Option::Some(Message_MessageType::ADD_PROVIDER),
            3 => ::std::option::Option::Some(Message_MessageType::GET_PROVIDERS),
            4 => ::std::option::Option::Some(Message_MessageType::FIND_NODE),
            5 => ::std::option::Option::Some(Message_MessageType::PING),
            _ => ::std::option::Option::None
        }
    }

    fn values() -> &'static [Self] {
        static values: &'static [Message_MessageType] = &[
            Message_MessageType::PUT_VALUE,
            Message_MessageType::GET_VALUE,
            Message_MessageType::ADD_PROVIDER,
            Message_MessageType::GET_PROVIDERS,
            Message_MessageType::FIND_NODE,
            Message_MessageType::PING,
        ];
        values
    }

    fn enum_descriptor_static(_: ::std::option::Option<Message_MessageType>) -> &'static ::protobuf::reflect::EnumDescriptor {
        static mut descriptor: ::protobuf::lazy::Lazy<::protobuf::reflect::EnumDescriptor> = ::protobuf::lazy::Lazy {
            lock: ::protobuf::lazy::ONCE_INIT,
            ptr: 0 as *const ::protobuf::reflect::EnumDescriptor,
        };
        unsafe {
            descriptor.get(|| {
                ::protobuf::reflect::EnumDescriptor::new("Message_MessageType", file_descriptor_proto())
            })
        }
    }
}

impl ::std::marker::Copy for Message_MessageType {
}

impl ::protobuf::reflect::ProtobufValue for Message_MessageType {
    fn as_ref(&self) -> ::protobuf::reflect::ProtobufValueRef {
        ::protobuf::reflect::ProtobufValueRef::Enum(self.descriptor())
    }
}

#[derive(Clone,PartialEq,Eq,Debug,Hash)]
pub enum Message_ConnectionType {
    NOT_CONNECTED = 0,
    CONNECTED = 1,
    CAN_CONNECT = 2,
    CANNOT_CONNECT = 3,
}

impl ::protobuf::ProtobufEnum for Message_ConnectionType {
    fn value(&self) -> i32 {
        *self as i32
    }

    fn from_i32(value: i32) -> ::std::option::Option<Message_ConnectionType> {
        match value {
            0 => ::std::option::Option::Some(Message_ConnectionType::NOT_CONNECTED),
            1 => ::std::option::Option::Some(Message_ConnectionType::CONNECTED),
            2 => ::std::option::Option::Some(Message_ConnectionType::CAN_CONNECT),
            3 => ::std::option::Option::Some(Message_ConnectionType::CANNOT_CONNECT),
            _ => ::std::option::Option::None
        }
    }

    fn values() -> &'static [Self] {
        static values: &'static [Message_ConnectionType] = &[
            Message_ConnectionType::NOT_CONNECTED,
            Message_ConnectionType::CONNECTED,
            Message_ConnectionType::CAN_CONNECT,
            Message_ConnectionType::CANNOT_CONNECT,
        ];
        values
    }

    fn enum_descriptor_static(_: ::std::option::Option<Message_ConnectionType>) -> &'static ::protobuf::reflect::EnumDescriptor {
        static mut descriptor: ::protobuf::lazy::Lazy<::protobuf::reflect::EnumDescriptor> = ::protobuf::lazy::Lazy {
            lock: ::protobuf::lazy::ONCE_INIT,
            ptr: 0 as *const ::protobuf::reflect::EnumDescriptor,
        };
        unsafe {
            descriptor.get(|| {
                ::protobuf::reflect::EnumDescriptor::new("Message_ConnectionType", file_descriptor_proto())
            })
        }
    }
}

impl ::std::marker::Copy for Message_ConnectionType {
}

impl ::protobuf::reflect::ProtobufValue for Message_ConnectionType {
    fn as_ref(&self) -> ::protobuf::reflect::ProtobufValueRef {
        ::protobuf::reflect::ProtobufValueRef::Enum(self.descriptor())
    }
}

static file_descriptor_proto_data: &'static [u8] = b"\
    \n\tdht.proto\x12\x06dht.pb\x1a\x0crecord.proto\"\xc7\x04\n\x07Message\
    \x12/\n\x04type\x18\x01\x20\x01(\x0e2\x1b.dht.pb.Message.MessageTypeR\
    \x04type\x12(\n\x0fclusterLevelRaw\x18\n\x20\x01(\x05R\x0fclusterLevelRa\
    w\x12\x10\n\x03key\x18\x02\x20\x01(\x0cR\x03key\x12)\n\x06record\x18\x03\
    \x20\x01(\x0b2\x11.record.pb.RecordR\x06record\x126\n\x0bcloserPeers\x18\
    \x08\x20\x03(\x0b2\x14.dht.pb.Message.PeerR\x0bcloserPeers\x12:\n\rprovi\
    derPeers\x18\t\x20\x03(\x0b2\x14.dht.pb.Message.PeerR\rproviderPeers\x1a\
    l\n\x04Peer\x12\x0e\n\x02id\x18\x01\x20\x01(\x0cR\x02id\x12\x14\n\x05add\
    rs\x18\x02\x20\x03(\x0cR\x05addrs\x12>\n\nconnection\x18\x03\x20\x01(\
    \x0e2\x1e.dht.pb.Message.ConnectionTypeR\nconnection\"i\n\x0bMessageType\
    \x12\r\n\tPUT_VALUE\x10\0\x12\r\n\tGET_VALUE\x10\x01\x12\x10\n\x0cADD_PR\
    OVIDER\x10\x02\x12\x11\n\rGET_PROVIDERS\x10\x03\x12\r\n\tFIND_NODE\x10\
    \x04\x12\x08\n\x04PING\x10\x05\"W\n\x0eConnectionType\x12\x11\n\rNOT_CON\
    NECTED\x10\0\x12\r\n\tCONNECTED\x10\x01\x12\x0f\n\x0bCAN_CONNECT\x10\x02\
    \x12\x12\n\x0eCANNOT_CONNECT\x10\x03J\xc9\x10\n\x06\x12\x04\0\0>\x01\n\
    \x08\n\x01\x0c\x12\x03\0\0\x12\n\x08\n\x01\x02\x12\x03\x01\x08\x0e\n\t\n\
    \x02\x03\0\x12\x03\x03\x07\x15\n\n\n\x02\x04\0\x12\x04\x05\0>\x01\n\n\n\
    \x03\x04\0\x01\x12\x03\x05\x08\x0f\n\x0c\n\x04\x04\0\x04\0\x12\x04\x06\
    \x08\r\t\n\x0c\n\x05\x04\0\x04\0\x01\x12\x03\x06\r\x18\n\r\n\x06\x04\0\
    \x04\0\x02\0\x12\x03\x07\x10\x1e\n\x0e\n\x07\x04\0\x04\0\x02\0\x01\x12\
    \x03\x07\x10\x19\n\x0e\n\x07\x04\0\x04\0\x02\0\x02\x12\x03\x07\x1c\x1d\n\
    \r\n\x06\x04\0\x04\0\x02\x01\x12\x03\x08\x10\x1e\n\x0e\n\x07\x04\0\x04\0\
    \x02\x01\x01\x12\x03\x08\x10\x19\n\x0e\n\x07\x04\0\x04\0\x02\x01\x02\x12\
    \x03\x08\x1c\x1d\n\r\n\x06\x04\0\x04\0\x02\x02\x12\x03\t\x10!\n\x0e\n\
    \x07\x04\0\x04\0\x02\x02\x01\x12\x03\t\x10\x1c\n\x0e\n\x07\x04\0\x04\0\
    \x02\x02\x02\x12\x03\t\x1f\x20\n\r\n\x06\x04\0\x04\0\x02\x03\x12\x03\n\
    \x10\"\n\x0e\n\x07\x04\0\x04\0\x02\x03\x01\x12\x03\n\x10\x1d\n\x0e\n\x07\
    \x04\0\x04\0\x02\x03\x02\x12\x03\n\x20!\n\r\n\x06\x04\0\x04\0\x02\x04\
    \x12\x03\x0b\x10\x1e\n\x0e\n\x07\x04\0\x04\0\x02\x04\x01\x12\x03\x0b\x10\
    \x19\n\x0e\n\x07\x04\0\x04\0\x02\x04\x02\x12\x03\x0b\x1c\x1d\n\r\n\x06\
    \x04\0\x04\0\x02\x05\x12\x03\x0c\x10\x19\n\x0e\n\x07\x04\0\x04\0\x02\x05\
    \x01\x12\x03\x0c\x10\x14\n\x0e\n\x07\x04\0\x04\0\x02\x05\x02\x12\x03\x0c\
    \x17\x18\n\x0c\n\x04\x04\0\x04\x01\x12\x04\x0f\x08\x1c\t\n\x0c\n\x05\x04\
    \0\x04\x01\x01\x12\x03\x0f\r\x1b\n^\n\x06\x04\0\x04\x01\x02\0\x12\x03\
    \x11\x10\"\x1aO\x20sender\x20does\x20not\x20have\x20a\x20connection\x20t\
    o\x20peer,\x20and\x20no\x20extra\x20information\x20(default)\n\n\x0e\n\
    \x07\x04\0\x04\x01\x02\0\x01\x12\x03\x11\x10\x1d\n\x0e\n\x07\x04\0\x04\
    \x01\x02\0\x02\x12\x03\x11\x20!\n5\n\x06\x04\0\x04\x01\x02\x01\x12\x03\
    \x14\x10\x1e\x1a&\x20sender\x20has\x20a\x20live\x20connection\x20to\x20p\
    eer\n\n\x0e\n\x07\x04\0\x04\x01\x02\x01\x01\x12\x03\x14\x10\x19\n\x0e\n\
    \x07\x04\0\x04\x01\x02\x01\x02\x12\x03\x14\x1c\x1d\n2\n\x06\x04\0\x04\
    \x01\x02\x02\x12\x03\x17\x10\x20\x1a#\x20sender\x20recently\x20connected\
    \x20to\x20peer\n\n\x0e\n\x07\x04\0\x04\x01\x02\x02\x01\x12\x03\x17\x10\
    \x1b\n\x0e\n\x07\x04\0\x04\x01\x02\x02\x02\x12\x03\x17\x1e\x1f\n\xa7\x01\
    \n\x06\x04\0\x04\x01\x02\x03\x12\x03\x1b\x10#\x1a\x97\x01\x20sender\x20r\
    ecently\x20tried\x20to\x20connect\x20to\x20peer\x20repeatedly\x20but\x20\
    failed\x20to\x20connect\n\x20(\"try\"\x20here\x20is\x20loose,\x20but\x20\
    this\x20should\x20signal\x20\"made\x20strong\x20effort,\x20failed\")\n\n\
    \x0e\n\x07\x04\0\x04\x01\x02\x03\x01\x12\x03\x1b\x10\x1e\n\x0e\n\x07\x04\
    \0\x04\x01\x02\x03\x02\x12\x03\x1b!\"\n\x0c\n\x04\x04\0\x03\0\x12\x04\
    \x1e\x08'\t\n\x0c\n\x05\x04\0\x03\0\x01\x12\x03\x1e\x10\x14\n$\n\x06\x04\
    \0\x03\0\x02\0\x12\x03\x20\x10&\x1a\x15\x20ID\x20of\x20a\x20given\x20pee\
    r.\n\n\x0e\n\x07\x04\0\x03\0\x02\0\x04\x12\x03\x20\x10\x18\n\x0e\n\x07\
    \x04\0\x03\0\x02\0\x05\x12\x03\x20\x19\x1e\n\x0e\n\x07\x04\0\x03\0\x02\0\
    \x01\x12\x03\x20\x1f!\n\x0e\n\x07\x04\0\x03\0\x02\0\x03\x12\x03\x20$%\n,\
    \n\x06\x04\0\x03\0\x02\x01\x12\x03#\x10)\x1a\x1d\x20multiaddrs\x20for\
    \x20a\x20given\x20peer\n\n\x0e\n\x07\x04\0\x03\0\x02\x01\x04\x12\x03#\
    \x10\x18\n\x0e\n\x07\x04\0\x03\0\x02\x01\x05\x12\x03#\x19\x1e\n\x0e\n\
    \x07\x04\0\x03\0\x02\x01\x01\x12\x03#\x1f$\n\x0e\n\x07\x04\0\x03\0\x02\
    \x01\x03\x12\x03#'(\nP\n\x06\x04\0\x03\0\x02\x02\x12\x03&\x107\x1aA\x20u\
    sed\x20to\x20signal\x20the\x20sender's\x20connection\x20capabilities\x20\
    to\x20the\x20peer\n\n\x0e\n\x07\x04\0\x03\0\x02\x02\x04\x12\x03&\x10\x18\
    \n\x0e\n\x07\x04\0\x03\0\x02\x02\x06\x12\x03&\x19'\n\x0e\n\x07\x04\0\x03\
    \0\x02\x02\x01\x12\x03&(2\n\x0e\n\x07\x04\0\x03\0\x02\x02\x03\x12\x03&56\
    \n2\n\x04\x04\0\x02\0\x12\x03*\x08&\x1a%\x20defines\x20what\x20type\x20o\
    f\x20message\x20it\x20is.\n\n\x0c\n\x05\x04\0\x02\0\x04\x12\x03*\x08\x10\
    \n\x0c\n\x05\x04\0\x02\0\x06\x12\x03*\x11\x1c\n\x0c\n\x05\x04\0\x02\0\
    \x01\x12\x03*\x1d!\n\x0c\n\x05\x04\0\x02\0\x03\x12\x03*$%\nO\n\x04\x04\0\
    \x02\x01\x12\x03-\x08,\x1aB\x20defines\x20what\x20coral\x20cluster\x20le\
    vel\x20this\x20query/response\x20belongs\x20to.\n\n\x0c\n\x05\x04\0\x02\
    \x01\x04\x12\x03-\x08\x10\n\x0c\n\x05\x04\0\x02\x01\x05\x12\x03-\x11\x16\
    \n\x0c\n\x05\x04\0\x02\x01\x01\x12\x03-\x17&\n\x0c\n\x05\x04\0\x02\x01\
    \x03\x12\x03-)+\nw\n\x04\x04\0\x02\x02\x12\x031\x08\x1f\x1aj\x20Used\x20\
    to\x20specify\x20the\x20key\x20associated\x20with\x20this\x20message.\n\
    \x20PUT_VALUE,\x20GET_VALUE,\x20ADD_PROVIDER,\x20GET_PROVIDERS\n\n\x0c\n\
    \x05\x04\0\x02\x02\x04\x12\x031\x08\x10\n\x0c\n\x05\x04\0\x02\x02\x05\
    \x12\x031\x11\x16\n\x0c\n\x05\x04\0\x02\x02\x01\x12\x031\x17\x1a\n\x0c\n\
    \x05\x04\0\x02\x02\x03\x12\x031\x1d\x1e\n;\n\x04\x04\0\x02\x03\x12\x035\
    \x08-\x1a.\x20Used\x20to\x20return\x20a\x20value\n\x20PUT_VALUE,\x20GET_\
    VALUE\n\n\x0c\n\x05\x04\0\x02\x03\x04\x12\x035\x08\x10\n\x0c\n\x05\x04\0\
    \x02\x03\x06\x12\x035\x11!\n\x0c\n\x05\x04\0\x02\x03\x01\x12\x035\"(\n\
    \x0c\n\x05\x04\0\x02\x03\x03\x12\x035+,\nc\n\x04\x04\0\x02\x04\x12\x039\
    \x08&\x1aV\x20Used\x20to\x20return\x20peers\x20closer\x20to\x20a\x20key\
    \x20in\x20a\x20query\n\x20GET_VALUE,\x20GET_PROVIDERS,\x20FIND_NODE\n\n\
    \x0c\n\x05\x04\0\x02\x04\x04\x12\x039\x08\x10\n\x0c\n\x05\x04\0\x02\x04\
    \x06\x12\x039\x11\x15\n\x0c\n\x05\x04\0\x02\x04\x01\x12\x039\x16!\n\x0c\
    \n\x05\x04\0\x02\x04\x03\x12\x039$%\nO\n\x04\x04\0\x02\x05\x12\x03=\x08(\
    \x1aB\x20Used\x20to\x20return\x20Providers\n\x20GET_VALUE,\x20ADD_PROVID\
    ER,\x20GET_PROVIDERS\n\n\x0c\n\x05\x04\0\x02\x05\x04\x12\x03=\x08\x10\n\
    \x0c\n\x05\x04\0\x02\x05\x06\x12\x03=\x11\x15\n\x0c\n\x05\x04\0\x02\x05\
    \x01\x12\x03=\x16#\n\x0c\n\x05\x04\0\x02\x05\x03\x12\x03=&'\
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
