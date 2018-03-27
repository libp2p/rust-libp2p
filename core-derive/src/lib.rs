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

//! Provides a custom derive `Libp2pConnectionUpgrade` that derives the `ConnectionUpgrade` trait
//! of the `libp2p_core` library.
//!
//! When derived on a struct, the custom derive supposes that each field is an implementation of
//! `CustomDerive`. Upgrades are tried one by one in order until one matches the protocol requested
//! by the remote.
//!
//! The custom derive generates a module whose name is the same as the struct that it is applied
//! on, except in snake case (eg. if you apply it on a struct named `Foo`, it will generate a
//! module named `foo`). This module contains an enum named `Output` where each variant corresponds
//! to a field of the struct. When an upgrade is successfully applied, you will obtain an `Output`.

#![recursion_limit = "256"]

#[macro_use]
extern crate quote;
extern crate syn;

extern crate proc_macro;
extern crate proc_macro2;

use proc_macro::TokenStream;
use std::mem;
use syn::DeriveInput;

#[proc_macro_derive(Libp2pConnectionUpgrade, attributes())]
pub fn derive_libp2p_upgrade(input: TokenStream) -> TokenStream {
    let input: DeriveInput = syn::parse(input).unwrap();
    match expand_derive(&input) {
        Ok(out) => out.into(),
        Err(msg) => panic!(msg),
    }
}

fn expand_derive(input: &syn::DeriveInput) -> Result<quote::Tokens, String> {
    let struct_name = &input.ident;
    let module_name: syn::Ident = {
        let original = input.ident.as_ref();
        let rest = original.chars().skip(1).flat_map(|chr| {
            // TODO: puts an _ before first char
            if chr.is_uppercase() {
                ['_'].iter()
            } else {
                [].iter()
            }.cloned()
                .chain(chr.to_lowercase())
        });
        original
            .chars()
            .next()
            .into_iter()
            .flat_map(|chr| chr.to_lowercase())
            .chain(rest)
            .collect::<String>()
            .into()
    };

    let fields: Vec<_> = match input.data {
        syn::Data::Struct(ref data_struct) => {
            data_struct.fields.iter().filter_map(|f| f.ident).collect()
        }
        _ => panic!("Cannot use this derive on non-structs"),
    };

    let fields_camel_case: Vec<_> = fields
        .iter()
        .map(|field| {
            let mut out = String::new();
            let mut prev_is_ = false;
            let mut is_first = true;
            for chr in field.to_string().chars() {
                let is_first = mem::replace(&mut is_first, false);
                if chr == '_' {
                    prev_is_ = true;
                    continue;
                }
                if is_first || prev_is_ {
                    prev_is_ = false;
                    for c in chr.to_uppercase() {
                        out.push(c);
                    }
                    continue;
                }
                prev_is_ = false;
                out.push(chr);
            }
            syn::Ident::from(out)
        })
        .collect();

    let field_tys: Vec<_> = match input.data {
        syn::Data::Struct(ref data_struct) => {
            data_struct.fields.iter().map(|f| f.ty.clone()).collect()
        }
        _ => panic!("Cannot use this derive on non-structs"),
    };

    let where_clause = {
        let clause1 = field_tys
            .iter()
            .map(|ty| {
                quote!{ #ty: ::libp2p_core::ConnectionUpgrade<C> }
            })
            .collect::<Vec<_>>();

        let clause2 = field_tys
            .iter()
            .map(|ty| {
                quote!{
                    <#ty as ::libp2p_core::ConnectionUpgrade<C>>::Future:
                        ::futures::Future<Error = ::std::io::Error>
                }
            })
            .collect::<Vec<_>>();

        quote!{
            where C: ::tokio_io::AsyncRead + ::tokio_io::AsyncWrite + 'static,
                #(#clause1,)*
                #(#clause2,)*
        }
    };

    let existing_where = input
        .generics
        .where_clause
        .as_ref()
        .map(|w| w.predicates.clone())
        .unwrap_or_default();

    let generated_names_iter = {
        let mut generated_names_iter = quote!{
            ::std::iter::Empty<(::bytes::Bytes, #module_name::UpgradeIdentifier<C>)>
        };
        for ty in field_tys.iter() {
            generated_names_iter = quote!{
                ::std::iter::Chain<
                    #generated_names_iter,
                    ::std::iter::Map<
                        <#ty as ::libp2p_core::ConnectionUpgrade<C>>::NamesIter,
                        fn(<<#ty as ::libp2p_core::ConnectionUpgrade<C>>::NamesIter as Iterator>
                                                                                            ::Item)
                            -> (::bytes::Bytes, #module_name::UpgradeIdentifier<C>)
                    >
                >
            };
        }
        generated_names_iter
    };

    let protocol_names_impl = {
        let elems = fields_camel_case
            .iter()
            .zip(fields.iter())
            .map(|(field_cc, field)| {
                quote!{
                    let iter = iter.chain(
                        self.#field
                            .protocol_names()
                            .map::<_, fn(_) -> _>(|(name, id)| {
                                (name, #module_name::UpgradeIdentifier::#field_cc(id))
                            })
                    );
                }
            })
            .collect::<Vec<_>>();

        quote!{
            let iter = ::std::iter::empty();
            #(#elems)*
            iter
        }
    };

    let upgrade_impl = {
        let elems = fields_camel_case
            .iter()
            .zip(fields.iter())
            .map(|(field_cc, field)| {
                quote!{
                    #module_name::UpgradeIdentifier::#field_cc(id) => {
                        let future = self.#field.upgrade(socket, id, ty, remote_addr);
                        #module_name::Future::#field_cc(future)
                    },
                }
            })
            .collect::<Vec<_>>();

        quote!{
            match id {
                #(#elems)*
            }
        }
    };

    let output_variants = fields_camel_case
        .iter()
        .zip(field_tys.iter())
        .map(|(field, ty)| {
            quote!{#field(<#ty as ::libp2p_core::ConnectionUpgrade<C>>::Output)}
        })
        .collect::<Vec<_>>();

    let upg_id_variants = fields_camel_case
        .iter()
        .zip(field_tys.iter())
        .map(|(field, ty)| {
            quote!{#field(<#ty as ::libp2p_core::ConnectionUpgrade<C>>::UpgradeIdentifier)}
        })
        .collect::<Vec<_>>();

    let future_variants = fields_camel_case
        .iter()
        .zip(field_tys.iter())
        .map(|(field, ty)| {
            quote!{#field(<#ty as ::libp2p_core::ConnectionUpgrade<C>>::Future)}
        })
        .collect::<Vec<_>>();

    let future_impl = {
        let elems = fields_camel_case
            .iter()
            .map(|field_cc| {
                quote!{
                    &mut Future::#field_cc(ref mut fut) => {
                        fut.poll().map(|good| good.map(|item| Output::#field_cc(item)))
                    },
                }
            })
            .collect::<Vec<_>>();

        quote!{
            match self {
                #(#elems)*
            }
        }
    };

    let generated = quote!{
        impl<C> ::libp2p_core::ConnectionUpgrade<C> for #struct_name
            #where_clause
            #existing_where
        {
            type NamesIter = #generated_names_iter;
            type UpgradeIdentifier = #module_name::UpgradeIdentifier<C>;

            fn protocol_names(&self) -> Self::NamesIter {
                #protocol_names_impl
            }

            type Output = #module_name::Output<C>;
            type Future = #module_name::Future<C>;

            fn upgrade(self, socket: C, id: Self::UpgradeIdentifier, ty: ::libp2p_core::Endpoint,
                       remote_addr: &::libp2p_core::Multiaddr) -> Self::Future
            {
                #upgrade_impl
            }
        }

        pub mod #module_name {
            pub enum Output<C>
                #where_clause
            {
                #(#output_variants,)*
            }

            pub enum UpgradeIdentifier<C>
                #where_clause
            {
                #(#upg_id_variants,)*
            }

            pub enum Future<C>
                #where_clause
            {
                #(#future_variants,)*
            }

            impl<C> ::futures::Future for Future<C>
                #where_clause
            {
                type Item = Output<C>;
                type Error = ::std::io::Error;

                fn poll(&mut self) -> ::futures::Poll<Self::Item, Self::Error> {
                    #future_impl
                }
            }
        }
    };

    Ok(generated)
}
