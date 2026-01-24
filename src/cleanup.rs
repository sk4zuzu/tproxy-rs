use crate::{
    self as tproxy,
    Result,
    IfNam,
    Config,
    DaemonDetector,
};

// ---

use nix::sys::signal::{self};
use nix::unistd::Pid;
use regex::Regex;
use std::process::{self};
use std::sync::LazyLock;

// ---

static RE_PREFIX_BRDEV: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"^{}_([^/:\s]+)$", tproxy::TPROXY_PREFIX)).unwrap()
});

static RE_PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"^{}$", tproxy::TPROXY_PREFIX)).unwrap()
});

// ---

pub struct Cleanup;

impl Cleanup {
    pub fn cancel_spurious_proxies(maybe_config: Option<&Config>) -> Result<()> {
        // remove proxies/peers that look like leftovers from previous/other runs
        // (for bridges that are completely missing from the running config)

        let config = match maybe_config {
            Some(v) => v, None => &Config::load_peer_config(None)?,
        };

        let spurious = DaemonDetector::new()?.fold(Vec::<Pid>::new(), |mut acc, (pid, cmd)| {
            if let Some(caps) = RE_PREFIX_BRDEV.captures(&cmd) {
                if !config.bridges.contains(&IfNam::from((&caps[1]).as_bytes())) {
                    acc.push(pid);
                }
                return acc
            }

            if RE_PREFIX.is_match(&cmd) {
                if config.endpoints.is_empty() && (pid.as_raw() as u32) != process::id() {
                    acc.push(pid);
                }
                return acc
            }

            acc
        });

        for pid in spurious { signal::kill(pid, signal::SIGTERM).ok(); }

        Ok(())
    }
}
