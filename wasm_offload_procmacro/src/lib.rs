#![feature(proc_macro_span)]
use std::{collections::HashMap, io::Write};

use heck::ToUpperCamelCase;
use proc_macro::TokenStream;
use quote::{format_ident, quote, ToTokens};
use syn::{ExprAssign, FnArg, GenericArgument, ItemFn, PathArguments, ReturnType};
use wit_encoder::{
    Field, Interface, Package, PackageName, Record, StandaloneFunc, TypeDef, Use, World, WorldItem,
    WorldNamedInterface,
};

extern crate proc_macro;

#[derive(Default)]
struct TypeContext {
    types: HashMap<String, Vec<Field>>,
}

impl TypeContext {
    fn add_struct_type(&mut self, ty: syn::ItemStruct) {
        let mut fields = vec![];
        let ty_name = ty.ident.to_string().to_lowercase().replace('_', "-");
        for field in ty.fields {
            let f_name = field
                .ident
                .expect("Tuple structs are currently unsupported")
                .to_string();
            let f_ty = self.to_wit_type(&field.ty).unwrap();
            fields.push(wit_encoder::Field::new(f_name, f_ty));
        }
        self.types.insert(ty_name, fields);
    }

    fn to_wit_type(&mut self, input: &syn::Type) -> Option<wit_encoder::Type> {
        match input {
            syn::Type::Array(_) => todo!(),
            syn::Type::BareFn(_) => todo!(),
            syn::Type::Group(_) => todo!(),
            syn::Type::ImplTrait(_) => todo!(),
            syn::Type::Infer(_) => todo!(),
            syn::Type::Macro(_) => todo!(),
            syn::Type::Never(_) => todo!(),
            syn::Type::Paren(_) => todo!(),
            syn::Type::Path(p) => {
                let t = p.path.segments.last().unwrap();
                let t_id = t.ident.to_string();
                match t_id.as_str() {
                    "i8" => Some(wit_encoder::Type::S8),
                    "i16" => Some(wit_encoder::Type::S16),
                    "i32" => Some(wit_encoder::Type::S32),
                    "i64" => Some(wit_encoder::Type::S64),
                    "u8" => Some(wit_encoder::Type::U8),
                    "u16" => Some(wit_encoder::Type::U16),
                    "u32" => Some(wit_encoder::Type::U32),
                    "u64" => Some(wit_encoder::Type::U64),
                    "usize" => Some(wit_encoder::Type::U64),
                    "isize" => Some(wit_encoder::Type::S64),
                    "f32" => Some(wit_encoder::Type::F32),
                    "f64" => Some(wit_encoder::Type::F64),
                    "String" => Some(wit_encoder::Type::String),
                    "Vec" | "Option" => {
                        let PathArguments::AngleBracketed(ab) = &t.arguments else {
                            return None;
                        };
                        let arg = ab.args.first().unwrap();
                        if let GenericArgument::Type(ty) = arg {
                            self.to_wit_type(ty).map(|t| match t_id.as_str() {
                                "Vec" => wit_encoder::Type::list(t),
                                "Option" => wit_encoder::Type::option(t),
                                _ => unreachable!(),
                            })
                        } else {
                            None
                        }
                    }
                    "Result" => {
                        let PathArguments::AngleBracketed(ab) = &t.arguments else {
                            return None;
                        };
                        let mut args = ab.args.iter().take(2);
                        let ok_arg = args.next().unwrap();
                        let err_arg = args.next().unwrap();
                        if let (GenericArgument::Type(o_ty), GenericArgument::Type(e_ty)) =
                            (ok_arg, err_arg)
                        {
                            let wo_ty = self.to_wit_type(o_ty).unwrap();
                            let we_ty = self.to_wit_type(e_ty).unwrap();
                            Some(wit_encoder::Type::result_both(wo_ty, we_ty))
                        } else {
                            None
                        }
                    }
                    id => {
                        // println!("{id}");
                        // None
                        Some(wit_encoder::Type::named(id.to_string().to_lowercase()))
                    }
                }
            }
            syn::Type::Ptr(_) => todo!(),
            syn::Type::Reference(_) => todo!(),
            syn::Type::Slice(_) => todo!(),
            syn::Type::TraitObject(_) => todo!(),
            syn::Type::Tuple(_) => todo!(),
            syn::Type::Verbatim(_) => todo!(),
            _ => None,
        }
    }
}

fn impl_ty_into_val(ty: (&str, &[Field])) -> proc_macro2::TokenStream {
    let (name, fields) = ty;
    let ty_name = format_ident!("{}", name.to_upper_camel_case());
    let fields = fields.iter().map(|f| {
        let f_name = f.name().to_string();
        let f_name_id = format_ident!("{}", f_name);
        quote! {
            value.push((#f_name.to_string(), self.#f_name_id.into()));
        }
    });
    quote! {
        impl Into<wasm_offload::Val> for #ty_name {
            fn into(self) -> wasm_offload::Val {
                let mut value = vec![];
                #(#fields)*
                wasm_offload::Val::Record(value)
            }
        }
    }
}

fn create_component_source(input: ItemFn) -> proc_macro2::TokenStream {
    quote! {
        mod bindings {
            wit_bindgen::generate!({
                world: "offload"
            });
        }

        use bindings::*;

        pub struct Component;
        bindings::export!(Component with_types_in bindings);

        impl bindings::Guest for Component {
            #input
        }
    }
}

fn create_wit_bindings(ctx: &mut TypeContext, input: ItemFn) -> String {
    let mut pkg = Package::new(PackageName::new("local", "offload", None));

    let mut world = World::new("offload");

    let mut types_intf = Interface::new("types");
    for (name, ty) in &ctx.types {
        types_intf.type_def(TypeDef::record(name.clone(), ty.clone()));
    }

    let fn_name = input.sig.ident.to_string().replace("_", "-");
    let mut func = StandaloneFunc::new(fn_name);

    let params = func.params_mut();
    for param in input.sig.inputs {
        let FnArg::Typed(param) = param else {
            unimplemented!()
        };

        let name = param.pat;
        let ty = ctx
            .to_wit_type(&param.ty)
            .expect(&format!("Failed to translate type {:?}", param.ty));
        params.push(quote! {#name}.to_string(), ty);
    }

    if let ReturnType::Type(_, ty) = input.sig.output {
        func.set_results(
            ctx.to_wit_type(&ty)
                .expect(&format!("Failed to translate type {:?}", ty)),
        );
    }

    let func_item = WorldItem::function_export(func);
    let mut use_itm = Use::new("types");
    use_itm.item("point", None);
    world.use_(use_itm);
    world.item(func_item);

    pkg.interface(types_intf);
    world.named_interface_export(WorldNamedInterface::new("types"));

    pkg.world(world);

    pkg.to_string()
}

#[proc_macro_attribute]
pub fn offload(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut ctx = TypeContext::default();
    let attr = syn::parse_macro_input!(attr as ExprAssign);

    let sf = proc_macro::Span::call_site().source_file();
    let src_path = sf.path();

    let left = attr.left.to_token_stream().to_string();
    let right = attr.right.to_token_stream().to_string();
    match left.as_str() {
        "types" => {
            let file = right.trim_matches('"');
            let mut path = src_path
                .parent()
                .expect("Source file has no parent")
                .to_path_buf();
            path.push(file);
            let types_file =
                std::fs::read_to_string(&path).expect(&format!("Could not read `{path:?}`",));
            let types = syn::parse_file(&types_file).expect("Failed to parse types file");
            for item in types.items {
                match item {
                    syn::Item::Enum(item_enum) => todo!(),
                    syn::Item::Struct(item_struct) => {
                        ctx.add_struct_type(item_struct);
                    }
                    syn::Item::Union(item_union) => todo!(),
                    _ => {
                        panic!("Items other than type definitions are not supported in type files")
                    }
                }
            }
        }
        other => {
            panic!("Unrecognized option: `{other}`")
        }
    }

    let input = syn::parse_macro_input!(item as ItemFn);

    if !std::fs::exists(concat!(env!("CARGO_MANIFEST_DIR"), "/offloaded")).unwrap() {
        std::process::Command::new("cargo")
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args(["new", "--lib", "offloaded"])
            .spawn()
            .unwrap()
            .wait()
            .unwrap();
        let mut manifest = std::fs::OpenOptions::new()
            .write(true)
            .open(concat!(env!("CARGO_MANIFEST_DIR"), "/offloaded/Cargo.toml"))
            .unwrap();
        manifest
            .write_all(
                r#"
[package]
name = "offloaded"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]

[workspace]
"#
                .as_bytes(),
            )
            .unwrap();
        std::process::Command::new("cargo")
            .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/offloaded"))
            .args(["add", "wit-bindgen"])
            .spawn()
            .unwrap()
            .wait()
            .unwrap();
    }
    std::fs::write(
        concat!(env!("CARGO_MANIFEST_DIR"), "/offloaded/src/lib.rs"),
        create_component_source(input.clone()).to_string(),
    )
    .unwrap();
    std::fs::create_dir_all(concat!(env!("CARGO_MANIFEST_DIR"), "/offloaded/wit")).unwrap();
    std::fs::write(
        concat!(env!("CARGO_MANIFEST_DIR"), "/offloaded/wit/offloaded.wit"),
        create_wit_bindings(&mut ctx, input.clone()),
    )
    .unwrap();
    std::process::Command::new("cargo")
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/offloaded"))
        .args(["build", "--release", "--target", "wasm32-wasip2"])
        .spawn()
        .unwrap()
        .wait()
        .unwrap();

    let wasm_output = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/offloaded/target/wasm32-wasip2/release/offloaded.wasm"
    ))
    .unwrap();

    // std::fs::remove_dir_all(concat!(env!("CARGO_MANIFEST_DIR"), "/offloaded")).unwrap();
    let impls = ctx.types.iter().map(|(ty, f)| impl_ty_into_val((ty, f)));

    let fn_sig = input.sig;
    let fn_name = fn_sig.ident;
    let fn_name_str = fn_name.to_string().replace("_", "-");
    let fn_args = fn_sig.inputs;
    let fn_params = fn_args.iter().map(|p| match p {
        syn::FnArg::Receiver(_) => panic!("Cannot offload methods"),
        syn::FnArg::Typed(t) => &t.pat,
    });

    let output = match fn_sig.output {
        ReturnType::Default => quote! {
            #(#impls)*

            pub fn #fn_name(#fn_args) -> Result<(), Box<dyn std::error::Error>> {
                let wasm = vec![#(#wasm_output),*];
                let res = OFFLOADER.lock()?.call_function(&wasm, #fn_name_str, &[#(Val::from(#fn_params)),*], false)?;
                Ok(())
            }
        },
        ReturnType::Type(_, _) => {
            quote! {
                #(#impls)*

                pub fn #fn_name(#fn_args) -> Result<wasm_offload::Val, Box<dyn std::error::Error>> {
                    let wasm = vec![#(#wasm_output),*];
                    let res = OFFLOADER.lock()?.call_function(&wasm, #fn_name_str, &[#(#fn_params.into()),*], true)?;
                    // let res = OFFLOADER.lock()?.call_function(&wasm, #fn_name_str, &[#(wasm_offload::Val::from(#fn_params)),*], true)?;
                    Ok(res.unwrap())
                }
            }
        }
    };

    TokenStream::from(output)
}
