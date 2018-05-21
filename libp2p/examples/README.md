# How the keys were generated

The keys used in the examples were generated like this:

```sh
openssl genrsa -out private.pem 2048
openssl rsa -in private.pem -outform DER -pubout -out public.der
openssl pkcs8 -in private.pem -topk8 -nocrypt -out private.pk8
rm private.pem      # optional
```
