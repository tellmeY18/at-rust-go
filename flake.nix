{
  description = "Generic Rust development environment (macOS / Linux)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

    # Uncomment for E2E testing with a real PDS (requires Rust edition 2024 support).
    # Canonical source: https://tangled.org/tranquil.farm/tranquil-pds
    # tranquil-pds = {
    #   url = "git+https://tangled.org/tranquil.farm/tranquil-pds";
    #   inputs.nixpkgs.follows = "nixpkgs";
    # };
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      rust-overlay,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
        lib = pkgs.lib;

        # -- Rust toolchain --
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [
            "rust-src" # for rust-analyzer
            "rust-analyzer"
            "clippy"
            "rustfmt"
          ];
        };

        # -- Common Rust dev utilities --
        rustDevTools = with pkgs; [
          cargo-watch
          cargo-nextest
          cargo-expand
          cargo-llvm-cov
          sqlx-cli # remove if not using sqlx
          pkg-config
          cmake # needed by many C dependencies
        ];

        # -- Runtime libraries --
        # On Linux we often need openssl; on macOS the system provides it.
        runtimeLibs =
          with pkgs;
          [
            sqlite # example: remove if not needed
          ]
          ++ lib.optionals pkgs.stdenv.isDarwin [ libiconv ]
          ++ lib.optionals (!pkgs.stdenv.isDarwin) [ openssl ];

        isDarwin = pkgs.stdenv.isDarwin;

      in
      {
        devShells.default = pkgs.mkShell {
          # Build tools (runs on the build machine, not the target)
          nativeBuildInputs = rustDevTools ++ [ rustToolchain ];
          # Libraries needed at runtime/build for linking
          buildInputs = runtimeLibs;

          env =
            pkgs.lib.optionalAttrs isDarwin {
              # Point to the Xcode Command Line Tools SDK so that
              # C dependencies (ring, aws-lc-sys, libsqlite3-sys, etc.)
              # find system frameworks (Security, SystemConfiguration, …)
              SDKROOT = "/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk";
            }
            // pkgs.lib.optionalAttrs (!isDarwin) {
              # Use mold linker on Linux for faster builds (optional)
              RUSTFLAGS = "-C link-arg=-fuse-ld=mold";
            };

          shellHook = ''
            echo "🦀  Rust development environment"
            echo "   Rust:  $(rustc --version)"
            echo "   OS:    $(uname -s)"
            echo "   For E2E tests with a real PDS: nix develop .#e2e"
            echo ""
          '';
        };

        # E2E testing shell — includes PostgreSQL for tranquil-pds integration tests.
        # To also include tranquil-pds itself, uncomment the tranquil-pds input above
        # and add `tranquil-pds.packages.${system}.default` to buildInputs below.
        # Source: https://tangled.org/tranquil.farm/tranquil-pds
        # NOTE: tranquil-pds requires Rust edition 2024 and may not build on all systems.
        devShells.e2e = pkgs.mkShell {
          nativeBuildInputs = rustDevTools ++ [ rustToolchain ];
          buildInputs = runtimeLibs ++ [
            pkgs.postgresql
            # tranquil-pds.packages.${system}.default  # uncomment when input is enabled
          ];

          env =
            pkgs.lib.optionalAttrs isDarwin {
              SDKROOT = "/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk";
            }
            // pkgs.lib.optionalAttrs (!isDarwin) {
              RUSTFLAGS = "-C link-arg=-fuse-ld=mold";
            };

          shellHook = ''
            echo "🦀  Rust E2E testing environment (with PostgreSQL)"
            echo "   Rust:  $(rustc --version)"
            echo "   psql:  $(psql --version)"
            echo "   tranquil-pds available for integration tests"
            echo ""
          '';
        };
      }
    );
}
