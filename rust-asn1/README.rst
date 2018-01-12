rust-asn1
=========

.. image:: https://travis-ci.org/alex/rust-asn1.svg?branch=master
    :target: https://travis-ci.org/alex/rust-asn1

This is a Rust library for serializing ASN.1 structures (DER only).

Installation
------------

Add ``asn1`` to the ``[dependencies]`` section of your ``Cargo.toml``:

.. code-block:: toml

    [dependencies]
    asn1 = "*"


Usage
-----

To write a structure like::

    Signature ::= SEQUENCE {
        r INTEGER,
        s INTEGER
    }

you would write:

.. code-block:: rust

    extern crate asn1;

    let data = asn1::to_vec(|s| {
        s.write_sequence(|new_s| {
            new_s.write_int(r);
            new_s.write_int(s);
        });
    });

and to read it:

.. code-block:: rust

    extern crate asn1;

    let result = asn1::from_vec(data, |d| {
        return d.read_sequence(|d| {
            r = try!(d.read_int());
            s = try!(d.read_int());
            return Ok((r, s))
        });
    });

    match result {
        Ok((r, s)) => println("r={}, s={}", r, s),
        Err(_) => println!("Error!"),
    }
