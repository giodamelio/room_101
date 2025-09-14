{
  lib,
  fetchFromGitHub,
  mkBunDerivation,
  ...
}:
mkBunDerivation {
  pname = "mcp-docsrs";
  version = "unstable-2025-06-20";

  src = fetchFromGitHub {
    owner = "vexxvakan";
    repo = "mcp-docsrs";
    rev = "b188a5e034de19e00a1321f1dd1979a32bb607f8";
    hash = "sha256-0tfN+uoKOCZDPvG0bkulKLr9EwJPU1USBQw0eXvHl+k=";
  };

  bunNix = ./mcp-docsrs-bun.nix;
  index = "./src/cli.ts";

  meta = {
    description = "";
    homepage = "https://github.com/vexxvakan/mcp-docsrs";
    license = lib.licenses.asl20;
    maintainers = with lib.maintainers; [];
    mainProgram = "mcp-docsrs";
    platforms = lib.platforms.all;
  };
}
