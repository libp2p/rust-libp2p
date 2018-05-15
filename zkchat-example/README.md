# zkChat example

Can be compiled both for both desktop (Linux, Windows, MacOS) or Emscripten (with the
`asmjs-unknown-emscripten` target).

When running the binary (on desktop), you can pass `bootstrap` in order to run the node as a
bootstrap node. The boostrap node has a hardcoded port and peer ID. When *not* running in boostrap
node, we will automatically connect to the bootstrap node by assuming it is on the same machine.
