{
  lib,
  rustPlatform,
  fetchFromGitHub,
}:
rustPlatform.buildRustPackage rec {
  pname = "iroh-relay";
  version = "0.91.2";

  src = fetchFromGitHub {
    owner = "n0-computer";
    repo = "iroh";
    rev = "v${version}";
    hash = "sha256-O1hWQNBLUqkBPoaW1lxmTrai+lktIAFgPX2Qa3y5HPc=";
  };

  # Build just the Iroh Relay server binary (it builds the lib by default)
  buildAndTestSubdir = "iroh-relay";
  cargoBuildFlags = ["--features=server"];

  cargoHash = "sha256-CgEit1HR6w0Y5SmSpErJnag3vaNgyocymLaM4RjYIBo=";

  meta = {
    description = "Peer-2-peer that just works";
    homepage = "https://github.com/n0-computer/iroh/tree/main/iroh-relay";
    changelog = "https://github.com/n0-computer/iroh/blob/${src.rev}/CHANGELOG.md";
    license = with lib.licenses; [asl20 mit];
    maintainers = with lib.maintainers; [];
    mainProgram = "iroh-relay";
  };
}
