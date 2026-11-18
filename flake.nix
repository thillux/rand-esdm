{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    esdm = {
      url = "github:thillux/esdm/mtheil/1.2.4-finalize";
      inputs.nixpkgs.follows = "nixpkgs";
      inputs.flake-utils.follows = "flake-utils";
    };
    gitignore = {
      url = "github:hercules-ci/gitignore.nix";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, gitignore, flake-utils, esdm, ... }:
    let
      pkgs = nixpkgs.legacyPackages.x86_64-linux;
      esdmPkg = esdm.packages.x86_64-linux.esdm;
      buildInputs = [ esdmPkg ];
      nativeBuildInputs = with pkgs; [ pkg-config rustPlatform.bindgenHook ];
      inherit (import gitignore { inherit (pkgs) lib; }) gitignoreSource;
    in
    {
      packages.x86_64-linux = rec {
        default = pkgs.callPackage ./build.nix {
          inherit buildInputs nativeBuildInputs gitignoreSource;
        };

        # ESDM itself (server + CLI tools), for interactive testing:
        #   nix build .#esdm && sudo ./result/bin/esdm-server -f
        esdm = esdmPkg;

        run = pkgs.writeShellScriptBin "run" ''
          ${default}/bin/rand-esdm
        '';
      };

      devShells.x86_64-linux.default = pkgs.mkShell {
        inherit buildInputs;
        nativeBuildInputs = nativeBuildInputs ++ (with pkgs; [
          cargo
          rustc
          rust-analyzer
          clippy
          rustfmt
        ]);
        RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
      };
    };
}
