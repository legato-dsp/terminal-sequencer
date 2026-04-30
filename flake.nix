{
  description = "A minimal Legato downstream example";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";
    legato.url = "github:legato-dsp/legato";
  };

  outputs = { self, nixpkgs, legato }: 
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];

      forAllSystems = f: nixpkgs.lib.genAttrs supportedSystems (system: f {
        pkgs = import nixpkgs { inherit system; };
        inherit system;
      });
    in
    {
      devShells = forAllSystems ({ pkgs, system }: {
        default = pkgs.mkShell {
          inputsFrom = [ 
            legato.devShells.${system}.default 
          ];

          buildInputs = with pkgs; [
            # Your project-specific additions
          ];

          shellHook = ''
            echo "Environment loaded for ${system}"
          '';
        };
      });

      packages = forAllSystems ({ pkgs, ... }: {
        # example-pkg = pkgs.callPackage ./pkg.nix {};
      });
    };
}
