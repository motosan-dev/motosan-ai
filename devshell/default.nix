{ pkgs
, lib
, stdenv
, ...
}:

# Most tools (Rust, Python, formatters, libiconv) come from home-config.
# This devshell only adds project-specific dev scripts and the CC override.

let
  scripts = pkgs.callPackage ./scripts.nix { };
in
pkgs.mkShell {
  name = "motosan-dev";

  nativeBuildInputs = [
    scripts.fmt
    scripts.lint
    scripts.check-rust
    scripts.check-python
    scripts.check-all
    scripts.test-live
  ];

  shellHook = ''
    export NIX_PATH="nixpkgs=${pkgs.path}"
  '' + lib.optionalString stdenv.isDarwin ''
    export CC=/usr/bin/cc
    export CXX=/usr/bin/c++
  '';
}
