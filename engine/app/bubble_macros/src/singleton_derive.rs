use quote::quote;
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input, ItemStruct, Result,
};

struct DeriveSingletonInput {
    item: ItemStruct,
}

impl Parse for DeriveSingletonInput {
    fn parse(input: ParseStream) -> Result<Self> {
        Ok(Self {
            item: input.parse()?,
        })
    }
}

pub fn derive_singleton(item: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(item as DeriveSingletonInput);
    let ident = input.item.ident;

    // Generate the implementation
    let implementation = quote! {
        static INSTANCE: std::sync::atomic::AtomicPtr<#ident> =
            std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());

        impl machinery::Singleton for #ident {
            unsafe fn create(value: Self) {
                let boxed = Box::new(value);
                let result = INSTANCE.swap(
                    Box::into_raw(boxed),
                    std::sync::atomic::Ordering::SeqCst,
                );

                if !result.is_null() {
                    panic!("Plugin was already loaded!");
                }
            }

            unsafe fn destroy() {
                let plugin = INSTANCE.swap(
                    std::ptr::null_mut(),
                    std::sync::atomic::Ordering::SeqCst,
                );
                let _ = Box::from_raw(plugin);
            }

            unsafe fn ptr() -> *mut #ident {
                INSTANCE.load(std::sync::atomic::Ordering::SeqCst)
            }
        }
    };

    implementation.into()
}
