{
  description = "jj-release: Semantic releases and changelog generation for jj-vcs repositories";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };
  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in
      {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "jj-release";
          version = "0.8.0";
          src = pkgs.lib.cleanSource ./.;
          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = with pkgs; [
            pkg-config
            mold
          ];
          meta = with pkgs.lib; {
            description = "Semantic releases and changelog generation for jj-vcs repositories";
            license = with licenses; [ asl20 ];
            mainProgram = "jj-release";
          };
        };
        apps.default = flake-utils.lib.mkApp {
          drv = self.packages.${system}.default;
        };
        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            pkg-config
            mold
            cargo
            rustc
            rust-analyzer
            clippy
            rustfmt
          ];
          shellHook = ''
            export RUST_BACKTRACE=1
            export CARGO_TERM_COLOR=always
            echo "jj-release dev shell loaded"
          '';
        };
      }
    );
}

