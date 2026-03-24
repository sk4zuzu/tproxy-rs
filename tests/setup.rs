use tproxy::{
    self,
    Result,
};

// ---

use env_logger::{self};
use indoc::{indoc as jinja};
use minijinja::{self, Value};
use std::net::SocketAddr;
use std::sync::LazyLock;
use tokio::{self};
use tokio::net::TcpListener;
use which::{self};

// ---

static JINJA: LazyLock<minijinja::Environment> = LazyLock::new(|| {
    let mut env = minijinja::Environment::new();

    env.add_template("BASH_ENABLE_TPROXY", jinja! {r#"
        set -e

        {# create local bridge (tproxy) #}

        ip link show {{ brdev }} || ip link add name {{ brdev }} type bridge

        ip link show {{ brdev }} && ip link set dev {{ brdev }} up

        {# create netns and veth pair (tproxy) #}

        ip link show {{ brdev }}b || ip link add {{ brdev }}b type veth peer name {{ brdev }}a

        ip link set dev {{ brdev }}b master {{ brdev }} up

        ip netns pids {{ prefix }}_{{ brdev }} || ip netns add {{ prefix }}_{{ brdev }}

        ip link show {{ brdev }}a && ip link set {{ brdev }}a netns {{ prefix }}_{{ brdev }}

        {# enable networking inside netns (tproxy) #}

        ip netns exec {{ prefix }}_{{ brdev }} ip address replace {{ service_addr }}/32 dev {{ brdev }}a

        ip netns exec {{ prefix }}_{{ brdev }} ip link set dev {{ brdev }}a up

        ip netns exec {{ prefix }}_{{ brdev }} ip route replace default dev {{ brdev }}a
    "#}).unwrap();

    env.add_template("BASH_ENABLE_GUEST", jinja! {r#"
        set -e

        {# create netns and veth pair (guest) #}

        ip link show {{ brdev }}d || ip link add {{ brdev }}d type veth peer name {{ brdev }}c

        ip link set dev {{ brdev }}d master {{ brdev }} up

        ip netns pids guest_{{ brdev }} || ip netns add guest_{{ brdev }}

        ip link show {{ brdev }}c && ip link set {{ brdev }}c netns guest_{{ brdev }}

        {# enable networking inside netns (guest) #}

        ip netns exec guest_{{ brdev }} ip address replace {{ guest_cidr }} dev {{ brdev }}c

        ip netns exec guest_{{ brdev }} ip link set dev {{ brdev }}c up

        ip netns exec guest_{{ brdev }} ip route replace default dev {{ brdev }}c
    "#}).unwrap();

    env.add_template("NFT_ENABLE_ARP_REDIR", jinja! {r#"
        table bridge {{ prefix }} {
            chain ch_{{ brdev }} {
                type filter hook forward priority filter; policy accept;
            };
        };

        flush chain bridge {{ prefix }} ch_{{ brdev }};

        table bridge {{ prefix }} {
            chain ch_{{ brdev }} {
                meta ibrname "{{ brdev }}" \
                oifname != "{{ brdev }}b" \
                arp operation request \
                arp daddr ip {{ service_addr }} \
                drop;
            };
        };
    "#}).unwrap();

    env.add_template("NFT_ENABLE_EP_MAP", jinja! {r#"
        table ip {{ prefix }} {
            map ep_{{ brdev }} {
                type inet_service : ipv4_addr . inet_service;
            };
        };

        flush map ip {{ prefix }} ep_{{ brdev }};

        {% for ep in endpoints %}
        add element ip {{ prefix }} ep_{{ brdev }} \
        { {{ ep.service_port }} : {{ ep.remote_addr }} . {{ ep.remote_port }} };
        {% endfor %}
    "#}).unwrap();

    env.add_template("BASH_START_TPROXY", jinja! {r#"
        exec ./target/debug/tproxy start
    "#}).unwrap();

    env.add_template("BASH_RELOAD_TPROXY", jinja! {r#"
        exec ./target/debug/tproxy reload
    "#}).unwrap();

    env.add_template("BASH_PING_SERVICE", jinja! {r#"
        set -e

        ip address show

        echo '3<>/dev/tcp/{{ service_addr }}/{{ service_port }}'
        exec  3<>/dev/tcp/{{ service_addr }}/{{ service_port }}

        for (( n = 1; n <= 5; n++ )); do
            SEND="PING$n"
            echo "$SEND" >&3
            sleep 1
            read -r RECV <&3
            echo "$SEND -> $RECV"
            [[ "$RECV" == "$SEND" ]]
        done
    "#}).unwrap();

    env.add_template("BASH_ASSERT_SERVICE_PORTS", jinja! {r#"
        set -e

        {% for service_port in assert_open | d([]) %}
        [[ -n "$(ss -N {{ prefix }}_{{ brdev }} -HQnt4 state listening src {{ service_addr }} sport {{ service_port }})" ]]
        {% endfor %}

        {% for service_port in assert_closed | d([]) %}
        [[ -z "$(ss -N {{ prefix }}_{{ brdev }} -HQnt4 state listening src {{ service_addr }} sport {{ service_port }})" ]]
        {% endfor %}
    "#}).unwrap();

    env.add_template("BASH_STOP_TPROXY", jinja! {r#"
        exec ./target/debug/tproxy stop
    "#}).unwrap();

    env.add_template("NFT_DISABLE_EP_MAP", jinja! {r#"
        table ip {{ prefix }} {
            map ep_{{ brdev }} {
                type inet_service : ipv4_addr . inet_service;
            };
        };

        delete map ip {{ prefix }} ep_{{ brdev }};
    "#}).unwrap();

    env.add_template("NFT_DISABLE_ARP_REDIR", jinja! {r#"
        table bridge {{ prefix }} {
            chain ch_{{ brdev }} {
                type filter hook forward priority filter; policy accept;
            };
        };

        delete chain bridge {{ prefix }} ch_{{ brdev }};
    "#}).unwrap();

    env.add_template("BASH_DISABLE_GUEST", jinja! {r#"
        set -e

        {# delete veth pair (guest) #}

        ip link show {{ brdev }}d && ip link delete {{ brdev }}d

        {# delete netns (guest) #}

        ip netns pids guest_{{ brdev }} && ip netns delete guest_{{ brdev }}
    "#}).unwrap();

    env.add_template("BASH_DISABLE_TPROXY", jinja! {r#"
        set -e

        {# delete veth pair (tproxy) #}

        ip link show {{ brdev }}b && ip link delete {{ brdev }}b

        {# delete netns (tproxy) #}

        ip netns pids {{ prefix }}_{{ brdev }} && ip netns delete {{ prefix }}_{{ brdev }}

        {# delete local bridge (tproxy) #}

        ip link show {{ brdev }} && ip link delete {{ brdev }} type bridge
    "#}).unwrap();

    env
});

// ---

pub async fn run(bin: &[&str], tpl: &str, ctx: &Value) -> Result<()> {
    let mut bin = bin.iter();

    let program = bin.next().unwrap();

    let args = bin.map(|arg| arg);

    let tpl = JINJA.get_template(tpl).unwrap().render(ctx).unwrap();

    let output = tokio::process::Command::new(program)
        .args(args)
        .stdin({
            use std::io::Write;

            let (ro, mut wo) = std::io::pipe().unwrap();

            wo.write_all(tpl.as_bytes()).unwrap();

            std::process::Stdio::from(ro)
        })
        .output()
        .await?;

    if !output.status.success() {
        Err(tproxy::TProxyError::Exited(output.status.code()))
    } else {
        println!("{}", String::from_utf8_lossy(&output.stderr));
        println!("{}", String::from_utf8_lossy(&output.stdout));
        Ok(())
    }
}

pub async fn tcp_echo(bind_addr: SocketAddr) -> Result<()> {
    let listener = TcpListener::bind(bind_addr).await?;
    loop {
        let (mut stream, _) = listener.accept().await?;
        tokio::spawn(async move {
            let (mut ro, mut wo) = tokio::io::split(&mut stream);
            tokio::io::copy(&mut ro, &mut wo).await
        });
    }
}

// ---

pub fn setup() {
    env_logger::try_init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "debug")).ok();

    which::which("bash").unwrap();
    which::which("sudo").unwrap();
    which::which("ip").unwrap();
    which::which("nft").unwrap();
    which::which("ss").unwrap();
}
