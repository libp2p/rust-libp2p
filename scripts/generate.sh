
# generate private key for ca
openssl genpkey -algorithm Ed25519 --out ca.key

# generate private key for alice
openssl genpkey -algorithm Ed25519 --out alice.key

# generate private key for bob
openssl genpkey -algorithm Ed25519 --out bob.key

# generate root certificate for parity
openssl req -nodes \
        -x509 \
        -key ca.key \
        -out ca.cert \
        -sha256 \
        -batch \
        -days 3650 \
        -subj "/CN=parity EdDSA CA"

# generate ca.cert with asn1 format
openssl asn1parse -in ca.cert -out ca.der

# generate request for alice certificate
openssl req -nodes \
        -new \
        -key alice.key \
        -out alice.req \
        -sha256 \
        -batch \
        -subj "/CN=parity client alice"

# generate certificate for alice
openssl x509 -req \
        -in alice.req \
        -out alice.cert \
        -CA ca.cert \
        -CAkey ca.key \
        -sha256 \
        -days 2000 \
        -set_serial 123 \
        -extensions v3_end \
        -extfile openssl.cnf

# generate ca.cert with asn1 format
openssl asn1parse -in alice.cert -out alice.der

# generate request for bob certificate
openssl req -nodes \
        -new \
        -key bob.key \
        -out bob.req \
        -sha256 \
        -batch \
        -subj "/CN=parity client bob"

# generate certificate for bob
openssl x509 -req \
        -in bob.req \
        -out bob.cert \
        -CA ca.cert \
        -CAkey ca.key \
        -sha256 \
        -days 2000 \
        -set_serial 456 \
        -extensions v3_end \
        -extfile openssl.cnf

# generate ca.cert with asn1 format
openssl asn1parse -in bob.cert -out bob.der
