use crate::{
    self as tproxy,
    Result, TProxyError,
    IfNam, Operation,
};

// ---

use regex::Regex;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::env::{self};
use std::net::IpAddr;
use std::process::Command;
use std::sync::LazyLock;

// ---

static RE_ENDPOINT_BRDEV: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^ep_([^/:\s]+)$").unwrap()
});

// ---

#[derive(Debug)]
pub struct Endpoint {
    pub brdev: HashSet<IfNam>,
    pub daddr: IpAddr,
    pub dport: u16,
}

#[derive(Debug)]
pub struct Config {
    pub endpoints: HashMap<u16, Endpoint>,
    pub bridges: HashSet<IfNam>,
}

impl Config {
    pub fn load_peer_config_unparsed(maybe_brdev: Option<IfNam>) -> Result<String> {
        // note that nft is executed here directly without nsenter, instead the same tproxy binary
        // is re-executed with the "config" command-line argument (this produces similar result
        // without extra dependency at the cost of somewhat increased complexity)

        if let Operation::Config(_) = Operation::get()? { tproxy::enter_default_netns()? }

        let output = if tproxy::in_default_netns()? {
            if let Some(brdev) = maybe_brdev {
                Command::new(which::which("nft")?)
                    .args(["-j", "list", "map", "ip", tproxy::TPROXY_PREFIX, &format!("ep_{}", brdev)])
                    .output()?
            } else {
                Command::new(which::which("nft")?)
                    .args(["-j", "list", "table", "ip", tproxy::TPROXY_PREFIX])
                    .output()?
            }
        } else {
            if let Some(brdev) = maybe_brdev {
                Command::new(env::current_exe()?)
                    .args(["config", &format!("{}", brdev)])
                    .output()?
            } else {
                Command::new(env::current_exe()?)
                    .args(["config"])
                    .output()?
            }
        };

        Ok(String::from_utf8(output.stdout)?)
    }

    pub fn load_peer_config(maybe_brdev: Option<IfNam>) -> Result<Config> {
        let unparsed = Self::load_peer_config_unparsed(maybe_brdev)?;

        let document: Value = serde_json::from_slice(unparsed.as_bytes())?;

        let Some(Value::Array(nftables)) = document.get("nftables") else { Err(TProxyError::InvalidSchema)? };

        let endpoints = nftables.into_iter().fold(HashMap::<u16, Endpoint>::new(), |mut acc, object| {
            let Some(map) = object.get("map") else { return acc };

            let brdev = {
                let Some(Value::String(name)) = map.get("name") else { return acc };

                let Some(caps) = RE_ENDPOINT_BRDEV.captures(name) else { return acc };

                IfNam::from((&caps[1]).as_bytes())
            };

            let Some(Value::Array(elem)) = map.get("elem") else { return acc };

            for item in elem {
                let (bport, daddr, dport) = {
                    let Value::Array(bport_daddr_dport) = item else { return acc };

                    let Some(bport) = (&bport_daddr_dport[0]).as_u64() else { return acc };

                    let Some(Value::Array(daddr_dport)) = (&bport_daddr_dport[1]).get("concat") else { return acc };

                    let Some(Ok(daddr)) = (&daddr_dport[0]).as_str().map(|s| s.parse::<IpAddr>()) else { return acc };

                    let Some(dport) = (&daddr_dport[1]).as_u64() else { return acc };

                    (bport as u16, daddr, dport as u16)
                };

                if let Some(ep) = acc.get_mut(&bport) {
                    ep.brdev.insert(brdev);
                    ep.daddr = daddr;
                    ep.dport = dport;
                } else {
                    acc.insert(bport, Endpoint {
                        brdev: HashSet::from([brdev]),
                        daddr: daddr,
                        dport: dport,
                    });
                }
            }

            acc
        });

        let bridges = endpoints.values().into_iter().fold(HashSet::<IfNam>::new(), |mut acc, ep| {
            acc.extend(ep.brdev.iter().cloned());
            acc
        });

        Ok(Config { endpoints, bridges })
    }
}
