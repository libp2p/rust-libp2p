Uses nightly Rust

`rustup default nightly`

## Run

Ensure you have trunk installed:

`cargo install trunk`

then run:

`trunk serve --open`

## Development

To update the css when the Rust files changes, run

`npx tailwindcss -i ./input.css -o ./style/output.css --watch`
