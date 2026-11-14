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

