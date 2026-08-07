androidTargets := "aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android"
iosTargets := "aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios"
targets := androidTargets + " " + iosTargets

install-toolchains:
    rustup target add {{targets}}
    
build target:
    cargo build --target {{target}} --release

build-release-all:
    for target in {{targets}}; do \
        just build $target; \
    done

create-bindings: 
    cargo run -p uniffi-bindgen -- generate \
      --library "./target/aarch64-apple-ios/release/libimage_flow.dylib" \
      --language swift \
      --out-dir ./bindings
    mv ./bindings/image_flowFFI.modulemap ./bindings/module.modulemap
    
swift-build-bindings:
    for target in {{iosTargets}}; do \
        just build "$target"; \
    done

    just create-bindings

    mkdir -p target/ios-universal-sim/release

    lipo -create \
        target/aarch64-apple-ios-sim/release/libimage_flow.a \
        target/x86_64-apple-ios/release/libimage_flow.a \
        -output target/ios-universal-sim/release/libimage_flow.a

    rm -rf ios/Mobile.xcframework

    xcodebuild -create-xcframework \
        -library target/aarch64-apple-ios/release/libimage_flow.a \
        -headers bindings \
        -library target/ios-universal-sim/release/libimage_flow.a \
        -headers bindings \
        -output ios/Mobile.xcframework
