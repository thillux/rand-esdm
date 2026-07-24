{ pkgs
, buildInputs
, nativeBuildInputs
, rustPlatform
, gitignoreSource
}:

rustPlatform.buildRustPackage rec {
  pname = "rand-esdm";
  version = "0.4.0";

  src = gitignoreSource ./.;

  doCheck = true;

  inherit buildInputs;
  inherit nativeBuildInputs;

  cargoLock = {
    lockFile = ./Cargo.lock;
  };
}
