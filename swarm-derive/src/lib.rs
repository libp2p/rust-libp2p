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

use heck::ToUpperCamelCase;
use proc_macro::TokenStream;
use quote::quote;
use syn::parse::Parse;
use syn::{parse_macro_input, Data, DataStruct, DeriveInput};

/// Generates a delegating `NetworkBehaviour` implementation for the struct this is used for. See
/// the trait documentation for better description.
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
    let prelude_path = parse_attribute_value_by_key::<syn::Path>(ast, "prelude")
        .unwrap_or_else(|| syn::parse_quote! { ::libp2p::swarm::derive_prelude });

    let multiaddr = quote! { #prelude_path::Multiaddr };
    let trait_to_impl = quote! { #prelude_path::NetworkBehaviour };
    let either_ident = quote! { #prelude_path::Either };
    let network_behaviour_action = quote! { #prelude_path::NetworkBehaviourAction };
    let into_connection_handler = quote! { #prelude_path::IntoConnectionHandler };
    let connection_handler = quote! { #prelude_path::ConnectionHandler };
    let into_proto_select_ident = quote! { #prelude_path::IntoConnectionHandlerSelect };
    let peer_id = quote! { #prelude_path::PeerId };
    let connection_id = quote! { #prelude_path::ConnectionId };
    let poll_parameters = quote! { #prelude_path::PollParameters };
    let from_swarm = quote! { #prelude_path::FromSwarm };
    let connection_established = quote! { #prelude_path::ConnectionEstablished };
    let address_change = quote! { #prelude_path::AddressChange };
    let connection_closed = quote! { #prelude_path::ConnectionClosed };
    let dial_failure = quote! { #prelude_path::DialFailure };
    let listen_failure = quote! { #prelude_path::ListenFailure };
    let new_listener = quote! { #prelude_path::NewListener };
    let new_listen_addr = quote! { #prelude_path::NewListenAddr };
    let expired_listen_addr = quote! { #prelude_path::ExpiredListenAddr };
    let new_external_addr = quote! { #prelude_path::NewExternalAddr };
    let expired_external_addr = quote! { #prelude_path::ExpiredExternalAddr };
    let listener_error = quote! { #prelude_path::ListenerError };
    let listener_closed = quote! { #prelude_path::ListenerClosed };

    // Build the generics.
    let impl_generics = {
        let tp = ast.generics.type_params();
        let lf = ast.generics.lifetimes();
        let cst = ast.generics.const_params();
        quote! {<#(#lf,)* #(#tp,)* #(#cst,)*>}
    };

    let (out_event_name, out_event_definition, out_event_from_clauses) = {
        // If we find a `#[behaviour(out_event = "Foo")]` attribute on the
        // struct, we set `Foo` as the out event. If not, the `OutEvent` is
        // generated.
        let user_provided_out_event_name =
            parse_attribute_value_by_key::<syn::Type>(ast, "out_event");

        match user_provided_out_event_name {
            // User provided `OutEvent`.
            Some(name) => {
                let definition = None;
                let from_clauses = data_struct
                    .fields
                    .iter()
                    .map(|field| {
                        let ty = &field.ty;
                        quote! {#name: From< <#ty as #trait_to_impl>::OutEvent >}
                    })
                    .collect::<Vec<_>>();
                (name, definition, from_clauses)
            }
            // User did not provide `OutEvent`. Generate it.
            None => {
                let name: syn::Type = syn::parse_str(&(ast.ident.to_string() + "Event")).unwrap();
                let definition = {
                    let fields = data_struct
                        .fields
                        .iter()
                        .map(|field| {
                            let variant: syn::Variant = syn::parse_str(
                                &field
                                    .ident
                                    .clone()
                                    .expect(
                                        "Fields of NetworkBehaviour implementation to be named.",
                                    )
                                    .to_string()
                                    .to_upper_camel_case(),
                            )
                            .unwrap();
                            let ty = &field.ty;
                            quote! {#variant(<#ty as #trait_to_impl>::OutEvent)}
                        })
                        .collect::<Vec<_>>();
                    let visibility = &ast.vis;

                    Some(quote! {
                        #[derive(::std::fmt::Debug)]
                        #visibility enum #name #impl_generics
                            #where_clause
                        {
                            #(#fields),*
                        }
                    })
                };
                let from_clauses = vec![];
                (name, definition, from_clauses)
            }
        }
    };

    // Build the `where ...` clause of the trait implementation.
    let where_clause = {
        let additional = data_struct
            .fields
            .iter()
            .map(|field| {
                let ty = &field.ty;
                quote! {#ty: #trait_to_impl}
            })
            .chain(out_event_from_clauses)
            .collect::<Vec<_>>();

        if let Some(where_clause) = where_clause {
            if where_clause.predicates.trailing_punct() {
                Some(quote! {#where_clause #(#additional),* })
            } else {
                Some(quote! {#where_clause, #(#additional),*})
            }
        } else {
            Some(quote! {where #(#additional),*})
        }
    };

    // Build the list of statements to put in the body of `addresses_of_peer()`.
    let addresses_of_peer_stmts = {
        data_struct
            .fields
            .iter()
            .enumerate()
            .map(move |(field_n, field)| match field.ident {
                Some(ref i) => quote! { out.extend(self.#i.addresses_of_peer(peer_id)); },
                None => quote! { out.extend(self.#field_n.addresses_of_peer(peer_id)); },
            })
    };

    // Build the list of statements to put in the body of `on_swarm_event()`
    // for the `FromSwarm::ConnectionEstablished` variant.
    let on_connection_established_stmts = {
        data_struct
            .fields
            .iter()
            .enumerate()
            .map(|(field_n, field)| match field.ident {
                Some(ref i) => quote! {
                    self.#i.on_swarm_event(#from_swarm::ConnectionEstablished(#connection_established {
                        peer_id,
                        connection_id,
                        endpoint,
                        failed_addresses,
                        other_established,
                    }));
                },
                None => quote! {
                    self.#field_n.on_swarm_event(#from_swarm::ConnectionEstablished(#connection_established {
                        peer_id,
                        connection_id,
                        endpoint,
                        failed_addresses,
                        other_established,
                    }));
                },
            })
    };

    // Build the list of statements to put in the body of `on_swarm_event()`
    // for the `FromSwarm::AddressChange variant`.
    let on_address_change_stmts = {
        data_struct
            .fields
            .iter()
            .enumerate()
            .map(|(field_n, field)| match field.ident {
                Some(ref i) => quote! {
                self.#i.on_swarm_event(#from_swarm::AddressChange(#address_change {
                        peer_id,
                        connection_id,
                        old,
                        new,
                    }));
                },
                None => quote! {
                self.#field_n.on_swarm_event(#from_swarm::AddressChange(#address_change {
                        peer_id,
                        connection_id,
                        old,
                        new,
                    }));
                },
            })
    };

    // Build the list of statements to put in the body of `on_swarm_event()`
    // for the `FromSwarm::ConnectionClosed` variant.
    let on_connection_closed_stmts = {
        data_struct
            .fields
            .iter()
            .enumerate()
            // The outmost handler belongs to the last behaviour.
            .rev()
            .enumerate()
            .map(|(enum_n, (field_n, field))| {
                let handler = if field_n == 0 {
                    // Given that the iterator is reversed, this is the innermost handler only.
                    quote! { let handler = handlers }
                } else {
                    quote! {
                        let (handlers, handler) = handlers.into_inner()
                    }
                };
                let inject = match field.ident {
                    Some(ref i) => quote! {
                    self.#i.on_swarm_event(#from_swarm::ConnectionClosed(#connection_closed {
                            peer_id,
                            connection_id,
                            endpoint,
                            handler,
                            remaining_established,
                        }));
                    },
                    None => quote! {
                    self.#enum_n.on_swarm_event(#from_swarm::ConnectionClosed(#connection_closed {
                            peer_id,
                            connection_id,
                            endpoint,
                            handler,
                            remaining_established,
                        }));
                    },
                };

                quote! {
                    #handler;
                    #inject;
                }
            })
    };

    // Build the list of statements to put in the body of `on_swarm_event()`
    // for the `FromSwarm::DialFailure` variant.
    let on_dial_failure_stmts = {
        data_struct
            .fields
            .iter()
            .enumerate()
            // The outmost handler belongs to the last behaviour.
            .rev()
            .enumerate()
            .map(|(enum_n, (field_n, field))| {
                let handler = if field_n == 0 {
                    // Given that the iterator is reversed, this is the innermost handler only.
                    quote! { let handler = handlers }
                } else {
                    quote! {
                        let (handlers, handler) = handlers.into_inner()
                    }
                };

                let inject = match field.ident {
                    Some(ref i) => quote! {
                    self.#i.on_swarm_event(#from_swarm::DialFailure(#dial_failure {
                            peer_id,
                            handler,
                            error,
                        }));
                    },
                    None => quote! {
                    self.#enum_n.on_swarm_event(#from_swarm::DialFailure(#dial_failure {
                            peer_id,
                            handler,
                            error,
                        }));
                    },
                };
                quote! {
                    #handler;
                    #inject;
                }
            })
    };

    // Build the list of statements to put in the body of `on_swarm_event()`
    // for the `FromSwarm::ListenFailure` variant.
    let on_listen_failure_stmts =
        {
            data_struct.fields.iter().enumerate().rev().enumerate().map(
                |(enum_n, (field_n, field))| {
                    let handler = if field_n == 0 {
                        quote! { let handler = handlers }
                    } else {
                        quote! {
                            let (handlers, handler) = handlers.into_inner()
                        }
                    };

                    let inject = match field.ident {
                        Some(ref i) => quote! {
                        self.#i.on_swarm_event(#from_swarm::ListenFailure(#listen_failure {
                                local_addr,
                                send_back_addr,
                                handler,
                            }));
                        },
                        None => quote! {
                        self.#enum_n.on_swarm_event(#from_swarm::ListenFailure(#listen_failure {
                                local_addr,
                                send_back_addr,
                                handler,
                            }));
                        },
                    };

                    quote! {
                        #handler;
                        #inject;
                    }
                },
            )
        };

    // Build the list of statements to put in the body of `on_swarm_event()`
    // for the `FromSwarm::NewListener` variant.
    let on_new_listener_stmts = {
        data_struct
            .fields
            .iter()
            .enumerate()
            .map(|(field_n, field)| match field.ident {
                Some(ref i) => quote! {
                self.#i.on_swarm_event(#from_swarm::NewListener(#new_listener {
                        listener_id,
                    }));
                },
                None => quote! {
                self.#field_n.on_swarm_event(#from_swarm::NewListener(#new_listener {
                        listener_id,
                    }));
                },
            })
    };

    // Build the list of statements to put in the body of `on_swarm_event()`
    // for the `FromSwarm::NewListenAddr` variant.
    let on_new_listen_addr_stmts = {
        data_struct
            .fields
            .iter()
            .enumerate()
            .map(|(field_n, field)| match field.ident {
                Some(ref i) => quote! {
                self.#i.on_swarm_event(#from_swarm::NewListenAddr(#new_listen_addr {
                        listener_id,
                        addr,
                    }));
                },
                None => quote! {
                self.#field_n.on_swarm_event(#from_swarm::NewListenAddr(#new_listen_addr {
                        listener_id,
                        addr,
                    }));
                },
            })
    };

    // Build the list of statements to put in the body of `on_swarm_event()`
    // for the `FromSwarm::ExpiredListenAddr` variant.
    let on_expired_listen_addr_stmts = {
        data_struct
            .fields
            .iter()
            .enumerate()
            .map(|(field_n, field)| match field.ident {
                Some(ref i) => quote! {
                self.#i.on_swarm_event(#from_swarm::ExpiredListenAddr(#expired_listen_addr {
                        listener_id,
                        addr,
                    }));
                },
                None => quote! {
                self.#field_n.on_swarm_event(#from_swarm::ExpiredListenAddr(#expired_listen_addr {
                        listener_id,
                        addr,
                    }));
                },
            })
    };

    // Build the list of statements to put in the body of `on_swarm_event()`
    // for the `FromSwarm::NewExternalAddr` variant.
    let on_new_external_addr_stmts = {
        data_struct
            .fields
            .iter()
            .enumerate()
            .map(|(field_n, field)| match field.ident {
                Some(ref i) => quote! {
                self.#i.on_swarm_event(#from_swarm::NewExternalAddr(#new_external_addr {
                        addr,
                    }));
                },
                None => quote! {
                self.#field_n.on_swarm_event(#from_swarm::NewExternalAddr(#new_external_addr {
                        addr,
                    }));
                },
            })
    };

    // Build the list of statements to put in the body of `on_swarm_event()`
    // for the `FromSwarm::ExpiredExternalAddr` variant.
    let on_expired_external_addr_stmts = {
        data_struct
            .fields
            .iter()
            .enumerate()
            .map(|(field_n, field)| match field.ident {
                Some(ref i) => quote! {
                self.#i.on_swarm_event(#from_swarm::ExpiredExternalAddr(#expired_external_addr {
                        addr,
                    }));
                },
                None => quote! {
                self.#field_n.on_swarm_event(#from_swarm::ExpiredExternalAddr(#expired_external_addr {
                        addr,
                    }));
                },
            })
    };

    // Build the list of statements to put in the body of `on_swarm_event()`
    // for the `FromSwarm::ListenerError` variant.
    let on_listener_error_stmts = {
        data_struct
            .fields
            .iter()
            .enumerate()
            .map(|(field_n, field)| match field.ident {
                Some(ref i) => quote! {
                    self.#i.on_swarm_event(#from_swarm::ListenerError(#listener_error {
                        listener_id,
                        err,
                    }));
                },
                None => quote! {
                    self.#field_n.on_swarm_event(#from_swarm::ListenerError(#listener_error {
                        listener_id,
                        err,
                    }));
                },
            })
    };

    // Build the list of statements to put in the body of `on_swarm_event()`
    // for the `FromSwarm::ListenerClosed` variant.
    let on_listener_closed_stmts = {
        data_struct
            .fields
            .iter()
            .enumerate()
            .map(|(field_n, field)| match field.ident {
                Some(ref i) => quote! {
                self.#i.on_swarm_event(#from_swarm::ListenerClosed(#listener_closed {
                        listener_id,
                        reason,
                    }));
                },
                None => quote! {
                self.#field_n.on_swarm_event(#from_swarm::ListenerClosed(#listener_closed {
                        listener_id,
                        reason,
                    }));
                },
            })
    };

    // Build the list of variants to put in the body of `on_connection_handler_event()`.
    //
    // The event type is a construction of nested `#either_ident`s of the events of the children.
    // We call `on_connection_handler_event` on the corresponding child.
    let on_node_event_stmts =
        data_struct
            .fields
            .iter()
            .enumerate()
            .enumerate()
            .map(|(enum_n, (field_n, field))| {
                let mut elem = if enum_n != 0 {
                    quote! { #either_ident::Right(ev) }
                } else {
                    quote! { ev }
                };

                for _ in 0..data_struct.fields.len() - 1 - enum_n {
                    elem = quote! { #either_ident::Left(#elem) };
                }

                Some(match field.ident {
                    Some(ref i) => quote! { #elem => {
                    #trait_to_impl::on_connection_handler_event(&mut self.#i, peer_id, connection_id, ev) }},
                    None => quote! { #elem => {
                    #trait_to_impl::on_connection_handler_event(&mut self.#field_n, peer_id, connection_id, ev) }},
                })
            });

    // The [`ConnectionHandler`] associated type.
    let connection_handler_ty = {
        let mut ph_ty = None;
        for field in data_struct.fields.iter() {
            let ty = &field.ty;
            let field_info = quote! { <#ty as #trait_to_impl>::ConnectionHandler };
            match ph_ty {
                Some(ev) => ph_ty = Some(quote! { #into_proto_select_ident<#ev, #field_info> }),
                ref mut ev @ None => *ev = Some(field_info),
            }
        }
        // ph_ty = Some(quote! )
        ph_ty.unwrap_or(quote! {()}) // TODO: `!` instead
    };

    // The content of `new_handler()`.
    // Example output: `self.field1.select(self.field2.select(self.field3))`.
    let new_handler = {
        let mut out_handler = None;

        for (field_n, field) in data_struct.fields.iter().enumerate() {
            let field_name = match field.ident {
                Some(ref i) => quote! { self.#i },
                None => quote! { self.#field_n },
            };

            let builder = quote! {
                #field_name.new_handler()
            };

            match out_handler {
                Some(h) => {
                    out_handler = Some(quote! { #into_connection_handler::select(#h, #builder) })
                }
                ref mut h @ None => *h = Some(builder),
            }
        }

        out_handler.unwrap_or(quote! {()}) // TODO: See test `empty`.
    };

    // List of statements to put in `poll()`.
    //
    // We poll each child one by one and wrap around the output.
    let poll_stmts = data_struct.fields.iter().enumerate().map(|(field_n, field)| {
        let field = field
            .ident
            .clone()
            .expect("Fields of NetworkBehaviour implementation to be named.");

        let mut wrapped_event = if field_n != 0 {
            quote!{ #either_ident::Right(event) }
        } else {
            quote!{ event }
        };
        for _ in 0 .. data_struct.fields.len() - 1 - field_n {
            wrapped_event = quote!{ #either_ident::Left(#wrapped_event) };
        }

        // `Dial` provides a handler of the specific behaviour triggering the
        // event. Though in order for the final handler to be able to handle
        // protocols of all behaviours, the provided handler needs to be
        // combined with handlers of all other behaviours.
        let provided_handler_and_new_handlers = {
            let mut out_handler = None;

            for (f_n, f) in data_struct.fields.iter().enumerate() {
                let f_name = match f.ident {
                    Some(ref i) => quote! { self.#i },
                    None => quote! { self.#f_n },
                };

                let builder = if field_n == f_n {
                    // The behaviour that triggered the event. Thus, instead of
                    // creating a new handler, use the provided handler.
                    quote! { provided_handler }
                } else {
                    quote! { #f_name.new_handler() }
                };

                match out_handler {
                    Some(h) => {
                        out_handler = Some(quote! { #into_connection_handler::select(#h, #builder) })
                    }
                    ref mut h @ None => *h = Some(builder),
                }
            }

            out_handler.unwrap_or(quote! {()}) // TODO: See test `empty`.
        };

        let generate_event_match_arm =  {
            // If the `NetworkBehaviour`'s `OutEvent` is generated by the derive macro, wrap the sub
            // `NetworkBehaviour` `OutEvent` in the variant of the generated `OutEvent`. If the
            // `NetworkBehaviour`'s `OutEvent` is provided by the user, use the corresponding `From`
            // implementation.
            let into_out_event = if out_event_definition.is_some() {
                let event_variant: syn::Variant = syn::parse_str(
                    &field
                        .to_string()
                        .to_upper_camel_case()
                ).unwrap();
                quote! { #out_event_name::#event_variant(event) }
            } else {
                quote! { event.into() }
            };

            quote! {
                std::task::Poll::Ready(#network_behaviour_action::GenerateEvent(event)) => {
                    return std::task::Poll::Ready(#network_behaviour_action::GenerateEvent(#into_out_event))
                }
            }
        };

        Some(quote!{
            loop {
                match #trait_to_impl::poll(&mut self.#field, cx, poll_params) {
                    #generate_event_match_arm
                    std::task::Poll::Ready(#network_behaviour_action::Dial { opts, handler: provided_handler }) => {
                        return std::task::Poll::Ready(#network_behaviour_action::Dial { opts, handler: #provided_handler_and_new_handlers });
                    }
                    std::task::Poll::Ready(#network_behaviour_action::NotifyHandler { peer_id, handler, event }) => {
                        return std::task::Poll::Ready(#network_behaviour_action::NotifyHandler {
                            peer_id,
                            handler,
                            event: #wrapped_event,
                        });
                    }
                    std::task::Poll::Ready(#network_behaviour_action::ReportObservedAddr { address, score }) => {
                        return std::task::Poll::Ready(#network_behaviour_action::ReportObservedAddr { address, score });
                    }
                    std::task::Poll::Ready(#network_behaviour_action::CloseConnection { peer_id, connection }) => {
                        return std::task::Poll::Ready(#network_behaviour_action::CloseConnection { peer_id, connection });
                    }
                    std::task::Poll::Pending => break,
                }
            }
        })
    });

    let out_event_reference = if out_event_definition.is_some() {
        quote! { #out_event_name #ty_generics }
    } else {
        quote! { #out_event_name }
    };

    // Now the magic happens.
    let final_quote = quote! {
        #out_event_definition

        impl #impl_generics #trait_to_impl for #name #ty_generics
        #where_clause
        {
            type ConnectionHandler = #connection_handler_ty;
            type OutEvent = #out_event_reference;

            fn new_handler(&mut self) -> Self::ConnectionHandler {
                use #into_connection_handler;
                #new_handler
            }

            fn addresses_of_peer(&mut self, peer_id: &#peer_id) -> Vec<#multiaddr> {
                let mut out = Vec::new();
                #(#addresses_of_peer_stmts);*
                out
            }

            fn on_connection_handler_event(
                &mut self,
                peer_id: #peer_id,
                connection_id: #connection_id,
                event: <<Self::ConnectionHandler as #into_connection_handler>::Handler as #connection_handler>::OutEvent
            ) {
                match event {
                    #(#on_node_event_stmts),*
                }
            }

            fn poll(&mut self, cx: &mut std::task::Context, poll_params: &mut impl #poll_parameters) -> std::task::Poll<#network_behaviour_action<Self::OutEvent, Self::ConnectionHandler>> {
                use #prelude_path::futures::*;
                #(#poll_stmts)*
                std::task::Poll::Pending
            }

            fn on_swarm_event(&mut self, event: #from_swarm<Self::ConnectionHandler>) {
                match event {
                    #from_swarm::ConnectionEstablished(
                        #connection_established { peer_id, connection_id, endpoint, failed_addresses, other_established })
                    => { #(#on_connection_established_stmts)* }
                    #from_swarm::AddressChange(
                        #address_change { peer_id, connection_id, old, new })
                    => { #(#on_address_change_stmts)* }
                    #from_swarm::ConnectionClosed(
                        #connection_closed { peer_id, connection_id, endpoint, handler: handlers, remaining_established })
                    => { #(#on_connection_closed_stmts)* }
                    #from_swarm::DialFailure(
                        #dial_failure { peer_id, handler: handlers, error })
                    => { #(#on_dial_failure_stmts)* }
                    #from_swarm::ListenFailure(
                        #listen_failure { local_addr, send_back_addr, handler: handlers })
                    => { #(#on_listen_failure_stmts)* }
                    #from_swarm::NewListener(
                        #new_listener { listener_id })
                    => { #(#on_new_listener_stmts)* }
                    #from_swarm::NewListenAddr(
                        #new_listen_addr { listener_id, addr })
                    => { #(#on_new_listen_addr_stmts)* }
                    #from_swarm::ExpiredListenAddr(
                        #expired_listen_addr { listener_id, addr })
                    => { #(#on_expired_listen_addr_stmts)* }
                    #from_swarm::NewExternalAddr(
                        #new_external_addr { addr })
                    => { #(#on_new_external_addr_stmts)* }
                    #from_swarm::ExpiredExternalAddr(
                        #expired_external_addr { addr })
                    => { #(#on_expired_external_addr_stmts)* }
                    #from_swarm::ListenerError(
                        #listener_error { listener_id, err })
                    => { #(#on_listener_error_stmts)* }
                    #from_swarm::ListenerClosed(
                        #listener_closed { listener_id, reason })
                    => { #(#on_listener_closed_stmts)* }
                    _ => {}
                }
            }
        }
    };

    final_quote.into()
}

/// Parses the `value` of a key=value pair in the `#[behaviour]` attribute into the requested type.
///
/// Only `String` values are supported, e.g. `#[behaviour(foo="bar")]`.
fn parse_attribute_value_by_key<T>(ast: &DeriveInput, key: &str) -> Option<T>
where
    T: Parse,
{
    ast.attrs
        .iter()
        .filter_map(get_meta_items)
        .flatten()
        .filter_map(|meta_item| {
            if let syn::NestedMeta::Meta(syn::Meta::NameValue(ref m)) = meta_item {
                if m.path.is_ident(key) {
                    if let syn::Lit::Str(ref s) = m.lit {
                        return Some(syn::parse_str(&s.value()).unwrap());
                    }
                }
            }
            None
        })
        .next()
}

fn get_meta_items(attr: &syn::Attribute) -> Option<Vec<syn::NestedMeta>> {
    if attr.path.segments.len() == 1 && attr.path.segments[0].ident == "behaviour" {
        match attr.parse_meta() {
            Ok(syn::Meta::List(ref meta)) => Some(meta.nested.iter().cloned().collect()),
            Ok(_) => None,
            Err(e) => {
                eprintln!("error parsing attribute metadata: {e}");
                None
            }
        }
    } else {
        None
    }
}
