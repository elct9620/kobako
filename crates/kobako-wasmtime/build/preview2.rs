//! The generator behind `build.rs`: each WIT function a Guest Binary imports
//! becomes one core `Linker` function, lifting its arguments from the core
//! values and linear memory, calling the `Host` trait, and lowering the
//! answer back, as the Canonical ABI's guest-import side has the host do.
//! `wit-bindgen-core` walks the ABI; this file only says how each step is
//! spelled in Rust.

use std::fmt::Write as _;
use std::path::Path;

use wit_bindgen_core::abi::{self, Bindgen, Instruction, LiftLower, WasmType};
use wit_bindgen_core::wit_parser::{
    Alignment, ArchitectureSize, Function, FunctionKind, Handle, InterfaceId, Resolve, SizeAlign,
    Type, TypeDefKind, TypeId, TypeOwner,
};

/// The packages vendored under `wit/deps`, each depending only on those
/// before it.
const PACKAGES: [&str; 7] = [
    "io.wit",
    "clocks.wit",
    "random.wit",
    "filesystem.wit",
    "sockets.wit",
    "cli.wit",
    "http.wit",
];

/// The functions kobako answers, each interface's by its WIT name, resource
/// drops included: those a Guest Binary imports, and those a scenario holds
/// an answer to. No other links, so a call no answer was written for fails to
/// link rather than to run.
const IMPORTS: &[(&str, &[&str])] = &[
    (
        "wasi:io/error@0.2.12",
        &["[method]error.to-debug-string", "[resource-drop]error"],
    ),
    (
        "wasi:io/poll@0.2.12",
        &["[method]pollable.block", "[resource-drop]pollable", "poll"],
    ),
    (
        "wasi:io/streams@0.2.12",
        &[
            "[method]input-stream.blocking-read",
            "[method]input-stream.read",
            "[method]input-stream.subscribe",
            "[method]output-stream.blocking-write-and-flush",
            "[method]output-stream.check-write",
            "[method]output-stream.write",
            "[method]output-stream.flush",
            "[method]output-stream.subscribe",
            "[resource-drop]input-stream",
            "[resource-drop]output-stream",
        ],
    ),
    (
        "wasi:clocks/monotonic-clock@0.2.12",
        &["subscribe-duration"],
    ),
    ("wasi:sockets/network@0.2.12", &["[resource-drop]network"]),
    (
        "wasi:sockets/instance-network@0.2.12",
        &["instance-network"],
    ),
    (
        "wasi:sockets/tcp-create-socket@0.2.12",
        &["create-tcp-socket"],
    ),
    (
        "wasi:sockets/tcp@0.2.12",
        &[
            "[method]tcp-socket.start-bind",
            "[method]tcp-socket.finish-bind",
            "[method]tcp-socket.start-connect",
            "[method]tcp-socket.finish-connect",
            "[method]tcp-socket.shutdown",
            "[method]tcp-socket.subscribe",
            "[method]tcp-socket.local-address",
            "[method]tcp-socket.remote-address",
            "[method]tcp-socket.keep-alive-enabled",
            "[method]tcp-socket.set-keep-alive-enabled",
            "[method]tcp-socket.keep-alive-idle-time",
            "[method]tcp-socket.set-keep-alive-idle-time",
            "[method]tcp-socket.keep-alive-interval",
            "[method]tcp-socket.set-keep-alive-interval",
            "[method]tcp-socket.keep-alive-count",
            "[method]tcp-socket.set-keep-alive-count",
            "[method]tcp-socket.receive-buffer-size",
            "[method]tcp-socket.set-receive-buffer-size",
            "[method]tcp-socket.send-buffer-size",
            "[method]tcp-socket.set-send-buffer-size",
            "[method]tcp-socket.hop-limit",
            "[method]tcp-socket.set-hop-limit",
            "[resource-drop]tcp-socket",
        ],
    ),
    (
        "wasi:sockets/ip-name-lookup@0.2.12",
        &[
            "resolve-addresses",
            "[method]resolve-address-stream.resolve-next-address",
            "[method]resolve-address-stream.subscribe",
            "[resource-drop]resolve-address-stream",
        ],
    ),
    (
        "wasi:sockets/udp-create-socket@0.2.12",
        &["create-udp-socket"],
    ),
    (
        "wasi:sockets/udp@0.2.12",
        &[
            "[method]udp-socket.start-bind",
            "[method]udp-socket.finish-bind",
            "[method]udp-socket.stream",
            "[method]udp-socket.subscribe",
            "[method]udp-socket.local-address",
            "[method]udp-socket.unicast-hop-limit",
            "[method]udp-socket.set-unicast-hop-limit",
            "[method]udp-socket.receive-buffer-size",
            "[method]udp-socket.set-receive-buffer-size",
            "[method]udp-socket.send-buffer-size",
            "[method]udp-socket.set-send-buffer-size",
            "[method]incoming-datagram-stream.receive",
            "[method]incoming-datagram-stream.subscribe",
            "[method]outgoing-datagram-stream.check-send",
            "[method]outgoing-datagram-stream.send",
            "[method]outgoing-datagram-stream.subscribe",
            "[resource-drop]udp-socket",
            "[resource-drop]incoming-datagram-stream",
            "[resource-drop]outgoing-datagram-stream",
        ],
    ),
    (
        "wasi:http/types@0.2.12",
        &[
            "[constructor]fields",
            "[method]fields.append",
            "[method]fields.entries",
            "[constructor]outgoing-request",
            "[method]outgoing-request.body",
            "[method]outgoing-request.set-method",
            "[method]outgoing-request.set-path-with-query",
            "[method]outgoing-request.set-scheme",
            "[method]outgoing-request.set-authority",
            "[method]outgoing-body.write",
            "[static]outgoing-body.finish",
            "[method]future-incoming-response.subscribe",
            "[method]future-incoming-response.get",
            "[method]incoming-response.status",
            "[method]incoming-response.headers",
            "[method]incoming-response.consume",
            "[method]incoming-body.stream",
            "[static]incoming-body.finish",
            "[resource-drop]fields",
            "[resource-drop]outgoing-request",
            "[resource-drop]outgoing-body",
            "[resource-drop]future-incoming-response",
            "[resource-drop]incoming-response",
            "[resource-drop]incoming-body",
            "[resource-drop]future-trailers",
        ],
    ),
    ("wasi:http/outgoing-handler@0.2.12", &["handle"]),
];

/// The host type each linked WIT resource's handle stands for, as `bindgen!`
/// takes `with:`, keyed by interface and resource name.
const WITH: &[(&str, &str, &str)] = &[
    ("wasi:io/error@0.2.12", "error", "table::Error"),
    ("wasi:io/poll@0.2.12", "pollable", "table::Pollable"),
    (
        "wasi:io/streams@0.2.12",
        "input-stream",
        "table::InputStream",
    ),
    (
        "wasi:io/streams@0.2.12",
        "output-stream",
        "table::OutputStream",
    ),
    ("wasi:sockets/network@0.2.12", "network", "table::Network"),
    ("wasi:sockets/tcp@0.2.12", "tcp-socket", "table::TcpSocket"),
    (
        "wasi:sockets/ip-name-lookup@0.2.12",
        "resolve-address-stream",
        "table::ResolveAddressStream",
    ),
    ("wasi:sockets/udp@0.2.12", "udp-socket", "table::UdpSocket"),
    (
        "wasi:sockets/udp@0.2.12",
        "incoming-datagram-stream",
        "table::IncomingDatagramStream",
    ),
    (
        "wasi:sockets/udp@0.2.12",
        "outgoing-datagram-stream",
        "table::OutgoingDatagramStream",
    ),
    ("wasi:http/types@0.2.12", "fields", "http::resource::Fields"),
    (
        "wasi:http/types@0.2.12",
        "outgoing-request",
        "http::resource::OutgoingRequest",
    ),
    (
        "wasi:http/types@0.2.12",
        "outgoing-body",
        "http::resource::OutgoingBody",
    ),
    (
        "wasi:http/types@0.2.12",
        "future-incoming-response",
        "http::resource::FutureIncomingResponse",
    ),
    (
        "wasi:http/types@0.2.12",
        "incoming-response",
        "http::resource::IncomingResponse",
    ),
    (
        "wasi:http/types@0.2.12",
        "incoming-body",
        "http::resource::IncomingBody",
    ),
    (
        "wasi:http/types@0.2.12",
        "future-trailers",
        "http::resource::FutureTrailers",
    ),
];

/// The host type each WIT error stands for, as `bindgen!` takes
/// `trappable_error_type:`, keyed by interface and type name: a function
/// answering a lone `result<_, e>` of one returns it, so its host answers the
/// error or traps through one `?`.
const TRAPPABLE: &[(&str, &str, &str)] = &[
    ("wasi:io/streams@0.2.12", "stream-error", "StreamError"),
    ("wasi:sockets/network@0.2.12", "error-code", "SocketError"),
    ("wasi:http/types@0.2.12", "error-code", "HttpError"),
    ("wasi:http/types@0.2.12", "header-error", "HeaderError"),
];

/// Where the generated file is included, so every type has one path.
const ROOT: &str = "crate::preview2::bindings";

/// Where `WITH`'s host types and the `Handle` trait live.
const HOSTS: &str = "crate::preview2";

pub fn generate(wit: &Path) -> anyhow::Result<String> {
    let mut resolve = Resolve::default();
    for package in PACKAGES {
        resolve.push_file(wit.join(package))?;
    }
    let mut sizes = SizeAlign::default();
    sizes.fill(&resolve)?;

    let imports = IMPORTS
        .iter()
        .map(|(name, functions)| {
            let id = resolve
                .interfaces
                .iter()
                .find(|(id, _)| resolve.id_of(*id).as_deref() == Some(*name))
                .map(|(id, _)| id)
                .ok_or_else(|| anyhow::anyhow!("{name} is not in the vendored WIT"))?;
            let found = functions_of(&resolve, id, functions).count()
                + resources_of(&resolve, id, functions).count();
            if found != functions.len() {
                return Err(anyhow::anyhow!("{name} lacks a function IMPORTS names"));
            }
            match resources_of(&resolve, id, functions)
                .find(|(r, _)| host_by_resource_of(name, r).is_none())
            {
                Some((r, _)) => Err(anyhow::anyhow!("WITH names no host type for {name} {r}")),
                None => Ok((id, *functions)),
            }
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    let mut out = String::new();
    out.push_str("// Generated by build.rs from wit/deps; do not edit.\n\n");
    for (id, _) in &imports {
        write_types(&resolve, *id, &mut out);
    }
    write_host_trait(&resolve, &imports, &mut out);
    write_linker(&resolve, &sizes, &imports, &mut out);
    Ok(out)
}

/// The host type `WITH` names for `interface`'s `resource`.
fn host_by_resource_of(interface: &str, resource: &str) -> Option<&'static str> {
    WITH.iter()
        .find(|(i, r, _)| *i == interface && *r == resource)
        .map(|(_, _, host)| *host)
}

/// The module one interface's types live in.
fn module_of(resolve: &Resolve, id: InterfaceId) -> String {
    to_snake(&resolve.id_of(id).expect("a named interface"))
}

fn to_snake(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        match c {
            'a'..='z' | '0'..='9' => out.push(c),
            'A'..='Z' => out.push(c.to_ascii_lowercase()),
            _ if out.ends_with('_') || out.is_empty() => {}
            _ => out.push('_'),
        }
    }
    let out = out.trim_end_matches('_').to_string();
    match out.as_str() {
        // A method's receiver, which `self` already names in the trait.
        "self" => "this".into(),
        "type" | "in" | "loop" | "move" | "ref" | "use" | "match" | "mod" => format!("r#{out}"),
        _ => out,
    }
}

fn to_camel(name: &str) -> String {
    name.split(['-', '_', '.'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            let first = chars.next().expect("a part").to_ascii_uppercase();
            std::iter::once(first).chain(chars).collect::<String>()
        })
        .collect()
}

/// The Rust path of a named WIT type.
fn type_path_of(resolve: &Resolve, id: TypeId) -> String {
    let def = &resolve.types[id];
    let name = to_camel(def.name.as_deref().expect("only named types have a path"));
    match def.owner {
        TypeOwner::Interface(owner) => format!("{ROOT}::{}::{name}", module_of(resolve, owner)),
        _ => unreachable!("every imported type belongs to an interface"),
    }
}

/// How a WIT type is spelled in Rust.
fn to_rust_type(resolve: &Resolve, ty: &Type) -> String {
    match ty {
        Type::Bool => "bool".into(),
        Type::U8 => "u8".into(),
        Type::U16 => "u16".into(),
        Type::U32 => "u32".into(),
        Type::U64 => "u64".into(),
        Type::S8 => "i8".into(),
        Type::S16 => "i16".into(),
        Type::S32 => "i32".into(),
        Type::S64 => "i64".into(),
        Type::String => "String".into(),
        Type::Id(id) => match resolve.types[*id].name {
            Some(_) => type_path_of(resolve, *id),
            None => to_structural_type(resolve, &resolve.types[*id].kind),
        },
        other => unreachable!("no import kobako answers takes {other:?}"),
    }
}

/// The type `id` names, through every `use` alias of it.
fn definition_of(resolve: &Resolve, mut id: TypeId) -> TypeId {
    while let TypeDefKind::Type(Type::Id(aliased)) = resolve.types[id].kind {
        id = aliased;
    }
    id
}

/// How a type made of other types is spelled, named or not.
fn to_structural_type(resolve: &Resolve, kind: &TypeDefKind) -> String {
    match kind {
        TypeDefKind::Handle(Handle::Own(r) | Handle::Borrow(r)) => {
            type_path_of(resolve, definition_of(resolve, *r))
        }
        TypeDefKind::List(t) => format!("Vec<{}>", to_rust_type(resolve, t)),
        TypeDefKind::Option(t) => format!("Option<{}>", to_rust_type(resolve, t)),
        TypeDefKind::Result(r) => format!(
            "Result<{}, {}>",
            r.ok.as_ref()
                .map_or("()".into(), |t| to_rust_type(resolve, t)),
            r.err
                .as_ref()
                .map_or("()".into(), |t| to_rust_type(resolve, t)),
        ),
        TypeDefKind::Tuple(t) => format!(
            "({})",
            t.types
                .iter()
                .map(|t| format!("{},", to_rust_type(resolve, t)))
                .collect::<String>()
        ),
        TypeDefKind::Type(t) => to_rust_type(resolve, t),
        other => unreachable!("no 0.2 import takes {other:?}"),
    }
}

/// Each named type an interface declares.
fn write_types(resolve: &Resolve, id: InterfaceId, out: &mut String) {
    let interface = &resolve.interfaces[id];
    let _ = writeln!(
        out,
        "#[allow(dead_code, clippy::all)]\npub mod {} {{",
        module_of(resolve, id)
    );
    let wit_name = resolve.id_of(id).expect("a named interface");
    for (wit, ty) in &interface.types {
        let name = to_camel(wit);
        match &resolve.types[*ty].kind {
            TypeDefKind::Record(record) => {
                let _ = writeln!(
                    out,
                    "#[derive(Clone, Debug, PartialEq)]\npub struct {name} {{"
                );
                for field in &record.fields {
                    let _ = writeln!(
                        out,
                        "pub {}: {},",
                        to_snake(&field.name),
                        to_rust_type(resolve, &field.ty)
                    );
                }
                out.push_str("}\n");
            }
            TypeDefKind::Variant(variant) => {
                let _ = writeln!(
                    out,
                    "#[derive(Clone, Debug, PartialEq)]\npub enum {name} {{"
                );
                for case in &variant.cases {
                    match &case.ty {
                        Some(ty) => {
                            let _ = writeln!(
                                out,
                                "{}({}),",
                                to_camel(&case.name),
                                to_rust_type(resolve, ty)
                            );
                        }
                        None => {
                            let _ = writeln!(out, "{},", to_camel(&case.name));
                        }
                    }
                }
                out.push_str("}\n");
            }
            TypeDefKind::Enum(enum_) => {
                let _ = writeln!(
                    out,
                    "#[derive(Clone, Copy, Debug, PartialEq, Eq)]\npub enum {name} {{"
                );
                for case in &enum_.cases {
                    let _ = writeln!(out, "{},", to_camel(&case.name));
                }
                out.push_str("}\n");
            }
            TypeDefKind::Resource => {
                let _ = writeln!(
                    out,
                    "/// A handle the guest holds, its index in the resource table.\n\
                     #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]\npub struct {name}(pub u32);"
                );
                if let Some(host) = host_by_resource_of(&wit_name, wit) {
                    let _ = writeln!(
                        out,
                        "impl {HOSTS}::table::Handle for {name} {{\n\
                         type Host = {HOSTS}::{host};\n\
                         fn new(index: u32) -> Self {{ Self(index) }}\n\
                         fn index(self) -> u32 {{ self.0 }}\n}}"
                    );
                }
            }
            other => {
                let _ = writeln!(
                    out,
                    "pub type {name} = {};",
                    to_structural_type(resolve, other)
                );
            }
        }
    }
    out.push_str("}\n\n");
}

/// The functions of `id` among `names`.
fn functions_of<'a>(
    resolve: &'a Resolve,
    id: InterfaceId,
    names: &'a [&str],
) -> impl Iterator<Item = &'a Function> {
    resolve.interfaces[id]
        .functions
        .values()
        .filter(move |func| names.contains(&func.name.as_str()))
}

/// The resources of `id` whose drop is among `names`.
fn resources_of<'a>(
    resolve: &'a Resolve,
    id: InterfaceId,
    names: &'a [&str],
) -> impl Iterator<Item = (&'a str, TypeId)> {
    resolve.interfaces[id]
        .types
        .iter()
        .filter(|(_, ty)| matches!(resolve.types[**ty].kind, TypeDefKind::Resource))
        .filter(move |(name, _)| names.contains(&format!("[resource-drop]{name}").as_str()))
        .map(|(name, ty)| (name.as_str(), *ty))
}

/// The `Host` method answering `func`.
fn to_method_name(resolve: &Resolve, id: InterfaceId, func: &Function) -> String {
    let interface = to_snake(
        resolve.interfaces[id]
            .name
            .as_deref()
            .expect("a named interface"),
    );
    let item = match &func.kind {
        FunctionKind::Constructor(_) => "new".to_string(),
        _ => func.item_name().to_string(),
    };
    let owner = match &func.kind {
        FunctionKind::Method(r) | FunctionKind::Static(r) | FunctionKind::Constructor(r) => {
            format!(
                "{}_",
                to_snake(resolve.types[*r].name.as_deref().expect("a named resource"))
            )
        }
        _ => String::new(),
    };
    format!("{interface}_{owner}{}", to_snake(&item))
}

/// A WIT error `TRAPPABLE` gives a host type.
struct Trappable {
    /// The error's own definition, past every `use` alias.
    error: TypeId,
    /// The host type a method answering it returns.
    host: String,
}

impl Trappable {
    /// The trappable error `func` answers, as `bindgen!` special-cases a
    /// function whose one result is `result<_, e>` with `e` in its table.
    fn of(resolve: &Resolve, func: &Function) -> Option<Self> {
        let Some(Type::Id(result)) = func.result else {
            return None;
        };
        let TypeDefKind::Result(result) = &resolve.types[result].kind else {
            return None;
        };
        let Some(Type::Id(error)) = result.err else {
            return None;
        };
        let error = definition_of(resolve, error);
        let def = &resolve.types[error];
        let TypeOwner::Interface(owner) = def.owner else {
            return None;
        };
        let interface = resolve.id_of(owner)?;
        let name = def.name.as_deref()?;
        TRAPPABLE
            .iter()
            .find(|(i, t, _)| *i == interface && *t == name)
            .map(|(_, _, host)| Self {
                error,
                host: format!("{HOSTS}::error::{host}"),
            })
    }

    /// The method turning the host type back into the WIT error, or into a
    /// trap, on the trait of the interface that declares it, as `bindgen!`
    /// names it `convert_<error>`.
    fn convert_method_of(&self, resolve: &Resolve) -> String {
        let def = &resolve.types[self.error];
        let TypeOwner::Interface(owner) = def.owner else {
            unreachable!("a trappable error belongs to an interface");
        };
        format!(
            "{}_convert_{}",
            to_snake(resolve.interfaces[owner].name.as_deref().expect("named")),
            to_snake(def.name.as_deref().expect("a named error"))
        )
    }
}

/// The trait one WIT package's imports are answered through, as `IoHost` for
/// `wasi:io`.
fn package_trait_of(resolve: &Resolve, id: InterfaceId) -> String {
    let package = resolve.interfaces[id]
        .package
        .expect("a vendored interface has a package");
    format!("{}Host", to_camel(&resolve.packages[package].name.name))
}

/// The traits the Wasm Driver implements, one per WIT package and one method
/// per import, and `Host`, which every one of them makes. No method has a
/// default, so an import without an answer does not compile.
fn write_host_trait(resolve: &Resolve, imports: &[(InterfaceId, &[&str])], out: &mut String) {
    let mut traits = Vec::<String>::new();
    for (id, _) in imports {
        let name = package_trait_of(resolve, *id);
        if !traits.contains(&name) {
            traits.push(name);
        }
    }
    for name in &traits {
        let package = imports
            .iter()
            .filter(|(id, _)| package_trait_of(resolve, *id) == *name)
            .copied()
            .collect::<Vec<_>>();
        let _ = writeln!(
            out,
            "#[allow(unused_variables, clippy::too_many_arguments)]\npub trait {name} {{"
        );
        write_package_methods(resolve, &package, out);
        write_convert_methods(resolve, imports, &package, out);
        out.push_str("}\n\n");
    }
    let _ = writeln!(
        out,
        "pub trait Host: {} {{\n\
         /// The trap a call ends the run with once it answered, if its host gave the\n\
         /// invocation up meanwhile, as each preview1 call does.\n\
         fn abandonment(&self) -> Option<wasmtime::Error>;\n}}\n",
        traits.join(" + ")
    );
}

fn write_package_methods(resolve: &Resolve, imports: &[(InterfaceId, &[&str])], out: &mut String) {
    for (id, names) in imports {
        for func in functions_of(resolve, *id, names) {
            let params = func
                .params
                .iter()
                .map(|p| format!(", {}: {}", to_snake(&p.name), to_rust_type(resolve, &p.ty)))
                .collect::<String>();
            let result = match (Trappable::of(resolve, func), func.result) {
                (Some(trappable), Some(Type::Id(result))) => {
                    let TypeDefKind::Result(result) = &resolve.types[result].kind else {
                        unreachable!("a trappable function answers a result");
                    };
                    let ok = result
                        .ok
                        .as_ref()
                        .map_or("()".into(), |t| to_rust_type(resolve, t));
                    format!("Result<{ok}, {}>", trappable.host)
                }
                (_, result) => format!(
                    "wasmtime::Result<{}>",
                    result
                        .as_ref()
                        .map_or("()".into(), |t| to_rust_type(resolve, t))
                ),
            };
            let _ = writeln!(
                out,
                "fn {}(&mut self{params}) -> {result};",
                to_method_name(resolve, *id, func),
            );
        }
        for (name, ty) in resources_of(resolve, *id, names) {
            let interface = to_snake(resolve.interfaces[*id].name.as_deref().expect("named"));
            let _ = writeln!(
                out,
                "fn {interface}_{}_drop(&mut self, handle: {}) -> wasmtime::Result<()>;",
                to_snake(name),
                type_path_of(resolve, ty),
            );
        }
    }
}

/// One `convert_<error>` method for each trappable error some import answers
/// and an interface of `package` declares.
fn write_convert_methods(
    resolve: &Resolve,
    imports: &[(InterfaceId, &[&str])],
    package: &[(InterfaceId, &[&str])],
    out: &mut String,
) {
    let mut written = Vec::<TypeId>::new();
    let answered = imports
        .iter()
        .flat_map(|(id, names)| functions_of(resolve, *id, names))
        .filter_map(|func| Trappable::of(resolve, func));
    for trappable in answered {
        let declared = package
            .iter()
            .any(|(id, _)| resolve.types[trappable.error].owner == TypeOwner::Interface(*id));
        if declared && !written.contains(&trappable.error) {
            written.push(trappable.error);
            let _ = writeln!(
                out,
                "fn {}(&mut self, err: {}) -> wasmtime::Result<{}>;",
                trappable.convert_method_of(resolve),
                trappable.host,
                type_path_of(resolve, trappable.error),
            );
        }
    }
}

fn to_core_type(ty: &WasmType) -> &'static str {
    match ty {
        WasmType::I32 | WasmType::Pointer | WasmType::Length => "i32",
        WasmType::I64 | WasmType::PointerOrI64 => "i64",
        other => unreachable!("no import kobako answers carries {other:?}"),
    }
}

/// `add_to_linker`, linking every import under its interface's module.
fn write_linker(
    resolve: &Resolve,
    sizes: &SizeAlign,
    imports: &[(InterfaceId, &[&str])],
    out: &mut String,
) {
    out.push_str(
        "#[allow(unused_mut, unused_variables, clippy::all)]\n\
         pub fn add_to_linker<T: Host + 'static>(linker: &mut wasmtime::Linker<T>) -> wasmtime::Result<()> {\n",
    );
    for (id, names) in imports {
        let module = resolve.id_of(*id).expect("a named interface");
        for func in functions_of(resolve, *id, names) {
            let sig = resolve.wasm_signature(abi::AbiVariant::GuestImport, func);
            let params = sig
                .params
                .iter()
                .enumerate()
                .map(|(i, ty)| format!(", arg{i}: {}", to_core_type(ty)))
                .collect::<String>();
            let result = match sig.results.as_slice() {
                [] => "()".to_string(),
                [one] => to_core_type(one).to_string(),
                _ => unreachable!("a core import answers at most one value"),
            };
            let mut body = FunctionBody::new(resolve, sizes, *id, func);
            abi::call(
                resolve,
                abi::AbiVariant::GuestImport,
                LiftLower::LiftArgsLowerResults,
                func,
                &mut body,
                false,
            );
            // The answer lands in the return area the guest's last argument
            // names, which the Canonical ABI requires aligned to the answer.
            let aligned = match (&func.result, sig.retptr) {
                (Some(answer), true) => format!(
                    "crate::preview2::abi::ensure_aligned(arg{}, {})?;\n",
                    sig.params.len() - 1,
                    sizes.align(answer).align_wasm32()
                ),
                _ => String::new(),
            };
            let _ = writeln!(
                out,
                "linker.func_wrap({module:?}, {:?}, |mut caller: wasmtime::Caller<'_, T>{params}| \
                 -> wasmtime::Result<{result}> {{\n\
                 let mut fuel = crate::preview2::abi::Fuel::new(&mut caller);\n{aligned}{}}})?;",
                func.name, body.src
            );
        }
        for (name, ty) in resources_of(resolve, *id, names) {
            let interface = to_snake(resolve.interfaces[*id].name.as_deref().expect("named"));
            let _ = writeln!(
                out,
                "linker.func_wrap({module:?}, \"[resource-drop]{name}\", \
                 |mut caller: wasmtime::Caller<'_, T>, handle: i32| -> wasmtime::Result<()> {{ \
                 caller.data_mut().{interface}_{}_drop({}(handle as u32))?; \
                 caller.data().abandonment().map_or(Ok(()), Err) }})?;",
                to_snake(name),
                type_path_of(resolve, ty)
            );
        }
    }
    out.push_str("Ok(())\n}\n");
}

/// One import's body, as the Canonical ABI walks it.
struct FunctionBody<'a> {
    resolve: &'a Resolve,
    sizes: &'a SizeAlign,
    method: String,
    /// The `convert_<error>` method a trappable answer passes through.
    convert: Option<String>,
    src: String,
    blocks: Vec<String>,
    stack: Vec<String>,
    tmp: usize,
}

impl<'a> FunctionBody<'a> {
    fn new(resolve: &'a Resolve, sizes: &'a SizeAlign, id: InterfaceId, func: &Function) -> Self {
        Self {
            resolve,
            sizes,
            method: to_method_name(resolve, id, func),
            convert: Trappable::of(resolve, func).map(|t| t.convert_method_of(resolve)),
            src: String::new(),
            blocks: Vec::new(),
            stack: Vec::new(),
            tmp: 0,
        }
    }

    fn tmp(&mut self) -> String {
        self.tmp += 1;
        format!("t{}", self.tmp)
    }

    /// Bind `expr` to a fresh name, so each operand is read once.
    fn bind(&mut self, expr: String) -> String {
        let name = self.tmp();
        let _ = writeln!(self.src, "let {name} = {expr};");
        name
    }

    fn blocks(&mut self, count: usize) -> Vec<String> {
        self.blocks.drain(self.blocks.len() - count..).collect()
    }

    fn rust_type_of(&self, id: TypeId) -> String {
        to_rust_type(self.resolve, &Type::Id(id))
    }

    fn size_of(&self, element: &Type) -> usize {
        self.sizes.size(element).size_wasm32()
    }

    fn align_of(&self, element: &Type) -> usize {
        self.sizes.align(element).align_wasm32()
    }
}

fn to_offset(offset: &ArchitectureSize) -> usize {
    offset.size_wasm32()
}

impl Bindgen for FunctionBody<'_> {
    type Operand = String;

    fn emit(
        &mut self,
        resolve: &Resolve,
        inst: &Instruction<'_>,
        operands: &mut Vec<String>,
        results: &mut Vec<String>,
    ) {
        let abi = "crate::preview2::abi";
        match inst {
            Instruction::GetArg { nth } => results.push(format!("arg{nth}")),
            Instruction::I32Const { val } => results.push(format!("{val}i32")),
            Instruction::I32Load { offset: o }
            | Instruction::PointerLoad { offset: o }
            | Instruction::LengthLoad { offset: o } => {
                let e = format!(
                    "{abi}::load::<i32>(&mut caller, {}, {})?",
                    operands[0],
                    to_offset(o)
                );
                results.push(self.bind(e));
            }
            Instruction::I32Load8U { offset: o } => {
                let e = format!(
                    "({abi}::load::<u8>(&mut caller, {}, {})? as i32)",
                    operands[0],
                    to_offset(o)
                );
                results.push(self.bind(e));
            }
            Instruction::I32Load16U { offset: o } => {
                let e = format!(
                    "({abi}::load::<u16>(&mut caller, {}, {})? as i32)",
                    operands[0],
                    to_offset(o)
                );
                results.push(self.bind(e));
            }
            Instruction::I32Store { offset: o }
            | Instruction::PointerStore { offset: o }
            | Instruction::LengthStore { offset: o } => {
                let _ = writeln!(
                    self.src,
                    "{abi}::store::<i32>(&mut caller, {}, {}, {})?;",
                    operands[1],
                    to_offset(o),
                    operands[0]
                );
            }
            Instruction::I32Store8 { offset: o } => {
                let _ = writeln!(
                    self.src,
                    "{abi}::store::<u8>(&mut caller, {}, {}, {} as u8)?;",
                    operands[1],
                    to_offset(o),
                    operands[0]
                );
            }
            Instruction::I32Store16 { offset: o } => {
                let _ = writeln!(
                    self.src,
                    "{abi}::store::<u16>(&mut caller, {}, {}, {} as u16)?;",
                    operands[1],
                    to_offset(o),
                    operands[0]
                );
            }
            Instruction::I64Store { offset: o } => {
                let _ = writeln!(
                    self.src,
                    "{abi}::store::<i64>(&mut caller, {}, {}, {})?;",
                    operands[1],
                    to_offset(o),
                    operands[0]
                );
            }
            Instruction::I64FromU64 | Instruction::I64FromS64 => {
                results.push(format!("({} as i64)", operands[0]))
            }
            Instruction::I32FromU32
            | Instruction::I32FromS32
            | Instruction::I32FromU16
            | Instruction::I32FromS16
            | Instruction::I32FromU8
            | Instruction::I32FromS8 => results.push(format!("({} as i32)", operands[0])),
            Instruction::U8FromI32 => results.push(format!("({} as u8)", operands[0])),
            Instruction::U16FromI32 => results.push(format!("({} as u16)", operands[0])),
            Instruction::U32FromI32 => results.push(format!("({} as u32)", operands[0])),
            Instruction::U64FromI64 => results.push(format!("({} as u64)", operands[0])),
            Instruction::I32FromBool => results.push(format!("({} as i32)", operands[0])),
            Instruction::BoolFromI32 => results.push(format!("({} != 0)", operands[0])),
            Instruction::ListCanonLift { element, .. } => {
                let e = format!(
                    "{abi}::load_list::<{}>(&mut caller, &mut fuel, {}, {})?",
                    to_rust_type(resolve, element),
                    operands[0],
                    operands[1]
                );
                results.push(self.bind(e));
            }
            Instruction::StringLift => {
                let e = format!(
                    "{abi}::load_string(&mut caller, &mut fuel, {}, {})?",
                    operands[0], operands[1]
                );
                results.push(self.bind(e));
            }
            Instruction::ListCanonLower { element, .. } => {
                let (ptr, len) = (self.tmp(), self.tmp());
                let _ = writeln!(
                    self.src,
                    "let ({ptr}, {len}) = {abi}::store_list::<{}>(&mut caller, &{}, {})?;",
                    to_rust_type(resolve, element),
                    operands[0],
                    self.align_of(element)
                );
                results.push(ptr);
                results.push(len);
            }
            Instruction::StringLower { .. } => {
                let (ptr, len) = (self.tmp(), self.tmp());
                let _ = writeln!(
                    self.src,
                    "let ({ptr}, {len}) = {abi}::store_list::<u8>(&mut caller, {}.as_bytes(), 1)?;",
                    operands[0]
                );
                results.push(ptr);
                results.push(len);
            }
            Instruction::ListLift { element, .. } => {
                let block = self.blocks(1).remove(0);
                let (size, align) = (self.size_of(element), self.align_of(element));
                let (list, len) = (self.tmp(), self.tmp());
                let _ = writeln!(
                    self.src,
                    "let {len} = {abi}::list_len(&mut caller, {ptr}, {given}, {size}, {align})?;\n\
                     fuel.spend({len}, {size})?;\n\
                     let mut {list} = Vec::with_capacity({len});\n\
                     for i in 0..{len} {{ let base = {ptr}.wrapping_add((i * {size}) as i32); {list}.push({block}); }}",
                    ptr = operands[0],
                    given = operands[1],
                );
                results.push(list);
            }
            Instruction::ListLower { element, .. } => {
                let block = self.blocks(1).remove(0);
                let (size, align) = (self.size_of(element), self.align_of(element));
                let (ptr, len) = (self.tmp(), self.tmp());
                let _ = writeln!(
                    self.src,
                    "let {len} = {list}.len() as i32;\n\
                     let {ptr} = {abi}::realloc(&mut caller, ({len} as usize * {size}) as i32, {align})?;\n\
                     for (i, e) in {list}.into_iter().enumerate() {{ let base = {ptr}.wrapping_add((i * {size}) as i32); {block}; }}",
                    list = operands[0],
                );
                results.push(ptr);
                results.push(len);
            }
            Instruction::IterElem { .. } => results.push("e".into()),
            Instruction::IterBasePointer => results.push("base".into()),
            Instruction::RecordLift { record, ty, .. } => {
                let fields = record
                    .fields
                    .iter()
                    .zip(operands.iter())
                    .map(|(f, op)| format!("{}: {op}", to_snake(&f.name)))
                    .collect::<Vec<_>>()
                    .join(", ");
                let e = format!("{} {{ {fields} }}", self.rust_type_of(*ty));
                results.push(self.bind(e));
            }
            Instruction::RecordLower { record, ty, .. } => {
                let names = record.fields.iter().map(|_| self.tmp()).collect::<Vec<_>>();
                let pattern = record
                    .fields
                    .iter()
                    .zip(&names)
                    .map(|(f, n)| format!("{}: {n}", to_snake(&f.name)))
                    .collect::<Vec<_>>()
                    .join(", ");
                let _ = writeln!(
                    self.src,
                    "let {} {{ {pattern} }} = {};",
                    self.rust_type_of(*ty),
                    operands[0]
                );
                results.extend(names);
            }
            Instruction::TupleLift { .. } => {
                let e = format!("({},)", operands.join(", "));
                results.push(self.bind(e));
            }
            Instruction::TupleLower { tuple, .. } => {
                let names = tuple.types.iter().map(|_| self.tmp()).collect::<Vec<_>>();
                let _ = writeln!(self.src, "let ({},) = {};", names.join(", "), operands[0]);
                results.extend(names);
            }
            Instruction::HandleLift { handle, .. } => {
                let (Handle::Own(r) | Handle::Borrow(r)) = handle;
                let path = type_path_of(resolve, definition_of(resolve, *r));
                results.push(format!("{path}({} as u32)", operands[0]))
            }
            Instruction::HandleLower { .. } => results.push(format!("({}.0 as i32)", operands[0])),
            Instruction::EnumLift { enum_, ty, .. } => {
                let name = self.rust_type_of(*ty);
                let arms = enum_
                    .cases
                    .iter()
                    .enumerate()
                    .map(|(i, c)| format!("{i} => {name}::{},", to_camel(&c.name)))
                    .collect::<String>();
                let e = format!(
                    "match {} {{ {arms} _ => return Err({abi}::invalid(\"enum discriminant\")) }}",
                    operands[0]
                );
                results.push(self.bind(e));
            }
            Instruction::EnumLower { enum_, ty, .. } => {
                let name = self.rust_type_of(*ty);
                let arms = enum_
                    .cases
                    .iter()
                    .enumerate()
                    .map(|(i, c)| format!("{name}::{} => {i}i32,", to_camel(&c.name)))
                    .collect::<String>();
                results.push(format!("(match {} {{ {arms} }})", operands[0]));
            }
            Instruction::VariantPayloadName => results.push("e".into()),
            Instruction::VariantLift { variant, ty, .. } => {
                let blocks = self.blocks(variant.cases.len());
                let name = self.rust_type_of(*ty);
                let arms = variant
                    .cases
                    .iter()
                    .zip(blocks)
                    .enumerate()
                    .map(|(i, (c, b))| match c.ty {
                        Some(_) => format!("{i} => {name}::{}({b}),", to_camel(&c.name)),
                        None => format!("{i} => {{ {b}; {name}::{} }},", to_camel(&c.name)),
                    })
                    .collect::<String>();
                let e = format!(
                    "match {} {{ {arms} _ => return Err({abi}::invalid(\"variant discriminant\")) }}",
                    operands[0]
                );
                results.push(self.bind(e));
            }
            Instruction::VariantLower {
                variant,
                ty,
                results: types,
                ..
            } => {
                let blocks = self.blocks(variant.cases.len());
                let name = self.rust_type_of(*ty);
                let arms = variant
                    .cases
                    .iter()
                    .zip(blocks)
                    .map(|(c, b)| match c.ty {
                        Some(_) => format!("{name}::{}(e) => {b},", to_camel(&c.name)),
                        None => format!("{name}::{} => {b},", to_camel(&c.name)),
                    })
                    .collect::<String>();
                self.lower_match(&operands[0], &arms, types.len(), results);
            }
            Instruction::OptionLift { .. } => {
                let blocks = self.blocks(2);
                let e = format!(
                    "match {} {{ 0 => {{ {}; None }}, 1 => Some({}), _ => return Err({abi}::invalid(\"option discriminant\")) }}",
                    operands[0], blocks[0], blocks[1]
                );
                results.push(self.bind(e));
            }
            Instruction::OptionLower { results: types, .. } => {
                let blocks = self.blocks(2);
                let arms = format!("None => {}, Some(e) => {},", blocks[0], blocks[1]);
                self.lower_match(&operands[0], &arms, types.len(), results);
            }
            Instruction::ResultLift { result, .. } => {
                let blocks = self.blocks(2);
                let ok = match result.ok {
                    Some(_) => format!("Ok({})", blocks[0]),
                    None => format!("{{ {}; Ok(()) }}", blocks[0]),
                };
                let err = match result.err {
                    Some(_) => format!("Err({})", blocks[1]),
                    None => format!("{{ {}; Err(()) }}", blocks[1]),
                };
                let e = format!(
                    "match {} {{ 0 => {ok}, 1 => {err}, _ => return Err({abi}::invalid(\"result discriminant\")) }}",
                    operands[0]
                );
                results.push(self.bind(e));
            }
            Instruction::ResultLower { results: types, .. } => {
                let blocks = self.blocks(2);
                let arms = format!("Ok(e) => {}, Err(e) => {},", blocks[0], blocks[1]);
                self.lower_match(&operands[0], &arms, types.len(), results);
            }
            Instruction::CallInterface { func, .. } => {
                let args = operands.join(", ");
                let call = match &self.convert {
                    Some(convert) => format!(
                        "match caller.data_mut().{}({args}) {{ Ok(answer) => Ok(answer), \
                         Err(err) => Err(caller.data_mut().{convert}(err)?) }}",
                        self.method
                    ),
                    None => format!("caller.data_mut().{}({args})?", self.method),
                };
                let answered = match func.result {
                    Some(_) => Some(self.bind(call)),
                    None => {
                        let _ = writeln!(self.src, "{call};");
                        None
                    }
                };
                self.src.push_str(
                    "if let Some(trap) = caller.data().abandonment() { return Err(trap); }\n",
                );
                results.extend(answered);
            }
            Instruction::Return { amt, .. } => match amt {
                0 => self.src.push_str("Ok(())\n"),
                _ => {
                    let _ = writeln!(self.src, "Ok({})", operands[0]);
                }
            },
            other => unreachable!("no import kobako answers lowers through {other:?}"),
        }
    }

    fn return_pointer(&mut self, _size: ArchitectureSize, _align: Alignment) -> String {
        unreachable!("the guest hands an import its return area")
    }

    fn push_block(&mut self) {
        let outer = std::mem::take(&mut self.src);
        self.stack.push(outer);
    }

    fn finish_block(&mut self, operands: &mut Vec<String>) {
        let outer = self.stack.pop().expect("a block was pushed");
        let inner = std::mem::replace(&mut self.src, outer);
        let value = match operands.len() {
            0 => "()".to_string(),
            1 => operands[0].clone(),
            _ => format!("({})", operands.join(", ")),
        };
        self.blocks.push(format!("{{\n{inner}{value}\n}}"));
    }

    fn sizes(&self) -> &SizeAlign {
        self.sizes
    }

    fn is_list_canonical(&self, _resolve: &Resolve, element: &Type) -> bool {
        matches!(
            element,
            Type::U8
                | Type::S8
                | Type::U16
                | Type::S16
                | Type::U32
                | Type::S32
                | Type::U64
                | Type::S64
        )
    }
}

impl FunctionBody<'_> {
    /// Lower a value through `arms`, each arm a block answering `count` core
    /// values.
    fn lower_match(&mut self, op: &str, arms: &str, count: usize, results: &mut Vec<String>) {
        match count {
            0 => {
                let _ = writeln!(self.src, "match {op} {{ {arms} }};");
            }
            1 => {
                let name = self.tmp();
                let _ = writeln!(self.src, "let {name} = match {op} {{ {arms} }};");
                results.push(name);
            }
            n => {
                let names = (0..n).map(|_| self.tmp()).collect::<Vec<_>>();
                let _ = writeln!(
                    self.src,
                    "let ({}) = match {op} {{ {arms} }};",
                    names.join(", ")
                );
                results.extend(names);
            }
        }
    }
}
