{ pkgs
, craneLib
, buildInputs
, nativeBuildInputs
,
}:

let
  cFilter = path: _type: builtins.match ".*c$" path != null;
  cOrCargo = path: type:
    (cFilter path type) || (craneLib.filterCargoSources path type);
in
craneLib.buildPackage {
  src = pkgs.lib.cleanSourceWith {
    src = craneLib.path ./.;
    filter = cOrCargo;
  };
  doCheck = true;
  inherit buildInputs;
  inherit nativeBuildInputs;
}
