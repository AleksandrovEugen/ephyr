#
# Stage 'build-ephyr' builds Ephyr for the final stage.
#

# https://github.com/jrottenberg/ffmpeg/blob/master/docker-images/4.3/centos7/Dockerfile
FROM jrottenberg/ffmpeg:4.3-centos7 AS build-ephyr


# Install Rust.
WORKDIR /tmp/rust/

ENV RUSTUP_HOME=/tmp/rust/rustup \
    CARGO_HOME=/tmp/rust/cargo \
    PATH=/tmp/rust/cargo/bin:$PATH

RUN curl -sLO https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init \
 && chmod +x rustup-init \
 && ./rustup-init -y --no-modify-path --profile minimal \
                  --default-toolchain stable \
 && chmod -R a+w $RUSTUP_HOME $CARGO_HOME \
 && rustup --version \
 && cargo --version \
 && rustc --version


# Install build dependencies.
RUN yum --enablerepo=extras install -y epel-release \
 && yum --enablerepo=epel install -y automake gcc libtool make \
                                     openssl-devel


# First, build all the dependencies to cache them in a separate Docker layer and
# avoid recompilation each time project sources are changed.
WORKDIR /tmp/ephyr/

COPY common/api/allatra-video/Cargo.toml ./common/api/allatra-video/
COPY common/log/Cargo.toml ./common/log/
COPY common/serde/Cargo.toml ./common/serde/
COPY components/mixer/Cargo.toml ./components/mixer/
COPY components/restreamer/Cargo.toml ./components/restreamer/
COPY components/vod-meta-server/Cargo.toml ./components/vod-meta-server/
COPY Cargo.toml Cargo.lock ./

RUN mkdir -p common/api/allatra-video/src/ \
 && touch common/api/allatra-video/src/lib.rs \
 && mkdir -p common/log/src/ \
 && touch common/log/src/lib.rs \
 && mkdir -p common/serde/src/ \
 && touch common/serde/src/lib.rs \
 && mkdir -p components/mixer/src/ \
 && touch components/mixer/src/lib.rs \
 && mkdir -p components/restreamer/src/ \
 && touch components/restreamer/src/lib.rs components/restreamer/src/main.rs \
 && mkdir -p components/vod-meta-server/src/ \
 && touch components/vod-meta-server/src/lib.rs

RUN cargo build -p ephyr-mixer --lib --release

# Finally, build the whole project.
RUN rm -rf ./target/release/.fingerprint/ephyr-*

COPY common/log/ ./common/log/
COPY components/mixer/ ./components/mixer/

RUN cargo build -p ephyr-mixer --bin ephyr-mixer --release




#
# Stage 'build-srs' prepares SRS distribution for the final stage.
#

# https://github.com/ossrs/srs-docker/blob/v3/Dockerfile
FROM ossrs/srs:3 AS build-srs




#
# Stage 'runtime' creates final Docker image to use in runtime.
#

# https://github.com/jrottenberg/ffmpeg/blob/master/docker-images/4.3/centos7/Dockerfile
FROM jrottenberg/ffmpeg:4.3-centos7 AS runtime

COPY --from=build-ephyr /tmp/ephyr/target/release/ephyr-mixer \
                        /usr/local/bin/ephyr-mixer

COPY --from=build-srs /usr/local/srs/ /usr/local/srs/

WORKDIR /usr/local/srs/
ENTRYPOINT  ["/usr/local/srs/objs/srs"]
CMD ["-c", "conf/srs.conf"]
