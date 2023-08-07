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
use quote::quote;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
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

/// The version for structs
fn build_struct(ast: &DeriveInput, data_struct: &DataStruct) -> syn::Result<TokenStream> {
    let name = &ast.ident;
    let (_, ty_generics, where_clause) = ast.generics.split_for_impl();
    let BehaviourAttributes {
        prelude_path,
        user_specified_out_event,
        deprecation_tokenstream,
    } = parse_attributes(ast)?;

    let multiaddr = quote! { #prelude_path::Multiaddr };
    let trait_to_impl = quote! { #prelude_path::NetworkBehaviour };
    let either_ident = quote! { #prelude_path::Either };
    let network_behaviour_action = quote! { #prelude_path::ToSwarm };
    let connection_handler = quote! { #prelude_path::ConnectionHandler };
    let proto_select_ident = quote! { #prelude_path::ConnectionHandlerSelect };
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
    let new_external_addr_candidate = quote! { #prelude_path::NewExternalAddrCandidate };
    let external_addr_expired = quote! { #prelude_path::ExternalAddrExpired };
    let external_addr_confirmed = quote! { #prelude_path::ExternalAddrConfirmed };
    let listener_error = quote! { #prelude_path::ListenerError };
    let listener_closed = quote! { #prelude_path::ListenerClosed };
    let t_handler = quote! { #prelude_path::THandler };
    let t_handler_in_event = quote! { #prelude_path::THandlerInEvent };
    let t_handler_out_event = quote! { #prelude_path::THandlerOutEvent };
    let endpoint = quote! { #prelude_path::Endpoint };
    let connection_denied = quote! { #prelude_path::ConnectionDenied };

    // Build the generics.
    let impl_generics = {
        let tp = ast.generics.type_params();
        let lf = ast.generics.lifetimes();
        let cst = ast.generics.const_params();
        quote! {<#(#lf,)* #(#tp,)* #(#cst,)*>}
    };

    let (out_event_name, out_event_definition, out_event_from_clauses) = {
        // If we find a `#[behaviour(to_swarm = "Foo")]` attribute on the
        // struct, we set `Foo` as the out event. If not, the `ToSwarm` is
        // generated.
        match user_specified_out_event {
            // User provided `ToSwarm`.
            Some(name) => {
                let definition = None;
                let from_clauses = data_struct
                    .fields
                    .iter()
                    .map(|field| {
                        let ty = &field.ty;
                        quote! {#name: From< <#ty as #trait_to_impl>::ToSwarm >}
                    })
                    .collect::<Vec<_>>();
                (name, definition, from_clauses)
            }
            // User did not provide `ToSwarm`. Generate it.
            None => {
                let enum_name_str = ast.ident.to_string() + "Event";
                let enum_name: syn::Type =
                    syn::parse_str(&enum_name_str).expect("ident + `Event` is a valid type");
                let definition = {
                    let fields = data_struct.fields.iter().map(|field| {
                        let variant: syn::Variant = syn::parse_str(
                            &field
                                .ident
                                .clone()
                                .expect("Fields of NetworkBehaviour implementation to be named.")
                                .to_string()
                                .to_upper_camel_case(),
                        )
                        .expect("uppercased field name to be a valid enum variant");
                        let ty = &field.ty;
                        (variant, ty)
                    });

                    let enum_variants = fields
                        .clone()
                        .map(|(variant, ty)| quote! {#variant(<#ty as #trait_to_impl>::ToSwarm)});

                    let visibility = &ast.vis;

                    let additional = fields
                        .clone()
                        .map(|(_variant, tp)| quote! { #tp : #trait_to_impl })
                        .collect::<Vec<_>>();

                    let additional_debug = fields
                        .clone()
                        .map(|(_variant, ty)| quote! { <#ty as #trait_to_impl>::ToSwarm : ::core::fmt::Debug })
                        .collect::<Vec<_>>();

                    let where_clause = {
                        if let Some(where_clause) = where_clause {
                            if where_clause.predicates.trailing_punct() {
                                Some(quote! {#where_clause #(#additional),* })
                            } else {
                                Some(quote! {#where_clause, #(#additional),*})
                            }
                        } else if additional.is_empty() {
                            None
                        } else {
                            Some(quote! {where #(#additional),*})
                        }
                    };

                    let where_clause_debug = where_clause
                        .as_ref()
                        .map(|where_clause| quote! {#where_clause, #(#additional_debug),*});

                    let match_variants = fields.map(|(variant, _ty)| variant);
                    let msg = format!("`NetworkBehaviour::ToSwarm` produced by {name}.");

                    Some(quote! {
                        #[doc = #msg]
                        #visibility enum #enum_name #ty_generics
                            #where_clause
                        {
                            #(#enum_variants),*
                        }

                        impl #impl_generics ::core::fmt::Debug for #enum_name #ty_generics #where_clause_debug {
                            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
                                match &self {
                                    #(#enum_name::#match_variants(event) => {
                                        write!(f, "{}: {:?}", #enum_name_str, event)
                                    }),*
                                }
                            }
                        }
                    })
                };
                let from_clauses = vec![];
                (enum_name, definition, from_clauses)
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
    let on_dial_failure_stmts = data_struct
        .fields
        .iter()
        .enumerate()
        .map(|(enum_n, field)| match field.ident {
            Some(ref i) => quote! {
                self.#i.on_swarm_event(#from_swarm::DialFailure(#dial_failure {
                    peer_id,
                    connection_id,
                    error,
                }));
            },
            None => quote! {
                self.#enum_n.on_swarm_event(#from_swarm::DialFailure(#dial_failure {
                    peer_id,
                    connection_id,
                    error,
                }));
            },
        });

    // Build the list of statements to put in the body of `on_swarm_event()`
    // for the `FromSwarm::ListenFailure` variant.
    let on_listen_failure_stmts = data_struct
        .fields
        .iter()
        .enumerate()
        .map(|(enum_n, field)| match field.ident {
            Some(ref i) => quote! {
                self.#i.on_swarm_event(#from_swarm::ListenFailure(#listen_failure {
                    local_addr,
                    send_back_addr,
                    connection_id,
                    error
                }));
            },
            None => quote! {
                self.#enum_n.on_swarm_event(#from_swarm::ListenFailure(#listen_failure {
                    local_addr,
                    send_back_addr,
                    connection_id,
                    error
                }));
            },
        });

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
    let on_new_external_addr_candidate_stmts = {
        data_struct
            .fields
            .iter()
            .enumerate()
            .map(|(field_n, field)| match field.ident {
                Some(ref i) => quote! {
                self.#i.on_swarm_event(#from_swarm::NewExternalAddrCandidate(#new_external_addr_candidate {
                        addr,
                    }));
                },
                None => quote! {
                self.#field_n.on_swarm_event(#from_swarm::NewExternalAddrCandidate(#new_external_addr_candidate {
                        addr,
                    }));
                },
            })
    };

    // Build the list of statements to put in the body of `on_swarm_event()`
    // for the `FromSwarm::ExternalAddrExpired` variant.
    let on_external_addr_expired_stmts = {
        data_struct
            .fields
            .iter()
            .enumerate()
            .map(|(field_n, field)| match field.ident {
                Some(ref i) => quote! {
                self.#i.on_swarm_event(#from_swarm::ExternalAddrExpired(#external_addr_expired {
                        addr,
                    }));
                },
                None => quote! {
                self.#field_n.on_swarm_event(#from_swarm::ExternalAddrExpired(#external_addr_expired {
                        addr,
                    }));
                },
            })
    };

    // Build the list of statements to put in the body of `on_swarm_event()`
    // for the `FromSwarm::ExternalAddrConfirmed` variant.
    let on_external_addr_confirmed_stmts = {
        data_struct
            .fields
            .iter()
            .enumerate()
            .map(|(field_n, field)| match field.ident {
                Some(ref i) => quote! {
                self.#i.on_swarm_event(#from_swarm::ExternalAddrConfirmed(#external_addr_confirmed {
                        addr,
                    }));
                },
                None => quote! {
                self.#field_n.on_swarm_event(#from_swarm::ExternalAddrConfirmed(#external_addr_confirmed {
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
            let field_info = quote! { #t_handler<#ty> };
            match ph_ty {
                Some(ev) => ph_ty = Some(quote! { #proto_select_ident<#ev, #field_info> }),
                ref mut ev @ None => *ev = Some(field_info),
            }
        }
        // ph_ty = Some(quote! )
        ph_ty.unwrap_or(quote! {()}) // TODO: `!` instead
    };

    // The content of `handle_pending_inbound_connection`.
    let handle_pending_inbound_connection_stmts =
        data_struct
            .fields
            .iter()
            .enumerate()
            .map(|(field_n, field)| {
                match field.ident {
                    Some(ref i) => quote! {
                        #trait_to_impl::handle_pending_inbound_connection(&mut self.#i, connection_id, local_addr, remote_addr)?;
                    },
                    None => quote! {
                        #trait_to_impl::handle_pending_inbound_connection(&mut self.#field_n, connection_id, local_addr, remote_addr)?;
                    }
                }
            });

    // The content of `handle_established_inbound_connection`.
    let handle_established_inbound_connection = {
        let mut out_handler = None;

        for (field_n, field) in data_struct.fields.iter().enumerate() {
            let field_name = match field.ident {
                Some(ref i) => quote! { self.#i },
                None => quote! { self.#field_n },
            };

            let builder = quote! {
                #field_name.handle_established_inbound_connection(connection_id, peer, local_addr, remote_addr)?
            };

            match out_handler {
                Some(h) => out_handler = Some(quote! { #connection_handler::select(#h, #builder) }),
                ref mut h @ None => *h = Some(builder),
            }
        }

        out_handler.unwrap_or(quote! {()}) // TODO: See test `empty`.
    };

    // The content of `handle_pending_outbound_connection`.
    let handle_pending_outbound_connection = {
        let extend_stmts =
            data_struct
                .fields
                .iter()
                .enumerate()
                .map(|(field_n, field)| {
                    match field.ident {
                        Some(ref i) => quote! {
                            combined_addresses.extend(#trait_to_impl::handle_pending_outbound_connection(&mut self.#i, connection_id, maybe_peer, addresses, effective_role)?);
                        },
                        None => quote! {
                            combined_addresses.extend(#trait_to_impl::handle_pending_outbound_connection(&mut self.#field_n, connection_id, maybe_peer, addresses, effective_role)?);
                        }
                    }
                });

        quote! {
            let mut combined_addresses = vec![];

            #(#extend_stmts)*

            Ok(combined_addresses)
        }
    };

    // The content of `handle_established_outbound_connection`.
    let handle_established_outbound_connection = {
        let mut out_handler = None;

        for (field_n, field) in data_struct.fields.iter().enumerate() {
            let field_name = match field.ident {
                Some(ref i) => quote! { self.#i },
                None => quote! { self.#field_n },
            };

            let builder = quote! {
                #field_name.handle_established_outbound_connection(connection_id, peer, addr, role_override)?
            };

            match out_handler {
                Some(h) => out_handler = Some(quote! { #connection_handler::select(#h, #builder) }),
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

        let generate_event_match_arm =  {
            // If the `NetworkBehaviour`'s `ToSwarm` is generated by the derive macro, wrap the sub
            // `NetworkBehaviour` `ToSwarm` in the variant of the generated `ToSwarm`. If the
            // `NetworkBehaviour`'s `ToSwarm` is provided by the user, use the corresponding `From`
            // implementation.
            let into_out_event = if out_event_definition.is_some() {
                let event_variant: syn::Variant = syn::parse_str(
                    &field
                        .to_string()
                        .to_upper_camel_case()
                ).expect("uppercased field name to be a valid enum variant name");
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

        quote!{
            match #trait_to_impl::poll(&mut self.#field, cx, poll_params) {
                #generate_event_match_arm
                std::task::Poll::Ready(#network_behaviour_action::Dial { opts }) => {
                    return std::task::Poll::Ready(#network_behaviour_action::Dial { opts });
                }
                std::task::Poll::Ready(#network_behaviour_action::ListenOn { opts }) => {
                    return std::task::Poll::Ready(#network_behaviour_action::ListenOn { opts });
                }
                std::task::Poll::Ready(#network_behaviour_action::RemoveListener { id }) => {
                    return std::task::Poll::Ready(#network_behaviour_action::RemoveListener { id });
                }
                std::task::Poll::Ready(#network_behaviour_action::NotifyHandler { peer_id, handler, event }) => {
                    return std::task::Poll::Ready(#network_behaviour_action::NotifyHandler {
                        peer_id,
                        handler,
                        event: #wrapped_event,
                    });
                }
                std::task::Poll::Ready(#network_behaviour_action::NewExternalAddrCandidate(addr)) => {
                    return std::task::Poll::Ready(#network_behaviour_action::NewExternalAddrCandidate(addr));
                }
                std::task::Poll::Ready(#network_behaviour_action::ExternalAddrConfirmed(addr)) => {
                    return std::task::Poll::Ready(#network_behaviour_action::ExternalAddrConfirmed(addr));
                }
                std::task::Poll::Ready(#network_behaviour_action::ExternalAddrExpired(addr)) => {
                    return std::task::Poll::Ready(#network_behaviour_action::ExternalAddrExpired(addr));
                }
                std::task::Poll::Ready(#network_behaviour_action::CloseConnection { peer_id, connection }) => {
                    return std::task::Poll::Ready(#network_behaviour_action::CloseConnection { peer_id, connection });
                }
                std::task::Poll::Pending => {},
            }
        }
    });

    let out_event_reference = if out_event_definition.is_some() {
        quote! { #out_event_name #ty_generics }
    } else {
        quote! { #out_event_name }
    };

    // Now the magic happens.
    let final_quote = quote! {
        #deprecation_tokenstream

        #out_event_definition

        impl #impl_generics #trait_to_impl for #name #ty_generics
        #where_clause
        {
            type ConnectionHandler = #connection_handler_ty;
            type ToSwarm = #out_event_reference;

            #[allow(clippy::needless_question_mark)]
            fn handle_pending_inbound_connection(
                &mut self,
                connection_id: #connection_id,
                local_addr: &#multiaddr,
                remote_addr: &#multiaddr,
            ) -> Result<(), #connection_denied> {
                #(#handle_pending_inbound_connection_stmts)*

                Ok(())
            }

            #[allow(clippy::needless_question_mark)]
            fn handle_established_inbound_connection(
                &mut self,
                connection_id: #connection_id,
                peer: #peer_id,
                local_addr: &#multiaddr,
                remote_addr: &#multiaddr,
            ) -> Result<#t_handler<Self>, #connection_denied> {
                Ok(#handle_established_inbound_connection)
            }

            #[allow(clippy::needless_question_mark)]
            fn handle_pending_outbound_connection(
                &mut self,
                connection_id: #connection_id,
                maybe_peer: Option<#peer_id>,
                addresses: &[#multiaddr],
                effective_role: #endpoint,
            ) -> Result<::std::vec::Vec<#multiaddr>, #connection_denied> {
                #handle_pending_outbound_connection
            }

            #[allow(clippy::needless_question_mark)]
            fn handle_established_outbound_connection(
                &mut self,
                connection_id: #connection_id,
                peer: #peer_id,
                addr: &#multiaddr,
                role_override: #endpoint,
            ) -> Result<#t_handler<Self>, #connection_denied> {
                Ok(#handle_established_outbound_connection)
            }

            fn on_connection_handler_event(
                &mut self,
                peer_id: #peer_id,
                connection_id: #connection_id,
                event: #t_handler_out_event<Self>
            ) {
                match event {
                    #(#on_node_event_stmts),*
                }
            }

            fn poll(&mut self, cx: &mut std::task::Context, poll_params: &mut impl #poll_parameters) -> std::task::Poll<#network_behaviour_action<Self::ToSwarm, #t_handler_in_event<Self>>> {
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
                        #dial_failure { peer_id, connection_id, error })
                    => { #(#on_dial_failure_stmts)* }
                    #from_swarm::ListenFailure(
                        #listen_failure { local_addr, send_back_addr, connection_id, error })
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
                    #from_swarm::NewExternalAddrCandidate(
                        #new_external_addr_candidate { addr })
                    => { #(#on_new_external_addr_candidate_stmts)* }
                    #from_swarm::ExternalAddrExpired(
                        #external_addr_expired { addr })
                    => { #(#on_external_addr_expired_stmts)* }
                    #from_swarm::ExternalAddrConfirmed(
                        #external_addr_confirmed { addr })
                    => { #(#on_external_addr_confirmed_stmts)* }
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

    Ok(final_quote.into())
}

struct BehaviourAttributes {
    prelude_path: syn::Path,
    user_specified_out_event: Option<syn::Type>,
    deprecation_tokenstream: proc_macro2::TokenStream,
}

/// Parses the `value` of a key=value pair in the `#[behaviour]` attribute into the requested type.
fn parse_attributes(ast: &DeriveInput) -> syn::Result<BehaviourAttributes> {
    let mut attributes = BehaviourAttributes {
        prelude_path: syn::parse_quote! { ::libp2p::swarm::derive_prelude },
        user_specified_out_event: None,
        deprecation_tokenstream: proc_macro2::TokenStream::new(),
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
                if meta.path().is_ident("out_event") {
                    let warning = proc_macro_warning::FormattedWarning::new_deprecated(
                        "out_event_renamed_to_to_swarm",
                        "The `out_event` attribute has been renamed to `to_swarm`.",
                        meta.span(),
                    );

                    attributes.deprecation_tokenstream = quote::quote! { #warning };
                }

                let value = meta.require_name_value()?.value.require_str_lit()?;

                attributes.user_specified_out_event = Some(syn::parse_str(&value)?);

                continue;
            }
        }
    }

    Ok(attributes)
}
