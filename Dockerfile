# Builds the node, the command line wallet and the pool from the sources:
#
#   docker build -t tesserax .
#   docker run -p 30333:30333 -p 9944:9944 -v tesserax-data:/data tesserax --chain <spec>
#
# The chain specifications of chain-specs/ are in /usr/share/tesserax. Releases publish a ready
# image on the GitHub container registry instead.

FROM docker.io/library/rust:1.99.1-bookworm@sha256:48b7f435b8fc6378ce43b44f15e3a47fd8919b2825f835c39bd4bd3da50af5b8 AS builder

RUN apt-get update \
	&& apt-get install -y --no-install-recommends clang libclang-dev protobuf-compiler cmake pkg-config \
	&& rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY . .
RUN cargo build --release --locked -p tesserax-node -p tesserax-wallet -p tesserax-pool \
	&& mkdir /binaries \
	&& cp target/release/tesserax target/release/tesserax-wallet target/release/tesserax-pool /binaries/

FROM docker.io/library/debian:bookworm-slim@sha256:7ff6ba31527b9e336162fa061746c366c4b2032f4d2cff2078bea43f78efe3f8

RUN apt-get update \
	&& apt-get install -y --no-install-recommends ca-certificates \
	&& rm -rf /var/lib/apt/lists/* \
	&& useradd --create-home --home-dir /tesserax --uid 1000 --user-group --shell /usr/sbin/nologin tesserax \
	&& mkdir /data && chown tesserax:tesserax /data

COPY --from=builder /binaries/ /usr/local/bin/
COPY chain-specs/ /usr/share/tesserax/

USER tesserax
WORKDIR /tesserax
# Peer-to-peer, RPC, Prometheus, stratum of the node.
EXPOSE 30333 9944 9615 3333

ENTRYPOINT ["tesserax"]
CMD ["--base-path", "/data", "--rpc-external", "--rpc-methods", "safe"]
