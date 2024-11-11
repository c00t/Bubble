use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

pub fn plugin_export(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let struct_name = &input.ident;

    let expanded = quote! {
        // #attr
        #input

        #[no_mangle]
        pub fn load_plugin(
            context: &self::PluginContext,
            api_registry: self::ApiHandle<dyn self::ApiRegistryApi>,
            is_reload: bool,
        ) -> bool {
            #struct_name::load_plugin(context, api_registry, is_reload)
        }

        #[no_mangle]
        pub fn unload_plugin(
            context: &self::PluginContext,
            api_registry: self::ApiHandle<dyn self::ApiRegistryApi>,
            is_reload: bool,
        ) -> bool {
            #struct_name::unload_plugin(context, api_registry, is_reload)
        }

        #[no_mangle]
        pub fn plugin_info() -> self::PluginInfo {
            #struct_name::plugin_info()
        }
    };

    TokenStream::from(expanded)
}
