{
  description = "image-flow: the pipeline CLI binary";

  inputs = {
    nixpkgs.url = "https://flakehub.com/f/NixOS/nixpkgs/0";

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { nixpkgs, rust-overlay, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];

      forAllSystems = f: nixpkgs.lib.genAttrs systems f;
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };

          toolchain =
            pkgs.rust-bin.fromRustupToolchainFile
              ./rust-toolchain.toml;

          rustPlatform = pkgs.makeRustPlatform {
            cargo = toolchain;
            rustc = toolchain;
          };

          pipeline = rustPlatform.buildRustPackage {
            pname = "image-flow-pipeline";
            version = "0.3.0";

            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;

            OPENSSL_LIB_DIR = "${pkgs.lib.getLib pkgs.openssl}/lib";
            OPENSSL_INCLUDE_DIR = "${pkgs.lib.getDev pkgs.openssl}/include";
            PKG_CONFIG_PATH = "${pkgs.openssl.dev}/lib/pkgconfig";

            cargoBuildFlags = [
              "--package" "image_flow_lib"
              "--bin" "pipeline"
            ];

            buildFeatures = [
              "video"
              "cli"
            ];

            nativeBuildInputs = with pkgs; [
              pkg-config
              nasm
              openssl
              rustPlatform.bindgenHook
            ];

            buildInputs = with pkgs; [
              ffmpeg
              openssl
            ];

            doCheck = false;

            meta = {
              mainProgram = "pipeline";
              platforms = pkgs.lib.platforms.unix;
            };
          };
        in
        {
          default = pipeline;
          inherit pipeline;
        });
    };
}

