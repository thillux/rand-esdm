{ pkgs
, buildInputs
, nativeBuildInputs
, rustPlatform
, gitignoreSource
}:

rustPlatform.buildRustPackage rec {
  pname = "rand-esdm";
  version = "0.1.5";

  src = gitignoreSource ./.;

  doCheck = true;

  inherit buildInputs;
  inherit nativeBuildInputs;

  cargoLock = {
    lockFile = ./Cargo.lock;
  };
}
