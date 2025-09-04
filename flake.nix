{
  description = "Room 101 - A P2P networking application";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    treefmt-nix = {
      url = "github:numtide/treefmt-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    git-hooks = {
      url = "github:cachix/git-hooks.nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crate2nix = {
      url = "github:nix-community/crate2nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    gitignore = {
      url = "github:hercules-ci/gitignore.nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = inputs @ {flake-parts, ...}:
    flake-parts.lib.mkFlake {inherit inputs;} {
      imports = [
        inputs.treefmt-nix.flakeModule
        inputs.git-hooks.flakeModule
      ];

      systems = ["x86_64-linux" "aarch64-linux" "aarch64-darwin" "x86_64-darwin"];

      perSystem = {
        config,
        self',
        inputs',
        pkgs,
        system,
        ...
      }: let
        # TODO: We need to edit the Cargo.nix to support this each time we regen it
        sourceFilter = inputs.gitignore.lib.gitignoreFilterWith {
          basePath = ./.;
          extraRules = ''
            .claude
          '';
        };
        room_101Package = pkgs.callPackage ./Cargo.nix {inherit pkgs sourceFilter;};
      in {
        # Main package
        packages = {
          default = room_101Package.rootCrate.build;
          room_101 = room_101Package.rootCrate.build;
        };

        # Development shell
        devShells.default = pkgs.mkShell {
          inputsFrom = [
            config.pre-commit.devShell
            config.treefmt.build.devShell
          ];

          packages = with pkgs; [
            # Rust toolchain
            rustc
            cargo
            clippy
            rustfmt

            # Development tools
            nil
            sqlx-cli
            cargo-nextest
            sqlite
            litecli
            crate2nix
            nix-output-monitor

            # System dependencies
            pkg-config
          ];

          shellHook = ''
            echo
            echo "🦀 Room 101 Development Environment"
            echo "Available commands:"
            echo "  cargo check         - Check code for errors"
            echo "  cargo build         - Build the project"
            echo "  cargo nextest run   - Run tests"
            echo "  treefmt             - Format all code files"
            echo "  sqlx database reset - Reset database"
            echo "  sqlx migrate run    - Run database migrations"
            echo "  cargo sqlx prepare  - Generate SQLx metadata"
            echo
          '';

          # Environment variables
          RUST_LOG = "room_101=debug";
        };

        # Treefmt configuration
        treefmt = {
          projectRootFile = "flake.nix";
          programs = {
            alejandra.enable = true; # Nix formatter
            rustfmt.enable = true; # Rust formatter
            shfmt.enable = true; # Shell script formatter
          };
        };

        # Git hooks configuration
        pre-commit = {
          check.enable = true;
          settings = {
            enable = true;
            hooks = {
              # Formatting
              treefmt = {
                enable = true;
                package = config.treefmt.build.wrapper;
              };

              # Rust linting
              clippy = {
                enable = true;
                settings = {
                  denyWarnings = true;
                };
              };

              # Shell script linting
              shellcheck.enable = true;

              # General checks
              check-merge-conflicts.enable = true;
              check-added-large-files = {
                enable = true;
                excludes = ["Cargo.nix"];
              };
              check-toml.enable = true;
              check-yaml.enable = true;
              end-of-file-fixer.enable = true;
              trim-trailing-whitespace.enable = true;
            };
          };
        };

        # Checks
        checks = {};
      };
    };
}
