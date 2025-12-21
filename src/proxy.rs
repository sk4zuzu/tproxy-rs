use crate::{
    self as tproxy,
    Result,
    IfNam,
    Config,
    InnerPeer, OuterPeer, Peer,
};

// ---

use std::collections::HashMap;
use tokio::signal::unix::{signal, SignalKind};
use tokio::spawn;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

// ---

pub trait Proxy<P>: Send {
    fn run(&mut self) -> impl Future<Output = Result<()>> + Send {
        async {
            let mut sighup = signal(SignalKind::hangup())?;
            loop {
                self.reload().await?;
                sighup.recv().await;
            }
        }
    }

    fn reload(&mut self) -> impl Future<Output = Result<()>> + Send;
}

// ---

pub struct InnerProxy {
    brdev: IfNam,
    peers: HashMap<(IfNam, u16), (CancellationToken, JoinHandle<Result<()>>)>,
}

impl InnerProxy {
    pub fn new(brdev: IfNam) -> Self { Self { brdev, peers: HashMap::new() } }
}

impl Proxy<InnerPeer> for InnerProxy {
    fn reload(&mut self) -> impl Future<Output = Result<()>> + Send {
        async {
            let config = Config::load_peer_config(Some(self.brdev))?;

            // stop and remove cancelled peers
            self.peers.retain(|(brdev, service_port), (ctoken1, jhandle)| {
                if let Some(ep) = config.endpoints.get(&service_port) {
                    if ep.brdev.contains(&brdev) { return true }
                }
                ctoken1.cancel();
                jhandle.abort();
                false
            });

            // create and start missing peers
            for (service_port, ep) in config.endpoints {
                for brdev in ep.brdev {
                    if self.peers.contains_key(&(brdev, service_port)) { continue }

                    let ctoken1 = CancellationToken::new();
                    let ctoken2 = ctoken1.clone();

                    let jhandle = spawn(async move {
                        InnerPeer::new(
                            format!("{}:{}", tproxy::SERVICE_ADDR, service_port).parse()?,
                            tproxy::get_socket_path(service_port),
                            ctoken2
                        ).run().await.map_err(tproxy::log_err)
                    });

                    self.peers.insert((brdev, service_port), (ctoken1, jhandle));
                }
            }

            Ok(())
        }
    }
}

// ---

pub struct OuterProxy {
    peers: HashMap<u16, (CancellationToken, JoinHandle<Result<()>>)>,
}

impl OuterProxy {
    pub fn new() -> Self { Self { peers: HashMap::new() } }
}

impl Proxy<OuterPeer> for OuterProxy {
    fn reload(&mut self) -> impl Future<Output = Result<()>> + Send {
        async {
            let config = Config::load_peer_config(None)?;

            // stop and remove cancelled peers
            self.peers.retain(|service_port, (ctoken1, jhandle)| {
                if config.endpoints.contains_key(&service_port) { return true }
                ctoken1.cancel();
                jhandle.abort();
                false
            });

            // create and start missing peers
            for (service_port, ep) in config.endpoints {
                if self.peers.contains_key(&service_port) { continue }

                let ctoken1 = CancellationToken::new();
                let ctoken2 = ctoken1.clone();

                let jhandle = spawn(async move {
                    OuterPeer::new(
                        tproxy::get_socket_path(service_port),
                        format!("{}:{}", ep.daddr, ep.dport).parse()?,
                        ctoken2
                    ).run().await.map_err(tproxy::log_err)
                });

                self.peers.insert(service_port, (ctoken1, jhandle));
            }

            Ok(())
        }
    }
}
