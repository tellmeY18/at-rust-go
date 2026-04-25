{
  description = "Generic Rust development environment (macOS / Linux)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
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
            echo ""
          '';
        };
      }
    );
}
