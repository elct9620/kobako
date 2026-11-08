//! The generator behind `build.rs`: each WIT function a Guest Binary imports
//! becomes one core `Linker` function, lifting its arguments from the core
//! values and linear memory, calling the `Host` trait, and lowering the
//! answer back, as the Canonical ABI's guest-import side has the host do.
//! `wit-bindgen-core` walks the ABI; this file only says how each step is
//! spelled in Rust.

use std::fmt::Write as _;
use std::path::Path;

use wit_bindgen_core::abi::{self, Bindgen, Bitcast, Instruction, LiftLower, WasmType};
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
            "[method]tcp-socket.start-connect",
            "[method]tcp-socket.finish-connect",
            "[method]tcp-socket.shutdown",
            "[method]tcp-socket.subscribe",
            "[method]tcp-socket.remote-address",
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
            let found = imported(&resolve, id, functions).count()
                + resources(&resolve, id, functions).count();
            if found != functions.len() {
                return Err(anyhow::anyhow!("{name} lacks a function IMPORTS names"));
            }
            match resources(&resolve, id, functions).find(|(r, _)| host(name, r).is_none()) {
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
fn host(interface: &str, resource: &str) -> Option<&'static str> {
    WITH.iter()
        .find(|(i, r, _)| *i == interface && *r == resource)
        .map(|(_, _, host)| *host)
}

/// The module one interface's types live in.
fn module(resolve: &Resolve, id: InterfaceId) -> String {
    snake(&resolve.id_of(id).expect("a named interface"))
}

fn snake(name: &str) -> String {
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

fn camel(name: &str) -> String {
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
fn type_path(resolve: &Resolve, id: TypeId) -> String {
    let def = &resolve.types[id];
    let name = camel(def.name.as_deref().expect("only named types have a path"));
    match def.owner {
        TypeOwner::Interface(owner) => format!("{ROOT}::{}::{name}", module(resolve, owner)),
        _ => unreachable!("every imported type belongs to an interface"),
    }
}

/// How a WIT type is spelled in Rust.
fn rust_type(resolve: &Resolve, ty: &Type) -> String {
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
            Some(_) => type_path(resolve, *id),
            None => structural(resolve, &resolve.types[*id].kind),
        },
        other => unreachable!("no import kobako answers takes {other:?}"),
    }
}

/// The resource `id` names, through every `use` alias of it.
fn resource(resolve: &Resolve, mut id: TypeId) -> TypeId {
    while let TypeDefKind::Type(Type::Id(aliased)) = resolve.types[id].kind {
        id = aliased;
    }
    id
}

/// How a type made of other types is spelled, named or not.
fn structural(resolve: &Resolve, kind: &TypeDefKind) -> String {
    match kind {
        TypeDefKind::Handle(Handle::Own(r) | Handle::Borrow(r)) => {
            type_path(resolve, resource(resolve, *r))
        }
        TypeDefKind::List(t) => format!("Vec<{}>", rust_type(resolve, t)),
        TypeDefKind::Option(t) => format!("Option<{}>", rust_type(resolve, t)),
        TypeDefKind::Result(r) => format!(
            "Result<{}, {}>",
            r.ok.as_ref().map_or("()".into(), |t| rust_type(resolve, t)),
            r.err
                .as_ref()
                .map_or("()".into(), |t| rust_type(resolve, t)),
        ),
        TypeDefKind::Tuple(t) => format!(
            "({})",
            t.types
                .iter()
                .map(|t| format!("{},", rust_type(resolve, t)))
                .collect::<String>()
        ),
        TypeDefKind::Type(t) => rust_type(resolve, t),
        other => unreachable!("no 0.2 import takes {other:?}"),
    }
}

/// Each named type an interface declares.
fn write_types(resolve: &Resolve, id: InterfaceId, out: &mut String) {
    let interface = &resolve.interfaces[id];
    let _ = writeln!(
        out,
        "#[allow(dead_code, clippy::all)]\npub mod {} {{",
        module(resolve, id)
    );
    let wit_name = resolve.id_of(id).expect("a named interface");
    for (wit, ty) in &interface.types {
        let name = camel(wit);
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
                        snake(&field.name),
                        rust_type(resolve, &field.ty)
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
                            let _ =
                                writeln!(out, "{}({}),", camel(&case.name), rust_type(resolve, ty));
                        }
                        None => {
                            let _ = writeln!(out, "{},", camel(&case.name));
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
                    let _ = writeln!(out, "{},", camel(&case.name));
                }
                out.push_str("}\n");
            }
            TypeDefKind::Resource => {
                let _ = writeln!(
                    out,
                    "/// A handle the guest holds, its index in the resource table.\n\
                     #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]\npub struct {name}(pub u32);"
                );
                if let Some(host) = host(&wit_name, wit) {
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
                let _ = writeln!(out, "pub type {name} = {};", structural(resolve, other));
            }
        }
    }
    out.push_str("}\n\n");
}

/// The functions of `id` among `names`.
fn imported<'a>(
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
fn resources<'a>(
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
fn method_name(resolve: &Resolve, id: InterfaceId, func: &Function) -> String {
    let interface = snake(
        resolve.interfaces[id]
            .name
            .as_deref()
            .expect("a named interface"),
    );
    let item = match &func.kind {
        FunctionKind::Constructor(_) => format!("{}-new", func.item_name()),
        _ => func.item_name().to_string(),
    };
    let owner = match &func.kind {
        FunctionKind::Method(r) | FunctionKind::Static(r) | FunctionKind::Constructor(r) => {
            format!(
                "{}_",
                snake(resolve.types[*r].name.as_deref().expect("a named resource"))
            )
        }
        _ => String::new(),
    };
    format!("{interface}_{owner}{}", snake(&item))
}

/// The trait one WIT package's imports are answered through, as `IoHost` for
/// `wasi:io`.
fn package_trait(resolve: &Resolve, id: InterfaceId) -> String {
    let package = resolve.interfaces[id]
        .package
        .expect("a vendored interface has a package");
    format!("{}Host", camel(&resolve.packages[package].name.name))
}

/// The traits the Wasm Driver implements, one per WIT package and one method
/// per import, and `Host`, which every one of them makes. No method has a
/// default, so an import without an answer does not compile.
fn write_host_trait(resolve: &Resolve, imports: &[(InterfaceId, &[&str])], out: &mut String) {
    let mut traits = Vec::<String>::new();
    for (id, _) in imports {
        let name = package_trait(resolve, *id);
        if !traits.contains(&name) {
            traits.push(name);
        }
    }
    for name in &traits {
        let package = imports
            .iter()
            .filter(|(id, _)| package_trait(resolve, *id) == *name)
            .copied()
            .collect::<Vec<_>>();
        let _ = writeln!(
            out,
            "#[allow(unused_variables, clippy::too_many_arguments)]\npub trait {name} {{"
        );
        package_methods(resolve, &package, out);
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

fn package_methods(resolve: &Resolve, imports: &[(InterfaceId, &[&str])], out: &mut String) {
    for (id, names) in imports {
        for func in imported(resolve, *id, names) {
            let params = func
                .params
                .iter()
                .map(|p| format!(", {}: {}", snake(&p.name), rust_type(resolve, &p.ty)))
                .collect::<String>();
            let result = func
                .result
                .as_ref()
                .map_or("()".into(), |t| rust_type(resolve, t));
            let _ = writeln!(
                out,
                "fn {}(&mut self{params}) -> wasmtime::Result<{result}>;",
                method_name(resolve, *id, func),
            );
        }
        for (name, ty) in resources(resolve, *id, names) {
            let interface = snake(resolve.interfaces[*id].name.as_deref().expect("named"));
            let _ = writeln!(
                out,
                "fn {interface}_{}_drop(&mut self, handle: {}) -> wasmtime::Result<()>;",
                snake(name),
                type_path(resolve, ty),
            );
        }
    }
}

fn core_type(ty: &WasmType) -> &'static str {
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
        for func in imported(resolve, *id, names) {
            let sig = resolve.wasm_signature(abi::AbiVariant::GuestImport, func);
            let params = sig
                .params
                .iter()
                .enumerate()
                .map(|(i, ty)| format!(", arg{i}: {}", core_type(ty)))
                .collect::<String>();
            let result = match sig.results.as_slice() {
                [] => "()".to_string(),
                [one] => core_type(one).to_string(),
                _ => unreachable!("a core import answers at most one value"),
            };
            let mut body = FunctionBody::new(resolve, sizes, method_name(resolve, *id, func));
            abi::call(
                resolve,
                abi::AbiVariant::GuestImport,
                LiftLower::LiftArgsLowerResults,
                func,
                &mut body,
                false,
            );
            let _ = writeln!(
                out,
                "linker.func_wrap({module:?}, {:?}, |mut caller: wasmtime::Caller<'_, T>{params}| \
                 -> wasmtime::Result<{result}> {{\n{}}})?;",
                func.name, body.src
            );
        }
        for (name, ty) in resources(resolve, *id, names) {
            let interface = snake(resolve.interfaces[*id].name.as_deref().expect("named"));
            let _ = writeln!(
                out,
                "linker.func_wrap({module:?}, \"[resource-drop]{name}\", \
                 |mut caller: wasmtime::Caller<'_, T>, handle: i32| -> wasmtime::Result<()> {{ \
                 caller.data_mut().{interface}_{}_drop({}(handle as u32))?; \
                 caller.data().abandonment().map_or(Ok(()), Err) }})?;",
                snake(name),
                type_path(resolve, ty)
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
    src: String,
    blocks: Vec<String>,
    stack: Vec<String>,
    tmp: usize,
}

impl<'a> FunctionBody<'a> {
    fn new(resolve: &'a Resolve, sizes: &'a SizeAlign, method: String) -> Self {
        Self {
            resolve,
            sizes,
            method,
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
        rust_type(self.resolve, &Type::Id(id))
    }

    fn element_size(&self, element: &Type) -> usize {
        self.sizes.size(element).size_wasm32()
    }

    fn element_align(&self, element: &Type) -> usize {
        self.sizes.align(element).align_wasm32()
    }
}

fn offset(offset: &ArchitectureSize) -> usize {
    offset.size_wasm32()
}

fn cast(bitcast: &Bitcast, op: &str) -> String {
    match bitcast {
        Bitcast::None
        | Bitcast::I32ToP
        | Bitcast::PToI32
        | Bitcast::PToL
        | Bitcast::LToP
        | Bitcast::I32ToL
        | Bitcast::LToI32
        | Bitcast::P64ToI64
        | Bitcast::I64ToP64 => op.to_string(),
        Bitcast::I32ToI64 | Bitcast::PToP64 | Bitcast::LToI64 => format!("({op} as i64)"),
        Bitcast::I64ToI32 | Bitcast::P64ToP | Bitcast::I64ToL => format!("({op} as i32)"),
        Bitcast::Sequence(both) => cast(&both[1], &cast(&both[0], op)),
        other => unreachable!("no import kobako answers carries a float, so none casts {other:?}"),
    }
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
            Instruction::Bitcasts { casts } => {
                for (c, op) in casts.iter().zip(operands.iter()) {
                    results.push(cast(c, op));
                }
            }
            Instruction::ConstZero { tys } => {
                for ty in tys.iter() {
                    results.push(format!("0{}", core_type(ty)));
                }
            }
            Instruction::I32Load { offset: o }
            | Instruction::PointerLoad { offset: o }
            | Instruction::LengthLoad { offset: o } => {
                let e = format!(
                    "{abi}::load::<i32>(&mut caller, {}, {})?",
                    operands[0],
                    offset(o)
                );
                results.push(self.bind(e));
            }
            Instruction::I32Load8U { offset: o } => {
                let e = format!(
                    "({abi}::load::<u8>(&mut caller, {}, {})? as i32)",
                    operands[0],
                    offset(o)
                );
                results.push(self.bind(e));
            }
            Instruction::I32Load8S { offset: o } => {
                let e = format!(
                    "({abi}::load::<i8>(&mut caller, {}, {})? as i32)",
                    operands[0],
                    offset(o)
                );
                results.push(self.bind(e));
            }
            Instruction::I32Load16U { offset: o } => {
                let e = format!(
                    "({abi}::load::<u16>(&mut caller, {}, {})? as i32)",
                    operands[0],
                    offset(o)
                );
                results.push(self.bind(e));
            }
            Instruction::I32Load16S { offset: o } => {
                let e = format!(
                    "({abi}::load::<i16>(&mut caller, {}, {})? as i32)",
                    operands[0],
                    offset(o)
                );
                results.push(self.bind(e));
            }
            Instruction::I64Load { offset: o } => {
                let e = format!(
                    "{abi}::load::<i64>(&mut caller, {}, {})?",
                    operands[0],
                    offset(o)
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
                    offset(o),
                    operands[0]
                );
            }
            Instruction::I32Store8 { offset: o } => {
                let _ = writeln!(
                    self.src,
                    "{abi}::store::<u8>(&mut caller, {}, {}, {} as u8)?;",
                    operands[1],
                    offset(o),
                    operands[0]
                );
            }
            Instruction::I32Store16 { offset: o } => {
                let _ = writeln!(
                    self.src,
                    "{abi}::store::<u16>(&mut caller, {}, {}, {} as u16)?;",
                    operands[1],
                    offset(o),
                    operands[0]
                );
            }
            Instruction::I64Store { offset: o } => {
                let _ = writeln!(
                    self.src,
                    "{abi}::store::<i64>(&mut caller, {}, {}, {})?;",
                    operands[1],
                    offset(o),
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
            Instruction::S8FromI32 => results.push(format!("({} as i8)", operands[0])),
            Instruction::U8FromI32 => results.push(format!("({} as u8)", operands[0])),
            Instruction::S16FromI32 => results.push(format!("({} as i16)", operands[0])),
            Instruction::U16FromI32 => results.push(format!("({} as u16)", operands[0])),
            Instruction::S32FromI32 => results.push(format!("({} as i32)", operands[0])),
            Instruction::U32FromI32 => results.push(format!("({} as u32)", operands[0])),
            Instruction::S64FromI64 => results.push(format!("({} as i64)", operands[0])),
            Instruction::U64FromI64 => results.push(format!("({} as u64)", operands[0])),
            Instruction::BoolFromI32 => results.push(format!("({} != 0)", operands[0])),
            Instruction::I32FromBool => results.push(format!("({} as i32)", operands[0])),
            Instruction::ListCanonLift { element, .. } => {
                let e = format!(
                    "{abi}::load_list::<{}>(&mut caller, {}, {})?",
                    rust_type(resolve, element),
                    operands[0],
                    operands[1]
                );
                results.push(self.bind(e));
            }
            Instruction::StringLift => {
                let e = format!(
                    "{abi}::load_string(&mut caller, {}, {})?",
                    operands[0], operands[1]
                );
                results.push(self.bind(e));
            }
            Instruction::ListCanonLower { element, .. } => {
                let (ptr, len) = (self.tmp(), self.tmp());
                let _ = writeln!(
                    self.src,
                    "let ({ptr}, {len}) = {abi}::store_list::<{}>(&mut caller, &{}, {})?;",
                    rust_type(resolve, element),
                    operands[0],
                    self.element_align(element)
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
                let (size, align) = (self.element_size(element), self.element_align(element));
                let (list, len) = (self.tmp(), self.tmp());
                let _ = writeln!(
                    self.src,
                    "let {len} = {abi}::list_len(&mut caller, {ptr}, {given}, {size}, {align})?;\n\
                     let mut {list} = Vec::with_capacity({len});\n\
                     for i in 0..{len} {{ let base = {ptr}.wrapping_add((i * {size}) as i32); {list}.push({block}); }}",
                    ptr = operands[0],
                    given = operands[1],
                );
                results.push(list);
            }
            Instruction::ListLower { element, .. } => {
                let block = self.blocks(1).remove(0);
                let (size, align) = (self.element_size(element), self.element_align(element));
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
                    .map(|(f, op)| format!("{}: {op}", snake(&f.name)))
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
                    .map(|(f, n)| format!("{}: {n}", snake(&f.name)))
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
                let path = type_path(resolve, resource(resolve, *r));
                results.push(format!("{path}({} as u32)", operands[0]))
            }
            Instruction::HandleLower { .. } => results.push(format!("({}.0 as i32)", operands[0])),
            Instruction::EnumLift { enum_, ty, .. } => {
                let name = self.rust_type_of(*ty);
                let arms = enum_
                    .cases
                    .iter()
                    .enumerate()
                    .map(|(i, c)| format!("{i} => {name}::{},", camel(&c.name)))
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
                    .map(|(i, c)| format!("{name}::{} => {i}i32,", camel(&c.name)))
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
                        Some(_) => format!("{i} => {name}::{}({b}),", camel(&c.name)),
                        None => format!("{i} => {{ {b}; {name}::{} }},", camel(&c.name)),
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
                        Some(_) => format!("{name}::{}(e) => {b},", camel(&c.name)),
                        None => format!("{name}::{} => {b},", camel(&c.name)),
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
                let call = format!("caller.data_mut().{}({args})?", self.method);
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
            Instruction::Malloc { size, align, .. } => {
                let e = format!(
                    "{abi}::realloc(&mut caller, {}, {})?",
                    size.size_wasm32(),
                    align.align_wasm32()
                );
                results.push(self.bind(e));
            }
            other => unreachable!("no 0.2 import lowers through {other:?}"),
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
