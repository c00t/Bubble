use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input, parse_quote, DeriveInput, ExprTuple, Path, Token,
};

struct InterfaceAttr {
    version: (u64, u64, u64),
    api_trait_path: Path,
}

impl Parse for InterfaceAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let version: ExprTuple = input.parse()?;
        input.parse::<Token![,]>()?;
        let api_trait_path: Path = input.parse()?;

        if version.elems.len() != 3 {
            return Err(syn::Error::new_spanned(
                version,
                "Expected version in the format (major, minor, patch)",
            ));
        }

        let parse_int = |expr: &syn::Expr| -> syn::Result<u64> {
            if let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Int(lit_int),
                ..
            }) = expr
            {
                lit_int.base10_parse()
            } else {
                Err(syn::Error::new_spanned(
                    expr,
                    "Expected integer literal for version number",
                ))
            }
        };

        let version = (
            parse_int(&version.elems[0])?,
            parse_int(&version.elems[1])?,
            parse_int(&version.elems[2])?,
        );

        Ok(InterfaceAttr {
            version,
            api_trait_path,
        })
    }
}

fn snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, ch) in s.char_indices() {
        if i > 0 && ch.is_uppercase() {
            result.push('_');
        }
        result.push(ch.to_lowercase().next().unwrap());
    }
    result
}

pub fn define_interface(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    let InterfaceAttr {
        version: (version_major, version_minor, version_patch),
        api_trait_path,
    } = parse_macro_input!(attr as InterfaceAttr);

    let struct_name = &input.ident;
    let api_trait_last_segment = api_trait_path
        .segments
        .last()
        .expect("API trait path should have at least one segment")
        .clone();
    let api_trait_last_path = &api_trait_last_segment.ident;
    let register_api_fn_name =
        format_ident!("register_{}", snake_case(&api_trait_last_path.to_string()));

    let doc_comment_register_api_fn = format!(
        "Register one {} instance into the API registry",
        api_trait_last_path
    );

    let source_path = proc_macro::Span::call_site().source_file().path();

    let expanded = quote! {
        // #[repr(C)]
        #[make_trait_castable(Interface, #api_trait_last_path)]
        #input

        impl_interface!(#struct_name, #api_trait_last_path, (#version_major, #version_minor, #version_patch));

        #[doc = #doc_comment_register_api_fn]
        pub fn #register_api_fn_name(api_registry_api: &ApiHandle<dyn ApiRegistryApi>) -> InterfaceHandle<dyn #api_trait_last_path> {
            api_registry_api
                .get()
                .expect("Failed to get API registry api")
                .add_interface(constants::NAME, constants::VERSION, #struct_name::builder().build().into())
                .downcast()
        }
    };

    TokenStream::from(expanded)
}

pub fn define_interface_with_id(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    let InterfaceAttr {
        version: (version_major, version_minor, version_patch),
        api_trait_path,
    } = parse_macro_input!(attr as InterfaceAttr);

    let struct_name = &input.ident;
    let api_trait_last_segment = api_trait_path
        .segments
        .last()
        .expect("API trait path should have at least one segment")
        .clone();
    let api_trait_last_path = &api_trait_last_segment.ident;
    let register_api_fn_name =
        format_ident!("register_{}", snake_case(&api_trait_last_path.to_string()));

    let doc_comment_register_api_fn = format!(
        "Register one {} instance into the API registry",
        api_trait_last_path
    );

    let source_path = proc_macro::Span::call_site().source_file().path();

    let expanded = quote! {
        // #[repr(C)]
        #[make_trait_castable(Interface, #api_trait_last_path)]
        #input

        unique_id! {
            #[UniqueTypeIdVersion((#version_major, #version_minor, #version_patch))]
            dyn #api_trait_path
        }

        impl_interface!(#struct_name, #api_trait_last_path, (#version_major, #version_minor, #version_patch));

        #[doc = #doc_comment_register_api_fn]
        pub fn #register_api_fn_name(api_registry_api: &ApiHandle<dyn ApiRegistryApi>) -> InterfaceHandle<dyn #api_trait_last_path> {
            api_registry_api
                .get()
                .expect("Failed to get API registry api")
                .add_interface(constants::NAME, constants::VERSION, #struct_name::builder().build().into())
                .downcast()
        }
    };

    TokenStream::from(expanded)
}
