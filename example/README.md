# Examples

Running one of the examples:

```sh
cargo run --example <name-of-the-example>
```

The follow examples exist:

- `echo-dialer` will attempt to connect to `/ip4/127.0.0.1/tcp/10333`, negotiate the `/echo/1.0.0`
  protocol, then send the `"hello world"` message. Compatible with the `echo-server` example.
- `echo-server` will listen on `/ip4/0.0.0.0/tcp/10333`, negotiate the `/echo/1.0.0` protocol with
  each incoming connection, then send back any entering message. Compatible with the `echo-dialer`
  example.
- `ping-client` will try to connect to `/ip4/127.0.0.1/tcp/4001`, which is the default address of
  your local IPFS node if you're running one. It will then open a substream and ping the node.

## How the keys were generated

The keys used in the examples were generated like this:

```sh
openssl genrsa -out private.pem 2048
openssl rsa -in private.pem -outform DER -pubout -out public.der
openssl pkcs8 -in private.pem -topk8 -nocrypt -out private.pk8
rm private.pem      # optional
```
