extern crate proc_macro;

use proc_macro::TokenStream;
use quote::quote;
use syn::ItemStruct;
use syn::{parse_macro_input, Attribute, ItemFn, Lit};

#[proc_macro_attribute]
pub fn register_llmapi(attr: TokenStream, item: TokenStream) -> TokenStream {
    // 将输入解析为一个结构体
    let input = parse_macro_input!(item as ItemStruct);

    // 获取结构体的名字
    let struct_name = &input.ident;

    let function_name_str = format!("{}_reg", struct_name.to_string().to_lowercase());
    let function_name = syn::Ident::new(&function_name_str, struct_name.span());

    // 生成额外的注册代码
    let generated_code = quote! {
        #[ctor]
        fn #function_name() {
            register(#struct_name::name(), |apikey,model| Box::new(#struct_name::new(apikey,model)));
        }
    };

    let expanded = quote! {
        // 自动完成注册
        #generated_code
         // Include the original struct definition
        #input
    };

    TokenStream::from(expanded)
}

#[cfg(test)]
mod tests {
    use super::*;
}
