{
  description = "motosan-ai — multi-provider LLM SDK (Rust + Python)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, fenix, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ fenix.overlays.default ];
        };

        rustToolchain = with fenix.packages.${system}; combine [
          stable.rustc
          stable.cargo
          stable.clippy
          stable.rust-src
          stable.rust-std
          default.rustfmt
        ];
      in
      {
        devShells.default = pkgs.callPackage ./devshell {
          inherit rustToolchain;
        };
      });
}
