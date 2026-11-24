{
  pkgs,
  lib,
  config,
  ...
}:

{
  packages = with pkgs; [
    just
    cargo-ndk
    openssl
    pkg-config
    ffmpeg
    nasm
    git
    git-lfs
    cargo-udeps
    cargo-insta
    imagemagick
  ];

  # ffmpeg-sys-next uses bindgen, which loads libclang dynamically.
  env.LIBCLANG_PATH = "${lib.getLib pkgs.llvmPackages.libclang}/lib";

  scripts.pipeline-all = {
    package = pkgs.nushell;
    binary = "nu";
    description = "Clear pipeline output and process all sample recordings";
    exec = ''
      # Delete out/ and pipeline_out/, then process all ZIPs in sample_data/.
      def main [] {
        cd $env.DEVENV_ROOT
        # Treat the two output paths literally, even if the checkout path contains glob characters.
        ^${pkgs.coreutils}/bin/rm -rf -- ./out ./pipeline_out
        mkdir pipeline_out/tmp_out pipeline_out/results

        (^cargo run -p image_flow_lib --bin pipeline --release --features "video,cli" --
          --input sample_data
          --serialized-output pipeline_out/results
          --output pipeline_out/tmp_out
          --error-log pipeline_out/errors.log)
      }
    '';
  };

  languages = {
    rust = {
      enable = true;
      channel = "nightly";
      components = [
        "rustc"
        "cargo"
        "clippy"
        "rustfmt"
        "rust-analyzer"
      ];
      targets = [
        "aarch64-linux-android"
        "aarch64-apple-ios"
        "aarch64-apple-ios-sim"
      ];
    };
  };

  dotenv = {
    enable = true;
    filename = [".env" ".env.production" ".env.development"];
  };
}

