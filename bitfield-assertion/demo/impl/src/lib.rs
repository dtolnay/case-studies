extern crate proc_macro;

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, Data, DeriveInput};

#[proc_macro_attribute]
pub fn bitfield(_args: TokenStream, input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let fields = match &input.data {
        Data::Struct(data) => data.fields.iter().map(|field| &field.ty),
        _ => unimplemented!(),
    };

    TokenStream::from(quote! {
        fn __bitfield() {
            let _: bitfield::MultipleOfEight<
                [(); (0 #(+ <#fields as bitfield::Field>::BITS)*) % 8]
            >;
        }
    })
}

#[proc_macro]
pub fn generate_specifiers(_input: TokenStream) -> TokenStream {
    (0usize..=64usize)
        .map(|width| {
            let name = format_ident!("B{}", width);
            TokenStream::from(quote! {
                pub enum #name {}

                impl Field for #name {
                    const BITS: usize = #width;
                }
            })
        })
        .collect()
}
