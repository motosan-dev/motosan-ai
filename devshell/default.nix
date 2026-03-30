{ rustToolchain
, pkgs
, lib
, stdenv
, libiconv
, ...
}:

let
  scripts = pkgs.callPackage ./scripts.nix { };
in
pkgs.mkShell {
  name = "motosan-dev";

  buildInputs = lib.optionals stdenv.isDarwin [
    libiconv
  ];

  nativeBuildInputs = with pkgs; [
    # Rust
    rustToolchain
    cargo-nextest

    # Python
    python312
    uv
    ruff

    # Formatters (used by treefmt)
    treefmt
    taplo
    nixpkgs-fmt

    # Dev scripts
    scripts.fmt
    scripts.lint
    scripts.check-rust
    scripts.check-python
    scripts.check-all
    scripts.test-live
  ];

  shellHook = ''
    export NIX_PATH="nixpkgs=${pkgs.path}"
  '';
}
