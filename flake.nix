{
  description = "LilVault - A secure, encrypted secrets management system for homelabs";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    flake-parts.url = "github:hercules-ci/flake-parts";

    treefmt-nix = {
      url = "github:numtide/treefmt-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    git-hooks-nix = {
      url = "github:cachix/git-hooks.nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    # Fenix for Rust toolchain
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    # MCP servers for development tools
    mcp-servers = {
      url = "github:natsukium/mcp-servers-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    services-flake.url = "github:juspay/services-flake";
  };

  outputs = inputs @ {flake-parts, ...}:
    flake-parts.lib.mkFlake {inherit inputs;} {
      systems = ["x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin"];

      imports = [
        inputs.treefmt-nix.flakeModule
        inputs.git-hooks-nix.flakeModule
      ];

      perSystem = {
        config,
        self',
        inputs',
        pkgs,
        system,
        ...
      }: let
        inherit (pkgs) lib;

        # Prepare SQLx database for compile-time verification
        sqlx-db =
          pkgs.runCommand "sqlx-db-prepare" {
            nativeBuildInputs = with pkgs; [sqlx-cli sqlite];
          } ''
            mkdir -p $out
            export DATABASE_URL=sqlite:$out/db.sqlite3

            # Create database and run migrations
            sqlx database create
            sqlx migrate run --source ${./migrations}
          '';

        # Fenix Rust toolchain
        rustToolchain = with inputs.fenix.packages.${system};
          combine [
            stable.toolchain
            targets.wasm32-unknown-unknown.stable.toolchain
          ];

        # Read Cargo.toml for package metadata
        # cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);

        # MCP Language Server package
        mcp-language-server = pkgs.callPackage ./mcp-language-server.nix {};

        # MCP configuration
        mcpConfig = inputs.mcp-servers.lib.mkConfig pkgs {
          format = "json";
          fileName = ".mcp.json";

          programs = {
            memory = {
              enable = true;
              env = {
                "MEMORY_FILE_PATH" = "\${MEMORY_FILE_PATH}";
              };
            };
            sequential-thinking.enable = true;
          };

          settings.servers = {
            language-server = {
              command = "${mcp-language-server}/bin/mcp-language-server";
              args = ["--workspace" "." "--lsp" "rust-analyzer"];
            };
          };
        };
        # Manual Rust package using Fenix toolchain
        # lilvault =
        #   (pkgs.makeRustPlatform {
        #     cargo = rustToolchain;
        #     rustc = rustToolchain;
        #   }).buildRustPackage {
        #     pname = cargoToml.package.name;
        #     version = cargoToml.package.version;
        #
        #     src = pkgs.lib.cleanSource ./.;
        #
        #     cargoLock = {
        #       lockFile = ./Cargo.lock;
        #     };
        #
        #     nativeBuildInputs = with pkgs; [
        #       pkg-config
        #       sqlx-cli
        #       installShellFiles
        #     ];
        #
        #     buildInputs = with pkgs; [
        #       sqlite
        #       openssl
        #     ];
        #
        #     # Set up database for SQLx compile-time verification
        #     preBuild = ''
        #       export DATABASE_URL=sqlite:${sqlx-db}/db.sqlite3
        #     '';
        #
        #     # Generate and install shell completions
        #     postInstall = ''
        #       # Generate shell completions for all supported shells
        #       $out/bin/lilvault completion bash > lilvault.bash
        #       $out/bin/lilvault completion zsh > lilvault.zsh
        #       $out/bin/lilvault completion fish > lilvault.fish
        #       $out/bin/lilvault completion power-shell > lilvault.ps1
        #       $out/bin/lilvault completion elvish > lilvault.elv
        #
        #       # Install completions to standard locations (bash, zsh, fish are supported)
        #       installShellCompletion \
        #         --bash lilvault.bash \
        #         --zsh lilvault.zsh \
        #         --fish lilvault.fish
        #
        #       # Manually install PowerShell and Elvish completions to share directory
        #       # These shells don't have native installShellCompletion support yet
        #       mkdir -p $out/share/lilvault/completions
        #       cp lilvault.ps1 $out/share/lilvault/completions/
        #       cp lilvault.elv $out/share/lilvault/completions/
        #     '';
        #
        #     # Skip tests in Nix build (tests require interactive terminal features)
        #     doCheck = false;
        #
        #     meta = with pkgs.lib; {
        #       description = cargoToml.package.description;
        #       homepage = cargoToml.package.repository;
        #       license = with licenses; [mit asl20];
        #       maintainers = [];
        #       mainProgram = cargoToml.package.name;
        #     };
        #   };
      in {
        imports = [
          "${inputs.nixpkgs}/nixos/modules/misc/nixpkgs.nix"
        ];

        nixpkgs = {
          hostPlatform = system;
          config.allowUnfree = true;
        };

        # Flake checks (equivalent to .claude/ hook scripts)
        checks = {
          # Rust compilation check
          rust-check =
            pkgs.runCommand "rust-check" {
              nativeBuildInputs = with pkgs; [cargo rustc];
              src = pkgs.lib.cleanSource ./.;
            } ''
              cd $src
              cargo check
              touch $out
            '';

          # Rust linting with clippy
          rust-clippy =
            pkgs.runCommand "rust-clippy" {
              nativeBuildInputs = with pkgs; [cargo rustc clippy];
              src = pkgs.lib.cleanSource ./.;
            } ''
              cd $src
              cargo clippy -- -D warnings
              touch $out
            '';

          # Rust tests (when not running in interactive mode)
          rust-test =
            pkgs.runCommand "rust-test" {
              nativeBuildInputs = with pkgs; [cargo rustc];
              src = pkgs.lib.cleanSource ./.;
            } ''
              cd $src
              cargo test
              touch $out
            '';

          # Code formatting check
          treefmt-check = config.treefmt.build.check (pkgs.lib.cleanSource ./.);

          # Pre-commit hooks check
          pre-commit-check = config.pre-commit.build.devShell;
        };

        packages = {
          inherit mcp-language-server;
          inherit rustToolchain;
          # inherit lilvault mcp-language-server;
          # default = lilvault;
        };

        # Treefmt configuration
        treefmt.config = {
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
            hooks = {
              # Tree formatting (formats all files)
              treefmt = {
                enable = true;
                description = "Format code with treefmt";
              };

              # Rust linting
              clippy = {
                enable = true;
                description = "Lint Rust code with clippy";
                settings = {
                  denyWarnings = true;
                };
              };

              # Shell script linting
              shellcheck = {
                enable = true;
                description = "Lint shell scripts with shellcheck";
              };

              # General code quality
              check-merge-conflicts.enable = true;
              check-added-large-files.enable = true;
              check-toml.enable = true;
              check-yaml.enable = true;
              end-of-file-fixer.enable = true;
              trim-trailing-whitespace.enable = true;
            };
          };
        };

        # Simple development shell using Fenix toolchain
        devShells.default = pkgs.mkShell {
          packages = with pkgs;
            [
              # Fenix Rust toolchain with all components
              rustToolchain

              # Treefmt tools
              config.treefmt.build.wrapper # Wrapped treefmt script

              # Nix Language Server
              nil

              # Claude Code
              claude-code

              # Rust hotreloading web server helper
              trunk

              # Mold fast linker
              mold
            ]
            ++
            # All the formatter programs
            (lib.attrValues config.treefmt.build.programs);

          shellHook = ''
            # Set up MCP environment
            export MEMORY_FILE_PATH=$(pwd)/.claude/memory.json
            ln -sf ${mcpConfig} .mcp.json

            # Configure Rust to use mold linker
            export RUSTFLAGS="-C link-arg=-fuse-ld=mold"
          '';
        };
      };
    };
}
