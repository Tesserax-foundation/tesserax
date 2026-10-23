# Public RPC gateway of Tesserax, built from the repository:
#
#   docker build -f rpc-gateway/Dockerfile -t tesserax-rpc-gateway .
#   docker run -p 443:8443 -v gateway-state:/data -e VERCEL_TOKEN=... tesserax-rpc-gateway \
#       --domain rpc.example.com --dns-zone example.com --upstream node:9945

FROM docker.io/library/rust:1.99.1-bookworm@sha256:48b7f435b8fc6378ce43b44f15e3a47fd8919b2825f835c39bd4bd3da50af5b8 AS builder

RUN apt-get update \
	&& apt-get install -y --no-install-recommends cmake \
	&& rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY . .
RUN cargo build --release --locked -p tesserax-rpc-gateway \
	&& cp target/release/tesserax-rpc-gateway /tesserax-rpc-gateway

FROM docker.io/library/debian:bookworm-slim@sha256:7ff6ba31527b9e336162fa061746c366c4b2032f4d2cff2078bea43f78efe3f8

RUN apt-get update \
	&& apt-get install -y --no-install-recommends ca-certificates \
	&& rm -rf /var/lib/apt/lists/* \
	&& useradd --create-home --home-dir /tesserax --uid 1000 --user-group --shell /usr/sbin/nologin tesserax \
	&& mkdir /data && chown tesserax:tesserax /data

COPY --from=builder /tesserax-rpc-gateway /usr/local/bin/

USER tesserax
WORKDIR /tesserax
EXPOSE 8443

ENTRYPOINT ["tesserax-rpc-gateway"]
