use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input, parse_quote,
    punctuated::Punctuated,
    DeriveInput, ExprTuple, FnArg, ItemTrait, Path, Token, TraitItem,
};

use crate::shared::{dyn_ident, snake_case, Version};

struct InterfaceDefineAttr {
    interface_trait_paths: Punctuated<Path, Token![,]>,
}

impl Parse for InterfaceDefineAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        // Parse one or more trait paths separated by commas
        let interface_trait_paths = Punctuated::parse_terminated(input)?;

        Ok(InterfaceDefineAttr {
            interface_trait_paths,
        })
    }
}

pub fn define_interface(attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    let InterfaceDefineAttr {
        interface_trait_paths,
    } = parse_macro_input!(attr as InterfaceDefineAttr);

    assert_eq!(interface_trait_paths.len(), 1, "Only 1 path permitted");

    let struct_name = &input.ident;

    // Get all trait segments for a single make_trait_castable attribute
    let trait_segments: Vec<_> = interface_trait_paths
        .iter()
        .map(|path| {
            path.segments
                .last()
                .expect("Interface trait path should have at least one segment")
                .ident
                .clone()
        })
        .collect();

    // Generate impl_interface! calls for all trait paths
    let impl_interface_calls = interface_trait_paths.iter().map(|path| {
        let last_segment = path
            .segments
            .last()
            .expect("Interface trait path should have at least one segment");
        quote! {
            impl self::Interface for #struct_name {
                fn name(&self) -> &'static str {
                    <dyn #last_segment as InterfaceConstant>::NAME
                }
                fn version(&self) -> self::Version {
                    <dyn #last_segment as InterfaceConstant>::VERSION
                }
                fn id(&self) -> self::UniqueId {
                    <Self as self::UniqueTypeId>::TYPE_ID
                }
            }
        }
    });

    // Generate register functions for all trait paths
    let register_fns = interface_trait_paths.iter().map(|path| {
        let last_segment = path.segments.last()
            .expect("Interface trait path should have at least one segment");
        let interface_trait_last_path = &last_segment.ident;
        let register_interface_fn_name = format_ident!("register_{}", snake_case(&interface_trait_last_path.to_string()));
        let doc_comment = format!("Register one {} instance into the API registry", interface_trait_last_path);
        let dyn_type_ident = dyn_ident(&last_segment.ident);
        quote! {
            #[doc = #doc_comment]
            pub fn #register_interface_fn_name(api_registry_api: &ApiHandle<dyn ApiRegistryApi>) -> InterfaceHandle<dyn #interface_trait_last_path> {
                api_registry_api
                    .get()
                    .expect("Failed to get API registry api")
                    .local_add_interface::<#dyn_type_ident>(#struct_name::builder().build().into())
            }
        }
    });

    let expanded = quote! {
        #[make_trait_castable(Interface, #(#trait_segments),*)]
        #input

        #(#impl_interface_calls)*

        #(#register_fns)*
    };

    TokenStream::from(expanded)
}

// Structure to hold both version and path
struct InterfaceDeclarationAttr {
    version: Version,
    path: syn::Path,
}

// Parse implementation for InterfaceDeclaration
impl Parse for InterfaceDeclarationAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let version = input.parse()?;
        input.parse::<Token![,]>()?;
        let path = input.parse()?;

        Ok(InterfaceDeclarationAttr { version, path })
    }
}

pub fn declare_interface(args: TokenStream, item: TokenStream) -> TokenStream {
    let InterfaceDeclarationAttr { version, path } =
        parse_macro_input!(args as InterfaceDeclarationAttr);
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

        impl InterfaceConstant for #dyn_type_ident {
            const NAME: &'static str = #constant_mod_ident::NAME;

            const VERSION: self::Version = #constant_mod_ident::VERSION;
        }

        pub type #dyn_type_ident = dyn #trait_ident;

        unique_id! {
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
