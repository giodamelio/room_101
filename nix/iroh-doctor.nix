{
  lib,
  rustPlatform,
  fetchFromGitHub,
}:
rustPlatform.buildRustPackage rec {
  pname = "iroh-doctor";
  version = "0.91.0";

  src = fetchFromGitHub {
    owner = "n0-computer";
    repo = "iroh-doctor";
    rev = "v${version}";
    hash = "sha256-5ncCYBKMbxSsPUTkmBaK23MAPFQi5Tj+CwfujJPuBbQ=";
  };

  cargoHash = "sha256-M0mGA03DaoyTn7vjevFN640tctnvw/994viaiOsoArk=";

  meta = {
    description = "Your tool for testing iroh connectivity";
    homepage = "https://github.com/n0-computer/iroh-doctor";
    license = with lib.licenses; [asl20 mit];
    maintainers = with lib.maintainers; [];
    mainProgram = "iroh-doctor";
  };
}
