// Copyright 2018 Parity Technologies (UK) Ltd.
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

#![recursion_limit = "256"]
#![cfg_attr(docsrs, feature(doc_cfg, doc_auto_cfg))]

mod syn_ext;

use crate::syn_ext::RequireStrLit;
use heck::ToUpperCamelCase;
use proc_macro::TokenStream;
use quote::{format_ident, quote, ToTokens};
use syn::punctuated::Punctuated;
use syn::visit::Visit;
use syn::{parse_macro_input, Data, DataStruct, DeriveInput, Meta, Token};

/// Generates a delegating `NetworkBehaviour` implementation for the struct this is used for. See
/// the trait documentation for better description.
#[proc_macro_derive(NetworkBehaviour, attributes(behaviour))]
pub fn hello_macro_derive(input: TokenStream) -> TokenStream {
    let ast = parse_macro_input!(input as DeriveInput);
    build(&ast).unwrap_or_else(|e| e.to_compile_error().into())
}

/// The actual implementation.
fn build(ast: &DeriveInput) -> syn::Result<TokenStream> {
    match ast.data {
        Data::Struct(ref s) => build_struct(ast, s),
        Data::Enum(_) => Err(syn::Error::new_spanned(
            ast,
            "Cannot derive `NetworkBehaviour` on enums",
        )),
        Data::Union(_) => Err(syn::Error::new_spanned(
            ast,
            "Cannot derive `NetworkBehaviour` on union",
        )),
    }
}

struct FindParam<'a> {
    found: bool,
    generics: &'a syn::Generics,
}

impl<'a> Visit<'a> for FindParam<'_> {
    fn visit_ident(&mut self, i: &syn::Ident) {
        self.found |= self.generics.type_params().any(|param| param.ident == *i);
    }
}

fn build_struct(ast: &DeriveInput, data_struct: &DataStruct) -> syn::Result<TokenStream> {
    let BehaviourAttributes {
        prelude_path,
        user_specified_out_event,
    } = parse_attributes(ast)?;

    let multiaddr = quote! { #prelude_path::Multiaddr };
    let trait_to_impl = quote! { #prelude_path::NetworkBehaviour };
    let peer_id = quote! { #prelude_path::PeerId };
    let connection_id = quote! { #prelude_path::ConnectionId };
    let from_swarm = quote! { #prelude_path::FromSwarm };
    let t_handler = quote! { #prelude_path::THandler };
    let t_handler_in_event = quote! { #prelude_path::THandlerInEvent };
    let t_handler_out_event = quote! { #prelude_path::THandlerOutEvent };
    let endpoint = quote! { #prelude_path::Endpoint };
    let connection_denied = quote! { #prelude_path::ConnectionDenied };
    let to_swarm = quote! { #prelude_path::ToSwarm };
    let connection_handler = quote! { #prelude_path::ConnectionHandler };
    let substream_protocol = quote! { #prelude_path::SubstreamProtocol };
    let connection_handler_event = quote! { #prelude_path::ConnectionHandlerEvent };
    let connection_event = quote! { #prelude_path::ConnectionEvent };
    let upgrade_info = quote! { #prelude_path::UpgradeInfo };
    let upgrade_info_send = quote! { #prelude_path::UpgradeInfoSend };
    let inbound_upgrade = quote! { #prelude_path::InboundUpgrade };
    let inbound_upgrade_send = quote! { #prelude_path::InboundUpgradeSend };
    let outbound_upgrade = quote! { #prelude_path::OutboundUpgrade };
    let outbound_upgrade_send = quote! { #prelude_path::OutboundUpgradeSend };
    let stream = quote! { #prelude_path::Stream };
    let fully_negotiated_inbound = quote! { #prelude_path::FullyNegotiatedInbound };
    let fully_negotiated_outbound = quote! { #prelude_path::FullyNegotiatedOutbound };
    let address_change = quote! { #prelude_path::HandlerAddressChange };
    let dial_upgrade_error = quote! { #prelude_path::DialUpgradeError };
    let listen_upgrade_error = quote! { #prelude_path::ListenUpgradeError };
    let stream_upgrade_error = quote! { #prelude_path::StreamUpgradeError };

    let mut generics = ast.generics.clone();
    generics.params.iter_mut().for_each(|param| {
        if let syn::GenericParam::Type(ref mut type_param) = *param {
            type_param.bounds.push(syn::parse_quote!('static));
        }
    });

    let generic_fields = data_struct
        .fields
        .iter()
        .map(|field| &field.ty)
        .filter(|ty| {
            let mut visitor = FindParam {
                found: false,
                generics: &generics,
            };
            visitor.visit_type(ty);
            visitor.found
        })
        .collect::<Vec<_>>();

    for field in generic_fields.iter() {
        generics
            .make_where_clause()
            .predicates
            .push(syn::parse_quote!(#field: #trait_to_impl));
        generics
            .make_where_clause()
            .predicates
            .push(syn::parse_quote!(<#field as #trait_to_impl>::ToSwarm: std::fmt::Debug));
    }

    let fields = data_struct
        .fields
        .iter()
        .map(unwrap_field_name)
        .collect::<syn::Result<Vec<_>>>()?;
    let var_names = fields
        .iter()
        .copied()
        .map(ident_to_camcel_case)
        .collect::<Vec<_>>();
    let behaviour_types = data_struct
        .fields
        .iter()
        .map(|field| &field.ty)
        .collect::<Vec<_>>();
    let indices = (0..data_struct.fields.len()).collect::<Vec<_>>();
    let placeholder_names = (0..data_struct.fields.len())
        .map(|i| format_ident!("placeholder{}", i))
        .collect::<Vec<_>>();
    let beh_count = data_struct.fields.len();

    let (impl_gen, type_gen, where_clause) = generics.split_for_impl();

    let ci = |i| concat_idents(&ast.ident, i);

    let handler = ci("Handler");
    let to_beh = ci("ToBehaviour");
    let from_beh = ci("FromBehaviour");
    let iu = ci("InboundProtocol");
    let ou = ci("OutboundProtocol");
    let ioi = ci("InboundOpenInfo");
    let ooi = ci("OutboundOpenInfo");
    let iu_ui = ci("InboundUpgradeInfo");
    let iu_uii = ci("InboundUpgradeInfoIter");
    let iu_out = ci("InboundUpgradeOutput");
    let iu_err = ci("InboundUpgradeError");
    let iu_fut = ci("InboundUpgradeFuture");
    let ou_ui = ci("OutboundUpgradeInfo");
    let ou_uii = ci("OutboundUpgradeInfoIter");
    let ou_out = ci("OutboundUpgradeOutput");
    let ou_err = ci("OutboundUpgradeError");
    let ou_fut = ci("OutboundUpgradeFuture");

    let decl_enum = |derives, name: &_, ty: proc_macro2::TokenStream| {
        quote! {
            pub enum #name #impl_gen #where_clause {#(
                #var_names(#prelude_path::#ty<#behaviour_types>),
            )*}
            #derives
        }
    };
    let debug_enum = |name: &_, ty| {
        decl_enum(
            quote! {
                impl #impl_gen std::fmt::Debug for #name #type_gen #where_clause {
                    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        match self {#(
                            Self::#var_names(v) => write!(f, "{}({:?})", stringify!(#var_names), v),
                        )*}
                    }
                }
            },
            name,
            ty,
        )
    };
    let plain_enum = |name: &_, ty| decl_enum(quote! {}, name, ty);

    let enums = [
        debug_enum(&to_beh, quote!(THandlerOutEvent)),
        debug_enum(&from_beh, quote!(THandlerInEvent)),
        plain_enum(&ou, quote!(THandlerOutProtocol)),
        plain_enum(&ooi, quote!(THandlerOutOpenInfo)),
        plain_enum(&iu_out, quote!(THandlerInUpgradeOutput)),
        plain_enum(&iu_err, quote!(THandlerInUpgradeError)),
        plain_enum(&iu_fut, quote!(THandlerInUpgradeFuture)),
        plain_enum(&iu_ui, quote!(THandlerInUpgradeInfo)),
        plain_enum(&ou_ui, quote!(THandlerOutUpgradeInfo)),
        plain_enum(&ou_uii, quote!(THandlerOutUpgradeInfoIter)),
        plain_enum(&ou_out, quote!(THandlerOutUpgradeOutput)),
        plain_enum(&ou_err, quote!(THandlerOutUpgradeError)),
        plain_enum(&ou_fut, quote!(THandlerOutUpgradeFuture)),
    ];

    let (event, event_impl, poll_impl) = match user_specified_out_event {
        Some(ty) => (
            ty.to_token_stream(),
            quote! {},
            quote! {#(
                if let std::task::Poll::Ready(event) = self.#fields.poll(cx) {
                    return std::task::Poll::Ready(event
                        .map_in(#t_handler_in_event::<Self>::#var_names)
                        .map_out(|e| e.into()));
                }
            )*},
        ),
        None => {
            let ty = concat_idents(&ast.ident, "Event");
            let def = debug_enum(&ty, quote!(TBehaviourOutEvent));
            let poll = quote! {#(
                if let std::task::Poll::Ready(event) = self.#fields.poll(cx) {
                    return std::task::Poll::Ready(event
                        .map_in(#t_handler_in_event::<Self>::#var_names)
                        .map_out(Self::ToSwarm::#var_names));
                }
            )*};
            (quote!(#ty #type_gen), def, poll)
        }
    };

    let decl_struct =
        |name, ty: proc_macro2::TokenStream, extra_fields: proc_macro2::TokenStream| {
            quote! {
                pub struct #name #impl_gen #where_clause {#(
                    #fields: #prelude_path::#ty<#behaviour_types>,
                )* #extra_fields}
            }
        };
    let plain_struct = |name, ty| decl_struct(name, ty, quote!());
    let indexed_struct = |name, ty| decl_struct(name, ty, quote!(field_index: usize,));

    let structs = [
        indexed_struct(&handler, quote!(THandler)),
        indexed_struct(&iu_uii, quote!(THandlerInUpgradeInfoIter)),
        plain_struct(&iu, quote!(THandlerInProtocol)),
        plain_struct(&ioi, quote!(THandlerInOpenInfo)),
    ];

    let name = &ast.ident;
    Ok(quote! {
        #event_impl

        impl #impl_gen #trait_to_impl for #name #type_gen #where_clause {
            type ToSwarm = #event;
            type ConnectionHandler = #handler #type_gen;

            fn handle_pending_inbound_connection(
                &mut self,
                connection_id: #connection_id,
                local_addr: &#multiaddr,
                remote_addr: &#multiaddr,
            ) -> std::result::Result<(), #connection_denied> {
                #(
                    self.#fields.handle_pending_inbound_connection(
                        connection_id,
                        local_addr,
                        remote_addr,
                    )?;
                )*

                Ok(())
            }

            fn handle_established_inbound_connection(
                &mut self,
                connection_id: #connection_id,
                peer: #peer_id,
                local_addr: &#multiaddr,
                remote_addr: &#multiaddr,
            ) -> std::result::Result<#t_handler<Self>, #connection_denied> {
                Ok(Self::ConnectionHandler {
                    #(#fields: self.#fields.handle_established_inbound_connection(
                        connection_id,
                        peer,
                        local_addr,
                        remote_addr,
                    )?,)*
                    field_index: 0,
                })
            }

            fn handle_pending_outbound_connection(
                &mut self,
                connection_id: #connection_id,
                maybe_peer: Option<#peer_id>,
                addresses: &[#multiaddr],
                effective_role: #endpoint,
            ) -> std::result::Result<Vec<#multiaddr>, #connection_denied> {
                let mut addrs = Vec::new();
                #(
                    addrs.extend(self.#fields.handle_pending_outbound_connection(
                        connection_id,
                        maybe_peer,
                        addresses,
                        effective_role,
                    )?);
                )*
                Ok(addrs)
            }

            fn handle_established_outbound_connection(
                &mut self,
                connection_id: #connection_id,
                peer: #peer_id,
                addr: &#multiaddr,
                role_override: #endpoint,
            ) -> std::result::Result<#t_handler<Self>, #connection_denied> {
                Ok(Self::ConnectionHandler {
                    #(#fields: self.#fields.handle_established_outbound_connection(
                        connection_id,
                        peer,
                        addr,
                        role_override,
                    )?,)*
                    field_index: 0,
                })
            }

            fn on_swarm_event(&mut self, event: #from_swarm) {
                #(self.#fields.on_swarm_event(event.clone());)*
            }

            fn on_connection_handler_event(
                &mut self,
                peer_id: #peer_id,
                connection_id: #connection_id,
                event: #t_handler_out_event<Self>,
            ) {
                match event {#(
                    #t_handler_out_event::<Self>::#var_names(event) => {
                        self.#fields.on_connection_handler_event(
                            peer_id,
                            connection_id,
                            event,
                        );
                    }
                )*}
            }

            fn poll(&mut self, cx: &mut std::task::Context<'_>)
                -> std::task::Poll<#to_swarm<Self::ToSwarm, #t_handler_in_event<Self>>>
            {
                #poll_impl
                std::task::Poll::Pending
            }
        }

        #(#enums)*
        #(#structs)*

        impl #impl_gen #connection_handler for #handler #type_gen #where_clause {
            type FromBehaviour = #from_beh #type_gen;
            type ToBehaviour = #to_beh #type_gen;
            type InboundProtocol = #iu #type_gen;
            type OutboundProtocol = #ou #type_gen;
            type InboundOpenInfo = #ioi #type_gen;
            type OutboundOpenInfo = #ooi #type_gen;

            fn listen_protocol(&self) -> #substream_protocol<Self::InboundProtocol, Self::InboundOpenInfo> {
                #(let #fields = self.#fields.listen_protocol();)*
                let timeout = std::time::Duration::from_secs(0) #(.max(*#fields.timeout()))*;
                #(let (#fields, #placeholder_names) = #fields.into_upgrade();)*
                let upgrade = #iu { #(#fields,)* };
                let info = #ioi { #(#fields: #placeholder_names,)* };
                #substream_protocol::new(upgrade, info).with_timeout(timeout)
            }

            fn connection_keep_alive(&self) -> bool {
                false #(|| self.#fields.connection_keep_alive())*
            }

            fn poll(
                &mut self,
                cx: &mut std::task::Context<'_>,
            ) -> std::task::Poll<
                #connection_handler_event<Self::OutboundProtocol, Self::OutboundOpenInfo, Self::ToBehaviour>,
            > {
                for _ in 0..#beh_count {
                    // save the poll position to avoid repolling exhaused handlers
                    match self.field_index {
                        #(#indices => match self.#fields.poll(cx) {
                            std::task::Poll::Ready(event) =>
                                return std::task::Poll::Ready(event
                                    .map_custom(#to_beh::#var_names)
                                    .map_outbound_open_info(#ooi::#var_names)
                                    .map_protocol(#ou::#var_names)),
                            std::task::Poll::Pending => {}
                        },)*
                        _ => {
                            self.field_index = 0;
                            continue;
                        }
                    }
                    self.field_index += 1;
                }

                std::task::Poll::Pending
            }

            fn poll_close(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<Option<Self::ToBehaviour>> {
                let mut found_pending = false;
                for _ in 0..#beh_count {
                    match self.field_index {
                        #(#indices => match self.#fields.poll_close(cx) {
                            std::task::Poll::Ready(Some(event)) =>
                                return std::task::Poll::Ready(Some(#to_beh::#var_names(event))),
                            std::task::Poll::Pending => found_pending = true,
                            std::task::Poll::Ready(None) => {}
                        },)*
                        _ => {
                            self.field_index = 0;
                            continue;
                        }
                    }
                    self.field_index += 1;
                }

                if found_pending {
                    std::task::Poll::Pending
                } else {
                    std::task::Poll::Ready(None)
                }
            }

            fn on_behaviour_event(&mut self, event: Self::FromBehaviour) {
                match event {#(
                    Self::FromBehaviour::#var_names(event) => self.#fields.on_behaviour_event(event),
                )*}
            }

            fn on_connection_event(
                &mut self,
                event: #connection_event<
                    Self::InboundProtocol,
                    Self::OutboundProtocol,
                    Self::InboundOpenInfo,
                    Self::OutboundOpenInfo,
                >,
            ) {
                match event {
                    #connection_event::FullyNegotiatedOutbound(fully_negotiated_outbound) => {
                        match (fully_negotiated_outbound.protocol, fully_negotiated_outbound.info) {
                            #(
                                (#ou_out::#var_names(protocol), #ooi::#var_names(info)) => {
                                    self.#fields.on_connection_event(
                                        #connection_event::FullyNegotiatedOutbound(#fully_negotiated_outbound {
                                            protocol,
                                            info,
                                        }),
                                    );
                                }
                            )*
                            _ => unreachable!(),
                        }
                    }
                    #connection_event::FullyNegotiatedInbound(fully_negotiated_inbound) => {
                        match fully_negotiated_inbound.protocol {#(
                            #iu_out::#var_names(protocol) => {
                                self.#fields.on_connection_event(
                                    #connection_event::FullyNegotiatedInbound(#fully_negotiated_inbound {
                                        protocol,
                                        info: fully_negotiated_inbound.info.#fields,
                                    }),
                                );
                            }
                        )*}
                    }
                    #connection_event::AddressChange(address) => {
                        #(
                            self.#fields
                                .on_connection_event(#connection_event::AddressChange(#address_change {
                                    new_address: address.new_address,
                                }));
                        )*
                    }
                    #connection_event::DialUpgradeError(dial_upgrade_error) => {
                        match (dial_upgrade_error.error, dial_upgrade_error.info) {
                            #(
                                (#stream_upgrade_error::Apply(#ou_err::#var_names(error)), #ooi::#var_names(info)) => {
                                    self.#fields
                                        .on_connection_event(#connection_event::DialUpgradeError(#dial_upgrade_error {
                                            error: #stream_upgrade_error::Apply(error),
                                            info,
                                        }));
                                }
                            )*
                            #(
                                (error, #ooi::#var_names(info)) => {
                                    self.#fields
                                        .on_connection_event(#connection_event::DialUpgradeError(#dial_upgrade_error {
                                            error: error.map_upgrade_err(|_| unreachable!()),
                                            info,
                                        }));
                                }
                            )*
                        }
                    }
                    #connection_event::ListenUpgradeError(listen_upgrade_error) => {
                        match listen_upgrade_error.error {#(
                            #iu_err::#var_names(error) => {
                                self.#fields
                                    .on_connection_event(#connection_event::ListenUpgradeError(#listen_upgrade_error {
                                        error,
                                        info: listen_upgrade_error.info.#fields,
                                    }));
                            }
                        )*}
                    }
                    #connection_event::LocalProtocolsChange(supported_protocols) => {
                        #(
                            self.#fields
                                .on_connection_event(#connection_event::LocalProtocolsChange(supported_protocols.clone()));
                        )*
                    }
                    #connection_event::RemoteProtocolsChange(supported_protocols) => {
                        #(
                            self.#fields
                                .on_connection_event(#connection_event::RemoteProtocolsChange(supported_protocols.clone()));
                        )*
                    }
                    _ => {}
                }
            }
        }


        impl #impl_gen #upgrade_info for #ou #type_gen #where_clause {
            type Info = #ou_ui #type_gen;
            type InfoIter = #ou_uii #type_gen;

            fn protocol_info(&self) -> Self::InfoIter {
                match self {#(
                    Self::#var_names(ou) => #ou_uii::#var_names(#upgrade_info_send::protocol_info(ou)),
                )*}
            }
        }

        impl #impl_gen AsRef<str> for #ou_ui #type_gen #where_clause {
            fn as_ref(&self) -> &str {
                match self {#(
                    Self::#var_names(info) => AsRef::<str>::as_ref(info),
                )*}
            }
        }

        impl #impl_gen Clone for #ou_ui #type_gen #where_clause {
            fn clone(&self) -> Self {
                match self {#(
                    Self::#var_names(info) => Self::#var_names(info.clone()),
                )*}
            }
        }

        impl #impl_gen #outbound_upgrade<#stream> for #ou #type_gen #where_clause {
            type Output = #ou_out #type_gen;
            type Error = #ou_err #type_gen;
            type Future = #ou_fut #type_gen;

            fn upgrade_outbound(self, socket: #stream, info: Self::Info) -> Self::Future {
                match (self, info) {
                    #((Self::#var_names(this), Self::Info::#var_names(info)) =>
                        Self::Future::#var_names(#outbound_upgrade_send::upgrade_outbound(this, socket, info)),)*
                    _ => unreachable!("incorect invocation")
                }
            }
        }

        impl #impl_gen Iterator for #ou_uii #type_gen #where_clause {
            type Item = #ou_ui #type_gen;

            fn next(&mut self) -> Option<Self::Item> {
                match self {#(
                    Self::#var_names(i) => i.next().map(#ou_ui::#var_names),
                )*}
            }
        }

        impl #impl_gen std::future::Future for #ou_fut #type_gen #where_clause {
            type Output = std::result::Result<#ou_out #type_gen, #ou_err #type_gen>;

            fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
                // SAFETY: we dont move anything out
                unsafe {
                    match self.get_unchecked_mut() {#(
                        Self::#var_names(fut) => std::pin::Pin::new_unchecked(fut)
                            .poll(cx).map_ok(#ou_out::#var_names).map_err(#ou_err::#var_names),
                    )*}
                }
            }
        }

        impl #impl_gen #upgrade_info for #iu #type_gen #where_clause {
            type Info = #iu_ui #type_gen;
            type InfoIter = #iu_uii #type_gen;

            fn protocol_info(&self) -> Self::InfoIter {
                Self::InfoIter {
                    #(#fields: #upgrade_info_send::protocol_info(&self.#fields),)*
                    field_index: 0,
                }
            }
        }

        impl #impl_gen Iterator for #iu_uii #type_gen #where_clause {
            type Item = #iu_ui #type_gen;

            fn next(&mut self) -> Option<Self::Item> {
                // simpl #impl_gene state machine that yields better performance than
                // naive sequential calls of next
                loop {
                    match self.field_index {
                        #(
                            #indices => match self.#fields.next() {
                                Some(info) => return Some(#iu_ui::#var_names(info)),
                                None => {}
                            },
                        )*
                        _ => return None,
                    }
                    self.field_index += 1;
                }
            }
        }

        impl #impl_gen AsRef<str> for #iu_ui #type_gen #where_clause {
            fn as_ref(&self) -> &str {
                match self {#(
                    Self::#var_names(info) => AsRef::<str>::as_ref(info),
                )*}
            }
        }

        impl #impl_gen Clone for #iu_ui #type_gen #where_clause {
            fn clone(&self) -> Self {
                match self {#(
                    Self::#var_names(info) => Self::#var_names(info.clone()),
                )*}
            }
        }

        impl #impl_gen #inbound_upgrade<#stream> for #iu #type_gen #where_clause {
            type Output = #iu_out #type_gen;
            type Error = #iu_err #type_gen;
            type Future = #iu_fut #type_gen;

            fn upgrade_inbound(self, socket: #stream, info: Self::Info) -> Self::Future {
                match info {#(
                    Self::Info::#var_names(info) =>
                        Self::Future::#var_names(#inbound_upgrade_send::upgrade_inbound(self.#fields, socket, info)),
                )*}
            }
        }

        impl #impl_gen std::future::Future for #iu_fut #type_gen #where_clause {
            type Output = std::result::Result<#iu_out #type_gen, #iu_err #type_gen>;

            fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
                unsafe {
                    match self.get_unchecked_mut() {#(
                        Self::#var_names(fut) => std::pin::Pin::new_unchecked(fut)
                            .poll(cx).map_ok(#iu_out::#var_names).map_err(#iu_err::#var_names),
                    )*}
                }
            }
        }

    }.into())
}

fn ident_to_camcel_case(ident: &syn::Ident) -> syn::Ident {
    syn::Ident::new(&ident.to_string().to_upper_camel_case(), ident.span())
}

fn unwrap_field_name(field: &syn::Field) -> syn::Result<&syn::Ident> {
    field
        .ident
        .as_ref()
        .ok_or_else(|| syn::Error::new_spanned(field, "fields must be named"))
}

struct BehaviourAttributes {
    prelude_path: syn::Path,
    user_specified_out_event: Option<syn::Type>,
}

/// Parses the `value` of a key=value pair in the `#[behaviour]` attribute into the requested type.
fn parse_attributes(ast: &DeriveInput) -> syn::Result<BehaviourAttributes> {
    let mut attributes = BehaviourAttributes {
        prelude_path: syn::parse_quote! { ::libp2p::swarm::derive_prelude },
        user_specified_out_event: None,
    };

    for attr in ast
        .attrs
        .iter()
        .filter(|attr| attr.path().is_ident("behaviour"))
    {
        let nested = attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;

        for meta in nested {
            if meta.path().is_ident("prelude") {
                let value = meta.require_name_value()?.value.require_str_lit()?;

                attributes.prelude_path = syn::parse_str(&value)?;

                continue;
            }

            if meta.path().is_ident("to_swarm") || meta.path().is_ident("out_event") {
                let value = meta.require_name_value()?.value.require_str_lit()?;

                attributes.user_specified_out_event = Some(syn::parse_str(&value)?);

                continue;
            }
        }
    }

    Ok(attributes)
}

fn concat_idents(a: &syn::Ident, b: &str) -> syn::Ident {
    syn::Ident::new(&format!("{a}{b}"), a.span())
}
