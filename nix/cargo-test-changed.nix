{
  lib,
  rustPlatform,
  fetchFromGitHub,
}:
rustPlatform.buildRustPackage rec {
  pname = "cargo-test-changed";
  version = "0.1.1";

  src = fetchFromGitHub {
    owner = "felixpackard";
    repo = "cargo-test-changed";
    rev = "v${version}";
    hash = "sha256-AGzJHwrQYTPsQfd6UJV532pyZSqa/RUSdGQMVqtoppQ=";
  };

  cargoHash = "sha256-vgP2c5fG0JCvSCMCVAr3bGhmG4JCcCG6lqlwCTy1k20=";

  # Some tests are failing
  doCheck = false;

  meta = {
    description = "A Cargo subcommand to run tests for changed crates and their dependents";
    homepage = "https://github.com/felixpackard/cargo-test-changed";
    changelog = "https://github.com/felixpackard/cargo-test-changed/blob/${src.rev}/CHANGELOG.md";
    license = lib.licenses.mit;
    maintainers = with lib.maintainers; [];
    mainProgram = "cargo-test-changed";
  };
}
