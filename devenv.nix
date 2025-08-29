{
  pkgs,
  lib,
  config,
  inputs,
  ...
}: let
  treefmt =
    (inputs.treefmt-nix.lib.evalModule pkgs {
      projectRootFile = "devenv.nix";
      programs = {
        alejandra.enable = true; # Nix formatter
        rustfmt.enable = true; # Rust formatter
        shfmt.enable = true; # Shell script formatter
      };
    }).config.build;
in {
  languages.rust = {
    enable = true;
    mold.enable = true;
  };

  outputs = {
    room_101 = config.languages.rust.import ./. {};
  };

  packages = with pkgs;
    [
      nil
      claude-code
      sqlx-cli
      cargo-nextest

      # Treefmt stuff
      treefmt.wrapper
    ]
    ++ (lib.attrValues treefmt.programs);

  tasks = {
    "db:reset".exec = "sqlx database reset -y";
    "db:start-fresh" = {
      after = ["db:reset"];
      exec = "sqlx database setup";
    };
  };

  enterShell = ''
    echo
    echo "Available tasks:"
    devenv tasks list
    echo
  '';

  enterTest = ''
    cargo nextest run
  '';

  git-hooks.hooks = {
    # Tree formatting (formats all files)
    treefmt = {
      enable = true;
      description = "Format code with treefmt";
      package = treefmt.wrapper;
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

  dotenv.enable = true;

  claude.code.enable = true;
}
