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
        ((esdm.override {
          jitterentropy = pkgs.jitterentropy.overrideAttrs (
                _: prevAttrs: {
                  version = "3.7.0";
                  src = pkgs.fetchFromGitHub {
                    owner = "smuellerDD";
                    repo = "jitterentropy-library";
                    rev = "e7bf6282407d1ea52815cdd7746b4c086c0b19af";
                    hash = "sha256-PC8CQBRjJKWWfSLuEWyl09yjxZ9XS2ZGI7OMSFPwZ48=";
                  };
                  # for secure memory
                  propagatedBuildInputs = [
                    pkgs.openssl
                  ];
                  patches = [ ];
                  postPatch = ''
                    sed -i '/add_subdirectory(tests\/gcd)/d' CMakeLists.txt
                  '';
                  # better find openssl
                  nativeBuildInputs = prevAttrs.nativeBuildInputs ++ [ pkgs.pkg-config ];
                  # enables secure memory mode
                  cmakeFlags = [
                    "-DINTERNAL_TIMER=OFF"
                    "-DEXTERNAL_CRYPTO=OPENSSL"
                    "-DBUILD_SHARED_LIBS=ON"
                  ];
                }
              );
        }).overrideAttrs(prev: {
          src = fetchFromGitHub {
            owner = "smuellerDD";
            repo = "esdm";
            rev = "master";
            sha256 = "sha256-lREbeWlu1J6IPe6lgGxqeYRNx615BGDU16o+TLoPOYE=";
          };
          # mesonBuildType = "debug";
          # dontStrip = true;
          # mesonFlags = prev.mesonFlags ++ [
          #   "-Dstrip=false"
          #   "-Ddebug=true"
          # ];
        }))
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
