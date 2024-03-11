{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs";
    crane.url = "github:ipetkov/crane";
    crane.inputs.nixpkgs.follows = "nixpkgs";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, crane, flake-utils, ... }:
    let
      pkgs = nixpkgs.legacyPackages.x86_64-linux;
      craneLib = crane.lib.x86_64-linux;
      buildInputs = with pkgs; [
        (esdm.overrideAttrs (finalAttrs: previousAttrs: {
        version = "1.0.3";
        src = pkgs.fetchFromGitHub {
          owner = "thillux";
          repo = "esdm";
          rev = "sem-aux-lib";
          sha256 = "sha256-Y77SJ3OlxJn6J5FjqiFLs3QCV8wRaA1aDqOAKG/OYtM=";
        };
        mesonFlags = previousAttrs.mesonFlags ++ [
          # "-Desdm-server-term-on-signal=false"
        ];
      }))
      ];
      nativeBuildInputs = with pkgs; [ pkg-config ];
    in {
      packages.x86_64-linux = rec {
        default = let
          cFilter = path: _type: builtins.match ".*c$" path != null;
          cOrCargo = path: type:
            (cFilter path type) || (craneLib.filterCargoSources path type);
        in craneLib.buildPackage {
          src = pkgs.lib.cleanSourceWith {
            src = craneLib.path ./.;
            filter = cOrCargo;
          };
          doCheck = true;
          inherit buildInputs;
          inherit nativeBuildInputs;
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
