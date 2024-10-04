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

#[cfg(feature = "experimental")]
mod experimental;
#[cfg(not(feature = "experimental"))]
mod stable;
mod syn_ext;

#[cfg(feature = "experimental")]
use experimental::*;
#[cfg(not(feature = "experimental"))]
use stable::*;

use crate::syn_ext::RequireStrLit;
use proc_macro::TokenStream;
use syn::punctuated::Punctuated;
use syn::{parse_macro_input, Data, DeriveInput, Meta, Token};

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
