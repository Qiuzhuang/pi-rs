//! Running scripts: the host side ([`execution`]), the VM ([`vm`]) with its prelude, the
//! protocol between them, and the launchers that decide where a VM runs ([`host`]: a thread per
//! execution; [`remote`]: another process or machine).

pub mod execution;
pub mod host;
pub(crate) mod instrument;
pub mod prelude_source;
pub mod protocol;
pub mod remote;
pub mod vm;
