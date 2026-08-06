targets := "aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android"

build target:
    cargo build --target {{target}} --release

build-release-all:
    for target in {{targets}}; do \
        just build $target; \
    done
