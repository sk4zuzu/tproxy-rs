## OPENNEBULA TPROXY RE-IMPLEMENTATION IN TOKIO/RUST

Please refer to the official documentation [OpenNebula Transparent Proxies](https://docs.opennebula.io/7.0/product/virtual_machines_operation/virtual_machines_networking/tproxy/) first. :point_up::relieved:

To build statically-linked **tproxy-rs** binary you can use included `flake.nix` and `Makefile`:
```shell
make static
```

```shell
file ./result/bin/tproxy
./result/bin/tproxy: ELF 64-bit LSB pie executable, x86-64, version 1 (SYSV), static-pie linked, not stripped
```

To install **tproxy-rs** in an OpenNebula environment copy the statically-linked binary to `~oneadmin/remotes/vnm/tproxy` and re-sync HV nodes:
```shell
sudo -u oneadmin onehost sync -f
```

To run integration tests (requires root privileges via `doas`):
```shell
make test
```
