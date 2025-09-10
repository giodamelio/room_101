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
    fenix = {
      url = "github:nix-community/fenix";
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

        # Our own Rust toolchain from Fenix
        rustNightly = inputs'.fenix.packages.complete;
        rustToolchain = rustNightly.toolchain;

        # Custom version of Nixpkgs with Rust and Cargo replaced with the Fenix toolchain
        pkgsWithFenix = import inputs.nixpkgs {
          inherit system;
          overlays = [
            (final: prev: {
              rust = rustToolchain;
              cargo = rustToolchain;
            })
          ];
        };

        room_101Package = pkgsWithFenix.callPackage ./Cargo.nix {
          inherit sourceFilter;
          pkgs = pkgsWithFenix;
        };
      in {
        # Main package
        packages = {
          default = room_101Package.rootCrate.build;
          room_101 = room_101Package.rootCrate.build;

          # Check for the discovery DNS records
          dns_check = pkgs.writeShellApplication {
            name = "dns_check";
            runtimeInputs = with pkgs; [dogdns];
            text = ''
              read -r -p "Enter Z32 Node ID: " Z32_ID
              dog "_iroh.''${Z32_ID}.dns.iroh.link" TXT
            '';
          };
        };

        # Development shell
        devShells.default = pkgs.mkShell {
          inputsFrom = [
            config.pre-commit.devShell
            config.treefmt.build.devShell
          ];

          packages = with pkgs; [
            # Rust toolchain
            rustToolchain

            # Development tools
            nil
            cargo-nextest
            litecli
            crate2nix
            nix-output-monitor
            dogdns

            # System dependencies
            pkg-config

            # Helper Scripts
            self'.packages.dns_check

            # Native deps needed when using Surrealist
            glib
            pango
            libsoup_3
            webkitgtk_4_1
          ];

          shellHook = ''
            echo
            echo "🦀 Room 101 Development Environment"
            echo "Available commands:"
            echo "  cargo check         - Check code for errors"
            echo "  cargo build         - Build the project"
            echo "  cargo nextest run   - Run tests"
            echo "  treefmt             - Format all code files"
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
            # Rust formatter
            rustfmt = {
              enable = true;
              package = rustToolchain;
            };
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
                package = rustNightly.clippy;
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
