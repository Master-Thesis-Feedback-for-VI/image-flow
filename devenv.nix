{
  pkgs,
  lib,
  config,
  ...
}:

{
  packages = [
    pkgs.just
    pkgs.cargo-ndk
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

