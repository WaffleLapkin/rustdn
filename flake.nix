# This is a simple nix flake which provides a dev shell for rustdn development on on NixOS.
# You can either use `nix develop` to activate it manually or [`direnv`] to activate it automatically.
#
# [`direnv`]: https://github.com/nix-community/nix-direnv

{
  description = "dev shell for `rustdn`";

  inputs = {
    nixpkgs.url      = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url  = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
      in
      {
        devShells.default = with pkgs; mkShell {
          buildInputs = [
            (rust-bin.fromRustupToolchainFile ./rust-toolchain.toml)
          ];
        };
      }
    );
}
