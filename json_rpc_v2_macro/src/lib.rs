use proc_macro::TokenStream;

use quote::{quote, quote_spanned};
use syn::{Error, FnArg, ImplItem, Item, ItemFn, ItemImpl, parse, parse_macro_input, parse_str, Pat, Path, ReturnType, Signature, Token, Type};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;

#[proc_macro_attribute]
pub fn json_rpc(_attr: TokenStream, input: TokenStream) -> TokenStream {
    match parse_macro_input!(input as Item) {
        Item::Fn(item) => expand_fn(item).unwrap_or_else(|e| e.to_compile_error().into()),
        Item::Impl(item) => expand_impl(item).unwrap_or_else(|e| e.to_compile_error().into()),
        item => Error::new_spanned(item, "json_rpc: expected fn or impl block").to_compile_error().into(),
    }
}

fn expand_impl(mut item: ItemImpl) -> Result<TokenStream, Error> {
    if item.trait_.is_some() {
        return Err(syn::Error::new_spanned(item.trait_.unwrap().1, "json_rpc: trait impl is not supported"));
    }
    if !item.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(item.generics, "json_rpc: generic is not supported"));
    }

    let prefix = match *item.self_ty {
        Type::Path(ref path) => path.path.segments.last().unwrap().ident.to_string().to_ascii_lowercase(),
        _ => return Err(syn::Error::new_spanned(item.self_ty, "json_rpc: not supported")),
    };

    let mut names = Vec::new();
    let mut func: Vec<Path> = Vec::new();
    for impl_item in &mut item.items {
        if let ImplItem::Fn(impl_item) = impl_item {
            names.push(format!("{}.{}", prefix, impl_item.sig.ident));
            func.push(parse_str(&format!("Self::{}", impl_item.sig.ident))?);
            *impl_item = parse(expand_fn(ItemFn {
                attrs: impl_item.attrs.clone(),
                vis: impl_item.vis.clone(),
                sig: impl_item.sig.clone(),
                block: Box::new(impl_item.block.clone()),
            })?)?;
        }
    }

    let ty = &item.self_ty;
    Ok(quote! {
        #item
        impl json_rpc_v2::Provider for #ty {
            fn methods() -> &'static [(&'static str, json_rpc_v2::Method)] {
                &[#((#names, #func as json_rpc_v2::Method)),*]
            }
        }
    }.into())
}

fn expand_fn(item: ItemFn) -> Result<TokenStream, Error> {
    let ident = &item.sig.ident;
    let vis = &item.vis;
    if !item.sig.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(item.sig.generics, "json_rpc: generic is not supported"));
    }

    let ret_assert = gen_ret_assert(&item.sig)?;
    let (arg_assert, args) = gen_arg_assert(&item.sig.inputs)?;
    let wait = item.sig.asyncness.map(|_| quote!(let result = result.await;));
    let argc = 0..args.len();
    let gen = quote! {
        #vis fn #ident(args: json_rpc_v2::serde_json::Value) -> std::pin::Pin<Box<dyn std::future::Future<Output=std::result::Result<json_rpc_v2::serde_json::Value, json_rpc_v2::Error>> + Send>> {
            #ret_assert
            #arg_assert
            #item

            #[allow(unused)]
            macro_rules! arg {
                ($v:expr) => {
                    json_rpc_v2::serde_json::from_value($v).map_err(|err| {
                        json_rpc_v2::error!("deserialize parameter error: {}", err);
                        json_rpc_v2::Error::invalid_params()
                    })
                };
            }

            Box::pin(async move {
                let result = match args {
                    json_rpc_v2::serde_json::Value::Array(mut args) => {
                        #ident(#(arg!(args.get_mut(#argc).map(json_rpc_v2::serde_json::Value::take).unwrap_or(json_rpc_v2::serde_json::Value::Null))?),*)
                    }
                    json_rpc_v2::serde_json::Value::Object(mut args) => {
                        #ident(#(arg!(args.remove(#args).unwrap_or(json_rpc_v2::serde_json::Value::Null))?),*)
                    }
                    _ => return Err(json_rpc_v2::Error::invalid_params()),
                };
                #wait
                Ok(json_rpc_v2::serde_json::to_value(result?).expect("serialize error"))
            })
        }
    };
    Ok(gen.into())
}

fn gen_ret_assert(sig: &Signature) -> Result<proc_macro2::TokenStream, Error> {
    match sig.output {
        ReturnType::Default => Err(Error::new_spanned(sig, "json rpc: expected return value")),
        ReturnType::Type(_, ref ty) => Ok(quote_spanned! {ty.span()=>
            {
                fn assert(_: Option<std::result::Result<impl json_rpc_v2::serde::Serialize, impl Into<json_rpc_v2::Error>>>) {}
                assert(None::<#ty>);
            }
        }),
    }
}

fn gen_arg_assert(inputs: &Punctuated<FnArg, Token![,]>) -> Result<(proc_macro2::TokenStream, Vec<String>), Error> {
    let mut assert = quote!();
    let mut args = Vec::with_capacity(inputs.len());
    for arg in inputs {
        match arg {
            FnArg::Typed(arg) => match *arg.pat {
                Pat::Ident(ref pat) => {
                    args.push(pat.ident.to_string());
                    let ty = &arg.ty;
                    assert = quote_spanned! {ty.span()=>
                        #assert
                        { struct _Assert where #ty: json_rpc_v2::serde::de::DeserializeOwned; }
                    };
                }
                _ => return Err(Error::new_spanned(arg, "json_rpc: unsupported argument")),
            },
            FnArg::Receiver(arg) => return Err(Error::new_spanned(arg, "json_rpc: method is not supported")),
        }
    }
    Ok((assert, args))
}