use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Ident, ItemFn};

/// Spawn variant determined by the macro attribute.
enum SpawnKind {
    /// `task.spawn(name, move |task| async move { .. }).await`
    Async,
    /// `task.spawn_sync(name, move |task| { .. })`
    Sync,
    /// `task.spawn_tokio(name, move |task| async move { .. }).await`
    Tokio,
    /// `task.spawn_blocking(name, move |task| { .. }).await`
    Blocking,
}

/// Parsed `#[task(...)]` attributes.
struct TaskAttr {
    kind: SpawnKind,
    data_args: Vec<Ident>,
    name_override: Option<String>,
}

impl TaskAttr {
    fn parse(attr: TokenStream) -> syn::Result<Self> {
        let mut kind = SpawnKind::Async;
        let mut data_args = Vec::new();
        let mut name_override = None;

        if attr.is_empty() {
            return Ok(Self {
                kind,
                data_args,
                name_override,
            });
        }

        // Parse as a comma-separated list of meta items
        let parsed = syn::parse::Parser::parse(
            |input: syn::parse::ParseStream| {
                let items =
                    syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated(
                        input,
                    )?;
                Ok(items)
            },
            attr,
        )?;

        for meta in parsed {
            match &meta {
                // #[task(sync)], #[task(tokio)], #[task(blocking)]
                syn::Meta::Path(path) => {
                    if path.is_ident("sync") {
                        kind = SpawnKind::Sync;
                    } else if path.is_ident("tokio") {
                        kind = SpawnKind::Tokio;
                    } else if path.is_ident("blocking") {
                        kind = SpawnKind::Blocking;
                    } else {
                        return Err(syn::Error::new_spanned(
                            path,
                            "expected `sync`, `tokio`, or `blocking`",
                        ));
                    }
                }
                // #[task(data(a, b, c))]
                syn::Meta::List(list) if list.path.is_ident("data") => {
                    let idents = list.parse_args_with(
                        syn::punctuated::Punctuated::<Ident, syn::Token![,]>::parse_terminated,
                    )?;
                    data_args = idents.into_iter().collect();
                }
                // #[task(name = "custom_name")]
                syn::Meta::NameValue(nv) if nv.path.is_ident("name") => {
                    if let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(s),
                        ..
                    }) = &nv.value
                    {
                        name_override = Some(s.value());
                    } else {
                        return Err(syn::Error::new_spanned(
                            &nv.value,
                            "expected a string literal",
                        ));
                    }
                }
                _ => {
                    return Err(syn::Error::new_spanned(
                        &meta,
                        "unexpected attribute, expected `sync`, `tokio`, `blocking`, \
                         `data(...)`, or `name = \"...\"`",
                    ));
                }
            }
        }

        Ok(Self {
            kind,
            data_args,
            name_override,
        })
    }
}

/// Check whether a type (after stripping references) ends in `Task`.
fn is_task_type(ty: &syn::Type) -> bool {
    let mut ty = ty;
    while let syn::Type::Reference(r) = ty {
        ty = &r.elem;
    }
    if let syn::Type::Path(type_path) = ty {
        if let Some(seg) = type_path.path.segments.last() {
            return seg.ident == "Task";
        }
    }
    false
}

/// Find the task parameter (type path ending in `Task`).
/// Errors if zero or multiple Task parameters are found.
fn find_task_param(sig: &syn::Signature) -> syn::Result<&Ident> {
    let mut found: Option<&Ident> = None;

    for param in sig.inputs.iter() {
        if let syn::FnArg::Typed(pat_type) = param {
            if is_task_type(&pat_type.ty) {
                if let syn::Pat::Ident(pat_ident) = &*pat_type.pat {
                    if found.is_some() {
                        return Err(syn::Error::new_spanned(
                            param,
                            "multiple Task parameters found; #[task] requires exactly one",
                        ));
                    }
                    found = Some(&pat_ident.ident);
                }
            }
        }
    }

    found.ok_or_else(|| {
        syn::Error::new_spanned(
            sig,
            "#[task] requires a parameter whose type is `Task` (e.g. `task: &Task`)",
        )
    })
}

/// Wraps a function body in a [`Task::spawn`] call, turning the function into
/// a task in the `ll` task tree.
///
/// The macro looks for a parameter whose type is `Task` (typically `task: &Task`)
/// and uses it as the **parent**. Inside the function body, that same name refers
/// to the **child** task created by the spawn — the parent is shadowed.
///
/// The task name defaults to the function name. The return type must be
/// `Result<T>` (from `anyhow`).
///
/// # Spawn variants
///
/// | Attribute | Method | Function must be |
/// |-----------|--------|-----------------|
/// | `#[task]` | [`Task::spawn`] | `async fn` |
/// | `#[task(sync)]` | [`Task::spawn_sync`] | `fn` |
/// | `#[task(tokio)]` | [`Task::spawn_tokio`] | `async fn` |
/// | `#[task(blocking)]` | [`Task::spawn_blocking`] | `async fn` |
///
/// # Optional attributes
///
/// - **`data(arg1, arg2, ...)`** — emit `task.data("arg1", arg1)` at the top
///   of the task body. Only listed arguments are logged; the task parameter
///   itself cannot be listed.
///
/// - **`name = "custom_name"`** — override the task name (defaults to the
///   function name). Useful when the name contains tags like `#l2` or
///   `#nostatus`.
///
/// Attributes can be combined: `#[task(sync, data(path), name = "check #l2")]`.
///
/// # Examples
///
/// Basic async task — the most common case:
///
/// ```ignore
/// #[task]
/// async fn build(task: &Task) -> Result<()> {
///     task.data("compiler", "rustc 1.78");
///     // ... do work ...
///     Ok(())
/// }
///
/// // caller:
/// build(&parent_task).await?;
/// ```
///
/// Sync task with automatic data logging:
///
/// ```ignore
/// #[task(sync, data(path))]
/// fn check_lockfile(path: &str, task: &Task) -> Result<()> {
///     // `task.data("path", path)` is emitted automatically
///     Ok(())
/// }
/// ```
///
/// Custom task name with tags:
///
/// ```ignore
/// #[task(name = "test #l2")]
/// async fn run_tests(task: &Task) -> Result<()> {
///     // creates a task named "test" with tag "l2"
///     Ok(())
/// }
/// ```
///
/// Blocking task on tokio's thread pool:
///
/// ```ignore
/// #[task(blocking)]
/// async fn compress(data: Vec<u8>, task: &Task) -> Result<Vec<u8>> {
///     // runs on spawn_blocking pool, won't stall the async executor
///     Ok(zstd::compress(&data, 3)?)
/// }
/// ```
///
/// Nested usage — macro-wrapped functions calling each other:
///
/// ```ignore
/// #[task]
/// async fn deploy(task: &Task) -> Result<()> {
///     provision(&task).await?;   // another #[task] fn
///     restart(&task).await?;
///     Ok(())
/// }
///
/// #[task]
/// async fn provision(task: &Task) -> Result<()> {
///     // task tree: deploy > provision
///     Ok(())
/// }
/// ```
#[proc_macro_attribute]
pub fn task(attr: TokenStream, item: TokenStream) -> TokenStream {
    let task_attr = match TaskAttr::parse(attr) {
        Ok(a) => a,
        Err(e) => return e.to_compile_error().into(),
    };

    let mut func = parse_macro_input!(item as ItemFn);

    // Validate async/sync match
    let is_async = func.sig.asyncness.is_some();
    match (&task_attr.kind, is_async) {
        (SpawnKind::Sync, true) => {
            return syn::Error::new_spanned(
                func.sig.fn_token,
                "#[task(sync)] requires a non-async `fn`; remove `async` or use #[task]",
            )
            .to_compile_error()
            .into();
        }
        (SpawnKind::Async | SpawnKind::Tokio | SpawnKind::Blocking, false) => {
            return syn::Error::new_spanned(
                func.sig.fn_token,
                "#[task] requires `async fn`; use #[task(sync)] for synchronous functions",
            )
            .to_compile_error()
            .into();
        }
        _ => {}
    }

    // Find the task parameter
    let task_ident = match find_task_param(&func.sig) {
        Ok(ident) => ident.clone(),
        Err(e) => return e.to_compile_error().into(),
    };

    // Validate data args exist as function parameters
    let param_names: Vec<Ident> = func
        .sig
        .inputs
        .iter()
        .filter_map(|arg| {
            if let syn::FnArg::Typed(pat_type) = arg {
                if let syn::Pat::Ident(pat_ident) = &*pat_type.pat {
                    return Some(pat_ident.ident.clone());
                }
            }
            None
        })
        .collect();

    for data_arg in &task_attr.data_args {
        if !param_names.contains(data_arg) {
            return syn::Error::new_spanned(
                data_arg,
                format!("data arg `{data_arg}` is not a parameter of this function"),
            )
            .to_compile_error()
            .into();
        }
        if *data_arg == task_ident {
            return syn::Error::new_spanned(data_arg, "cannot log the task parameter as data")
                .to_compile_error()
                .into();
        }
    }

    // Task name: override or function name
    let task_name = task_attr
        .name_override
        .unwrap_or_else(|| func.sig.ident.to_string());

    // Generate data logging statements
    let data_stmts: Vec<_> = task_attr
        .data_args
        .iter()
        .map(|arg| {
            let arg_str = arg.to_string();
            quote! { #task_ident.data(#arg_str, #arg); }
        })
        .collect();

    // Original function body
    let body = &func.block;

    // Build the new body based on spawn kind
    let new_body: syn::Block = match task_attr.kind {
        SpawnKind::Async => {
            syn::parse_quote!({
                #task_ident.spawn(#task_name, move |#task_ident| async move {
                    #(#data_stmts)*
                    #body
                }).await
            })
        }
        SpawnKind::Sync => {
            syn::parse_quote!({
                #task_ident.spawn_sync(#task_name, move |#task_ident| {
                    #(#data_stmts)*
                    #body
                })
            })
        }
        SpawnKind::Tokio => {
            syn::parse_quote!({
                #task_ident.spawn_tokio(#task_name, move |#task_ident| async move {
                    #(#data_stmts)*
                    #body
                }).await
            })
        }
        SpawnKind::Blocking => {
            syn::parse_quote!({
                #task_ident.spawn_blocking(#task_name, move |#task_ident| {
                    #(#data_stmts)*
                    #body
                }).await
            })
        }
    };

    *func.block = new_body;

    quote!(#func).into()
}
