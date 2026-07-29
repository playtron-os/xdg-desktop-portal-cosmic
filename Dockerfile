# libcosmic and cosmic-files both declare rust-version 1.93, so this cannot drop
# to the 1.92 the other Playtron repos pin.
ARG version=1.93
FROM rust:${version}
ARG version

RUN dpkg --add-architecture arm64

# Native build tooling. libclang is only needed on the host arch: bindgen
# (libspa-sys) runs natively even when cross-compiling.
RUN apt-get update && apt-get install -y \
    cmake \
    pkg-config \
    libclang-dev

# Portal build dependencies. This set mirrors the one the CI workflow installs
# (.github/workflows/ci.yml) and must exist for both the host and the arm64
# target so the cross link step can resolve every pkg-config dependency.
RUN apt-get install -y \
    libssl-dev \
    libglib2.0-dev \
    libegl-dev \
    libgbm-dev \
    libwayland-dev \
    libxkbcommon-dev \
    libpipewire-0.3-dev \
    libspa-0.2-dev \
    libgstreamer1.0-dev

RUN apt-get install -y \
    libssl-dev:arm64 \
    libglib2.0-dev:arm64 \
    libegl-dev:arm64 \
    libgbm-dev:arm64 \
    libwayland-dev:arm64 \
    libxkbcommon-dev:arm64 \
    libpipewire-0.3-dev:arm64 \
    libspa-0.2-dev:arm64 \
    libgstreamer1.0-dev:arm64

RUN apt-get install -y \
    g++-aarch64-linux-gnu \
    libc6-dev-arm64-cross

# Taskfile support
RUN curl -1sLf 'https://dl.cloudsmith.io/public/task/task/setup.deb.sh' | bash
RUN apt-get install -y task

# RPM support
RUN apt-get install -y rpm librpmbuild10 elfutils

RUN rustup target add aarch64-unknown-linux-gnu
RUN rustup component add clippy
RUN chmod -R 777 /usr/local/rustup

ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
ENV CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc
ENV CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++

# `libspa-sys` runs bindgen, so it needs the pipewire/spa headers on the include
# path. The Taskfile sets PKG_CONFIG_SYSROOT_DIR to a cross sysroot that does not
# exist under Debian multiarch (the :arm64 dev packages put headers in the native
# /usr/include), which rewrites pkg-config's -I flags into nothing. Naming the
# header dirs directly restores them. Dropping PKG_CONFIG_SYSROOT_DIR from the
# Taskfile instead would also fix it, at the cost of diverging from the other
# repos' docker:run target.
ENV C_INCLUDE_PATH=/usr/include/pipewire-0.3:/usr/include/spa-0.2
