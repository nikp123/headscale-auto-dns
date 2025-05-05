{
  description = "Automatic Headscale DNS generator based on Traefik server state";

  outputs = { self, nixpkgs }:
    let
      systems =
        [ "i686-linux" "x86_64-linux" "armv6l-linux" "armv7l-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];

      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f system);

      nixpkgsFor = forAllSystems (system:
        import nixpkgs {
          inherit system;
          overlays = [ self.overlay ];
        });
    in rec {
      overlay = final: prev: {
        headscale-auto-dns = final.rustPlatform.buildRustPackage {
          pname = "headscale-auto-dns";
          version = "unstable";
          description = "Automatic Headscale DNS generator based on Traefik server state";

          nativeBuildInputs = with final; [ gcc pkg-config ];

          buildInputs = with final; [ openssl ]; 

          src = self;

          cargoLock = { lockFile = self + "/Cargo.lock"; };

          CARGO_FEATURE_USE_SYSTEM_LIBS = "1";
        };
      };

      packages =
        forAllSystems (system: { inherit (nixpkgsFor.${system}) headscale-auto-dns; });

      defaultPackage = forAllSystems (system: self.packages.${system}.headscale-auto-dns);

      apps = forAllSystems (system: {
        headscale-auto-dns = {
          type = "app";
          program = "${self.packages.${system}.headscale-auto-dns}/bin/headscale-auto-dns";
        };
      });

      defaultApp = forAllSystems (system: self.apps.${system}.headscale-auto-dns);

      devShell = forAllSystems (system:
        nixpkgs.legacyPackages.${system}.mkShell {
          inputsFrom = builtins.attrValues (packages.${system});
        });
    };
}

