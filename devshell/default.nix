{ rustToolchain
, pkgs
, lib
, stdenv
, libiconv
, ...
}:

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

    # formatters
    nixpkgs-fmt
    taplo
    treefmt
  ];

  shellHook = ''
    export NIX_PATH="nixpkgs=${pkgs.path}"
  '';
}
