use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input, parse_quote,
    punctuated::Punctuated,
    DeriveInput, ExprTuple, FnArg, ItemTrait, Path, Token, TraitItem,
};

use crate::shared::{dyn_ident, snake_case, Version};

struct ApiDefineAttr {
    api_trait_paths: Punctuated<Path, Token![,]>,
}

impl Parse for ApiDefineAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        // Parse one or more trait paths separated by commas
        let api_trait_paths = Punctuated::parse_terminated(input)?;

        Ok(ApiDefineAttr { api_trait_paths })
    }
}

pub fn define_api(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    let ApiDefineAttr { api_trait_paths } = parse_macro_input!(attr as ApiDefineAttr);

    assert_eq!(api_trait_paths.len(), 1, "Only 1 path permitted");

    let struct_name = &input.ident;

    // Get all trait segments for a single make_trait_castable attribute
    let trait_segments: Vec<_> = api_trait_paths
        .iter()
        .map(|path| {
            path.segments
                .last()
                .expect("API trait path should have at least one segment")
                .ident
                .clone()
        })
        .collect();

    // Generate impl_api! calls for all trait paths
    let impl_api_calls = api_trait_paths.iter().map(|path| {
        let last_segment = path
            .segments
            .last()
            .expect("API trait path should have at least one segment");
        let constant_mod_ident = format_ident!(
            "{}",
            format!(
                "{}",
                snake_case(&format!("{}Constants", last_segment.ident))
            )
        );
        quote! {
            impl self::Api for #struct_name {
                fn name(&self) -> &'static str {
                    <dyn #last_segment as ApiConstant>::NAME
                }
                fn version(&self) -> self::Version {
                    <dyn #last_segment as ApiConstant>::VERSION
                }
            }
        }
    });

    // Generate register functions for all trait paths
    let register_fns = api_trait_paths.iter().map(|path| {
        let last_segment = path.segments.last()
            .expect("API trait path should have at least one segment");
        let api_trait_last_path = &last_segment.ident;
        let register_api_fn_name = format_ident!("register_{}", snake_case(&api_trait_last_path.to_string()));
        let doc_comment = format!("Register one {} instance into the API registry", api_trait_last_path);
        let dyn_type_ident = dyn_ident(&last_segment.ident);
        let local_api_handle = format_ident!("{}Handle", api_trait_last_path);
        quote! {
            #[doc = #doc_comment]
            pub fn #register_api_fn_name(api_registry_api: &ApiHandle<dyn ApiRegistryApi>) -> ApiHandle<#dyn_type_ident> {
                api_registry_api
                    .get()
                    .expect("Failed to get API registry api")
                    .local_set::<#dyn_type_ident>(#struct_name::builder().build().into())
            }

            pub struct #local_api_handle<'local> {
                api: LocalApiHandle<'local, #dyn_type_ident>
            }

            // implement From<LocalApiHandle<'local, #dyn_type_ident>> for #local_api_handle<'local>
            impl<'local> From<LocalApiHandle<'local, #dyn_type_ident>> for #local_api_handle<'local> {
                fn from(api: LocalApiHandle<'local, #dyn_type_ident>) -> Self {
                    Self { api }
                }
            }
        }
    });

    let expanded = quote! {
        #[make_trait_castable(Api, #(#trait_segments),*)]
        #input

        #(#impl_api_calls)*

        #(#register_fns)*
    };

    TokenStream::from(expanded)
}

// Structure to hold both version and path
struct ApiDeclarationAttr {
    version: Version,
    path: syn::Path,
}

// Parse implementation for ApiDeclaration
impl Parse for ApiDeclarationAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let version = input.parse()?;
        input.parse::<Token![,]>()?;
        let path = input.parse()?;

        Ok(ApiDeclarationAttr { version, path })
    }
}

pub fn declare_api(args: TokenStream, item: TokenStream) -> TokenStream {
    let ApiDeclarationAttr { version, path } = parse_macro_input!(args as ApiDeclarationAttr);
    let input = parse_macro_input!(item as ItemTrait);
    let trait_ident = &input.ident;
    let major = version.major;
    let minor = version.minor;
    let patch = version.patch;

    let constant_mod_ident = format_ident!(
        "{}",
        format!("{}", snake_case(&format!("{}Constants", trait_ident)))
    );
    let dyn_type_ident = dyn_ident(trait_ident);

    // Generate implementation methods for each trait method
    let methods = input
        .items
        .iter()
        .filter_map(|item| {
            if let syn::TraitItem::Fn(method) = item {
                let sig = &method.sig;
                let name = &sig.ident;
                let inputs = &sig.inputs;

                // Extract argument names for forwarding
                let args = inputs
                    .iter()
                    .skip(1)
                    .filter_map(|arg| {
                        if let syn::FnArg::Typed(pat_type) = arg {
                            if let syn::Pat::Ident(pat_ident) = &*pat_type.pat {
                                Some(&pat_ident.ident)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();

                let impl_method = quote! {
                    #sig {
                        (**self).#name(#(#args),*)
                    }
                };
                Some(impl_method)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    let expanded = quote! {
        pub(crate) mod #constant_mod_ident {
            use super::Version;
            pub const NAME: &'static str = ::std::stringify!(#path);
            pub const VERSION: self::Version = self::Version::new(
                #major,
                #minor,
                #patch
            );
        }

        impl ApiConstant for #dyn_type_ident {
            const NAME: &'static str = #constant_mod_ident::NAME;

            const VERSION: self::Version = #constant_mod_ident::VERSION;
        }

        pub type #dyn_type_ident = dyn #trait_ident;

        unique_id_without_version_hash! {
            #[UniqueTypeIdVersion((#major, #minor, #patch))]
            dyn #path
        }

        impl<T: #trait_ident + ?Sized> #trait_ident for Box<T> {
            #(#methods)*
        }

        #input
    };

    TokenStream::from(expanded)
}
