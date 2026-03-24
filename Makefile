SELF := $(patsubst %/,%,$(dir $(abspath $(firstword $(MAKEFILE_LIST)))))

PACKAGE_NAME := tproxy
PACKAGE_LIB  := lib$(PACKAGE_NAME)
PACKAGE_BIN  := $(PACKAGE_NAME)

RUST_LOG       := debug
RUST_BACKTRACE := full

export

.PHONY: all t test b build c config d debug p purge

all: build

t test: build
	cd $(SELF)/ && cargo test --lib --bins --tests -- --nocapture --test-threads=1

t-% test-%: build
	cd $(SELF)/ && cargo test $* -- --nocapture

b build:
	cd $(SELF)/ && cargo build

c config: build
	cd $(SELF)/ && sudo ./target/debug/$(PACKAGE_BIN) config

d debug: build
	cd $(SELF)/ && sudo ./target/debug/$(PACKAGE_BIN) start

p purge:
	rm -rf $(SELF)/target/

.PHONY: s static

s static:
	cd $(SELF) && nix build
	cd $(SELF) && install -m u=rwx,go=rx ./result/bin/tproxy ./tproxy
	cd $(SELF) && nix develop --command strip --strip-unneeded ./tproxy
