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
  ];

  languages = {
    rust = {
      enable = true;
      channel = "nightly";
      targets = [
        "aarch64-linux-android"
        "armv7-linux-androideabi"
        "i686-linux-android"
        "x86_64-linux-android"
        "aarch64-apple-ios"
        "aarch64-apple-ios-sim"
        "x86_64-apple-ios"
      ];
    };
  };

  android = {
    enable = true;
    abis = [
      "arm64-v8a"
      "armeabi-v7a"
      "x86"
      "x86_64"
    ];
    emulator.enable = false;
    systemImages.enable = false;
    ndk = {
      enable = true;
      version = [ "26.1.10909125" ];
    };
  };
}

