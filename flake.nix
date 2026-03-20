{
  description = "Use superposition to configure applications using FUSE";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      nixpkgs,
      crane,
      flake-utils,
      rust-overlay,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };

        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        # Common arguments can be set here to avoid repeating them later
        # Note: changes here will rebuild all dependency crates
        src = pkgs.lib.cleanSourceWith {
          src = ./.;
          filter =
            path: type:
            (builtins.match ".*rust-toolchain\\.toml$" path != null) || (craneLib.filterCargoSources path type);
        };

        commonArgs = {
          inherit src;
          strictDeps = true;

          nativeBuildInputs = [
            pkgs.pkg-config
          ];

          buildInputs = [
          ]
          ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
            pkgs.fuse
          ]
          ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
            pkgs.libiconv
            pkgs.macfuse-stubs
          ];
        };

        superfuse = craneLib.buildPackage (
          commonArgs
          // {
            cargoArtifacts = craneLib.buildDepsOnly commonArgs;
          }
        );
      in
      {
        checks = {
          inherit superfuse;
        };

        packages.default = superfuse;

        apps.default = flake-utils.lib.mkApp {
          drv = superfuse;
        };

        devShells.default = pkgs.mkShell {
          nativeBuildInputs = [
            rustToolchain
            pkgs.pkg-config
          ];

          buildInputs = [
          ]
          ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
            pkgs.fuse
          ]
          ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
            pkgs.libiconv
            pkgs.macfuse-stubs
          ];

          packages = with pkgs; [
            cargo-watch
            jq
          ];
        };
      }
    );
}
