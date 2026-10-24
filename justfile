androidTargets := "aarch64-linux-android"
iosTargets := "aarch64-apple-ios aarch64-apple-ios-sim"
targets := androidTargets + " " + iosTargets

# Android API level passed to cargo-ndk (-p)
ndkPlatform := "30"

default: build-release-all

[group('tests')]
test:
    cargo test --workspace

[group('tests')]
test-core:
    cargo test --workspace --no-default-features

[group('tests')]
test-all:
    cargo test --workspace --all-features

# Opt-in: live map services, ONNX inference, and large sample recordings.
[group('tests')]
test-integration:
    cargo test -p image_flow_lib --lib -- --ignored --test-threads=1

[group('tool-chain')]
install-toolchains:
    rustup target add {{targets}}
    # cargo install cargo-ndk

[group('build')]
build target:
    cargo build --target {{target}} --release --lib --no-default-features

[group('build')]
build-android target:
    cargo ndk -t {{target}} -P {{ndkPlatform}} build --release --lib

[group('build')]
build-multiple target_multiple:
    for target in {{target_multiple}}; do \
        just build $target; \
    done

[group('build')]
build-multiple-android target_multiple:
    for target in {{target_multiple}}; do \
        just build-android $target; \
    done

[group('build')]
build-unix:
    just build-multiple-android '{{androidTargets}}'

[group('build')]
[macos]
build-mac:
    just build-multiple '{{iosTargets}}'

[group('build')]
build-release-all: build-unix build-mac

[group('bindings')]
[macos]
[script]
create-bindings-swift:
    lang="swift"
    aarch="aarch64-apple-ios-sim"
    just build "$aarch"
    mkdir -p "./bindings/$lang"
    rm -rf ./bindings/$lang/*
    cargo run -p uniffi-bindgen-runner -- generate \
      --library "./target/$aarch/release/libimage_flow.dylib" \
      --language "$lang" \
      --out-dir "./bindings/$lang"

    mv ./bindings/swift/image_flowFFI.modulemap ./bindings/swift/module.modulemap

[group('bindings')]
[script]
create-bindings-kotlin:
    lang="kotlin"
    aarch="aarch64-linux-android"
    just build-android "$aarch"
    mkdir -p "./bindings/$lang"
    rm -rf ./bindings/$lang/*
    cargo run -p uniffi-bindgen-runner -- generate \
      --library "./target/$aarch/release/libimage_flow.so" \
      --language "$lang" \
      --no-format \
      --out-dir "./bindings/$lang"

[group('bindings')]
[macos]
swift-build-xcode-bindings: build-mac create-bindings-swift
    mkdir -p target/ios-universal-sim/release
    rm -rf target/ios-universal-sim/release/*
    cp target/aarch64-apple-ios-sim/release/libimage_flow.a target/ios-universal-sim/release/libimage_flow.a

    rm -rf ios/Mobile.xcframework

    xcodebuild -create-xcframework \
        -library target/aarch64-apple-ios/release/libimage_flow.a \
        -headers bindings/swift/ \
        -library target/ios-universal-sim/release/libimage_flow.a \
        -headers bindings/swift/ \
        -output ios/Mobile.xcframework

[group('bindings')]
create-bindings-all: create-bindings-swift create-bindings-kotlin

[group('house keeping')]
clean:
    cargo clean
    rm -rf ./.android ./bindings
    rm -rf out pipeline_out

[group('house keeping')]
clean_out:
    rm -rf ./out
    rm -rf ./pipeline_out

tmp_output := "pipeline_out"

[group('scripts')]
pipeline input_path tmp_out serialized_output:
    mkdir -p '{{tmp_out}}'
    cargo run --bin pipeline --release --features="video, cli" -- \
        --input '{{input_path}}' \
        -s '{{serialized_output}}' \
        --output '{{tmp_out}}'

[group('scripts')]
pipeline_all: clean_out
    mkdir -p \
        '{{"." / tmp_output / "tmp_out"}}' \
        '{{"." / tmp_output / "results"}}' 
    just pipeline \
        '{{"." / "sample_data"}}' \
        '{{"." / tmp_output / "tmp_out"}}' \
        '{{"." / tmp_output / "results"}}' 
