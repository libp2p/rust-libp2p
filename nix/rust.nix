{ nixpkgs
, channel ? "nightly"
, date ? "2021-05-30"
}:
let
  inherit channel date;
  targets = [ "wasm32-unknown-unknown" "wasm32-wasi" ];
  sha256 = "N+G7d3+glt0O5n1yFRJdwFGg2xHRLl31YbxNRzwXP2w=";
  rust = (nixpkgs.rustChannelOf {
    inherit channel date sha256;
  }).rust.override {
    inherit targets;
    extensions = [ "rust-src" "rust-analysis" ];
  };
in
rust
