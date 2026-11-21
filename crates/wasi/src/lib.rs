//! lacrymira-wasi: the WASI component channel for lacrymira (M4).
//!
//! The `celestia:lacrymira/data-client` wit world (`wit/lacrymira.wit`) lets a
//! wasm component ship `Command`s to a host-side executor and receive `Reply`s.
//! The payload on the wire is msgpack (2026-11-21 user ruling: msgpack across
//! the family). The `Command`/`Reply` msgpack encoding itself is frozen by
//! `lacrymira`'s contract tests; this crate only adds the wit transport
//! on top and never re-encodes anything.
//!
//! Two sides live here:
//!
//! * **Guest side** — [`WasiExecutor`] serializes a `Command` with `rmp-serde`,
//!   calls the wit `exec.execute` import, and deserializes the `Reply` (or
//!   reconstructs an `Error` from the wit `error-info`).
//! * **Host side** — [`HostExecutor`] is what a host implements to serve the
//!   import; [`host_serve_bytes`] adapts it to the raw byte surface that a
//!   component-model host (wasmtime &c.) actually registers. M3's Postgres
//!   executor will implement [`HostExecutor`]; until then, every closure
//!   `Fn(&Command) -> Result<Reply, Error>` implements it out of the box.
//!
//! The crate compiles for `wasm32-wasip2` (the guest) **and** for the host
//! target (tests + host glue). wit-bindgen cfg-gates the generated import:
//! on non-wasm targets it compiles to a stub that panics when invoked, so
//! native tests never call the real import — they inject an in-process
//! transport via [`WasiExecutor::with_transport`].
//!
//! There is no tokio here: the channel is synchronous end to end.
//!
//! [`WasiExecutor::with_transport`]: crate::WasiExecutor::with_transport

use lacrymira::{Command, Error, Reply};

/// Bindings generated from the `celestia:lacrymira/data-client` wit world.
///
/// The only import is `celestia::lacrymira::exec::execute`: msgpack bytes in,
/// msgpack bytes out (plus the `ErrorInfo` record mirroring the wit
/// `error-info`). Generated code is cfg-gated on `target_arch = "wasm32"`;
/// on the host target the import body is an `unreachable!()` stub.
#[cfg(feature = "wit")]
#[allow(missing_docs, unsafe_code)]
pub mod bindings {
    wit_bindgen::generate!({
        // Resolved from CARGO_MANIFEST_DIR (crates/wasi) → the repo-root wit dir.
        path: "../../wit",
        world: "data-client",
    });
}

// ── wire (de)serialization ──────────────────────────────────────────────────

/// Serialize a [`Command`](lacrymira::Command) to msgpack wire bytes.
///
/// Encoding the frozen contract types cannot practically fail (their msgpack
/// encoding is total); the error arm exists for exhaustiveness and maps to
/// [`Error::Config`](lacrymira::Error::Config).
pub fn encode_command(cmd: &Command) -> Result<Vec<u8>, Error> {
    rmp_serde::to_vec(cmd)
        .map_err(|e| Error::Config(format!("msgpack encode of Command failed: {e}")))
}

/// Deserialize a [`Command`](lacrymira::Command) from msgpack wire bytes.
pub fn decode_command(bytes: &[u8]) -> Result<Command, Error> {
    rmp_serde::from_slice(bytes)
        .map_err(|e| Error::Decode(format!("msgpack decode of Command failed: {e}")))
}

/// Serialize a [`Reply`](lacrymira::Reply) to msgpack wire bytes.
pub fn encode_reply(reply: &Reply) -> Result<Vec<u8>, Error> {
    rmp_serde::to_vec(reply)
        .map_err(|e| Error::Config(format!("msgpack encode of Reply failed: {e}")))
}

/// Deserialize a [`Reply`](lacrymira::Reply) from msgpack wire bytes.
pub fn decode_reply(bytes: &[u8]) -> Result<Reply, Error> {
    rmp_serde::from_slice(bytes)
        .map_err(|e| Error::Decode(format!("msgpack decode of Reply failed: {e}")))
}

// ── wit error-info ↔ core Error ─────────────────────────────────────────────

#[cfg(feature = "wit")]
pub use bindings::celestia::lacrymira::exec::ErrorInfo;

/// Prefix marking `error-info.kind` values reserved for the core error
/// variants, so they can never collide with a host-side database error kind
/// (those are free-form snake_case classifications like `connection`).
#[cfg(feature = "wit")]
pub const ERR_KIND_PREFIX: &str = "err:";

/// `error-info.kind` carrying [`Error::NotFound`](lacrymira::Error::NotFound).
#[cfg(feature = "wit")]
pub const KIND_NOT_FOUND: &str = "err:not_found";

/// `error-info.kind` carrying [`Error::Decode`](lacrymira::Error::Decode).
#[cfg(feature = "wit")]
pub const KIND_DECODE: &str = "err:decode";

/// `error-info.kind` carrying [`Error::Config`](lacrymira::Error::Config).
#[cfg(feature = "wit")]
pub const KIND_CONFIG: &str = "err:config";

#[cfg(feature = "wit")]
impl From<Error> for ErrorInfo {
    fn from(err: Error) -> Self {
        match err {
            // The wit error-info shape mirrors Db one-to-one.
            Error::Db {
                kind,
                message,
                sqlstate,
            } => ErrorInfo {
                kind,
                message,
                sqlstate,
            },
            Error::NotFound => ErrorInfo {
                kind: KIND_NOT_FOUND.into(),
                message: "record not found".into(),
                sqlstate: None,
            },
            Error::Decode(message) => ErrorInfo {
                kind: KIND_DECODE.into(),
                message,
                sqlstate: None,
            },
            Error::Config(message) => ErrorInfo {
                kind: KIND_CONFIG.into(),
                message,
                sqlstate: None,
            },
            Error::VersionConflict { expected, actual } => ErrorInfo {
                kind: "err:version_conflict".into(),
                message: format!("expected {expected}, actual {actual}"),
                sqlstate: None,
            },
            Error::IdCollision { attempts, id } => ErrorInfo {
                kind: "err:id_collision".into(),
                message: format!("{attempts} attempts, id: {id}"),
                sqlstate: None,
            },
        }
    }
}

#[cfg(feature = "wit")]
impl From<ErrorInfo> for Error {
    fn from(info: ErrorInfo) -> Self {
        if let Some(variant) = info.kind.strip_prefix(ERR_KIND_PREFIX) {
            match variant {
                "not_found" => return Error::NotFound,
                "decode" => return Error::Decode(info.message),
                "config" => return Error::Config(info.message),
                // Unknown reserved kind (e.g. from a newer world): degrade to
                // Db verbatim instead of losing it.
                _ => {}
            }
        }
        // Anything else — including every host-side driver classification —
        // is a database error carried verbatim.
        Error::Db {
            kind: info.kind,
            message: info.message,
            sqlstate: info.sqlstate,
        }
    }
}

// ── guest side ──────────────────────────────────────────────────────────────

/// The raw wire transport behind [`WasiExecutor`]: the byte-level surface of
/// the wit `exec.execute` import.
///
/// The production transport ([`WitImport`]) calls the generated import and is
/// only invocable inside a component (`wasm32`). Host-side code and native
/// tests inject their own implementation; closures
/// `Fn(&[u8]) -> Result<Vec<u8>, ErrorInfo>` implement it out of the box.
#[cfg(feature = "wit")]
pub trait WireTransport {
    /// Send msgpack-encoded `Command` bytes; get msgpack-encoded `Reply`
    /// bytes back, or the wit `error-info`.
    fn raw_execute(&self, cmd: &[u8]) -> Result<Vec<u8>, ErrorInfo>;
}

#[cfg(feature = "wit")]
impl<F> WireTransport for F
where
    F: Fn(&[u8]) -> Result<Vec<u8>, ErrorInfo>,
{
    fn raw_execute(&self, cmd: &[u8]) -> Result<Vec<u8>, ErrorInfo> {
        self(cmd)
    }
}

/// [`WireTransport`] that calls the generated wit import — the transport an
/// actual wasm component uses.
///
/// Compiles on every target, but on non-wasm targets the generated import is
/// an `unreachable!()` stub: invoking it there panics. Native tests must use
/// [`WasiExecutor::with_transport`] instead.
#[cfg(feature = "wit")]
#[derive(Debug, Clone, Copy, Default)]
pub struct WitImport;

#[cfg(feature = "wit")]
impl WireTransport for WitImport {
    fn raw_execute(&self, cmd: &[u8]) -> Result<Vec<u8>, ErrorInfo> {
        bindings::celestia::lacrymira::exec::execute(cmd)
    }
}

/// The guest-side executor: sends a `Command` through the wit world and
/// returns the decoded `Reply`.
///
/// `WasiExecutor::new()` wraps the generated wit import and is what a wasm
/// component constructs; [`with_transport`](WasiExecutor::with_transport)
/// swaps in any [`WireTransport`], which is the hook native tests and
/// embedders use.
#[cfg(feature = "wit")]
#[derive(Debug, Clone, Copy, Default)]
pub struct WasiExecutor<T = WitImport> {
    transport: T,
}

#[cfg(feature = "wit")]
impl WasiExecutor<WitImport> {
    /// Build the executor over the generated wit import (component side).
    pub fn new() -> Self {
        Self {
            transport: WitImport,
        }
    }
}

#[cfg(feature = "wit")]
impl<T: WireTransport> WasiExecutor<T> {
    /// Build the executor over a custom wire transport.
    pub fn with_transport(transport: T) -> Self {
        Self { transport }
    }

    /// Serialize `cmd` to msgpack, execute it through the wit world, and
    /// deserialize the `Reply`.
    ///
    /// Failures surface as the core [`Error`]: a wit `error-info` is mapped
    /// back (see the `err:`-prefix convention above), and a malformed reply
    /// payload is an [`Error::Decode`](lacrymira::Error::Decode).
    pub fn execute(&self, cmd: &Command) -> Result<Reply, Error> {
        let wire = encode_command(cmd)?;
        match self.transport.raw_execute(&wire) {
            Ok(reply_bytes) => decode_reply(&reply_bytes),
            Err(info) => Err(Error::from(info)),
        }
    }
}

// ── host side ───────────────────────────────────────────────────────────────

/// The host side of the `data-client` world: decode a `Command`, execute it,
/// answer with a `Reply`.
///
/// M3's Postgres executor will implement this trait. Any closure
/// `Fn(&Command) -> Result<Reply, Error>` already implements it, which is the
/// intended shape for tests and for embedding until M3 lands.
pub trait HostExecutor {
    /// Execute one decoded command.
    fn execute(&self, cmd: &Command) -> Result<Reply, Error>;
}

impl<F> HostExecutor for F
where
    F: Fn(&Command) -> Result<Reply, Error>,
{
    fn execute(&self, cmd: &Command) -> Result<Reply, Error> {
        self(cmd)
    }
}

/// Serve one raw wit-level `execute` call with a [`HostExecutor`].
///
/// This is the adapter a component-model host registers for the
/// `celestia:lacrymira/exec.execute` import: it decodes the msgpack `Command`
/// (rejecting malformed bytes before they reach the executor), delegates,
/// then encodes the `Reply` — or maps the [`Error`] into the wit `error-info`
/// shape.
#[cfg(feature = "wit")]
pub fn host_serve_bytes<H: HostExecutor + ?Sized>(
    host: &H,
    cmd: &[u8],
) -> Result<Vec<u8>, ErrorInfo> {
    let decoded = decode_command(cmd).map_err(ErrorInfo::from)?;
    let reply = host.execute(&decoded).map_err(ErrorInfo::from)?;
    encode_reply(&reply).map_err(ErrorInfo::from)
}

/// A [`WireTransport`] backed by an in-process [`HostExecutor`] via
/// [`host_serve_bytes`].
///
/// Native tests use this to drive the full wire path — msgpack both ways plus
/// the wit error mapping — without a component runtime: hand one to
/// [`WasiExecutor::with_transport`] and the guest and host sides run in the
/// same process.
#[cfg(feature = "wit")]
#[derive(Debug, Clone, Copy, Default)]
pub struct HostTransport<H> {
    host: H,
}

#[cfg(feature = "wit")]
impl<H: HostExecutor> HostTransport<H> {
    /// Wrap a host executor as an in-process wire transport.
    pub fn new(host: H) -> Self {
        Self { host }
    }
}

#[cfg(feature = "wit")]
impl<H: HostExecutor> WireTransport for HostTransport<H> {
    fn raw_execute(&self, cmd: &[u8]) -> Result<Vec<u8>, ErrorInfo> {
        host_serve_bytes(&self.host, cmd)
    }
}
