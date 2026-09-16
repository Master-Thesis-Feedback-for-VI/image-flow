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
  ];

  languages = {
    rust = {
      enable = true;
      channel = "nightly";
      targets = [
        "aarch64-linux-android"
        "aarch64-apple-ios"
        "aarch64-apple-ios-sim"
      ];
    };
  };

  android = {
    enable = true;
    emulator.enable = false;
    systemImages.enable = false;
    ndk = {
      enable = true;
      version = [ "26.1.10909125" ];
    };
  };
}

