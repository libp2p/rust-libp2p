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

use heck::ToUpperCamelCase;
use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, punctuated::Punctuated, Data, DataStruct, DeriveInput, Meta, Token};

use crate::syn_ext::RequireStrLit;

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
    } = parse_attributes(ast)?;

    let multiaddr = quote! { #prelude_path::Multiaddr };
    let trait_to_impl = quote! { #prelude_path::NetworkBehaviour };
    let either_ident = quote! { #prelude_path::Either };
    let network_behaviour_action = quote! { #prelude_path::ToSwarm };
    let connection_handler = quote! { #prelude_path::ConnectionHandler };
    let proto_select_ident = quote! { #prelude_path::ConnectionHandlerSelect };
    let peer_id = quote! { #prelude_path::PeerId };
    let connection_id = quote! { #prelude_path::ConnectionId };
    let from_swarm = quote! { #prelude_path::FromSwarm };
    let t_handler = quote! { #prelude_path::THandler };
    let t_handler_in_event = quote! { #prelude_path::THandlerInEvent };
    let t_handler_out_event = quote! { #prelude_path::THandlerOutEvent };
    let endpoint = quote! { #prelude_path::Endpoint };
    let connection_denied = quote! { #prelude_path::ConnectionDenied };
    let port_use = quote! { #prelude_path::PortUse };

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
                        #visibility enum #enum_name #impl_generics
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

    // Build the list of statements to put in the body of `on_swarm_event()`.
    let on_swarm_event_stmts = {
        data_struct
            .fields
            .iter()
            .enumerate()
            .map(|(field_n, field)| match field.ident {
                Some(ref i) => quote! {
                    self.#i.on_swarm_event(event);
                },
                None => quote! {
                    self.#field_n.on_swarm_event(event);
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
                #field_name.handle_established_outbound_connection(connection_id, peer, addr, role_override, port_use)?
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
    let poll_stmts = data_struct
        .fields
        .iter()
        .enumerate()
        .map(|(field_n, field)| {
            let field = field
                .ident
                .clone()
                .expect("Fields of NetworkBehaviour implementation to be named.");

            let mut wrapped_event = if field_n != 0 {
                quote! { #either_ident::Right(event) }
            } else {
                quote! { event }
            };
            for _ in 0..data_struct.fields.len() - 1 - field_n {
                wrapped_event = quote! { #either_ident::Left(#wrapped_event) };
            }

            // If the `NetworkBehaviour`'s `ToSwarm` is generated by the derive macro, wrap the sub
            // `NetworkBehaviour` `ToSwarm` in the variant of the generated `ToSwarm`. If the
            // `NetworkBehaviour`'s `ToSwarm` is provided by the user, use the corresponding `From`
            // implementation.
            let map_out_event = if out_event_definition.is_some() {
                let event_variant: syn::Variant =
                    syn::parse_str(&field.to_string().to_upper_camel_case())
                        .expect("uppercased field name to be a valid enum variant name");
                quote! { #out_event_name::#event_variant }
            } else {
                quote! { |e| e.into() }
            };

            let map_in_event = quote! { |event| #wrapped_event };

            quote! {
                match #trait_to_impl::poll(&mut self.#field, cx) {
                    std::task::Poll::Ready(e) => return std::task::Poll::Ready(e.map_out(#map_out_event).map_in(#map_in_event)),
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
                port_use: #port_use,
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

            fn poll(&mut self, cx: &mut std::task::Context) -> std::task::Poll<#network_behaviour_action<Self::ToSwarm, #t_handler_in_event<Self>>> {
                #(#poll_stmts)*
                std::task::Poll::Pending
            }

            fn on_swarm_event(&mut self, event: #from_swarm) {
                #(#on_swarm_event_stmts)*
            }
        }
    };

    Ok(final_quote.into())
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
