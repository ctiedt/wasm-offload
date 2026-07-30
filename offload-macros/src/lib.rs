use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{format_ident, quote, quote_spanned};
use syn::parse::Parser;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{
    Data, DeriveInput, Fields, FnArg, ItemFn, LitStr, ReturnType, Type, parse_quote,
    parse_quote_spanned,
};

#[derive(Default)]
struct OffloadOptions {
    export: Option<LitStr>,
    deny_floats: bool,
    try_mode: bool,
}

#[proc_macro_attribute]
pub fn offload(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut options = OffloadOptions::default();
    let parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("export") {
            if options.export.is_some() {
                return Err(meta.error("duplicate `export` option"));
            }
            options.export = Some(meta.value()?.parse()?);
            return Ok(());
        }
        if meta.path.is_ident("deny_floats") {
            if options.deny_floats {
                return Err(meta.error("duplicate `deny_floats` option"));
            }
            options.deny_floats = true;
            return Ok(());
        }
        if meta.path.is_ident("try") {
            if options.try_mode {
                return Err(meta.error("duplicate `try` option"));
            }
            options.try_mode = true;
            return Ok(());
        }
        Err(meta.error(
            "unsupported offload option; valid options are `export = \"...\"`, `deny_floats`, and `try`",
        ))
    });
    if let Err(error) = parser.parse(attr) {
        return error.into_compile_error().into();
    }

    let function = match syn::parse::<ItemFn>(item) {
        Ok(function) => function,
        Err(error) => return error.into_compile_error().into(),
    };
    expand_offload(function, options)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn expand_offload(
    function: ItemFn,
    options: OffloadOptions,
) -> syn::Result<proc_macro2::TokenStream> {
    validate_signature(&function, &options)?;

    let attrs = &function.attrs;
    let visibility = &function.vis;
    let original_name = &function.sig.ident;
    let export_name = options
        .export
        .as_ref()
        .map(LitStr::value)
        .unwrap_or_else(|| format!("{}{}", offload_core::EXPORT_PREFIX, original_name));
    let export_literal = LitStr::new(
        &export_name,
        options
            .export
            .as_ref()
            .map_or_else(Span::call_site, LitStr::span),
    );
    let wrapper_name = format_ident!("{}{}", offload_core::EXPORT_PREFIX, original_name);

    const RESERVED_EXPORTS: [&str; 4] = [
        offload_core::ALLOC_EXPORT,
        offload_core::FREE_EXPORT,
        offload_core::ABI_VERSION_EXPORT,
        offload_core::MEMORY_EXPORT,
    ];
    if RESERVED_EXPORTS.contains(&export_name.as_str()) {
        return Err(syn::Error::new(
            export_literal.span(),
            format!("export name `{export_name}` is reserved by the offload guest runtime"),
        ));
    }

    let argument_types: Vec<Type> = function
        .sig
        .inputs
        .iter()
        .map(|argument| match argument {
            FnArg::Typed(argument) => (*argument.ty).clone(),
            FnArg::Receiver(_) => unreachable!("receiver rejected by validation"),
        })
        .collect();
    let return_type = declared_return_type(&function.sig.output);
    let argument_names: Vec<_> = (0..argument_types.len())
        .map(|index| format_ident!("__offload_arg_{index}"))
        .collect();

    let normalized = quote!(fn(#(#argument_types),*) -> #return_type).to_string();
    let signature_hash = offload_core::sig_hash(normalized.as_bytes());
    let record = offload_core::ManifestRecord::new(
        export_name.clone(),
        offload_core::ABI_VERSION,
        signature_hash,
    );
    let manifest = postcard::to_allocvec(&record).map_err(|error| {
        syn::Error::new(
            original_name.span(),
            format!("failed to encode offload manifest: {error}"),
        )
    })?;
    let manifest_len = manifest.len();
    let manifest_bytes = manifest.iter();
    let manifest_section = LitStr::new(offload_core::MANIFEST_SECTION, Span::call_site());

    let tuple_type = tuple_tokens(&argument_types);
    let argument_tuple = tuple_tokens(&argument_names);
    let invoke = if matches!(&function.sig.safety, syn::Safety::Unsafe(_)) {
        quote!(unsafe { #original_name(#(#argument_names),*) })
    } else {
        quote!(#original_name(#(#argument_names),*))
    };
    let guest_invoke = if options.try_mode {
        quote!(#invoke.expect("offload: `try` guest function unexpectedly returned an infrastructure error. If you see this, something is seriously wrong"))
    } else {
        invoke
    };
    let export_attribute = if options.export.is_some() {
        quote!(#[unsafe(export_name = #export_literal)])
    } else {
        quote!(#[unsafe(no_mangle)])
    };

    let mut guest_signature = function.sig.clone();
    let guest_body = if options.try_mode {
        guest_signature.output = result_return_type(&return_type);
        let body = &function.block;
        quote!({
            ::core::result::Result::Ok((|| -> #return_type #body)())
        })
    } else {
        let body = &function.block;
        quote!(#body)
    };

    let mut host_signature = function.sig.clone();
    for (argument, name) in host_signature.inputs.iter_mut().zip(&argument_names) {
        if let FnArg::Typed(argument) = argument {
            *argument.pat = parse_quote!(#name);
        }
    }
    if options.try_mode {
        host_signature.output = result_return_type(&return_type);
    }

    let host_body = if options.try_mode {
        quote!({
            const SIG: u64 = #signature_hash;
            ::offload::__private::host::call_checked::<_, #return_type>(
                #export_literal,
                SIG,
                &#argument_tuple,
            )
        })
    } else {
        let display_name = original_name.to_string();
        quote!({
            const SIG: u64 = #signature_hash;
            match ::offload::__private::host::call_checked::<_, #return_type>(
                #export_literal,
                SIG,
                &#argument_tuple,
            ) {
                ::core::result::Result::Ok(value) => value,
                ::core::result::Result::Err(error) => {
                    panic!("offload call `{}` failed: {}", #display_name, error)
                }
            }
        })
    };

    let boundary_checks =
        compatibility_assertions(&argument_types, &return_type, options.deny_floats);

    Ok(quote! {
        #boundary_checks

        #(#attrs)*
        #[cfg(target_arch = "wasm32")]
        #visibility #guest_signature #guest_body

        #[cfg(target_arch = "wasm32")]
        const _: () = {
            #export_attribute
            pub extern "C" fn #wrapper_name(ptr: u32, len: u32) -> u64 {
                ::offload::__private::guest::entry(
                    ptr,
                    len,
                    |#argument_tuple: #tuple_type| #guest_invoke,
                )
            }

            #[used]
            #[unsafe(link_section = #manifest_section)]
            static MANIFEST: [u8; #manifest_len] = [#(#manifest_bytes),*];
        };

        #(#attrs)*
        #[cfg(not(target_arch = "wasm32"))]
        #visibility #host_signature #host_body
    })
}

fn validate_signature(function: &ItemFn, options: &OffloadOptions) -> syn::Result<()> {
    let signature = &function.sig;
    if let Some(constness) = signature.constness {
        return Err(syn::Error::new(
            constness.span(),
            "`#[offload]` does not support `const fn` because host calls require a runtime",
        ));
    }
    if let Some(asyncness) = signature.asyncness {
        return Err(syn::Error::new(
            asyncness.span(),
            "`#[offload]` does not support async functions at the moment",
        ));
    }
    if let Some(abi) = &signature.abi {
        return Err(syn::Error::new(
            abi.span(),
            "`#[offload]` does not support extern functions",
        ));
    }
    if let Some(variadic) = &signature.variadic {
        return Err(syn::Error::new(
            variadic.span(),
            "`#[offload]` does not support variadic functions",
        ));
    }
    if !signature.generics.params.is_empty() || signature.generics.where_clause.is_some() {
        let span = signature
            .generics
            .lt_token
            .as_ref()
            .map(Spanned::span)
            .or_else(|| {
                signature
                    .generics
                    .where_clause
                    .as_ref()
                    .map(|clause| clause.where_token.span())
            })
            .unwrap_or_else(|| signature.generics.span());
        return Err(syn::Error::new(
            span,
            "`#[offload]` does not support generic functions or where clauses",
        ));
    }

    for argument in &signature.inputs {
        match argument {
            FnArg::Receiver(receiver) => {
                return Err(syn::Error::new(
                    receiver.span(),
                    "`#[offload]` only supports standalone functions; methods with `self` are not supported",
                ));
            }
            FnArg::Typed(argument) => validate_boundary_type(&argument.ty, options)?,
        }
    }
    if let ReturnType::Type(_, ty) = &signature.output {
        validate_boundary_type(ty, options)?;
    }
    Ok(())
}

#[derive(Default)]
struct TypeInspection {
    reference: Option<Span>,
    slice: Option<Span>,
    impl_trait: Option<Span>,
    pointer_sized_integer: Option<(Span, String)>,
    float: Option<(Span, String)>,
}

impl<'ast> Visit<'ast> for TypeInspection {
    fn visit_type_reference(&mut self, node: &'ast syn::TypeReference) {
        self.reference.get_or_insert(node.and_token.span());
        visit::visit_type_reference(self, node);
    }

    fn visit_type_slice(&mut self, node: &'ast syn::TypeSlice) {
        self.slice.get_or_insert(node.bracket_token.span.open());
        visit::visit_type_slice(self, node);
    }

    fn visit_type_impl_trait(&mut self, node: &'ast syn::TypeImplTrait) {
        self.impl_trait.get_or_insert(node.impl_token.span());
        visit::visit_type_impl_trait(self, node);
    }

    fn visit_type_path(&mut self, node: &'ast syn::TypePath) {
        if node.qself.is_none()
            && let Some(segment) = node.path.segments.last()
        {
            let name = segment.ident.to_string();
            if matches!(name.as_str(), "usize" | "isize") {
                self.pointer_sized_integer
                    .get_or_insert((segment.ident.span(), name.clone()));
            }
            if matches!(name.as_str(), "f32" | "f64") {
                self.float.get_or_insert((segment.ident.span(), name));
            }
        }
        visit::visit_type_path(self, node);
    }
}

fn validate_boundary_type(ty: &Type, options: &OffloadOptions) -> syn::Result<()> {
    let mut inspection = TypeInspection::default();
    inspection.visit_type(ty);
    if let Some(span) = inspection.reference {
        return Err(syn::Error::new(
            span,
            "references cannot cross the offload boundary; use owned data instead",
        ));
    }
    if let Some(span) = inspection.slice {
        return Err(syn::Error::new(
            span,
            "slice types cannot cross the offload boundary; use `Vec<T>`",
        ));
    }
    if let Some(span) = inspection.impl_trait {
        return Err(syn::Error::new(
            span,
            "`impl Trait` cannot appear in an offload signature; use a concrete owned type",
        ));
    }
    if let Some((span, name)) = inspection.pointer_sized_integer {
        return Err(syn::Error::new(
            span,
            format!(
                "`{name}` cannot cross the 64-bit host / 32-bit guest boundary; use `u32`, `u64`, `i32`, or `i64`"
            ),
        ));
    }
    if options.deny_floats
        && let Some((span, name)) = inspection.float
    {
        return Err(syn::Error::new(
            span,
            format!("`{name}` cannot cross an AN-compatible offload boundary"),
        ));
    }
    Ok(())
}

fn declared_return_type(output: &ReturnType) -> Type {
    match output {
        ReturnType::Default => parse_quote!(()),
        ReturnType::Type(_, ty) => (**ty).clone(),
    }
}

fn result_return_type(value: &Type) -> ReturnType {
    parse_quote!(-> ::core::result::Result<#value, ::offload::OffloadError>)
}

fn tuple_tokens<T: quote::ToTokens>(items: &[T]) -> proc_macro2::TokenStream {
    match items {
        [] => quote!(()),
        [one] => quote!((#one,)),
        many => quote!((#(#many),*)),
    }
}

fn compatibility_assertions(
    argument_types: &[Type],
    return_type: &Type,
    deny_floats: bool,
) -> proc_macro2::TokenStream {
    let all_types = argument_types.iter().chain(core::iter::once(return_type));
    let boundary = all_types.clone().map(|ty| {
        quote_spanned!(ty.span()=>
            let _ = assert_boundary::<#ty> as fn();
        )
    });
    let an = if deny_floats {
        let checks = all_types.map(|ty| {
            quote_spanned!(ty.span()=>
                let _ = assert_an::<#ty> as fn();
            )
        });
        quote! {
            fn assert_an<T: ::offload::AnCompatible>() {}
            #(#checks)*
        }
    } else {
        quote!()
    };

    quote! {
        const _: () = {
            fn assert_boundary<T: ::offload::BoundaryCompatible>() {}
            #(#boundary)*
            #an
        };
    }
}

#[proc_macro]
pub fn include_guest(input: TokenStream) -> TokenStream {
    expand_include_guest(input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn expand_include_guest(input: proc_macro2::TokenStream) -> syn::Result<proc_macro2::TokenStream> {
    let (variable, hint) = if input.is_empty() {
        (
            offload_core::GUEST_PATH_ENV.to_string(),
            "is offload_build::GuestBuilder::new(..).build() called in this crate's build.rs?"
                .to_string(),
        )
    } else if let Ok(package) = syn::parse2::<LitStr>(input.clone()) {
        let suffix = offload_core::guest_path_env_suffix(&package.value());
        (
            format!("{}_{suffix}", offload_core::GUEST_PATH_ENV),
            format!(
                "is offload_build::GuestBuilder::new(\"{}\").build() called in this crate's build.rs?",
                package.value()
            ),
        )
    } else {
        struct ArtifactArg(LitStr);
        impl syn::parse::Parse for ArtifactArg {
            fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
                let keyword: syn::Ident = input.parse()?;
                if keyword != "artifact" {
                    return Err(syn::Error::new(
                        keyword.span(),
                        "expected `include_guest!()`, `include_guest!(\"package\")`, \
                         or `include_guest!(artifact = \"dependency\")`",
                    ));
                }
                input.parse::<syn::Token![=]>()?;
                Ok(Self(input.parse()?))
            }
        }
        let dependency = syn::parse2::<ArtifactArg>(input)?.0;
        let suffix = offload_core::guest_path_env_suffix(&dependency.value());
        (
            format!("CARGO_CDYLIB_FILE_{suffix}"),
            format!(
                "is `{}` declared as an `artifact = \"cdylib\"` dependency? (requires -Z bindeps)",
                dependency.value()
            ),
        )
    };
    let message = format!("offload guest artifact not found ({variable} is unset): {hint}");
    Ok(quote! {
        ::core::include_bytes!(::core::env!(#variable, #message))
    })
}

#[proc_macro]
pub fn init_guest(input: TokenStream) -> TokenStream {
    expand_init_guest(input.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn expand_init_guest(input: proc_macro2::TokenStream) -> syn::Result<proc_macro2::TokenStream> {
    struct InitGuestArgs {
        guest: proc_macro2::TokenStream,
        options: Vec<(syn::Ident, syn::Expr)>,
    }
    impl syn::parse::Parse for InitGuestArgs {
        fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
            let mut guest = proc_macro2::TokenStream::new();
            let mut options = Vec::new();
            let mut first = true;
            while !input.is_empty() {
                if !first {
                    input.parse::<syn::Token![,]>()?;
                    if input.is_empty() {
                        break;
                    }
                }
                if first && input.peek(LitStr) {
                    let package: LitStr = input.parse()?;
                    guest = quote!(#package);
                } else {
                    let name: syn::Ident = input.parse()?;
                    input.parse::<syn::Token![=]>()?;
                    if first && name == "artifact" {
                        let dependency: LitStr = input.parse()?;
                        guest = quote!(#name = #dependency);
                    } else {
                        let value: syn::Expr = input.parse()?;
                        options.push((name, value));
                    }
                }
                first = false;
            }
            Ok(Self { guest, options })
        }
    }

    let args = syn::parse2::<InitGuestArgs>(input)?;
    let bytes = expand_include_guest(args.guest)?;
    let methods = args
        .options
        .iter()
        .map(|(name, value)| quote!(.#name(#value)));
    Ok(quote! {
        {
            #[cfg(not(target_arch = "wasm32"))]
            {
                ::offload::Offloader::builder(#bytes)
                    #(#methods)*
                    .build()
                    .and_then(::offload::init)
            }

            #[cfg(target_arch = "wasm32")]
            {
                ::core::result::Result::<(), ::offload::OffloadError>::Ok(())
            }
        }
    })
}

#[proc_macro_derive(AnCompatible)]
pub fn derive_an_compatible(item: TokenStream) -> TokenStream {
    let input = match syn::parse::<DeriveInput>(item) {
        Ok(input) => input,
        Err(error) => return error.into_compile_error().into(),
    };
    expand_an_compatible(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn expand_an_compatible(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let field_types: Vec<Type> = match &input.data {
        Data::Struct(data) => fields(&data.fields).cloned().collect(),
        Data::Enum(data) => data
            .variants
            .iter()
            .flat_map(|variant| fields(&variant.fields))
            .cloned()
            .collect(),
        Data::Union(data) => {
            return Err(syn::Error::new(
                data.union_token.span(),
                "`AnCompatible` cannot be derived for unions",
            ));
        }
    };

    for ty in &field_types {
        let mut inspection = TypeInspection::default();
        inspection.visit_type(ty);
        if let Some((span, name)) = inspection.float {
            return Err(syn::Error::new(
                span,
                format!(
                    "`AnCompatible` cannot be derived for a field containing `{name}`; \
                     floating-point values are not supported by AN encoding"
                ),
            ));
        }
        if let Some((span, name)) = inspection.pointer_sized_integer {
            return Err(syn::Error::new(
                span,
                format!(
                    "`AnCompatible` cannot be derived for a field containing `{name}`; \
                     use a fixed-width integer"
                ),
            ));
        }
    }

    let name = &input.ident;
    let mut generics = input.generics.clone();
    let where_clause = generics.make_where_clause();
    for ty in field_types {
        where_clause
            .predicates
            .push(parse_quote_spanned!(ty.span()=> #ty: ::offload::AnCompatible));
    }
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    Ok(quote! {
        impl #impl_generics ::offload::AnCompatible for #name #type_generics #where_clause {}
    })
}

fn fields(fields: &Fields) -> impl Iterator<Item = &Type> {
    fields.iter().map(|field| &field.ty)
}
