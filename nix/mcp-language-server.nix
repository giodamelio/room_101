{
  pkgs,
  buildGoModule,
  fetchFromGitHub,
  ...
}:
buildGoModule rec {
  pname = "mcp-language-server";
  version = "0.1.1";

  src = fetchFromGitHub {
    owner = "isaacphi";
    repo = "mcp-language-server";
    rev = "v${version}";
    hash = "sha256-T0wuPSShJqVW+CcQHQuZnh3JOwqUxAKv1OCHwZMr7KM=";
  };

  vendorHash = "sha256-3NEG9o5AF2ZEFWkA9Gub8vn6DNptN6DwVcn/oR8ujW0=";

  # Only build the main package
  subPackages = ["."];

  ldflags = [
    "-s"
    "-w"
    "-X main.version=${version}"
  ];

  meta = with pkgs.lib; {
    description = "MCP server that runs and exposes a language server to LLMs";
    homepage = "https://github.com/isaacphi/mcp-language-server";
    license = licenses.bsd3;
    maintainers = [];
    mainProgram = "mcp-language-server";
    platforms = platforms.unix;
  };

  # Skip tests for now as they require external language servers
  doCheck = false;
}
