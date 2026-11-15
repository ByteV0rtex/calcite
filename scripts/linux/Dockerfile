# Linux build/test environment for Calcite (used locally and mirrors CI).
FROM rust:1-bookworm

RUN apt-get update && apt-get install -y --no-install-recommends \
      libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libvulkan-dev \
      libx11-xcb-dev libxcb1-dev libfontconfig-dev libfreetype-dev \
      libssl-dev pkg-config openjdk-17-jdk-headless \
      libgtk-3-dev libwebkit2gtk-4.1-dev xvfb mesa-vulkan-drivers \
    && rm -rf /var/lib/apt/lists/*

RUN rustup component add rustfmt clippy
ENV CARGO_TARGET_DIR=/target GRADLE_USER_HOME=/gradle HOME=/tmp/home
WORKDIR /src
