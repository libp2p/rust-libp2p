#ED25519 (works):
openssl genpkey -algorithm ed25519 -out privateKey.key
openssl req -new -subj="/" -key privateKey.key -out req.pem
openssl x509 -req -in req.pem -signkey privateKey.key -out certificate.crt -extensions p2p_ext -extfile ./openssl.cfg
openssl x509 -outform der -in certificate.crt -out ed25519.der

#ED448 (works):
openssl genpkey -algorithm ed448 -out privateKey.key
openssl req -new -subj="/" -key privateKey.key -out req.pem
openssl x509 -req -in req.pem -signkey privateKey.key -out certificate.crt -extensions p2p_ext -extfile ./openssl.cfg
openssl x509 -outform der -in certificate.crt -out ed448.der

#RSA_PKCS1_SHA256 (works):
openssl genpkey -algorithm rsa -out privateKey.key
openssl req -new -subj="/" -key privateKey.key -out req.pem
openssl x509 -req -in req.pem -signkey privateKey.key -sha256 -out certificate.crt -extensions p2p_ext -extfile ./openssl.cfg
openssl x509 -outform der -in certificate.crt -out rsa_pkcs1_sha256.der

#RSA_PKCS1_SHA384 (works):
# reuse privateKey.key and req.pem
openssl x509 -req -in req.pem -signkey privateKey.key -sha384 -out certificate.crt -extensions p2p_ext -extfile ./openssl.cfg
openssl x509 -outform der -in certificate.crt -out rsa_pkcs1_sha384.der

#RSA_PKCS1_SHA512 (works):
# reuse privateKey.key and req.pem
openssl x509 -req -in req.pem -signkey privateKey.key -sha512 -out certificate.crt -extensions p2p_ext -extfile ./openssl.cfg
openssl x509 -outform der -in certificate.crt -out rsa_pkcs1_sha512.der

#RSA-PSS TODO
# openssl genpkey -algorithm rsa-pss -pkeyopt rsa_keygen_bits:2048 -pkeyopt rsa_keygen_pubexp:3 -out privateKey.key
# # -sigopt rsa_pss_saltlen:20
# # -sigopt rsa_padding_mode:pss
# # -sigopt rsa_mgf1_md:sha256
# openssl req -x509 -nodes -days 365 -subj="/" -key privateKey.key -sha256 -sigopt rsa_pss_saltlen:20 -sigopt rsa_padding_mode:pss -sigopt rsa_mgf1_md:sha256 -out certificate.crt

#ECDSA_NISTP256_SHA256 (works):
openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-256 -out privateKey.key
openssl req -new -subj="/" -key privateKey.key -out req.pem
openssl x509 -req -in req.pem -signkey privateKey.key -sha256 -out certificate.crt -extensions p2p_ext -extfile ./openssl.cfg
openssl x509 -outform der -in certificate.crt -out nistp256_sha256.der

#ECDSA_NISTP384_SHA384 (works):
openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-384 -out privateKey.key
openssl req -new -subj="/" -key privateKey.key -out req.pem
openssl x509 -req -in req.pem -signkey privateKey.key -sha384 -out certificate.crt -extensions p2p_ext -extfile ./openssl.cfg
openssl x509 -outform der -in certificate.crt -out nistp384_sha384.der

#ECDSA_NISTP521_SHA512 (works):
openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-521 -out privateKey.key
openssl req -new -subj="/" -key privateKey.key -out req.pem
openssl x509 -req -in req.pem -signkey privateKey.key -sha512 -out certificate.crt -extensions p2p_ext -extfile ./openssl.cfg
openssl x509 -outform der -in certificate.crt -out nistp521_sha512.der

#ECDSA_NISTP384_SHA256 (must fail):
openssl genpkey -algorithm EC -pkeyopt ec_paramgen_curve:P-384 -out privateKey.key
openssl req -new -subj="/" -key privateKey.key -out req.pem
openssl x509 -req -in req.pem -signkey privateKey.key -sha256 -out certificate.crt -extensions p2p_ext -extfile ./openssl.cfg
openssl x509 -outform der -in certificate.crt -out nistp384_sha256.der


# Remove tmp files

rm req.pem certificate.crt privateKey.key
