use tproxy::{
    self,
    Result,
    TProxyError,
};

mod setup;
pub use setup::*;

// ---

use futures::{self};
use minijinja::{context as ctx};
use serial_test::{self};
use tokio::{self};

// ---

#[tokio::test]
#[serial_test::serial]
async fn test_two_bridges_two_services_e2e() -> Result<()> {
    setup();

    let ctx0 = ctx! {
        prefix => tproxy::TPROXY_PREFIX,
        brdev => "tp0",
        service_addr => tproxy::SERVICE_ADDR,
        endpoints => vec![
            ctx! {
                service_port => "1234",
                remote_addr => "127.0.0.1",
                remote_port => "4321",
            },
            ctx! {
                service_port => "2345",
                remote_addr => "127.0.0.1",
                remote_port => "5432",
            },
        ],
        guest_cidr => "10.9.8.7/24",
    };

    let ctx1 = ctx! {
        prefix => tproxy::TPROXY_PREFIX,
        brdev => "tp1",
        service_addr => tproxy::SERVICE_ADDR,
        endpoints => vec![
            ctx! {
                service_port => "1234",
                remote_addr => "127.0.0.1",
                remote_port => "4321",
            },
            ctx! {
                service_port => "2345",
                remote_addr => "127.0.0.1",
                remote_port => "5432",
            },
        ],
        guest_cidr => "10.11.12.13/24",
    };

    match async {
        run(&["doas", "bash", "-xs"], "BASH_ENABLE_TPROXY", &ctx0).await?;
        run(&["doas", "bash", "-xs"], "BASH_ENABLE_GUEST", &ctx0).await?;
        run(&["doas", "nft", "-ef-"], "NFT_ENABLE_ARP_REDIR", &ctx0).await?;
        run(&["doas", "nft", "-ef-"], "NFT_ENABLE_EP_MAP", &ctx0).await?;

        run(&["doas", "bash", "-xs"], "BASH_ENABLE_TPROXY", &ctx1).await?;
        run(&["doas", "bash", "-xs"], "BASH_ENABLE_GUEST", &ctx1).await?;
        run(&["doas", "nft", "-ef-"], "NFT_ENABLE_ARP_REDIR", &ctx1).await?;
        run(&["doas", "nft", "-ef-"], "NFT_ENABLE_EP_MAP", &ctx1).await?;

        run(&["doas", "bash", "-xs"], "BASH_START_TPROXY", &ctx! {}).await?;

        loop {
            tokio::select! {
                r = {
                    let echos = ctx0.get_attr("endpoints").unwrap().try_iter().unwrap().map(|ep| {
                        async move {
                            let remote_port = ep.get_attr("remote_port")
                                                .unwrap()
                                                .as_str()
                                                .unwrap()
                                                .parse::<u16>()
                                                .unwrap();
                            setup::tcp_echo(([0, 0, 0, 0], remote_port).into()).await
                        }
                    });
                    futures::future::try_join_all(echos)
                } => match r {
                    Ok(_) => break Ok(()), Err(_) => break Err(TProxyError::Fatal),
                },
                r = async {
                    let pings0 = ctx0.get_attr("endpoints").unwrap().try_iter().unwrap().map(|ep| {
                        let brdev = ctx0.get_attr("brdev")
                                        .unwrap()
                                        .to_string();
                        let service_addr = ctx0.get_attr("service_addr")
                                               .unwrap()
                                               .to_string();
                        async move {
                            let netns = format!("guest_{}", brdev);
                            let service_port = ep.get_attr("service_port")
                                                 .unwrap()
                                                 .to_string();
                            run(
                                &["doas", "ip", "netns", "exec", &netns, "bash", "-xs"],
                                "BASH_PING_SERVICE",
                                &ctx! {
                                    service_addr => service_addr,
                                    service_port => service_port,
                                },
                            ).await
                        }
                    });
                    let pings1 = ctx1.get_attr("endpoints").unwrap().try_iter().unwrap().map(|ep| {
                        let brdev = ctx1.get_attr("brdev")
                                        .unwrap()
                                        .to_string();
                        let service_addr = ctx1.get_attr("service_addr")
                                               .unwrap()
                                               .to_string();
                        async move {
                            let netns = format!("guest_{}", brdev);
                            let service_port = ep.get_attr("service_port")
                                                 .unwrap()
                                                 .to_string();
                            run(
                                &["doas", "ip", "netns", "exec", &netns, "bash", "-xs"],
                                "BASH_PING_SERVICE",
                                &ctx! {
                                    service_addr => service_addr,
                                    service_port => service_port,
                                },
                            ).await
                        }
                    });
                    futures::future::try_join_all(pings0).await?;
                    futures::future::try_join_all(pings1).await?;
                    Ok::<(), TProxyError>(())
                } => match r {
                    Ok(_) => break Ok(()), Err(_) => break Err(TProxyError::Fatal),
                },
            };
        }
    }.await {
        r => {
            run(&["doas", "bash", "-xs"], "BASH_STOP_TPROXY", &ctx! {}).await.ok();

            run(&["doas", "nft", "-ef-"], "NFT_DISABLE_EP_MAP", &ctx0).await.ok();
            run(&["doas", "nft", "-ef-"], "NFT_DISABLE_ARP_REDIR", &ctx0).await.ok();
            run(&["doas", "bash", "-xs"], "BASH_DISABLE_GUEST", &ctx0).await.ok();
            run(&["doas", "bash", "-xs"], "BASH_DISABLE_TPROXY", &ctx0).await.ok();

            run(&["doas", "nft", "-ef-"], "NFT_DISABLE_EP_MAP", &ctx1).await.ok();
            run(&["doas", "nft", "-ef-"], "NFT_DISABLE_ARP_REDIR", &ctx1).await.ok();
            run(&["doas", "bash", "-xs"], "BASH_DISABLE_GUEST", &ctx1).await.ok();
            run(&["doas", "bash", "-xs"], "BASH_DISABLE_TPROXY", &ctx1).await.ok();

            r.map_err(tproxy::log_err)
        }
    }
}
