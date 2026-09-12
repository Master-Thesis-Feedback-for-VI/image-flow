androidTargets := "aarch64-linux-android"
iosTargets := "aarch64-apple-ios aarch64-apple-ios-sim"
targets := androidTargets + " " + iosTargets

# Android API level passed to cargo-ndk (-p)
ndkPlatform := "30"

default: build-release-all

[group('tool-chain')]
install-toolchains:
    rustup target add {{targets}}
    # cargo install cargo-ndk

[group('build')]
build target:
    # IPHONEOS_DEPLOYMENT_TARGET=15.0 cargo build --target {{target}} --release --lib
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
