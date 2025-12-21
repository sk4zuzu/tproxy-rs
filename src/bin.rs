use tproxy::{
    self,
    Result,
    Operation,
    Cleanup,
    Config,
    Daemon,
    InnerProxy, OuterProxy, Proxy,
};

// ---

use env_logger::{self};
use tokio::{self};

// ---

fn main() -> Result<()> {
    match Operation::get()? {
        Operation::Config(maybe_brdev) => {
            println!("{}", Config::load_peer_config_unparsed(maybe_brdev)?);

            Ok(())
        },
        _ => {
            env_logger::init_from_env(
                env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "debug".to_owned()));

            let config = Config::load_peer_config(None)?;

            Cleanup::cancel_spurious_proxies(Some(&config))?;

            // silently refuse to start if no configuration is discovered
            if config.endpoints.is_empty() { return Ok(()) }

            Daemon::new(format!("{}", tproxy::TPROXY_PREFIX)).run(move || {
                tokio::runtime::Runtime::new()?.block_on(async {
                    OuterProxy::new().run().await.map_err(tproxy::log_err)
                })
            })?;

            for brdev in config.bridges {
                Daemon::new(format!("{}_{}", tproxy::TPROXY_PREFIX, brdev)).run(move || {
                    let netns = format!("{}_{}", tproxy::TPROXY_PREFIX, brdev);

                    tproxy::enter_named_netns(netns).map_err(tproxy::log_err)?;

                    tokio::runtime::Runtime::new()?.block_on(async {
                        InnerProxy::new(brdev).run().await.map_err(tproxy::log_err)
                    })
                })?;
            }

            Ok(())
        },
    }
}
