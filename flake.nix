{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    gitignore = {
      url = "github:hercules-ci/gitignore.nix";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, gitignore, flake-utils, ... }:
    let
      pkgs = nixpkgs.legacyPackages.x86_64-linux;
      buildInputs = with pkgs; [
        (esdm.overrideAttrs(prev: {
          src = fetchFromGitHub {
            owner = "thillux";
            repo = "esdm";
            rev = "client-close-on-exec";
            sha256 = "sha256-HDvnfoZuEsW7aOBBFWF4AXGhu67r76YHmdPfB1ux/2Q=";
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
      inherit (import gitignore { inherit (pkgs) lib; }) gitignoreSource;
    in
    {
      packages.x86_64-linux = rec {
        default = pkgs.callPackage ./build.nix {
          inherit buildInputs nativeBuildInputs gitignoreSource;
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
