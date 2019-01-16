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

extern crate proc_macro;
#[macro_use]
extern crate syn;
#[macro_use]
extern crate quote;

use self::proc_macro::TokenStream;
use syn::{DeriveInput, Data, DataStruct, Ident};

/// The interface that satisfies Rust.
#[proc_macro_derive(NetworkBehaviour, attributes(behaviour))]
pub fn hello_macro_derive(input: TokenStream) -> TokenStream {
    let ast = parse_macro_input!(input as DeriveInput);
    build(&ast)
}

/// The actual implementation.
fn build(ast: &DeriveInput) -> TokenStream {
    match ast.data {
        Data::Struct(ref s) => build_struct(ast, s),
        Data::Enum(_) => unimplemented!("Deriving NetworkBehaviour is not implemented for enums"),
        Data::Union(_) => unimplemented!("Deriving NetworkBehaviour is not implemented for unions"),
    }
}

/// The version for structs
fn build_struct(ast: &DeriveInput, data_struct: &DataStruct) -> TokenStream {
    let name = &ast.ident;
    let (_, ty_generics, where_clause) = ast.generics.split_for_impl();
    let trait_to_impl = quote!{::libp2p::core::swarm::NetworkBehaviour};
    let swarm_event = quote!{::libp2p::core::swarm::SwarmEvent};
    let net_behv_event_proc = quote!{::libp2p::core::swarm::NetworkBehaviourEventProcess};
    let either_ident = quote!{::libp2p::core::either::EitherOutput};
    let network_behaviour_action = quote!{::libp2p::core::swarm::NetworkBehaviourAction};
    let into_protocols_handler = quote!{::libp2p::core::protocols_handler::IntoProtocolsHandler};
    let protocols_handler = quote!{::libp2p::core::protocols_handler::ProtocolsHandler};
    let into_proto_select_ident = quote!{::libp2p::core::protocols_handler::IntoProtocolsHandlerSelect};
    let peer_id = quote!{::libp2p::core::PeerId};
    let connected_point = quote!{::libp2p::core::swarm::ConnectedPoint};

    // Name of the type parameter that represents the substream.
    let substream_generic = {
        let mut n = "TSubstream".to_string();
        // Avoid collisions.
        while ast.generics.type_params().any(|tp| tp.ident.to_string() == n) {
            n.push('1');
        }
        let n = Ident::new(&n, name.span());
        quote!{#n}
    };

    // Name of the type parameter that represents the topology.
    let topology_generic = {
        let mut n = "TTopology".to_string();
        // Avoid collisions.
        while ast.generics.type_params().any(|tp| tp.ident.to_string() == n) {
            n.push('1');
        }
        let n = Ident::new(&n, name.span());
        quote!{#n}
    };

    let poll_parameters = quote!{::libp2p::core::swarm::PollParameters<#topology_generic, <<Self::ProtocolsHandler as #into_protocols_handler>::Handler as #protocols_handler>::InEvent>};

    // Build the generics.
    let impl_generics = {
        let tp = ast.generics.type_params();
        let lf = ast.generics.lifetimes();
        let cst = ast.generics.const_params();
        quote!{<#(#lf,)* #(#tp,)* #(#cst,)* #topology_generic, #substream_generic>}
    };

    // Build the `where ...` clause of the trait implementation.
    let where_clause = {
        let mut additional = data_struct.fields.iter()
            .filter(|x| !is_ignored(x))
            .flat_map(|field| {
                let ty = &field.ty;
                vec![
                    quote!{#ty: #trait_to_impl<#topology_generic>},
                    quote!{Self: #net_behv_event_proc<<#ty as #trait_to_impl<#topology_generic>>::OutEvent>},
                    quote!{<<#ty as #trait_to_impl<#topology_generic>>::ProtocolsHandler as #into_protocols_handler>::Handler: #protocols_handler<Substream = #substream_generic>},
                    // Note: this bound is required because of https://github.com/rust-lang/rust/issues/55697
                    quote!{<<<#ty as #trait_to_impl<#topology_generic>>::ProtocolsHandler as #into_protocols_handler>::Handler as #protocols_handler>::InboundProtocol: ::libp2p::core::InboundUpgrade<#substream_generic>},
                    quote!{<<<#ty as #trait_to_impl<#topology_generic>>::ProtocolsHandler as #into_protocols_handler>::Handler as #protocols_handler>::OutboundProtocol: ::libp2p::core::OutboundUpgrade<#substream_generic>},
                ]
            })
            .collect::<Vec<_>>();

        additional.push(quote!{#substream_generic: ::libp2p::tokio_io::AsyncRead});
        additional.push(quote!{#substream_generic: ::libp2p::tokio_io::AsyncWrite});

        if let Some(where_clause) = where_clause {
            Some(quote!{#where_clause, #(#additional),*})
        } else {
            Some(quote!{where #(#additional),*})
        }
    };

    // The final out event.
    // If we find a `#[behaviour(out_event = "Foo")]` attribute on the struct, we set `Foo` as
    // the out event. Otherwise we use `()`.
    let out_event = {
        let mut out = quote!{()};
        for meta_items in ast.attrs.iter().filter_map(get_meta_items) {
            for meta_item in meta_items {
                match meta_item {
                    syn::NestedMeta::Meta(syn::Meta::NameValue(ref m)) if m.ident == "out_event" => {
                        if let syn::Lit::Str(ref s) = m.lit {
                            let ident: Ident = syn::parse_str(&s.value()).unwrap();
                            out = quote!{#ident};
                        }
                    }
                    _ => ()
                }
            }
        }
        out
    };

    // The `ProtocolsHandler` associated type.
    let protocols_handler_ty = {
        let mut ph_ty = None;
        for field in data_struct.fields.iter() {
            if is_ignored(&field) {
                continue;
            }
            let ty = &field.ty;
            let field_info = quote!{ <#ty as #trait_to_impl<#topology_generic>>::ProtocolsHandler };
            match ph_ty {
                Some(ev) => ph_ty = Some(quote!{ #into_proto_select_ident<#ev, #field_info> }),
                ref mut ev @ None => *ev = Some(field_info),
            }
        }
        ph_ty.unwrap_or(quote!{()})     // TODO: `!` instead
    };

    // The content of `new_handler()`.
    // Example output: `self.field1.select(self.field2.select(self.field3))`.
    let new_handler = {
        let mut out_handler = None;

        for (field_n, field) in data_struct.fields.iter().enumerate() {
            if is_ignored(&field) {
                continue;
            }

            let field_name = match field.ident {
                Some(ref i) => quote!{ self.#i },
                None => quote!{ self.#field_n },
            };

            let builder = quote! {
                #field_name.new_handler()
            };

            match out_handler {
                Some(h) => out_handler = Some(quote!{ #h.select(#builder) }),
                ref mut h @ None => *h = Some(builder),
            }
        }

        out_handler.unwrap_or(quote!{()})     // TODO: incorrect
    };

    // The method to use to poll.
    // If we find a `#[behaviour(poll_method = "poll")]` attribute on the struct, we call
    // `self.poll()` at the end of the polling.
    let poll_method = {
        let mut poll_method = quote!{Async::NotReady};
        for meta_items in ast.attrs.iter().filter_map(get_meta_items) {
            for meta_item in meta_items {
                match meta_item {
                    syn::NestedMeta::Meta(syn::Meta::NameValue(ref m)) if m.ident == "poll_method" => {
                        if let syn::Lit::Str(ref s) = m.lit {
                            let ident: Ident = syn::parse_str(&s.value()).unwrap();
                            poll_method = quote!{#name::#ident(self)};
                        }
                    }
                    _ => ()
                }
            }
        }
        poll_method
    };

    // Build the list of statements for when we have a `None` event.
    let none_stmts = data_struct.fields.iter().enumerate().filter(|f| !is_ignored(&f.1)).enumerate().map(|(enum_n, (field_n, field))| {
        if is_ignored(&field) {
            return None;
        }

        Some(match field.ident {
            Some(ref i) => {
                let closure = gen_field_unwrap(field_n, enum_n, data_struct);
                let ev = quote!{ self.#i.poll(#swarm_event::None, &mut poll_params.with_event_map(#closure)) };
                gen_field_wrap(ev, field_n, enum_n, data_struct)
            },
            None => {
                let closure = gen_field_unwrap(field_n, enum_n, data_struct);
                let ev = quote!{ self.#field_n.poll(#swarm_event::None, &mut poll_params.with_event_map(#closure)) };
                gen_field_wrap(ev, field_n, enum_n, data_struct)
            },
        })
    });

    // Build the list of statements for when we have a `Connected` event.
    let connected_stmts = data_struct.fields.iter().enumerate().filter(|f| !is_ignored(&f.1)).enumerate().map(|(enum_n, (field_n, field))| {
        if is_ignored(&field) {
            return None;
        }

        Some(match field.ident {
            Some(ref i) => {
                let closure = gen_field_unwrap(field_n, enum_n, data_struct);
                let ev = quote!{ self.#i.poll(#swarm_event::Connected { peer_id, endpoint }, &mut poll_params.with_event_map(#closure)) };
                gen_field_wrap(ev, field_n, enum_n, data_struct)
            }
            None => {
                let closure = gen_field_unwrap(field_n, enum_n, data_struct);
                let ev = quote!{ self.#field_n.poll(#swarm_event::Connected { peer_id, endpoint }, &mut poll_params.with_event_map(#closure)) };
                gen_field_wrap(ev, field_n, enum_n, data_struct)
            }
        })
    });

    // Build the list of statements for when we have a `Disconnected` event.
    let disconnected_stmts = data_struct.fields.iter().enumerate().filter(|f| !is_ignored(&f.1)).enumerate().map(|(enum_n, (field_n, field))| {
        if is_ignored(&field) {
            return None;
        }

        Some(match field.ident {
            Some(ref i) => {
                let closure = gen_field_unwrap(field_n, enum_n, data_struct);
                let ev = quote!{ self.#i.poll(#swarm_event::Disconnected { peer_id, endpoint }, &mut poll_params.with_event_map(#closure)) };
                gen_field_wrap(ev, field_n, enum_n, data_struct)
            }
            None => {
                let closure = gen_field_unwrap(field_n, enum_n, data_struct);
                let ev = quote!{ self.#field_n.poll(#swarm_event::Disconnected { peer_id, endpoint }, &mut poll_params.with_event_map(#closure)) };
                gen_field_wrap(ev, field_n, enum_n, data_struct)
            }
        })
    });

    // Build the list of variants to put in the body of `inject_node_event()`.
    //
    // The event type is a construction of nested `#either_ident`s of the events of the children.
    // We call `inject_node_event` on the corresponding child.
    let events_stmts = data_struct.fields.iter().enumerate().filter(|f| !is_ignored(&f.1)).enumerate().map(|(enum_n, (field_n, field))| {
        let mut elem = if enum_n != 0 {
            quote!{ #either_ident::Second(ev) }
        } else {
            quote!{ ev }
        };

        for _ in 0 .. data_struct.fields.iter().filter(|f| !is_ignored(f)).count() - 1 - field_n {
            elem = quote!{ #either_ident::First(#elem) };
        }

        let elem = quote!{ #swarm_event::ProtocolsHandlerEvent { peer_id, event: #elem } };

        Some(match field.ident {
            Some(ref i) => {
                let closure = gen_field_unwrap(field_n, enum_n, data_struct);
                let ev = quote!{ self.#i.poll(#swarm_event::ProtocolsHandlerEvent { peer_id, event: ev }, &mut poll_params.with_event_map(#closure)) };
                let stmt = gen_field_wrap(ev, field_n, enum_n, data_struct);
                quote!{ #elem => { #stmt; } }
            },
            None => {
                let closure = gen_field_unwrap(field_n, enum_n, data_struct);
                let ev = quote!{ self.#field_n.poll(#swarm_event::ProtocolsHandlerEvent { peer_id, event: ev }, &mut poll_params.with_event_map(#closure)) };
                let stmt = gen_field_wrap(ev, field_n, enum_n, data_struct);
                quote!{ #elem => { #stmt; } }
            },
        })
    });

    // Now the magic happens.
    let final_quote = quote!{
        impl #impl_generics #trait_to_impl<#topology_generic> for #name #ty_generics
        #where_clause
        {
            type ProtocolsHandler = #protocols_handler_ty;
            type OutEvent = #out_event;

            #[inline]
            fn new_handler(&mut self) -> Self::ProtocolsHandler {
                use #into_protocols_handler;
                #new_handler
            }

            fn poll(&mut self, event: #swarm_event<<<Self::ProtocolsHandler as #into_protocols_handler>::Handler as #protocols_handler>::OutEvent>, poll_params: &mut #poll_parameters) -> ::libp2p::futures::Async<#network_behaviour_action<<<Self::ProtocolsHandler as #into_protocols_handler>::Handler as #protocols_handler>::InEvent, Self::OutEvent>> {
                use libp2p::futures::prelude::*;

                let mut event = Some(event);
                loop {
                    let mut none_ready = true;
                    let poll_result = match event.take().unwrap_or(#swarm_event::None) {
                        #swarm_event::None => {
                            #(#none_stmts)*
                        },
                        #swarm_event::Connected { peer_id, endpoint } => {
                            #(#connected_stmts)*
                        },
                        #swarm_event::Disconnected { peer_id, endpoint } => {
                            #(#disconnected_stmts)*
                        },
                        #(#events_stmts),*
                    };
                    if none_ready {
                        break;
                    }
                }
                let f: ::libp2p::futures::Async<#network_behaviour_action<<<Self::ProtocolsHandler as #into_protocols_handler>::Handler as #protocols_handler>::InEvent, Self::OutEvent>> = #poll_method;
                f
            }
        }
    };

    final_quote.into()
}

fn get_meta_items(attr: &syn::Attribute) -> Option<Vec<syn::NestedMeta>> {
    if attr.path.segments.len() == 1 && attr.path.segments[0].ident == "behaviour" {
        match attr.interpret_meta() {
            Some(syn::Meta::List(ref meta)) => Some(meta.nested.iter().cloned().collect()),
            _ => {
                None
            }
        }
    } else {
        None
    }
}

/// Generates a `match` block that converts between the output of `poll()` of a sub-field, and
/// the output of the `poll()` of the type we're deriving.
fn gen_field_wrap(
    event: syn::export::TokenStream2,
    field_n: usize,
    enum_n: usize,
    data_struct: &syn::DataStruct,
) -> syn::export::TokenStream2 {
    let net_behv_event_proc = quote!{::libp2p::core::swarm::NetworkBehaviourEventProcess};
    let either_ident = quote!{::libp2p::core::either::EitherOutput};
    let network_behaviour_action = quote!{::libp2p::core::swarm::NetworkBehaviourAction};

    let mut wrapped_event = if enum_n != 0 {
        quote!{ #either_ident::Second(event) }
    } else {
        quote!{ event }
    };
    for _ in 0 .. data_struct.fields.iter().filter(|f| !is_ignored(f)).count() - 1 - field_n {
        wrapped_event = quote!{ #either_ident::First(#wrapped_event) };
    }

    // TODO: wrong, because we shouldn't return from here; we're in the middle of an event process
    // and it hasn't been dispatched to all the fields
    quote!{
        match #event {
            Async::Ready(#network_behaviour_action::GenerateEvent(event)) => {
                #net_behv_event_proc::inject_event(self, event)
            }
            Async::Ready(#network_behaviour_action::DialAddress { address }) => {
                return Async::Ready(#network_behaviour_action::DialAddress { address });
            }
            Async::Ready(#network_behaviour_action::DialPeer { peer_id }) => {
                return Async::Ready(#network_behaviour_action::DialPeer { peer_id });
            }
            Async::Ready(#network_behaviour_action::SendEvent { peer_id, event }) => {
                return Async::Ready(#network_behaviour_action::SendEvent {
                    peer_id,
                    event: #wrapped_event,
                });
            }
            Async::NotReady => (),
        }
    }
}

/// Generates a closure block that converts from the combined `InEvent` of the type we're deriving
/// to the `InEvent` of a sub-field.
fn gen_field_unwrap(
    field_n: usize,
    enum_n: usize,
    data_struct: &syn::DataStruct,
) -> syn::export::TokenStream2 {
    let either_ident = quote!{::libp2p::core::either::EitherOutput};

    let mut wrapped_event = if enum_n != 0 {
        quote!{ #either_ident::Second(event) }
    } else {
        quote!{ event }
    };
    for _ in 0 .. data_struct.fields.iter().filter(|f| !is_ignored(f)).count() - 1 - field_n {
        wrapped_event = quote!{ #either_ident::First(#wrapped_event) };
    }

    quote!{|event| #wrapped_event}
}

/// Returns true if a field is marked as ignored by the user.
fn is_ignored(field: &syn::Field) -> bool {
    for meta_items in field.attrs.iter().filter_map(get_meta_items) {
        for meta_item in meta_items {
            match meta_item {
                syn::NestedMeta::Meta(syn::Meta::Word(ref m)) if m == "ignore" => {
                    return true;
                }
                _ => ()
            }
        }
    }

    false
}
