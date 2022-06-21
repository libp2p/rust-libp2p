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

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DataStruct, DeriveInput, Ident};

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
    let multiaddr = quote! {::libp2p::core::Multiaddr};
    let trait_to_impl = quote! {::libp2p::swarm::NetworkBehaviour};
    let net_behv_event_proc = quote! {::libp2p::swarm::NetworkBehaviourEventProcess};
    let either_ident = quote! {::libp2p::core::either::EitherOutput};
    let network_behaviour_action = quote! {::libp2p::swarm::NetworkBehaviourAction};
    let into_connection_handler = quote! {::libp2p::swarm::IntoConnectionHandler};
    let connection_handler = quote! {::libp2p::swarm::ConnectionHandler};
    let into_proto_select_ident = quote! {::libp2p::swarm::IntoConnectionHandlerSelect};
    let peer_id = quote! {::libp2p::core::PeerId};
    let connection_id = quote! {::libp2p::core::connection::ConnectionId};
    let dial_errors = quote! {Option<&Vec<::libp2p::core::Multiaddr>>};
    let connected_point = quote! {::libp2p::core::ConnectedPoint};
    let listener_id = quote! {::libp2p::core::connection::ListenerId};
    let dial_error = quote! {::libp2p::swarm::DialError};

    let poll_parameters = quote! {::libp2p::swarm::PollParameters};

    // Build the generics.
    let impl_generics = {
        let tp = ast.generics.type_params();
        let lf = ast.generics.lifetimes();
        let cst = ast.generics.const_params();
        quote! {<#(#lf,)* #(#tp,)* #(#cst,)*>}
    };

    // Whether or not we require the `NetworkBehaviourEventProcess` trait to be implemented.
    let event_process = {
        let mut event_process = false;

        for meta_items in ast.attrs.iter().filter_map(get_meta_items) {
            for meta_item in meta_items {
                match meta_item {
                    syn::NestedMeta::Meta(syn::Meta::NameValue(ref m))
                        if m.path.is_ident("event_process") =>
                    {
                        if let syn::Lit::Bool(ref b) = m.lit {
                            event_process = b.value
                        }
                    }
                    _ => (),
                }
            }
        }

        event_process
    };

    // The fields of the struct we are interested in (no ignored fields).
    let data_struct_fields = data_struct
        .fields
        .iter()
        .filter(|f| !is_ignored(f))
        .collect::<Vec<_>>();

    // The final out event.
    // If we find a `#[behaviour(out_event = "Foo")]` attribute on the struct, we set `Foo` as
    // the out event. Otherwise we use `()`.
    let out_event = {
        let mut out = quote! {()};
        for meta_items in ast.attrs.iter().filter_map(get_meta_items) {
            for meta_item in meta_items {
                match meta_item {
                    syn::NestedMeta::Meta(syn::Meta::NameValue(ref m))
                        if m.path.is_ident("out_event") =>
                    {
                        if let syn::Lit::Str(ref s) = m.lit {
                            let ident: syn::Type = syn::parse_str(&s.value()).unwrap();
                            out = quote! {#ident};
                        }
                    }
                    _ => (),
                }
            }
        }
        out
    };

    // Build the `where ...` clause of the trait implementation.
    let where_clause = {
        let additional = data_struct_fields
            .iter()
            .flat_map(|field| {
                let ty = &field.ty;
                vec![
                    quote! {#ty: #trait_to_impl},
                    if event_process {
                        quote! {Self: #net_behv_event_proc<<#ty as #trait_to_impl>::OutEvent>}
                    } else {
                        quote! {#out_event: From< <#ty as #trait_to_impl>::OutEvent >}
                    },
                ]
            })
            .collect::<Vec<_>>();

        if let Some(where_clause) = where_clause {
            if where_clause.predicates.trailing_punct() {
                Some(quote! {#where_clause #(#additional),*})
            } else {
                Some(quote! {#where_clause, #(#additional),*})
            }
        } else {
            Some(quote! {where #(#additional),*})
        }
    };

    // Build the list of statements to put in the body of `addresses_of_peer()`.
    let addresses_of_peer_stmts = {
        data_struct_fields
            .iter()
            .enumerate()
            .map(move |(field_n, field)| match field.ident {
                Some(ref i) => quote! { out.extend(self.#i.addresses_of_peer(peer_id)); },
                None => quote! { out.extend(self.#field_n.addresses_of_peer(peer_id)); },
            })
    };

    // Build the list of statements to put in the body of `inject_connection_established()`.
    let inject_connection_established_stmts = {
        data_struct_fields.iter().enumerate().map(move |(field_n, field)| {
            match field.ident {
                Some(ref i) => quote!{ self.#i.inject_connection_established(peer_id, connection_id, endpoint, errors, other_established); },
                None => quote!{ self.#field_n.inject_connection_established(peer_id, connection_id, endpoint, errors, other_established); },
            }
        })
    };

    // Build the list of statements to put in the body of `inject_address_change()`.
    let inject_address_change_stmts = {
        data_struct_fields.iter().enumerate().map(move |(field_n, field)| {
            match field.ident {
                Some(ref i) => quote!{ self.#i.inject_address_change(peer_id, connection_id, old, new); },
                None => quote!{ self.#field_n.inject_address_change(peer_id, connection_id, old, new); },
            }
        })
    };

    // Build the list of statements to put in the body of `inject_connection_closed()`.
    let inject_connection_closed_stmts = {
        data_struct_fields
            .iter()
            .enumerate()
            // The outmost handler belongs to the last behaviour.
            .rev()
            .enumerate()
            .map(move |(enum_n, (field_n, field))| {
                let handler = if field_n == 0 {
                    // Given that the iterator is reversed, this is the innermost handler only.
                    quote! { let handler = handlers }
                } else {
                    quote! {
                        let (handlers, handler) = handlers.into_inner()
                    }
                };
                let inject = match field.ident {
                    Some(ref i) => quote!{ self.#i.inject_connection_closed(peer_id, connection_id, endpoint, handler, remaining_established) },
                    None => quote!{ self.#enum_n.inject_connection_closed(peer_id, connection_id, endpoint, handler, remaining_established) },
                };

                quote! {
                    #handler;
                    #inject;
                }
            })
    };

    // Build the list of statements to put in the body of `inject_dial_failure()`.
    let inject_dial_failure_stmts = {
        data_struct_fields
            .iter()
            .enumerate()
            // The outmost handler belongs to the last behaviour.
            .rev()
            .enumerate()
            .map(move |(enum_n, (field_n, field))| {
                let handler = if field_n == 0 {
                    // Given that the iterator is reversed, this is the innermost handler only.
                    quote! { let handler = handlers }
                } else {
                    quote! {
                        let (handlers, handler) = handlers.into_inner()
                    }
                };

                let inject = match field.ident {
                    Some(ref i) => {
                        quote! { self.#i.inject_dial_failure(peer_id, handler, error) }
                    }
                    None => {
                        quote! { self.#enum_n.inject_dial_failure(peer_id, handler, error) }
                    }
                };

                quote! {
                    #handler;
                    #inject;
                }
            })
    };

    // Build the list of statements to put in the body of `inject_listen_failure()`.
    let inject_listen_failure_stmts = {
        data_struct_fields
            .iter()
            .enumerate()
            .rev()
            .enumerate()
            .map(move |(enum_n, (field_n, field))| {
                let handler = if field_n == 0 {
                    quote! { let handler = handlers }
                } else {
                    quote! {
                        let (handlers, handler) = handlers.into_inner()
                    }
                };

                let inject = match field.ident {
                    Some(ref i) => quote! { self.#i.inject_listen_failure(local_addr, send_back_addr, handler) },
                    None => quote! { self.#enum_n.inject_listen_failure(local_addr, send_back_addr, handler) },
                };

                quote! {
                    #handler;
                    #inject;
                }
            })
    };

    // Build the list of statements to put in the body of `inject_new_listener()`.
    let inject_new_listener_stmts = {
        data_struct_fields
            .iter()
            .enumerate()
            .map(move |(field_n, field)| match field.ident {
                Some(ref i) => quote! { self.#i.inject_new_listener(id); },
                None => quote! { self.#field_n.inject_new_listener(id); },
            })
    };

    // Build the list of statements to put in the body of `inject_new_listen_addr()`.
    let inject_new_listen_addr_stmts = {
        data_struct_fields
            .iter()
            .enumerate()
            .map(move |(field_n, field)| match field.ident {
                Some(ref i) => quote! { self.#i.inject_new_listen_addr(id, addr); },
                None => quote! { self.#field_n.inject_new_listen_addr(id, addr); },
            })
    };

    // Build the list of statements to put in the body of `inject_expired_listen_addr()`.
    let inject_expired_listen_addr_stmts = {
        data_struct_fields
            .iter()
            .enumerate()
            .map(move |(field_n, field)| match field.ident {
                Some(ref i) => quote! { self.#i.inject_expired_listen_addr(id, addr); },
                None => quote! { self.#field_n.inject_expired_listen_addr(id, addr); },
            })
    };

    // Build the list of statements to put in the body of `inject_new_external_addr()`.
    let inject_new_external_addr_stmts = {
        data_struct_fields
            .iter()
            .enumerate()
            .map(move |(field_n, field)| match field.ident {
                Some(ref i) => quote! { self.#i.inject_new_external_addr(addr); },
                None => quote! { self.#field_n.inject_new_external_addr(addr); },
            })
    };

    // Build the list of statements to put in the body of `inject_expired_external_addr()`.
    let inject_expired_external_addr_stmts = {
        data_struct_fields
            .iter()
            .enumerate()
            .map(move |(field_n, field)| match field.ident {
                Some(ref i) => quote! { self.#i.inject_expired_external_addr(addr); },
                None => quote! { self.#field_n.inject_expired_external_addr(addr); },
            })
    };

    // Build the list of statements to put in the body of `inject_listener_error()`.
    let inject_listener_error_stmts = {
        data_struct_fields
            .iter()
            .enumerate()
            .map(move |(field_n, field)| match field.ident {
                Some(ref i) => quote!(self.#i.inject_listener_error(id, err);),
                None => quote!(self.#field_n.inject_listener_error(id, err);),
            })
    };

    // Build the list of statements to put in the body of `inject_listener_closed()`.
    let inject_listener_closed_stmts = {
        data_struct_fields
            .iter()
            .enumerate()
            .map(move |(field_n, field)| match field.ident {
                Some(ref i) => quote!(self.#i.inject_listener_closed(id, reason);),
                None => quote!(self.#field_n.inject_listener_closed(id, reason);),
            })
    };

    // Build the list of variants to put in the body of `inject_event()`.
    //
    // The event type is a construction of nested `#either_ident`s of the events of the children.
    // We call `inject_event` on the corresponding child.
    let inject_node_event_stmts = data_struct_fields.iter().enumerate().enumerate().map(|(enum_n, (field_n, field))| {
        let mut elem = if enum_n != 0 {
            quote!{ #either_ident::Second(ev) }
        } else {
            quote!{ ev }
        };

        for _ in 0 .. data_struct_fields.len() - 1 - enum_n {
            elem = quote!{ #either_ident::First(#elem) };
        }

        Some(match field.ident {
            Some(ref i) => quote!{ #elem => #trait_to_impl::inject_event(&mut self.#i, peer_id, connection_id, ev) },
            None => quote!{ #elem => #trait_to_impl::inject_event(&mut self.#field_n, peer_id, connection_id, ev) },
        })
    });

    // The [`ConnectionHandler`] associated type.
    let connection_handler_ty = {
        let mut ph_ty = None;
        for field in data_struct_fields.iter() {
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

        for (field_n, field) in data_struct_fields.iter().enumerate() {
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

    // The method to use to poll.
    // If we find a `#[behaviour(poll_method = "poll")]` attribute on the struct, we call
    // `self.poll()` at the end of the polling.
    let poll_method = {
        let mut poll_method = quote! {std::task::Poll::Pending};
        for meta_items in ast.attrs.iter().filter_map(get_meta_items) {
            for meta_item in meta_items {
                match meta_item {
                    syn::NestedMeta::Meta(syn::Meta::NameValue(ref m))
                        if m.path.is_ident("poll_method") =>
                    {
                        if let syn::Lit::Str(ref s) = m.lit {
                            let ident: Ident = syn::parse_str(&s.value()).unwrap();
                            poll_method = quote! {#name::#ident(self, cx, poll_params)};
                        }
                    }
                    _ => (),
                }
            }
        }
        poll_method
    };

    // List of statements to put in `poll()`.
    //
    // We poll each child one by one and wrap around the output.
    let poll_stmts = data_struct_fields.iter().enumerate().enumerate().map(|(enum_n, (field_n, field))| {
        let field_name = match field.ident {
            Some(ref i) => quote!{ self.#i },
            None => quote!{ self.#field_n },
        };

        let mut wrapped_event = if enum_n != 0 {
            quote!{ #either_ident::Second(event) }
        } else {
            quote!{ event }
        };
        for _ in 0 .. data_struct_fields.len() - 1 - enum_n {
            wrapped_event = quote!{ #either_ident::First(#wrapped_event) };
        }

        // `Dial` provides a handler of the specific behaviour triggering the
        // event. Though in order for the final handler to be able to handle
        // protocols of all behaviours, the provided handler needs to be
        // combined with handlers of all other behaviours.
        let provided_handler_and_new_handlers = {
            let mut out_handler = None;

            for (f_n, f) in data_struct_fields.iter().enumerate() {

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

        let generate_event_match_arm = if event_process {
            quote! {
                std::task::Poll::Ready(#network_behaviour_action::GenerateEvent(event)) => {
                    #net_behv_event_proc::inject_event(self, event)
                }
            }
        } else {
            quote! {
                std::task::Poll::Ready(#network_behaviour_action::GenerateEvent(event)) => {
                    return std::task::Poll::Ready(#network_behaviour_action::GenerateEvent(event.into()))
                }
            }
        };

        Some(quote!{
            loop {
                match #trait_to_impl::poll(&mut #field_name, cx, poll_params) {
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

    // Now the magic happens.
    let final_quote = quote! {
        impl #impl_generics #trait_to_impl for #name #ty_generics
        #where_clause
        {
            type ConnectionHandler = #connection_handler_ty;
            type OutEvent = #out_event;

            fn new_handler(&mut self) -> Self::ConnectionHandler {
                use #into_connection_handler;
                #new_handler
            }

            fn addresses_of_peer(&mut self, peer_id: &#peer_id) -> Vec<#multiaddr> {
                let mut out = Vec::new();
                #(#addresses_of_peer_stmts);*
                out
            }

            fn inject_connection_established(&mut self, peer_id: &#peer_id, connection_id: &#connection_id, endpoint: &#connected_point, errors: #dial_errors, other_established: usize) {
                #(#inject_connection_established_stmts);*
            }

            fn inject_address_change(&mut self, peer_id: &#peer_id, connection_id: &#connection_id, old: &#connected_point, new: &#connected_point) {
                #(#inject_address_change_stmts);*
            }

            fn inject_connection_closed(&mut self, peer_id: &#peer_id, connection_id: &#connection_id, endpoint: &#connected_point, handlers: <Self::ConnectionHandler as #into_connection_handler>::Handler, remaining_established: usize) {
                #(#inject_connection_closed_stmts);*
            }

            fn inject_dial_failure(&mut self, peer_id: Option<#peer_id>, handlers: Self::ConnectionHandler, error: &#dial_error) {
                #(#inject_dial_failure_stmts);*
            }

            fn inject_listen_failure(&mut self, local_addr: &#multiaddr, send_back_addr: &#multiaddr, handlers: Self::ConnectionHandler) {
                #(#inject_listen_failure_stmts);*
            }

            fn inject_new_listener(&mut self, id: #listener_id) {
                #(#inject_new_listener_stmts);*
            }

            fn inject_new_listen_addr(&mut self, id: #listener_id, addr: &#multiaddr) {
                #(#inject_new_listen_addr_stmts);*
            }

            fn inject_expired_listen_addr(&mut self, id: #listener_id, addr: &#multiaddr) {
                #(#inject_expired_listen_addr_stmts);*
            }

            fn inject_new_external_addr(&mut self, addr: &#multiaddr) {
                #(#inject_new_external_addr_stmts);*
            }

            fn inject_expired_external_addr(&mut self, addr: &#multiaddr) {
                #(#inject_expired_external_addr_stmts);*
            }

            fn inject_listener_error(&mut self, id: #listener_id, err: &(dyn std::error::Error + 'static)) {
                #(#inject_listener_error_stmts);*
            }

            fn inject_listener_closed(&mut self, id: #listener_id, reason: std::result::Result<(), &std::io::Error>) {
                #(#inject_listener_closed_stmts);*
            }

            fn inject_event(
                &mut self,
                peer_id: #peer_id,
                connection_id: #connection_id,
                event: <<Self::ConnectionHandler as #into_connection_handler>::Handler as #connection_handler>::OutEvent
            ) {
                match event {
                    #(#inject_node_event_stmts),*
                }
            }

            fn poll(&mut self, cx: &mut std::task::Context, poll_params: &mut impl #poll_parameters) -> std::task::Poll<#network_behaviour_action<Self::OutEvent, Self::ConnectionHandler>> {
                use libp2p::futures::prelude::*;
                #(#poll_stmts)*
                let f: std::task::Poll<#network_behaviour_action<Self::OutEvent, Self::ConnectionHandler>> = #poll_method;
                f
            }
        }
    };

    final_quote.into()
}

fn get_meta_items(attr: &syn::Attribute) -> Option<Vec<syn::NestedMeta>> {
    if attr.path.segments.len() == 1 && attr.path.segments[0].ident == "behaviour" {
        match attr.parse_meta() {
            Ok(syn::Meta::List(ref meta)) => Some(meta.nested.iter().cloned().collect()),
            Ok(_) => None,
            Err(e) => {
                eprintln!("error parsing attribute metadata: {}", e);
                None
            }
        }
    } else {
        None
    }
}

/// Returns true if a field is marked as ignored by the user.
fn is_ignored(field: &syn::Field) -> bool {
    for meta_items in field.attrs.iter().filter_map(get_meta_items) {
        for meta_item in meta_items {
            match meta_item {
                syn::NestedMeta::Meta(syn::Meta::Path(ref m)) if m.is_ident("ignore") => {
                    return true;
                }
                _ => (),
            }
        }
    }

    false
}
