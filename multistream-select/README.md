# Multistream-select

Multistream-select is the "main" protocol of libp2p.
Whenever a connection opens between two peers, it starts talking in `multistream-select`.

The purpose of `multistream-select` is to choose which protocol we are going to use. As soon as
both sides agree on a given protocol, the socket immediately starts using it and multistream is no
longer relevant.

However note that `multistream-select` is also sometimes used on top of another protocol such as
secio or multiplex. For example, two hosts can use `multistream-select` to decide to use secio,
then use `multistream-select` again (wrapped inside `secio`) to decide to use `multiplex`, then use
`multistream-select` one more time (wrapped inside `secio` and `multiplex`) to decide to use
the final actual protocol.
