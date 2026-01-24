use crate::{Result, TProxyError};

// ---

use libc::{self};
use nix::sched::{self};
use std::cmp::{self};
use std::env::{self};
use std::fs::OpenOptions;
use std::fmt::{self, Debug, Display};
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::process::{self};

// ---

pub const TPROXY_PREFIX: &'static str = "one_tproxy";
pub const SERVICE_ADDR: &'static str = "169.254.16.9";

pub const RUN_LOCATION: &'static str = "/var/run";
pub const LOG_LOCATION: &'static str = "/var/log";

// ---

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct IfNam { bytes: [u8; libc::IFNAMSIZ] }

impl Debug for IfNam {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // the resulting string has to be truncated so there are no
        // 0-valued bytes as it may cause problems in various places
        let n = match self.bytes.into_iter().position(|b| b == 0) {
            Some(n) => n, None => libc::IFNAMSIZ,
        };
        write!(f, "{}", String::from_utf8_lossy(&self.bytes[..n]))
    }
}

impl Display for IfNam {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // the resulting string has to be truncated so there are no
        // 0-valued bytes as it may cause problems in various places
        let n = match self.bytes.into_iter().position(|b| b == 0) {
            Some(n) => n, None => libc::IFNAMSIZ,
        };
        write!(f, "{}", String::from_utf8_lossy(&self.bytes[..n]))
    }
}

impl From<&[u8]> for IfNam {
    fn from(src: &[u8]) -> Self {
        // the interface's name length must be capped at the value of IFNAMSIZ
        let n = cmp::min(src.len(), libc::IFNAMSIZ - 1);
        let mut dst = Self { bytes: [0; libc::IFNAMSIZ] };
        dst.bytes[..n].copy_from_slice(&src[..n]);
        dst
    }
}

// ---

pub enum Operation { Config(Option<IfNam>), None, Reload, Restart, Start, Status, Stop }

impl Operation {
    pub fn get() -> Result<Operation> {
        match env::args().nth(1) {
            Some(operation) => {
                match operation.as_str() {
                    "config" => match env::args().nth(2) {
                        Some(v) => Ok(Operation::Config(Some(IfNam::from(v.as_bytes())))),
                        None => Ok(Operation::Config(None)),
                    },
                    "reload" => Ok(Operation::Reload),
                    "restart" => Ok(Operation::Restart),
                    "start" => Ok(Operation::Start),
                    "status" => Ok(Operation::Status),
                    "stop" => Ok(Operation::Stop),
                    _ => Err(TProxyError::InvalidOperation),
                }
            },
            None => Ok(Operation::None),
        }
    }
}

// ---

pub fn get_socket_path(service_port: u16) -> PathBuf {
    format!("{}/{}_{}.sock", RUN_LOCATION, TPROXY_PREFIX, service_port).into()
}

// ---

#[cfg(target_os = "linux")]
pub fn in_default_netns() -> Result<bool> {
    let current_netns = OpenOptions::new()
        .read(true)
        .write(false)
        .open(format!("/proc/{}/ns/net", process::id()))?;
    let current_netns_inode = current_netns.metadata()?.ino();

    let default_netns = OpenOptions::new()
        .read(true)
        .write(false)
        .open("/proc/1/ns/net")?;
    let default_netns_inode = default_netns.metadata()?.ino();

    Ok(current_netns_inode == default_netns_inode)
}

#[cfg(target_os = "linux")]
pub fn enter_default_netns() -> Result<()> {
    let default_netns = OpenOptions::new()
        .read(true)
        .write(false)
        .open("/proc/1/ns/net")?;

    sched::setns(default_netns, sched::CloneFlags::CLONE_NEWNET)?;

    Ok(())
}

#[cfg(target_os = "linux")]
pub fn enter_named_netns(name: String) -> Result<()> {
    let named_netns = OpenOptions::new()
        .read(true)
        .write(false)
        .open(format!("/var/run/netns/{}", name))?; // this is what iproute2 does

    sched::setns(named_netns, sched::CloneFlags::CLONE_NEWNET)?;

    Ok(())
}
