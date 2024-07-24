{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
    crane.inputs.nixpkgs.follows = "nixpkgs";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, crane, flake-utils, ... }:
    let
      pkgs = nixpkgs.legacyPackages.x86_64-linux;
      craneLib = crane.lib.x86_64-linux;
      buildInputs = with pkgs; [
        (esdm.overrideAttrs(prev: {
          src = fetchFromGitHub {
            owner = "smuellerDD";
            repo = "esdm";
            rev = "master";
            sha256 = "sha256-L/DKpMrB4mWuCbDQVTt4/7TjpO6eCvP3KNKj4Fw9qY4=";
          };
          # mesonBuildType = "debug";
          # dontStrip = true;
          # mesonFlags = prev.mesonFlags ++ [
          #   "-Dstrip=false"
          #   "-Ddebug=true"
          # ];
        }))
        protobufc
      ];
      nativeBuildInputs = with pkgs; [ pkg-config rustPlatform.bindgenHook ];
    in
    {
      packages.x86_64-linux = rec {
        default = pkgs.callPackage ./esdm/build.nix {
          inherit buildInputs nativeBuildInputs craneLib;
        };

        run = pkgs.writeShellScriptBin "run" ''
          ${default}/bin/rand-esdm
        '';
      };

      devShells.x86_64-linux.default = pkgs.mkShell {
        inherit buildInputs;
        inherit nativeBuildInputs;
      };
    };
}
