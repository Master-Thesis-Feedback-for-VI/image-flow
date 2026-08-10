androidTargets := "aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android"
iosTargets := "aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios"
targets := androidTargets + " " + iosTargets

languages := "kotlin swift"

default: build-release-all

[group('tool-chain')]
install-toolchains:
    rustup target add {{targets}}
    
[group('build')]
build target:
    cargo build --target {{target}} --release

[group('build')]
build-multiple target_multiple:
    for target in {{target_multiple}}; do \
        just build $target; \
    done

[group('build')]
build-unix:
    just build-multiple '{{androidTargets}}'

[group('build')]
[macos]
build-mac:
    just build-multiple '{{iosTargets}}'

[group('build')]
build-release-all: build-unix build-mac

[group('bindings')]
create-bindings lang:
    mkdir -p "./bindings/$lang"; \
    rm -rf "./bindings/$lang/*"; \
    cargo run -p uniffi-bindgen -- generate \
      --library "./target/aarch64-linux-android/release/libimage_flow.so" \
      --language "$lang" \
      --out-dir "./bindings/$lang"; \

create-bindings-all: 
    just build "aarch64-linux-android"
    for lang in {{languages}}; do \
        just create-bindings $lang; \
    done

    mv ./bindings/swift/image_flowFFI.modulemap ./bindings/swift/module.modulemap
    

[group('bindings')]
[macos]
swift-build-bindings: create-bindings build-mac
    mkdir -p target/ios-universal-sim/release
    rm -rf target/ios-universal-sim/release/*

    lipo -create \
        target/aarch64-apple-ios-sim/release/libimage_flow.a \
        target/x86_64-apple-ios/release/libimage_flow.a \
        -output target/ios-universal-sim/release/libimage_flow.a

    rm -rf ios/Mobile.xcframework

    xcodebuild -create-xcframework \
        -library target/aarch64-apple-ios/release/libimage_flow.a \
        -headers bindings/swift/ \
        -library target/ios-universal-sim/release/libimage_flow.a \
        -headers bindings/swift/ \
        -output ios/Mobile.xcframework
