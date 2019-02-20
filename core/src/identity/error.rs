// Copyright 2019 Parity Technologies (UK) Ltd.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the "Software"),
// to deal in the Software without restriction, including without limitation
// the rights to use, copy, modify, merge, publish, distribute, sublicense,
// and/or sell copies of the Software, and to permit persons to whom the
// Software is furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
// DEALINGS IN THE SOFTWARE.

use std::error::Error;
use std::fmt;

#[derive(Debug)]
pub struct EncodingError {
    msg: String,
    source: Option<Box<dyn Error + Send + Sync>>
}

/// An error during encoding of key material.
impl EncodingError {
    pub fn new(msg: &str, source: impl Error + Send + Sync + 'static) -> EncodingError {
        EncodingError { msg: msg.to_string(), source: Some(Box::new(source)) }
    }
}

impl fmt::Display for EncodingError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Key encoding error: {}", self.msg)
    }
}

impl From<String> for EncodingError {
    fn from(s: String) -> EncodingError {
        EncodingError { msg: s, source: None }
    }
}

impl Error for EncodingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_ref().map(|s| &**s as &dyn Error)
    }
}

/// An error during decoding of key material.
#[derive(Debug)]
pub struct DecodingError {
    msg: String,
    source: Option<Box<dyn Error + Send + Sync>>
}

impl DecodingError {
    pub fn new(msg: &str, source: impl Error + Send + Sync + 'static) -> DecodingError {
        DecodingError { msg: msg.to_string(), source: Some(Box::new(source)) }
    }
}

impl From<String> for DecodingError {
    fn from(s: String) -> DecodingError {
        DecodingError { msg: s, source: None }
    }
}

impl fmt::Display for DecodingError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Key decoding error: {}", self.msg)
    }
}

impl Error for DecodingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_ref().map(|s| &**s as &dyn Error)
    }
}

/// An error during signing of a message.
#[derive(Debug)]
pub struct SigningError {
    msg: String,
    source: Option<Box<dyn Error + Send + Sync>>
}

/// An error during encoding of key material.
impl SigningError {
    pub fn new(msg: &str, source: impl Error + Send + Sync + 'static) -> SigningError {
        SigningError { msg: msg.to_string(), source: Some(Box::new(source)) }
    }
}

impl fmt::Display for SigningError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Key signing error: {}", self.msg)
    }
}

impl From<String> for SigningError {
    fn from(s: String) -> SigningError {
        SigningError { msg: s, source: None }
    }
}

impl Error for SigningError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_ref().map(|s| &**s as &dyn Error)
    }
}

