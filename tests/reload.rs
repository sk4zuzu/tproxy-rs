use tproxy::{
    self,
    Result,
};

mod setup;
pub use setup::*;

// ---

use minijinja::{context as ctx};
use serial_test::{self};
use tokio::{self};

// ---

#[tokio::test]
#[serial_test::serial]
async fn test_one_bridge_one_service_modify_service_port() -> Result<()> {
    setup();

    let ctx0 = ctx! {
        prefix => tproxy::TPROXY_PREFIX,
        brdev => "tp0",
    };

    let ctx1 = ctx! {
        prefix => tproxy::TPROXY_PREFIX,
        brdev => "tp0",
        service_addr => tproxy::SERVICE_ADDR,
        endpoints => vec![
            ctx! {
                service_port => "1234",
                remote_addr => "127.0.0.1",
                remote_port => "4321",
            },
        ],
        assert_open => vec![ "1234" ],
        assert_closed => vec![ "2345" ],
    };

    let ctx2 = ctx! {
        prefix => tproxy::TPROXY_PREFIX,
        brdev => "tp0",
        service_addr => tproxy::SERVICE_ADDR,
        endpoints => vec![
            ctx! {
                service_port => "2345",
                remote_addr => "127.0.0.1",
                remote_port => "4321",
            },
        ],
        assert_open => vec![ "2345" ],
        assert_closed => vec![ "1234" ],
    };

    match async {
        run(&["sudo", "bash", "-xs"], "BASH_ENABLE_TPROXY", &ctx1).await?;
        run(&["sudo", "nft", "-ef-"], "NFT_ENABLE_EP_MAP", &ctx1).await?;

        run(&["sudo", "bash", "-xs"], "BASH_START_TPROXY", &ctx! {}).await?;

        run(&["sudo", "bash", "-xs"], "BASH_ASSERT_SERVICE_PORTS", &ctx1).await?;

        run(&["sudo", "bash", "-xs"], "BASH_ENABLE_TPROXY", &ctx2).await?;
        run(&["sudo", "nft", "-ef-"], "NFT_ENABLE_EP_MAP", &ctx2).await?;

        run(&["sudo", "bash", "-xs"], "BASH_RELOAD_TPROXY", &ctx! {}).await?;

        run(&["sudo", "bash", "-xs"], "BASH_ASSERT_SERVICE_PORTS", &ctx2).await?;

        Ok(())
    }.await {
        r => {
            run(&["sudo", "bash", "-xs"], "BASH_STOP_TPROXY", &ctx! {}).await.ok();

            run(&["sudo", "nft", "-ef-"], "NFT_DISABLE_EP_MAP", &ctx0).await.ok();
            run(&["sudo", "bash", "-xs"], "BASH_DISABLE_TPROXY", &ctx0).await.ok();

            r.map_err(tproxy::log_err)
        }
    }
}
