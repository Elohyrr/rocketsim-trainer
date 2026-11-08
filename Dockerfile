# Image du learner H24. Les meshes de collision ne sont JAMAIS dans l'image : ils sont téléchargés au
# démarrage du pod dans le volume (initContainer qui lance /opt/trainer/scripts/fetch-meshes.sh).

FROM debian:12 AS build
ENV CARGO_TERM_COLOR=never \
    CC=clang-16 CXX=clang++-16 \
    CFLAGS=-march=x86-64-v2 CXXFLAGS=-march=x86-64-v2 \
    RUSTFLAGS="-C target-cpu=x86-64-v2" \
    PATH=/root/.cargo/bin:$PATH
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl git build-essential clang-16 pkg-config \
 && rm -rf /var/lib/apt/lists/*
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
WORKDIR /src
COPY . .
# `game` (macroquad, manette) n'est pas construit : seulement le learner et ses dépendances.
RUN cargo build --release --locked -p learner \
 && strip target/release/learner

FROM debian:12-slim
# curl + ca-certificates : téléchargement des meshes (initContainer) ; libstdc++6 : RocketSim (C++).
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl libstdc++6 \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --uid 10001 --no-create-home --shell /usr/sbin/nologin trainer
COPY --from=build /src/target/release/learner /usr/local/bin/learner
COPY scripts/fetch-meshes.sh scripts/meshes.sha256 /opt/trainer/scripts/
ENV ROCKETSIM_MESHES_DIR=/data/collision_meshes
USER 10001:10001
WORKDIR /data
EXPOSE 9090
# Par défaut : mode cluster (sans limite de durée, reprise du dernier checkpoint, /metrics sur 9090).
ENTRYPOINT ["/usr/local/bin/learner"]
CMD ["--minutes", "0", "--resume", "--out", "/data/run", "--metrics", "0.0.0.0:9090"]
