var __create = Object.create;
var __defProp = Object.defineProperty;
var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __getProtoOf = Object.getPrototypeOf;
var __hasOwnProp = Object.prototype.hasOwnProperty;
var __commonJS = (cb, mod4) => function __require() {
  return mod4 || (0, cb[__getOwnPropNames(cb)[0]])((mod4 = { exports: {} }).exports, mod4), mod4.exports;
};
var __export = (target, all) => {
  for (var name3 in all)
    __defProp(target, name3, { get: all[name3], enumerable: true });
};
var __copyProps = (to, from5, except, desc) => {
  if (from5 && typeof from5 === "object" || typeof from5 === "function") {
    for (let key of __getOwnPropNames(from5))
      if (!__hasOwnProp.call(to, key) && key !== except)
        __defProp(to, key, { get: () => from5[key], enumerable: !(desc = __getOwnPropDesc(from5, key)) || desc.enumerable });
  }
  return to;
};
var __toESM = (mod4, isNodeMode, target) => (target = mod4 != null ? __create(__getProtoOf(mod4)) : {}, __copyProps(
  // If the importer is in node compatibility mode or this is not an ESM
  // file that has been converted to a CommonJS file using a Babel-
  // compatible transform (i.e. "__esModule" has not been set), then set
  // "default" to the CommonJS "module.exports" for node compatibility.
  isNodeMode || !mod4 || !mod4.__esModule ? __defProp(target, "default", { value: mod4, enumerable: true }) : target,
  mod4
));

// ../../node_modules/node-forge/lib/forge.js
var require_forge = __commonJS({
  "../../node_modules/node-forge/lib/forge.js"(exports2, module2) {
    module2.exports = {
      // default options
      options: {
        usePureJavaScript: false
      }
    };
  }
});

// ../../node_modules/node-forge/lib/baseN.js
var require_baseN = __commonJS({
  "../../node_modules/node-forge/lib/baseN.js"(exports2, module2) {
    var api = {};
    module2.exports = api;
    var _reverseAlphabets = {};
    api.encode = function(input, alphabet3, maxline) {
      if (typeof alphabet3 !== "string") {
        throw new TypeError('"alphabet" must be a string.');
      }
      if (maxline !== void 0 && typeof maxline !== "number") {
        throw new TypeError('"maxline" must be a number.');
      }
      var output = "";
      if (!(input instanceof Uint8Array)) {
        output = _encodeWithByteBuffer(input, alphabet3);
      } else {
        var i = 0;
        var base4 = alphabet3.length;
        var first = alphabet3.charAt(0);
        var digits = [0];
        for (i = 0; i < input.length; ++i) {
          for (var j = 0, carry = input[i]; j < digits.length; ++j) {
            carry += digits[j] << 8;
            digits[j] = carry % base4;
            carry = carry / base4 | 0;
          }
          while (carry > 0) {
            digits.push(carry % base4);
            carry = carry / base4 | 0;
          }
        }
        for (i = 0; input[i] === 0 && i < input.length - 1; ++i) {
          output += first;
        }
        for (i = digits.length - 1; i >= 0; --i) {
          output += alphabet3[digits[i]];
        }
      }
      if (maxline) {
        var regex = new RegExp(".{1," + maxline + "}", "g");
        output = output.match(regex).join("\r\n");
      }
      return output;
    };
    api.decode = function(input, alphabet3) {
      if (typeof input !== "string") {
        throw new TypeError('"input" must be a string.');
      }
      if (typeof alphabet3 !== "string") {
        throw new TypeError('"alphabet" must be a string.');
      }
      var table = _reverseAlphabets[alphabet3];
      if (!table) {
        table = _reverseAlphabets[alphabet3] = [];
        for (var i = 0; i < alphabet3.length; ++i) {
          table[alphabet3.charCodeAt(i)] = i;
        }
      }
      input = input.replace(/\s/g, "");
      var base4 = alphabet3.length;
      var first = alphabet3.charAt(0);
      var bytes = [0];
      for (var i = 0; i < input.length; i++) {
        var value = table[input.charCodeAt(i)];
        if (value === void 0) {
          return;
        }
        for (var j = 0, carry = value; j < bytes.length; ++j) {
          carry += bytes[j] * base4;
          bytes[j] = carry & 255;
          carry >>= 8;
        }
        while (carry > 0) {
          bytes.push(carry & 255);
          carry >>= 8;
        }
      }
      for (var k = 0; input[k] === first && k < input.length - 1; ++k) {
        bytes.push(0);
      }
      if (typeof Buffer !== "undefined") {
        return Buffer.from(bytes.reverse());
      }
      return new Uint8Array(bytes.reverse());
    };
    function _encodeWithByteBuffer(input, alphabet3) {
      var i = 0;
      var base4 = alphabet3.length;
      var first = alphabet3.charAt(0);
      var digits = [0];
      for (i = 0; i < input.length(); ++i) {
        for (var j = 0, carry = input.at(i); j < digits.length; ++j) {
          carry += digits[j] << 8;
          digits[j] = carry % base4;
          carry = carry / base4 | 0;
        }
        while (carry > 0) {
          digits.push(carry % base4);
          carry = carry / base4 | 0;
        }
      }
      var output = "";
      for (i = 0; input.at(i) === 0 && i < input.length() - 1; ++i) {
        output += first;
      }
      for (i = digits.length - 1; i >= 0; --i) {
        output += alphabet3[digits[i]];
      }
      return output;
    }
  }
});

// ../../node_modules/node-forge/lib/util.js
var require_util = __commonJS({
  "../../node_modules/node-forge/lib/util.js"(exports2, module2) {
    var forge6 = require_forge();
    var baseN = require_baseN();
    var util2 = module2.exports = forge6.util = forge6.util || {};
    (function() {
      if (typeof process !== "undefined" && process.nextTick && !process.browser) {
        util2.nextTick = process.nextTick;
        if (typeof setImmediate === "function") {
          util2.setImmediate = setImmediate;
        } else {
          util2.setImmediate = util2.nextTick;
        }
        return;
      }
      if (typeof setImmediate === "function") {
        util2.setImmediate = function() {
          return setImmediate.apply(void 0, arguments);
        };
        util2.nextTick = function(callback) {
          return setImmediate(callback);
        };
        return;
      }
      util2.setImmediate = function(callback) {
        setTimeout(callback, 0);
      };
      if (typeof window !== "undefined" && typeof window.postMessage === "function") {
        let handler2 = function(event) {
          if (event.source === window && event.data === msg) {
            event.stopPropagation();
            var copy = callbacks.slice();
            callbacks.length = 0;
            copy.forEach(function(callback) {
              callback();
            });
          }
        };
        var handler = handler2;
        var msg = "forge.setImmediate";
        var callbacks = [];
        util2.setImmediate = function(callback) {
          callbacks.push(callback);
          if (callbacks.length === 1) {
            window.postMessage(msg, "*");
          }
        };
        window.addEventListener("message", handler2, true);
      }
      if (typeof MutationObserver !== "undefined") {
        var now = Date.now();
        var attr = true;
        var div = document.createElement("div");
        var callbacks = [];
        new MutationObserver(function() {
          var copy = callbacks.slice();
          callbacks.length = 0;
          copy.forEach(function(callback) {
            callback();
          });
        }).observe(div, { attributes: true });
        var oldSetImmediate = util2.setImmediate;
        util2.setImmediate = function(callback) {
          if (Date.now() - now > 15) {
            now = Date.now();
            oldSetImmediate(callback);
          } else {
            callbacks.push(callback);
            if (callbacks.length === 1) {
              div.setAttribute("a", attr = !attr);
            }
          }
        };
      }
      util2.nextTick = util2.setImmediate;
    })();
    util2.isNodejs = typeof process !== "undefined" && process.versions && process.versions.node;
    util2.globalScope = function() {
      if (util2.isNodejs) {
        return global;
      }
      return typeof self === "undefined" ? window : self;
    }();
    util2.isArray = Array.isArray || function(x) {
      return Object.prototype.toString.call(x) === "[object Array]";
    };
    util2.isArrayBuffer = function(x) {
      return typeof ArrayBuffer !== "undefined" && x instanceof ArrayBuffer;
    };
    util2.isArrayBufferView = function(x) {
      return x && util2.isArrayBuffer(x.buffer) && x.byteLength !== void 0;
    };
    function _checkBitsParam(n) {
      if (!(n === 8 || n === 16 || n === 24 || n === 32)) {
        throw new Error("Only 8, 16, 24, or 32 bits supported: " + n);
      }
    }
    util2.ByteBuffer = ByteStringBuffer;
    function ByteStringBuffer(b) {
      this.data = "";
      this.read = 0;
      if (typeof b === "string") {
        this.data = b;
      } else if (util2.isArrayBuffer(b) || util2.isArrayBufferView(b)) {
        if (typeof Buffer !== "undefined" && b instanceof Buffer) {
          this.data = b.toString("binary");
        } else {
          var arr = new Uint8Array(b);
          try {
            this.data = String.fromCharCode.apply(null, arr);
          } catch (e) {
            for (var i = 0; i < arr.length; ++i) {
              this.putByte(arr[i]);
            }
          }
        }
      } else if (b instanceof ByteStringBuffer || typeof b === "object" && typeof b.data === "string" && typeof b.read === "number") {
        this.data = b.data;
        this.read = b.read;
      }
      this._constructedStringLength = 0;
    }
    util2.ByteStringBuffer = ByteStringBuffer;
    var _MAX_CONSTRUCTED_STRING_LENGTH = 4096;
    util2.ByteStringBuffer.prototype._optimizeConstructedString = function(x) {
      this._constructedStringLength += x;
      if (this._constructedStringLength > _MAX_CONSTRUCTED_STRING_LENGTH) {
        this.data.substr(0, 1);
        this._constructedStringLength = 0;
      }
    };
    util2.ByteStringBuffer.prototype.length = function() {
      return this.data.length - this.read;
    };
    util2.ByteStringBuffer.prototype.isEmpty = function() {
      return this.length() <= 0;
    };
    util2.ByteStringBuffer.prototype.putByte = function(b) {
      return this.putBytes(String.fromCharCode(b));
    };
    util2.ByteStringBuffer.prototype.fillWithByte = function(b, n) {
      b = String.fromCharCode(b);
      var d = this.data;
      while (n > 0) {
        if (n & 1) {
          d += b;
        }
        n >>>= 1;
        if (n > 0) {
          b += b;
        }
      }
      this.data = d;
      this._optimizeConstructedString(n);
      return this;
    };
    util2.ByteStringBuffer.prototype.putBytes = function(bytes) {
      this.data += bytes;
      this._optimizeConstructedString(bytes.length);
      return this;
    };
    util2.ByteStringBuffer.prototype.putString = function(str) {
      return this.putBytes(util2.encodeUtf8(str));
    };
    util2.ByteStringBuffer.prototype.putInt16 = function(i) {
      return this.putBytes(
        String.fromCharCode(i >> 8 & 255) + String.fromCharCode(i & 255)
      );
    };
    util2.ByteStringBuffer.prototype.putInt24 = function(i) {
      return this.putBytes(
        String.fromCharCode(i >> 16 & 255) + String.fromCharCode(i >> 8 & 255) + String.fromCharCode(i & 255)
      );
    };
    util2.ByteStringBuffer.prototype.putInt32 = function(i) {
      return this.putBytes(
        String.fromCharCode(i >> 24 & 255) + String.fromCharCode(i >> 16 & 255) + String.fromCharCode(i >> 8 & 255) + String.fromCharCode(i & 255)
      );
    };
    util2.ByteStringBuffer.prototype.putInt16Le = function(i) {
      return this.putBytes(
        String.fromCharCode(i & 255) + String.fromCharCode(i >> 8 & 255)
      );
    };
    util2.ByteStringBuffer.prototype.putInt24Le = function(i) {
      return this.putBytes(
        String.fromCharCode(i & 255) + String.fromCharCode(i >> 8 & 255) + String.fromCharCode(i >> 16 & 255)
      );
    };
    util2.ByteStringBuffer.prototype.putInt32Le = function(i) {
      return this.putBytes(
        String.fromCharCode(i & 255) + String.fromCharCode(i >> 8 & 255) + String.fromCharCode(i >> 16 & 255) + String.fromCharCode(i >> 24 & 255)
      );
    };
    util2.ByteStringBuffer.prototype.putInt = function(i, n) {
      _checkBitsParam(n);
      var bytes = "";
      do {
        n -= 8;
        bytes += String.fromCharCode(i >> n & 255);
      } while (n > 0);
      return this.putBytes(bytes);
    };
    util2.ByteStringBuffer.prototype.putSignedInt = function(i, n) {
      if (i < 0) {
        i += 2 << n - 1;
      }
      return this.putInt(i, n);
    };
    util2.ByteStringBuffer.prototype.putBuffer = function(buffer) {
      return this.putBytes(buffer.getBytes());
    };
    util2.ByteStringBuffer.prototype.getByte = function() {
      return this.data.charCodeAt(this.read++);
    };
    util2.ByteStringBuffer.prototype.getInt16 = function() {
      var rval = this.data.charCodeAt(this.read) << 8 ^ this.data.charCodeAt(this.read + 1);
      this.read += 2;
      return rval;
    };
    util2.ByteStringBuffer.prototype.getInt24 = function() {
      var rval = this.data.charCodeAt(this.read) << 16 ^ this.data.charCodeAt(this.read + 1) << 8 ^ this.data.charCodeAt(this.read + 2);
      this.read += 3;
      return rval;
    };
    util2.ByteStringBuffer.prototype.getInt32 = function() {
      var rval = this.data.charCodeAt(this.read) << 24 ^ this.data.charCodeAt(this.read + 1) << 16 ^ this.data.charCodeAt(this.read + 2) << 8 ^ this.data.charCodeAt(this.read + 3);
      this.read += 4;
      return rval;
    };
    util2.ByteStringBuffer.prototype.getInt16Le = function() {
      var rval = this.data.charCodeAt(this.read) ^ this.data.charCodeAt(this.read + 1) << 8;
      this.read += 2;
      return rval;
    };
    util2.ByteStringBuffer.prototype.getInt24Le = function() {
      var rval = this.data.charCodeAt(this.read) ^ this.data.charCodeAt(this.read + 1) << 8 ^ this.data.charCodeAt(this.read + 2) << 16;
      this.read += 3;
      return rval;
    };
    util2.ByteStringBuffer.prototype.getInt32Le = function() {
      var rval = this.data.charCodeAt(this.read) ^ this.data.charCodeAt(this.read + 1) << 8 ^ this.data.charCodeAt(this.read + 2) << 16 ^ this.data.charCodeAt(this.read + 3) << 24;
      this.read += 4;
      return rval;
    };
    util2.ByteStringBuffer.prototype.getInt = function(n) {
      _checkBitsParam(n);
      var rval = 0;
      do {
        rval = (rval << 8) + this.data.charCodeAt(this.read++);
        n -= 8;
      } while (n > 0);
      return rval;
    };
    util2.ByteStringBuffer.prototype.getSignedInt = function(n) {
      var x = this.getInt(n);
      var max = 2 << n - 2;
      if (x >= max) {
        x -= max << 1;
      }
      return x;
    };
    util2.ByteStringBuffer.prototype.getBytes = function(count) {
      var rval;
      if (count) {
        count = Math.min(this.length(), count);
        rval = this.data.slice(this.read, this.read + count);
        this.read += count;
      } else if (count === 0) {
        rval = "";
      } else {
        rval = this.read === 0 ? this.data : this.data.slice(this.read);
        this.clear();
      }
      return rval;
    };
    util2.ByteStringBuffer.prototype.bytes = function(count) {
      return typeof count === "undefined" ? this.data.slice(this.read) : this.data.slice(this.read, this.read + count);
    };
    util2.ByteStringBuffer.prototype.at = function(i) {
      return this.data.charCodeAt(this.read + i);
    };
    util2.ByteStringBuffer.prototype.setAt = function(i, b) {
      this.data = this.data.substr(0, this.read + i) + String.fromCharCode(b) + this.data.substr(this.read + i + 1);
      return this;
    };
    util2.ByteStringBuffer.prototype.last = function() {
      return this.data.charCodeAt(this.data.length - 1);
    };
    util2.ByteStringBuffer.prototype.copy = function() {
      var c = util2.createBuffer(this.data);
      c.read = this.read;
      return c;
    };
    util2.ByteStringBuffer.prototype.compact = function() {
      if (this.read > 0) {
        this.data = this.data.slice(this.read);
        this.read = 0;
      }
      return this;
    };
    util2.ByteStringBuffer.prototype.clear = function() {
      this.data = "";
      this.read = 0;
      return this;
    };
    util2.ByteStringBuffer.prototype.truncate = function(count) {
      var len = Math.max(0, this.length() - count);
      this.data = this.data.substr(this.read, len);
      this.read = 0;
      return this;
    };
    util2.ByteStringBuffer.prototype.toHex = function() {
      var rval = "";
      for (var i = this.read; i < this.data.length; ++i) {
        var b = this.data.charCodeAt(i);
        if (b < 16) {
          rval += "0";
        }
        rval += b.toString(16);
      }
      return rval;
    };
    util2.ByteStringBuffer.prototype.toString = function() {
      return util2.decodeUtf8(this.bytes());
    };
    function DataBuffer(b, options) {
      options = options || {};
      this.read = options.readOffset || 0;
      this.growSize = options.growSize || 1024;
      var isArrayBuffer = util2.isArrayBuffer(b);
      var isArrayBufferView = util2.isArrayBufferView(b);
      if (isArrayBuffer || isArrayBufferView) {
        if (isArrayBuffer) {
          this.data = new DataView(b);
        } else {
          this.data = new DataView(b.buffer, b.byteOffset, b.byteLength);
        }
        this.write = "writeOffset" in options ? options.writeOffset : this.data.byteLength;
        return;
      }
      this.data = new DataView(new ArrayBuffer(0));
      this.write = 0;
      if (b !== null && b !== void 0) {
        this.putBytes(b);
      }
      if ("writeOffset" in options) {
        this.write = options.writeOffset;
      }
    }
    util2.DataBuffer = DataBuffer;
    util2.DataBuffer.prototype.length = function() {
      return this.write - this.read;
    };
    util2.DataBuffer.prototype.isEmpty = function() {
      return this.length() <= 0;
    };
    util2.DataBuffer.prototype.accommodate = function(amount, growSize) {
      if (this.length() >= amount) {
        return this;
      }
      growSize = Math.max(growSize || this.growSize, amount);
      var src3 = new Uint8Array(
        this.data.buffer,
        this.data.byteOffset,
        this.data.byteLength
      );
      var dst = new Uint8Array(this.length() + growSize);
      dst.set(src3);
      this.data = new DataView(dst.buffer);
      return this;
    };
    util2.DataBuffer.prototype.putByte = function(b) {
      this.accommodate(1);
      this.data.setUint8(this.write++, b);
      return this;
    };
    util2.DataBuffer.prototype.fillWithByte = function(b, n) {
      this.accommodate(n);
      for (var i = 0; i < n; ++i) {
        this.data.setUint8(b);
      }
      return this;
    };
    util2.DataBuffer.prototype.putBytes = function(bytes, encoding) {
      if (util2.isArrayBufferView(bytes)) {
        var src3 = new Uint8Array(bytes.buffer, bytes.byteOffset, bytes.byteLength);
        var len = src3.byteLength - src3.byteOffset;
        this.accommodate(len);
        var dst = new Uint8Array(this.data.buffer, this.write);
        dst.set(src3);
        this.write += len;
        return this;
      }
      if (util2.isArrayBuffer(bytes)) {
        var src3 = new Uint8Array(bytes);
        this.accommodate(src3.byteLength);
        var dst = new Uint8Array(this.data.buffer);
        dst.set(src3, this.write);
        this.write += src3.byteLength;
        return this;
      }
      if (bytes instanceof util2.DataBuffer || typeof bytes === "object" && typeof bytes.read === "number" && typeof bytes.write === "number" && util2.isArrayBufferView(bytes.data)) {
        var src3 = new Uint8Array(bytes.data.byteLength, bytes.read, bytes.length());
        this.accommodate(src3.byteLength);
        var dst = new Uint8Array(bytes.data.byteLength, this.write);
        dst.set(src3);
        this.write += src3.byteLength;
        return this;
      }
      if (bytes instanceof util2.ByteStringBuffer) {
        bytes = bytes.data;
        encoding = "binary";
      }
      encoding = encoding || "binary";
      if (typeof bytes === "string") {
        var view;
        if (encoding === "hex") {
          this.accommodate(Math.ceil(bytes.length / 2));
          view = new Uint8Array(this.data.buffer, this.write);
          this.write += util2.binary.hex.decode(bytes, view, this.write);
          return this;
        }
        if (encoding === "base64") {
          this.accommodate(Math.ceil(bytes.length / 4) * 3);
          view = new Uint8Array(this.data.buffer, this.write);
          this.write += util2.binary.base64.decode(bytes, view, this.write);
          return this;
        }
        if (encoding === "utf8") {
          bytes = util2.encodeUtf8(bytes);
          encoding = "binary";
        }
        if (encoding === "binary" || encoding === "raw") {
          this.accommodate(bytes.length);
          view = new Uint8Array(this.data.buffer, this.write);
          this.write += util2.binary.raw.decode(view);
          return this;
        }
        if (encoding === "utf16") {
          this.accommodate(bytes.length * 2);
          view = new Uint16Array(this.data.buffer, this.write);
          this.write += util2.text.utf16.encode(view);
          return this;
        }
        throw new Error("Invalid encoding: " + encoding);
      }
      throw Error("Invalid parameter: " + bytes);
    };
    util2.DataBuffer.prototype.putBuffer = function(buffer) {
      this.putBytes(buffer);
      buffer.clear();
      return this;
    };
    util2.DataBuffer.prototype.putString = function(str) {
      return this.putBytes(str, "utf16");
    };
    util2.DataBuffer.prototype.putInt16 = function(i) {
      this.accommodate(2);
      this.data.setInt16(this.write, i);
      this.write += 2;
      return this;
    };
    util2.DataBuffer.prototype.putInt24 = function(i) {
      this.accommodate(3);
      this.data.setInt16(this.write, i >> 8 & 65535);
      this.data.setInt8(this.write, i >> 16 & 255);
      this.write += 3;
      return this;
    };
    util2.DataBuffer.prototype.putInt32 = function(i) {
      this.accommodate(4);
      this.data.setInt32(this.write, i);
      this.write += 4;
      return this;
    };
    util2.DataBuffer.prototype.putInt16Le = function(i) {
      this.accommodate(2);
      this.data.setInt16(this.write, i, true);
      this.write += 2;
      return this;
    };
    util2.DataBuffer.prototype.putInt24Le = function(i) {
      this.accommodate(3);
      this.data.setInt8(this.write, i >> 16 & 255);
      this.data.setInt16(this.write, i >> 8 & 65535, true);
      this.write += 3;
      return this;
    };
    util2.DataBuffer.prototype.putInt32Le = function(i) {
      this.accommodate(4);
      this.data.setInt32(this.write, i, true);
      this.write += 4;
      return this;
    };
    util2.DataBuffer.prototype.putInt = function(i, n) {
      _checkBitsParam(n);
      this.accommodate(n / 8);
      do {
        n -= 8;
        this.data.setInt8(this.write++, i >> n & 255);
      } while (n > 0);
      return this;
    };
    util2.DataBuffer.prototype.putSignedInt = function(i, n) {
      _checkBitsParam(n);
      this.accommodate(n / 8);
      if (i < 0) {
        i += 2 << n - 1;
      }
      return this.putInt(i, n);
    };
    util2.DataBuffer.prototype.getByte = function() {
      return this.data.getInt8(this.read++);
    };
    util2.DataBuffer.prototype.getInt16 = function() {
      var rval = this.data.getInt16(this.read);
      this.read += 2;
      return rval;
    };
    util2.DataBuffer.prototype.getInt24 = function() {
      var rval = this.data.getInt16(this.read) << 8 ^ this.data.getInt8(this.read + 2);
      this.read += 3;
      return rval;
    };
    util2.DataBuffer.prototype.getInt32 = function() {
      var rval = this.data.getInt32(this.read);
      this.read += 4;
      return rval;
    };
    util2.DataBuffer.prototype.getInt16Le = function() {
      var rval = this.data.getInt16(this.read, true);
      this.read += 2;
      return rval;
    };
    util2.DataBuffer.prototype.getInt24Le = function() {
      var rval = this.data.getInt8(this.read) ^ this.data.getInt16(this.read + 1, true) << 8;
      this.read += 3;
      return rval;
    };
    util2.DataBuffer.prototype.getInt32Le = function() {
      var rval = this.data.getInt32(this.read, true);
      this.read += 4;
      return rval;
    };
    util2.DataBuffer.prototype.getInt = function(n) {
      _checkBitsParam(n);
      var rval = 0;
      do {
        rval = (rval << 8) + this.data.getInt8(this.read++);
        n -= 8;
      } while (n > 0);
      return rval;
    };
    util2.DataBuffer.prototype.getSignedInt = function(n) {
      var x = this.getInt(n);
      var max = 2 << n - 2;
      if (x >= max) {
        x -= max << 1;
      }
      return x;
    };
    util2.DataBuffer.prototype.getBytes = function(count) {
      var rval;
      if (count) {
        count = Math.min(this.length(), count);
        rval = this.data.slice(this.read, this.read + count);
        this.read += count;
      } else if (count === 0) {
        rval = "";
      } else {
        rval = this.read === 0 ? this.data : this.data.slice(this.read);
        this.clear();
      }
      return rval;
    };
    util2.DataBuffer.prototype.bytes = function(count) {
      return typeof count === "undefined" ? this.data.slice(this.read) : this.data.slice(this.read, this.read + count);
    };
    util2.DataBuffer.prototype.at = function(i) {
      return this.data.getUint8(this.read + i);
    };
    util2.DataBuffer.prototype.setAt = function(i, b) {
      this.data.setUint8(i, b);
      return this;
    };
    util2.DataBuffer.prototype.last = function() {
      return this.data.getUint8(this.write - 1);
    };
    util2.DataBuffer.prototype.copy = function() {
      return new util2.DataBuffer(this);
    };
    util2.DataBuffer.prototype.compact = function() {
      if (this.read > 0) {
        var src3 = new Uint8Array(this.data.buffer, this.read);
        var dst = new Uint8Array(src3.byteLength);
        dst.set(src3);
        this.data = new DataView(dst);
        this.write -= this.read;
        this.read = 0;
      }
      return this;
    };
    util2.DataBuffer.prototype.clear = function() {
      this.data = new DataView(new ArrayBuffer(0));
      this.read = this.write = 0;
      return this;
    };
    util2.DataBuffer.prototype.truncate = function(count) {
      this.write = Math.max(0, this.length() - count);
      this.read = Math.min(this.read, this.write);
      return this;
    };
    util2.DataBuffer.prototype.toHex = function() {
      var rval = "";
      for (var i = this.read; i < this.data.byteLength; ++i) {
        var b = this.data.getUint8(i);
        if (b < 16) {
          rval += "0";
        }
        rval += b.toString(16);
      }
      return rval;
    };
    util2.DataBuffer.prototype.toString = function(encoding) {
      var view = new Uint8Array(this.data, this.read, this.length());
      encoding = encoding || "utf8";
      if (encoding === "binary" || encoding === "raw") {
        return util2.binary.raw.encode(view);
      }
      if (encoding === "hex") {
        return util2.binary.hex.encode(view);
      }
      if (encoding === "base64") {
        return util2.binary.base64.encode(view);
      }
      if (encoding === "utf8") {
        return util2.text.utf8.decode(view);
      }
      if (encoding === "utf16") {
        return util2.text.utf16.decode(view);
      }
      throw new Error("Invalid encoding: " + encoding);
    };
    util2.createBuffer = function(input, encoding) {
      encoding = encoding || "raw";
      if (input !== void 0 && encoding === "utf8") {
        input = util2.encodeUtf8(input);
      }
      return new util2.ByteBuffer(input);
    };
    util2.fillString = function(c, n) {
      var s = "";
      while (n > 0) {
        if (n & 1) {
          s += c;
        }
        n >>>= 1;
        if (n > 0) {
          c += c;
        }
      }
      return s;
    };
    util2.xorBytes = function(s1, s2, n) {
      var s3 = "";
      var b = "";
      var t = "";
      var i = 0;
      var c = 0;
      for (; n > 0; --n, ++i) {
        b = s1.charCodeAt(i) ^ s2.charCodeAt(i);
        if (c >= 10) {
          s3 += t;
          t = "";
          c = 0;
        }
        t += String.fromCharCode(b);
        ++c;
      }
      s3 += t;
      return s3;
    };
    util2.hexToBytes = function(hex) {
      var rval = "";
      var i = 0;
      if (hex.length & true) {
        i = 1;
        rval += String.fromCharCode(parseInt(hex[0], 16));
      }
      for (; i < hex.length; i += 2) {
        rval += String.fromCharCode(parseInt(hex.substr(i, 2), 16));
      }
      return rval;
    };
    util2.bytesToHex = function(bytes) {
      return util2.createBuffer(bytes).toHex();
    };
    util2.int32ToBytes = function(i) {
      return String.fromCharCode(i >> 24 & 255) + String.fromCharCode(i >> 16 & 255) + String.fromCharCode(i >> 8 & 255) + String.fromCharCode(i & 255);
    };
    var _base64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=";
    var _base64Idx = [
      /*43 -43 = 0*/
      /*'+',  1,  2,  3,'/' */
      62,
      -1,
      -1,
      -1,
      63,
      /*'0','1','2','3','4','5','6','7','8','9' */
      52,
      53,
      54,
      55,
      56,
      57,
      58,
      59,
      60,
      61,
      /*15, 16, 17,'=', 19, 20, 21 */
      -1,
      -1,
      -1,
      64,
      -1,
      -1,
      -1,
      /*65 - 43 = 22*/
      /*'A','B','C','D','E','F','G','H','I','J','K','L','M', */
      0,
      1,
      2,
      3,
      4,
      5,
      6,
      7,
      8,
      9,
      10,
      11,
      12,
      /*'N','O','P','Q','R','S','T','U','V','W','X','Y','Z' */
      13,
      14,
      15,
      16,
      17,
      18,
      19,
      20,
      21,
      22,
      23,
      24,
      25,
      /*91 - 43 = 48 */
      /*48, 49, 50, 51, 52, 53 */
      -1,
      -1,
      -1,
      -1,
      -1,
      -1,
      /*97 - 43 = 54*/
      /*'a','b','c','d','e','f','g','h','i','j','k','l','m' */
      26,
      27,
      28,
      29,
      30,
      31,
      32,
      33,
      34,
      35,
      36,
      37,
      38,
      /*'n','o','p','q','r','s','t','u','v','w','x','y','z' */
      39,
      40,
      41,
      42,
      43,
      44,
      45,
      46,
      47,
      48,
      49,
      50,
      51
    ];
    var _base58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    util2.encode64 = function(input, maxline) {
      var line = "";
      var output = "";
      var chr1, chr2, chr3;
      var i = 0;
      while (i < input.length) {
        chr1 = input.charCodeAt(i++);
        chr2 = input.charCodeAt(i++);
        chr3 = input.charCodeAt(i++);
        line += _base64.charAt(chr1 >> 2);
        line += _base64.charAt((chr1 & 3) << 4 | chr2 >> 4);
        if (isNaN(chr2)) {
          line += "==";
        } else {
          line += _base64.charAt((chr2 & 15) << 2 | chr3 >> 6);
          line += isNaN(chr3) ? "=" : _base64.charAt(chr3 & 63);
        }
        if (maxline && line.length > maxline) {
          output += line.substr(0, maxline) + "\r\n";
          line = line.substr(maxline);
        }
      }
      output += line;
      return output;
    };
    util2.decode64 = function(input) {
      input = input.replace(/[^A-Za-z0-9\+\/\=]/g, "");
      var output = "";
      var enc1, enc2, enc3, enc4;
      var i = 0;
      while (i < input.length) {
        enc1 = _base64Idx[input.charCodeAt(i++) - 43];
        enc2 = _base64Idx[input.charCodeAt(i++) - 43];
        enc3 = _base64Idx[input.charCodeAt(i++) - 43];
        enc4 = _base64Idx[input.charCodeAt(i++) - 43];
        output += String.fromCharCode(enc1 << 2 | enc2 >> 4);
        if (enc3 !== 64) {
          output += String.fromCharCode((enc2 & 15) << 4 | enc3 >> 2);
          if (enc4 !== 64) {
            output += String.fromCharCode((enc3 & 3) << 6 | enc4);
          }
        }
      }
      return output;
    };
    util2.encodeUtf8 = function(str) {
      return unescape(encodeURIComponent(str));
    };
    util2.decodeUtf8 = function(str) {
      return decodeURIComponent(escape(str));
    };
    util2.binary = {
      raw: {},
      hex: {},
      base64: {},
      base58: {},
      baseN: {
        encode: baseN.encode,
        decode: baseN.decode
      }
    };
    util2.binary.raw.encode = function(bytes) {
      return String.fromCharCode.apply(null, bytes);
    };
    util2.binary.raw.decode = function(str, output, offset) {
      var out = output;
      if (!out) {
        out = new Uint8Array(str.length);
      }
      offset = offset || 0;
      var j = offset;
      for (var i = 0; i < str.length; ++i) {
        out[j++] = str.charCodeAt(i);
      }
      return output ? j - offset : out;
    };
    util2.binary.hex.encode = util2.bytesToHex;
    util2.binary.hex.decode = function(hex, output, offset) {
      var out = output;
      if (!out) {
        out = new Uint8Array(Math.ceil(hex.length / 2));
      }
      offset = offset || 0;
      var i = 0, j = offset;
      if (hex.length & 1) {
        i = 1;
        out[j++] = parseInt(hex[0], 16);
      }
      for (; i < hex.length; i += 2) {
        out[j++] = parseInt(hex.substr(i, 2), 16);
      }
      return output ? j - offset : out;
    };
    util2.binary.base64.encode = function(input, maxline) {
      var line = "";
      var output = "";
      var chr1, chr2, chr3;
      var i = 0;
      while (i < input.byteLength) {
        chr1 = input[i++];
        chr2 = input[i++];
        chr3 = input[i++];
        line += _base64.charAt(chr1 >> 2);
        line += _base64.charAt((chr1 & 3) << 4 | chr2 >> 4);
        if (isNaN(chr2)) {
          line += "==";
        } else {
          line += _base64.charAt((chr2 & 15) << 2 | chr3 >> 6);
          line += isNaN(chr3) ? "=" : _base64.charAt(chr3 & 63);
        }
        if (maxline && line.length > maxline) {
          output += line.substr(0, maxline) + "\r\n";
          line = line.substr(maxline);
        }
      }
      output += line;
      return output;
    };
    util2.binary.base64.decode = function(input, output, offset) {
      var out = output;
      if (!out) {
        out = new Uint8Array(Math.ceil(input.length / 4) * 3);
      }
      input = input.replace(/[^A-Za-z0-9\+\/\=]/g, "");
      offset = offset || 0;
      var enc1, enc2, enc3, enc4;
      var i = 0, j = offset;
      while (i < input.length) {
        enc1 = _base64Idx[input.charCodeAt(i++) - 43];
        enc2 = _base64Idx[input.charCodeAt(i++) - 43];
        enc3 = _base64Idx[input.charCodeAt(i++) - 43];
        enc4 = _base64Idx[input.charCodeAt(i++) - 43];
        out[j++] = enc1 << 2 | enc2 >> 4;
        if (enc3 !== 64) {
          out[j++] = (enc2 & 15) << 4 | enc3 >> 2;
          if (enc4 !== 64) {
            out[j++] = (enc3 & 3) << 6 | enc4;
          }
        }
      }
      return output ? j - offset : out.subarray(0, j);
    };
    util2.binary.base58.encode = function(input, maxline) {
      return util2.binary.baseN.encode(input, _base58, maxline);
    };
    util2.binary.base58.decode = function(input, maxline) {
      return util2.binary.baseN.decode(input, _base58, maxline);
    };
    util2.text = {
      utf8: {},
      utf16: {}
    };
    util2.text.utf8.encode = function(str, output, offset) {
      str = util2.encodeUtf8(str);
      var out = output;
      if (!out) {
        out = new Uint8Array(str.length);
      }
      offset = offset || 0;
      var j = offset;
      for (var i = 0; i < str.length; ++i) {
        out[j++] = str.charCodeAt(i);
      }
      return output ? j - offset : out;
    };
    util2.text.utf8.decode = function(bytes) {
      return util2.decodeUtf8(String.fromCharCode.apply(null, bytes));
    };
    util2.text.utf16.encode = function(str, output, offset) {
      var out = output;
      if (!out) {
        out = new Uint8Array(str.length * 2);
      }
      var view = new Uint16Array(out.buffer);
      offset = offset || 0;
      var j = offset;
      var k = offset;
      for (var i = 0; i < str.length; ++i) {
        view[k++] = str.charCodeAt(i);
        j += 2;
      }
      return output ? j - offset : out;
    };
    util2.text.utf16.decode = function(bytes) {
      return String.fromCharCode.apply(null, new Uint16Array(bytes.buffer));
    };
    util2.deflate = function(api, bytes, raw) {
      bytes = util2.decode64(api.deflate(util2.encode64(bytes)).rval);
      if (raw) {
        var start = 2;
        var flg = bytes.charCodeAt(1);
        if (flg & 32) {
          start = 6;
        }
        bytes = bytes.substring(start, bytes.length - 4);
      }
      return bytes;
    };
    util2.inflate = function(api, bytes, raw) {
      var rval = api.inflate(util2.encode64(bytes)).rval;
      return rval === null ? null : util2.decode64(rval);
    };
    var _setStorageObject = function(api, id, obj) {
      if (!api) {
        throw new Error("WebStorage not available.");
      }
      var rval;
      if (obj === null) {
        rval = api.removeItem(id);
      } else {
        obj = util2.encode64(JSON.stringify(obj));
        rval = api.setItem(id, obj);
      }
      if (typeof rval !== "undefined" && rval.rval !== true) {
        var error = new Error(rval.error.message);
        error.id = rval.error.id;
        error.name = rval.error.name;
        throw error;
      }
    };
    var _getStorageObject = function(api, id) {
      if (!api) {
        throw new Error("WebStorage not available.");
      }
      var rval = api.getItem(id);
      if (api.init) {
        if (rval.rval === null) {
          if (rval.error) {
            var error = new Error(rval.error.message);
            error.id = rval.error.id;
            error.name = rval.error.name;
            throw error;
          }
          rval = null;
        } else {
          rval = rval.rval;
        }
      }
      if (rval !== null) {
        rval = JSON.parse(util2.decode64(rval));
      }
      return rval;
    };
    var _setItem = function(api, id, key, data) {
      var obj = _getStorageObject(api, id);
      if (obj === null) {
        obj = {};
      }
      obj[key] = data;
      _setStorageObject(api, id, obj);
    };
    var _getItem = function(api, id, key) {
      var rval = _getStorageObject(api, id);
      if (rval !== null) {
        rval = key in rval ? rval[key] : null;
      }
      return rval;
    };
    var _removeItem = function(api, id, key) {
      var obj = _getStorageObject(api, id);
      if (obj !== null && key in obj) {
        delete obj[key];
        var empty3 = true;
        for (var prop in obj) {
          empty3 = false;
          break;
        }
        if (empty3) {
          obj = null;
        }
        _setStorageObject(api, id, obj);
      }
    };
    var _clearItems = function(api, id) {
      _setStorageObject(api, id, null);
    };
    var _callStorageFunction = function(func, args, location) {
      var rval = null;
      if (typeof location === "undefined") {
        location = ["web", "flash"];
      }
      var type;
      var done = false;
      var exception = null;
      for (var idx in location) {
        type = location[idx];
        try {
          if (type === "flash" || type === "both") {
            if (args[0] === null) {
              throw new Error("Flash local storage not available.");
            }
            rval = func.apply(this, args);
            done = type === "flash";
          }
          if (type === "web" || type === "both") {
            args[0] = localStorage;
            rval = func.apply(this, args);
            done = true;
          }
        } catch (ex) {
          exception = ex;
        }
        if (done) {
          break;
        }
      }
      if (!done) {
        throw exception;
      }
      return rval;
    };
    util2.setItem = function(api, id, key, data, location) {
      _callStorageFunction(_setItem, arguments, location);
    };
    util2.getItem = function(api, id, key, location) {
      return _callStorageFunction(_getItem, arguments, location);
    };
    util2.removeItem = function(api, id, key, location) {
      _callStorageFunction(_removeItem, arguments, location);
    };
    util2.clearItems = function(api, id, location) {
      _callStorageFunction(_clearItems, arguments, location);
    };
    util2.isEmpty = function(obj) {
      for (var prop in obj) {
        if (obj.hasOwnProperty(prop)) {
          return false;
        }
      }
      return true;
    };
    util2.format = function(format3) {
      var re = /%./g;
      var match;
      var part;
      var argi = 0;
      var parts = [];
      var last = 0;
      while (match = re.exec(format3)) {
        part = format3.substring(last, re.lastIndex - 2);
        if (part.length > 0) {
          parts.push(part);
        }
        last = re.lastIndex;
        var code3 = match[0][1];
        switch (code3) {
          case "s":
          case "o":
            if (argi < arguments.length) {
              parts.push(arguments[argi++ + 1]);
            } else {
              parts.push("<?>");
            }
            break;
          case "%":
            parts.push("%");
            break;
          default:
            parts.push("<%" + code3 + "?>");
        }
      }
      parts.push(format3.substring(last));
      return parts.join("");
    };
    util2.formatNumber = function(number, decimals, dec_point, thousands_sep) {
      var n = number, c = isNaN(decimals = Math.abs(decimals)) ? 2 : decimals;
      var d = dec_point === void 0 ? "," : dec_point;
      var t = thousands_sep === void 0 ? "." : thousands_sep, s = n < 0 ? "-" : "";
      var i = parseInt(n = Math.abs(+n || 0).toFixed(c), 10) + "";
      var j = i.length > 3 ? i.length % 3 : 0;
      return s + (j ? i.substr(0, j) + t : "") + i.substr(j).replace(/(\d{3})(?=\d)/g, "$1" + t) + (c ? d + Math.abs(n - i).toFixed(c).slice(2) : "");
    };
    util2.formatSize = function(size) {
      if (size >= 1073741824) {
        size = util2.formatNumber(size / 1073741824, 2, ".", "") + " GiB";
      } else if (size >= 1048576) {
        size = util2.formatNumber(size / 1048576, 2, ".", "") + " MiB";
      } else if (size >= 1024) {
        size = util2.formatNumber(size / 1024, 0) + " KiB";
      } else {
        size = util2.formatNumber(size, 0) + " bytes";
      }
      return size;
    };
    util2.bytesFromIP = function(ip) {
      if (ip.indexOf(".") !== -1) {
        return util2.bytesFromIPv4(ip);
      }
      if (ip.indexOf(":") !== -1) {
        return util2.bytesFromIPv6(ip);
      }
      return null;
    };
    util2.bytesFromIPv4 = function(ip) {
      ip = ip.split(".");
      if (ip.length !== 4) {
        return null;
      }
      var b = util2.createBuffer();
      for (var i = 0; i < ip.length; ++i) {
        var num = parseInt(ip[i], 10);
        if (isNaN(num)) {
          return null;
        }
        b.putByte(num);
      }
      return b.getBytes();
    };
    util2.bytesFromIPv6 = function(ip) {
      var blanks = 0;
      ip = ip.split(":").filter(function(e) {
        if (e.length === 0)
          ++blanks;
        return true;
      });
      var zeros = (8 - ip.length + blanks) * 2;
      var b = util2.createBuffer();
      for (var i = 0; i < 8; ++i) {
        if (!ip[i] || ip[i].length === 0) {
          b.fillWithByte(0, zeros);
          zeros = 0;
          continue;
        }
        var bytes = util2.hexToBytes(ip[i]);
        if (bytes.length < 2) {
          b.putByte(0);
        }
        b.putBytes(bytes);
      }
      return b.getBytes();
    };
    util2.bytesToIP = function(bytes) {
      if (bytes.length === 4) {
        return util2.bytesToIPv4(bytes);
      }
      if (bytes.length === 16) {
        return util2.bytesToIPv6(bytes);
      }
      return null;
    };
    util2.bytesToIPv4 = function(bytes) {
      if (bytes.length !== 4) {
        return null;
      }
      var ip = [];
      for (var i = 0; i < bytes.length; ++i) {
        ip.push(bytes.charCodeAt(i));
      }
      return ip.join(".");
    };
    util2.bytesToIPv6 = function(bytes) {
      if (bytes.length !== 16) {
        return null;
      }
      var ip = [];
      var zeroGroups = [];
      var zeroMaxGroup = 0;
      for (var i = 0; i < bytes.length; i += 2) {
        var hex = util2.bytesToHex(bytes[i] + bytes[i + 1]);
        while (hex[0] === "0" && hex !== "0") {
          hex = hex.substr(1);
        }
        if (hex === "0") {
          var last = zeroGroups[zeroGroups.length - 1];
          var idx = ip.length;
          if (!last || idx !== last.end + 1) {
            zeroGroups.push({ start: idx, end: idx });
          } else {
            last.end = idx;
            if (last.end - last.start > zeroGroups[zeroMaxGroup].end - zeroGroups[zeroMaxGroup].start) {
              zeroMaxGroup = zeroGroups.length - 1;
            }
          }
        }
        ip.push(hex);
      }
      if (zeroGroups.length > 0) {
        var group = zeroGroups[zeroMaxGroup];
        if (group.end - group.start > 0) {
          ip.splice(group.start, group.end - group.start + 1, "");
          if (group.start === 0) {
            ip.unshift("");
          }
          if (group.end === 7) {
            ip.push("");
          }
        }
      }
      return ip.join(":");
    };
    util2.estimateCores = function(options, callback) {
      if (typeof options === "function") {
        callback = options;
        options = {};
      }
      options = options || {};
      if ("cores" in util2 && !options.update) {
        return callback(null, util2.cores);
      }
      if (typeof navigator !== "undefined" && "hardwareConcurrency" in navigator && navigator.hardwareConcurrency > 0) {
        util2.cores = navigator.hardwareConcurrency;
        return callback(null, util2.cores);
      }
      if (typeof Worker === "undefined") {
        util2.cores = 1;
        return callback(null, util2.cores);
      }
      if (typeof Blob === "undefined") {
        util2.cores = 2;
        return callback(null, util2.cores);
      }
      var blobUrl = URL.createObjectURL(new Blob([
        "(",
        function() {
          self.addEventListener("message", function(e) {
            var st = Date.now();
            var et = st + 4;
            while (Date.now() < et)
              ;
            self.postMessage({ st, et });
          });
        }.toString(),
        ")()"
      ], { type: "application/javascript" }));
      sample([], 5, 16);
      function sample(max, samples, numWorkers) {
        if (samples === 0) {
          var avg = Math.floor(max.reduce(function(avg2, x) {
            return avg2 + x;
          }, 0) / max.length);
          util2.cores = Math.max(1, avg);
          URL.revokeObjectURL(blobUrl);
          return callback(null, util2.cores);
        }
        map(numWorkers, function(err, results) {
          max.push(reduce(numWorkers, results));
          sample(max, samples - 1, numWorkers);
        });
      }
      function map(numWorkers, callback2) {
        var workers = [];
        var results = [];
        for (var i = 0; i < numWorkers; ++i) {
          var worker = new Worker(blobUrl);
          worker.addEventListener("message", function(e) {
            results.push(e.data);
            if (results.length === numWorkers) {
              for (var i2 = 0; i2 < numWorkers; ++i2) {
                workers[i2].terminate();
              }
              callback2(null, results);
            }
          });
          workers.push(worker);
        }
        for (var i = 0; i < numWorkers; ++i) {
          workers[i].postMessage(i);
        }
      }
      function reduce(numWorkers, results) {
        var overlaps = [];
        for (var n = 0; n < numWorkers; ++n) {
          var r1 = results[n];
          var overlap = overlaps[n] = [];
          for (var i = 0; i < numWorkers; ++i) {
            if (n === i) {
              continue;
            }
            var r2 = results[i];
            if (r1.st > r2.st && r1.st < r2.et || r2.st > r1.st && r2.st < r1.et) {
              overlap.push(i);
            }
          }
        }
        return overlaps.reduce(function(max, overlap2) {
          return Math.max(max, overlap2.length);
        }, 0);
      }
    };
  }
});

// ../../node_modules/node-forge/lib/oids.js
var require_oids = __commonJS({
  "../../node_modules/node-forge/lib/oids.js"(exports2, module2) {
    var forge6 = require_forge();
    forge6.pki = forge6.pki || {};
    var oids = module2.exports = forge6.pki.oids = forge6.oids = forge6.oids || {};
    function _IN(id, name3) {
      oids[id] = name3;
      oids[name3] = id;
    }
    function _I_(id, name3) {
      oids[id] = name3;
    }
    _IN("1.2.840.113549.1.1.1", "rsaEncryption");
    _IN("1.2.840.113549.1.1.4", "md5WithRSAEncryption");
    _IN("1.2.840.113549.1.1.5", "sha1WithRSAEncryption");
    _IN("1.2.840.113549.1.1.7", "RSAES-OAEP");
    _IN("1.2.840.113549.1.1.8", "mgf1");
    _IN("1.2.840.113549.1.1.9", "pSpecified");
    _IN("1.2.840.113549.1.1.10", "RSASSA-PSS");
    _IN("1.2.840.113549.1.1.11", "sha256WithRSAEncryption");
    _IN("1.2.840.113549.1.1.12", "sha384WithRSAEncryption");
    _IN("1.2.840.113549.1.1.13", "sha512WithRSAEncryption");
    _IN("1.3.101.112", "EdDSA25519");
    _IN("1.2.840.10040.4.3", "dsa-with-sha1");
    _IN("1.3.14.3.2.7", "desCBC");
    _IN("1.3.14.3.2.26", "sha1");
    _IN("1.3.14.3.2.29", "sha1WithRSASignature");
    _IN("2.16.840.1.101.3.4.2.1", "sha256");
    _IN("2.16.840.1.101.3.4.2.2", "sha384");
    _IN("2.16.840.1.101.3.4.2.3", "sha512");
    _IN("2.16.840.1.101.3.4.2.4", "sha224");
    _IN("2.16.840.1.101.3.4.2.5", "sha512-224");
    _IN("2.16.840.1.101.3.4.2.6", "sha512-256");
    _IN("1.2.840.113549.2.2", "md2");
    _IN("1.2.840.113549.2.5", "md5");
    _IN("1.2.840.113549.1.7.1", "data");
    _IN("1.2.840.113549.1.7.2", "signedData");
    _IN("1.2.840.113549.1.7.3", "envelopedData");
    _IN("1.2.840.113549.1.7.4", "signedAndEnvelopedData");
    _IN("1.2.840.113549.1.7.5", "digestedData");
    _IN("1.2.840.113549.1.7.6", "encryptedData");
    _IN("1.2.840.113549.1.9.1", "emailAddress");
    _IN("1.2.840.113549.1.9.2", "unstructuredName");
    _IN("1.2.840.113549.1.9.3", "contentType");
    _IN("1.2.840.113549.1.9.4", "messageDigest");
    _IN("1.2.840.113549.1.9.5", "signingTime");
    _IN("1.2.840.113549.1.9.6", "counterSignature");
    _IN("1.2.840.113549.1.9.7", "challengePassword");
    _IN("1.2.840.113549.1.9.8", "unstructuredAddress");
    _IN("1.2.840.113549.1.9.14", "extensionRequest");
    _IN("1.2.840.113549.1.9.20", "friendlyName");
    _IN("1.2.840.113549.1.9.21", "localKeyId");
    _IN("1.2.840.113549.1.9.22.1", "x509Certificate");
    _IN("1.2.840.113549.1.12.10.1.1", "keyBag");
    _IN("1.2.840.113549.1.12.10.1.2", "pkcs8ShroudedKeyBag");
    _IN("1.2.840.113549.1.12.10.1.3", "certBag");
    _IN("1.2.840.113549.1.12.10.1.4", "crlBag");
    _IN("1.2.840.113549.1.12.10.1.5", "secretBag");
    _IN("1.2.840.113549.1.12.10.1.6", "safeContentsBag");
    _IN("1.2.840.113549.1.5.13", "pkcs5PBES2");
    _IN("1.2.840.113549.1.5.12", "pkcs5PBKDF2");
    _IN("1.2.840.113549.1.12.1.1", "pbeWithSHAAnd128BitRC4");
    _IN("1.2.840.113549.1.12.1.2", "pbeWithSHAAnd40BitRC4");
    _IN("1.2.840.113549.1.12.1.3", "pbeWithSHAAnd3-KeyTripleDES-CBC");
    _IN("1.2.840.113549.1.12.1.4", "pbeWithSHAAnd2-KeyTripleDES-CBC");
    _IN("1.2.840.113549.1.12.1.5", "pbeWithSHAAnd128BitRC2-CBC");
    _IN("1.2.840.113549.1.12.1.6", "pbewithSHAAnd40BitRC2-CBC");
    _IN("1.2.840.113549.2.7", "hmacWithSHA1");
    _IN("1.2.840.113549.2.8", "hmacWithSHA224");
    _IN("1.2.840.113549.2.9", "hmacWithSHA256");
    _IN("1.2.840.113549.2.10", "hmacWithSHA384");
    _IN("1.2.840.113549.2.11", "hmacWithSHA512");
    _IN("1.2.840.113549.3.7", "des-EDE3-CBC");
    _IN("2.16.840.1.101.3.4.1.2", "aes128-CBC");
    _IN("2.16.840.1.101.3.4.1.22", "aes192-CBC");
    _IN("2.16.840.1.101.3.4.1.42", "aes256-CBC");
    _IN("2.5.4.3", "commonName");
    _IN("2.5.4.4", "surname");
    _IN("2.5.4.5", "serialNumber");
    _IN("2.5.4.6", "countryName");
    _IN("2.5.4.7", "localityName");
    _IN("2.5.4.8", "stateOrProvinceName");
    _IN("2.5.4.9", "streetAddress");
    _IN("2.5.4.10", "organizationName");
    _IN("2.5.4.11", "organizationalUnitName");
    _IN("2.5.4.12", "title");
    _IN("2.5.4.13", "description");
    _IN("2.5.4.15", "businessCategory");
    _IN("2.5.4.17", "postalCode");
    _IN("2.5.4.42", "givenName");
    _IN("1.3.6.1.4.1.311.60.2.1.2", "jurisdictionOfIncorporationStateOrProvinceName");
    _IN("1.3.6.1.4.1.311.60.2.1.3", "jurisdictionOfIncorporationCountryName");
    _IN("2.16.840.1.113730.1.1", "nsCertType");
    _IN("2.16.840.1.113730.1.13", "nsComment");
    _I_("2.5.29.1", "authorityKeyIdentifier");
    _I_("2.5.29.2", "keyAttributes");
    _I_("2.5.29.3", "certificatePolicies");
    _I_("2.5.29.4", "keyUsageRestriction");
    _I_("2.5.29.5", "policyMapping");
    _I_("2.5.29.6", "subtreesConstraint");
    _I_("2.5.29.7", "subjectAltName");
    _I_("2.5.29.8", "issuerAltName");
    _I_("2.5.29.9", "subjectDirectoryAttributes");
    _I_("2.5.29.10", "basicConstraints");
    _I_("2.5.29.11", "nameConstraints");
    _I_("2.5.29.12", "policyConstraints");
    _I_("2.5.29.13", "basicConstraints");
    _IN("2.5.29.14", "subjectKeyIdentifier");
    _IN("2.5.29.15", "keyUsage");
    _I_("2.5.29.16", "privateKeyUsagePeriod");
    _IN("2.5.29.17", "subjectAltName");
    _IN("2.5.29.18", "issuerAltName");
    _IN("2.5.29.19", "basicConstraints");
    _I_("2.5.29.20", "cRLNumber");
    _I_("2.5.29.21", "cRLReason");
    _I_("2.5.29.22", "expirationDate");
    _I_("2.5.29.23", "instructionCode");
    _I_("2.5.29.24", "invalidityDate");
    _I_("2.5.29.25", "cRLDistributionPoints");
    _I_("2.5.29.26", "issuingDistributionPoint");
    _I_("2.5.29.27", "deltaCRLIndicator");
    _I_("2.5.29.28", "issuingDistributionPoint");
    _I_("2.5.29.29", "certificateIssuer");
    _I_("2.5.29.30", "nameConstraints");
    _IN("2.5.29.31", "cRLDistributionPoints");
    _IN("2.5.29.32", "certificatePolicies");
    _I_("2.5.29.33", "policyMappings");
    _I_("2.5.29.34", "policyConstraints");
    _IN("2.5.29.35", "authorityKeyIdentifier");
    _I_("2.5.29.36", "policyConstraints");
    _IN("2.5.29.37", "extKeyUsage");
    _I_("2.5.29.46", "freshestCRL");
    _I_("2.5.29.54", "inhibitAnyPolicy");
    _IN("1.3.6.1.4.1.11129.2.4.2", "timestampList");
    _IN("1.3.6.1.5.5.7.1.1", "authorityInfoAccess");
    _IN("1.3.6.1.5.5.7.3.1", "serverAuth");
    _IN("1.3.6.1.5.5.7.3.2", "clientAuth");
    _IN("1.3.6.1.5.5.7.3.3", "codeSigning");
    _IN("1.3.6.1.5.5.7.3.4", "emailProtection");
    _IN("1.3.6.1.5.5.7.3.8", "timeStamping");
  }
});

// ../../node_modules/node-forge/lib/asn1.js
var require_asn1 = __commonJS({
  "../../node_modules/node-forge/lib/asn1.js"(exports2, module2) {
    var forge6 = require_forge();
    require_util();
    require_oids();
    var asn1 = module2.exports = forge6.asn1 = forge6.asn1 || {};
    asn1.Class = {
      UNIVERSAL: 0,
      APPLICATION: 64,
      CONTEXT_SPECIFIC: 128,
      PRIVATE: 192
    };
    asn1.Type = {
      NONE: 0,
      BOOLEAN: 1,
      INTEGER: 2,
      BITSTRING: 3,
      OCTETSTRING: 4,
      NULL: 5,
      OID: 6,
      ODESC: 7,
      EXTERNAL: 8,
      REAL: 9,
      ENUMERATED: 10,
      EMBEDDED: 11,
      UTF8: 12,
      ROID: 13,
      SEQUENCE: 16,
      SET: 17,
      PRINTABLESTRING: 19,
      IA5STRING: 22,
      UTCTIME: 23,
      GENERALIZEDTIME: 24,
      BMPSTRING: 30
    };
    asn1.create = function(tagClass, type, constructed, value, options) {
      if (forge6.util.isArray(value)) {
        var tmp = [];
        for (var i = 0; i < value.length; ++i) {
          if (value[i] !== void 0) {
            tmp.push(value[i]);
          }
        }
        value = tmp;
      }
      var obj = {
        tagClass,
        type,
        constructed,
        composed: constructed || forge6.util.isArray(value),
        value
      };
      if (options && "bitStringContents" in options) {
        obj.bitStringContents = options.bitStringContents;
        obj.original = asn1.copy(obj);
      }
      return obj;
    };
    asn1.copy = function(obj, options) {
      var copy;
      if (forge6.util.isArray(obj)) {
        copy = [];
        for (var i = 0; i < obj.length; ++i) {
          copy.push(asn1.copy(obj[i], options));
        }
        return copy;
      }
      if (typeof obj === "string") {
        return obj;
      }
      copy = {
        tagClass: obj.tagClass,
        type: obj.type,
        constructed: obj.constructed,
        composed: obj.composed,
        value: asn1.copy(obj.value, options)
      };
      if (options && !options.excludeBitStringContents) {
        copy.bitStringContents = obj.bitStringContents;
      }
      return copy;
    };
    asn1.equals = function(obj1, obj2, options) {
      if (forge6.util.isArray(obj1)) {
        if (!forge6.util.isArray(obj2)) {
          return false;
        }
        if (obj1.length !== obj2.length) {
          return false;
        }
        for (var i = 0; i < obj1.length; ++i) {
          if (!asn1.equals(obj1[i], obj2[i])) {
            return false;
          }
        }
        return true;
      }
      if (typeof obj1 !== typeof obj2) {
        return false;
      }
      if (typeof obj1 === "string") {
        return obj1 === obj2;
      }
      var equal = obj1.tagClass === obj2.tagClass && obj1.type === obj2.type && obj1.constructed === obj2.constructed && obj1.composed === obj2.composed && asn1.equals(obj1.value, obj2.value);
      if (options && options.includeBitStringContents) {
        equal = equal && obj1.bitStringContents === obj2.bitStringContents;
      }
      return equal;
    };
    asn1.getBerValueLength = function(b) {
      var b2 = b.getByte();
      if (b2 === 128) {
        return void 0;
      }
      var length3;
      var longForm = b2 & 128;
      if (!longForm) {
        length3 = b2;
      } else {
        length3 = b.getInt((b2 & 127) << 3);
      }
      return length3;
    };
    function _checkBufferLength(bytes, remaining, n) {
      if (n > remaining) {
        var error = new Error("Too few bytes to parse DER.");
        error.available = bytes.length();
        error.remaining = remaining;
        error.requested = n;
        throw error;
      }
    }
    var _getValueLength = function(bytes, remaining) {
      var b2 = bytes.getByte();
      remaining--;
      if (b2 === 128) {
        return void 0;
      }
      var length3;
      var longForm = b2 & 128;
      if (!longForm) {
        length3 = b2;
      } else {
        var longFormBytes = b2 & 127;
        _checkBufferLength(bytes, remaining, longFormBytes);
        length3 = bytes.getInt(longFormBytes << 3);
      }
      if (length3 < 0) {
        throw new Error("Negative length: " + length3);
      }
      return length3;
    };
    asn1.fromDer = function(bytes, options) {
      if (options === void 0) {
        options = {
          strict: true,
          parseAllBytes: true,
          decodeBitStrings: true
        };
      }
      if (typeof options === "boolean") {
        options = {
          strict: options,
          parseAllBytes: true,
          decodeBitStrings: true
        };
      }
      if (!("strict" in options)) {
        options.strict = true;
      }
      if (!("parseAllBytes" in options)) {
        options.parseAllBytes = true;
      }
      if (!("decodeBitStrings" in options)) {
        options.decodeBitStrings = true;
      }
      if (typeof bytes === "string") {
        bytes = forge6.util.createBuffer(bytes);
      }
      var byteCount = bytes.length();
      var value = _fromDer(bytes, bytes.length(), 0, options);
      if (options.parseAllBytes && bytes.length() !== 0) {
        var error = new Error("Unparsed DER bytes remain after ASN.1 parsing.");
        error.byteCount = byteCount;
        error.remaining = bytes.length();
        throw error;
      }
      return value;
    };
    function _fromDer(bytes, remaining, depth, options) {
      var start;
      _checkBufferLength(bytes, remaining, 2);
      var b1 = bytes.getByte();
      remaining--;
      var tagClass = b1 & 192;
      var type = b1 & 31;
      start = bytes.length();
      var length3 = _getValueLength(bytes, remaining);
      remaining -= start - bytes.length();
      if (length3 !== void 0 && length3 > remaining) {
        if (options.strict) {
          var error = new Error("Too few bytes to read ASN.1 value.");
          error.available = bytes.length();
          error.remaining = remaining;
          error.requested = length3;
          throw error;
        }
        length3 = remaining;
      }
      var value;
      var bitStringContents;
      var constructed = (b1 & 32) === 32;
      if (constructed) {
        value = [];
        if (length3 === void 0) {
          for (; ; ) {
            _checkBufferLength(bytes, remaining, 2);
            if (bytes.bytes(2) === String.fromCharCode(0, 0)) {
              bytes.getBytes(2);
              remaining -= 2;
              break;
            }
            start = bytes.length();
            value.push(_fromDer(bytes, remaining, depth + 1, options));
            remaining -= start - bytes.length();
          }
        } else {
          while (length3 > 0) {
            start = bytes.length();
            value.push(_fromDer(bytes, length3, depth + 1, options));
            remaining -= start - bytes.length();
            length3 -= start - bytes.length();
          }
        }
      }
      if (value === void 0 && tagClass === asn1.Class.UNIVERSAL && type === asn1.Type.BITSTRING) {
        bitStringContents = bytes.bytes(length3);
      }
      if (value === void 0 && options.decodeBitStrings && tagClass === asn1.Class.UNIVERSAL && // FIXME: OCTET STRINGs not yet supported here
      // .. other parts of forge expect to decode OCTET STRINGs manually
      type === asn1.Type.BITSTRING && length3 > 1) {
        var savedRead = bytes.read;
        var savedRemaining = remaining;
        var unused = 0;
        if (type === asn1.Type.BITSTRING) {
          _checkBufferLength(bytes, remaining, 1);
          unused = bytes.getByte();
          remaining--;
        }
        if (unused === 0) {
          try {
            start = bytes.length();
            var subOptions = {
              // enforce strict mode to avoid parsing ASN.1 from plain data
              strict: true,
              decodeBitStrings: true
            };
            var composed = _fromDer(bytes, remaining, depth + 1, subOptions);
            var used = start - bytes.length();
            remaining -= used;
            if (type == asn1.Type.BITSTRING) {
              used++;
            }
            var tc = composed.tagClass;
            if (used === length3 && (tc === asn1.Class.UNIVERSAL || tc === asn1.Class.CONTEXT_SPECIFIC)) {
              value = [composed];
            }
          } catch (ex) {
          }
        }
        if (value === void 0) {
          bytes.read = savedRead;
          remaining = savedRemaining;
        }
      }
      if (value === void 0) {
        if (length3 === void 0) {
          if (options.strict) {
            throw new Error("Non-constructed ASN.1 object of indefinite length.");
          }
          length3 = remaining;
        }
        if (type === asn1.Type.BMPSTRING) {
          value = "";
          for (; length3 > 0; length3 -= 2) {
            _checkBufferLength(bytes, remaining, 2);
            value += String.fromCharCode(bytes.getInt16());
            remaining -= 2;
          }
        } else {
          value = bytes.getBytes(length3);
          remaining -= length3;
        }
      }
      var asn1Options = bitStringContents === void 0 ? null : {
        bitStringContents
      };
      return asn1.create(tagClass, type, constructed, value, asn1Options);
    }
    asn1.toDer = function(obj) {
      var bytes = forge6.util.createBuffer();
      var b1 = obj.tagClass | obj.type;
      var value = forge6.util.createBuffer();
      var useBitStringContents = false;
      if ("bitStringContents" in obj) {
        useBitStringContents = true;
        if (obj.original) {
          useBitStringContents = asn1.equals(obj, obj.original);
        }
      }
      if (useBitStringContents) {
        value.putBytes(obj.bitStringContents);
      } else if (obj.composed) {
        if (obj.constructed) {
          b1 |= 32;
        } else {
          value.putByte(0);
        }
        for (var i = 0; i < obj.value.length; ++i) {
          if (obj.value[i] !== void 0) {
            value.putBuffer(asn1.toDer(obj.value[i]));
          }
        }
      } else {
        if (obj.type === asn1.Type.BMPSTRING) {
          for (var i = 0; i < obj.value.length; ++i) {
            value.putInt16(obj.value.charCodeAt(i));
          }
        } else {
          if (obj.type === asn1.Type.INTEGER && obj.value.length > 1 && // leading 0x00 for positive integer
          (obj.value.charCodeAt(0) === 0 && (obj.value.charCodeAt(1) & 128) === 0 || // leading 0xFF for negative integer
          obj.value.charCodeAt(0) === 255 && (obj.value.charCodeAt(1) & 128) === 128)) {
            value.putBytes(obj.value.substr(1));
          } else {
            value.putBytes(obj.value);
          }
        }
      }
      bytes.putByte(b1);
      if (value.length() <= 127) {
        bytes.putByte(value.length() & 127);
      } else {
        var len = value.length();
        var lenBytes = "";
        do {
          lenBytes += String.fromCharCode(len & 255);
          len = len >>> 8;
        } while (len > 0);
        bytes.putByte(lenBytes.length | 128);
        for (var i = lenBytes.length - 1; i >= 0; --i) {
          bytes.putByte(lenBytes.charCodeAt(i));
        }
      }
      bytes.putBuffer(value);
      return bytes;
    };
    asn1.oidToDer = function(oid) {
      var values = oid.split(".");
      var bytes = forge6.util.createBuffer();
      bytes.putByte(40 * parseInt(values[0], 10) + parseInt(values[1], 10));
      var last, valueBytes, value, b;
      for (var i = 2; i < values.length; ++i) {
        last = true;
        valueBytes = [];
        value = parseInt(values[i], 10);
        do {
          b = value & 127;
          value = value >>> 7;
          if (!last) {
            b |= 128;
          }
          valueBytes.push(b);
          last = false;
        } while (value > 0);
        for (var n = valueBytes.length - 1; n >= 0; --n) {
          bytes.putByte(valueBytes[n]);
        }
      }
      return bytes;
    };
    asn1.derToOid = function(bytes) {
      var oid;
      if (typeof bytes === "string") {
        bytes = forge6.util.createBuffer(bytes);
      }
      var b = bytes.getByte();
      oid = Math.floor(b / 40) + "." + b % 40;
      var value = 0;
      while (bytes.length() > 0) {
        b = bytes.getByte();
        value = value << 7;
        if (b & 128) {
          value += b & 127;
        } else {
          oid += "." + (value + b);
          value = 0;
        }
      }
      return oid;
    };
    asn1.utcTimeToDate = function(utc) {
      var date = /* @__PURE__ */ new Date();
      var year = parseInt(utc.substr(0, 2), 10);
      year = year >= 50 ? 1900 + year : 2e3 + year;
      var MM = parseInt(utc.substr(2, 2), 10) - 1;
      var DD = parseInt(utc.substr(4, 2), 10);
      var hh = parseInt(utc.substr(6, 2), 10);
      var mm = parseInt(utc.substr(8, 2), 10);
      var ss = 0;
      if (utc.length > 11) {
        var c = utc.charAt(10);
        var end = 10;
        if (c !== "+" && c !== "-") {
          ss = parseInt(utc.substr(10, 2), 10);
          end += 2;
        }
      }
      date.setUTCFullYear(year, MM, DD);
      date.setUTCHours(hh, mm, ss, 0);
      if (end) {
        c = utc.charAt(end);
        if (c === "+" || c === "-") {
          var hhoffset = parseInt(utc.substr(end + 1, 2), 10);
          var mmoffset = parseInt(utc.substr(end + 4, 2), 10);
          var offset = hhoffset * 60 + mmoffset;
          offset *= 6e4;
          if (c === "+") {
            date.setTime(+date - offset);
          } else {
            date.setTime(+date + offset);
          }
        }
      }
      return date;
    };
    asn1.generalizedTimeToDate = function(gentime) {
      var date = /* @__PURE__ */ new Date();
      var YYYY = parseInt(gentime.substr(0, 4), 10);
      var MM = parseInt(gentime.substr(4, 2), 10) - 1;
      var DD = parseInt(gentime.substr(6, 2), 10);
      var hh = parseInt(gentime.substr(8, 2), 10);
      var mm = parseInt(gentime.substr(10, 2), 10);
      var ss = parseInt(gentime.substr(12, 2), 10);
      var fff = 0;
      var offset = 0;
      var isUTC = false;
      if (gentime.charAt(gentime.length - 1) === "Z") {
        isUTC = true;
      }
      var end = gentime.length - 5, c = gentime.charAt(end);
      if (c === "+" || c === "-") {
        var hhoffset = parseInt(gentime.substr(end + 1, 2), 10);
        var mmoffset = parseInt(gentime.substr(end + 4, 2), 10);
        offset = hhoffset * 60 + mmoffset;
        offset *= 6e4;
        if (c === "+") {
          offset *= -1;
        }
        isUTC = true;
      }
      if (gentime.charAt(14) === ".") {
        fff = parseFloat(gentime.substr(14), 10) * 1e3;
      }
      if (isUTC) {
        date.setUTCFullYear(YYYY, MM, DD);
        date.setUTCHours(hh, mm, ss, fff);
        date.setTime(+date + offset);
      } else {
        date.setFullYear(YYYY, MM, DD);
        date.setHours(hh, mm, ss, fff);
      }
      return date;
    };
    asn1.dateToUtcTime = function(date) {
      if (typeof date === "string") {
        return date;
      }
      var rval = "";
      var format3 = [];
      format3.push(("" + date.getUTCFullYear()).substr(2));
      format3.push("" + (date.getUTCMonth() + 1));
      format3.push("" + date.getUTCDate());
      format3.push("" + date.getUTCHours());
      format3.push("" + date.getUTCMinutes());
      format3.push("" + date.getUTCSeconds());
      for (var i = 0; i < format3.length; ++i) {
        if (format3[i].length < 2) {
          rval += "0";
        }
        rval += format3[i];
      }
      rval += "Z";
      return rval;
    };
    asn1.dateToGeneralizedTime = function(date) {
      if (typeof date === "string") {
        return date;
      }
      var rval = "";
      var format3 = [];
      format3.push("" + date.getUTCFullYear());
      format3.push("" + (date.getUTCMonth() + 1));
      format3.push("" + date.getUTCDate());
      format3.push("" + date.getUTCHours());
      format3.push("" + date.getUTCMinutes());
      format3.push("" + date.getUTCSeconds());
      for (var i = 0; i < format3.length; ++i) {
        if (format3[i].length < 2) {
          rval += "0";
        }
        rval += format3[i];
      }
      rval += "Z";
      return rval;
    };
    asn1.integerToDer = function(x) {
      var rval = forge6.util.createBuffer();
      if (x >= -128 && x < 128) {
        return rval.putSignedInt(x, 8);
      }
      if (x >= -32768 && x < 32768) {
        return rval.putSignedInt(x, 16);
      }
      if (x >= -8388608 && x < 8388608) {
        return rval.putSignedInt(x, 24);
      }
      if (x >= -2147483648 && x < 2147483648) {
        return rval.putSignedInt(x, 32);
      }
      var error = new Error("Integer too large; max is 32-bits.");
      error.integer = x;
      throw error;
    };
    asn1.derToInteger = function(bytes) {
      if (typeof bytes === "string") {
        bytes = forge6.util.createBuffer(bytes);
      }
      var n = bytes.length() * 8;
      if (n > 32) {
        throw new Error("Integer too large; max is 32-bits.");
      }
      return bytes.getSignedInt(n);
    };
    asn1.validate = function(obj, v, capture, errors) {
      var rval = false;
      if ((obj.tagClass === v.tagClass || typeof v.tagClass === "undefined") && (obj.type === v.type || typeof v.type === "undefined")) {
        if (obj.constructed === v.constructed || typeof v.constructed === "undefined") {
          rval = true;
          if (v.value && forge6.util.isArray(v.value)) {
            var j = 0;
            for (var i = 0; rval && i < v.value.length; ++i) {
              rval = v.value[i].optional || false;
              if (obj.value[j]) {
                rval = asn1.validate(obj.value[j], v.value[i], capture, errors);
                if (rval) {
                  ++j;
                } else if (v.value[i].optional) {
                  rval = true;
                }
              }
              if (!rval && errors) {
                errors.push(
                  "[" + v.name + '] Tag class "' + v.tagClass + '", type "' + v.type + '" expected value length "' + v.value.length + '", got "' + obj.value.length + '"'
                );
              }
            }
          }
          if (rval && capture) {
            if (v.capture) {
              capture[v.capture] = obj.value;
            }
            if (v.captureAsn1) {
              capture[v.captureAsn1] = obj;
            }
            if (v.captureBitStringContents && "bitStringContents" in obj) {
              capture[v.captureBitStringContents] = obj.bitStringContents;
            }
            if (v.captureBitStringValue && "bitStringContents" in obj) {
              var value;
              if (obj.bitStringContents.length < 2) {
                capture[v.captureBitStringValue] = "";
              } else {
                var unused = obj.bitStringContents.charCodeAt(0);
                if (unused !== 0) {
                  throw new Error(
                    "captureBitStringValue only supported for zero unused bits"
                  );
                }
                capture[v.captureBitStringValue] = obj.bitStringContents.slice(1);
              }
            }
          }
        } else if (errors) {
          errors.push(
            "[" + v.name + '] Expected constructed "' + v.constructed + '", got "' + obj.constructed + '"'
          );
        }
      } else if (errors) {
        if (obj.tagClass !== v.tagClass) {
          errors.push(
            "[" + v.name + '] Expected tag class "' + v.tagClass + '", got "' + obj.tagClass + '"'
          );
        }
        if (obj.type !== v.type) {
          errors.push(
            "[" + v.name + '] Expected type "' + v.type + '", got "' + obj.type + '"'
          );
        }
      }
      return rval;
    };
    var _nonLatinRegex = /[^\\u0000-\\u00ff]/;
    asn1.prettyPrint = function(obj, level, indentation) {
      var rval = "";
      level = level || 0;
      indentation = indentation || 2;
      if (level > 0) {
        rval += "\n";
      }
      var indent = "";
      for (var i = 0; i < level * indentation; ++i) {
        indent += " ";
      }
      rval += indent + "Tag: ";
      switch (obj.tagClass) {
        case asn1.Class.UNIVERSAL:
          rval += "Universal:";
          break;
        case asn1.Class.APPLICATION:
          rval += "Application:";
          break;
        case asn1.Class.CONTEXT_SPECIFIC:
          rval += "Context-Specific:";
          break;
        case asn1.Class.PRIVATE:
          rval += "Private:";
          break;
      }
      if (obj.tagClass === asn1.Class.UNIVERSAL) {
        rval += obj.type;
        switch (obj.type) {
          case asn1.Type.NONE:
            rval += " (None)";
            break;
          case asn1.Type.BOOLEAN:
            rval += " (Boolean)";
            break;
          case asn1.Type.INTEGER:
            rval += " (Integer)";
            break;
          case asn1.Type.BITSTRING:
            rval += " (Bit string)";
            break;
          case asn1.Type.OCTETSTRING:
            rval += " (Octet string)";
            break;
          case asn1.Type.NULL:
            rval += " (Null)";
            break;
          case asn1.Type.OID:
            rval += " (Object Identifier)";
            break;
          case asn1.Type.ODESC:
            rval += " (Object Descriptor)";
            break;
          case asn1.Type.EXTERNAL:
            rval += " (External or Instance of)";
            break;
          case asn1.Type.REAL:
            rval += " (Real)";
            break;
          case asn1.Type.ENUMERATED:
            rval += " (Enumerated)";
            break;
          case asn1.Type.EMBEDDED:
            rval += " (Embedded PDV)";
            break;
          case asn1.Type.UTF8:
            rval += " (UTF8)";
            break;
          case asn1.Type.ROID:
            rval += " (Relative Object Identifier)";
            break;
          case asn1.Type.SEQUENCE:
            rval += " (Sequence)";
            break;
          case asn1.Type.SET:
            rval += " (Set)";
            break;
          case asn1.Type.PRINTABLESTRING:
            rval += " (Printable String)";
            break;
          case asn1.Type.IA5String:
            rval += " (IA5String (ASCII))";
            break;
          case asn1.Type.UTCTIME:
            rval += " (UTC time)";
            break;
          case asn1.Type.GENERALIZEDTIME:
            rval += " (Generalized time)";
            break;
          case asn1.Type.BMPSTRING:
            rval += " (BMP String)";
            break;
        }
      } else {
        rval += obj.type;
      }
      rval += "\n";
      rval += indent + "Constructed: " + obj.constructed + "\n";
      if (obj.composed) {
        var subvalues = 0;
        var sub = "";
        for (var i = 0; i < obj.value.length; ++i) {
          if (obj.value[i] !== void 0) {
            subvalues += 1;
            sub += asn1.prettyPrint(obj.value[i], level + 1, indentation);
            if (i + 1 < obj.value.length) {
              sub += ",";
            }
          }
        }
        rval += indent + "Sub values: " + subvalues + sub;
      } else {
        rval += indent + "Value: ";
        if (obj.type === asn1.Type.OID) {
          var oid = asn1.derToOid(obj.value);
          rval += oid;
          if (forge6.pki && forge6.pki.oids) {
            if (oid in forge6.pki.oids) {
              rval += " (" + forge6.pki.oids[oid] + ") ";
            }
          }
        }
        if (obj.type === asn1.Type.INTEGER) {
          try {
            rval += asn1.derToInteger(obj.value);
          } catch (ex) {
            rval += "0x" + forge6.util.bytesToHex(obj.value);
          }
        } else if (obj.type === asn1.Type.BITSTRING) {
          if (obj.value.length > 1) {
            rval += "0x" + forge6.util.bytesToHex(obj.value.slice(1));
          } else {
            rval += "(none)";
          }
          if (obj.value.length > 0) {
            var unused = obj.value.charCodeAt(0);
            if (unused == 1) {
              rval += " (1 unused bit shown)";
            } else if (unused > 1) {
              rval += " (" + unused + " unused bits shown)";
            }
          }
        } else if (obj.type === asn1.Type.OCTETSTRING) {
          if (!_nonLatinRegex.test(obj.value)) {
            rval += "(" + obj.value + ") ";
          }
          rval += "0x" + forge6.util.bytesToHex(obj.value);
        } else if (obj.type === asn1.Type.UTF8) {
          try {
            rval += forge6.util.decodeUtf8(obj.value);
          } catch (e) {
            if (e.message === "URI malformed") {
              rval += "0x" + forge6.util.bytesToHex(obj.value) + " (malformed UTF8)";
            } else {
              throw e;
            }
          }
        } else if (obj.type === asn1.Type.PRINTABLESTRING || obj.type === asn1.Type.IA5String) {
          rval += obj.value;
        } else if (_nonLatinRegex.test(obj.value)) {
          rval += "0x" + forge6.util.bytesToHex(obj.value);
        } else if (obj.value.length === 0) {
          rval += "[null]";
        } else {
          rval += obj.value;
        }
      }
      return rval;
    };
  }
});

// ../../node_modules/node-forge/lib/cipher.js
var require_cipher = __commonJS({
  "../../node_modules/node-forge/lib/cipher.js"(exports2, module2) {
    var forge6 = require_forge();
    require_util();
    module2.exports = forge6.cipher = forge6.cipher || {};
    forge6.cipher.algorithms = forge6.cipher.algorithms || {};
    forge6.cipher.createCipher = function(algorithm, key) {
      var api = algorithm;
      if (typeof api === "string") {
        api = forge6.cipher.getAlgorithm(api);
        if (api) {
          api = api();
        }
      }
      if (!api) {
        throw new Error("Unsupported algorithm: " + algorithm);
      }
      return new forge6.cipher.BlockCipher({
        algorithm: api,
        key,
        decrypt: false
      });
    };
    forge6.cipher.createDecipher = function(algorithm, key) {
      var api = algorithm;
      if (typeof api === "string") {
        api = forge6.cipher.getAlgorithm(api);
        if (api) {
          api = api();
        }
      }
      if (!api) {
        throw new Error("Unsupported algorithm: " + algorithm);
      }
      return new forge6.cipher.BlockCipher({
        algorithm: api,
        key,
        decrypt: true
      });
    };
    forge6.cipher.registerAlgorithm = function(name3, algorithm) {
      name3 = name3.toUpperCase();
      forge6.cipher.algorithms[name3] = algorithm;
    };
    forge6.cipher.getAlgorithm = function(name3) {
      name3 = name3.toUpperCase();
      if (name3 in forge6.cipher.algorithms) {
        return forge6.cipher.algorithms[name3];
      }
      return null;
    };
    var BlockCipher = forge6.cipher.BlockCipher = function(options) {
      this.algorithm = options.algorithm;
      this.mode = this.algorithm.mode;
      this.blockSize = this.mode.blockSize;
      this._finish = false;
      this._input = null;
      this.output = null;
      this._op = options.decrypt ? this.mode.decrypt : this.mode.encrypt;
      this._decrypt = options.decrypt;
      this.algorithm.initialize(options);
    };
    BlockCipher.prototype.start = function(options) {
      options = options || {};
      var opts = {};
      for (var key in options) {
        opts[key] = options[key];
      }
      opts.decrypt = this._decrypt;
      this._finish = false;
      this._input = forge6.util.createBuffer();
      this.output = options.output || forge6.util.createBuffer();
      this.mode.start(opts);
    };
    BlockCipher.prototype.update = function(input) {
      if (input) {
        this._input.putBuffer(input);
      }
      while (!this._op.call(this.mode, this._input, this.output, this._finish) && !this._finish) {
      }
      this._input.compact();
    };
    BlockCipher.prototype.finish = function(pad) {
      if (pad && (this.mode.name === "ECB" || this.mode.name === "CBC")) {
        this.mode.pad = function(input) {
          return pad(this.blockSize, input, false);
        };
        this.mode.unpad = function(output) {
          return pad(this.blockSize, output, true);
        };
      }
      var options = {};
      options.decrypt = this._decrypt;
      options.overflow = this._input.length() % this.blockSize;
      if (!this._decrypt && this.mode.pad) {
        if (!this.mode.pad(this._input, options)) {
          return false;
        }
      }
      this._finish = true;
      this.update();
      if (this._decrypt && this.mode.unpad) {
        if (!this.mode.unpad(this.output, options)) {
          return false;
        }
      }
      if (this.mode.afterFinish) {
        if (!this.mode.afterFinish(this.output, options)) {
          return false;
        }
      }
      return true;
    };
  }
});

// ../../node_modules/node-forge/lib/cipherModes.js
var require_cipherModes = __commonJS({
  "../../node_modules/node-forge/lib/cipherModes.js"(exports2, module2) {
    var forge6 = require_forge();
    require_util();
    forge6.cipher = forge6.cipher || {};
    var modes = module2.exports = forge6.cipher.modes = forge6.cipher.modes || {};
    modes.ecb = function(options) {
      options = options || {};
      this.name = "ECB";
      this.cipher = options.cipher;
      this.blockSize = options.blockSize || 16;
      this._ints = this.blockSize / 4;
      this._inBlock = new Array(this._ints);
      this._outBlock = new Array(this._ints);
    };
    modes.ecb.prototype.start = function(options) {
    };
    modes.ecb.prototype.encrypt = function(input, output, finish) {
      if (input.length() < this.blockSize && !(finish && input.length() > 0)) {
        return true;
      }
      for (var i = 0; i < this._ints; ++i) {
        this._inBlock[i] = input.getInt32();
      }
      this.cipher.encrypt(this._inBlock, this._outBlock);
      for (var i = 0; i < this._ints; ++i) {
        output.putInt32(this._outBlock[i]);
      }
    };
    modes.ecb.prototype.decrypt = function(input, output, finish) {
      if (input.length() < this.blockSize && !(finish && input.length() > 0)) {
        return true;
      }
      for (var i = 0; i < this._ints; ++i) {
        this._inBlock[i] = input.getInt32();
      }
      this.cipher.decrypt(this._inBlock, this._outBlock);
      for (var i = 0; i < this._ints; ++i) {
        output.putInt32(this._outBlock[i]);
      }
    };
    modes.ecb.prototype.pad = function(input, options) {
      var padding = input.length() === this.blockSize ? this.blockSize : this.blockSize - input.length();
      input.fillWithByte(padding, padding);
      return true;
    };
    modes.ecb.prototype.unpad = function(output, options) {
      if (options.overflow > 0) {
        return false;
      }
      var len = output.length();
      var count = output.at(len - 1);
      if (count > this.blockSize << 2) {
        return false;
      }
      output.truncate(count);
      return true;
    };
    modes.cbc = function(options) {
      options = options || {};
      this.name = "CBC";
      this.cipher = options.cipher;
      this.blockSize = options.blockSize || 16;
      this._ints = this.blockSize / 4;
      this._inBlock = new Array(this._ints);
      this._outBlock = new Array(this._ints);
    };
    modes.cbc.prototype.start = function(options) {
      if (options.iv === null) {
        if (!this._prev) {
          throw new Error("Invalid IV parameter.");
        }
        this._iv = this._prev.slice(0);
      } else if (!("iv" in options)) {
        throw new Error("Invalid IV parameter.");
      } else {
        this._iv = transformIV(options.iv, this.blockSize);
        this._prev = this._iv.slice(0);
      }
    };
    modes.cbc.prototype.encrypt = function(input, output, finish) {
      if (input.length() < this.blockSize && !(finish && input.length() > 0)) {
        return true;
      }
      for (var i = 0; i < this._ints; ++i) {
        this._inBlock[i] = this._prev[i] ^ input.getInt32();
      }
      this.cipher.encrypt(this._inBlock, this._outBlock);
      for (var i = 0; i < this._ints; ++i) {
        output.putInt32(this._outBlock[i]);
      }
      this._prev = this._outBlock;
    };
    modes.cbc.prototype.decrypt = function(input, output, finish) {
      if (input.length() < this.blockSize && !(finish && input.length() > 0)) {
        return true;
      }
      for (var i = 0; i < this._ints; ++i) {
        this._inBlock[i] = input.getInt32();
      }
      this.cipher.decrypt(this._inBlock, this._outBlock);
      for (var i = 0; i < this._ints; ++i) {
        output.putInt32(this._prev[i] ^ this._outBlock[i]);
      }
      this._prev = this._inBlock.slice(0);
    };
    modes.cbc.prototype.pad = function(input, options) {
      var padding = input.length() === this.blockSize ? this.blockSize : this.blockSize - input.length();
      input.fillWithByte(padding, padding);
      return true;
    };
    modes.cbc.prototype.unpad = function(output, options) {
      if (options.overflow > 0) {
        return false;
      }
      var len = output.length();
      var count = output.at(len - 1);
      if (count > this.blockSize << 2) {
        return false;
      }
      output.truncate(count);
      return true;
    };
    modes.cfb = function(options) {
      options = options || {};
      this.name = "CFB";
      this.cipher = options.cipher;
      this.blockSize = options.blockSize || 16;
      this._ints = this.blockSize / 4;
      this._inBlock = null;
      this._outBlock = new Array(this._ints);
      this._partialBlock = new Array(this._ints);
      this._partialOutput = forge6.util.createBuffer();
      this._partialBytes = 0;
    };
    modes.cfb.prototype.start = function(options) {
      if (!("iv" in options)) {
        throw new Error("Invalid IV parameter.");
      }
      this._iv = transformIV(options.iv, this.blockSize);
      this._inBlock = this._iv.slice(0);
      this._partialBytes = 0;
    };
    modes.cfb.prototype.encrypt = function(input, output, finish) {
      var inputLength = input.length();
      if (inputLength === 0) {
        return true;
      }
      this.cipher.encrypt(this._inBlock, this._outBlock);
      if (this._partialBytes === 0 && inputLength >= this.blockSize) {
        for (var i = 0; i < this._ints; ++i) {
          this._inBlock[i] = input.getInt32() ^ this._outBlock[i];
          output.putInt32(this._inBlock[i]);
        }
        return;
      }
      var partialBytes = (this.blockSize - inputLength) % this.blockSize;
      if (partialBytes > 0) {
        partialBytes = this.blockSize - partialBytes;
      }
      this._partialOutput.clear();
      for (var i = 0; i < this._ints; ++i) {
        this._partialBlock[i] = input.getInt32() ^ this._outBlock[i];
        this._partialOutput.putInt32(this._partialBlock[i]);
      }
      if (partialBytes > 0) {
        input.read -= this.blockSize;
      } else {
        for (var i = 0; i < this._ints; ++i) {
          this._inBlock[i] = this._partialBlock[i];
        }
      }
      if (this._partialBytes > 0) {
        this._partialOutput.getBytes(this._partialBytes);
      }
      if (partialBytes > 0 && !finish) {
        output.putBytes(this._partialOutput.getBytes(
          partialBytes - this._partialBytes
        ));
        this._partialBytes = partialBytes;
        return true;
      }
      output.putBytes(this._partialOutput.getBytes(
        inputLength - this._partialBytes
      ));
      this._partialBytes = 0;
    };
    modes.cfb.prototype.decrypt = function(input, output, finish) {
      var inputLength = input.length();
      if (inputLength === 0) {
        return true;
      }
      this.cipher.encrypt(this._inBlock, this._outBlock);
      if (this._partialBytes === 0 && inputLength >= this.blockSize) {
        for (var i = 0; i < this._ints; ++i) {
          this._inBlock[i] = input.getInt32();
          output.putInt32(this._inBlock[i] ^ this._outBlock[i]);
        }
        return;
      }
      var partialBytes = (this.blockSize - inputLength) % this.blockSize;
      if (partialBytes > 0) {
        partialBytes = this.blockSize - partialBytes;
      }
      this._partialOutput.clear();
      for (var i = 0; i < this._ints; ++i) {
        this._partialBlock[i] = input.getInt32();
        this._partialOutput.putInt32(this._partialBlock[i] ^ this._outBlock[i]);
      }
      if (partialBytes > 0) {
        input.read -= this.blockSize;
      } else {
        for (var i = 0; i < this._ints; ++i) {
          this._inBlock[i] = this._partialBlock[i];
        }
      }
      if (this._partialBytes > 0) {
        this._partialOutput.getBytes(this._partialBytes);
      }
      if (partialBytes > 0 && !finish) {
        output.putBytes(this._partialOutput.getBytes(
          partialBytes - this._partialBytes
        ));
        this._partialBytes = partialBytes;
        return true;
      }
      output.putBytes(this._partialOutput.getBytes(
        inputLength - this._partialBytes
      ));
      this._partialBytes = 0;
    };
    modes.ofb = function(options) {
      options = options || {};
      this.name = "OFB";
      this.cipher = options.cipher;
      this.blockSize = options.blockSize || 16;
      this._ints = this.blockSize / 4;
      this._inBlock = null;
      this._outBlock = new Array(this._ints);
      this._partialOutput = forge6.util.createBuffer();
      this._partialBytes = 0;
    };
    modes.ofb.prototype.start = function(options) {
      if (!("iv" in options)) {
        throw new Error("Invalid IV parameter.");
      }
      this._iv = transformIV(options.iv, this.blockSize);
      this._inBlock = this._iv.slice(0);
      this._partialBytes = 0;
    };
    modes.ofb.prototype.encrypt = function(input, output, finish) {
      var inputLength = input.length();
      if (input.length() === 0) {
        return true;
      }
      this.cipher.encrypt(this._inBlock, this._outBlock);
      if (this._partialBytes === 0 && inputLength >= this.blockSize) {
        for (var i = 0; i < this._ints; ++i) {
          output.putInt32(input.getInt32() ^ this._outBlock[i]);
          this._inBlock[i] = this._outBlock[i];
        }
        return;
      }
      var partialBytes = (this.blockSize - inputLength) % this.blockSize;
      if (partialBytes > 0) {
        partialBytes = this.blockSize - partialBytes;
      }
      this._partialOutput.clear();
      for (var i = 0; i < this._ints; ++i) {
        this._partialOutput.putInt32(input.getInt32() ^ this._outBlock[i]);
      }
      if (partialBytes > 0) {
        input.read -= this.blockSize;
      } else {
        for (var i = 0; i < this._ints; ++i) {
          this._inBlock[i] = this._outBlock[i];
        }
      }
      if (this._partialBytes > 0) {
        this._partialOutput.getBytes(this._partialBytes);
      }
      if (partialBytes > 0 && !finish) {
        output.putBytes(this._partialOutput.getBytes(
          partialBytes - this._partialBytes
        ));
        this._partialBytes = partialBytes;
        return true;
      }
      output.putBytes(this._partialOutput.getBytes(
        inputLength - this._partialBytes
      ));
      this._partialBytes = 0;
    };
    modes.ofb.prototype.decrypt = modes.ofb.prototype.encrypt;
    modes.ctr = function(options) {
      options = options || {};
      this.name = "CTR";
      this.cipher = options.cipher;
      this.blockSize = options.blockSize || 16;
      this._ints = this.blockSize / 4;
      this._inBlock = null;
      this._outBlock = new Array(this._ints);
      this._partialOutput = forge6.util.createBuffer();
      this._partialBytes = 0;
    };
    modes.ctr.prototype.start = function(options) {
      if (!("iv" in options)) {
        throw new Error("Invalid IV parameter.");
      }
      this._iv = transformIV(options.iv, this.blockSize);
      this._inBlock = this._iv.slice(0);
      this._partialBytes = 0;
    };
    modes.ctr.prototype.encrypt = function(input, output, finish) {
      var inputLength = input.length();
      if (inputLength === 0) {
        return true;
      }
      this.cipher.encrypt(this._inBlock, this._outBlock);
      if (this._partialBytes === 0 && inputLength >= this.blockSize) {
        for (var i = 0; i < this._ints; ++i) {
          output.putInt32(input.getInt32() ^ this._outBlock[i]);
        }
      } else {
        var partialBytes = (this.blockSize - inputLength) % this.blockSize;
        if (partialBytes > 0) {
          partialBytes = this.blockSize - partialBytes;
        }
        this._partialOutput.clear();
        for (var i = 0; i < this._ints; ++i) {
          this._partialOutput.putInt32(input.getInt32() ^ this._outBlock[i]);
        }
        if (partialBytes > 0) {
          input.read -= this.blockSize;
        }
        if (this._partialBytes > 0) {
          this._partialOutput.getBytes(this._partialBytes);
        }
        if (partialBytes > 0 && !finish) {
          output.putBytes(this._partialOutput.getBytes(
            partialBytes - this._partialBytes
          ));
          this._partialBytes = partialBytes;
          return true;
        }
        output.putBytes(this._partialOutput.getBytes(
          inputLength - this._partialBytes
        ));
        this._partialBytes = 0;
      }
      inc32(this._inBlock);
    };
    modes.ctr.prototype.decrypt = modes.ctr.prototype.encrypt;
    modes.gcm = function(options) {
      options = options || {};
      this.name = "GCM";
      this.cipher = options.cipher;
      this.blockSize = options.blockSize || 16;
      this._ints = this.blockSize / 4;
      this._inBlock = new Array(this._ints);
      this._outBlock = new Array(this._ints);
      this._partialOutput = forge6.util.createBuffer();
      this._partialBytes = 0;
      this._R = 3774873600;
    };
    modes.gcm.prototype.start = function(options) {
      if (!("iv" in options)) {
        throw new Error("Invalid IV parameter.");
      }
      var iv = forge6.util.createBuffer(options.iv);
      this._cipherLength = 0;
      var additionalData;
      if ("additionalData" in options) {
        additionalData = forge6.util.createBuffer(options.additionalData);
      } else {
        additionalData = forge6.util.createBuffer();
      }
      if ("tagLength" in options) {
        this._tagLength = options.tagLength;
      } else {
        this._tagLength = 128;
      }
      this._tag = null;
      if (options.decrypt) {
        this._tag = forge6.util.createBuffer(options.tag).getBytes();
        if (this._tag.length !== this._tagLength / 8) {
          throw new Error("Authentication tag does not match tag length.");
        }
      }
      this._hashBlock = new Array(this._ints);
      this.tag = null;
      this._hashSubkey = new Array(this._ints);
      this.cipher.encrypt([0, 0, 0, 0], this._hashSubkey);
      this.componentBits = 4;
      this._m = this.generateHashTable(this._hashSubkey, this.componentBits);
      var ivLength = iv.length();
      if (ivLength === 12) {
        this._j0 = [iv.getInt32(), iv.getInt32(), iv.getInt32(), 1];
      } else {
        this._j0 = [0, 0, 0, 0];
        while (iv.length() > 0) {
          this._j0 = this.ghash(
            this._hashSubkey,
            this._j0,
            [iv.getInt32(), iv.getInt32(), iv.getInt32(), iv.getInt32()]
          );
        }
        this._j0 = this.ghash(
          this._hashSubkey,
          this._j0,
          [0, 0].concat(from64To32(ivLength * 8))
        );
      }
      this._inBlock = this._j0.slice(0);
      inc32(this._inBlock);
      this._partialBytes = 0;
      additionalData = forge6.util.createBuffer(additionalData);
      this._aDataLength = from64To32(additionalData.length() * 8);
      var overflow = additionalData.length() % this.blockSize;
      if (overflow) {
        additionalData.fillWithByte(0, this.blockSize - overflow);
      }
      this._s = [0, 0, 0, 0];
      while (additionalData.length() > 0) {
        this._s = this.ghash(this._hashSubkey, this._s, [
          additionalData.getInt32(),
          additionalData.getInt32(),
          additionalData.getInt32(),
          additionalData.getInt32()
        ]);
      }
    };
    modes.gcm.prototype.encrypt = function(input, output, finish) {
      var inputLength = input.length();
      if (inputLength === 0) {
        return true;
      }
      this.cipher.encrypt(this._inBlock, this._outBlock);
      if (this._partialBytes === 0 && inputLength >= this.blockSize) {
        for (var i = 0; i < this._ints; ++i) {
          output.putInt32(this._outBlock[i] ^= input.getInt32());
        }
        this._cipherLength += this.blockSize;
      } else {
        var partialBytes = (this.blockSize - inputLength) % this.blockSize;
        if (partialBytes > 0) {
          partialBytes = this.blockSize - partialBytes;
        }
        this._partialOutput.clear();
        for (var i = 0; i < this._ints; ++i) {
          this._partialOutput.putInt32(input.getInt32() ^ this._outBlock[i]);
        }
        if (partialBytes <= 0 || finish) {
          if (finish) {
            var overflow = inputLength % this.blockSize;
            this._cipherLength += overflow;
            this._partialOutput.truncate(this.blockSize - overflow);
          } else {
            this._cipherLength += this.blockSize;
          }
          for (var i = 0; i < this._ints; ++i) {
            this._outBlock[i] = this._partialOutput.getInt32();
          }
          this._partialOutput.read -= this.blockSize;
        }
        if (this._partialBytes > 0) {
          this._partialOutput.getBytes(this._partialBytes);
        }
        if (partialBytes > 0 && !finish) {
          input.read -= this.blockSize;
          output.putBytes(this._partialOutput.getBytes(
            partialBytes - this._partialBytes
          ));
          this._partialBytes = partialBytes;
          return true;
        }
        output.putBytes(this._partialOutput.getBytes(
          inputLength - this._partialBytes
        ));
        this._partialBytes = 0;
      }
      this._s = this.ghash(this._hashSubkey, this._s, this._outBlock);
      inc32(this._inBlock);
    };
    modes.gcm.prototype.decrypt = function(input, output, finish) {
      var inputLength = input.length();
      if (inputLength < this.blockSize && !(finish && inputLength > 0)) {
        return true;
      }
      this.cipher.encrypt(this._inBlock, this._outBlock);
      inc32(this._inBlock);
      this._hashBlock[0] = input.getInt32();
      this._hashBlock[1] = input.getInt32();
      this._hashBlock[2] = input.getInt32();
      this._hashBlock[3] = input.getInt32();
      this._s = this.ghash(this._hashSubkey, this._s, this._hashBlock);
      for (var i = 0; i < this._ints; ++i) {
        output.putInt32(this._outBlock[i] ^ this._hashBlock[i]);
      }
      if (inputLength < this.blockSize) {
        this._cipherLength += inputLength % this.blockSize;
      } else {
        this._cipherLength += this.blockSize;
      }
    };
    modes.gcm.prototype.afterFinish = function(output, options) {
      var rval = true;
      if (options.decrypt && options.overflow) {
        output.truncate(this.blockSize - options.overflow);
      }
      this.tag = forge6.util.createBuffer();
      var lengths = this._aDataLength.concat(from64To32(this._cipherLength * 8));
      this._s = this.ghash(this._hashSubkey, this._s, lengths);
      var tag = [];
      this.cipher.encrypt(this._j0, tag);
      for (var i = 0; i < this._ints; ++i) {
        this.tag.putInt32(this._s[i] ^ tag[i]);
      }
      this.tag.truncate(this.tag.length() % (this._tagLength / 8));
      if (options.decrypt && this.tag.bytes() !== this._tag) {
        rval = false;
      }
      return rval;
    };
    modes.gcm.prototype.multiply = function(x, y) {
      var z_i = [0, 0, 0, 0];
      var v_i = y.slice(0);
      for (var i = 0; i < 128; ++i) {
        var x_i = x[i / 32 | 0] & 1 << 31 - i % 32;
        if (x_i) {
          z_i[0] ^= v_i[0];
          z_i[1] ^= v_i[1];
          z_i[2] ^= v_i[2];
          z_i[3] ^= v_i[3];
        }
        this.pow(v_i, v_i);
      }
      return z_i;
    };
    modes.gcm.prototype.pow = function(x, out) {
      var lsb = x[3] & 1;
      for (var i = 3; i > 0; --i) {
        out[i] = x[i] >>> 1 | (x[i - 1] & 1) << 31;
      }
      out[0] = x[0] >>> 1;
      if (lsb) {
        out[0] ^= this._R;
      }
    };
    modes.gcm.prototype.tableMultiply = function(x) {
      var z = [0, 0, 0, 0];
      for (var i = 0; i < 32; ++i) {
        var idx = i / 8 | 0;
        var x_i = x[idx] >>> (7 - i % 8) * 4 & 15;
        var ah = this._m[i][x_i];
        z[0] ^= ah[0];
        z[1] ^= ah[1];
        z[2] ^= ah[2];
        z[3] ^= ah[3];
      }
      return z;
    };
    modes.gcm.prototype.ghash = function(h, y, x) {
      y[0] ^= x[0];
      y[1] ^= x[1];
      y[2] ^= x[2];
      y[3] ^= x[3];
      return this.tableMultiply(y);
    };
    modes.gcm.prototype.generateHashTable = function(h, bits2) {
      var multiplier = 8 / bits2;
      var perInt = 4 * multiplier;
      var size = 16 * multiplier;
      var m = new Array(size);
      for (var i = 0; i < size; ++i) {
        var tmp = [0, 0, 0, 0];
        var idx = i / perInt | 0;
        var shft = (perInt - 1 - i % perInt) * bits2;
        tmp[idx] = 1 << bits2 - 1 << shft;
        m[i] = this.generateSubHashTable(this.multiply(tmp, h), bits2);
      }
      return m;
    };
    modes.gcm.prototype.generateSubHashTable = function(mid, bits2) {
      var size = 1 << bits2;
      var half = size >>> 1;
      var m = new Array(size);
      m[half] = mid.slice(0);
      var i = half >>> 1;
      while (i > 0) {
        this.pow(m[2 * i], m[i] = []);
        i >>= 1;
      }
      i = 2;
      while (i < half) {
        for (var j = 1; j < i; ++j) {
          var m_i = m[i];
          var m_j = m[j];
          m[i + j] = [
            m_i[0] ^ m_j[0],
            m_i[1] ^ m_j[1],
            m_i[2] ^ m_j[2],
            m_i[3] ^ m_j[3]
          ];
        }
        i *= 2;
      }
      m[0] = [0, 0, 0, 0];
      for (i = half + 1; i < size; ++i) {
        var c = m[i ^ half];
        m[i] = [mid[0] ^ c[0], mid[1] ^ c[1], mid[2] ^ c[2], mid[3] ^ c[3]];
      }
      return m;
    };
    function transformIV(iv, blockSize) {
      if (typeof iv === "string") {
        iv = forge6.util.createBuffer(iv);
      }
      if (forge6.util.isArray(iv) && iv.length > 4) {
        var tmp = iv;
        iv = forge6.util.createBuffer();
        for (var i = 0; i < tmp.length; ++i) {
          iv.putByte(tmp[i]);
        }
      }
      if (iv.length() < blockSize) {
        throw new Error(
          "Invalid IV length; got " + iv.length() + " bytes and expected " + blockSize + " bytes."
        );
      }
      if (!forge6.util.isArray(iv)) {
        var ints = [];
        var blocks = blockSize / 4;
        for (var i = 0; i < blocks; ++i) {
          ints.push(iv.getInt32());
        }
        iv = ints;
      }
      return iv;
    }
    function inc32(block) {
      block[block.length - 1] = block[block.length - 1] + 1 & 4294967295;
    }
    function from64To32(num) {
      return [num / 4294967296 | 0, num & 4294967295];
    }
  }
});

// ../../node_modules/node-forge/lib/aes.js
var require_aes = __commonJS({
  "../../node_modules/node-forge/lib/aes.js"(exports2, module2) {
    var forge6 = require_forge();
    require_cipher();
    require_cipherModes();
    require_util();
    module2.exports = forge6.aes = forge6.aes || {};
    forge6.aes.startEncrypting = function(key, iv, output, mode) {
      var cipher = _createCipher({
        key,
        output,
        decrypt: false,
        mode
      });
      cipher.start(iv);
      return cipher;
    };
    forge6.aes.createEncryptionCipher = function(key, mode) {
      return _createCipher({
        key,
        output: null,
        decrypt: false,
        mode
      });
    };
    forge6.aes.startDecrypting = function(key, iv, output, mode) {
      var cipher = _createCipher({
        key,
        output,
        decrypt: true,
        mode
      });
      cipher.start(iv);
      return cipher;
    };
    forge6.aes.createDecryptionCipher = function(key, mode) {
      return _createCipher({
        key,
        output: null,
        decrypt: true,
        mode
      });
    };
    forge6.aes.Algorithm = function(name3, mode) {
      if (!init) {
        initialize();
      }
      var self2 = this;
      self2.name = name3;
      self2.mode = new mode({
        blockSize: 16,
        cipher: {
          encrypt: function(inBlock, outBlock) {
            return _updateBlock(self2._w, inBlock, outBlock, false);
          },
          decrypt: function(inBlock, outBlock) {
            return _updateBlock(self2._w, inBlock, outBlock, true);
          }
        }
      });
      self2._init = false;
    };
    forge6.aes.Algorithm.prototype.initialize = function(options) {
      if (this._init) {
        return;
      }
      var key = options.key;
      var tmp;
      if (typeof key === "string" && (key.length === 16 || key.length === 24 || key.length === 32)) {
        key = forge6.util.createBuffer(key);
      } else if (forge6.util.isArray(key) && (key.length === 16 || key.length === 24 || key.length === 32)) {
        tmp = key;
        key = forge6.util.createBuffer();
        for (var i = 0; i < tmp.length; ++i) {
          key.putByte(tmp[i]);
        }
      }
      if (!forge6.util.isArray(key)) {
        tmp = key;
        key = [];
        var len = tmp.length();
        if (len === 16 || len === 24 || len === 32) {
          len = len >>> 2;
          for (var i = 0; i < len; ++i) {
            key.push(tmp.getInt32());
          }
        }
      }
      if (!forge6.util.isArray(key) || !(key.length === 4 || key.length === 6 || key.length === 8)) {
        throw new Error("Invalid key parameter.");
      }
      var mode = this.mode.name;
      var encryptOp = ["CFB", "OFB", "CTR", "GCM"].indexOf(mode) !== -1;
      this._w = _expandKey(key, options.decrypt && !encryptOp);
      this._init = true;
    };
    forge6.aes._expandKey = function(key, decrypt2) {
      if (!init) {
        initialize();
      }
      return _expandKey(key, decrypt2);
    };
    forge6.aes._updateBlock = _updateBlock;
    registerAlgorithm("AES-ECB", forge6.cipher.modes.ecb);
    registerAlgorithm("AES-CBC", forge6.cipher.modes.cbc);
    registerAlgorithm("AES-CFB", forge6.cipher.modes.cfb);
    registerAlgorithm("AES-OFB", forge6.cipher.modes.ofb);
    registerAlgorithm("AES-CTR", forge6.cipher.modes.ctr);
    registerAlgorithm("AES-GCM", forge6.cipher.modes.gcm);
    function registerAlgorithm(name3, mode) {
      var factory = function() {
        return new forge6.aes.Algorithm(name3, mode);
      };
      forge6.cipher.registerAlgorithm(name3, factory);
    }
    var init = false;
    var Nb = 4;
    var sbox;
    var isbox;
    var rcon;
    var mix;
    var imix;
    function initialize() {
      init = true;
      rcon = [0, 1, 2, 4, 8, 16, 32, 64, 128, 27, 54];
      var xtime = new Array(256);
      for (var i = 0; i < 128; ++i) {
        xtime[i] = i << 1;
        xtime[i + 128] = i + 128 << 1 ^ 283;
      }
      sbox = new Array(256);
      isbox = new Array(256);
      mix = new Array(4);
      imix = new Array(4);
      for (var i = 0; i < 4; ++i) {
        mix[i] = new Array(256);
        imix[i] = new Array(256);
      }
      var e = 0, ei = 0, e2, e4, e8, sx, sx2, me, ime;
      for (var i = 0; i < 256; ++i) {
        sx = ei ^ ei << 1 ^ ei << 2 ^ ei << 3 ^ ei << 4;
        sx = sx >> 8 ^ sx & 255 ^ 99;
        sbox[e] = sx;
        isbox[sx] = e;
        sx2 = xtime[sx];
        e2 = xtime[e];
        e4 = xtime[e2];
        e8 = xtime[e4];
        me = sx2 << 24 ^ // 2
        sx << 16 ^ // 1
        sx << 8 ^ // 1
        (sx ^ sx2);
        ime = (e2 ^ e4 ^ e8) << 24 ^ // E (14)
        (e ^ e8) << 16 ^ // 9
        (e ^ e4 ^ e8) << 8 ^ // D (13)
        (e ^ e2 ^ e8);
        for (var n = 0; n < 4; ++n) {
          mix[n][e] = me;
          imix[n][sx] = ime;
          me = me << 24 | me >>> 8;
          ime = ime << 24 | ime >>> 8;
        }
        if (e === 0) {
          e = ei = 1;
        } else {
          e = e2 ^ xtime[xtime[xtime[e2 ^ e8]]];
          ei ^= xtime[xtime[ei]];
        }
      }
    }
    function _expandKey(key, decrypt2) {
      var w = key.slice(0);
      var temp, iNk = 1;
      var Nk = w.length;
      var Nr1 = Nk + 6 + 1;
      var end = Nb * Nr1;
      for (var i = Nk; i < end; ++i) {
        temp = w[i - 1];
        if (i % Nk === 0) {
          temp = sbox[temp >>> 16 & 255] << 24 ^ sbox[temp >>> 8 & 255] << 16 ^ sbox[temp & 255] << 8 ^ sbox[temp >>> 24] ^ rcon[iNk] << 24;
          iNk++;
        } else if (Nk > 6 && i % Nk === 4) {
          temp = sbox[temp >>> 24] << 24 ^ sbox[temp >>> 16 & 255] << 16 ^ sbox[temp >>> 8 & 255] << 8 ^ sbox[temp & 255];
        }
        w[i] = w[i - Nk] ^ temp;
      }
      if (decrypt2) {
        var tmp;
        var m0 = imix[0];
        var m1 = imix[1];
        var m2 = imix[2];
        var m3 = imix[3];
        var wnew = w.slice(0);
        end = w.length;
        for (var i = 0, wi = end - Nb; i < end; i += Nb, wi -= Nb) {
          if (i === 0 || i === end - Nb) {
            wnew[i] = w[wi];
            wnew[i + 1] = w[wi + 3];
            wnew[i + 2] = w[wi + 2];
            wnew[i + 3] = w[wi + 1];
          } else {
            for (var n = 0; n < Nb; ++n) {
              tmp = w[wi + n];
              wnew[i + (3 & -n)] = m0[sbox[tmp >>> 24]] ^ m1[sbox[tmp >>> 16 & 255]] ^ m2[sbox[tmp >>> 8 & 255]] ^ m3[sbox[tmp & 255]];
            }
          }
        }
        w = wnew;
      }
      return w;
    }
    function _updateBlock(w, input, output, decrypt2) {
      var Nr = w.length / 4 - 1;
      var m0, m1, m2, m3, sub;
      if (decrypt2) {
        m0 = imix[0];
        m1 = imix[1];
        m2 = imix[2];
        m3 = imix[3];
        sub = isbox;
      } else {
        m0 = mix[0];
        m1 = mix[1];
        m2 = mix[2];
        m3 = mix[3];
        sub = sbox;
      }
      var a, b, c, d, a2, b2, c2;
      a = input[0] ^ w[0];
      b = input[decrypt2 ? 3 : 1] ^ w[1];
      c = input[2] ^ w[2];
      d = input[decrypt2 ? 1 : 3] ^ w[3];
      var i = 3;
      for (var round = 1; round < Nr; ++round) {
        a2 = m0[a >>> 24] ^ m1[b >>> 16 & 255] ^ m2[c >>> 8 & 255] ^ m3[d & 255] ^ w[++i];
        b2 = m0[b >>> 24] ^ m1[c >>> 16 & 255] ^ m2[d >>> 8 & 255] ^ m3[a & 255] ^ w[++i];
        c2 = m0[c >>> 24] ^ m1[d >>> 16 & 255] ^ m2[a >>> 8 & 255] ^ m3[b & 255] ^ w[++i];
        d = m0[d >>> 24] ^ m1[a >>> 16 & 255] ^ m2[b >>> 8 & 255] ^ m3[c & 255] ^ w[++i];
        a = a2;
        b = b2;
        c = c2;
      }
      output[0] = sub[a >>> 24] << 24 ^ sub[b >>> 16 & 255] << 16 ^ sub[c >>> 8 & 255] << 8 ^ sub[d & 255] ^ w[++i];
      output[decrypt2 ? 3 : 1] = sub[b >>> 24] << 24 ^ sub[c >>> 16 & 255] << 16 ^ sub[d >>> 8 & 255] << 8 ^ sub[a & 255] ^ w[++i];
      output[2] = sub[c >>> 24] << 24 ^ sub[d >>> 16 & 255] << 16 ^ sub[a >>> 8 & 255] << 8 ^ sub[b & 255] ^ w[++i];
      output[decrypt2 ? 1 : 3] = sub[d >>> 24] << 24 ^ sub[a >>> 16 & 255] << 16 ^ sub[b >>> 8 & 255] << 8 ^ sub[c & 255] ^ w[++i];
    }
    function _createCipher(options) {
      options = options || {};
      var mode = (options.mode || "CBC").toUpperCase();
      var algorithm = "AES-" + mode;
      var cipher;
      if (options.decrypt) {
        cipher = forge6.cipher.createDecipher(algorithm, options.key);
      } else {
        cipher = forge6.cipher.createCipher(algorithm, options.key);
      }
      var start = cipher.start;
      cipher.start = function(iv, options2) {
        var output = null;
        if (options2 instanceof forge6.util.ByteBuffer) {
          output = options2;
          options2 = {};
        }
        options2 = options2 || {};
        options2.output = output;
        options2.iv = iv;
        start.call(cipher, options2);
      };
      return cipher;
    }
  }
});

// ../../node_modules/node-forge/lib/des.js
var require_des = __commonJS({
  "../../node_modules/node-forge/lib/des.js"(exports2, module2) {
    var forge6 = require_forge();
    require_cipher();
    require_cipherModes();
    require_util();
    module2.exports = forge6.des = forge6.des || {};
    forge6.des.startEncrypting = function(key, iv, output, mode) {
      var cipher = _createCipher({
        key,
        output,
        decrypt: false,
        mode: mode || (iv === null ? "ECB" : "CBC")
      });
      cipher.start(iv);
      return cipher;
    };
    forge6.des.createEncryptionCipher = function(key, mode) {
      return _createCipher({
        key,
        output: null,
        decrypt: false,
        mode
      });
    };
    forge6.des.startDecrypting = function(key, iv, output, mode) {
      var cipher = _createCipher({
        key,
        output,
        decrypt: true,
        mode: mode || (iv === null ? "ECB" : "CBC")
      });
      cipher.start(iv);
      return cipher;
    };
    forge6.des.createDecryptionCipher = function(key, mode) {
      return _createCipher({
        key,
        output: null,
        decrypt: true,
        mode
      });
    };
    forge6.des.Algorithm = function(name3, mode) {
      var self2 = this;
      self2.name = name3;
      self2.mode = new mode({
        blockSize: 8,
        cipher: {
          encrypt: function(inBlock, outBlock) {
            return _updateBlock(self2._keys, inBlock, outBlock, false);
          },
          decrypt: function(inBlock, outBlock) {
            return _updateBlock(self2._keys, inBlock, outBlock, true);
          }
        }
      });
      self2._init = false;
    };
    forge6.des.Algorithm.prototype.initialize = function(options) {
      if (this._init) {
        return;
      }
      var key = forge6.util.createBuffer(options.key);
      if (this.name.indexOf("3DES") === 0) {
        if (key.length() !== 24) {
          throw new Error("Invalid Triple-DES key size: " + key.length() * 8);
        }
      }
      this._keys = _createKeys(key);
      this._init = true;
    };
    registerAlgorithm("DES-ECB", forge6.cipher.modes.ecb);
    registerAlgorithm("DES-CBC", forge6.cipher.modes.cbc);
    registerAlgorithm("DES-CFB", forge6.cipher.modes.cfb);
    registerAlgorithm("DES-OFB", forge6.cipher.modes.ofb);
    registerAlgorithm("DES-CTR", forge6.cipher.modes.ctr);
    registerAlgorithm("3DES-ECB", forge6.cipher.modes.ecb);
    registerAlgorithm("3DES-CBC", forge6.cipher.modes.cbc);
    registerAlgorithm("3DES-CFB", forge6.cipher.modes.cfb);
    registerAlgorithm("3DES-OFB", forge6.cipher.modes.ofb);
    registerAlgorithm("3DES-CTR", forge6.cipher.modes.ctr);
    function registerAlgorithm(name3, mode) {
      var factory = function() {
        return new forge6.des.Algorithm(name3, mode);
      };
      forge6.cipher.registerAlgorithm(name3, factory);
    }
    var spfunction1 = [16843776, 0, 65536, 16843780, 16842756, 66564, 4, 65536, 1024, 16843776, 16843780, 1024, 16778244, 16842756, 16777216, 4, 1028, 16778240, 16778240, 66560, 66560, 16842752, 16842752, 16778244, 65540, 16777220, 16777220, 65540, 0, 1028, 66564, 16777216, 65536, 16843780, 4, 16842752, 16843776, 16777216, 16777216, 1024, 16842756, 65536, 66560, 16777220, 1024, 4, 16778244, 66564, 16843780, 65540, 16842752, 16778244, 16777220, 1028, 66564, 16843776, 1028, 16778240, 16778240, 0, 65540, 66560, 0, 16842756];
    var spfunction2 = [-2146402272, -2147450880, 32768, 1081376, 1048576, 32, -2146435040, -2147450848, -2147483616, -2146402272, -2146402304, -2147483648, -2147450880, 1048576, 32, -2146435040, 1081344, 1048608, -2147450848, 0, -2147483648, 32768, 1081376, -2146435072, 1048608, -2147483616, 0, 1081344, 32800, -2146402304, -2146435072, 32800, 0, 1081376, -2146435040, 1048576, -2147450848, -2146435072, -2146402304, 32768, -2146435072, -2147450880, 32, -2146402272, 1081376, 32, 32768, -2147483648, 32800, -2146402304, 1048576, -2147483616, 1048608, -2147450848, -2147483616, 1048608, 1081344, 0, -2147450880, 32800, -2147483648, -2146435040, -2146402272, 1081344];
    var spfunction3 = [520, 134349312, 0, 134348808, 134218240, 0, 131592, 134218240, 131080, 134217736, 134217736, 131072, 134349320, 131080, 134348800, 520, 134217728, 8, 134349312, 512, 131584, 134348800, 134348808, 131592, 134218248, 131584, 131072, 134218248, 8, 134349320, 512, 134217728, 134349312, 134217728, 131080, 520, 131072, 134349312, 134218240, 0, 512, 131080, 134349320, 134218240, 134217736, 512, 0, 134348808, 134218248, 131072, 134217728, 134349320, 8, 131592, 131584, 134217736, 134348800, 134218248, 520, 134348800, 131592, 8, 134348808, 131584];
    var spfunction4 = [8396801, 8321, 8321, 128, 8396928, 8388737, 8388609, 8193, 0, 8396800, 8396800, 8396929, 129, 0, 8388736, 8388609, 1, 8192, 8388608, 8396801, 128, 8388608, 8193, 8320, 8388737, 1, 8320, 8388736, 8192, 8396928, 8396929, 129, 8388736, 8388609, 8396800, 8396929, 129, 0, 0, 8396800, 8320, 8388736, 8388737, 1, 8396801, 8321, 8321, 128, 8396929, 129, 1, 8192, 8388609, 8193, 8396928, 8388737, 8193, 8320, 8388608, 8396801, 128, 8388608, 8192, 8396928];
    var spfunction5 = [256, 34078976, 34078720, 1107296512, 524288, 256, 1073741824, 34078720, 1074266368, 524288, 33554688, 1074266368, 1107296512, 1107820544, 524544, 1073741824, 33554432, 1074266112, 1074266112, 0, 1073742080, 1107820800, 1107820800, 33554688, 1107820544, 1073742080, 0, 1107296256, 34078976, 33554432, 1107296256, 524544, 524288, 1107296512, 256, 33554432, 1073741824, 34078720, 1107296512, 1074266368, 33554688, 1073741824, 1107820544, 34078976, 1074266368, 256, 33554432, 1107820544, 1107820800, 524544, 1107296256, 1107820800, 34078720, 0, 1074266112, 1107296256, 524544, 33554688, 1073742080, 524288, 0, 1074266112, 34078976, 1073742080];
    var spfunction6 = [536870928, 541065216, 16384, 541081616, 541065216, 16, 541081616, 4194304, 536887296, 4210704, 4194304, 536870928, 4194320, 536887296, 536870912, 16400, 0, 4194320, 536887312, 16384, 4210688, 536887312, 16, 541065232, 541065232, 0, 4210704, 541081600, 16400, 4210688, 541081600, 536870912, 536887296, 16, 541065232, 4210688, 541081616, 4194304, 16400, 536870928, 4194304, 536887296, 536870912, 16400, 536870928, 541081616, 4210688, 541065216, 4210704, 541081600, 0, 541065232, 16, 16384, 541065216, 4210704, 16384, 4194320, 536887312, 0, 541081600, 536870912, 4194320, 536887312];
    var spfunction7 = [2097152, 69206018, 67110914, 0, 2048, 67110914, 2099202, 69208064, 69208066, 2097152, 0, 67108866, 2, 67108864, 69206018, 2050, 67110912, 2099202, 2097154, 67110912, 67108866, 69206016, 69208064, 2097154, 69206016, 2048, 2050, 69208066, 2099200, 2, 67108864, 2099200, 67108864, 2099200, 2097152, 67110914, 67110914, 69206018, 69206018, 2, 2097154, 67108864, 67110912, 2097152, 69208064, 2050, 2099202, 69208064, 2050, 67108866, 69208066, 69206016, 2099200, 0, 2, 69208066, 0, 2099202, 69206016, 2048, 67108866, 67110912, 2048, 2097154];
    var spfunction8 = [268439616, 4096, 262144, 268701760, 268435456, 268439616, 64, 268435456, 262208, 268697600, 268701760, 266240, 268701696, 266304, 4096, 64, 268697600, 268435520, 268439552, 4160, 266240, 262208, 268697664, 268701696, 4160, 0, 0, 268697664, 268435520, 268439552, 266304, 262144, 266304, 262144, 268701696, 4096, 64, 268697664, 4096, 266304, 268439552, 64, 268435520, 268697600, 268697664, 268435456, 262144, 268439616, 0, 268701760, 262208, 268435520, 268697600, 268439552, 268439616, 0, 268701760, 266240, 266240, 4160, 4160, 262208, 268435456, 268701696];
    function _createKeys(key) {
      var pc2bytes0 = [0, 4, 536870912, 536870916, 65536, 65540, 536936448, 536936452, 512, 516, 536871424, 536871428, 66048, 66052, 536936960, 536936964], pc2bytes1 = [0, 1, 1048576, 1048577, 67108864, 67108865, 68157440, 68157441, 256, 257, 1048832, 1048833, 67109120, 67109121, 68157696, 68157697], pc2bytes2 = [0, 8, 2048, 2056, 16777216, 16777224, 16779264, 16779272, 0, 8, 2048, 2056, 16777216, 16777224, 16779264, 16779272], pc2bytes3 = [0, 2097152, 134217728, 136314880, 8192, 2105344, 134225920, 136323072, 131072, 2228224, 134348800, 136445952, 139264, 2236416, 134356992, 136454144], pc2bytes4 = [0, 262144, 16, 262160, 0, 262144, 16, 262160, 4096, 266240, 4112, 266256, 4096, 266240, 4112, 266256], pc2bytes5 = [0, 1024, 32, 1056, 0, 1024, 32, 1056, 33554432, 33555456, 33554464, 33555488, 33554432, 33555456, 33554464, 33555488], pc2bytes6 = [0, 268435456, 524288, 268959744, 2, 268435458, 524290, 268959746, 0, 268435456, 524288, 268959744, 2, 268435458, 524290, 268959746], pc2bytes7 = [0, 65536, 2048, 67584, 536870912, 536936448, 536872960, 536938496, 131072, 196608, 133120, 198656, 537001984, 537067520, 537004032, 537069568], pc2bytes8 = [0, 262144, 0, 262144, 2, 262146, 2, 262146, 33554432, 33816576, 33554432, 33816576, 33554434, 33816578, 33554434, 33816578], pc2bytes9 = [0, 268435456, 8, 268435464, 0, 268435456, 8, 268435464, 1024, 268436480, 1032, 268436488, 1024, 268436480, 1032, 268436488], pc2bytes10 = [0, 32, 0, 32, 1048576, 1048608, 1048576, 1048608, 8192, 8224, 8192, 8224, 1056768, 1056800, 1056768, 1056800], pc2bytes11 = [0, 16777216, 512, 16777728, 2097152, 18874368, 2097664, 18874880, 67108864, 83886080, 67109376, 83886592, 69206016, 85983232, 69206528, 85983744], pc2bytes12 = [0, 4096, 134217728, 134221824, 524288, 528384, 134742016, 134746112, 16, 4112, 134217744, 134221840, 524304, 528400, 134742032, 134746128], pc2bytes13 = [0, 4, 256, 260, 0, 4, 256, 260, 1, 5, 257, 261, 1, 5, 257, 261];
      var iterations = key.length() > 8 ? 3 : 1;
      var keys = [];
      var shifts = [0, 0, 1, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 0];
      var n = 0, tmp;
      for (var j = 0; j < iterations; j++) {
        var left = key.getInt32();
        var right = key.getInt32();
        tmp = (left >>> 4 ^ right) & 252645135;
        right ^= tmp;
        left ^= tmp << 4;
        tmp = (right >>> -16 ^ left) & 65535;
        left ^= tmp;
        right ^= tmp << -16;
        tmp = (left >>> 2 ^ right) & 858993459;
        right ^= tmp;
        left ^= tmp << 2;
        tmp = (right >>> -16 ^ left) & 65535;
        left ^= tmp;
        right ^= tmp << -16;
        tmp = (left >>> 1 ^ right) & 1431655765;
        right ^= tmp;
        left ^= tmp << 1;
        tmp = (right >>> 8 ^ left) & 16711935;
        left ^= tmp;
        right ^= tmp << 8;
        tmp = (left >>> 1 ^ right) & 1431655765;
        right ^= tmp;
        left ^= tmp << 1;
        tmp = left << 8 | right >>> 20 & 240;
        left = right << 24 | right << 8 & 16711680 | right >>> 8 & 65280 | right >>> 24 & 240;
        right = tmp;
        for (var i = 0; i < shifts.length; ++i) {
          if (shifts[i]) {
            left = left << 2 | left >>> 26;
            right = right << 2 | right >>> 26;
          } else {
            left = left << 1 | left >>> 27;
            right = right << 1 | right >>> 27;
          }
          left &= -15;
          right &= -15;
          var lefttmp = pc2bytes0[left >>> 28] | pc2bytes1[left >>> 24 & 15] | pc2bytes2[left >>> 20 & 15] | pc2bytes3[left >>> 16 & 15] | pc2bytes4[left >>> 12 & 15] | pc2bytes5[left >>> 8 & 15] | pc2bytes6[left >>> 4 & 15];
          var righttmp = pc2bytes7[right >>> 28] | pc2bytes8[right >>> 24 & 15] | pc2bytes9[right >>> 20 & 15] | pc2bytes10[right >>> 16 & 15] | pc2bytes11[right >>> 12 & 15] | pc2bytes12[right >>> 8 & 15] | pc2bytes13[right >>> 4 & 15];
          tmp = (righttmp >>> 16 ^ lefttmp) & 65535;
          keys[n++] = lefttmp ^ tmp;
          keys[n++] = righttmp ^ tmp << 16;
        }
      }
      return keys;
    }
    function _updateBlock(keys, input, output, decrypt2) {
      var iterations = keys.length === 32 ? 3 : 9;
      var looping;
      if (iterations === 3) {
        looping = decrypt2 ? [30, -2, -2] : [0, 32, 2];
      } else {
        looping = decrypt2 ? [94, 62, -2, 32, 64, 2, 30, -2, -2] : [0, 32, 2, 62, 30, -2, 64, 96, 2];
      }
      var tmp;
      var left = input[0];
      var right = input[1];
      tmp = (left >>> 4 ^ right) & 252645135;
      right ^= tmp;
      left ^= tmp << 4;
      tmp = (left >>> 16 ^ right) & 65535;
      right ^= tmp;
      left ^= tmp << 16;
      tmp = (right >>> 2 ^ left) & 858993459;
      left ^= tmp;
      right ^= tmp << 2;
      tmp = (right >>> 8 ^ left) & 16711935;
      left ^= tmp;
      right ^= tmp << 8;
      tmp = (left >>> 1 ^ right) & 1431655765;
      right ^= tmp;
      left ^= tmp << 1;
      left = left << 1 | left >>> 31;
      right = right << 1 | right >>> 31;
      for (var j = 0; j < iterations; j += 3) {
        var endloop = looping[j + 1];
        var loopinc = looping[j + 2];
        for (var i = looping[j]; i != endloop; i += loopinc) {
          var right1 = right ^ keys[i];
          var right2 = (right >>> 4 | right << 28) ^ keys[i + 1];
          tmp = left;
          left = right;
          right = tmp ^ (spfunction2[right1 >>> 24 & 63] | spfunction4[right1 >>> 16 & 63] | spfunction6[right1 >>> 8 & 63] | spfunction8[right1 & 63] | spfunction1[right2 >>> 24 & 63] | spfunction3[right2 >>> 16 & 63] | spfunction5[right2 >>> 8 & 63] | spfunction7[right2 & 63]);
        }
        tmp = left;
        left = right;
        right = tmp;
      }
      left = left >>> 1 | left << 31;
      right = right >>> 1 | right << 31;
      tmp = (left >>> 1 ^ right) & 1431655765;
      right ^= tmp;
      left ^= tmp << 1;
      tmp = (right >>> 8 ^ left) & 16711935;
      left ^= tmp;
      right ^= tmp << 8;
      tmp = (right >>> 2 ^ left) & 858993459;
      left ^= tmp;
      right ^= tmp << 2;
      tmp = (left >>> 16 ^ right) & 65535;
      right ^= tmp;
      left ^= tmp << 16;
      tmp = (left >>> 4 ^ right) & 252645135;
      right ^= tmp;
      left ^= tmp << 4;
      output[0] = left;
      output[1] = right;
    }
    function _createCipher(options) {
      options = options || {};
      var mode = (options.mode || "CBC").toUpperCase();
      var algorithm = "DES-" + mode;
      var cipher;
      if (options.decrypt) {
        cipher = forge6.cipher.createDecipher(algorithm, options.key);
      } else {
        cipher = forge6.cipher.createCipher(algorithm, options.key);
      }
      var start = cipher.start;
      cipher.start = function(iv, options2) {
        var output = null;
        if (options2 instanceof forge6.util.ByteBuffer) {
          output = options2;
          options2 = {};
        }
        options2 = options2 || {};
        options2.output = output;
        options2.iv = iv;
        start.call(cipher, options2);
      };
      return cipher;
    }
  }
});

// ../../node_modules/node-forge/lib/md.js
var require_md = __commonJS({
  "../../node_modules/node-forge/lib/md.js"(exports2, module2) {
    var forge6 = require_forge();
    module2.exports = forge6.md = forge6.md || {};
    forge6.md.algorithms = forge6.md.algorithms || {};
  }
});

// ../../node_modules/node-forge/lib/hmac.js
var require_hmac = __commonJS({
  "../../node_modules/node-forge/lib/hmac.js"(exports2, module2) {
    var forge6 = require_forge();
    require_md();
    require_util();
    var hmac = module2.exports = forge6.hmac = forge6.hmac || {};
    hmac.create = function() {
      var _key = null;
      var _md = null;
      var _ipadding = null;
      var _opadding = null;
      var ctx = {};
      ctx.start = function(md, key) {
        if (md !== null) {
          if (typeof md === "string") {
            md = md.toLowerCase();
            if (md in forge6.md.algorithms) {
              _md = forge6.md.algorithms[md].create();
            } else {
              throw new Error('Unknown hash algorithm "' + md + '"');
            }
          } else {
            _md = md;
          }
        }
        if (key === null) {
          key = _key;
        } else {
          if (typeof key === "string") {
            key = forge6.util.createBuffer(key);
          } else if (forge6.util.isArray(key)) {
            var tmp = key;
            key = forge6.util.createBuffer();
            for (var i = 0; i < tmp.length; ++i) {
              key.putByte(tmp[i]);
            }
          }
          var keylen = key.length();
          if (keylen > _md.blockLength) {
            _md.start();
            _md.update(key.bytes());
            key = _md.digest();
          }
          _ipadding = forge6.util.createBuffer();
          _opadding = forge6.util.createBuffer();
          keylen = key.length();
          for (var i = 0; i < keylen; ++i) {
            var tmp = key.at(i);
            _ipadding.putByte(54 ^ tmp);
            _opadding.putByte(92 ^ tmp);
          }
          if (keylen < _md.blockLength) {
            var tmp = _md.blockLength - keylen;
            for (var i = 0; i < tmp; ++i) {
              _ipadding.putByte(54);
              _opadding.putByte(92);
            }
          }
          _key = key;
          _ipadding = _ipadding.bytes();
          _opadding = _opadding.bytes();
        }
        _md.start();
        _md.update(_ipadding);
      };
      ctx.update = function(bytes) {
        _md.update(bytes);
      };
      ctx.getMac = function() {
        var inner = _md.digest().bytes();
        _md.start();
        _md.update(_opadding);
        _md.update(inner);
        return _md.digest();
      };
      ctx.digest = ctx.getMac;
      return ctx;
    };
  }
});

// (disabled):crypto
var require_crypto = __commonJS({
  "(disabled):crypto"() {
  }
});

// ../../node_modules/node-forge/lib/pbkdf2.js
var require_pbkdf2 = __commonJS({
  "../../node_modules/node-forge/lib/pbkdf2.js"(exports2, module2) {
    var forge6 = require_forge();
    require_hmac();
    require_md();
    require_util();
    var pkcs5 = forge6.pkcs5 = forge6.pkcs5 || {};
    var crypto4;
    if (forge6.util.isNodejs && !forge6.options.usePureJavaScript) {
      crypto4 = require_crypto();
    }
    module2.exports = forge6.pbkdf2 = pkcs5.pbkdf2 = function(p, s, c, dkLen, md, callback) {
      if (typeof md === "function") {
        callback = md;
        md = null;
      }
      if (forge6.util.isNodejs && !forge6.options.usePureJavaScript && crypto4.pbkdf2 && (md === null || typeof md !== "object") && (crypto4.pbkdf2Sync.length > 4 || (!md || md === "sha1"))) {
        if (typeof md !== "string") {
          md = "sha1";
        }
        p = Buffer.from(p, "binary");
        s = Buffer.from(s, "binary");
        if (!callback) {
          if (crypto4.pbkdf2Sync.length === 4) {
            return crypto4.pbkdf2Sync(p, s, c, dkLen).toString("binary");
          }
          return crypto4.pbkdf2Sync(p, s, c, dkLen, md).toString("binary");
        }
        if (crypto4.pbkdf2Sync.length === 4) {
          return crypto4.pbkdf2(p, s, c, dkLen, function(err2, key) {
            if (err2) {
              return callback(err2);
            }
            callback(null, key.toString("binary"));
          });
        }
        return crypto4.pbkdf2(p, s, c, dkLen, md, function(err2, key) {
          if (err2) {
            return callback(err2);
          }
          callback(null, key.toString("binary"));
        });
      }
      if (typeof md === "undefined" || md === null) {
        md = "sha1";
      }
      if (typeof md === "string") {
        if (!(md in forge6.md.algorithms)) {
          throw new Error("Unknown hash algorithm: " + md);
        }
        md = forge6.md[md].create();
      }
      var hLen = md.digestLength;
      if (dkLen > 4294967295 * hLen) {
        var err = new Error("Derived key is too long.");
        if (callback) {
          return callback(err);
        }
        throw err;
      }
      var len = Math.ceil(dkLen / hLen);
      var r = dkLen - (len - 1) * hLen;
      var prf = forge6.hmac.create();
      prf.start(md, p);
      var dk = "";
      var xor, u_c, u_c1;
      if (!callback) {
        for (var i = 1; i <= len; ++i) {
          prf.start(null, null);
          prf.update(s);
          prf.update(forge6.util.int32ToBytes(i));
          xor = u_c1 = prf.digest().getBytes();
          for (var j = 2; j <= c; ++j) {
            prf.start(null, null);
            prf.update(u_c1);
            u_c = prf.digest().getBytes();
            xor = forge6.util.xorBytes(xor, u_c, hLen);
            u_c1 = u_c;
          }
          dk += i < len ? xor : xor.substr(0, r);
        }
        return dk;
      }
      var i = 1, j;
      function outer() {
        if (i > len) {
          return callback(null, dk);
        }
        prf.start(null, null);
        prf.update(s);
        prf.update(forge6.util.int32ToBytes(i));
        xor = u_c1 = prf.digest().getBytes();
        j = 2;
        inner();
      }
      function inner() {
        if (j <= c) {
          prf.start(null, null);
          prf.update(u_c1);
          u_c = prf.digest().getBytes();
          xor = forge6.util.xorBytes(xor, u_c, hLen);
          u_c1 = u_c;
          ++j;
          return forge6.util.setImmediate(inner);
        }
        dk += i < len ? xor : xor.substr(0, r);
        ++i;
        outer();
      }
      outer();
    };
  }
});

// ../../node_modules/node-forge/lib/pem.js
var require_pem = __commonJS({
  "../../node_modules/node-forge/lib/pem.js"(exports2, module2) {
    var forge6 = require_forge();
    require_util();
    var pem = module2.exports = forge6.pem = forge6.pem || {};
    pem.encode = function(msg, options) {
      options = options || {};
      var rval = "-----BEGIN " + msg.type + "-----\r\n";
      var header;
      if (msg.procType) {
        header = {
          name: "Proc-Type",
          values: [String(msg.procType.version), msg.procType.type]
        };
        rval += foldHeader(header);
      }
      if (msg.contentDomain) {
        header = { name: "Content-Domain", values: [msg.contentDomain] };
        rval += foldHeader(header);
      }
      if (msg.dekInfo) {
        header = { name: "DEK-Info", values: [msg.dekInfo.algorithm] };
        if (msg.dekInfo.parameters) {
          header.values.push(msg.dekInfo.parameters);
        }
        rval += foldHeader(header);
      }
      if (msg.headers) {
        for (var i = 0; i < msg.headers.length; ++i) {
          rval += foldHeader(msg.headers[i]);
        }
      }
      if (msg.procType) {
        rval += "\r\n";
      }
      rval += forge6.util.encode64(msg.body, options.maxline || 64) + "\r\n";
      rval += "-----END " + msg.type + "-----\r\n";
      return rval;
    };
    pem.decode = function(str) {
      var rval = [];
      var rMessage = /\s*-----BEGIN ([A-Z0-9- ]+)-----\r?\n?([\x21-\x7e\s]+?(?:\r?\n\r?\n))?([:A-Za-z0-9+\/=\s]+?)-----END \1-----/g;
      var rHeader = /([\x21-\x7e]+):\s*([\x21-\x7e\s^:]+)/;
      var rCRLF = /\r?\n/;
      var match;
      while (true) {
        match = rMessage.exec(str);
        if (!match) {
          break;
        }
        var type = match[1];
        if (type === "NEW CERTIFICATE REQUEST") {
          type = "CERTIFICATE REQUEST";
        }
        var msg = {
          type,
          procType: null,
          contentDomain: null,
          dekInfo: null,
          headers: [],
          body: forge6.util.decode64(match[3])
        };
        rval.push(msg);
        if (!match[2]) {
          continue;
        }
        var lines = match[2].split(rCRLF);
        var li = 0;
        while (match && li < lines.length) {
          var line = lines[li].replace(/\s+$/, "");
          for (var nl = li + 1; nl < lines.length; ++nl) {
            var next = lines[nl];
            if (!/\s/.test(next[0])) {
              break;
            }
            line += next;
            li = nl;
          }
          match = line.match(rHeader);
          if (match) {
            var header = { name: match[1], values: [] };
            var values = match[2].split(",");
            for (var vi = 0; vi < values.length; ++vi) {
              header.values.push(ltrim(values[vi]));
            }
            if (!msg.procType) {
              if (header.name !== "Proc-Type") {
                throw new Error('Invalid PEM formatted message. The first encapsulated header must be "Proc-Type".');
              } else if (header.values.length !== 2) {
                throw new Error('Invalid PEM formatted message. The "Proc-Type" header must have two subfields.');
              }
              msg.procType = { version: values[0], type: values[1] };
            } else if (!msg.contentDomain && header.name === "Content-Domain") {
              msg.contentDomain = values[0] || "";
            } else if (!msg.dekInfo && header.name === "DEK-Info") {
              if (header.values.length === 0) {
                throw new Error('Invalid PEM formatted message. The "DEK-Info" header must have at least one subfield.');
              }
              msg.dekInfo = { algorithm: values[0], parameters: values[1] || null };
            } else {
              msg.headers.push(header);
            }
          }
          ++li;
        }
        if (msg.procType === "ENCRYPTED" && !msg.dekInfo) {
          throw new Error('Invalid PEM formatted message. The "DEK-Info" header must be present if "Proc-Type" is "ENCRYPTED".');
        }
      }
      if (rval.length === 0) {
        throw new Error("Invalid PEM formatted message.");
      }
      return rval;
    };
    function foldHeader(header) {
      var rval = header.name + ": ";
      var values = [];
      var insertSpace = function(match, $1) {
        return " " + $1;
      };
      for (var i = 0; i < header.values.length; ++i) {
        values.push(header.values[i].replace(/^(\S+\r\n)/, insertSpace));
      }
      rval += values.join(",") + "\r\n";
      var length3 = 0;
      var candidate = -1;
      for (var i = 0; i < rval.length; ++i, ++length3) {
        if (length3 > 65 && candidate !== -1) {
          var insert = rval[candidate];
          if (insert === ",") {
            ++candidate;
            rval = rval.substr(0, candidate) + "\r\n " + rval.substr(candidate);
          } else {
            rval = rval.substr(0, candidate) + "\r\n" + insert + rval.substr(candidate + 1);
          }
          length3 = i - candidate - 1;
          candidate = -1;
          ++i;
        } else if (rval[i] === " " || rval[i] === "	" || rval[i] === ",") {
          candidate = i;
        }
      }
      return rval;
    }
    function ltrim(str) {
      return str.replace(/^\s+/, "");
    }
  }
});

// ../../node_modules/node-forge/lib/sha256.js
var require_sha256 = __commonJS({
  "../../node_modules/node-forge/lib/sha256.js"(exports2, module2) {
    var forge6 = require_forge();
    require_md();
    require_util();
    var sha2563 = module2.exports = forge6.sha256 = forge6.sha256 || {};
    forge6.md.sha256 = forge6.md.algorithms.sha256 = sha2563;
    sha2563.create = function() {
      if (!_initialized) {
        _init();
      }
      var _state = null;
      var _input = forge6.util.createBuffer();
      var _w = new Array(64);
      var md = {
        algorithm: "sha256",
        blockLength: 64,
        digestLength: 32,
        // 56-bit length of message so far (does not including padding)
        messageLength: 0,
        // true message length
        fullMessageLength: null,
        // size of message length in bytes
        messageLengthSize: 8
      };
      md.start = function() {
        md.messageLength = 0;
        md.fullMessageLength = md.messageLength64 = [];
        var int32s = md.messageLengthSize / 4;
        for (var i = 0; i < int32s; ++i) {
          md.fullMessageLength.push(0);
        }
        _input = forge6.util.createBuffer();
        _state = {
          h0: 1779033703,
          h1: 3144134277,
          h2: 1013904242,
          h3: 2773480762,
          h4: 1359893119,
          h5: 2600822924,
          h6: 528734635,
          h7: 1541459225
        };
        return md;
      };
      md.start();
      md.update = function(msg, encoding) {
        if (encoding === "utf8") {
          msg = forge6.util.encodeUtf8(msg);
        }
        var len = msg.length;
        md.messageLength += len;
        len = [len / 4294967296 >>> 0, len >>> 0];
        for (var i = md.fullMessageLength.length - 1; i >= 0; --i) {
          md.fullMessageLength[i] += len[1];
          len[1] = len[0] + (md.fullMessageLength[i] / 4294967296 >>> 0);
          md.fullMessageLength[i] = md.fullMessageLength[i] >>> 0;
          len[0] = len[1] / 4294967296 >>> 0;
        }
        _input.putBytes(msg);
        _update(_state, _w, _input);
        if (_input.read > 2048 || _input.length() === 0) {
          _input.compact();
        }
        return md;
      };
      md.digest = function() {
        var finalBlock = forge6.util.createBuffer();
        finalBlock.putBytes(_input.bytes());
        var remaining = md.fullMessageLength[md.fullMessageLength.length - 1] + md.messageLengthSize;
        var overflow = remaining & md.blockLength - 1;
        finalBlock.putBytes(_padding.substr(0, md.blockLength - overflow));
        var next, carry;
        var bits2 = md.fullMessageLength[0] * 8;
        for (var i = 0; i < md.fullMessageLength.length - 1; ++i) {
          next = md.fullMessageLength[i + 1] * 8;
          carry = next / 4294967296 >>> 0;
          bits2 += carry;
          finalBlock.putInt32(bits2 >>> 0);
          bits2 = next >>> 0;
        }
        finalBlock.putInt32(bits2);
        var s2 = {
          h0: _state.h0,
          h1: _state.h1,
          h2: _state.h2,
          h3: _state.h3,
          h4: _state.h4,
          h5: _state.h5,
          h6: _state.h6,
          h7: _state.h7
        };
        _update(s2, _w, finalBlock);
        var rval = forge6.util.createBuffer();
        rval.putInt32(s2.h0);
        rval.putInt32(s2.h1);
        rval.putInt32(s2.h2);
        rval.putInt32(s2.h3);
        rval.putInt32(s2.h4);
        rval.putInt32(s2.h5);
        rval.putInt32(s2.h6);
        rval.putInt32(s2.h7);
        return rval;
      };
      return md;
    };
    var _padding = null;
    var _initialized = false;
    var _k = null;
    function _init() {
      _padding = String.fromCharCode(128);
      _padding += forge6.util.fillString(String.fromCharCode(0), 64);
      _k = [
        1116352408,
        1899447441,
        3049323471,
        3921009573,
        961987163,
        1508970993,
        2453635748,
        2870763221,
        3624381080,
        310598401,
        607225278,
        1426881987,
        1925078388,
        2162078206,
        2614888103,
        3248222580,
        3835390401,
        4022224774,
        264347078,
        604807628,
        770255983,
        1249150122,
        1555081692,
        1996064986,
        2554220882,
        2821834349,
        2952996808,
        3210313671,
        3336571891,
        3584528711,
        113926993,
        338241895,
        666307205,
        773529912,
        1294757372,
        1396182291,
        1695183700,
        1986661051,
        2177026350,
        2456956037,
        2730485921,
        2820302411,
        3259730800,
        3345764771,
        3516065817,
        3600352804,
        4094571909,
        275423344,
        430227734,
        506948616,
        659060556,
        883997877,
        958139571,
        1322822218,
        1537002063,
        1747873779,
        1955562222,
        2024104815,
        2227730452,
        2361852424,
        2428436474,
        2756734187,
        3204031479,
        3329325298
      ];
      _initialized = true;
    }
    function _update(s, w, bytes) {
      var t1, t2, s0, s1, ch, maj, i, a, b, c, d, e, f, g, h;
      var len = bytes.length();
      while (len >= 64) {
        for (i = 0; i < 16; ++i) {
          w[i] = bytes.getInt32();
        }
        for (; i < 64; ++i) {
          t1 = w[i - 2];
          t1 = (t1 >>> 17 | t1 << 15) ^ (t1 >>> 19 | t1 << 13) ^ t1 >>> 10;
          t2 = w[i - 15];
          t2 = (t2 >>> 7 | t2 << 25) ^ (t2 >>> 18 | t2 << 14) ^ t2 >>> 3;
          w[i] = t1 + w[i - 7] + t2 + w[i - 16] | 0;
        }
        a = s.h0;
        b = s.h1;
        c = s.h2;
        d = s.h3;
        e = s.h4;
        f = s.h5;
        g = s.h6;
        h = s.h7;
        for (i = 0; i < 64; ++i) {
          s1 = (e >>> 6 | e << 26) ^ (e >>> 11 | e << 21) ^ (e >>> 25 | e << 7);
          ch = g ^ e & (f ^ g);
          s0 = (a >>> 2 | a << 30) ^ (a >>> 13 | a << 19) ^ (a >>> 22 | a << 10);
          maj = a & b | c & (a ^ b);
          t1 = h + s1 + ch + _k[i] + w[i];
          t2 = s0 + maj;
          h = g;
          g = f;
          f = e;
          e = d + t1 >>> 0;
          d = c;
          c = b;
          b = a;
          a = t1 + t2 >>> 0;
        }
        s.h0 = s.h0 + a | 0;
        s.h1 = s.h1 + b | 0;
        s.h2 = s.h2 + c | 0;
        s.h3 = s.h3 + d | 0;
        s.h4 = s.h4 + e | 0;
        s.h5 = s.h5 + f | 0;
        s.h6 = s.h6 + g | 0;
        s.h7 = s.h7 + h | 0;
        len -= 64;
      }
    }
  }
});

// ../../node_modules/node-forge/lib/prng.js
var require_prng = __commonJS({
  "../../node_modules/node-forge/lib/prng.js"(exports2, module2) {
    var forge6 = require_forge();
    require_util();
    var _crypto = null;
    if (forge6.util.isNodejs && !forge6.options.usePureJavaScript && !process.versions["node-webkit"]) {
      _crypto = require_crypto();
    }
    var prng = module2.exports = forge6.prng = forge6.prng || {};
    prng.create = function(plugin) {
      var ctx = {
        plugin,
        key: null,
        seed: null,
        time: null,
        // number of reseeds so far
        reseeds: 0,
        // amount of data generated so far
        generated: 0,
        // no initial key bytes
        keyBytes: ""
      };
      var md = plugin.md;
      var pools = new Array(32);
      for (var i = 0; i < 32; ++i) {
        pools[i] = md.create();
      }
      ctx.pools = pools;
      ctx.pool = 0;
      ctx.generate = function(count, callback) {
        if (!callback) {
          return ctx.generateSync(count);
        }
        var cipher = ctx.plugin.cipher;
        var increment = ctx.plugin.increment;
        var formatKey = ctx.plugin.formatKey;
        var formatSeed = ctx.plugin.formatSeed;
        var b = forge6.util.createBuffer();
        ctx.key = null;
        generate();
        function generate(err) {
          if (err) {
            return callback(err);
          }
          if (b.length() >= count) {
            return callback(null, b.getBytes(count));
          }
          if (ctx.generated > 1048575) {
            ctx.key = null;
          }
          if (ctx.key === null) {
            return forge6.util.nextTick(function() {
              _reseed(generate);
            });
          }
          var bytes = cipher(ctx.key, ctx.seed);
          ctx.generated += bytes.length;
          b.putBytes(bytes);
          ctx.key = formatKey(cipher(ctx.key, increment(ctx.seed)));
          ctx.seed = formatSeed(cipher(ctx.key, ctx.seed));
          forge6.util.setImmediate(generate);
        }
      };
      ctx.generateSync = function(count) {
        var cipher = ctx.plugin.cipher;
        var increment = ctx.plugin.increment;
        var formatKey = ctx.plugin.formatKey;
        var formatSeed = ctx.plugin.formatSeed;
        ctx.key = null;
        var b = forge6.util.createBuffer();
        while (b.length() < count) {
          if (ctx.generated > 1048575) {
            ctx.key = null;
          }
          if (ctx.key === null) {
            _reseedSync();
          }
          var bytes = cipher(ctx.key, ctx.seed);
          ctx.generated += bytes.length;
          b.putBytes(bytes);
          ctx.key = formatKey(cipher(ctx.key, increment(ctx.seed)));
          ctx.seed = formatSeed(cipher(ctx.key, ctx.seed));
        }
        return b.getBytes(count);
      };
      function _reseed(callback) {
        if (ctx.pools[0].messageLength >= 32) {
          _seed();
          return callback();
        }
        var needed = 32 - ctx.pools[0].messageLength << 5;
        ctx.seedFile(needed, function(err, bytes) {
          if (err) {
            return callback(err);
          }
          ctx.collect(bytes);
          _seed();
          callback();
        });
      }
      function _reseedSync() {
        if (ctx.pools[0].messageLength >= 32) {
          return _seed();
        }
        var needed = 32 - ctx.pools[0].messageLength << 5;
        ctx.collect(ctx.seedFileSync(needed));
        _seed();
      }
      function _seed() {
        ctx.reseeds = ctx.reseeds === 4294967295 ? 0 : ctx.reseeds + 1;
        var md2 = ctx.plugin.md.create();
        md2.update(ctx.keyBytes);
        var _2powK = 1;
        for (var k = 0; k < 32; ++k) {
          if (ctx.reseeds % _2powK === 0) {
            md2.update(ctx.pools[k].digest().getBytes());
            ctx.pools[k].start();
          }
          _2powK = _2powK << 1;
        }
        ctx.keyBytes = md2.digest().getBytes();
        md2.start();
        md2.update(ctx.keyBytes);
        var seedBytes = md2.digest().getBytes();
        ctx.key = ctx.plugin.formatKey(ctx.keyBytes);
        ctx.seed = ctx.plugin.formatSeed(seedBytes);
        ctx.generated = 0;
      }
      function defaultSeedFile(needed) {
        var getRandomValues = null;
        var globalScope = forge6.util.globalScope;
        var _crypto2 = globalScope.crypto || globalScope.msCrypto;
        if (_crypto2 && _crypto2.getRandomValues) {
          getRandomValues = function(arr) {
            return _crypto2.getRandomValues(arr);
          };
        }
        var b = forge6.util.createBuffer();
        if (getRandomValues) {
          while (b.length() < needed) {
            var count = Math.max(1, Math.min(needed - b.length(), 65536) / 4);
            var entropy = new Uint32Array(Math.floor(count));
            try {
              getRandomValues(entropy);
              for (var i2 = 0; i2 < entropy.length; ++i2) {
                b.putInt32(entropy[i2]);
              }
            } catch (e) {
              if (!(typeof QuotaExceededError !== "undefined" && e instanceof QuotaExceededError)) {
                throw e;
              }
            }
          }
        }
        if (b.length() < needed) {
          var hi, lo, next;
          var seed = Math.floor(Math.random() * 65536);
          while (b.length() < needed) {
            lo = 16807 * (seed & 65535);
            hi = 16807 * (seed >> 16);
            lo += (hi & 32767) << 16;
            lo += hi >> 15;
            lo = (lo & 2147483647) + (lo >> 31);
            seed = lo & 4294967295;
            for (var i2 = 0; i2 < 3; ++i2) {
              next = seed >>> (i2 << 3);
              next ^= Math.floor(Math.random() * 256);
              b.putByte(next & 255);
            }
          }
        }
        return b.getBytes(needed);
      }
      if (_crypto) {
        ctx.seedFile = function(needed, callback) {
          _crypto.randomBytes(needed, function(err, bytes) {
            if (err) {
              return callback(err);
            }
            callback(null, bytes.toString());
          });
        };
        ctx.seedFileSync = function(needed) {
          return _crypto.randomBytes(needed).toString();
        };
      } else {
        ctx.seedFile = function(needed, callback) {
          try {
            callback(null, defaultSeedFile(needed));
          } catch (e) {
            callback(e);
          }
        };
        ctx.seedFileSync = defaultSeedFile;
      }
      ctx.collect = function(bytes) {
        var count = bytes.length;
        for (var i2 = 0; i2 < count; ++i2) {
          ctx.pools[ctx.pool].update(bytes.substr(i2, 1));
          ctx.pool = ctx.pool === 31 ? 0 : ctx.pool + 1;
        }
      };
      ctx.collectInt = function(i2, n) {
        var bytes = "";
        for (var x = 0; x < n; x += 8) {
          bytes += String.fromCharCode(i2 >> x & 255);
        }
        ctx.collect(bytes);
      };
      ctx.registerWorker = function(worker) {
        if (worker === self) {
          ctx.seedFile = function(needed, callback) {
            function listener2(e) {
              var data = e.data;
              if (data.forge && data.forge.prng) {
                self.removeEventListener("message", listener2);
                callback(data.forge.prng.err, data.forge.prng.bytes);
              }
            }
            self.addEventListener("message", listener2);
            self.postMessage({ forge: { prng: { needed } } });
          };
        } else {
          var listener = function(e) {
            var data = e.data;
            if (data.forge && data.forge.prng) {
              ctx.seedFile(data.forge.prng.needed, function(err, bytes) {
                worker.postMessage({ forge: { prng: { err, bytes } } });
              });
            }
          };
          worker.addEventListener("message", listener);
        }
      };
      return ctx;
    };
  }
});

// ../../node_modules/node-forge/lib/random.js
var require_random = __commonJS({
  "../../node_modules/node-forge/lib/random.js"(exports2, module2) {
    var forge6 = require_forge();
    require_aes();
    require_sha256();
    require_prng();
    require_util();
    (function() {
      if (forge6.random && forge6.random.getBytes) {
        module2.exports = forge6.random;
        return;
      }
      (function(jQuery2) {
        var prng_aes = {};
        var _prng_aes_output = new Array(4);
        var _prng_aes_buffer = forge6.util.createBuffer();
        prng_aes.formatKey = function(key2) {
          var tmp = forge6.util.createBuffer(key2);
          key2 = new Array(4);
          key2[0] = tmp.getInt32();
          key2[1] = tmp.getInt32();
          key2[2] = tmp.getInt32();
          key2[3] = tmp.getInt32();
          return forge6.aes._expandKey(key2, false);
        };
        prng_aes.formatSeed = function(seed) {
          var tmp = forge6.util.createBuffer(seed);
          seed = new Array(4);
          seed[0] = tmp.getInt32();
          seed[1] = tmp.getInt32();
          seed[2] = tmp.getInt32();
          seed[3] = tmp.getInt32();
          return seed;
        };
        prng_aes.cipher = function(key2, seed) {
          forge6.aes._updateBlock(key2, seed, _prng_aes_output, false);
          _prng_aes_buffer.putInt32(_prng_aes_output[0]);
          _prng_aes_buffer.putInt32(_prng_aes_output[1]);
          _prng_aes_buffer.putInt32(_prng_aes_output[2]);
          _prng_aes_buffer.putInt32(_prng_aes_output[3]);
          return _prng_aes_buffer.getBytes();
        };
        prng_aes.increment = function(seed) {
          ++seed[3];
          return seed;
        };
        prng_aes.md = forge6.md.sha256;
        function spawnPrng() {
          var ctx = forge6.prng.create(prng_aes);
          ctx.getBytes = function(count, callback) {
            return ctx.generate(count, callback);
          };
          ctx.getBytesSync = function(count) {
            return ctx.generate(count);
          };
          return ctx;
        }
        var _ctx = spawnPrng();
        var getRandomValues = null;
        var globalScope = forge6.util.globalScope;
        var _crypto = globalScope.crypto || globalScope.msCrypto;
        if (_crypto && _crypto.getRandomValues) {
          getRandomValues = function(arr) {
            return _crypto.getRandomValues(arr);
          };
        }
        if (forge6.options.usePureJavaScript || !forge6.util.isNodejs && !getRandomValues) {
          if (typeof window === "undefined" || window.document === void 0) {
          }
          _ctx.collectInt(+/* @__PURE__ */ new Date(), 32);
          if (typeof navigator !== "undefined") {
            var _navBytes = "";
            for (var key in navigator) {
              try {
                if (typeof navigator[key] == "string") {
                  _navBytes += navigator[key];
                }
              } catch (e) {
              }
            }
            _ctx.collect(_navBytes);
            _navBytes = null;
          }
          if (jQuery2) {
            jQuery2().mousemove(function(e) {
              _ctx.collectInt(e.clientX, 16);
              _ctx.collectInt(e.clientY, 16);
            });
            jQuery2().keypress(function(e) {
              _ctx.collectInt(e.charCode, 8);
            });
          }
        }
        if (!forge6.random) {
          forge6.random = _ctx;
        } else {
          for (var key in _ctx) {
            forge6.random[key] = _ctx[key];
          }
        }
        forge6.random.createInstance = spawnPrng;
        module2.exports = forge6.random;
      })(typeof jQuery !== "undefined" ? jQuery : null);
    })();
  }
});

// ../../node_modules/node-forge/lib/rc2.js
var require_rc2 = __commonJS({
  "../../node_modules/node-forge/lib/rc2.js"(exports2, module2) {
    var forge6 = require_forge();
    require_util();
    var piTable = [
      217,
      120,
      249,
      196,
      25,
      221,
      181,
      237,
      40,
      233,
      253,
      121,
      74,
      160,
      216,
      157,
      198,
      126,
      55,
      131,
      43,
      118,
      83,
      142,
      98,
      76,
      100,
      136,
      68,
      139,
      251,
      162,
      23,
      154,
      89,
      245,
      135,
      179,
      79,
      19,
      97,
      69,
      109,
      141,
      9,
      129,
      125,
      50,
      189,
      143,
      64,
      235,
      134,
      183,
      123,
      11,
      240,
      149,
      33,
      34,
      92,
      107,
      78,
      130,
      84,
      214,
      101,
      147,
      206,
      96,
      178,
      28,
      115,
      86,
      192,
      20,
      167,
      140,
      241,
      220,
      18,
      117,
      202,
      31,
      59,
      190,
      228,
      209,
      66,
      61,
      212,
      48,
      163,
      60,
      182,
      38,
      111,
      191,
      14,
      218,
      70,
      105,
      7,
      87,
      39,
      242,
      29,
      155,
      188,
      148,
      67,
      3,
      248,
      17,
      199,
      246,
      144,
      239,
      62,
      231,
      6,
      195,
      213,
      47,
      200,
      102,
      30,
      215,
      8,
      232,
      234,
      222,
      128,
      82,
      238,
      247,
      132,
      170,
      114,
      172,
      53,
      77,
      106,
      42,
      150,
      26,
      210,
      113,
      90,
      21,
      73,
      116,
      75,
      159,
      208,
      94,
      4,
      24,
      164,
      236,
      194,
      224,
      65,
      110,
      15,
      81,
      203,
      204,
      36,
      145,
      175,
      80,
      161,
      244,
      112,
      57,
      153,
      124,
      58,
      133,
      35,
      184,
      180,
      122,
      252,
      2,
      54,
      91,
      37,
      85,
      151,
      49,
      45,
      93,
      250,
      152,
      227,
      138,
      146,
      174,
      5,
      223,
      41,
      16,
      103,
      108,
      186,
      201,
      211,
      0,
      230,
      207,
      225,
      158,
      168,
      44,
      99,
      22,
      1,
      63,
      88,
      226,
      137,
      169,
      13,
      56,
      52,
      27,
      171,
      51,
      255,
      176,
      187,
      72,
      12,
      95,
      185,
      177,
      205,
      46,
      197,
      243,
      219,
      71,
      229,
      165,
      156,
      119,
      10,
      166,
      32,
      104,
      254,
      127,
      193,
      173
    ];
    var s = [1, 2, 3, 5];
    var rol = function(word, bits2) {
      return word << bits2 & 65535 | (word & 65535) >> 16 - bits2;
    };
    var ror = function(word, bits2) {
      return (word & 65535) >> bits2 | word << 16 - bits2 & 65535;
    };
    module2.exports = forge6.rc2 = forge6.rc2 || {};
    forge6.rc2.expandKey = function(key, effKeyBits) {
      if (typeof key === "string") {
        key = forge6.util.createBuffer(key);
      }
      effKeyBits = effKeyBits || 128;
      var L = key;
      var T = key.length();
      var T1 = effKeyBits;
      var T8 = Math.ceil(T1 / 8);
      var TM = 255 >> (T1 & 7);
      var i;
      for (i = T; i < 128; i++) {
        L.putByte(piTable[L.at(i - 1) + L.at(i - T) & 255]);
      }
      L.setAt(128 - T8, piTable[L.at(128 - T8) & TM]);
      for (i = 127 - T8; i >= 0; i--) {
        L.setAt(i, piTable[L.at(i + 1) ^ L.at(i + T8)]);
      }
      return L;
    };
    var createCipher = function(key, bits2, encrypt2) {
      var _finish = false, _input = null, _output = null, _iv = null;
      var mixRound, mashRound;
      var i, j, K = [];
      key = forge6.rc2.expandKey(key, bits2);
      for (i = 0; i < 64; i++) {
        K.push(key.getInt16Le());
      }
      if (encrypt2) {
        mixRound = function(R) {
          for (i = 0; i < 4; i++) {
            R[i] += K[j] + (R[(i + 3) % 4] & R[(i + 2) % 4]) + (~R[(i + 3) % 4] & R[(i + 1) % 4]);
            R[i] = rol(R[i], s[i]);
            j++;
          }
        };
        mashRound = function(R) {
          for (i = 0; i < 4; i++) {
            R[i] += K[R[(i + 3) % 4] & 63];
          }
        };
      } else {
        mixRound = function(R) {
          for (i = 3; i >= 0; i--) {
            R[i] = ror(R[i], s[i]);
            R[i] -= K[j] + (R[(i + 3) % 4] & R[(i + 2) % 4]) + (~R[(i + 3) % 4] & R[(i + 1) % 4]);
            j--;
          }
        };
        mashRound = function(R) {
          for (i = 3; i >= 0; i--) {
            R[i] -= K[R[(i + 3) % 4] & 63];
          }
        };
      }
      var runPlan = function(plan) {
        var R = [];
        for (i = 0; i < 4; i++) {
          var val = _input.getInt16Le();
          if (_iv !== null) {
            if (encrypt2) {
              val ^= _iv.getInt16Le();
            } else {
              _iv.putInt16Le(val);
            }
          }
          R.push(val & 65535);
        }
        j = encrypt2 ? 0 : 63;
        for (var ptr = 0; ptr < plan.length; ptr++) {
          for (var ctr = 0; ctr < plan[ptr][0]; ctr++) {
            plan[ptr][1](R);
          }
        }
        for (i = 0; i < 4; i++) {
          if (_iv !== null) {
            if (encrypt2) {
              _iv.putInt16Le(R[i]);
            } else {
              R[i] ^= _iv.getInt16Le();
            }
          }
          _output.putInt16Le(R[i]);
        }
      };
      var cipher = null;
      cipher = {
        /**
         * Starts or restarts the encryption or decryption process, whichever
         * was previously configured.
         *
         * To use the cipher in CBC mode, iv may be given either as a string
         * of bytes, or as a byte buffer.  For ECB mode, give null as iv.
         *
         * @param iv the initialization vector to use, null for ECB mode.
         * @param output the output the buffer to write to, null to create one.
         */
        start: function(iv, output) {
          if (iv) {
            if (typeof iv === "string") {
              iv = forge6.util.createBuffer(iv);
            }
          }
          _finish = false;
          _input = forge6.util.createBuffer();
          _output = output || new forge6.util.createBuffer();
          _iv = iv;
          cipher.output = _output;
        },
        /**
         * Updates the next block.
         *
         * @param input the buffer to read from.
         */
        update: function(input) {
          if (!_finish) {
            _input.putBuffer(input);
          }
          while (_input.length() >= 8) {
            runPlan([
              [5, mixRound],
              [1, mashRound],
              [6, mixRound],
              [1, mashRound],
              [5, mixRound]
            ]);
          }
        },
        /**
         * Finishes encrypting or decrypting.
         *
         * @param pad a padding function to use, null for PKCS#7 padding,
         *           signature(blockSize, buffer, decrypt).
         *
         * @return true if successful, false on error.
         */
        finish: function(pad) {
          var rval = true;
          if (encrypt2) {
            if (pad) {
              rval = pad(8, _input, !encrypt2);
            } else {
              var padding = _input.length() === 8 ? 8 : 8 - _input.length();
              _input.fillWithByte(padding, padding);
            }
          }
          if (rval) {
            _finish = true;
            cipher.update();
          }
          if (!encrypt2) {
            rval = _input.length() === 0;
            if (rval) {
              if (pad) {
                rval = pad(8, _output, !encrypt2);
              } else {
                var len = _output.length();
                var count = _output.at(len - 1);
                if (count > len) {
                  rval = false;
                } else {
                  _output.truncate(count);
                }
              }
            }
          }
          return rval;
        }
      };
      return cipher;
    };
    forge6.rc2.startEncrypting = function(key, iv, output) {
      var cipher = forge6.rc2.createEncryptionCipher(key, 128);
      cipher.start(iv, output);
      return cipher;
    };
    forge6.rc2.createEncryptionCipher = function(key, bits2) {
      return createCipher(key, bits2, true);
    };
    forge6.rc2.startDecrypting = function(key, iv, output) {
      var cipher = forge6.rc2.createDecryptionCipher(key, 128);
      cipher.start(iv, output);
      return cipher;
    };
    forge6.rc2.createDecryptionCipher = function(key, bits2) {
      return createCipher(key, bits2, false);
    };
  }
});

// ../../node_modules/node-forge/lib/jsbn.js
var require_jsbn = __commonJS({
  "../../node_modules/node-forge/lib/jsbn.js"(exports2, module2) {
    var forge6 = require_forge();
    module2.exports = forge6.jsbn = forge6.jsbn || {};
    var dbits;
    var canary = 244837814094590;
    var j_lm = (canary & 16777215) == 15715070;
    function BigInteger(a, b, c) {
      this.data = [];
      if (a != null)
        if ("number" == typeof a)
          this.fromNumber(a, b, c);
        else if (b == null && "string" != typeof a)
          this.fromString(a, 256);
        else
          this.fromString(a, b);
    }
    forge6.jsbn.BigInteger = BigInteger;
    function nbi() {
      return new BigInteger(null);
    }
    function am1(i, x, w, j, c, n) {
      while (--n >= 0) {
        var v = x * this.data[i++] + w.data[j] + c;
        c = Math.floor(v / 67108864);
        w.data[j++] = v & 67108863;
      }
      return c;
    }
    function am2(i, x, w, j, c, n) {
      var xl = x & 32767, xh = x >> 15;
      while (--n >= 0) {
        var l = this.data[i] & 32767;
        var h = this.data[i++] >> 15;
        var m = xh * l + h * xl;
        l = xl * l + ((m & 32767) << 15) + w.data[j] + (c & 1073741823);
        c = (l >>> 30) + (m >>> 15) + xh * h + (c >>> 30);
        w.data[j++] = l & 1073741823;
      }
      return c;
    }
    function am3(i, x, w, j, c, n) {
      var xl = x & 16383, xh = x >> 14;
      while (--n >= 0) {
        var l = this.data[i] & 16383;
        var h = this.data[i++] >> 14;
        var m = xh * l + h * xl;
        l = xl * l + ((m & 16383) << 14) + w.data[j] + c;
        c = (l >> 28) + (m >> 14) + xh * h;
        w.data[j++] = l & 268435455;
      }
      return c;
    }
    if (typeof navigator === "undefined") {
      BigInteger.prototype.am = am3;
      dbits = 28;
    } else if (j_lm && navigator.appName == "Microsoft Internet Explorer") {
      BigInteger.prototype.am = am2;
      dbits = 30;
    } else if (j_lm && navigator.appName != "Netscape") {
      BigInteger.prototype.am = am1;
      dbits = 26;
    } else {
      BigInteger.prototype.am = am3;
      dbits = 28;
    }
    BigInteger.prototype.DB = dbits;
    BigInteger.prototype.DM = (1 << dbits) - 1;
    BigInteger.prototype.DV = 1 << dbits;
    var BI_FP = 52;
    BigInteger.prototype.FV = Math.pow(2, BI_FP);
    BigInteger.prototype.F1 = BI_FP - dbits;
    BigInteger.prototype.F2 = 2 * dbits - BI_FP;
    var BI_RM = "0123456789abcdefghijklmnopqrstuvwxyz";
    var BI_RC = new Array();
    var rr;
    var vv;
    rr = "0".charCodeAt(0);
    for (vv = 0; vv <= 9; ++vv)
      BI_RC[rr++] = vv;
    rr = "a".charCodeAt(0);
    for (vv = 10; vv < 36; ++vv)
      BI_RC[rr++] = vv;
    rr = "A".charCodeAt(0);
    for (vv = 10; vv < 36; ++vv)
      BI_RC[rr++] = vv;
    function int2char(n) {
      return BI_RM.charAt(n);
    }
    function intAt(s, i) {
      var c = BI_RC[s.charCodeAt(i)];
      return c == null ? -1 : c;
    }
    function bnpCopyTo(r) {
      for (var i = this.t - 1; i >= 0; --i)
        r.data[i] = this.data[i];
      r.t = this.t;
      r.s = this.s;
    }
    function bnpFromInt(x) {
      this.t = 1;
      this.s = x < 0 ? -1 : 0;
      if (x > 0)
        this.data[0] = x;
      else if (x < -1)
        this.data[0] = x + this.DV;
      else
        this.t = 0;
    }
    function nbv(i) {
      var r = nbi();
      r.fromInt(i);
      return r;
    }
    function bnpFromString(s, b) {
      var k;
      if (b == 16)
        k = 4;
      else if (b == 8)
        k = 3;
      else if (b == 256)
        k = 8;
      else if (b == 2)
        k = 1;
      else if (b == 32)
        k = 5;
      else if (b == 4)
        k = 2;
      else {
        this.fromRadix(s, b);
        return;
      }
      this.t = 0;
      this.s = 0;
      var i = s.length, mi = false, sh = 0;
      while (--i >= 0) {
        var x = k == 8 ? s[i] & 255 : intAt(s, i);
        if (x < 0) {
          if (s.charAt(i) == "-")
            mi = true;
          continue;
        }
        mi = false;
        if (sh == 0)
          this.data[this.t++] = x;
        else if (sh + k > this.DB) {
          this.data[this.t - 1] |= (x & (1 << this.DB - sh) - 1) << sh;
          this.data[this.t++] = x >> this.DB - sh;
        } else
          this.data[this.t - 1] |= x << sh;
        sh += k;
        if (sh >= this.DB)
          sh -= this.DB;
      }
      if (k == 8 && (s[0] & 128) != 0) {
        this.s = -1;
        if (sh > 0)
          this.data[this.t - 1] |= (1 << this.DB - sh) - 1 << sh;
      }
      this.clamp();
      if (mi)
        BigInteger.ZERO.subTo(this, this);
    }
    function bnpClamp() {
      var c = this.s & this.DM;
      while (this.t > 0 && this.data[this.t - 1] == c)
        --this.t;
    }
    function bnToString(b) {
      if (this.s < 0)
        return "-" + this.negate().toString(b);
      var k;
      if (b == 16)
        k = 4;
      else if (b == 8)
        k = 3;
      else if (b == 2)
        k = 1;
      else if (b == 32)
        k = 5;
      else if (b == 4)
        k = 2;
      else
        return this.toRadix(b);
      var km = (1 << k) - 1, d, m = false, r = "", i = this.t;
      var p = this.DB - i * this.DB % k;
      if (i-- > 0) {
        if (p < this.DB && (d = this.data[i] >> p) > 0) {
          m = true;
          r = int2char(d);
        }
        while (i >= 0) {
          if (p < k) {
            d = (this.data[i] & (1 << p) - 1) << k - p;
            d |= this.data[--i] >> (p += this.DB - k);
          } else {
            d = this.data[i] >> (p -= k) & km;
            if (p <= 0) {
              p += this.DB;
              --i;
            }
          }
          if (d > 0)
            m = true;
          if (m)
            r += int2char(d);
        }
      }
      return m ? r : "0";
    }
    function bnNegate() {
      var r = nbi();
      BigInteger.ZERO.subTo(this, r);
      return r;
    }
    function bnAbs() {
      return this.s < 0 ? this.negate() : this;
    }
    function bnCompareTo(a) {
      var r = this.s - a.s;
      if (r != 0)
        return r;
      var i = this.t;
      r = i - a.t;
      if (r != 0)
        return this.s < 0 ? -r : r;
      while (--i >= 0)
        if ((r = this.data[i] - a.data[i]) != 0)
          return r;
      return 0;
    }
    function nbits(x) {
      var r = 1, t;
      if ((t = x >>> 16) != 0) {
        x = t;
        r += 16;
      }
      if ((t = x >> 8) != 0) {
        x = t;
        r += 8;
      }
      if ((t = x >> 4) != 0) {
        x = t;
        r += 4;
      }
      if ((t = x >> 2) != 0) {
        x = t;
        r += 2;
      }
      if ((t = x >> 1) != 0) {
        x = t;
        r += 1;
      }
      return r;
    }
    function bnBitLength() {
      if (this.t <= 0)
        return 0;
      return this.DB * (this.t - 1) + nbits(this.data[this.t - 1] ^ this.s & this.DM);
    }
    function bnpDLShiftTo(n, r) {
      var i;
      for (i = this.t - 1; i >= 0; --i)
        r.data[i + n] = this.data[i];
      for (i = n - 1; i >= 0; --i)
        r.data[i] = 0;
      r.t = this.t + n;
      r.s = this.s;
    }
    function bnpDRShiftTo(n, r) {
      for (var i = n; i < this.t; ++i)
        r.data[i - n] = this.data[i];
      r.t = Math.max(this.t - n, 0);
      r.s = this.s;
    }
    function bnpLShiftTo(n, r) {
      var bs = n % this.DB;
      var cbs = this.DB - bs;
      var bm = (1 << cbs) - 1;
      var ds = Math.floor(n / this.DB), c = this.s << bs & this.DM, i;
      for (i = this.t - 1; i >= 0; --i) {
        r.data[i + ds + 1] = this.data[i] >> cbs | c;
        c = (this.data[i] & bm) << bs;
      }
      for (i = ds - 1; i >= 0; --i)
        r.data[i] = 0;
      r.data[ds] = c;
      r.t = this.t + ds + 1;
      r.s = this.s;
      r.clamp();
    }
    function bnpRShiftTo(n, r) {
      r.s = this.s;
      var ds = Math.floor(n / this.DB);
      if (ds >= this.t) {
        r.t = 0;
        return;
      }
      var bs = n % this.DB;
      var cbs = this.DB - bs;
      var bm = (1 << bs) - 1;
      r.data[0] = this.data[ds] >> bs;
      for (var i = ds + 1; i < this.t; ++i) {
        r.data[i - ds - 1] |= (this.data[i] & bm) << cbs;
        r.data[i - ds] = this.data[i] >> bs;
      }
      if (bs > 0)
        r.data[this.t - ds - 1] |= (this.s & bm) << cbs;
      r.t = this.t - ds;
      r.clamp();
    }
    function bnpSubTo(a, r) {
      var i = 0, c = 0, m = Math.min(a.t, this.t);
      while (i < m) {
        c += this.data[i] - a.data[i];
        r.data[i++] = c & this.DM;
        c >>= this.DB;
      }
      if (a.t < this.t) {
        c -= a.s;
        while (i < this.t) {
          c += this.data[i];
          r.data[i++] = c & this.DM;
          c >>= this.DB;
        }
        c += this.s;
      } else {
        c += this.s;
        while (i < a.t) {
          c -= a.data[i];
          r.data[i++] = c & this.DM;
          c >>= this.DB;
        }
        c -= a.s;
      }
      r.s = c < 0 ? -1 : 0;
      if (c < -1)
        r.data[i++] = this.DV + c;
      else if (c > 0)
        r.data[i++] = c;
      r.t = i;
      r.clamp();
    }
    function bnpMultiplyTo(a, r) {
      var x = this.abs(), y = a.abs();
      var i = x.t;
      r.t = i + y.t;
      while (--i >= 0)
        r.data[i] = 0;
      for (i = 0; i < y.t; ++i)
        r.data[i + x.t] = x.am(0, y.data[i], r, i, 0, x.t);
      r.s = 0;
      r.clamp();
      if (this.s != a.s)
        BigInteger.ZERO.subTo(r, r);
    }
    function bnpSquareTo(r) {
      var x = this.abs();
      var i = r.t = 2 * x.t;
      while (--i >= 0)
        r.data[i] = 0;
      for (i = 0; i < x.t - 1; ++i) {
        var c = x.am(i, x.data[i], r, 2 * i, 0, 1);
        if ((r.data[i + x.t] += x.am(i + 1, 2 * x.data[i], r, 2 * i + 1, c, x.t - i - 1)) >= x.DV) {
          r.data[i + x.t] -= x.DV;
          r.data[i + x.t + 1] = 1;
        }
      }
      if (r.t > 0)
        r.data[r.t - 1] += x.am(i, x.data[i], r, 2 * i, 0, 1);
      r.s = 0;
      r.clamp();
    }
    function bnpDivRemTo(m, q, r) {
      var pm = m.abs();
      if (pm.t <= 0)
        return;
      var pt = this.abs();
      if (pt.t < pm.t) {
        if (q != null)
          q.fromInt(0);
        if (r != null)
          this.copyTo(r);
        return;
      }
      if (r == null)
        r = nbi();
      var y = nbi(), ts = this.s, ms = m.s;
      var nsh = this.DB - nbits(pm.data[pm.t - 1]);
      if (nsh > 0) {
        pm.lShiftTo(nsh, y);
        pt.lShiftTo(nsh, r);
      } else {
        pm.copyTo(y);
        pt.copyTo(r);
      }
      var ys = y.t;
      var y0 = y.data[ys - 1];
      if (y0 == 0)
        return;
      var yt = y0 * (1 << this.F1) + (ys > 1 ? y.data[ys - 2] >> this.F2 : 0);
      var d1 = this.FV / yt, d2 = (1 << this.F1) / yt, e = 1 << this.F2;
      var i = r.t, j = i - ys, t = q == null ? nbi() : q;
      y.dlShiftTo(j, t);
      if (r.compareTo(t) >= 0) {
        r.data[r.t++] = 1;
        r.subTo(t, r);
      }
      BigInteger.ONE.dlShiftTo(ys, t);
      t.subTo(y, y);
      while (y.t < ys)
        y.data[y.t++] = 0;
      while (--j >= 0) {
        var qd = r.data[--i] == y0 ? this.DM : Math.floor(r.data[i] * d1 + (r.data[i - 1] + e) * d2);
        if ((r.data[i] += y.am(0, qd, r, j, 0, ys)) < qd) {
          y.dlShiftTo(j, t);
          r.subTo(t, r);
          while (r.data[i] < --qd)
            r.subTo(t, r);
        }
      }
      if (q != null) {
        r.drShiftTo(ys, q);
        if (ts != ms)
          BigInteger.ZERO.subTo(q, q);
      }
      r.t = ys;
      r.clamp();
      if (nsh > 0)
        r.rShiftTo(nsh, r);
      if (ts < 0)
        BigInteger.ZERO.subTo(r, r);
    }
    function bnMod(a) {
      var r = nbi();
      this.abs().divRemTo(a, null, r);
      if (this.s < 0 && r.compareTo(BigInteger.ZERO) > 0)
        a.subTo(r, r);
      return r;
    }
    function Classic(m) {
      this.m = m;
    }
    function cConvert(x) {
      if (x.s < 0 || x.compareTo(this.m) >= 0)
        return x.mod(this.m);
      else
        return x;
    }
    function cRevert(x) {
      return x;
    }
    function cReduce(x) {
      x.divRemTo(this.m, null, x);
    }
    function cMulTo(x, y, r) {
      x.multiplyTo(y, r);
      this.reduce(r);
    }
    function cSqrTo(x, r) {
      x.squareTo(r);
      this.reduce(r);
    }
    Classic.prototype.convert = cConvert;
    Classic.prototype.revert = cRevert;
    Classic.prototype.reduce = cReduce;
    Classic.prototype.mulTo = cMulTo;
    Classic.prototype.sqrTo = cSqrTo;
    function bnpInvDigit() {
      if (this.t < 1)
        return 0;
      var x = this.data[0];
      if ((x & 1) == 0)
        return 0;
      var y = x & 3;
      y = y * (2 - (x & 15) * y) & 15;
      y = y * (2 - (x & 255) * y) & 255;
      y = y * (2 - ((x & 65535) * y & 65535)) & 65535;
      y = y * (2 - x * y % this.DV) % this.DV;
      return y > 0 ? this.DV - y : -y;
    }
    function Montgomery(m) {
      this.m = m;
      this.mp = m.invDigit();
      this.mpl = this.mp & 32767;
      this.mph = this.mp >> 15;
      this.um = (1 << m.DB - 15) - 1;
      this.mt2 = 2 * m.t;
    }
    function montConvert(x) {
      var r = nbi();
      x.abs().dlShiftTo(this.m.t, r);
      r.divRemTo(this.m, null, r);
      if (x.s < 0 && r.compareTo(BigInteger.ZERO) > 0)
        this.m.subTo(r, r);
      return r;
    }
    function montRevert(x) {
      var r = nbi();
      x.copyTo(r);
      this.reduce(r);
      return r;
    }
    function montReduce(x) {
      while (x.t <= this.mt2)
        x.data[x.t++] = 0;
      for (var i = 0; i < this.m.t; ++i) {
        var j = x.data[i] & 32767;
        var u0 = j * this.mpl + ((j * this.mph + (x.data[i] >> 15) * this.mpl & this.um) << 15) & x.DM;
        j = i + this.m.t;
        x.data[j] += this.m.am(0, u0, x, i, 0, this.m.t);
        while (x.data[j] >= x.DV) {
          x.data[j] -= x.DV;
          x.data[++j]++;
        }
      }
      x.clamp();
      x.drShiftTo(this.m.t, x);
      if (x.compareTo(this.m) >= 0)
        x.subTo(this.m, x);
    }
    function montSqrTo(x, r) {
      x.squareTo(r);
      this.reduce(r);
    }
    function montMulTo(x, y, r) {
      x.multiplyTo(y, r);
      this.reduce(r);
    }
    Montgomery.prototype.convert = montConvert;
    Montgomery.prototype.revert = montRevert;
    Montgomery.prototype.reduce = montReduce;
    Montgomery.prototype.mulTo = montMulTo;
    Montgomery.prototype.sqrTo = montSqrTo;
    function bnpIsEven() {
      return (this.t > 0 ? this.data[0] & 1 : this.s) == 0;
    }
    function bnpExp(e, z) {
      if (e > 4294967295 || e < 1)
        return BigInteger.ONE;
      var r = nbi(), r2 = nbi(), g = z.convert(this), i = nbits(e) - 1;
      g.copyTo(r);
      while (--i >= 0) {
        z.sqrTo(r, r2);
        if ((e & 1 << i) > 0)
          z.mulTo(r2, g, r);
        else {
          var t = r;
          r = r2;
          r2 = t;
        }
      }
      return z.revert(r);
    }
    function bnModPowInt(e, m) {
      var z;
      if (e < 256 || m.isEven())
        z = new Classic(m);
      else
        z = new Montgomery(m);
      return this.exp(e, z);
    }
    BigInteger.prototype.copyTo = bnpCopyTo;
    BigInteger.prototype.fromInt = bnpFromInt;
    BigInteger.prototype.fromString = bnpFromString;
    BigInteger.prototype.clamp = bnpClamp;
    BigInteger.prototype.dlShiftTo = bnpDLShiftTo;
    BigInteger.prototype.drShiftTo = bnpDRShiftTo;
    BigInteger.prototype.lShiftTo = bnpLShiftTo;
    BigInteger.prototype.rShiftTo = bnpRShiftTo;
    BigInteger.prototype.subTo = bnpSubTo;
    BigInteger.prototype.multiplyTo = bnpMultiplyTo;
    BigInteger.prototype.squareTo = bnpSquareTo;
    BigInteger.prototype.divRemTo = bnpDivRemTo;
    BigInteger.prototype.invDigit = bnpInvDigit;
    BigInteger.prototype.isEven = bnpIsEven;
    BigInteger.prototype.exp = bnpExp;
    BigInteger.prototype.toString = bnToString;
    BigInteger.prototype.negate = bnNegate;
    BigInteger.prototype.abs = bnAbs;
    BigInteger.prototype.compareTo = bnCompareTo;
    BigInteger.prototype.bitLength = bnBitLength;
    BigInteger.prototype.mod = bnMod;
    BigInteger.prototype.modPowInt = bnModPowInt;
    BigInteger.ZERO = nbv(0);
    BigInteger.ONE = nbv(1);
    function bnClone() {
      var r = nbi();
      this.copyTo(r);
      return r;
    }
    function bnIntValue() {
      if (this.s < 0) {
        if (this.t == 1)
          return this.data[0] - this.DV;
        else if (this.t == 0)
          return -1;
      } else if (this.t == 1)
        return this.data[0];
      else if (this.t == 0)
        return 0;
      return (this.data[1] & (1 << 32 - this.DB) - 1) << this.DB | this.data[0];
    }
    function bnByteValue() {
      return this.t == 0 ? this.s : this.data[0] << 24 >> 24;
    }
    function bnShortValue() {
      return this.t == 0 ? this.s : this.data[0] << 16 >> 16;
    }
    function bnpChunkSize(r) {
      return Math.floor(Math.LN2 * this.DB / Math.log(r));
    }
    function bnSigNum() {
      if (this.s < 0)
        return -1;
      else if (this.t <= 0 || this.t == 1 && this.data[0] <= 0)
        return 0;
      else
        return 1;
    }
    function bnpToRadix(b) {
      if (b == null)
        b = 10;
      if (this.signum() == 0 || b < 2 || b > 36)
        return "0";
      var cs = this.chunkSize(b);
      var a = Math.pow(b, cs);
      var d = nbv(a), y = nbi(), z = nbi(), r = "";
      this.divRemTo(d, y, z);
      while (y.signum() > 0) {
        r = (a + z.intValue()).toString(b).substr(1) + r;
        y.divRemTo(d, y, z);
      }
      return z.intValue().toString(b) + r;
    }
    function bnpFromRadix(s, b) {
      this.fromInt(0);
      if (b == null)
        b = 10;
      var cs = this.chunkSize(b);
      var d = Math.pow(b, cs), mi = false, j = 0, w = 0;
      for (var i = 0; i < s.length; ++i) {
        var x = intAt(s, i);
        if (x < 0) {
          if (s.charAt(i) == "-" && this.signum() == 0)
            mi = true;
          continue;
        }
        w = b * w + x;
        if (++j >= cs) {
          this.dMultiply(d);
          this.dAddOffset(w, 0);
          j = 0;
          w = 0;
        }
      }
      if (j > 0) {
        this.dMultiply(Math.pow(b, j));
        this.dAddOffset(w, 0);
      }
      if (mi)
        BigInteger.ZERO.subTo(this, this);
    }
    function bnpFromNumber(a, b, c) {
      if ("number" == typeof b) {
        if (a < 2)
          this.fromInt(1);
        else {
          this.fromNumber(a, c);
          if (!this.testBit(a - 1))
            this.bitwiseTo(BigInteger.ONE.shiftLeft(a - 1), op_or, this);
          if (this.isEven())
            this.dAddOffset(1, 0);
          while (!this.isProbablePrime(b)) {
            this.dAddOffset(2, 0);
            if (this.bitLength() > a)
              this.subTo(BigInteger.ONE.shiftLeft(a - 1), this);
          }
        }
      } else {
        var x = new Array(), t = a & 7;
        x.length = (a >> 3) + 1;
        b.nextBytes(x);
        if (t > 0)
          x[0] &= (1 << t) - 1;
        else
          x[0] = 0;
        this.fromString(x, 256);
      }
    }
    function bnToByteArray() {
      var i = this.t, r = new Array();
      r[0] = this.s;
      var p = this.DB - i * this.DB % 8, d, k = 0;
      if (i-- > 0) {
        if (p < this.DB && (d = this.data[i] >> p) != (this.s & this.DM) >> p)
          r[k++] = d | this.s << this.DB - p;
        while (i >= 0) {
          if (p < 8) {
            d = (this.data[i] & (1 << p) - 1) << 8 - p;
            d |= this.data[--i] >> (p += this.DB - 8);
          } else {
            d = this.data[i] >> (p -= 8) & 255;
            if (p <= 0) {
              p += this.DB;
              --i;
            }
          }
          if ((d & 128) != 0)
            d |= -256;
          if (k == 0 && (this.s & 128) != (d & 128))
            ++k;
          if (k > 0 || d != this.s)
            r[k++] = d;
        }
      }
      return r;
    }
    function bnEquals(a) {
      return this.compareTo(a) == 0;
    }
    function bnMin(a) {
      return this.compareTo(a) < 0 ? this : a;
    }
    function bnMax(a) {
      return this.compareTo(a) > 0 ? this : a;
    }
    function bnpBitwiseTo(a, op, r) {
      var i, f, m = Math.min(a.t, this.t);
      for (i = 0; i < m; ++i)
        r.data[i] = op(this.data[i], a.data[i]);
      if (a.t < this.t) {
        f = a.s & this.DM;
        for (i = m; i < this.t; ++i)
          r.data[i] = op(this.data[i], f);
        r.t = this.t;
      } else {
        f = this.s & this.DM;
        for (i = m; i < a.t; ++i)
          r.data[i] = op(f, a.data[i]);
        r.t = a.t;
      }
      r.s = op(this.s, a.s);
      r.clamp();
    }
    function op_and(x, y) {
      return x & y;
    }
    function bnAnd(a) {
      var r = nbi();
      this.bitwiseTo(a, op_and, r);
      return r;
    }
    function op_or(x, y) {
      return x | y;
    }
    function bnOr(a) {
      var r = nbi();
      this.bitwiseTo(a, op_or, r);
      return r;
    }
    function op_xor(x, y) {
      return x ^ y;
    }
    function bnXor(a) {
      var r = nbi();
      this.bitwiseTo(a, op_xor, r);
      return r;
    }
    function op_andnot(x, y) {
      return x & ~y;
    }
    function bnAndNot(a) {
      var r = nbi();
      this.bitwiseTo(a, op_andnot, r);
      return r;
    }
    function bnNot() {
      var r = nbi();
      for (var i = 0; i < this.t; ++i)
        r.data[i] = this.DM & ~this.data[i];
      r.t = this.t;
      r.s = ~this.s;
      return r;
    }
    function bnShiftLeft(n) {
      var r = nbi();
      if (n < 0)
        this.rShiftTo(-n, r);
      else
        this.lShiftTo(n, r);
      return r;
    }
    function bnShiftRight(n) {
      var r = nbi();
      if (n < 0)
        this.lShiftTo(-n, r);
      else
        this.rShiftTo(n, r);
      return r;
    }
    function lbit(x) {
      if (x == 0)
        return -1;
      var r = 0;
      if ((x & 65535) == 0) {
        x >>= 16;
        r += 16;
      }
      if ((x & 255) == 0) {
        x >>= 8;
        r += 8;
      }
      if ((x & 15) == 0) {
        x >>= 4;
        r += 4;
      }
      if ((x & 3) == 0) {
        x >>= 2;
        r += 2;
      }
      if ((x & 1) == 0)
        ++r;
      return r;
    }
    function bnGetLowestSetBit() {
      for (var i = 0; i < this.t; ++i)
        if (this.data[i] != 0)
          return i * this.DB + lbit(this.data[i]);
      if (this.s < 0)
        return this.t * this.DB;
      return -1;
    }
    function cbit(x) {
      var r = 0;
      while (x != 0) {
        x &= x - 1;
        ++r;
      }
      return r;
    }
    function bnBitCount() {
      var r = 0, x = this.s & this.DM;
      for (var i = 0; i < this.t; ++i)
        r += cbit(this.data[i] ^ x);
      return r;
    }
    function bnTestBit(n) {
      var j = Math.floor(n / this.DB);
      if (j >= this.t)
        return this.s != 0;
      return (this.data[j] & 1 << n % this.DB) != 0;
    }
    function bnpChangeBit(n, op) {
      var r = BigInteger.ONE.shiftLeft(n);
      this.bitwiseTo(r, op, r);
      return r;
    }
    function bnSetBit(n) {
      return this.changeBit(n, op_or);
    }
    function bnClearBit(n) {
      return this.changeBit(n, op_andnot);
    }
    function bnFlipBit(n) {
      return this.changeBit(n, op_xor);
    }
    function bnpAddTo(a, r) {
      var i = 0, c = 0, m = Math.min(a.t, this.t);
      while (i < m) {
        c += this.data[i] + a.data[i];
        r.data[i++] = c & this.DM;
        c >>= this.DB;
      }
      if (a.t < this.t) {
        c += a.s;
        while (i < this.t) {
          c += this.data[i];
          r.data[i++] = c & this.DM;
          c >>= this.DB;
        }
        c += this.s;
      } else {
        c += this.s;
        while (i < a.t) {
          c += a.data[i];
          r.data[i++] = c & this.DM;
          c >>= this.DB;
        }
        c += a.s;
      }
      r.s = c < 0 ? -1 : 0;
      if (c > 0)
        r.data[i++] = c;
      else if (c < -1)
        r.data[i++] = this.DV + c;
      r.t = i;
      r.clamp();
    }
    function bnAdd(a) {
      var r = nbi();
      this.addTo(a, r);
      return r;
    }
    function bnSubtract(a) {
      var r = nbi();
      this.subTo(a, r);
      return r;
    }
    function bnMultiply(a) {
      var r = nbi();
      this.multiplyTo(a, r);
      return r;
    }
    function bnDivide(a) {
      var r = nbi();
      this.divRemTo(a, r, null);
      return r;
    }
    function bnRemainder(a) {
      var r = nbi();
      this.divRemTo(a, null, r);
      return r;
    }
    function bnDivideAndRemainder(a) {
      var q = nbi(), r = nbi();
      this.divRemTo(a, q, r);
      return new Array(q, r);
    }
    function bnpDMultiply(n) {
      this.data[this.t] = this.am(0, n - 1, this, 0, 0, this.t);
      ++this.t;
      this.clamp();
    }
    function bnpDAddOffset(n, w) {
      if (n == 0)
        return;
      while (this.t <= w)
        this.data[this.t++] = 0;
      this.data[w] += n;
      while (this.data[w] >= this.DV) {
        this.data[w] -= this.DV;
        if (++w >= this.t)
          this.data[this.t++] = 0;
        ++this.data[w];
      }
    }
    function NullExp() {
    }
    function nNop(x) {
      return x;
    }
    function nMulTo(x, y, r) {
      x.multiplyTo(y, r);
    }
    function nSqrTo(x, r) {
      x.squareTo(r);
    }
    NullExp.prototype.convert = nNop;
    NullExp.prototype.revert = nNop;
    NullExp.prototype.mulTo = nMulTo;
    NullExp.prototype.sqrTo = nSqrTo;
    function bnPow(e) {
      return this.exp(e, new NullExp());
    }
    function bnpMultiplyLowerTo(a, n, r) {
      var i = Math.min(this.t + a.t, n);
      r.s = 0;
      r.t = i;
      while (i > 0)
        r.data[--i] = 0;
      var j;
      for (j = r.t - this.t; i < j; ++i)
        r.data[i + this.t] = this.am(0, a.data[i], r, i, 0, this.t);
      for (j = Math.min(a.t, n); i < j; ++i)
        this.am(0, a.data[i], r, i, 0, n - i);
      r.clamp();
    }
    function bnpMultiplyUpperTo(a, n, r) {
      --n;
      var i = r.t = this.t + a.t - n;
      r.s = 0;
      while (--i >= 0)
        r.data[i] = 0;
      for (i = Math.max(n - this.t, 0); i < a.t; ++i)
        r.data[this.t + i - n] = this.am(n - i, a.data[i], r, 0, 0, this.t + i - n);
      r.clamp();
      r.drShiftTo(1, r);
    }
    function Barrett(m) {
      this.r2 = nbi();
      this.q3 = nbi();
      BigInteger.ONE.dlShiftTo(2 * m.t, this.r2);
      this.mu = this.r2.divide(m);
      this.m = m;
    }
    function barrettConvert(x) {
      if (x.s < 0 || x.t > 2 * this.m.t)
        return x.mod(this.m);
      else if (x.compareTo(this.m) < 0)
        return x;
      else {
        var r = nbi();
        x.copyTo(r);
        this.reduce(r);
        return r;
      }
    }
    function barrettRevert(x) {
      return x;
    }
    function barrettReduce(x) {
      x.drShiftTo(this.m.t - 1, this.r2);
      if (x.t > this.m.t + 1) {
        x.t = this.m.t + 1;
        x.clamp();
      }
      this.mu.multiplyUpperTo(this.r2, this.m.t + 1, this.q3);
      this.m.multiplyLowerTo(this.q3, this.m.t + 1, this.r2);
      while (x.compareTo(this.r2) < 0)
        x.dAddOffset(1, this.m.t + 1);
      x.subTo(this.r2, x);
      while (x.compareTo(this.m) >= 0)
        x.subTo(this.m, x);
    }
    function barrettSqrTo(x, r) {
      x.squareTo(r);
      this.reduce(r);
    }
    function barrettMulTo(x, y, r) {
      x.multiplyTo(y, r);
      this.reduce(r);
    }
    Barrett.prototype.convert = barrettConvert;
    Barrett.prototype.revert = barrettRevert;
    Barrett.prototype.reduce = barrettReduce;
    Barrett.prototype.mulTo = barrettMulTo;
    Barrett.prototype.sqrTo = barrettSqrTo;
    function bnModPow(e, m) {
      var i = e.bitLength(), k, r = nbv(1), z;
      if (i <= 0)
        return r;
      else if (i < 18)
        k = 1;
      else if (i < 48)
        k = 3;
      else if (i < 144)
        k = 4;
      else if (i < 768)
        k = 5;
      else
        k = 6;
      if (i < 8)
        z = new Classic(m);
      else if (m.isEven())
        z = new Barrett(m);
      else
        z = new Montgomery(m);
      var g = new Array(), n = 3, k1 = k - 1, km = (1 << k) - 1;
      g[1] = z.convert(this);
      if (k > 1) {
        var g2 = nbi();
        z.sqrTo(g[1], g2);
        while (n <= km) {
          g[n] = nbi();
          z.mulTo(g2, g[n - 2], g[n]);
          n += 2;
        }
      }
      var j = e.t - 1, w, is1 = true, r2 = nbi(), t;
      i = nbits(e.data[j]) - 1;
      while (j >= 0) {
        if (i >= k1)
          w = e.data[j] >> i - k1 & km;
        else {
          w = (e.data[j] & (1 << i + 1) - 1) << k1 - i;
          if (j > 0)
            w |= e.data[j - 1] >> this.DB + i - k1;
        }
        n = k;
        while ((w & 1) == 0) {
          w >>= 1;
          --n;
        }
        if ((i -= n) < 0) {
          i += this.DB;
          --j;
        }
        if (is1) {
          g[w].copyTo(r);
          is1 = false;
        } else {
          while (n > 1) {
            z.sqrTo(r, r2);
            z.sqrTo(r2, r);
            n -= 2;
          }
          if (n > 0)
            z.sqrTo(r, r2);
          else {
            t = r;
            r = r2;
            r2 = t;
          }
          z.mulTo(r2, g[w], r);
        }
        while (j >= 0 && (e.data[j] & 1 << i) == 0) {
          z.sqrTo(r, r2);
          t = r;
          r = r2;
          r2 = t;
          if (--i < 0) {
            i = this.DB - 1;
            --j;
          }
        }
      }
      return z.revert(r);
    }
    function bnGCD(a) {
      var x = this.s < 0 ? this.negate() : this.clone();
      var y = a.s < 0 ? a.negate() : a.clone();
      if (x.compareTo(y) < 0) {
        var t = x;
        x = y;
        y = t;
      }
      var i = x.getLowestSetBit(), g = y.getLowestSetBit();
      if (g < 0)
        return x;
      if (i < g)
        g = i;
      if (g > 0) {
        x.rShiftTo(g, x);
        y.rShiftTo(g, y);
      }
      while (x.signum() > 0) {
        if ((i = x.getLowestSetBit()) > 0)
          x.rShiftTo(i, x);
        if ((i = y.getLowestSetBit()) > 0)
          y.rShiftTo(i, y);
        if (x.compareTo(y) >= 0) {
          x.subTo(y, x);
          x.rShiftTo(1, x);
        } else {
          y.subTo(x, y);
          y.rShiftTo(1, y);
        }
      }
      if (g > 0)
        y.lShiftTo(g, y);
      return y;
    }
    function bnpModInt(n) {
      if (n <= 0)
        return 0;
      var d = this.DV % n, r = this.s < 0 ? n - 1 : 0;
      if (this.t > 0)
        if (d == 0)
          r = this.data[0] % n;
        else
          for (var i = this.t - 1; i >= 0; --i)
            r = (d * r + this.data[i]) % n;
      return r;
    }
    function bnModInverse(m) {
      var ac = m.isEven();
      if (this.isEven() && ac || m.signum() == 0)
        return BigInteger.ZERO;
      var u = m.clone(), v = this.clone();
      var a = nbv(1), b = nbv(0), c = nbv(0), d = nbv(1);
      while (u.signum() != 0) {
        while (u.isEven()) {
          u.rShiftTo(1, u);
          if (ac) {
            if (!a.isEven() || !b.isEven()) {
              a.addTo(this, a);
              b.subTo(m, b);
            }
            a.rShiftTo(1, a);
          } else if (!b.isEven())
            b.subTo(m, b);
          b.rShiftTo(1, b);
        }
        while (v.isEven()) {
          v.rShiftTo(1, v);
          if (ac) {
            if (!c.isEven() || !d.isEven()) {
              c.addTo(this, c);
              d.subTo(m, d);
            }
            c.rShiftTo(1, c);
          } else if (!d.isEven())
            d.subTo(m, d);
          d.rShiftTo(1, d);
        }
        if (u.compareTo(v) >= 0) {
          u.subTo(v, u);
          if (ac)
            a.subTo(c, a);
          b.subTo(d, b);
        } else {
          v.subTo(u, v);
          if (ac)
            c.subTo(a, c);
          d.subTo(b, d);
        }
      }
      if (v.compareTo(BigInteger.ONE) != 0)
        return BigInteger.ZERO;
      if (d.compareTo(m) >= 0)
        return d.subtract(m);
      if (d.signum() < 0)
        d.addTo(m, d);
      else
        return d;
      if (d.signum() < 0)
        return d.add(m);
      else
        return d;
    }
    var lowprimes = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53, 59, 61, 67, 71, 73, 79, 83, 89, 97, 101, 103, 107, 109, 113, 127, 131, 137, 139, 149, 151, 157, 163, 167, 173, 179, 181, 191, 193, 197, 199, 211, 223, 227, 229, 233, 239, 241, 251, 257, 263, 269, 271, 277, 281, 283, 293, 307, 311, 313, 317, 331, 337, 347, 349, 353, 359, 367, 373, 379, 383, 389, 397, 401, 409, 419, 421, 431, 433, 439, 443, 449, 457, 461, 463, 467, 479, 487, 491, 499, 503, 509];
    var lplim = (1 << 26) / lowprimes[lowprimes.length - 1];
    function bnIsProbablePrime(t) {
      var i, x = this.abs();
      if (x.t == 1 && x.data[0] <= lowprimes[lowprimes.length - 1]) {
        for (i = 0; i < lowprimes.length; ++i)
          if (x.data[0] == lowprimes[i])
            return true;
        return false;
      }
      if (x.isEven())
        return false;
      i = 1;
      while (i < lowprimes.length) {
        var m = lowprimes[i], j = i + 1;
        while (j < lowprimes.length && m < lplim)
          m *= lowprimes[j++];
        m = x.modInt(m);
        while (i < j)
          if (m % lowprimes[i++] == 0)
            return false;
      }
      return x.millerRabin(t);
    }
    function bnpMillerRabin(t) {
      var n1 = this.subtract(BigInteger.ONE);
      var k = n1.getLowestSetBit();
      if (k <= 0)
        return false;
      var r = n1.shiftRight(k);
      var prng = bnGetPrng();
      var a;
      for (var i = 0; i < t; ++i) {
        do {
          a = new BigInteger(this.bitLength(), prng);
        } while (a.compareTo(BigInteger.ONE) <= 0 || a.compareTo(n1) >= 0);
        var y = a.modPow(r, this);
        if (y.compareTo(BigInteger.ONE) != 0 && y.compareTo(n1) != 0) {
          var j = 1;
          while (j++ < k && y.compareTo(n1) != 0) {
            y = y.modPowInt(2, this);
            if (y.compareTo(BigInteger.ONE) == 0)
              return false;
          }
          if (y.compareTo(n1) != 0)
            return false;
        }
      }
      return true;
    }
    function bnGetPrng() {
      return {
        // x is an array to fill with bytes
        nextBytes: function(x) {
          for (var i = 0; i < x.length; ++i) {
            x[i] = Math.floor(Math.random() * 256);
          }
        }
      };
    }
    BigInteger.prototype.chunkSize = bnpChunkSize;
    BigInteger.prototype.toRadix = bnpToRadix;
    BigInteger.prototype.fromRadix = bnpFromRadix;
    BigInteger.prototype.fromNumber = bnpFromNumber;
    BigInteger.prototype.bitwiseTo = bnpBitwiseTo;
    BigInteger.prototype.changeBit = bnpChangeBit;
    BigInteger.prototype.addTo = bnpAddTo;
    BigInteger.prototype.dMultiply = bnpDMultiply;
    BigInteger.prototype.dAddOffset = bnpDAddOffset;
    BigInteger.prototype.multiplyLowerTo = bnpMultiplyLowerTo;
    BigInteger.prototype.multiplyUpperTo = bnpMultiplyUpperTo;
    BigInteger.prototype.modInt = bnpModInt;
    BigInteger.prototype.millerRabin = bnpMillerRabin;
    BigInteger.prototype.clone = bnClone;
    BigInteger.prototype.intValue = bnIntValue;
    BigInteger.prototype.byteValue = bnByteValue;
    BigInteger.prototype.shortValue = bnShortValue;
    BigInteger.prototype.signum = bnSigNum;
    BigInteger.prototype.toByteArray = bnToByteArray;
    BigInteger.prototype.equals = bnEquals;
    BigInteger.prototype.min = bnMin;
    BigInteger.prototype.max = bnMax;
    BigInteger.prototype.and = bnAnd;
    BigInteger.prototype.or = bnOr;
    BigInteger.prototype.xor = bnXor;
    BigInteger.prototype.andNot = bnAndNot;
    BigInteger.prototype.not = bnNot;
    BigInteger.prototype.shiftLeft = bnShiftLeft;
    BigInteger.prototype.shiftRight = bnShiftRight;
    BigInteger.prototype.getLowestSetBit = bnGetLowestSetBit;
    BigInteger.prototype.bitCount = bnBitCount;
    BigInteger.prototype.testBit = bnTestBit;
    BigInteger.prototype.setBit = bnSetBit;
    BigInteger.prototype.clearBit = bnClearBit;
    BigInteger.prototype.flipBit = bnFlipBit;
    BigInteger.prototype.add = bnAdd;
    BigInteger.prototype.subtract = bnSubtract;
    BigInteger.prototype.multiply = bnMultiply;
    BigInteger.prototype.divide = bnDivide;
    BigInteger.prototype.remainder = bnRemainder;
    BigInteger.prototype.divideAndRemainder = bnDivideAndRemainder;
    BigInteger.prototype.modPow = bnModPow;
    BigInteger.prototype.modInverse = bnModInverse;
    BigInteger.prototype.pow = bnPow;
    BigInteger.prototype.gcd = bnGCD;
    BigInteger.prototype.isProbablePrime = bnIsProbablePrime;
  }
});

// ../../node_modules/node-forge/lib/sha1.js
var require_sha1 = __commonJS({
  "../../node_modules/node-forge/lib/sha1.js"(exports2, module2) {
    var forge6 = require_forge();
    require_md();
    require_util();
    var sha1 = module2.exports = forge6.sha1 = forge6.sha1 || {};
    forge6.md.sha1 = forge6.md.algorithms.sha1 = sha1;
    sha1.create = function() {
      if (!_initialized) {
        _init();
      }
      var _state = null;
      var _input = forge6.util.createBuffer();
      var _w = new Array(80);
      var md = {
        algorithm: "sha1",
        blockLength: 64,
        digestLength: 20,
        // 56-bit length of message so far (does not including padding)
        messageLength: 0,
        // true message length
        fullMessageLength: null,
        // size of message length in bytes
        messageLengthSize: 8
      };
      md.start = function() {
        md.messageLength = 0;
        md.fullMessageLength = md.messageLength64 = [];
        var int32s = md.messageLengthSize / 4;
        for (var i = 0; i < int32s; ++i) {
          md.fullMessageLength.push(0);
        }
        _input = forge6.util.createBuffer();
        _state = {
          h0: 1732584193,
          h1: 4023233417,
          h2: 2562383102,
          h3: 271733878,
          h4: 3285377520
        };
        return md;
      };
      md.start();
      md.update = function(msg, encoding) {
        if (encoding === "utf8") {
          msg = forge6.util.encodeUtf8(msg);
        }
        var len = msg.length;
        md.messageLength += len;
        len = [len / 4294967296 >>> 0, len >>> 0];
        for (var i = md.fullMessageLength.length - 1; i >= 0; --i) {
          md.fullMessageLength[i] += len[1];
          len[1] = len[0] + (md.fullMessageLength[i] / 4294967296 >>> 0);
          md.fullMessageLength[i] = md.fullMessageLength[i] >>> 0;
          len[0] = len[1] / 4294967296 >>> 0;
        }
        _input.putBytes(msg);
        _update(_state, _w, _input);
        if (_input.read > 2048 || _input.length() === 0) {
          _input.compact();
        }
        return md;
      };
      md.digest = function() {
        var finalBlock = forge6.util.createBuffer();
        finalBlock.putBytes(_input.bytes());
        var remaining = md.fullMessageLength[md.fullMessageLength.length - 1] + md.messageLengthSize;
        var overflow = remaining & md.blockLength - 1;
        finalBlock.putBytes(_padding.substr(0, md.blockLength - overflow));
        var next, carry;
        var bits2 = md.fullMessageLength[0] * 8;
        for (var i = 0; i < md.fullMessageLength.length - 1; ++i) {
          next = md.fullMessageLength[i + 1] * 8;
          carry = next / 4294967296 >>> 0;
          bits2 += carry;
          finalBlock.putInt32(bits2 >>> 0);
          bits2 = next >>> 0;
        }
        finalBlock.putInt32(bits2);
        var s2 = {
          h0: _state.h0,
          h1: _state.h1,
          h2: _state.h2,
          h3: _state.h3,
          h4: _state.h4
        };
        _update(s2, _w, finalBlock);
        var rval = forge6.util.createBuffer();
        rval.putInt32(s2.h0);
        rval.putInt32(s2.h1);
        rval.putInt32(s2.h2);
        rval.putInt32(s2.h3);
        rval.putInt32(s2.h4);
        return rval;
      };
      return md;
    };
    var _padding = null;
    var _initialized = false;
    function _init() {
      _padding = String.fromCharCode(128);
      _padding += forge6.util.fillString(String.fromCharCode(0), 64);
      _initialized = true;
    }
    function _update(s, w, bytes) {
      var t, a, b, c, d, e, f, i;
      var len = bytes.length();
      while (len >= 64) {
        a = s.h0;
        b = s.h1;
        c = s.h2;
        d = s.h3;
        e = s.h4;
        for (i = 0; i < 16; ++i) {
          t = bytes.getInt32();
          w[i] = t;
          f = d ^ b & (c ^ d);
          t = (a << 5 | a >>> 27) + f + e + 1518500249 + t;
          e = d;
          d = c;
          c = (b << 30 | b >>> 2) >>> 0;
          b = a;
          a = t;
        }
        for (; i < 20; ++i) {
          t = w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16];
          t = t << 1 | t >>> 31;
          w[i] = t;
          f = d ^ b & (c ^ d);
          t = (a << 5 | a >>> 27) + f + e + 1518500249 + t;
          e = d;
          d = c;
          c = (b << 30 | b >>> 2) >>> 0;
          b = a;
          a = t;
        }
        for (; i < 32; ++i) {
          t = w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16];
          t = t << 1 | t >>> 31;
          w[i] = t;
          f = b ^ c ^ d;
          t = (a << 5 | a >>> 27) + f + e + 1859775393 + t;
          e = d;
          d = c;
          c = (b << 30 | b >>> 2) >>> 0;
          b = a;
          a = t;
        }
        for (; i < 40; ++i) {
          t = w[i - 6] ^ w[i - 16] ^ w[i - 28] ^ w[i - 32];
          t = t << 2 | t >>> 30;
          w[i] = t;
          f = b ^ c ^ d;
          t = (a << 5 | a >>> 27) + f + e + 1859775393 + t;
          e = d;
          d = c;
          c = (b << 30 | b >>> 2) >>> 0;
          b = a;
          a = t;
        }
        for (; i < 60; ++i) {
          t = w[i - 6] ^ w[i - 16] ^ w[i - 28] ^ w[i - 32];
          t = t << 2 | t >>> 30;
          w[i] = t;
          f = b & c | d & (b ^ c);
          t = (a << 5 | a >>> 27) + f + e + 2400959708 + t;
          e = d;
          d = c;
          c = (b << 30 | b >>> 2) >>> 0;
          b = a;
          a = t;
        }
        for (; i < 80; ++i) {
          t = w[i - 6] ^ w[i - 16] ^ w[i - 28] ^ w[i - 32];
          t = t << 2 | t >>> 30;
          w[i] = t;
          f = b ^ c ^ d;
          t = (a << 5 | a >>> 27) + f + e + 3395469782 + t;
          e = d;
          d = c;
          c = (b << 30 | b >>> 2) >>> 0;
          b = a;
          a = t;
        }
        s.h0 = s.h0 + a | 0;
        s.h1 = s.h1 + b | 0;
        s.h2 = s.h2 + c | 0;
        s.h3 = s.h3 + d | 0;
        s.h4 = s.h4 + e | 0;
        len -= 64;
      }
    }
  }
});

// ../../node_modules/node-forge/lib/pkcs1.js
var require_pkcs1 = __commonJS({
  "../../node_modules/node-forge/lib/pkcs1.js"(exports2, module2) {
    var forge6 = require_forge();
    require_util();
    require_random();
    require_sha1();
    var pkcs1 = module2.exports = forge6.pkcs1 = forge6.pkcs1 || {};
    pkcs1.encode_rsa_oaep = function(key, message2, options) {
      var label;
      var seed;
      var md;
      var mgf1Md;
      if (typeof options === "string") {
        label = options;
        seed = arguments[3] || void 0;
        md = arguments[4] || void 0;
      } else if (options) {
        label = options.label || void 0;
        seed = options.seed || void 0;
        md = options.md || void 0;
        if (options.mgf1 && options.mgf1.md) {
          mgf1Md = options.mgf1.md;
        }
      }
      if (!md) {
        md = forge6.md.sha1.create();
      } else {
        md.start();
      }
      if (!mgf1Md) {
        mgf1Md = md;
      }
      var keyLength = Math.ceil(key.n.bitLength() / 8);
      var maxLength = keyLength - 2 * md.digestLength - 2;
      if (message2.length > maxLength) {
        var error = new Error("RSAES-OAEP input message length is too long.");
        error.length = message2.length;
        error.maxLength = maxLength;
        throw error;
      }
      if (!label) {
        label = "";
      }
      md.update(label, "raw");
      var lHash = md.digest();
      var PS = "";
      var PS_length = maxLength - message2.length;
      for (var i = 0; i < PS_length; i++) {
        PS += "\0";
      }
      var DB = lHash.getBytes() + PS + "" + message2;
      if (!seed) {
        seed = forge6.random.getBytes(md.digestLength);
      } else if (seed.length !== md.digestLength) {
        var error = new Error("Invalid RSAES-OAEP seed. The seed length must match the digest length.");
        error.seedLength = seed.length;
        error.digestLength = md.digestLength;
        throw error;
      }
      var dbMask = rsa_mgf1(seed, keyLength - md.digestLength - 1, mgf1Md);
      var maskedDB = forge6.util.xorBytes(DB, dbMask, DB.length);
      var seedMask = rsa_mgf1(maskedDB, md.digestLength, mgf1Md);
      var maskedSeed = forge6.util.xorBytes(seed, seedMask, seed.length);
      return "\0" + maskedSeed + maskedDB;
    };
    pkcs1.decode_rsa_oaep = function(key, em, options) {
      var label;
      var md;
      var mgf1Md;
      if (typeof options === "string") {
        label = options;
        md = arguments[3] || void 0;
      } else if (options) {
        label = options.label || void 0;
        md = options.md || void 0;
        if (options.mgf1 && options.mgf1.md) {
          mgf1Md = options.mgf1.md;
        }
      }
      var keyLength = Math.ceil(key.n.bitLength() / 8);
      if (em.length !== keyLength) {
        var error = new Error("RSAES-OAEP encoded message length is invalid.");
        error.length = em.length;
        error.expectedLength = keyLength;
        throw error;
      }
      if (md === void 0) {
        md = forge6.md.sha1.create();
      } else {
        md.start();
      }
      if (!mgf1Md) {
        mgf1Md = md;
      }
      if (keyLength < 2 * md.digestLength + 2) {
        throw new Error("RSAES-OAEP key is too short for the hash function.");
      }
      if (!label) {
        label = "";
      }
      md.update(label, "raw");
      var lHash = md.digest().getBytes();
      var y = em.charAt(0);
      var maskedSeed = em.substring(1, md.digestLength + 1);
      var maskedDB = em.substring(1 + md.digestLength);
      var seedMask = rsa_mgf1(maskedDB, md.digestLength, mgf1Md);
      var seed = forge6.util.xorBytes(maskedSeed, seedMask, maskedSeed.length);
      var dbMask = rsa_mgf1(seed, keyLength - md.digestLength - 1, mgf1Md);
      var db = forge6.util.xorBytes(maskedDB, dbMask, maskedDB.length);
      var lHashPrime = db.substring(0, md.digestLength);
      var error = y !== "\0";
      for (var i = 0; i < md.digestLength; ++i) {
        error |= lHash.charAt(i) !== lHashPrime.charAt(i);
      }
      var in_ps = 1;
      var index = md.digestLength;
      for (var j = md.digestLength; j < db.length; j++) {
        var code3 = db.charCodeAt(j);
        var is_0 = code3 & 1 ^ 1;
        var error_mask = in_ps ? 65534 : 0;
        error |= code3 & error_mask;
        in_ps = in_ps & is_0;
        index += in_ps;
      }
      if (error || db.charCodeAt(index) !== 1) {
        throw new Error("Invalid RSAES-OAEP padding.");
      }
      return db.substring(index + 1);
    };
    function rsa_mgf1(seed, maskLength, hash) {
      if (!hash) {
        hash = forge6.md.sha1.create();
      }
      var t = "";
      var count = Math.ceil(maskLength / hash.digestLength);
      for (var i = 0; i < count; ++i) {
        var c = String.fromCharCode(
          i >> 24 & 255,
          i >> 16 & 255,
          i >> 8 & 255,
          i & 255
        );
        hash.start();
        hash.update(seed + c);
        t += hash.digest().getBytes();
      }
      return t.substring(0, maskLength);
    }
  }
});

// ../../node_modules/node-forge/lib/prime.js
var require_prime = __commonJS({
  "../../node_modules/node-forge/lib/prime.js"(exports2, module2) {
    var forge6 = require_forge();
    require_util();
    require_jsbn();
    require_random();
    (function() {
      if (forge6.prime) {
        module2.exports = forge6.prime;
        return;
      }
      var prime = module2.exports = forge6.prime = forge6.prime || {};
      var BigInteger = forge6.jsbn.BigInteger;
      var GCD_30_DELTA = [6, 4, 2, 4, 2, 4, 6, 2];
      var THIRTY = new BigInteger(null);
      THIRTY.fromInt(30);
      var op_or = function(x, y) {
        return x | y;
      };
      prime.generateProbablePrime = function(bits2, options, callback) {
        if (typeof options === "function") {
          callback = options;
          options = {};
        }
        options = options || {};
        var algorithm = options.algorithm || "PRIMEINC";
        if (typeof algorithm === "string") {
          algorithm = { name: algorithm };
        }
        algorithm.options = algorithm.options || {};
        var prng = options.prng || forge6.random;
        var rng = {
          // x is an array to fill with bytes
          nextBytes: function(x) {
            var b = prng.getBytesSync(x.length);
            for (var i = 0; i < x.length; ++i) {
              x[i] = b.charCodeAt(i);
            }
          }
        };
        if (algorithm.name === "PRIMEINC") {
          return primeincFindPrime(bits2, rng, algorithm.options, callback);
        }
        throw new Error("Invalid prime generation algorithm: " + algorithm.name);
      };
      function primeincFindPrime(bits2, rng, options, callback) {
        if ("workers" in options) {
          return primeincFindPrimeWithWorkers(bits2, rng, options, callback);
        }
        return primeincFindPrimeWithoutWorkers(bits2, rng, options, callback);
      }
      function primeincFindPrimeWithoutWorkers(bits2, rng, options, callback) {
        var num = generateRandom(bits2, rng);
        var deltaIdx = 0;
        var mrTests = getMillerRabinTests(num.bitLength());
        if ("millerRabinTests" in options) {
          mrTests = options.millerRabinTests;
        }
        var maxBlockTime = 10;
        if ("maxBlockTime" in options) {
          maxBlockTime = options.maxBlockTime;
        }
        _primeinc(num, bits2, rng, deltaIdx, mrTests, maxBlockTime, callback);
      }
      function _primeinc(num, bits2, rng, deltaIdx, mrTests, maxBlockTime, callback) {
        var start = +/* @__PURE__ */ new Date();
        do {
          if (num.bitLength() > bits2) {
            num = generateRandom(bits2, rng);
          }
          if (num.isProbablePrime(mrTests)) {
            return callback(null, num);
          }
          num.dAddOffset(GCD_30_DELTA[deltaIdx++ % 8], 0);
        } while (maxBlockTime < 0 || +/* @__PURE__ */ new Date() - start < maxBlockTime);
        forge6.util.setImmediate(function() {
          _primeinc(num, bits2, rng, deltaIdx, mrTests, maxBlockTime, callback);
        });
      }
      function primeincFindPrimeWithWorkers(bits2, rng, options, callback) {
        if (typeof Worker === "undefined") {
          return primeincFindPrimeWithoutWorkers(bits2, rng, options, callback);
        }
        var num = generateRandom(bits2, rng);
        var numWorkers = options.workers;
        var workLoad = options.workLoad || 100;
        var range = workLoad * 30 / 8;
        var workerScript = options.workerScript || "forge/prime.worker.js";
        if (numWorkers === -1) {
          return forge6.util.estimateCores(function(err, cores) {
            if (err) {
              cores = 2;
            }
            numWorkers = cores - 1;
            generate();
          });
        }
        generate();
        function generate() {
          numWorkers = Math.max(1, numWorkers);
          var workers = [];
          for (var i = 0; i < numWorkers; ++i) {
            workers[i] = new Worker(workerScript);
          }
          var running = numWorkers;
          for (var i = 0; i < numWorkers; ++i) {
            workers[i].addEventListener("message", workerMessage);
          }
          var found = false;
          function workerMessage(e) {
            if (found) {
              return;
            }
            --running;
            var data = e.data;
            if (data.found) {
              for (var i2 = 0; i2 < workers.length; ++i2) {
                workers[i2].terminate();
              }
              found = true;
              return callback(null, new BigInteger(data.prime, 16));
            }
            if (num.bitLength() > bits2) {
              num = generateRandom(bits2, rng);
            }
            var hex = num.toString(16);
            e.target.postMessage({
              hex,
              workLoad
            });
            num.dAddOffset(range, 0);
          }
        }
      }
      function generateRandom(bits2, rng) {
        var num = new BigInteger(bits2, rng);
        var bits1 = bits2 - 1;
        if (!num.testBit(bits1)) {
          num.bitwiseTo(BigInteger.ONE.shiftLeft(bits1), op_or, num);
        }
        num.dAddOffset(31 - num.mod(THIRTY).byteValue(), 0);
        return num;
      }
      function getMillerRabinTests(bits2) {
        if (bits2 <= 100)
          return 27;
        if (bits2 <= 150)
          return 18;
        if (bits2 <= 200)
          return 15;
        if (bits2 <= 250)
          return 12;
        if (bits2 <= 300)
          return 9;
        if (bits2 <= 350)
          return 8;
        if (bits2 <= 400)
          return 7;
        if (bits2 <= 500)
          return 6;
        if (bits2 <= 600)
          return 5;
        if (bits2 <= 800)
          return 4;
        if (bits2 <= 1250)
          return 3;
        return 2;
      }
    })();
  }
});

// ../../node_modules/node-forge/lib/rsa.js
var require_rsa = __commonJS({
  "../../node_modules/node-forge/lib/rsa.js"(exports2, module2) {
    var forge6 = require_forge();
    require_asn1();
    require_jsbn();
    require_oids();
    require_pkcs1();
    require_prime();
    require_random();
    require_util();
    if (typeof BigInteger === "undefined") {
      BigInteger = forge6.jsbn.BigInteger;
    }
    var BigInteger;
    var _crypto = forge6.util.isNodejs ? require_crypto() : null;
    var asn1 = forge6.asn1;
    var util2 = forge6.util;
    forge6.pki = forge6.pki || {};
    module2.exports = forge6.pki.rsa = forge6.rsa = forge6.rsa || {};
    var pki = forge6.pki;
    var GCD_30_DELTA = [6, 4, 2, 4, 2, 4, 6, 2];
    var privateKeyValidator = {
      // PrivateKeyInfo
      name: "PrivateKeyInfo",
      tagClass: asn1.Class.UNIVERSAL,
      type: asn1.Type.SEQUENCE,
      constructed: true,
      value: [{
        // Version (INTEGER)
        name: "PrivateKeyInfo.version",
        tagClass: asn1.Class.UNIVERSAL,
        type: asn1.Type.INTEGER,
        constructed: false,
        capture: "privateKeyVersion"
      }, {
        // privateKeyAlgorithm
        name: "PrivateKeyInfo.privateKeyAlgorithm",
        tagClass: asn1.Class.UNIVERSAL,
        type: asn1.Type.SEQUENCE,
        constructed: true,
        value: [{
          name: "AlgorithmIdentifier.algorithm",
          tagClass: asn1.Class.UNIVERSAL,
          type: asn1.Type.OID,
          constructed: false,
          capture: "privateKeyOid"
        }]
      }, {
        // PrivateKey
        name: "PrivateKeyInfo",
        tagClass: asn1.Class.UNIVERSAL,
        type: asn1.Type.OCTETSTRING,
        constructed: false,
        capture: "privateKey"
      }]
    };
    var rsaPrivateKeyValidator = {
      // RSAPrivateKey
      name: "RSAPrivateKey",
      tagClass: asn1.Class.UNIVERSAL,
      type: asn1.Type.SEQUENCE,
      constructed: true,
      value: [{
        // Version (INTEGER)
        name: "RSAPrivateKey.version",
        tagClass: asn1.Class.UNIVERSAL,
        type: asn1.Type.INTEGER,
        constructed: false,
        capture: "privateKeyVersion"
      }, {
        // modulus (n)
        name: "RSAPrivateKey.modulus",
        tagClass: asn1.Class.UNIVERSAL,
        type: asn1.Type.INTEGER,
        constructed: false,
        capture: "privateKeyModulus"
      }, {
        // publicExponent (e)
        name: "RSAPrivateKey.publicExponent",
        tagClass: asn1.Class.UNIVERSAL,
        type: asn1.Type.INTEGER,
        constructed: false,
        capture: "privateKeyPublicExponent"
      }, {
        // privateExponent (d)
        name: "RSAPrivateKey.privateExponent",
        tagClass: asn1.Class.UNIVERSAL,
        type: asn1.Type.INTEGER,
        constructed: false,
        capture: "privateKeyPrivateExponent"
      }, {
        // prime1 (p)
        name: "RSAPrivateKey.prime1",
        tagClass: asn1.Class.UNIVERSAL,
        type: asn1.Type.INTEGER,
        constructed: false,
        capture: "privateKeyPrime1"
      }, {
        // prime2 (q)
        name: "RSAPrivateKey.prime2",
        tagClass: asn1.Class.UNIVERSAL,
        type: asn1.Type.INTEGER,
        constructed: false,
        capture: "privateKeyPrime2"
      }, {
        // exponent1 (d mod (p-1))
        name: "RSAPrivateKey.exponent1",
        tagClass: asn1.Class.UNIVERSAL,
        type: asn1.Type.INTEGER,
        constructed: false,
        capture: "privateKeyExponent1"
      }, {
        // exponent2 (d mod (q-1))
        name: "RSAPrivateKey.exponent2",
        tagClass: asn1.Class.UNIVERSAL,
        type: asn1.Type.INTEGER,
        constructed: false,
        capture: "privateKeyExponent2"
      }, {
        // coefficient ((inverse of q) mod p)
        name: "RSAPrivateKey.coefficient",
        tagClass: asn1.Class.UNIVERSAL,
        type: asn1.Type.INTEGER,
        constructed: false,
        capture: "privateKeyCoefficient"
      }]
    };
    var rsaPublicKeyValidator = {
      // RSAPublicKey
      name: "RSAPublicKey",
      tagClass: asn1.Class.UNIVERSAL,
      type: asn1.Type.SEQUENCE,
      constructed: true,
      value: [{
        // modulus (n)
        name: "RSAPublicKey.modulus",
        tagClass: asn1.Class.UNIVERSAL,
        type: asn1.Type.INTEGER,
        constructed: false,
        capture: "publicKeyModulus"
      }, {
        // publicExponent (e)
        name: "RSAPublicKey.exponent",
        tagClass: asn1.Class.UNIVERSAL,
        type: asn1.Type.INTEGER,
        constructed: false,
        capture: "publicKeyExponent"
      }]
    };
    var publicKeyValidator = forge6.pki.rsa.publicKeyValidator = {
      name: "SubjectPublicKeyInfo",
      tagClass: asn1.Class.UNIVERSAL,
      type: asn1.Type.SEQUENCE,
      constructed: true,
      captureAsn1: "subjectPublicKeyInfo",
      value: [{
        name: "SubjectPublicKeyInfo.AlgorithmIdentifier",
        tagClass: asn1.Class.UNIVERSAL,
        type: asn1.Type.SEQUENCE,
        constructed: true,
        value: [{
          name: "AlgorithmIdentifier.algorithm",
          tagClass: asn1.Class.UNIVERSAL,
          type: asn1.Type.OID,
          constructed: false,
          capture: "publicKeyOid"
        }]
      }, {
        // subjectPublicKey
        name: "SubjectPublicKeyInfo.subjectPublicKey",
        tagClass: asn1.Class.UNIVERSAL,
        type: asn1.Type.BITSTRING,
        constructed: false,
        value: [{
          // RSAPublicKey
          name: "SubjectPublicKeyInfo.subjectPublicKey.RSAPublicKey",
          tagClass: asn1.Class.UNIVERSAL,
          type: asn1.Type.SEQUENCE,
          constructed: true,
          optional: true,
          captureAsn1: "rsaPublicKey"
        }]
      }]
    };
    var digestInfoValidator = {
      name: "DigestInfo",
      tagClass: asn1.Class.UNIVERSAL,
      type: asn1.Type.SEQUENCE,
      constructed: true,
      value: [{
        name: "DigestInfo.DigestAlgorithm",
        tagClass: asn1.Class.UNIVERSAL,
        type: asn1.Type.SEQUENCE,
        constructed: true,
        value: [{
          name: "DigestInfo.DigestAlgorithm.algorithmIdentifier",
          tagClass: asn1.Class.UNIVERSAL,
          type: asn1.Type.OID,
          constructed: false,
          capture: "algorithmIdentifier"
        }, {
          // NULL paramters
          name: "DigestInfo.DigestAlgorithm.parameters",
          tagClass: asn1.Class.UNIVERSAL,
          type: asn1.Type.NULL,
          // captured only to check existence for md2 and md5
          capture: "parameters",
          optional: true,
          constructed: false
        }]
      }, {
        // digest
        name: "DigestInfo.digest",
        tagClass: asn1.Class.UNIVERSAL,
        type: asn1.Type.OCTETSTRING,
        constructed: false,
        capture: "digest"
      }]
    };
    var emsaPkcs1v15encode = function(md) {
      var oid;
      if (md.algorithm in pki.oids) {
        oid = pki.oids[md.algorithm];
      } else {
        var error = new Error("Unknown message digest algorithm.");
        error.algorithm = md.algorithm;
        throw error;
      }
      var oidBytes = asn1.oidToDer(oid).getBytes();
      var digestInfo = asn1.create(
        asn1.Class.UNIVERSAL,
        asn1.Type.SEQUENCE,
        true,
        []
      );
      var digestAlgorithm = asn1.create(
        asn1.Class.UNIVERSAL,
        asn1.Type.SEQUENCE,
        true,
        []
      );
      digestAlgorithm.value.push(asn1.create(
        asn1.Class.UNIVERSAL,
        asn1.Type.OID,
        false,
        oidBytes
      ));
      digestAlgorithm.value.push(asn1.create(
        asn1.Class.UNIVERSAL,
        asn1.Type.NULL,
        false,
        ""
      ));
      var digest3 = asn1.create(
        asn1.Class.UNIVERSAL,
        asn1.Type.OCTETSTRING,
        false,
        md.digest().getBytes()
      );
      digestInfo.value.push(digestAlgorithm);
      digestInfo.value.push(digest3);
      return asn1.toDer(digestInfo).getBytes();
    };
    var _modPow = function(x, key, pub) {
      if (pub) {
        return x.modPow(key.e, key.n);
      }
      if (!key.p || !key.q) {
        return x.modPow(key.d, key.n);
      }
      if (!key.dP) {
        key.dP = key.d.mod(key.p.subtract(BigInteger.ONE));
      }
      if (!key.dQ) {
        key.dQ = key.d.mod(key.q.subtract(BigInteger.ONE));
      }
      if (!key.qInv) {
        key.qInv = key.q.modInverse(key.p);
      }
      var r;
      do {
        r = new BigInteger(
          forge6.util.bytesToHex(forge6.random.getBytes(key.n.bitLength() / 8)),
          16
        );
      } while (r.compareTo(key.n) >= 0 || !r.gcd(key.n).equals(BigInteger.ONE));
      x = x.multiply(r.modPow(key.e, key.n)).mod(key.n);
      var xp = x.mod(key.p).modPow(key.dP, key.p);
      var xq = x.mod(key.q).modPow(key.dQ, key.q);
      while (xp.compareTo(xq) < 0) {
        xp = xp.add(key.p);
      }
      var y = xp.subtract(xq).multiply(key.qInv).mod(key.p).multiply(key.q).add(xq);
      y = y.multiply(r.modInverse(key.n)).mod(key.n);
      return y;
    };
    pki.rsa.encrypt = function(m, key, bt) {
      var pub = bt;
      var eb;
      var k = Math.ceil(key.n.bitLength() / 8);
      if (bt !== false && bt !== true) {
        pub = bt === 2;
        eb = _encodePkcs1_v1_5(m, key, bt);
      } else {
        eb = forge6.util.createBuffer();
        eb.putBytes(m);
      }
      var x = new BigInteger(eb.toHex(), 16);
      var y = _modPow(x, key, pub);
      var yhex = y.toString(16);
      var ed = forge6.util.createBuffer();
      var zeros = k - Math.ceil(yhex.length / 2);
      while (zeros > 0) {
        ed.putByte(0);
        --zeros;
      }
      ed.putBytes(forge6.util.hexToBytes(yhex));
      return ed.getBytes();
    };
    pki.rsa.decrypt = function(ed, key, pub, ml) {
      var k = Math.ceil(key.n.bitLength() / 8);
      if (ed.length !== k) {
        var error = new Error("Encrypted message length is invalid.");
        error.length = ed.length;
        error.expected = k;
        throw error;
      }
      var y = new BigInteger(forge6.util.createBuffer(ed).toHex(), 16);
      if (y.compareTo(key.n) >= 0) {
        throw new Error("Encrypted message is invalid.");
      }
      var x = _modPow(y, key, pub);
      var xhex = x.toString(16);
      var eb = forge6.util.createBuffer();
      var zeros = k - Math.ceil(xhex.length / 2);
      while (zeros > 0) {
        eb.putByte(0);
        --zeros;
      }
      eb.putBytes(forge6.util.hexToBytes(xhex));
      if (ml !== false) {
        return _decodePkcs1_v1_5(eb.getBytes(), key, pub);
      }
      return eb.getBytes();
    };
    pki.rsa.createKeyPairGenerationState = function(bits2, e, options) {
      if (typeof bits2 === "string") {
        bits2 = parseInt(bits2, 10);
      }
      bits2 = bits2 || 2048;
      options = options || {};
      var prng = options.prng || forge6.random;
      var rng = {
        // x is an array to fill with bytes
        nextBytes: function(x) {
          var b = prng.getBytesSync(x.length);
          for (var i = 0; i < x.length; ++i) {
            x[i] = b.charCodeAt(i);
          }
        }
      };
      var algorithm = options.algorithm || "PRIMEINC";
      var rval;
      if (algorithm === "PRIMEINC") {
        rval = {
          algorithm,
          state: 0,
          bits: bits2,
          rng,
          eInt: e || 65537,
          e: new BigInteger(null),
          p: null,
          q: null,
          qBits: bits2 >> 1,
          pBits: bits2 - (bits2 >> 1),
          pqState: 0,
          num: null,
          keys: null
        };
        rval.e.fromInt(rval.eInt);
      } else {
        throw new Error("Invalid key generation algorithm: " + algorithm);
      }
      return rval;
    };
    pki.rsa.stepKeyPairGenerationState = function(state, n) {
      if (!("algorithm" in state)) {
        state.algorithm = "PRIMEINC";
      }
      var THIRTY = new BigInteger(null);
      THIRTY.fromInt(30);
      var deltaIdx = 0;
      var op_or = function(x, y) {
        return x | y;
      };
      var t1 = +/* @__PURE__ */ new Date();
      var t2;
      var total = 0;
      while (state.keys === null && (n <= 0 || total < n)) {
        if (state.state === 0) {
          var bits2 = state.p === null ? state.pBits : state.qBits;
          var bits1 = bits2 - 1;
          if (state.pqState === 0) {
            state.num = new BigInteger(bits2, state.rng);
            if (!state.num.testBit(bits1)) {
              state.num.bitwiseTo(
                BigInteger.ONE.shiftLeft(bits1),
                op_or,
                state.num
              );
            }
            state.num.dAddOffset(31 - state.num.mod(THIRTY).byteValue(), 0);
            deltaIdx = 0;
            ++state.pqState;
          } else if (state.pqState === 1) {
            if (state.num.bitLength() > bits2) {
              state.pqState = 0;
            } else if (state.num.isProbablePrime(
              _getMillerRabinTests(state.num.bitLength())
            )) {
              ++state.pqState;
            } else {
              state.num.dAddOffset(GCD_30_DELTA[deltaIdx++ % 8], 0);
            }
          } else if (state.pqState === 2) {
            state.pqState = state.num.subtract(BigInteger.ONE).gcd(state.e).compareTo(BigInteger.ONE) === 0 ? 3 : 0;
          } else if (state.pqState === 3) {
            state.pqState = 0;
            if (state.p === null) {
              state.p = state.num;
            } else {
              state.q = state.num;
            }
            if (state.p !== null && state.q !== null) {
              ++state.state;
            }
            state.num = null;
          }
        } else if (state.state === 1) {
          if (state.p.compareTo(state.q) < 0) {
            state.num = state.p;
            state.p = state.q;
            state.q = state.num;
          }
          ++state.state;
        } else if (state.state === 2) {
          state.p1 = state.p.subtract(BigInteger.ONE);
          state.q1 = state.q.subtract(BigInteger.ONE);
          state.phi = state.p1.multiply(state.q1);
          ++state.state;
        } else if (state.state === 3) {
          if (state.phi.gcd(state.e).compareTo(BigInteger.ONE) === 0) {
            ++state.state;
          } else {
            state.p = null;
            state.q = null;
            state.state = 0;
          }
        } else if (state.state === 4) {
          state.n = state.p.multiply(state.q);
          if (state.n.bitLength() === state.bits) {
            ++state.state;
          } else {
            state.q = null;
            state.state = 0;
          }
        } else if (state.state === 5) {
          var d = state.e.modInverse(state.phi);
          state.keys = {
            privateKey: pki.rsa.setPrivateKey(
              state.n,
              state.e,
              d,
              state.p,
              state.q,
              d.mod(state.p1),
              d.mod(state.q1),
              state.q.modInverse(state.p)
            ),
            publicKey: pki.rsa.setPublicKey(state.n, state.e)
          };
        }
        t2 = +/* @__PURE__ */ new Date();
        total += t2 - t1;
        t1 = t2;
      }
      return state.keys !== null;
    };
    pki.rsa.generateKeyPair = function(bits2, e, options, callback) {
      if (arguments.length === 1) {
        if (typeof bits2 === "object") {
          options = bits2;
          bits2 = void 0;
        } else if (typeof bits2 === "function") {
          callback = bits2;
          bits2 = void 0;
        }
      } else if (arguments.length === 2) {
        if (typeof bits2 === "number") {
          if (typeof e === "function") {
            callback = e;
            e = void 0;
          } else if (typeof e !== "number") {
            options = e;
            e = void 0;
          }
        } else {
          options = bits2;
          callback = e;
          bits2 = void 0;
          e = void 0;
        }
      } else if (arguments.length === 3) {
        if (typeof e === "number") {
          if (typeof options === "function") {
            callback = options;
            options = void 0;
          }
        } else {
          callback = options;
          options = e;
          e = void 0;
        }
      }
      options = options || {};
      if (bits2 === void 0) {
        bits2 = options.bits || 2048;
      }
      if (e === void 0) {
        e = options.e || 65537;
      }
      if (!forge6.options.usePureJavaScript && !options.prng && bits2 >= 256 && bits2 <= 16384 && (e === 65537 || e === 3)) {
        if (callback) {
          if (_detectNodeCrypto("generateKeyPair")) {
            return _crypto.generateKeyPair("rsa", {
              modulusLength: bits2,
              publicExponent: e,
              publicKeyEncoding: {
                type: "spki",
                format: "pem"
              },
              privateKeyEncoding: {
                type: "pkcs8",
                format: "pem"
              }
            }, function(err, pub, priv) {
              if (err) {
                return callback(err);
              }
              callback(null, {
                privateKey: pki.privateKeyFromPem(priv),
                publicKey: pki.publicKeyFromPem(pub)
              });
            });
          }
          if (_detectSubtleCrypto("generateKey") && _detectSubtleCrypto("exportKey")) {
            return util2.globalScope.crypto.subtle.generateKey({
              name: "RSASSA-PKCS1-v1_5",
              modulusLength: bits2,
              publicExponent: _intToUint8Array(e),
              hash: { name: "SHA-256" }
            }, true, ["sign", "verify"]).then(function(pair) {
              return util2.globalScope.crypto.subtle.exportKey(
                "pkcs8",
                pair.privateKey
              );
            }).then(void 0, function(err) {
              callback(err);
            }).then(function(pkcs8) {
              if (pkcs8) {
                var privateKey = pki.privateKeyFromAsn1(
                  asn1.fromDer(forge6.util.createBuffer(pkcs8))
                );
                callback(null, {
                  privateKey,
                  publicKey: pki.setRsaPublicKey(privateKey.n, privateKey.e)
                });
              }
            });
          }
          if (_detectSubtleMsCrypto("generateKey") && _detectSubtleMsCrypto("exportKey")) {
            var genOp = util2.globalScope.msCrypto.subtle.generateKey({
              name: "RSASSA-PKCS1-v1_5",
              modulusLength: bits2,
              publicExponent: _intToUint8Array(e),
              hash: { name: "SHA-256" }
            }, true, ["sign", "verify"]);
            genOp.oncomplete = function(e2) {
              var pair = e2.target.result;
              var exportOp = util2.globalScope.msCrypto.subtle.exportKey(
                "pkcs8",
                pair.privateKey
              );
              exportOp.oncomplete = function(e3) {
                var pkcs8 = e3.target.result;
                var privateKey = pki.privateKeyFromAsn1(
                  asn1.fromDer(forge6.util.createBuffer(pkcs8))
                );
                callback(null, {
                  privateKey,
                  publicKey: pki.setRsaPublicKey(privateKey.n, privateKey.e)
                });
              };
              exportOp.onerror = function(err) {
                callback(err);
              };
            };
            genOp.onerror = function(err) {
              callback(err);
            };
            return;
          }
        } else {
          if (_detectNodeCrypto("generateKeyPairSync")) {
            var keypair = _crypto.generateKeyPairSync("rsa", {
              modulusLength: bits2,
              publicExponent: e,
              publicKeyEncoding: {
                type: "spki",
                format: "pem"
              },
              privateKeyEncoding: {
                type: "pkcs8",
                format: "pem"
              }
            });
            return {
              privateKey: pki.privateKeyFromPem(keypair.privateKey),
              publicKey: pki.publicKeyFromPem(keypair.publicKey)
            };
          }
        }
      }
      var state = pki.rsa.createKeyPairGenerationState(bits2, e, options);
      if (!callback) {
        pki.rsa.stepKeyPairGenerationState(state, 0);
        return state.keys;
      }
      _generateKeyPair(state, options, callback);
    };
    pki.setRsaPublicKey = pki.rsa.setPublicKey = function(n, e) {
      var key = {
        n,
        e
      };
      key.encrypt = function(data, scheme, schemeOptions) {
        if (typeof scheme === "string") {
          scheme = scheme.toUpperCase();
        } else if (scheme === void 0) {
          scheme = "RSAES-PKCS1-V1_5";
        }
        if (scheme === "RSAES-PKCS1-V1_5") {
          scheme = {
            encode: function(m, key2, pub) {
              return _encodePkcs1_v1_5(m, key2, 2).getBytes();
            }
          };
        } else if (scheme === "RSA-OAEP" || scheme === "RSAES-OAEP") {
          scheme = {
            encode: function(m, key2) {
              return forge6.pkcs1.encode_rsa_oaep(key2, m, schemeOptions);
            }
          };
        } else if (["RAW", "NONE", "NULL", null].indexOf(scheme) !== -1) {
          scheme = { encode: function(e3) {
            return e3;
          } };
        } else if (typeof scheme === "string") {
          throw new Error('Unsupported encryption scheme: "' + scheme + '".');
        }
        var e2 = scheme.encode(data, key, true);
        return pki.rsa.encrypt(e2, key, true);
      };
      key.verify = function(digest3, signature, scheme, options) {
        if (typeof scheme === "string") {
          scheme = scheme.toUpperCase();
        } else if (scheme === void 0) {
          scheme = "RSASSA-PKCS1-V1_5";
        }
        if (options === void 0) {
          options = {
            _parseAllDigestBytes: true
          };
        }
        if (!("_parseAllDigestBytes" in options)) {
          options._parseAllDigestBytes = true;
        }
        if (scheme === "RSASSA-PKCS1-V1_5") {
          scheme = {
            verify: function(digest4, d2) {
              d2 = _decodePkcs1_v1_5(d2, key, true);
              var obj = asn1.fromDer(d2, {
                parseAllBytes: options._parseAllDigestBytes
              });
              var capture = {};
              var errors = [];
              if (!asn1.validate(obj, digestInfoValidator, capture, errors)) {
                var error = new Error(
                  "ASN.1 object does not contain a valid RSASSA-PKCS1-v1_5 DigestInfo value."
                );
                error.errors = errors;
                throw error;
              }
              var oid = asn1.derToOid(capture.algorithmIdentifier);
              if (!(oid === forge6.oids.md2 || oid === forge6.oids.md5 || oid === forge6.oids.sha1 || oid === forge6.oids.sha224 || oid === forge6.oids.sha256 || oid === forge6.oids.sha384 || oid === forge6.oids.sha512 || oid === forge6.oids["sha512-224"] || oid === forge6.oids["sha512-256"])) {
                var error = new Error(
                  "Unknown RSASSA-PKCS1-v1_5 DigestAlgorithm identifier."
                );
                error.oid = oid;
                throw error;
              }
              if (oid === forge6.oids.md2 || oid === forge6.oids.md5) {
                if (!("parameters" in capture)) {
                  throw new Error(
                    "ASN.1 object does not contain a valid RSASSA-PKCS1-v1_5 DigestInfo value. Missing algorithm identifer NULL parameters."
                  );
                }
              }
              return digest4 === capture.digest;
            }
          };
        } else if (scheme === "NONE" || scheme === "NULL" || scheme === null) {
          scheme = {
            verify: function(digest4, d2) {
              d2 = _decodePkcs1_v1_5(d2, key, true);
              return digest4 === d2;
            }
          };
        }
        var d = pki.rsa.decrypt(signature, key, true, false);
        return scheme.verify(digest3, d, key.n.bitLength());
      };
      return key;
    };
    pki.setRsaPrivateKey = pki.rsa.setPrivateKey = function(n, e, d, p, q, dP, dQ, qInv) {
      var key = {
        n,
        e,
        d,
        p,
        q,
        dP,
        dQ,
        qInv
      };
      key.decrypt = function(data, scheme, schemeOptions) {
        if (typeof scheme === "string") {
          scheme = scheme.toUpperCase();
        } else if (scheme === void 0) {
          scheme = "RSAES-PKCS1-V1_5";
        }
        var d2 = pki.rsa.decrypt(data, key, false, false);
        if (scheme === "RSAES-PKCS1-V1_5") {
          scheme = { decode: _decodePkcs1_v1_5 };
        } else if (scheme === "RSA-OAEP" || scheme === "RSAES-OAEP") {
          scheme = {
            decode: function(d3, key2) {
              return forge6.pkcs1.decode_rsa_oaep(key2, d3, schemeOptions);
            }
          };
        } else if (["RAW", "NONE", "NULL", null].indexOf(scheme) !== -1) {
          scheme = { decode: function(d3) {
            return d3;
          } };
        } else {
          throw new Error('Unsupported encryption scheme: "' + scheme + '".');
        }
        return scheme.decode(d2, key, false);
      };
      key.sign = function(md, scheme) {
        var bt = false;
        if (typeof scheme === "string") {
          scheme = scheme.toUpperCase();
        }
        if (scheme === void 0 || scheme === "RSASSA-PKCS1-V1_5") {
          scheme = { encode: emsaPkcs1v15encode };
          bt = 1;
        } else if (scheme === "NONE" || scheme === "NULL" || scheme === null) {
          scheme = { encode: function() {
            return md;
          } };
          bt = 1;
        }
        var d2 = scheme.encode(md, key.n.bitLength());
        return pki.rsa.encrypt(d2, key, bt);
      };
      return key;
    };
    pki.wrapRsaPrivateKey = function(rsaKey) {
      return asn1.create(asn1.Class.UNIVERSAL, asn1.Type.SEQUENCE, true, [
        // version (0)
        asn1.create(
          asn1.Class.UNIVERSAL,
          asn1.Type.INTEGER,
          false,
          asn1.integerToDer(0).getBytes()
        ),
        // privateKeyAlgorithm
        asn1.create(asn1.Class.UNIVERSAL, asn1.Type.SEQUENCE, true, [
          asn1.create(
            asn1.Class.UNIVERSAL,
            asn1.Type.OID,
            false,
            asn1.oidToDer(pki.oids.rsaEncryption).getBytes()
          ),
          asn1.create(asn1.Class.UNIVERSAL, asn1.Type.NULL, false, "")
        ]),
        // PrivateKey
        asn1.create(
          asn1.Class.UNIVERSAL,
          asn1.Type.OCTETSTRING,
          false,
          asn1.toDer(rsaKey).getBytes()
        )
      ]);
    };
    pki.privateKeyFromAsn1 = function(obj) {
      var capture = {};
      var errors = [];
      if (asn1.validate(obj, privateKeyValidator, capture, errors)) {
        obj = asn1.fromDer(forge6.util.createBuffer(capture.privateKey));
      }
      capture = {};
      errors = [];
      if (!asn1.validate(obj, rsaPrivateKeyValidator, capture, errors)) {
        var error = new Error("Cannot read private key. ASN.1 object does not contain an RSAPrivateKey.");
        error.errors = errors;
        throw error;
      }
      var n, e, d, p, q, dP, dQ, qInv;
      n = forge6.util.createBuffer(capture.privateKeyModulus).toHex();
      e = forge6.util.createBuffer(capture.privateKeyPublicExponent).toHex();
      d = forge6.util.createBuffer(capture.privateKeyPrivateExponent).toHex();
      p = forge6.util.createBuffer(capture.privateKeyPrime1).toHex();
      q = forge6.util.createBuffer(capture.privateKeyPrime2).toHex();
      dP = forge6.util.createBuffer(capture.privateKeyExponent1).toHex();
      dQ = forge6.util.createBuffer(capture.privateKeyExponent2).toHex();
      qInv = forge6.util.createBuffer(capture.privateKeyCoefficient).toHex();
      return pki.setRsaPrivateKey(
        new BigInteger(n, 16),
        new BigInteger(e, 16),
        new BigInteger(d, 16),
        new BigInteger(p, 16),
        new BigInteger(q, 16),
        new BigInteger(dP, 16),
        new BigInteger(dQ, 16),
        new BigInteger(qInv, 16)
      );
    };
    pki.privateKeyToAsn1 = pki.privateKeyToRSAPrivateKey = function(key) {
      return asn1.create(asn1.Class.UNIVERSAL, asn1.Type.SEQUENCE, true, [
        // version (0 = only 2 primes, 1 multiple primes)
        asn1.create(
          asn1.Class.UNIVERSAL,
          asn1.Type.INTEGER,
          false,
          asn1.integerToDer(0).getBytes()
        ),
        // modulus (n)
        asn1.create(
          asn1.Class.UNIVERSAL,
          asn1.Type.INTEGER,
          false,
          _bnToBytes(key.n)
        ),
        // publicExponent (e)
        asn1.create(
          asn1.Class.UNIVERSAL,
          asn1.Type.INTEGER,
          false,
          _bnToBytes(key.e)
        ),
        // privateExponent (d)
        asn1.create(
          asn1.Class.UNIVERSAL,
          asn1.Type.INTEGER,
          false,
          _bnToBytes(key.d)
        ),
        // privateKeyPrime1 (p)
        asn1.create(
          asn1.Class.UNIVERSAL,
          asn1.Type.INTEGER,
          false,
          _bnToBytes(key.p)
        ),
        // privateKeyPrime2 (q)
        asn1.create(
          asn1.Class.UNIVERSAL,
          asn1.Type.INTEGER,
          false,
          _bnToBytes(key.q)
        ),
        // privateKeyExponent1 (dP)
        asn1.create(
          asn1.Class.UNIVERSAL,
          asn1.Type.INTEGER,
          false,
          _bnToBytes(key.dP)
        ),
        // privateKeyExponent2 (dQ)
        asn1.create(
          asn1.Class.UNIVERSAL,
          asn1.Type.INTEGER,
          false,
          _bnToBytes(key.dQ)
        ),
        // coefficient (qInv)
        asn1.create(
          asn1.Class.UNIVERSAL,
          asn1.Type.INTEGER,
          false,
          _bnToBytes(key.qInv)
        )
      ]);
    };
    pki.publicKeyFromAsn1 = function(obj) {
      var capture = {};
      var errors = [];
      if (asn1.validate(obj, publicKeyValidator, capture, errors)) {
        var oid = asn1.derToOid(capture.publicKeyOid);
        if (oid !== pki.oids.rsaEncryption) {
          var error = new Error("Cannot read public key. Unknown OID.");
          error.oid = oid;
          throw error;
        }
        obj = capture.rsaPublicKey;
      }
      errors = [];
      if (!asn1.validate(obj, rsaPublicKeyValidator, capture, errors)) {
        var error = new Error("Cannot read public key. ASN.1 object does not contain an RSAPublicKey.");
        error.errors = errors;
        throw error;
      }
      var n = forge6.util.createBuffer(capture.publicKeyModulus).toHex();
      var e = forge6.util.createBuffer(capture.publicKeyExponent).toHex();
      return pki.setRsaPublicKey(
        new BigInteger(n, 16),
        new BigInteger(e, 16)
      );
    };
    pki.publicKeyToAsn1 = pki.publicKeyToSubjectPublicKeyInfo = function(key) {
      return asn1.create(asn1.Class.UNIVERSAL, asn1.Type.SEQUENCE, true, [
        // AlgorithmIdentifier
        asn1.create(asn1.Class.UNIVERSAL, asn1.Type.SEQUENCE, true, [
          // algorithm
          asn1.create(
            asn1.Class.UNIVERSAL,
            asn1.Type.OID,
            false,
            asn1.oidToDer(pki.oids.rsaEncryption).getBytes()
          ),
          // parameters (null)
          asn1.create(asn1.Class.UNIVERSAL, asn1.Type.NULL, false, "")
        ]),
        // subjectPublicKey
        asn1.create(asn1.Class.UNIVERSAL, asn1.Type.BITSTRING, false, [
          pki.publicKeyToRSAPublicKey(key)
        ])
      ]);
    };
    pki.publicKeyToRSAPublicKey = function(key) {
      return asn1.create(asn1.Class.UNIVERSAL, asn1.Type.SEQUENCE, true, [
        // modulus (n)
        asn1.create(
          asn1.Class.UNIVERSAL,
          asn1.Type.INTEGER,
          false,
          _bnToBytes(key.n)
        ),
        // publicExponent (e)
        asn1.create(
          asn1.Class.UNIVERSAL,
          asn1.Type.INTEGER,
          false,
          _bnToBytes(key.e)
        )
      ]);
    };
    function _encodePkcs1_v1_5(m, key, bt) {
      var eb = forge6.util.createBuffer();
      var k = Math.ceil(key.n.bitLength() / 8);
      if (m.length > k - 11) {
        var error = new Error("Message is too long for PKCS#1 v1.5 padding.");
        error.length = m.length;
        error.max = k - 11;
        throw error;
      }
      eb.putByte(0);
      eb.putByte(bt);
      var padNum = k - 3 - m.length;
      var padByte;
      if (bt === 0 || bt === 1) {
        padByte = bt === 0 ? 0 : 255;
        for (var i = 0; i < padNum; ++i) {
          eb.putByte(padByte);
        }
      } else {
        while (padNum > 0) {
          var numZeros = 0;
          var padBytes = forge6.random.getBytes(padNum);
          for (var i = 0; i < padNum; ++i) {
            padByte = padBytes.charCodeAt(i);
            if (padByte === 0) {
              ++numZeros;
            } else {
              eb.putByte(padByte);
            }
          }
          padNum = numZeros;
        }
      }
      eb.putByte(0);
      eb.putBytes(m);
      return eb;
    }
    function _decodePkcs1_v1_5(em, key, pub, ml) {
      var k = Math.ceil(key.n.bitLength() / 8);
      var eb = forge6.util.createBuffer(em);
      var first = eb.getByte();
      var bt = eb.getByte();
      if (first !== 0 || pub && bt !== 0 && bt !== 1 || !pub && bt != 2 || pub && bt === 0 && typeof ml === "undefined") {
        throw new Error("Encryption block is invalid.");
      }
      var padNum = 0;
      if (bt === 0) {
        padNum = k - 3 - ml;
        for (var i = 0; i < padNum; ++i) {
          if (eb.getByte() !== 0) {
            throw new Error("Encryption block is invalid.");
          }
        }
      } else if (bt === 1) {
        padNum = 0;
        while (eb.length() > 1) {
          if (eb.getByte() !== 255) {
            --eb.read;
            break;
          }
          ++padNum;
        }
      } else if (bt === 2) {
        padNum = 0;
        while (eb.length() > 1) {
          if (eb.getByte() === 0) {
            --eb.read;
            break;
          }
          ++padNum;
        }
      }
      var zero = eb.getByte();
      if (zero !== 0 || padNum !== k - 3 - eb.length()) {
        throw new Error("Encryption block is invalid.");
      }
      return eb.getBytes();
    }
    function _generateKeyPair(state, options, callback) {
      if (typeof options === "function") {
        callback = options;
        options = {};
      }
      options = options || {};
      var opts = {
        algorithm: {
          name: options.algorithm || "PRIMEINC",
          options: {
            workers: options.workers || 2,
            workLoad: options.workLoad || 100,
            workerScript: options.workerScript
          }
        }
      };
      if ("prng" in options) {
        opts.prng = options.prng;
      }
      generate();
      function generate() {
        getPrime(state.pBits, function(err, num) {
          if (err) {
            return callback(err);
          }
          state.p = num;
          if (state.q !== null) {
            return finish(err, state.q);
          }
          getPrime(state.qBits, finish);
        });
      }
      function getPrime(bits2, callback2) {
        forge6.prime.generateProbablePrime(bits2, opts, callback2);
      }
      function finish(err, num) {
        if (err) {
          return callback(err);
        }
        state.q = num;
        if (state.p.compareTo(state.q) < 0) {
          var tmp = state.p;
          state.p = state.q;
          state.q = tmp;
        }
        if (state.p.subtract(BigInteger.ONE).gcd(state.e).compareTo(BigInteger.ONE) !== 0) {
          state.p = null;
          generate();
          return;
        }
        if (state.q.subtract(BigInteger.ONE).gcd(state.e).compareTo(BigInteger.ONE) !== 0) {
          state.q = null;
          getPrime(state.qBits, finish);
          return;
        }
        state.p1 = state.p.subtract(BigInteger.ONE);
        state.q1 = state.q.subtract(BigInteger.ONE);
        state.phi = state.p1.multiply(state.q1);
        if (state.phi.gcd(state.e).compareTo(BigInteger.ONE) !== 0) {
          state.p = state.q = null;
          generate();
          return;
        }
        state.n = state.p.multiply(state.q);
        if (state.n.bitLength() !== state.bits) {
          state.q = null;
          getPrime(state.qBits, finish);
          return;
        }
        var d = state.e.modInverse(state.phi);
        state.keys = {
          privateKey: pki.rsa.setPrivateKey(
            state.n,
            state.e,
            d,
            state.p,
            state.q,
            d.mod(state.p1),
            d.mod(state.q1),
            state.q.modInverse(state.p)
          ),
          publicKey: pki.rsa.setPublicKey(state.n, state.e)
        };
        callback(null, state.keys);
      }
    }
    function _bnToBytes(b) {
      var hex = b.toString(16);
      if (hex[0] >= "8") {
        hex = "00" + hex;
      }
      var bytes = forge6.util.hexToBytes(hex);
      if (bytes.length > 1 && // leading 0x00 for positive integer
      (bytes.charCodeAt(0) === 0 && (bytes.charCodeAt(1) & 128) === 0 || // leading 0xFF for negative integer
      bytes.charCodeAt(0) === 255 && (bytes.charCodeAt(1) & 128) === 128)) {
        return bytes.substr(1);
      }
      return bytes;
    }
    function _getMillerRabinTests(bits2) {
      if (bits2 <= 100)
        return 27;
      if (bits2 <= 150)
        return 18;
      if (bits2 <= 200)
        return 15;
      if (bits2 <= 250)
        return 12;
      if (bits2 <= 300)
        return 9;
      if (bits2 <= 350)
        return 8;
      if (bits2 <= 400)
        return 7;
      if (bits2 <= 500)
        return 6;
      if (bits2 <= 600)
        return 5;
      if (bits2 <= 800)
        return 4;
      if (bits2 <= 1250)
        return 3;
      return 2;
    }
    function _detectNodeCrypto(fn) {
      return forge6.util.isNodejs && typeof _crypto[fn] === "function";
    }
    function _detectSubtleCrypto(fn) {
      return typeof util2.globalScope !== "undefined" && typeof util2.globalScope.crypto === "object" && typeof util2.globalScope.crypto.subtle === "object" && typeof util2.globalScope.crypto.subtle[fn] === "function";
    }
    function _detectSubtleMsCrypto(fn) {
      return typeof util2.globalScope !== "undefined" && typeof util2.globalScope.msCrypto === "object" && typeof util2.globalScope.msCrypto.subtle === "object" && typeof util2.globalScope.msCrypto.subtle[fn] === "function";
    }
    function _intToUint8Array(x) {
      var bytes = forge6.util.hexToBytes(x.toString(16));
      var buffer = new Uint8Array(bytes.length);
      for (var i = 0; i < bytes.length; ++i) {
        buffer[i] = bytes.charCodeAt(i);
      }
      return buffer;
    }
  }
});

// ../../node_modules/node-forge/lib/pbe.js
var require_pbe = __commonJS({
  "../../node_modules/node-forge/lib/pbe.js"(exports2, module2) {
    var forge6 = require_forge();
    require_aes();
    require_asn1();
    require_des();
    require_md();
    require_oids();
    require_pbkdf2();
    require_pem();
    require_random();
    require_rc2();
    require_rsa();
    require_util();
    if (typeof BigInteger === "undefined") {
      BigInteger = forge6.jsbn.BigInteger;
    }
    var BigInteger;
    var asn1 = forge6.asn1;
    var pki = forge6.pki = forge6.pki || {};
    module2.exports = pki.pbe = forge6.pbe = forge6.pbe || {};
    var oids = pki.oids;
    var encryptedPrivateKeyValidator = {
      name: "EncryptedPrivateKeyInfo",
      tagClass: asn1.Class.UNIVERSAL,
      type: asn1.Type.SEQUENCE,
      constructed: true,
      value: [{
        name: "EncryptedPrivateKeyInfo.encryptionAlgorithm",
        tagClass: asn1.Class.UNIVERSAL,
        type: asn1.Type.SEQUENCE,
        constructed: true,
        value: [{
          name: "AlgorithmIdentifier.algorithm",
          tagClass: asn1.Class.UNIVERSAL,
          type: asn1.Type.OID,
          constructed: false,
          capture: "encryptionOid"
        }, {
          name: "AlgorithmIdentifier.parameters",
          tagClass: asn1.Class.UNIVERSAL,
          type: asn1.Type.SEQUENCE,
          constructed: true,
          captureAsn1: "encryptionParams"
        }]
      }, {
        // encryptedData
        name: "EncryptedPrivateKeyInfo.encryptedData",
        tagClass: asn1.Class.UNIVERSAL,
        type: asn1.Type.OCTETSTRING,
        constructed: false,
        capture: "encryptedData"
      }]
    };
    var PBES2AlgorithmsValidator = {
      name: "PBES2Algorithms",
      tagClass: asn1.Class.UNIVERSAL,
      type: asn1.Type.SEQUENCE,
      constructed: true,
      value: [{
        name: "PBES2Algorithms.keyDerivationFunc",
        tagClass: asn1.Class.UNIVERSAL,
        type: asn1.Type.SEQUENCE,
        constructed: true,
        value: [{
          name: "PBES2Algorithms.keyDerivationFunc.oid",
          tagClass: asn1.Class.UNIVERSAL,
          type: asn1.Type.OID,
          constructed: false,
          capture: "kdfOid"
        }, {
          name: "PBES2Algorithms.params",
          tagClass: asn1.Class.UNIVERSAL,
          type: asn1.Type.SEQUENCE,
          constructed: true,
          value: [{
            name: "PBES2Algorithms.params.salt",
            tagClass: asn1.Class.UNIVERSAL,
            type: asn1.Type.OCTETSTRING,
            constructed: false,
            capture: "kdfSalt"
          }, {
            name: "PBES2Algorithms.params.iterationCount",
            tagClass: asn1.Class.UNIVERSAL,
            type: asn1.Type.INTEGER,
            constructed: false,
            capture: "kdfIterationCount"
          }, {
            name: "PBES2Algorithms.params.keyLength",
            tagClass: asn1.Class.UNIVERSAL,
            type: asn1.Type.INTEGER,
            constructed: false,
            optional: true,
            capture: "keyLength"
          }, {
            // prf
            name: "PBES2Algorithms.params.prf",
            tagClass: asn1.Class.UNIVERSAL,
            type: asn1.Type.SEQUENCE,
            constructed: true,
            optional: true,
            value: [{
              name: "PBES2Algorithms.params.prf.algorithm",
              tagClass: asn1.Class.UNIVERSAL,
              type: asn1.Type.OID,
              constructed: false,
              capture: "prfOid"
            }]
          }]
        }]
      }, {
        name: "PBES2Algorithms.encryptionScheme",
        tagClass: asn1.Class.UNIVERSAL,
        type: asn1.Type.SEQUENCE,
        constructed: true,
        value: [{
          name: "PBES2Algorithms.encryptionScheme.oid",
          tagClass: asn1.Class.UNIVERSAL,
          type: asn1.Type.OID,
          constructed: false,
          capture: "encOid"
        }, {
          name: "PBES2Algorithms.encryptionScheme.iv",
          tagClass: asn1.Class.UNIVERSAL,
          type: asn1.Type.OCTETSTRING,
          constructed: false,
          capture: "encIv"
        }]
      }]
    };
    var pkcs12PbeParamsValidator = {
      name: "pkcs-12PbeParams",
      tagClass: asn1.Class.UNIVERSAL,
      type: asn1.Type.SEQUENCE,
      constructed: true,
      value: [{
        name: "pkcs-12PbeParams.salt",
        tagClass: asn1.Class.UNIVERSAL,
        type: asn1.Type.OCTETSTRING,
        constructed: false,
        capture: "salt"
      }, {
        name: "pkcs-12PbeParams.iterations",
        tagClass: asn1.Class.UNIVERSAL,
        type: asn1.Type.INTEGER,
        constructed: false,
        capture: "iterations"
      }]
    };
    pki.encryptPrivateKeyInfo = function(obj, password, options) {
      options = options || {};
      options.saltSize = options.saltSize || 8;
      options.count = options.count || 2048;
      options.algorithm = options.algorithm || "aes128";
      options.prfAlgorithm = options.prfAlgorithm || "sha1";
      var salt = forge6.random.getBytesSync(options.saltSize);
      var count = options.count;
      var countBytes = asn1.integerToDer(count);
      var dkLen;
      var encryptionAlgorithm;
      var encryptedData;
      if (options.algorithm.indexOf("aes") === 0 || options.algorithm === "des") {
        var ivLen, encOid, cipherFn;
        switch (options.algorithm) {
          case "aes128":
            dkLen = 16;
            ivLen = 16;
            encOid = oids["aes128-CBC"];
            cipherFn = forge6.aes.createEncryptionCipher;
            break;
          case "aes192":
            dkLen = 24;
            ivLen = 16;
            encOid = oids["aes192-CBC"];
            cipherFn = forge6.aes.createEncryptionCipher;
            break;
          case "aes256":
            dkLen = 32;
            ivLen = 16;
            encOid = oids["aes256-CBC"];
            cipherFn = forge6.aes.createEncryptionCipher;
            break;
          case "des":
            dkLen = 8;
            ivLen = 8;
            encOid = oids["desCBC"];
            cipherFn = forge6.des.createEncryptionCipher;
            break;
          default:
            var error = new Error("Cannot encrypt private key. Unknown encryption algorithm.");
            error.algorithm = options.algorithm;
            throw error;
        }
        var prfAlgorithm = "hmacWith" + options.prfAlgorithm.toUpperCase();
        var md = prfAlgorithmToMessageDigest(prfAlgorithm);
        var dk = forge6.pkcs5.pbkdf2(password, salt, count, dkLen, md);
        var iv = forge6.random.getBytesSync(ivLen);
        var cipher = cipherFn(dk);
        cipher.start(iv);
        cipher.update(asn1.toDer(obj));
        cipher.finish();
        encryptedData = cipher.output.getBytes();
        var params = createPbkdf2Params(salt, countBytes, dkLen, prfAlgorithm);
        encryptionAlgorithm = asn1.create(
          asn1.Class.UNIVERSAL,
          asn1.Type.SEQUENCE,
          true,
          [
            asn1.create(
              asn1.Class.UNIVERSAL,
              asn1.Type.OID,
              false,
              asn1.oidToDer(oids["pkcs5PBES2"]).getBytes()
            ),
            asn1.create(asn1.Class.UNIVERSAL, asn1.Type.SEQUENCE, true, [
              // keyDerivationFunc
              asn1.create(asn1.Class.UNIVERSAL, asn1.Type.SEQUENCE, true, [
                asn1.create(
                  asn1.Class.UNIVERSAL,
                  asn1.Type.OID,
                  false,
                  asn1.oidToDer(oids["pkcs5PBKDF2"]).getBytes()
                ),
                // PBKDF2-params
                params
              ]),
              // encryptionScheme
              asn1.create(asn1.Class.UNIVERSAL, asn1.Type.SEQUENCE, true, [
                asn1.create(
                  asn1.Class.UNIVERSAL,
                  asn1.Type.OID,
                  false,
                  asn1.oidToDer(encOid).getBytes()
                ),
                // iv
                asn1.create(
                  asn1.Class.UNIVERSAL,
                  asn1.Type.OCTETSTRING,
                  false,
                  iv
                )
              ])
            ])
          ]
        );
      } else if (options.algorithm === "3des") {
        dkLen = 24;
        var saltBytes = new forge6.util.ByteBuffer(salt);
        var dk = pki.pbe.generatePkcs12Key(password, saltBytes, 1, count, dkLen);
        var iv = pki.pbe.generatePkcs12Key(password, saltBytes, 2, count, dkLen);
        var cipher = forge6.des.createEncryptionCipher(dk);
        cipher.start(iv);
        cipher.update(asn1.toDer(obj));
        cipher.finish();
        encryptedData = cipher.output.getBytes();
        encryptionAlgorithm = asn1.create(
          asn1.Class.UNIVERSAL,
          asn1.Type.SEQUENCE,
          true,
          [
            asn1.create(
              asn1.Class.UNIVERSAL,
              asn1.Type.OID,
              false,
              asn1.oidToDer(oids["pbeWithSHAAnd3-KeyTripleDES-CBC"]).getBytes()
            ),
            // pkcs-12PbeParams
            asn1.create(asn1.Class.UNIVERSAL, asn1.Type.SEQUENCE, true, [
              // salt
              asn1.create(asn1.Class.UNIVERSAL, asn1.Type.OCTETSTRING, false, salt),
              // iteration count
              asn1.create(
                asn1.Class.UNIVERSAL,
                asn1.Type.INTEGER,
                false,
                countBytes.getBytes()
              )
            ])
          ]
        );
      } else {
        var error = new Error("Cannot encrypt private key. Unknown encryption algorithm.");
        error.algorithm = options.algorithm;
        throw error;
      }
      var rval = asn1.create(asn1.Class.UNIVERSAL, asn1.Type.SEQUENCE, true, [
        // encryptionAlgorithm
        encryptionAlgorithm,
        // encryptedData
        asn1.create(
          asn1.Class.UNIVERSAL,
          asn1.Type.OCTETSTRING,
          false,
          encryptedData
        )
      ]);
      return rval;
    };
    pki.decryptPrivateKeyInfo = function(obj, password) {
      var rval = null;
      var capture = {};
      var errors = [];
      if (!asn1.validate(obj, encryptedPrivateKeyValidator, capture, errors)) {
        var error = new Error("Cannot read encrypted private key. ASN.1 object is not a supported EncryptedPrivateKeyInfo.");
        error.errors = errors;
        throw error;
      }
      var oid = asn1.derToOid(capture.encryptionOid);
      var cipher = pki.pbe.getCipher(oid, capture.encryptionParams, password);
      var encrypted = forge6.util.createBuffer(capture.encryptedData);
      cipher.update(encrypted);
      if (cipher.finish()) {
        rval = asn1.fromDer(cipher.output);
      }
      return rval;
    };
    pki.encryptedPrivateKeyToPem = function(epki, maxline) {
      var msg = {
        type: "ENCRYPTED PRIVATE KEY",
        body: asn1.toDer(epki).getBytes()
      };
      return forge6.pem.encode(msg, { maxline });
    };
    pki.encryptedPrivateKeyFromPem = function(pem) {
      var msg = forge6.pem.decode(pem)[0];
      if (msg.type !== "ENCRYPTED PRIVATE KEY") {
        var error = new Error('Could not convert encrypted private key from PEM; PEM header type is "ENCRYPTED PRIVATE KEY".');
        error.headerType = msg.type;
        throw error;
      }
      if (msg.procType && msg.procType.type === "ENCRYPTED") {
        throw new Error("Could not convert encrypted private key from PEM; PEM is encrypted.");
      }
      return asn1.fromDer(msg.body);
    };
    pki.encryptRsaPrivateKey = function(rsaKey, password, options) {
      options = options || {};
      if (!options.legacy) {
        var rval = pki.wrapRsaPrivateKey(pki.privateKeyToAsn1(rsaKey));
        rval = pki.encryptPrivateKeyInfo(rval, password, options);
        return pki.encryptedPrivateKeyToPem(rval);
      }
      var algorithm;
      var iv;
      var dkLen;
      var cipherFn;
      switch (options.algorithm) {
        case "aes128":
          algorithm = "AES-128-CBC";
          dkLen = 16;
          iv = forge6.random.getBytesSync(16);
          cipherFn = forge6.aes.createEncryptionCipher;
          break;
        case "aes192":
          algorithm = "AES-192-CBC";
          dkLen = 24;
          iv = forge6.random.getBytesSync(16);
          cipherFn = forge6.aes.createEncryptionCipher;
          break;
        case "aes256":
          algorithm = "AES-256-CBC";
          dkLen = 32;
          iv = forge6.random.getBytesSync(16);
          cipherFn = forge6.aes.createEncryptionCipher;
          break;
        case "3des":
          algorithm = "DES-EDE3-CBC";
          dkLen = 24;
          iv = forge6.random.getBytesSync(8);
          cipherFn = forge6.des.createEncryptionCipher;
          break;
        case "des":
          algorithm = "DES-CBC";
          dkLen = 8;
          iv = forge6.random.getBytesSync(8);
          cipherFn = forge6.des.createEncryptionCipher;
          break;
        default:
          var error = new Error('Could not encrypt RSA private key; unsupported encryption algorithm "' + options.algorithm + '".');
          error.algorithm = options.algorithm;
          throw error;
      }
      var dk = forge6.pbe.opensslDeriveBytes(password, iv.substr(0, 8), dkLen);
      var cipher = cipherFn(dk);
      cipher.start(iv);
      cipher.update(asn1.toDer(pki.privateKeyToAsn1(rsaKey)));
      cipher.finish();
      var msg = {
        type: "RSA PRIVATE KEY",
        procType: {
          version: "4",
          type: "ENCRYPTED"
        },
        dekInfo: {
          algorithm,
          parameters: forge6.util.bytesToHex(iv).toUpperCase()
        },
        body: cipher.output.getBytes()
      };
      return forge6.pem.encode(msg);
    };
    pki.decryptRsaPrivateKey = function(pem, password) {
      var rval = null;
      var msg = forge6.pem.decode(pem)[0];
      if (msg.type !== "ENCRYPTED PRIVATE KEY" && msg.type !== "PRIVATE KEY" && msg.type !== "RSA PRIVATE KEY") {
        var error = new Error('Could not convert private key from PEM; PEM header type is not "ENCRYPTED PRIVATE KEY", "PRIVATE KEY", or "RSA PRIVATE KEY".');
        error.headerType = error;
        throw error;
      }
      if (msg.procType && msg.procType.type === "ENCRYPTED") {
        var dkLen;
        var cipherFn;
        switch (msg.dekInfo.algorithm) {
          case "DES-CBC":
            dkLen = 8;
            cipherFn = forge6.des.createDecryptionCipher;
            break;
          case "DES-EDE3-CBC":
            dkLen = 24;
            cipherFn = forge6.des.createDecryptionCipher;
            break;
          case "AES-128-CBC":
            dkLen = 16;
            cipherFn = forge6.aes.createDecryptionCipher;
            break;
          case "AES-192-CBC":
            dkLen = 24;
            cipherFn = forge6.aes.createDecryptionCipher;
            break;
          case "AES-256-CBC":
            dkLen = 32;
            cipherFn = forge6.aes.createDecryptionCipher;
            break;
          case "RC2-40-CBC":
            dkLen = 5;
            cipherFn = function(key) {
              return forge6.rc2.createDecryptionCipher(key, 40);
            };
            break;
          case "RC2-64-CBC":
            dkLen = 8;
            cipherFn = function(key) {
              return forge6.rc2.createDecryptionCipher(key, 64);
            };
            break;
          case "RC2-128-CBC":
            dkLen = 16;
            cipherFn = function(key) {
              return forge6.rc2.createDecryptionCipher(key, 128);
            };
            break;
          default:
            var error = new Error('Could not decrypt private key; unsupported encryption algorithm "' + msg.dekInfo.algorithm + '".');
            error.algorithm = msg.dekInfo.algorithm;
            throw error;
        }
        var iv = forge6.util.hexToBytes(msg.dekInfo.parameters);
        var dk = forge6.pbe.opensslDeriveBytes(password, iv.substr(0, 8), dkLen);
        var cipher = cipherFn(dk);
        cipher.start(iv);
        cipher.update(forge6.util.createBuffer(msg.body));
        if (cipher.finish()) {
          rval = cipher.output.getBytes();
        } else {
          return rval;
        }
      } else {
        rval = msg.body;
      }
      if (msg.type === "ENCRYPTED PRIVATE KEY") {
        rval = pki.decryptPrivateKeyInfo(asn1.fromDer(rval), password);
      } else {
        rval = asn1.fromDer(rval);
      }
      if (rval !== null) {
        rval = pki.privateKeyFromAsn1(rval);
      }
      return rval;
    };
    pki.pbe.generatePkcs12Key = function(password, salt, id, iter, n, md) {
      var j, l;
      if (typeof md === "undefined" || md === null) {
        if (!("sha1" in forge6.md)) {
          throw new Error('"sha1" hash algorithm unavailable.');
        }
        md = forge6.md.sha1.create();
      }
      var u = md.digestLength;
      var v = md.blockLength;
      var result = new forge6.util.ByteBuffer();
      var passBuf = new forge6.util.ByteBuffer();
      if (password !== null && password !== void 0) {
        for (l = 0; l < password.length; l++) {
          passBuf.putInt16(password.charCodeAt(l));
        }
        passBuf.putInt16(0);
      }
      var p = passBuf.length();
      var s = salt.length();
      var D = new forge6.util.ByteBuffer();
      D.fillWithByte(id, v);
      var Slen = v * Math.ceil(s / v);
      var S = new forge6.util.ByteBuffer();
      for (l = 0; l < Slen; l++) {
        S.putByte(salt.at(l % s));
      }
      var Plen = v * Math.ceil(p / v);
      var P = new forge6.util.ByteBuffer();
      for (l = 0; l < Plen; l++) {
        P.putByte(passBuf.at(l % p));
      }
      var I = S;
      I.putBuffer(P);
      var c = Math.ceil(n / u);
      for (var i = 1; i <= c; i++) {
        var buf = new forge6.util.ByteBuffer();
        buf.putBytes(D.bytes());
        buf.putBytes(I.bytes());
        for (var round = 0; round < iter; round++) {
          md.start();
          md.update(buf.getBytes());
          buf = md.digest();
        }
        var B = new forge6.util.ByteBuffer();
        for (l = 0; l < v; l++) {
          B.putByte(buf.at(l % u));
        }
        var k = Math.ceil(s / v) + Math.ceil(p / v);
        var Inew = new forge6.util.ByteBuffer();
        for (j = 0; j < k; j++) {
          var chunk = new forge6.util.ByteBuffer(I.getBytes(v));
          var x = 511;
          for (l = B.length() - 1; l >= 0; l--) {
            x = x >> 8;
            x += B.at(l) + chunk.at(l);
            chunk.setAt(l, x & 255);
          }
          Inew.putBuffer(chunk);
        }
        I = Inew;
        result.putBuffer(buf);
      }
      result.truncate(result.length() - n);
      return result;
    };
    pki.pbe.getCipher = function(oid, params, password) {
      switch (oid) {
        case pki.oids["pkcs5PBES2"]:
          return pki.pbe.getCipherForPBES2(oid, params, password);
        case pki.oids["pbeWithSHAAnd3-KeyTripleDES-CBC"]:
        case pki.oids["pbewithSHAAnd40BitRC2-CBC"]:
          return pki.pbe.getCipherForPKCS12PBE(oid, params, password);
        default:
          var error = new Error("Cannot read encrypted PBE data block. Unsupported OID.");
          error.oid = oid;
          error.supportedOids = [
            "pkcs5PBES2",
            "pbeWithSHAAnd3-KeyTripleDES-CBC",
            "pbewithSHAAnd40BitRC2-CBC"
          ];
          throw error;
      }
    };
    pki.pbe.getCipherForPBES2 = function(oid, params, password) {
      var capture = {};
      var errors = [];
      if (!asn1.validate(params, PBES2AlgorithmsValidator, capture, errors)) {
        var error = new Error("Cannot read password-based-encryption algorithm parameters. ASN.1 object is not a supported EncryptedPrivateKeyInfo.");
        error.errors = errors;
        throw error;
      }
      oid = asn1.derToOid(capture.kdfOid);
      if (oid !== pki.oids["pkcs5PBKDF2"]) {
        var error = new Error("Cannot read encrypted private key. Unsupported key derivation function OID.");
        error.oid = oid;
        error.supportedOids = ["pkcs5PBKDF2"];
        throw error;
      }
      oid = asn1.derToOid(capture.encOid);
      if (oid !== pki.oids["aes128-CBC"] && oid !== pki.oids["aes192-CBC"] && oid !== pki.oids["aes256-CBC"] && oid !== pki.oids["des-EDE3-CBC"] && oid !== pki.oids["desCBC"]) {
        var error = new Error("Cannot read encrypted private key. Unsupported encryption scheme OID.");
        error.oid = oid;
        error.supportedOids = [
          "aes128-CBC",
          "aes192-CBC",
          "aes256-CBC",
          "des-EDE3-CBC",
          "desCBC"
        ];
        throw error;
      }
      var salt = capture.kdfSalt;
      var count = forge6.util.createBuffer(capture.kdfIterationCount);
      count = count.getInt(count.length() << 3);
      var dkLen;
      var cipherFn;
      switch (pki.oids[oid]) {
        case "aes128-CBC":
          dkLen = 16;
          cipherFn = forge6.aes.createDecryptionCipher;
          break;
        case "aes192-CBC":
          dkLen = 24;
          cipherFn = forge6.aes.createDecryptionCipher;
          break;
        case "aes256-CBC":
          dkLen = 32;
          cipherFn = forge6.aes.createDecryptionCipher;
          break;
        case "des-EDE3-CBC":
          dkLen = 24;
          cipherFn = forge6.des.createDecryptionCipher;
          break;
        case "desCBC":
          dkLen = 8;
          cipherFn = forge6.des.createDecryptionCipher;
          break;
      }
      var md = prfOidToMessageDigest(capture.prfOid);
      var dk = forge6.pkcs5.pbkdf2(password, salt, count, dkLen, md);
      var iv = capture.encIv;
      var cipher = cipherFn(dk);
      cipher.start(iv);
      return cipher;
    };
    pki.pbe.getCipherForPKCS12PBE = function(oid, params, password) {
      var capture = {};
      var errors = [];
      if (!asn1.validate(params, pkcs12PbeParamsValidator, capture, errors)) {
        var error = new Error("Cannot read password-based-encryption algorithm parameters. ASN.1 object is not a supported EncryptedPrivateKeyInfo.");
        error.errors = errors;
        throw error;
      }
      var salt = forge6.util.createBuffer(capture.salt);
      var count = forge6.util.createBuffer(capture.iterations);
      count = count.getInt(count.length() << 3);
      var dkLen, dIvLen, cipherFn;
      switch (oid) {
        case pki.oids["pbeWithSHAAnd3-KeyTripleDES-CBC"]:
          dkLen = 24;
          dIvLen = 8;
          cipherFn = forge6.des.startDecrypting;
          break;
        case pki.oids["pbewithSHAAnd40BitRC2-CBC"]:
          dkLen = 5;
          dIvLen = 8;
          cipherFn = function(key2, iv2) {
            var cipher = forge6.rc2.createDecryptionCipher(key2, 40);
            cipher.start(iv2, null);
            return cipher;
          };
          break;
        default:
          var error = new Error("Cannot read PKCS #12 PBE data block. Unsupported OID.");
          error.oid = oid;
          throw error;
      }
      var md = prfOidToMessageDigest(capture.prfOid);
      var key = pki.pbe.generatePkcs12Key(password, salt, 1, count, dkLen, md);
      md.start();
      var iv = pki.pbe.generatePkcs12Key(password, salt, 2, count, dIvLen, md);
      return cipherFn(key, iv);
    };
    pki.pbe.opensslDeriveBytes = function(password, salt, dkLen, md) {
      if (typeof md === "undefined" || md === null) {
        if (!("md5" in forge6.md)) {
          throw new Error('"md5" hash algorithm unavailable.');
        }
        md = forge6.md.md5.create();
      }
      if (salt === null) {
        salt = "";
      }
      var digests = [hash(md, password + salt)];
      for (var length3 = 16, i = 1; length3 < dkLen; ++i, length3 += 16) {
        digests.push(hash(md, digests[i - 1] + password + salt));
      }
      return digests.join("").substr(0, dkLen);
    };
    function hash(md, bytes) {
      return md.start().update(bytes).digest().getBytes();
    }
    function prfOidToMessageDigest(prfOid) {
      var prfAlgorithm;
      if (!prfOid) {
        prfAlgorithm = "hmacWithSHA1";
      } else {
        prfAlgorithm = pki.oids[asn1.derToOid(prfOid)];
        if (!prfAlgorithm) {
          var error = new Error("Unsupported PRF OID.");
          error.oid = prfOid;
          error.supported = [
            "hmacWithSHA1",
            "hmacWithSHA224",
            "hmacWithSHA256",
            "hmacWithSHA384",
            "hmacWithSHA512"
          ];
          throw error;
        }
      }
      return prfAlgorithmToMessageDigest(prfAlgorithm);
    }
    function prfAlgorithmToMessageDigest(prfAlgorithm) {
      var factory = forge6.md;
      switch (prfAlgorithm) {
        case "hmacWithSHA224":
          factory = forge6.md.sha512;
        case "hmacWithSHA1":
        case "hmacWithSHA256":
        case "hmacWithSHA384":
        case "hmacWithSHA512":
          prfAlgorithm = prfAlgorithm.substr(8).toLowerCase();
          break;
        default:
          var error = new Error("Unsupported PRF algorithm.");
          error.algorithm = prfAlgorithm;
          error.supported = [
            "hmacWithSHA1",
            "hmacWithSHA224",
            "hmacWithSHA256",
            "hmacWithSHA384",
            "hmacWithSHA512"
          ];
          throw error;
      }
      if (!factory || !(prfAlgorithm in factory)) {
        throw new Error("Unknown hash algorithm: " + prfAlgorithm);
      }
      return factory[prfAlgorithm].create();
    }
    function createPbkdf2Params(salt, countBytes, dkLen, prfAlgorithm) {
      var params = asn1.create(asn1.Class.UNIVERSAL, asn1.Type.SEQUENCE, true, [
        // salt
        asn1.create(
          asn1.Class.UNIVERSAL,
          asn1.Type.OCTETSTRING,
          false,
          salt
        ),
        // iteration count
        asn1.create(
          asn1.Class.UNIVERSAL,
          asn1.Type.INTEGER,
          false,
          countBytes.getBytes()
        )
      ]);
      if (prfAlgorithm !== "hmacWithSHA1") {
        params.value.push(
          // key length
          asn1.create(
            asn1.Class.UNIVERSAL,
            asn1.Type.INTEGER,
            false,
            forge6.util.hexToBytes(dkLen.toString(16))
          ),
          // AlgorithmIdentifier
          asn1.create(asn1.Class.UNIVERSAL, asn1.Type.SEQUENCE, true, [
            // algorithm
            asn1.create(
              asn1.Class.UNIVERSAL,
              asn1.Type.OID,
              false,
              asn1.oidToDer(pki.oids[prfAlgorithm]).getBytes()
            ),
            // parameters (null)
            asn1.create(asn1.Class.UNIVERSAL, asn1.Type.NULL, false, "")
          ])
        );
      }
      return params;
    }
  }
});

// ../../node_modules/@protobufjs/aspromise/index.js
var require_aspromise = __commonJS({
  "../../node_modules/@protobufjs/aspromise/index.js"(exports2, module2) {
    "use strict";
    module2.exports = asPromise;
    function asPromise(fn, ctx) {
      var params = new Array(arguments.length - 1), offset = 0, index = 2, pending = true;
      while (index < arguments.length)
        params[offset++] = arguments[index++];
      return new Promise(function executor(resolve, reject) {
        params[offset] = function callback(err) {
          if (pending) {
            pending = false;
            if (err)
              reject(err);
            else {
              var params2 = new Array(arguments.length - 1), offset2 = 0;
              while (offset2 < params2.length)
                params2[offset2++] = arguments[offset2];
              resolve.apply(null, params2);
            }
          }
        };
        try {
          fn.apply(ctx || null, params);
        } catch (err) {
          if (pending) {
            pending = false;
            reject(err);
          }
        }
      });
    }
  }
});

// ../../node_modules/@protobufjs/base64/index.js
var require_base64 = __commonJS({
  "../../node_modules/@protobufjs/base64/index.js"(exports2) {
    "use strict";
    var base643 = exports2;
    base643.length = function length3(string2) {
      var p = string2.length;
      if (!p)
        return 0;
      var n = 0;
      while (--p % 4 > 1 && string2.charAt(p) === "=")
        ++n;
      return Math.ceil(string2.length * 3) / 4 - n;
    };
    var b64 = new Array(64);
    var s64 = new Array(123);
    for (i = 0; i < 64; )
      s64[b64[i] = i < 26 ? i + 65 : i < 52 ? i + 71 : i < 62 ? i - 4 : i - 59 | 43] = i++;
    var i;
    base643.encode = function encode9(buffer, start, end) {
      var parts = null, chunk = [];
      var i2 = 0, j = 0, t;
      while (start < end) {
        var b = buffer[start++];
        switch (j) {
          case 0:
            chunk[i2++] = b64[b >> 2];
            t = (b & 3) << 4;
            j = 1;
            break;
          case 1:
            chunk[i2++] = b64[t | b >> 4];
            t = (b & 15) << 2;
            j = 2;
            break;
          case 2:
            chunk[i2++] = b64[t | b >> 6];
            chunk[i2++] = b64[b & 63];
            j = 0;
            break;
        }
        if (i2 > 8191) {
          (parts || (parts = [])).push(String.fromCharCode.apply(String, chunk));
          i2 = 0;
        }
      }
      if (j) {
        chunk[i2++] = b64[t];
        chunk[i2++] = 61;
        if (j === 1)
          chunk[i2++] = 61;
      }
      if (parts) {
        if (i2)
          parts.push(String.fromCharCode.apply(String, chunk.slice(0, i2)));
        return parts.join("");
      }
      return String.fromCharCode.apply(String, chunk.slice(0, i2));
    };
    var invalidEncoding = "invalid encoding";
    base643.decode = function decode11(string2, buffer, offset) {
      var start = offset;
      var j = 0, t;
      for (var i2 = 0; i2 < string2.length; ) {
        var c = string2.charCodeAt(i2++);
        if (c === 61 && j > 1)
          break;
        if ((c = s64[c]) === void 0)
          throw Error(invalidEncoding);
        switch (j) {
          case 0:
            t = c;
            j = 1;
            break;
          case 1:
            buffer[offset++] = t << 2 | (c & 48) >> 4;
            t = c;
            j = 2;
            break;
          case 2:
            buffer[offset++] = (t & 15) << 4 | (c & 60) >> 2;
            t = c;
            j = 3;
            break;
          case 3:
            buffer[offset++] = (t & 3) << 6 | c;
            j = 0;
            break;
        }
      }
      if (j === 1)
        throw Error(invalidEncoding);
      return offset - start;
    };
    base643.test = function test(string2) {
      return /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(string2);
    };
  }
});

// ../../node_modules/@protobufjs/eventemitter/index.js
var require_eventemitter = __commonJS({
  "../../node_modules/@protobufjs/eventemitter/index.js"(exports2, module2) {
    "use strict";
    module2.exports = EventEmitter;
    function EventEmitter() {
      this._listeners = {};
    }
    EventEmitter.prototype.on = function on(evt, fn, ctx) {
      (this._listeners[evt] || (this._listeners[evt] = [])).push({
        fn,
        ctx: ctx || this
      });
      return this;
    };
    EventEmitter.prototype.off = function off(evt, fn) {
      if (evt === void 0)
        this._listeners = {};
      else {
        if (fn === void 0)
          this._listeners[evt] = [];
        else {
          var listeners = this._listeners[evt];
          for (var i = 0; i < listeners.length; )
            if (listeners[i].fn === fn)
              listeners.splice(i, 1);
            else
              ++i;
        }
      }
      return this;
    };
    EventEmitter.prototype.emit = function emit(evt) {
      var listeners = this._listeners[evt];
      if (listeners) {
        var args = [], i = 1;
        for (; i < arguments.length; )
          args.push(arguments[i++]);
        for (i = 0; i < listeners.length; )
          listeners[i].fn.apply(listeners[i++].ctx, args);
      }
      return this;
    };
  }
});

// ../../node_modules/@protobufjs/float/index.js
var require_float = __commonJS({
  "../../node_modules/@protobufjs/float/index.js"(exports2, module2) {
    "use strict";
    module2.exports = factory(factory);
    function factory(exports3) {
      if (typeof Float32Array !== "undefined")
        (function() {
          var f32 = new Float32Array([-0]), f8b = new Uint8Array(f32.buffer), le = f8b[3] === 128;
          function writeFloat_f32_cpy(val, buf, pos) {
            f32[0] = val;
            buf[pos] = f8b[0];
            buf[pos + 1] = f8b[1];
            buf[pos + 2] = f8b[2];
            buf[pos + 3] = f8b[3];
          }
          function writeFloat_f32_rev(val, buf, pos) {
            f32[0] = val;
            buf[pos] = f8b[3];
            buf[pos + 1] = f8b[2];
            buf[pos + 2] = f8b[1];
            buf[pos + 3] = f8b[0];
          }
          exports3.writeFloatLE = le ? writeFloat_f32_cpy : writeFloat_f32_rev;
          exports3.writeFloatBE = le ? writeFloat_f32_rev : writeFloat_f32_cpy;
          function readFloat_f32_cpy(buf, pos) {
            f8b[0] = buf[pos];
            f8b[1] = buf[pos + 1];
            f8b[2] = buf[pos + 2];
            f8b[3] = buf[pos + 3];
            return f32[0];
          }
          function readFloat_f32_rev(buf, pos) {
            f8b[3] = buf[pos];
            f8b[2] = buf[pos + 1];
            f8b[1] = buf[pos + 2];
            f8b[0] = buf[pos + 3];
            return f32[0];
          }
          exports3.readFloatLE = le ? readFloat_f32_cpy : readFloat_f32_rev;
          exports3.readFloatBE = le ? readFloat_f32_rev : readFloat_f32_cpy;
        })();
      else
        (function() {
          function writeFloat_ieee754(writeUint, val, buf, pos) {
            var sign3 = val < 0 ? 1 : 0;
            if (sign3)
              val = -val;
            if (val === 0)
              writeUint(1 / val > 0 ? (
                /* positive */
                0
              ) : (
                /* negative 0 */
                2147483648
              ), buf, pos);
            else if (isNaN(val))
              writeUint(2143289344, buf, pos);
            else if (val > 34028234663852886e22)
              writeUint((sign3 << 31 | 2139095040) >>> 0, buf, pos);
            else if (val < 11754943508222875e-54)
              writeUint((sign3 << 31 | Math.round(val / 1401298464324817e-60)) >>> 0, buf, pos);
            else {
              var exponent = Math.floor(Math.log(val) / Math.LN2), mantissa = Math.round(val * Math.pow(2, -exponent) * 8388608) & 8388607;
              writeUint((sign3 << 31 | exponent + 127 << 23 | mantissa) >>> 0, buf, pos);
            }
          }
          exports3.writeFloatLE = writeFloat_ieee754.bind(null, writeUintLE);
          exports3.writeFloatBE = writeFloat_ieee754.bind(null, writeUintBE);
          function readFloat_ieee754(readUint, buf, pos) {
            var uint = readUint(buf, pos), sign3 = (uint >> 31) * 2 + 1, exponent = uint >>> 23 & 255, mantissa = uint & 8388607;
            return exponent === 255 ? mantissa ? NaN : sign3 * Infinity : exponent === 0 ? sign3 * 1401298464324817e-60 * mantissa : sign3 * Math.pow(2, exponent - 150) * (mantissa + 8388608);
          }
          exports3.readFloatLE = readFloat_ieee754.bind(null, readUintLE);
          exports3.readFloatBE = readFloat_ieee754.bind(null, readUintBE);
        })();
      if (typeof Float64Array !== "undefined")
        (function() {
          var f64 = new Float64Array([-0]), f8b = new Uint8Array(f64.buffer), le = f8b[7] === 128;
          function writeDouble_f64_cpy(val, buf, pos) {
            f64[0] = val;
            buf[pos] = f8b[0];
            buf[pos + 1] = f8b[1];
            buf[pos + 2] = f8b[2];
            buf[pos + 3] = f8b[3];
            buf[pos + 4] = f8b[4];
            buf[pos + 5] = f8b[5];
            buf[pos + 6] = f8b[6];
            buf[pos + 7] = f8b[7];
          }
          function writeDouble_f64_rev(val, buf, pos) {
            f64[0] = val;
            buf[pos] = f8b[7];
            buf[pos + 1] = f8b[6];
            buf[pos + 2] = f8b[5];
            buf[pos + 3] = f8b[4];
            buf[pos + 4] = f8b[3];
            buf[pos + 5] = f8b[2];
            buf[pos + 6] = f8b[1];
            buf[pos + 7] = f8b[0];
          }
          exports3.writeDoubleLE = le ? writeDouble_f64_cpy : writeDouble_f64_rev;
          exports3.writeDoubleBE = le ? writeDouble_f64_rev : writeDouble_f64_cpy;
          function readDouble_f64_cpy(buf, pos) {
            f8b[0] = buf[pos];
            f8b[1] = buf[pos + 1];
            f8b[2] = buf[pos + 2];
            f8b[3] = buf[pos + 3];
            f8b[4] = buf[pos + 4];
            f8b[5] = buf[pos + 5];
            f8b[6] = buf[pos + 6];
            f8b[7] = buf[pos + 7];
            return f64[0];
          }
          function readDouble_f64_rev(buf, pos) {
            f8b[7] = buf[pos];
            f8b[6] = buf[pos + 1];
            f8b[5] = buf[pos + 2];
            f8b[4] = buf[pos + 3];
            f8b[3] = buf[pos + 4];
            f8b[2] = buf[pos + 5];
            f8b[1] = buf[pos + 6];
            f8b[0] = buf[pos + 7];
            return f64[0];
          }
          exports3.readDoubleLE = le ? readDouble_f64_cpy : readDouble_f64_rev;
          exports3.readDoubleBE = le ? readDouble_f64_rev : readDouble_f64_cpy;
        })();
      else
        (function() {
          function writeDouble_ieee754(writeUint, off0, off1, val, buf, pos) {
            var sign3 = val < 0 ? 1 : 0;
            if (sign3)
              val = -val;
            if (val === 0) {
              writeUint(0, buf, pos + off0);
              writeUint(1 / val > 0 ? (
                /* positive */
                0
              ) : (
                /* negative 0 */
                2147483648
              ), buf, pos + off1);
            } else if (isNaN(val)) {
              writeUint(0, buf, pos + off0);
              writeUint(2146959360, buf, pos + off1);
            } else if (val > 17976931348623157e292) {
              writeUint(0, buf, pos + off0);
              writeUint((sign3 << 31 | 2146435072) >>> 0, buf, pos + off1);
            } else {
              var mantissa;
              if (val < 22250738585072014e-324) {
                mantissa = val / 5e-324;
                writeUint(mantissa >>> 0, buf, pos + off0);
                writeUint((sign3 << 31 | mantissa / 4294967296) >>> 0, buf, pos + off1);
              } else {
                var exponent = Math.floor(Math.log(val) / Math.LN2);
                if (exponent === 1024)
                  exponent = 1023;
                mantissa = val * Math.pow(2, -exponent);
                writeUint(mantissa * 4503599627370496 >>> 0, buf, pos + off0);
                writeUint((sign3 << 31 | exponent + 1023 << 20 | mantissa * 1048576 & 1048575) >>> 0, buf, pos + off1);
              }
            }
          }
          exports3.writeDoubleLE = writeDouble_ieee754.bind(null, writeUintLE, 0, 4);
          exports3.writeDoubleBE = writeDouble_ieee754.bind(null, writeUintBE, 4, 0);
          function readDouble_ieee754(readUint, off0, off1, buf, pos) {
            var lo = readUint(buf, pos + off0), hi = readUint(buf, pos + off1);
            var sign3 = (hi >> 31) * 2 + 1, exponent = hi >>> 20 & 2047, mantissa = 4294967296 * (hi & 1048575) + lo;
            return exponent === 2047 ? mantissa ? NaN : sign3 * Infinity : exponent === 0 ? sign3 * 5e-324 * mantissa : sign3 * Math.pow(2, exponent - 1075) * (mantissa + 4503599627370496);
          }
          exports3.readDoubleLE = readDouble_ieee754.bind(null, readUintLE, 0, 4);
          exports3.readDoubleBE = readDouble_ieee754.bind(null, readUintBE, 4, 0);
        })();
      return exports3;
    }
    function writeUintLE(val, buf, pos) {
      buf[pos] = val & 255;
      buf[pos + 1] = val >>> 8 & 255;
      buf[pos + 2] = val >>> 16 & 255;
      buf[pos + 3] = val >>> 24;
    }
    function writeUintBE(val, buf, pos) {
      buf[pos] = val >>> 24;
      buf[pos + 1] = val >>> 16 & 255;
      buf[pos + 2] = val >>> 8 & 255;
      buf[pos + 3] = val & 255;
    }
    function readUintLE(buf, pos) {
      return (buf[pos] | buf[pos + 1] << 8 | buf[pos + 2] << 16 | buf[pos + 3] << 24) >>> 0;
    }
    function readUintBE(buf, pos) {
      return (buf[pos] << 24 | buf[pos + 1] << 16 | buf[pos + 2] << 8 | buf[pos + 3]) >>> 0;
    }
  }
});

// ../../node_modules/@protobufjs/inquire/index.js
var require_inquire = __commonJS({
  "../../node_modules/@protobufjs/inquire/index.js"(exports, module) {
    "use strict";
    module.exports = inquire;
    function inquire(moduleName) {
      try {
        var mod = eval("quire".replace(/^/, "re"))(moduleName);
        if (mod && (mod.length || Object.keys(mod).length))
          return mod;
      } catch (e) {
      }
      return null;
    }
  }
});

// ../../node_modules/@protobufjs/utf8/index.js
var require_utf8 = __commonJS({
  "../../node_modules/@protobufjs/utf8/index.js"(exports2) {
    "use strict";
    var utf8 = exports2;
    utf8.length = function utf8_length(string2) {
      var len = 0, c = 0;
      for (var i = 0; i < string2.length; ++i) {
        c = string2.charCodeAt(i);
        if (c < 128)
          len += 1;
        else if (c < 2048)
          len += 2;
        else if ((c & 64512) === 55296 && (string2.charCodeAt(i + 1) & 64512) === 56320) {
          ++i;
          len += 4;
        } else
          len += 3;
      }
      return len;
    };
    utf8.read = function utf8_read(buffer, start, end) {
      var len = end - start;
      if (len < 1)
        return "";
      var parts = null, chunk = [], i = 0, t;
      while (start < end) {
        t = buffer[start++];
        if (t < 128)
          chunk[i++] = t;
        else if (t > 191 && t < 224)
          chunk[i++] = (t & 31) << 6 | buffer[start++] & 63;
        else if (t > 239 && t < 365) {
          t = ((t & 7) << 18 | (buffer[start++] & 63) << 12 | (buffer[start++] & 63) << 6 | buffer[start++] & 63) - 65536;
          chunk[i++] = 55296 + (t >> 10);
          chunk[i++] = 56320 + (t & 1023);
        } else
          chunk[i++] = (t & 15) << 12 | (buffer[start++] & 63) << 6 | buffer[start++] & 63;
        if (i > 8191) {
          (parts || (parts = [])).push(String.fromCharCode.apply(String, chunk));
          i = 0;
        }
      }
      if (parts) {
        if (i)
          parts.push(String.fromCharCode.apply(String, chunk.slice(0, i)));
        return parts.join("");
      }
      return String.fromCharCode.apply(String, chunk.slice(0, i));
    };
    utf8.write = function utf8_write(string2, buffer, offset) {
      var start = offset, c1, c2;
      for (var i = 0; i < string2.length; ++i) {
        c1 = string2.charCodeAt(i);
        if (c1 < 128) {
          buffer[offset++] = c1;
        } else if (c1 < 2048) {
          buffer[offset++] = c1 >> 6 | 192;
          buffer[offset++] = c1 & 63 | 128;
        } else if ((c1 & 64512) === 55296 && ((c2 = string2.charCodeAt(i + 1)) & 64512) === 56320) {
          c1 = 65536 + ((c1 & 1023) << 10) + (c2 & 1023);
          ++i;
          buffer[offset++] = c1 >> 18 | 240;
          buffer[offset++] = c1 >> 12 & 63 | 128;
          buffer[offset++] = c1 >> 6 & 63 | 128;
          buffer[offset++] = c1 & 63 | 128;
        } else {
          buffer[offset++] = c1 >> 12 | 224;
          buffer[offset++] = c1 >> 6 & 63 | 128;
          buffer[offset++] = c1 & 63 | 128;
        }
      }
      return offset - start;
    };
  }
});

// ../../node_modules/@protobufjs/pool/index.js
var require_pool = __commonJS({
  "../../node_modules/@protobufjs/pool/index.js"(exports2, module2) {
    "use strict";
    module2.exports = pool;
    function pool(alloc, slice, size) {
      var SIZE = size || 8192;
      var MAX = SIZE >>> 1;
      var slab = null;
      var offset = SIZE;
      return function pool_alloc(size2) {
        if (size2 < 1 || size2 > MAX)
          return alloc(size2);
        if (offset + size2 > SIZE) {
          slab = alloc(SIZE);
          offset = 0;
        }
        var buf = slice.call(slab, offset, offset += size2);
        if (offset & 7)
          offset = (offset | 7) + 1;
        return buf;
      };
    }
  }
});

// ../../node_modules/protobufjs/src/util/longbits.js
var require_longbits = __commonJS({
  "../../node_modules/protobufjs/src/util/longbits.js"(exports2, module2) {
    "use strict";
    module2.exports = LongBits;
    var util2 = require_minimal();
    function LongBits(lo, hi) {
      this.lo = lo >>> 0;
      this.hi = hi >>> 0;
    }
    var zero = LongBits.zero = new LongBits(0, 0);
    zero.toNumber = function() {
      return 0;
    };
    zero.zzEncode = zero.zzDecode = function() {
      return this;
    };
    zero.length = function() {
      return 1;
    };
    var zeroHash = LongBits.zeroHash = "\0\0\0\0\0\0\0\0";
    LongBits.fromNumber = function fromNumber(value) {
      if (value === 0)
        return zero;
      var sign3 = value < 0;
      if (sign3)
        value = -value;
      var lo = value >>> 0, hi = (value - lo) / 4294967296 >>> 0;
      if (sign3) {
        hi = ~hi >>> 0;
        lo = ~lo >>> 0;
        if (++lo > 4294967295) {
          lo = 0;
          if (++hi > 4294967295)
            hi = 0;
        }
      }
      return new LongBits(lo, hi);
    };
    LongBits.from = function from5(value) {
      if (typeof value === "number")
        return LongBits.fromNumber(value);
      if (util2.isString(value)) {
        if (util2.Long)
          value = util2.Long.fromString(value);
        else
          return LongBits.fromNumber(parseInt(value, 10));
      }
      return value.low || value.high ? new LongBits(value.low >>> 0, value.high >>> 0) : zero;
    };
    LongBits.prototype.toNumber = function toNumber(unsigned) {
      if (!unsigned && this.hi >>> 31) {
        var lo = ~this.lo + 1 >>> 0, hi = ~this.hi >>> 0;
        if (!lo)
          hi = hi + 1 >>> 0;
        return -(lo + hi * 4294967296);
      }
      return this.lo + this.hi * 4294967296;
    };
    LongBits.prototype.toLong = function toLong(unsigned) {
      return util2.Long ? new util2.Long(this.lo | 0, this.hi | 0, Boolean(unsigned)) : { low: this.lo | 0, high: this.hi | 0, unsigned: Boolean(unsigned) };
    };
    var charCodeAt = String.prototype.charCodeAt;
    LongBits.fromHash = function fromHash(hash) {
      if (hash === zeroHash)
        return zero;
      return new LongBits(
        (charCodeAt.call(hash, 0) | charCodeAt.call(hash, 1) << 8 | charCodeAt.call(hash, 2) << 16 | charCodeAt.call(hash, 3) << 24) >>> 0,
        (charCodeAt.call(hash, 4) | charCodeAt.call(hash, 5) << 8 | charCodeAt.call(hash, 6) << 16 | charCodeAt.call(hash, 7) << 24) >>> 0
      );
    };
    LongBits.prototype.toHash = function toHash() {
      return String.fromCharCode(
        this.lo & 255,
        this.lo >>> 8 & 255,
        this.lo >>> 16 & 255,
        this.lo >>> 24,
        this.hi & 255,
        this.hi >>> 8 & 255,
        this.hi >>> 16 & 255,
        this.hi >>> 24
      );
    };
    LongBits.prototype.zzEncode = function zzEncode() {
      var mask = this.hi >> 31;
      this.hi = ((this.hi << 1 | this.lo >>> 31) ^ mask) >>> 0;
      this.lo = (this.lo << 1 ^ mask) >>> 0;
      return this;
    };
    LongBits.prototype.zzDecode = function zzDecode() {
      var mask = -(this.lo & 1);
      this.lo = ((this.lo >>> 1 | this.hi << 31) ^ mask) >>> 0;
      this.hi = (this.hi >>> 1 ^ mask) >>> 0;
      return this;
    };
    LongBits.prototype.length = function length3() {
      var part0 = this.lo, part1 = (this.lo >>> 28 | this.hi << 4) >>> 0, part2 = this.hi >>> 24;
      return part2 === 0 ? part1 === 0 ? part0 < 16384 ? part0 < 128 ? 1 : 2 : part0 < 2097152 ? 3 : 4 : part1 < 16384 ? part1 < 128 ? 5 : 6 : part1 < 2097152 ? 7 : 8 : part2 < 128 ? 9 : 10;
    };
  }
});

// ../../node_modules/protobufjs/src/util/minimal.js
var require_minimal = __commonJS({
  "../../node_modules/protobufjs/src/util/minimal.js"(exports2) {
    "use strict";
    var util2 = exports2;
    util2.asPromise = require_aspromise();
    util2.base64 = require_base64();
    util2.EventEmitter = require_eventemitter();
    util2.float = require_float();
    util2.inquire = require_inquire();
    util2.utf8 = require_utf8();
    util2.pool = require_pool();
    util2.LongBits = require_longbits();
    util2.isNode = Boolean(typeof global !== "undefined" && global && global.process && global.process.versions && global.process.versions.node);
    util2.global = util2.isNode && global || typeof window !== "undefined" && window || typeof self !== "undefined" && self || exports2;
    util2.emptyArray = Object.freeze ? Object.freeze([]) : (
      /* istanbul ignore next */
      []
    );
    util2.emptyObject = Object.freeze ? Object.freeze({}) : (
      /* istanbul ignore next */
      {}
    );
    util2.isInteger = Number.isInteger || /* istanbul ignore next */
    function isInteger(value) {
      return typeof value === "number" && isFinite(value) && Math.floor(value) === value;
    };
    util2.isString = function isString(value) {
      return typeof value === "string" || value instanceof String;
    };
    util2.isObject = function isObject(value) {
      return value && typeof value === "object";
    };
    util2.isset = /**
     * Checks if a property on a message is considered to be present.
     * @param {Object} obj Plain object or message instance
     * @param {string} prop Property name
     * @returns {boolean} `true` if considered to be present, otherwise `false`
     */
    util2.isSet = function isSet(obj, prop) {
      var value = obj[prop];
      if (value != null && obj.hasOwnProperty(prop))
        return typeof value !== "object" || (Array.isArray(value) ? value.length : Object.keys(value).length) > 0;
      return false;
    };
    util2.Buffer = function() {
      try {
        var Buffer2 = util2.inquire("buffer").Buffer;
        return Buffer2.prototype.utf8Write ? Buffer2 : (
          /* istanbul ignore next */
          null
        );
      } catch (e) {
        return null;
      }
    }();
    util2._Buffer_from = null;
    util2._Buffer_allocUnsafe = null;
    util2.newBuffer = function newBuffer(sizeOrArray) {
      return typeof sizeOrArray === "number" ? util2.Buffer ? util2._Buffer_allocUnsafe(sizeOrArray) : new util2.Array(sizeOrArray) : util2.Buffer ? util2._Buffer_from(sizeOrArray) : typeof Uint8Array === "undefined" ? sizeOrArray : new Uint8Array(sizeOrArray);
    };
    util2.Array = typeof Uint8Array !== "undefined" ? Uint8Array : Array;
    util2.Long = /* istanbul ignore next */
    util2.global.dcodeIO && /* istanbul ignore next */
    util2.global.dcodeIO.Long || /* istanbul ignore next */
    util2.global.Long || util2.inquire("long");
    util2.key2Re = /^true|false|0|1$/;
    util2.key32Re = /^-?(?:0|[1-9][0-9]*)$/;
    util2.key64Re = /^(?:[\\x00-\\xff]{8}|-?(?:0|[1-9][0-9]*))$/;
    util2.longToHash = function longToHash(value) {
      return value ? util2.LongBits.from(value).toHash() : util2.LongBits.zeroHash;
    };
    util2.longFromHash = function longFromHash(hash, unsigned) {
      var bits2 = util2.LongBits.fromHash(hash);
      if (util2.Long)
        return util2.Long.fromBits(bits2.lo, bits2.hi, unsigned);
      return bits2.toNumber(Boolean(unsigned));
    };
    function merge(dst, src3, ifNotSet) {
      for (var keys = Object.keys(src3), i = 0; i < keys.length; ++i)
        if (dst[keys[i]] === void 0 || !ifNotSet)
          dst[keys[i]] = src3[keys[i]];
      return dst;
    }
    util2.merge = merge;
    util2.lcFirst = function lcFirst(str) {
      return str.charAt(0).toLowerCase() + str.substring(1);
    };
    function newError(name3) {
      function CustomError(message2, properties) {
        if (!(this instanceof CustomError))
          return new CustomError(message2, properties);
        Object.defineProperty(this, "message", { get: function() {
          return message2;
        } });
        if (Error.captureStackTrace)
          Error.captureStackTrace(this, CustomError);
        else
          Object.defineProperty(this, "stack", { value: new Error().stack || "" });
        if (properties)
          merge(this, properties);
      }
      CustomError.prototype = Object.create(Error.prototype, {
        constructor: {
          value: CustomError,
          writable: true,
          enumerable: false,
          configurable: true
        },
        name: {
          get: function get() {
            return name3;
          },
          set: void 0,
          enumerable: false,
          // configurable: false would accurately preserve the behavior of
          // the original, but I'm guessing that was not intentional.
          // For an actual error subclass, this property would
          // be configurable.
          configurable: true
        },
        toString: {
          value: function value() {
            return this.name + ": " + this.message;
          },
          writable: true,
          enumerable: false,
          configurable: true
        }
      });
      return CustomError;
    }
    util2.newError = newError;
    util2.ProtocolError = newError("ProtocolError");
    util2.oneOfGetter = function getOneOf(fieldNames) {
      var fieldMap = {};
      for (var i = 0; i < fieldNames.length; ++i)
        fieldMap[fieldNames[i]] = 1;
      return function() {
        for (var keys = Object.keys(this), i2 = keys.length - 1; i2 > -1; --i2)
          if (fieldMap[keys[i2]] === 1 && this[keys[i2]] !== void 0 && this[keys[i2]] !== null)
            return keys[i2];
      };
    };
    util2.oneOfSetter = function setOneOf(fieldNames) {
      return function(name3) {
        for (var i = 0; i < fieldNames.length; ++i)
          if (fieldNames[i] !== name3)
            delete this[fieldNames[i]];
      };
    };
    util2.toJSONOptions = {
      longs: String,
      enums: String,
      bytes: String,
      json: true
    };
    util2._configure = function() {
      var Buffer2 = util2.Buffer;
      if (!Buffer2) {
        util2._Buffer_from = util2._Buffer_allocUnsafe = null;
        return;
      }
      util2._Buffer_from = Buffer2.from !== Uint8Array.from && Buffer2.from || /* istanbul ignore next */
      function Buffer_from(value, encoding) {
        return new Buffer2(value, encoding);
      };
      util2._Buffer_allocUnsafe = Buffer2.allocUnsafe || /* istanbul ignore next */
      function Buffer_allocUnsafe(size) {
        return new Buffer2(size);
      };
    };
  }
});

// ../../node_modules/protobufjs/src/reader.js
var require_reader = __commonJS({
  "../../node_modules/protobufjs/src/reader.js"(exports2, module2) {
    "use strict";
    module2.exports = Reader;
    var util2 = require_minimal();
    var BufferReader;
    var LongBits = util2.LongBits;
    var utf8 = util2.utf8;
    function indexOutOfRange(reader2, writeLength) {
      return RangeError("index out of range: " + reader2.pos + " + " + (writeLength || 1) + " > " + reader2.len);
    }
    function Reader(buffer) {
      this.buf = buffer;
      this.pos = 0;
      this.len = buffer.length;
    }
    var create_array = typeof Uint8Array !== "undefined" ? function create_typed_array(buffer) {
      if (buffer instanceof Uint8Array || Array.isArray(buffer))
        return new Reader(buffer);
      throw Error("illegal buffer");
    } : function create_array2(buffer) {
      if (Array.isArray(buffer))
        return new Reader(buffer);
      throw Error("illegal buffer");
    };
    var create5 = function create6() {
      return util2.Buffer ? function create_buffer_setup(buffer) {
        return (Reader.create = function create_buffer(buffer2) {
          return util2.Buffer.isBuffer(buffer2) ? new BufferReader(buffer2) : create_array(buffer2);
        })(buffer);
      } : create_array;
    };
    Reader.create = create5();
    Reader.prototype._slice = util2.Array.prototype.subarray || /* istanbul ignore next */
    util2.Array.prototype.slice;
    Reader.prototype.uint32 = function read_uint32_setup() {
      var value = 4294967295;
      return function read_uint32() {
        value = (this.buf[this.pos] & 127) >>> 0;
        if (this.buf[this.pos++] < 128)
          return value;
        value = (value | (this.buf[this.pos] & 127) << 7) >>> 0;
        if (this.buf[this.pos++] < 128)
          return value;
        value = (value | (this.buf[this.pos] & 127) << 14) >>> 0;
        if (this.buf[this.pos++] < 128)
          return value;
        value = (value | (this.buf[this.pos] & 127) << 21) >>> 0;
        if (this.buf[this.pos++] < 128)
          return value;
        value = (value | (this.buf[this.pos] & 15) << 28) >>> 0;
        if (this.buf[this.pos++] < 128)
          return value;
        if ((this.pos += 5) > this.len) {
          this.pos = this.len;
          throw indexOutOfRange(this, 10);
        }
        return value;
      };
    }();
    Reader.prototype.int32 = function read_int32() {
      return this.uint32() | 0;
    };
    Reader.prototype.sint32 = function read_sint32() {
      var value = this.uint32();
      return value >>> 1 ^ -(value & 1) | 0;
    };
    function readLongVarint() {
      var bits2 = new LongBits(0, 0);
      var i = 0;
      if (this.len - this.pos > 4) {
        for (; i < 4; ++i) {
          bits2.lo = (bits2.lo | (this.buf[this.pos] & 127) << i * 7) >>> 0;
          if (this.buf[this.pos++] < 128)
            return bits2;
        }
        bits2.lo = (bits2.lo | (this.buf[this.pos] & 127) << 28) >>> 0;
        bits2.hi = (bits2.hi | (this.buf[this.pos] & 127) >> 4) >>> 0;
        if (this.buf[this.pos++] < 128)
          return bits2;
        i = 0;
      } else {
        for (; i < 3; ++i) {
          if (this.pos >= this.len)
            throw indexOutOfRange(this);
          bits2.lo = (bits2.lo | (this.buf[this.pos] & 127) << i * 7) >>> 0;
          if (this.buf[this.pos++] < 128)
            return bits2;
        }
        bits2.lo = (bits2.lo | (this.buf[this.pos++] & 127) << i * 7) >>> 0;
        return bits2;
      }
      if (this.len - this.pos > 4) {
        for (; i < 5; ++i) {
          bits2.hi = (bits2.hi | (this.buf[this.pos] & 127) << i * 7 + 3) >>> 0;
          if (this.buf[this.pos++] < 128)
            return bits2;
        }
      } else {
        for (; i < 5; ++i) {
          if (this.pos >= this.len)
            throw indexOutOfRange(this);
          bits2.hi = (bits2.hi | (this.buf[this.pos] & 127) << i * 7 + 3) >>> 0;
          if (this.buf[this.pos++] < 128)
            return bits2;
        }
      }
      throw Error("invalid varint encoding");
    }
    Reader.prototype.bool = function read_bool() {
      return this.uint32() !== 0;
    };
    function readFixed32_end(buf, end) {
      return (buf[end - 4] | buf[end - 3] << 8 | buf[end - 2] << 16 | buf[end - 1] << 24) >>> 0;
    }
    Reader.prototype.fixed32 = function read_fixed32() {
      if (this.pos + 4 > this.len)
        throw indexOutOfRange(this, 4);
      return readFixed32_end(this.buf, this.pos += 4);
    };
    Reader.prototype.sfixed32 = function read_sfixed32() {
      if (this.pos + 4 > this.len)
        throw indexOutOfRange(this, 4);
      return readFixed32_end(this.buf, this.pos += 4) | 0;
    };
    function readFixed64() {
      if (this.pos + 8 > this.len)
        throw indexOutOfRange(this, 8);
      return new LongBits(readFixed32_end(this.buf, this.pos += 4), readFixed32_end(this.buf, this.pos += 4));
    }
    Reader.prototype.float = function read_float() {
      if (this.pos + 4 > this.len)
        throw indexOutOfRange(this, 4);
      var value = util2.float.readFloatLE(this.buf, this.pos);
      this.pos += 4;
      return value;
    };
    Reader.prototype.double = function read_double() {
      if (this.pos + 8 > this.len)
        throw indexOutOfRange(this, 4);
      var value = util2.float.readDoubleLE(this.buf, this.pos);
      this.pos += 8;
      return value;
    };
    Reader.prototype.bytes = function read_bytes() {
      var length3 = this.uint32(), start = this.pos, end = this.pos + length3;
      if (end > this.len)
        throw indexOutOfRange(this, length3);
      this.pos += length3;
      if (Array.isArray(this.buf))
        return this.buf.slice(start, end);
      return start === end ? new this.buf.constructor(0) : this._slice.call(this.buf, start, end);
    };
    Reader.prototype.string = function read_string() {
      var bytes = this.bytes();
      return utf8.read(bytes, 0, bytes.length);
    };
    Reader.prototype.skip = function skip(length3) {
      if (typeof length3 === "number") {
        if (this.pos + length3 > this.len)
          throw indexOutOfRange(this, length3);
        this.pos += length3;
      } else {
        do {
          if (this.pos >= this.len)
            throw indexOutOfRange(this);
        } while (this.buf[this.pos++] & 128);
      }
      return this;
    };
    Reader.prototype.skipType = function(wireType) {
      switch (wireType) {
        case 0:
          this.skip();
          break;
        case 1:
          this.skip(8);
          break;
        case 2:
          this.skip(this.uint32());
          break;
        case 3:
          while ((wireType = this.uint32() & 7) !== 4) {
            this.skipType(wireType);
          }
          break;
        case 5:
          this.skip(4);
          break;
        default:
          throw Error("invalid wire type " + wireType + " at offset " + this.pos);
      }
      return this;
    };
    Reader._configure = function(BufferReader_) {
      BufferReader = BufferReader_;
      Reader.create = create5();
      BufferReader._configure();
      var fn = util2.Long ? "toLong" : (
        /* istanbul ignore next */
        "toNumber"
      );
      util2.merge(Reader.prototype, {
        int64: function read_int64() {
          return readLongVarint.call(this)[fn](false);
        },
        uint64: function read_uint64() {
          return readLongVarint.call(this)[fn](true);
        },
        sint64: function read_sint64() {
          return readLongVarint.call(this).zzDecode()[fn](false);
        },
        fixed64: function read_fixed64() {
          return readFixed64.call(this)[fn](true);
        },
        sfixed64: function read_sfixed64() {
          return readFixed64.call(this)[fn](false);
        }
      });
    };
  }
});

// ../../node_modules/protobufjs/src/reader_buffer.js
var require_reader_buffer = __commonJS({
  "../../node_modules/protobufjs/src/reader_buffer.js"(exports2, module2) {
    "use strict";
    module2.exports = BufferReader;
    var Reader = require_reader();
    (BufferReader.prototype = Object.create(Reader.prototype)).constructor = BufferReader;
    var util2 = require_minimal();
    function BufferReader(buffer) {
      Reader.call(this, buffer);
    }
    BufferReader._configure = function() {
      if (util2.Buffer)
        BufferReader.prototype._slice = util2.Buffer.prototype.slice;
    };
    BufferReader.prototype.string = function read_string_buffer() {
      var len = this.uint32();
      return this.buf.utf8Slice ? this.buf.utf8Slice(this.pos, this.pos = Math.min(this.pos + len, this.len)) : this.buf.toString("utf-8", this.pos, this.pos = Math.min(this.pos + len, this.len));
    };
    BufferReader._configure();
  }
});

// ../../node_modules/protobufjs/src/writer.js
var require_writer = __commonJS({
  "../../node_modules/protobufjs/src/writer.js"(exports2, module2) {
    "use strict";
    module2.exports = Writer;
    var util2 = require_minimal();
    var BufferWriter;
    var LongBits = util2.LongBits;
    var base643 = util2.base64;
    var utf8 = util2.utf8;
    function Op(fn, len, val) {
      this.fn = fn;
      this.len = len;
      this.next = void 0;
      this.val = val;
    }
    function noop() {
    }
    function State(writer2) {
      this.head = writer2.head;
      this.tail = writer2.tail;
      this.len = writer2.len;
      this.next = writer2.states;
    }
    function Writer() {
      this.len = 0;
      this.head = new Op(noop, 0, 0);
      this.tail = this.head;
      this.states = null;
    }
    var create5 = function create6() {
      return util2.Buffer ? function create_buffer_setup() {
        return (Writer.create = function create_buffer() {
          return new BufferWriter();
        })();
      } : function create_array() {
        return new Writer();
      };
    };
    Writer.create = create5();
    Writer.alloc = function alloc(size) {
      return new util2.Array(size);
    };
    if (util2.Array !== Array)
      Writer.alloc = util2.pool(Writer.alloc, util2.Array.prototype.subarray);
    Writer.prototype._push = function push(fn, len, val) {
      this.tail = this.tail.next = new Op(fn, len, val);
      this.len += len;
      return this;
    };
    function writeByte(val, buf, pos) {
      buf[pos] = val & 255;
    }
    function writeVarint32(val, buf, pos) {
      while (val > 127) {
        buf[pos++] = val & 127 | 128;
        val >>>= 7;
      }
      buf[pos] = val;
    }
    function VarintOp(len, val) {
      this.len = len;
      this.next = void 0;
      this.val = val;
    }
    VarintOp.prototype = Object.create(Op.prototype);
    VarintOp.prototype.fn = writeVarint32;
    Writer.prototype.uint32 = function write_uint32(value) {
      this.len += (this.tail = this.tail.next = new VarintOp(
        (value = value >>> 0) < 128 ? 1 : value < 16384 ? 2 : value < 2097152 ? 3 : value < 268435456 ? 4 : 5,
        value
      )).len;
      return this;
    };
    Writer.prototype.int32 = function write_int32(value) {
      return value < 0 ? this._push(writeVarint64, 10, LongBits.fromNumber(value)) : this.uint32(value);
    };
    Writer.prototype.sint32 = function write_sint32(value) {
      return this.uint32((value << 1 ^ value >> 31) >>> 0);
    };
    function writeVarint64(val, buf, pos) {
      while (val.hi) {
        buf[pos++] = val.lo & 127 | 128;
        val.lo = (val.lo >>> 7 | val.hi << 25) >>> 0;
        val.hi >>>= 7;
      }
      while (val.lo > 127) {
        buf[pos++] = val.lo & 127 | 128;
        val.lo = val.lo >>> 7;
      }
      buf[pos++] = val.lo;
    }
    Writer.prototype.uint64 = function write_uint64(value) {
      var bits2 = LongBits.from(value);
      return this._push(writeVarint64, bits2.length(), bits2);
    };
    Writer.prototype.int64 = Writer.prototype.uint64;
    Writer.prototype.sint64 = function write_sint64(value) {
      var bits2 = LongBits.from(value).zzEncode();
      return this._push(writeVarint64, bits2.length(), bits2);
    };
    Writer.prototype.bool = function write_bool(value) {
      return this._push(writeByte, 1, value ? 1 : 0);
    };
    function writeFixed32(val, buf, pos) {
      buf[pos] = val & 255;
      buf[pos + 1] = val >>> 8 & 255;
      buf[pos + 2] = val >>> 16 & 255;
      buf[pos + 3] = val >>> 24;
    }
    Writer.prototype.fixed32 = function write_fixed32(value) {
      return this._push(writeFixed32, 4, value >>> 0);
    };
    Writer.prototype.sfixed32 = Writer.prototype.fixed32;
    Writer.prototype.fixed64 = function write_fixed64(value) {
      var bits2 = LongBits.from(value);
      return this._push(writeFixed32, 4, bits2.lo)._push(writeFixed32, 4, bits2.hi);
    };
    Writer.prototype.sfixed64 = Writer.prototype.fixed64;
    Writer.prototype.float = function write_float(value) {
      return this._push(util2.float.writeFloatLE, 4, value);
    };
    Writer.prototype.double = function write_double(value) {
      return this._push(util2.float.writeDoubleLE, 8, value);
    };
    var writeBytes = util2.Array.prototype.set ? function writeBytes_set(val, buf, pos) {
      buf.set(val, pos);
    } : function writeBytes_for(val, buf, pos) {
      for (var i = 0; i < val.length; ++i)
        buf[pos + i] = val[i];
    };
    Writer.prototype.bytes = function write_bytes(value) {
      var len = value.length >>> 0;
      if (!len)
        return this._push(writeByte, 1, 0);
      if (util2.isString(value)) {
        var buf = Writer.alloc(len = base643.length(value));
        base643.decode(value, buf, 0);
        value = buf;
      }
      return this.uint32(len)._push(writeBytes, len, value);
    };
    Writer.prototype.string = function write_string(value) {
      var len = utf8.length(value);
      return len ? this.uint32(len)._push(utf8.write, len, value) : this._push(writeByte, 1, 0);
    };
    Writer.prototype.fork = function fork() {
      this.states = new State(this);
      this.head = this.tail = new Op(noop, 0, 0);
      this.len = 0;
      return this;
    };
    Writer.prototype.reset = function reset() {
      if (this.states) {
        this.head = this.states.head;
        this.tail = this.states.tail;
        this.len = this.states.len;
        this.states = this.states.next;
      } else {
        this.head = this.tail = new Op(noop, 0, 0);
        this.len = 0;
      }
      return this;
    };
    Writer.prototype.ldelim = function ldelim() {
      var head = this.head, tail = this.tail, len = this.len;
      this.reset().uint32(len);
      if (len) {
        this.tail.next = head.next;
        this.tail = tail;
        this.len += len;
      }
      return this;
    };
    Writer.prototype.finish = function finish() {
      var head = this.head.next, buf = this.constructor.alloc(this.len), pos = 0;
      while (head) {
        head.fn(head.val, buf, pos);
        pos += head.len;
        head = head.next;
      }
      return buf;
    };
    Writer._configure = function(BufferWriter_) {
      BufferWriter = BufferWriter_;
      Writer.create = create5();
      BufferWriter._configure();
    };
  }
});

// ../../node_modules/protobufjs/src/writer_buffer.js
var require_writer_buffer = __commonJS({
  "../../node_modules/protobufjs/src/writer_buffer.js"(exports2, module2) {
    "use strict";
    module2.exports = BufferWriter;
    var Writer = require_writer();
    (BufferWriter.prototype = Object.create(Writer.prototype)).constructor = BufferWriter;
    var util2 = require_minimal();
    function BufferWriter() {
      Writer.call(this);
    }
    BufferWriter._configure = function() {
      BufferWriter.alloc = util2._Buffer_allocUnsafe;
      BufferWriter.writeBytesBuffer = util2.Buffer && util2.Buffer.prototype instanceof Uint8Array && util2.Buffer.prototype.set.name === "set" ? function writeBytesBuffer_set(val, buf, pos) {
        buf.set(val, pos);
      } : function writeBytesBuffer_copy(val, buf, pos) {
        if (val.copy)
          val.copy(buf, pos, 0, val.length);
        else
          for (var i = 0; i < val.length; )
            buf[pos++] = val[i++];
      };
    };
    BufferWriter.prototype.bytes = function write_bytes_buffer(value) {
      if (util2.isString(value))
        value = util2._Buffer_from(value, "base64");
      var len = value.length >>> 0;
      this.uint32(len);
      if (len)
        this._push(BufferWriter.writeBytesBuffer, len, value);
      return this;
    };
    function writeStringBuffer(val, buf, pos) {
      if (val.length < 40)
        util2.utf8.write(val, buf, pos);
      else if (buf.utf8Write)
        buf.utf8Write(val, pos);
      else
        buf.write(val, pos);
    }
    BufferWriter.prototype.string = function write_string_buffer(value) {
      var len = util2.Buffer.byteLength(value);
      this.uint32(len);
      if (len)
        this._push(writeStringBuffer, len, value);
      return this;
    };
    BufferWriter._configure();
  }
});

// ../../node_modules/node-forge/lib/sha512.js
var require_sha512 = __commonJS({
  "../../node_modules/node-forge/lib/sha512.js"(exports2, module2) {
    var forge6 = require_forge();
    require_md();
    require_util();
    var sha5123 = module2.exports = forge6.sha512 = forge6.sha512 || {};
    forge6.md.sha512 = forge6.md.algorithms.sha512 = sha5123;
    var sha384 = forge6.sha384 = forge6.sha512.sha384 = forge6.sha512.sha384 || {};
    sha384.create = function() {
      return sha5123.create("SHA-384");
    };
    forge6.md.sha384 = forge6.md.algorithms.sha384 = sha384;
    forge6.sha512.sha256 = forge6.sha512.sha256 || {
      create: function() {
        return sha5123.create("SHA-512/256");
      }
    };
    forge6.md["sha512/256"] = forge6.md.algorithms["sha512/256"] = forge6.sha512.sha256;
    forge6.sha512.sha224 = forge6.sha512.sha224 || {
      create: function() {
        return sha5123.create("SHA-512/224");
      }
    };
    forge6.md["sha512/224"] = forge6.md.algorithms["sha512/224"] = forge6.sha512.sha224;
    sha5123.create = function(algorithm) {
      if (!_initialized) {
        _init();
      }
      if (typeof algorithm === "undefined") {
        algorithm = "SHA-512";
      }
      if (!(algorithm in _states)) {
        throw new Error("Invalid SHA-512 algorithm: " + algorithm);
      }
      var _state = _states[algorithm];
      var _h = null;
      var _input = forge6.util.createBuffer();
      var _w = new Array(80);
      for (var wi = 0; wi < 80; ++wi) {
        _w[wi] = new Array(2);
      }
      var digestLength = 64;
      switch (algorithm) {
        case "SHA-384":
          digestLength = 48;
          break;
        case "SHA-512/256":
          digestLength = 32;
          break;
        case "SHA-512/224":
          digestLength = 28;
          break;
      }
      var md = {
        // SHA-512 => sha512
        algorithm: algorithm.replace("-", "").toLowerCase(),
        blockLength: 128,
        digestLength,
        // 56-bit length of message so far (does not including padding)
        messageLength: 0,
        // true message length
        fullMessageLength: null,
        // size of message length in bytes
        messageLengthSize: 16
      };
      md.start = function() {
        md.messageLength = 0;
        md.fullMessageLength = md.messageLength128 = [];
        var int32s = md.messageLengthSize / 4;
        for (var i = 0; i < int32s; ++i) {
          md.fullMessageLength.push(0);
        }
        _input = forge6.util.createBuffer();
        _h = new Array(_state.length);
        for (var i = 0; i < _state.length; ++i) {
          _h[i] = _state[i].slice(0);
        }
        return md;
      };
      md.start();
      md.update = function(msg, encoding) {
        if (encoding === "utf8") {
          msg = forge6.util.encodeUtf8(msg);
        }
        var len = msg.length;
        md.messageLength += len;
        len = [len / 4294967296 >>> 0, len >>> 0];
        for (var i = md.fullMessageLength.length - 1; i >= 0; --i) {
          md.fullMessageLength[i] += len[1];
          len[1] = len[0] + (md.fullMessageLength[i] / 4294967296 >>> 0);
          md.fullMessageLength[i] = md.fullMessageLength[i] >>> 0;
          len[0] = len[1] / 4294967296 >>> 0;
        }
        _input.putBytes(msg);
        _update(_h, _w, _input);
        if (_input.read > 2048 || _input.length() === 0) {
          _input.compact();
        }
        return md;
      };
      md.digest = function() {
        var finalBlock = forge6.util.createBuffer();
        finalBlock.putBytes(_input.bytes());
        var remaining = md.fullMessageLength[md.fullMessageLength.length - 1] + md.messageLengthSize;
        var overflow = remaining & md.blockLength - 1;
        finalBlock.putBytes(_padding.substr(0, md.blockLength - overflow));
        var next, carry;
        var bits2 = md.fullMessageLength[0] * 8;
        for (var i = 0; i < md.fullMessageLength.length - 1; ++i) {
          next = md.fullMessageLength[i + 1] * 8;
          carry = next / 4294967296 >>> 0;
          bits2 += carry;
          finalBlock.putInt32(bits2 >>> 0);
          bits2 = next >>> 0;
        }
        finalBlock.putInt32(bits2);
        var h = new Array(_h.length);
        for (var i = 0; i < _h.length; ++i) {
          h[i] = _h[i].slice(0);
        }
        _update(h, _w, finalBlock);
        var rval = forge6.util.createBuffer();
        var hlen;
        if (algorithm === "SHA-512") {
          hlen = h.length;
        } else if (algorithm === "SHA-384") {
          hlen = h.length - 2;
        } else {
          hlen = h.length - 4;
        }
        for (var i = 0; i < hlen; ++i) {
          rval.putInt32(h[i][0]);
          if (i !== hlen - 1 || algorithm !== "SHA-512/224") {
            rval.putInt32(h[i][1]);
          }
        }
        return rval;
      };
      return md;
    };
    var _padding = null;
    var _initialized = false;
    var _k = null;
    var _states = null;
    function _init() {
      _padding = String.fromCharCode(128);
      _padding += forge6.util.fillString(String.fromCharCode(0), 128);
      _k = [
        [1116352408, 3609767458],
        [1899447441, 602891725],
        [3049323471, 3964484399],
        [3921009573, 2173295548],
        [961987163, 4081628472],
        [1508970993, 3053834265],
        [2453635748, 2937671579],
        [2870763221, 3664609560],
        [3624381080, 2734883394],
        [310598401, 1164996542],
        [607225278, 1323610764],
        [1426881987, 3590304994],
        [1925078388, 4068182383],
        [2162078206, 991336113],
        [2614888103, 633803317],
        [3248222580, 3479774868],
        [3835390401, 2666613458],
        [4022224774, 944711139],
        [264347078, 2341262773],
        [604807628, 2007800933],
        [770255983, 1495990901],
        [1249150122, 1856431235],
        [1555081692, 3175218132],
        [1996064986, 2198950837],
        [2554220882, 3999719339],
        [2821834349, 766784016],
        [2952996808, 2566594879],
        [3210313671, 3203337956],
        [3336571891, 1034457026],
        [3584528711, 2466948901],
        [113926993, 3758326383],
        [338241895, 168717936],
        [666307205, 1188179964],
        [773529912, 1546045734],
        [1294757372, 1522805485],
        [1396182291, 2643833823],
        [1695183700, 2343527390],
        [1986661051, 1014477480],
        [2177026350, 1206759142],
        [2456956037, 344077627],
        [2730485921, 1290863460],
        [2820302411, 3158454273],
        [3259730800, 3505952657],
        [3345764771, 106217008],
        [3516065817, 3606008344],
        [3600352804, 1432725776],
        [4094571909, 1467031594],
        [275423344, 851169720],
        [430227734, 3100823752],
        [506948616, 1363258195],
        [659060556, 3750685593],
        [883997877, 3785050280],
        [958139571, 3318307427],
        [1322822218, 3812723403],
        [1537002063, 2003034995],
        [1747873779, 3602036899],
        [1955562222, 1575990012],
        [2024104815, 1125592928],
        [2227730452, 2716904306],
        [2361852424, 442776044],
        [2428436474, 593698344],
        [2756734187, 3733110249],
        [3204031479, 2999351573],
        [3329325298, 3815920427],
        [3391569614, 3928383900],
        [3515267271, 566280711],
        [3940187606, 3454069534],
        [4118630271, 4000239992],
        [116418474, 1914138554],
        [174292421, 2731055270],
        [289380356, 3203993006],
        [460393269, 320620315],
        [685471733, 587496836],
        [852142971, 1086792851],
        [1017036298, 365543100],
        [1126000580, 2618297676],
        [1288033470, 3409855158],
        [1501505948, 4234509866],
        [1607167915, 987167468],
        [1816402316, 1246189591]
      ];
      _states = {};
      _states["SHA-512"] = [
        [1779033703, 4089235720],
        [3144134277, 2227873595],
        [1013904242, 4271175723],
        [2773480762, 1595750129],
        [1359893119, 2917565137],
        [2600822924, 725511199],
        [528734635, 4215389547],
        [1541459225, 327033209]
      ];
      _states["SHA-384"] = [
        [3418070365, 3238371032],
        [1654270250, 914150663],
        [2438529370, 812702999],
        [355462360, 4144912697],
        [1731405415, 4290775857],
        [2394180231, 1750603025],
        [3675008525, 1694076839],
        [1203062813, 3204075428]
      ];
      _states["SHA-512/256"] = [
        [573645204, 4230739756],
        [2673172387, 3360449730],
        [596883563, 1867755857],
        [2520282905, 1497426621],
        [2519219938, 2827943907],
        [3193839141, 1401305490],
        [721525244, 746961066],
        [246885852, 2177182882]
      ];
      _states["SHA-512/224"] = [
        [2352822216, 424955298],
        [1944164710, 2312950998],
        [502970286, 855612546],
        [1738396948, 1479516111],
        [258812777, 2077511080],
        [2011393907, 79989058],
        [1067287976, 1780299464],
        [286451373, 2446758561]
      ];
      _initialized = true;
    }
    function _update(s, w, bytes) {
      var t1_hi, t1_lo;
      var t2_hi, t2_lo;
      var s0_hi, s0_lo;
      var s1_hi, s1_lo;
      var ch_hi, ch_lo;
      var maj_hi, maj_lo;
      var a_hi, a_lo;
      var b_hi, b_lo;
      var c_hi, c_lo;
      var d_hi, d_lo;
      var e_hi, e_lo;
      var f_hi, f_lo;
      var g_hi, g_lo;
      var h_hi, h_lo;
      var i, hi, lo, w2, w7, w15, w16;
      var len = bytes.length();
      while (len >= 128) {
        for (i = 0; i < 16; ++i) {
          w[i][0] = bytes.getInt32() >>> 0;
          w[i][1] = bytes.getInt32() >>> 0;
        }
        for (; i < 80; ++i) {
          w2 = w[i - 2];
          hi = w2[0];
          lo = w2[1];
          t1_hi = ((hi >>> 19 | lo << 13) ^ // ROTR 19
          (lo >>> 29 | hi << 3) ^ // ROTR 61/(swap + ROTR 29)
          hi >>> 6) >>> 0;
          t1_lo = ((hi << 13 | lo >>> 19) ^ // ROTR 19
          (lo << 3 | hi >>> 29) ^ // ROTR 61/(swap + ROTR 29)
          (hi << 26 | lo >>> 6)) >>> 0;
          w15 = w[i - 15];
          hi = w15[0];
          lo = w15[1];
          t2_hi = ((hi >>> 1 | lo << 31) ^ // ROTR 1
          (hi >>> 8 | lo << 24) ^ // ROTR 8
          hi >>> 7) >>> 0;
          t2_lo = ((hi << 31 | lo >>> 1) ^ // ROTR 1
          (hi << 24 | lo >>> 8) ^ // ROTR 8
          (hi << 25 | lo >>> 7)) >>> 0;
          w7 = w[i - 7];
          w16 = w[i - 16];
          lo = t1_lo + w7[1] + t2_lo + w16[1];
          w[i][0] = t1_hi + w7[0] + t2_hi + w16[0] + (lo / 4294967296 >>> 0) >>> 0;
          w[i][1] = lo >>> 0;
        }
        a_hi = s[0][0];
        a_lo = s[0][1];
        b_hi = s[1][0];
        b_lo = s[1][1];
        c_hi = s[2][0];
        c_lo = s[2][1];
        d_hi = s[3][0];
        d_lo = s[3][1];
        e_hi = s[4][0];
        e_lo = s[4][1];
        f_hi = s[5][0];
        f_lo = s[5][1];
        g_hi = s[6][0];
        g_lo = s[6][1];
        h_hi = s[7][0];
        h_lo = s[7][1];
        for (i = 0; i < 80; ++i) {
          s1_hi = ((e_hi >>> 14 | e_lo << 18) ^ // ROTR 14
          (e_hi >>> 18 | e_lo << 14) ^ // ROTR 18
          (e_lo >>> 9 | e_hi << 23)) >>> 0;
          s1_lo = ((e_hi << 18 | e_lo >>> 14) ^ // ROTR 14
          (e_hi << 14 | e_lo >>> 18) ^ // ROTR 18
          (e_lo << 23 | e_hi >>> 9)) >>> 0;
          ch_hi = (g_hi ^ e_hi & (f_hi ^ g_hi)) >>> 0;
          ch_lo = (g_lo ^ e_lo & (f_lo ^ g_lo)) >>> 0;
          s0_hi = ((a_hi >>> 28 | a_lo << 4) ^ // ROTR 28
          (a_lo >>> 2 | a_hi << 30) ^ // ROTR 34/(swap + ROTR 2)
          (a_lo >>> 7 | a_hi << 25)) >>> 0;
          s0_lo = ((a_hi << 4 | a_lo >>> 28) ^ // ROTR 28
          (a_lo << 30 | a_hi >>> 2) ^ // ROTR 34/(swap + ROTR 2)
          (a_lo << 25 | a_hi >>> 7)) >>> 0;
          maj_hi = (a_hi & b_hi | c_hi & (a_hi ^ b_hi)) >>> 0;
          maj_lo = (a_lo & b_lo | c_lo & (a_lo ^ b_lo)) >>> 0;
          lo = h_lo + s1_lo + ch_lo + _k[i][1] + w[i][1];
          t1_hi = h_hi + s1_hi + ch_hi + _k[i][0] + w[i][0] + (lo / 4294967296 >>> 0) >>> 0;
          t1_lo = lo >>> 0;
          lo = s0_lo + maj_lo;
          t2_hi = s0_hi + maj_hi + (lo / 4294967296 >>> 0) >>> 0;
          t2_lo = lo >>> 0;
          h_hi = g_hi;
          h_lo = g_lo;
          g_hi = f_hi;
          g_lo = f_lo;
          f_hi = e_hi;
          f_lo = e_lo;
          lo = d_lo + t1_lo;
          e_hi = d_hi + t1_hi + (lo / 4294967296 >>> 0) >>> 0;
          e_lo = lo >>> 0;
          d_hi = c_hi;
          d_lo = c_lo;
          c_hi = b_hi;
          c_lo = b_lo;
          b_hi = a_hi;
          b_lo = a_lo;
          lo = t1_lo + t2_lo;
          a_hi = t1_hi + t2_hi + (lo / 4294967296 >>> 0) >>> 0;
          a_lo = lo >>> 0;
        }
        lo = s[0][1] + a_lo;
        s[0][0] = s[0][0] + a_hi + (lo / 4294967296 >>> 0) >>> 0;
        s[0][1] = lo >>> 0;
        lo = s[1][1] + b_lo;
        s[1][0] = s[1][0] + b_hi + (lo / 4294967296 >>> 0) >>> 0;
        s[1][1] = lo >>> 0;
        lo = s[2][1] + c_lo;
        s[2][0] = s[2][0] + c_hi + (lo / 4294967296 >>> 0) >>> 0;
        s[2][1] = lo >>> 0;
        lo = s[3][1] + d_lo;
        s[3][0] = s[3][0] + d_hi + (lo / 4294967296 >>> 0) >>> 0;
        s[3][1] = lo >>> 0;
        lo = s[4][1] + e_lo;
        s[4][0] = s[4][0] + e_hi + (lo / 4294967296 >>> 0) >>> 0;
        s[4][1] = lo >>> 0;
        lo = s[5][1] + f_lo;
        s[5][0] = s[5][0] + f_hi + (lo / 4294967296 >>> 0) >>> 0;
        s[5][1] = lo >>> 0;
        lo = s[6][1] + g_lo;
        s[6][0] = s[6][0] + g_hi + (lo / 4294967296 >>> 0) >>> 0;
        s[6][1] = lo >>> 0;
        lo = s[7][1] + h_lo;
        s[7][0] = s[7][0] + h_hi + (lo / 4294967296 >>> 0) >>> 0;
        s[7][1] = lo >>> 0;
        len -= 128;
      }
    }
  }
});

// ../crypto/dist/src/keys/index.js
var import_asn12 = __toESM(require_asn1(), 1);
var import_pbe = __toESM(require_pbe(), 1);

// ../interface/dist/src/errors.js
var CodeError = class extends Error {
  code;
  props;
  constructor(message2, code3, props) {
    super(message2);
    this.code = code3;
    this.name = props?.name ?? "CodeError";
    this.props = props ?? {};
  }
};

// ../crypto/dist/src/keys/index.js
var import_forge5 = __toESM(require_forge(), 1);

// ../../node_modules/uint8arrays/dist/src/util/as-uint8array.js
function asUint8Array(buf) {
  if (globalThis.Buffer != null) {
    return new Uint8Array(buf.buffer, buf.byteOffset, buf.byteLength);
  }
  return buf;
}

// ../../node_modules/uint8arrays/node_modules/multiformats/src/bases/identity.js
var identity_exports = {};
__export(identity_exports, {
  identity: () => identity
});

// ../../node_modules/uint8arrays/node_modules/multiformats/vendor/base-x.js
function base(ALPHABET, name3) {
  if (ALPHABET.length >= 255) {
    throw new TypeError("Alphabet too long");
  }
  var BASE_MAP = new Uint8Array(256);
  for (var j = 0; j < BASE_MAP.length; j++) {
    BASE_MAP[j] = 255;
  }
  for (var i = 0; i < ALPHABET.length; i++) {
    var x = ALPHABET.charAt(i);
    var xc = x.charCodeAt(0);
    if (BASE_MAP[xc] !== 255) {
      throw new TypeError(x + " is ambiguous");
    }
    BASE_MAP[xc] = i;
  }
  var BASE = ALPHABET.length;
  var LEADER = ALPHABET.charAt(0);
  var FACTOR = Math.log(BASE) / Math.log(256);
  var iFACTOR = Math.log(256) / Math.log(BASE);
  function encode9(source) {
    if (source instanceof Uint8Array)
      ;
    else if (ArrayBuffer.isView(source)) {
      source = new Uint8Array(source.buffer, source.byteOffset, source.byteLength);
    } else if (Array.isArray(source)) {
      source = Uint8Array.from(source);
    }
    if (!(source instanceof Uint8Array)) {
      throw new TypeError("Expected Uint8Array");
    }
    if (source.length === 0) {
      return "";
    }
    var zeroes = 0;
    var length3 = 0;
    var pbegin = 0;
    var pend = source.length;
    while (pbegin !== pend && source[pbegin] === 0) {
      pbegin++;
      zeroes++;
    }
    var size = (pend - pbegin) * iFACTOR + 1 >>> 0;
    var b58 = new Uint8Array(size);
    while (pbegin !== pend) {
      var carry = source[pbegin];
      var i2 = 0;
      for (var it1 = size - 1; (carry !== 0 || i2 < length3) && it1 !== -1; it1--, i2++) {
        carry += 256 * b58[it1] >>> 0;
        b58[it1] = carry % BASE >>> 0;
        carry = carry / BASE >>> 0;
      }
      if (carry !== 0) {
        throw new Error("Non-zero carry");
      }
      length3 = i2;
      pbegin++;
    }
    var it2 = size - length3;
    while (it2 !== size && b58[it2] === 0) {
      it2++;
    }
    var str = LEADER.repeat(zeroes);
    for (; it2 < size; ++it2) {
      str += ALPHABET.charAt(b58[it2]);
    }
    return str;
  }
  function decodeUnsafe(source) {
    if (typeof source !== "string") {
      throw new TypeError("Expected String");
    }
    if (source.length === 0) {
      return new Uint8Array();
    }
    var psz = 0;
    if (source[psz] === " ") {
      return;
    }
    var zeroes = 0;
    var length3 = 0;
    while (source[psz] === LEADER) {
      zeroes++;
      psz++;
    }
    var size = (source.length - psz) * FACTOR + 1 >>> 0;
    var b256 = new Uint8Array(size);
    while (source[psz]) {
      var carry = BASE_MAP[source.charCodeAt(psz)];
      if (carry === 255) {
        return;
      }
      var i2 = 0;
      for (var it3 = size - 1; (carry !== 0 || i2 < length3) && it3 !== -1; it3--, i2++) {
        carry += BASE * b256[it3] >>> 0;
        b256[it3] = carry % 256 >>> 0;
        carry = carry / 256 >>> 0;
      }
      if (carry !== 0) {
        throw new Error("Non-zero carry");
      }
      length3 = i2;
      psz++;
    }
    if (source[psz] === " ") {
      return;
    }
    var it4 = size - length3;
    while (it4 !== size && b256[it4] === 0) {
      it4++;
    }
    var vch = new Uint8Array(zeroes + (size - it4));
    var j2 = zeroes;
    while (it4 !== size) {
      vch[j2++] = b256[it4++];
    }
    return vch;
  }
  function decode11(string2) {
    var buffer = decodeUnsafe(string2);
    if (buffer) {
      return buffer;
    }
    throw new Error(`Non-${name3} character`);
  }
  return {
    encode: encode9,
    decodeUnsafe,
    decode: decode11
  };
}
var src = base;
var _brrp__multiformats_scope_baseX = src;
var base_x_default = _brrp__multiformats_scope_baseX;

// ../../node_modules/uint8arrays/node_modules/multiformats/src/bytes.js
var empty = new Uint8Array(0);
var equals = (aa, bb) => {
  if (aa === bb)
    return true;
  if (aa.byteLength !== bb.byteLength) {
    return false;
  }
  for (let ii = 0; ii < aa.byteLength; ii++) {
    if (aa[ii] !== bb[ii]) {
      return false;
    }
  }
  return true;
};
var coerce = (o) => {
  if (o instanceof Uint8Array && o.constructor.name === "Uint8Array")
    return o;
  if (o instanceof ArrayBuffer)
    return new Uint8Array(o);
  if (ArrayBuffer.isView(o)) {
    return new Uint8Array(o.buffer, o.byteOffset, o.byteLength);
  }
  throw new Error("Unknown type, must be binary type");
};
var fromString = (str) => new TextEncoder().encode(str);
var toString = (b) => new TextDecoder().decode(b);

// ../../node_modules/uint8arrays/node_modules/multiformats/src/bases/base.js
var Encoder = class {
  /**
   * @param {Base} name
   * @param {Prefix} prefix
   * @param {(bytes:Uint8Array) => string} baseEncode
   */
  constructor(name3, prefix, baseEncode) {
    this.name = name3;
    this.prefix = prefix;
    this.baseEncode = baseEncode;
  }
  /**
   * @param {Uint8Array} bytes
   * @returns {API.Multibase<Prefix>}
   */
  encode(bytes) {
    if (bytes instanceof Uint8Array) {
      return `${this.prefix}${this.baseEncode(bytes)}`;
    } else {
      throw Error("Unknown type, must be binary type");
    }
  }
};
var Decoder = class {
  /**
   * @param {Base} name
   * @param {Prefix} prefix
   * @param {(text:string) => Uint8Array} baseDecode
   */
  constructor(name3, prefix, baseDecode) {
    this.name = name3;
    this.prefix = prefix;
    if (prefix.codePointAt(0) === void 0) {
      throw new Error("Invalid prefix character");
    }
    this.prefixCodePoint = /** @type {number} */
    prefix.codePointAt(0);
    this.baseDecode = baseDecode;
  }
  /**
   * @param {string} text
   */
  decode(text) {
    if (typeof text === "string") {
      if (text.codePointAt(0) !== this.prefixCodePoint) {
        throw Error(`Unable to decode multibase string ${JSON.stringify(text)}, ${this.name} decoder only supports inputs prefixed with ${this.prefix}`);
      }
      return this.baseDecode(text.slice(this.prefix.length));
    } else {
      throw Error("Can only multibase decode strings");
    }
  }
  /**
   * @template {string} OtherPrefix
   * @param {API.UnibaseDecoder<OtherPrefix>|ComposedDecoder<OtherPrefix>} decoder
   * @returns {ComposedDecoder<Prefix|OtherPrefix>}
   */
  or(decoder) {
    return or(this, decoder);
  }
};
var ComposedDecoder = class {
  /**
   * @param {Decoders<Prefix>} decoders
   */
  constructor(decoders) {
    this.decoders = decoders;
  }
  /**
   * @template {string} OtherPrefix
   * @param {API.UnibaseDecoder<OtherPrefix>|ComposedDecoder<OtherPrefix>} decoder
   * @returns {ComposedDecoder<Prefix|OtherPrefix>}
   */
  or(decoder) {
    return or(this, decoder);
  }
  /**
   * @param {string} input
   * @returns {Uint8Array}
   */
  decode(input) {
    const prefix = (
      /** @type {Prefix} */
      input[0]
    );
    const decoder = this.decoders[prefix];
    if (decoder) {
      return decoder.decode(input);
    } else {
      throw RangeError(`Unable to decode multibase string ${JSON.stringify(input)}, only inputs prefixed with ${Object.keys(this.decoders)} are supported`);
    }
  }
};
var or = (left, right) => new ComposedDecoder(
  /** @type {Decoders<L|R>} */
  {
    ...left.decoders || { [
      /** @type API.UnibaseDecoder<L> */
      left.prefix
    ]: left },
    ...right.decoders || { [
      /** @type API.UnibaseDecoder<R> */
      right.prefix
    ]: right }
  }
);
var Codec = class {
  /**
   * @param {Base} name
   * @param {Prefix} prefix
   * @param {(bytes:Uint8Array) => string} baseEncode
   * @param {(text:string) => Uint8Array} baseDecode
   */
  constructor(name3, prefix, baseEncode, baseDecode) {
    this.name = name3;
    this.prefix = prefix;
    this.baseEncode = baseEncode;
    this.baseDecode = baseDecode;
    this.encoder = new Encoder(name3, prefix, baseEncode);
    this.decoder = new Decoder(name3, prefix, baseDecode);
  }
  /**
   * @param {Uint8Array} input
   */
  encode(input) {
    return this.encoder.encode(input);
  }
  /**
   * @param {string} input
   */
  decode(input) {
    return this.decoder.decode(input);
  }
};
var from = ({ name: name3, prefix, encode: encode9, decode: decode11 }) => new Codec(name3, prefix, encode9, decode11);
var baseX = ({ prefix, name: name3, alphabet: alphabet3 }) => {
  const { encode: encode9, decode: decode11 } = base_x_default(alphabet3, name3);
  return from({
    prefix,
    name: name3,
    encode: encode9,
    /**
     * @param {string} text
     */
    decode: (text) => coerce(decode11(text))
  });
};
var decode = (string2, alphabet3, bitsPerChar, name3) => {
  const codes = {};
  for (let i = 0; i < alphabet3.length; ++i) {
    codes[alphabet3[i]] = i;
  }
  let end = string2.length;
  while (string2[end - 1] === "=") {
    --end;
  }
  const out = new Uint8Array(end * bitsPerChar / 8 | 0);
  let bits2 = 0;
  let buffer = 0;
  let written = 0;
  for (let i = 0; i < end; ++i) {
    const value = codes[string2[i]];
    if (value === void 0) {
      throw new SyntaxError(`Non-${name3} character`);
    }
    buffer = buffer << bitsPerChar | value;
    bits2 += bitsPerChar;
    if (bits2 >= 8) {
      bits2 -= 8;
      out[written++] = 255 & buffer >> bits2;
    }
  }
  if (bits2 >= bitsPerChar || 255 & buffer << 8 - bits2) {
    throw new SyntaxError("Unexpected end of data");
  }
  return out;
};
var encode = (data, alphabet3, bitsPerChar) => {
  const pad = alphabet3[alphabet3.length - 1] === "=";
  const mask = (1 << bitsPerChar) - 1;
  let out = "";
  let bits2 = 0;
  let buffer = 0;
  for (let i = 0; i < data.length; ++i) {
    buffer = buffer << 8 | data[i];
    bits2 += 8;
    while (bits2 > bitsPerChar) {
      bits2 -= bitsPerChar;
      out += alphabet3[mask & buffer >> bits2];
    }
  }
  if (bits2) {
    out += alphabet3[mask & buffer << bitsPerChar - bits2];
  }
  if (pad) {
    while (out.length * bitsPerChar & 7) {
      out += "=";
    }
  }
  return out;
};
var rfc4648 = ({ name: name3, prefix, bitsPerChar, alphabet: alphabet3 }) => {
  return from({
    prefix,
    name: name3,
    encode(input) {
      return encode(input, alphabet3, bitsPerChar);
    },
    decode(input) {
      return decode(input, alphabet3, bitsPerChar, name3);
    }
  });
};

// ../../node_modules/uint8arrays/node_modules/multiformats/src/bases/identity.js
var identity = from({
  prefix: "\0",
  name: "identity",
  encode: (buf) => toString(buf),
  decode: (str) => fromString(str)
});

// ../../node_modules/uint8arrays/node_modules/multiformats/src/bases/base2.js
var base2_exports = {};
__export(base2_exports, {
  base2: () => base2
});
var base2 = rfc4648({
  prefix: "0",
  name: "base2",
  alphabet: "01",
  bitsPerChar: 1
});

// ../../node_modules/uint8arrays/node_modules/multiformats/src/bases/base8.js
var base8_exports = {};
__export(base8_exports, {
  base8: () => base8
});
var base8 = rfc4648({
  prefix: "7",
  name: "base8",
  alphabet: "01234567",
  bitsPerChar: 3
});

// ../../node_modules/uint8arrays/node_modules/multiformats/src/bases/base10.js
var base10_exports = {};
__export(base10_exports, {
  base10: () => base10
});
var base10 = baseX({
  prefix: "9",
  name: "base10",
  alphabet: "0123456789"
});

// ../../node_modules/uint8arrays/node_modules/multiformats/src/bases/base16.js
var base16_exports = {};
__export(base16_exports, {
  base16: () => base16,
  base16upper: () => base16upper
});
var base16 = rfc4648({
  prefix: "f",
  name: "base16",
  alphabet: "0123456789abcdef",
  bitsPerChar: 4
});
var base16upper = rfc4648({
  prefix: "F",
  name: "base16upper",
  alphabet: "0123456789ABCDEF",
  bitsPerChar: 4
});

// ../../node_modules/uint8arrays/node_modules/multiformats/src/bases/base32.js
var base32_exports = {};
__export(base32_exports, {
  base32: () => base32,
  base32hex: () => base32hex,
  base32hexpad: () => base32hexpad,
  base32hexpadupper: () => base32hexpadupper,
  base32hexupper: () => base32hexupper,
  base32pad: () => base32pad,
  base32padupper: () => base32padupper,
  base32upper: () => base32upper,
  base32z: () => base32z
});
var base32 = rfc4648({
  prefix: "b",
  name: "base32",
  alphabet: "abcdefghijklmnopqrstuvwxyz234567",
  bitsPerChar: 5
});
var base32upper = rfc4648({
  prefix: "B",
  name: "base32upper",
  alphabet: "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567",
  bitsPerChar: 5
});
var base32pad = rfc4648({
  prefix: "c",
  name: "base32pad",
  alphabet: "abcdefghijklmnopqrstuvwxyz234567=",
  bitsPerChar: 5
});
var base32padupper = rfc4648({
  prefix: "C",
  name: "base32padupper",
  alphabet: "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567=",
  bitsPerChar: 5
});
var base32hex = rfc4648({
  prefix: "v",
  name: "base32hex",
  alphabet: "0123456789abcdefghijklmnopqrstuv",
  bitsPerChar: 5
});
var base32hexupper = rfc4648({
  prefix: "V",
  name: "base32hexupper",
  alphabet: "0123456789ABCDEFGHIJKLMNOPQRSTUV",
  bitsPerChar: 5
});
var base32hexpad = rfc4648({
  prefix: "t",
  name: "base32hexpad",
  alphabet: "0123456789abcdefghijklmnopqrstuv=",
  bitsPerChar: 5
});
var base32hexpadupper = rfc4648({
  prefix: "T",
  name: "base32hexpadupper",
  alphabet: "0123456789ABCDEFGHIJKLMNOPQRSTUV=",
  bitsPerChar: 5
});
var base32z = rfc4648({
  prefix: "h",
  name: "base32z",
  alphabet: "ybndrfg8ejkmcpqxot1uwisza345h769",
  bitsPerChar: 5
});

// ../../node_modules/uint8arrays/node_modules/multiformats/src/bases/base36.js
var base36_exports = {};
__export(base36_exports, {
  base36: () => base36,
  base36upper: () => base36upper
});
var base36 = baseX({
  prefix: "k",
  name: "base36",
  alphabet: "0123456789abcdefghijklmnopqrstuvwxyz"
});
var base36upper = baseX({
  prefix: "K",
  name: "base36upper",
  alphabet: "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ"
});

// ../../node_modules/uint8arrays/node_modules/multiformats/src/bases/base58.js
var base58_exports = {};
__export(base58_exports, {
  base58btc: () => base58btc,
  base58flickr: () => base58flickr
});
var base58btc = baseX({
  name: "base58btc",
  prefix: "z",
  alphabet: "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
});
var base58flickr = baseX({
  name: "base58flickr",
  prefix: "Z",
  alphabet: "123456789abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ"
});

// ../../node_modules/uint8arrays/node_modules/multiformats/src/bases/base64.js
var base64_exports = {};
__export(base64_exports, {
  base64: () => base64,
  base64pad: () => base64pad,
  base64url: () => base64url,
  base64urlpad: () => base64urlpad
});
var base64 = rfc4648({
  prefix: "m",
  name: "base64",
  alphabet: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/",
  bitsPerChar: 6
});
var base64pad = rfc4648({
  prefix: "M",
  name: "base64pad",
  alphabet: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=",
  bitsPerChar: 6
});
var base64url = rfc4648({
  prefix: "u",
  name: "base64url",
  alphabet: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_",
  bitsPerChar: 6
});
var base64urlpad = rfc4648({
  prefix: "U",
  name: "base64urlpad",
  alphabet: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_=",
  bitsPerChar: 6
});

// ../../node_modules/uint8arrays/node_modules/multiformats/src/bases/base256emoji.js
var base256emoji_exports = {};
__export(base256emoji_exports, {
  base256emoji: () => base256emoji
});
var alphabet = Array.from("\u{1F680}\u{1FA90}\u2604\u{1F6F0}\u{1F30C}\u{1F311}\u{1F312}\u{1F313}\u{1F314}\u{1F315}\u{1F316}\u{1F317}\u{1F318}\u{1F30D}\u{1F30F}\u{1F30E}\u{1F409}\u2600\u{1F4BB}\u{1F5A5}\u{1F4BE}\u{1F4BF}\u{1F602}\u2764\u{1F60D}\u{1F923}\u{1F60A}\u{1F64F}\u{1F495}\u{1F62D}\u{1F618}\u{1F44D}\u{1F605}\u{1F44F}\u{1F601}\u{1F525}\u{1F970}\u{1F494}\u{1F496}\u{1F499}\u{1F622}\u{1F914}\u{1F606}\u{1F644}\u{1F4AA}\u{1F609}\u263A\u{1F44C}\u{1F917}\u{1F49C}\u{1F614}\u{1F60E}\u{1F607}\u{1F339}\u{1F926}\u{1F389}\u{1F49E}\u270C\u2728\u{1F937}\u{1F631}\u{1F60C}\u{1F338}\u{1F64C}\u{1F60B}\u{1F497}\u{1F49A}\u{1F60F}\u{1F49B}\u{1F642}\u{1F493}\u{1F929}\u{1F604}\u{1F600}\u{1F5A4}\u{1F603}\u{1F4AF}\u{1F648}\u{1F447}\u{1F3B6}\u{1F612}\u{1F92D}\u2763\u{1F61C}\u{1F48B}\u{1F440}\u{1F62A}\u{1F611}\u{1F4A5}\u{1F64B}\u{1F61E}\u{1F629}\u{1F621}\u{1F92A}\u{1F44A}\u{1F973}\u{1F625}\u{1F924}\u{1F449}\u{1F483}\u{1F633}\u270B\u{1F61A}\u{1F61D}\u{1F634}\u{1F31F}\u{1F62C}\u{1F643}\u{1F340}\u{1F337}\u{1F63B}\u{1F613}\u2B50\u2705\u{1F97A}\u{1F308}\u{1F608}\u{1F918}\u{1F4A6}\u2714\u{1F623}\u{1F3C3}\u{1F490}\u2639\u{1F38A}\u{1F498}\u{1F620}\u261D\u{1F615}\u{1F33A}\u{1F382}\u{1F33B}\u{1F610}\u{1F595}\u{1F49D}\u{1F64A}\u{1F639}\u{1F5E3}\u{1F4AB}\u{1F480}\u{1F451}\u{1F3B5}\u{1F91E}\u{1F61B}\u{1F534}\u{1F624}\u{1F33C}\u{1F62B}\u26BD\u{1F919}\u2615\u{1F3C6}\u{1F92B}\u{1F448}\u{1F62E}\u{1F646}\u{1F37B}\u{1F343}\u{1F436}\u{1F481}\u{1F632}\u{1F33F}\u{1F9E1}\u{1F381}\u26A1\u{1F31E}\u{1F388}\u274C\u270A\u{1F44B}\u{1F630}\u{1F928}\u{1F636}\u{1F91D}\u{1F6B6}\u{1F4B0}\u{1F353}\u{1F4A2}\u{1F91F}\u{1F641}\u{1F6A8}\u{1F4A8}\u{1F92C}\u2708\u{1F380}\u{1F37A}\u{1F913}\u{1F619}\u{1F49F}\u{1F331}\u{1F616}\u{1F476}\u{1F974}\u25B6\u27A1\u2753\u{1F48E}\u{1F4B8}\u2B07\u{1F628}\u{1F31A}\u{1F98B}\u{1F637}\u{1F57A}\u26A0\u{1F645}\u{1F61F}\u{1F635}\u{1F44E}\u{1F932}\u{1F920}\u{1F927}\u{1F4CC}\u{1F535}\u{1F485}\u{1F9D0}\u{1F43E}\u{1F352}\u{1F617}\u{1F911}\u{1F30A}\u{1F92F}\u{1F437}\u260E\u{1F4A7}\u{1F62F}\u{1F486}\u{1F446}\u{1F3A4}\u{1F647}\u{1F351}\u2744\u{1F334}\u{1F4A3}\u{1F438}\u{1F48C}\u{1F4CD}\u{1F940}\u{1F922}\u{1F445}\u{1F4A1}\u{1F4A9}\u{1F450}\u{1F4F8}\u{1F47B}\u{1F910}\u{1F92E}\u{1F3BC}\u{1F975}\u{1F6A9}\u{1F34E}\u{1F34A}\u{1F47C}\u{1F48D}\u{1F4E3}\u{1F942}");
var alphabetBytesToChars = (
  /** @type {string[]} */
  alphabet.reduce(
    (p, c, i) => {
      p[i] = c;
      return p;
    },
    /** @type {string[]} */
    []
  )
);
var alphabetCharsToBytes = (
  /** @type {number[]} */
  alphabet.reduce(
    (p, c, i) => {
      p[
        /** @type {number} */
        c.codePointAt(0)
      ] = i;
      return p;
    },
    /** @type {number[]} */
    []
  )
);
function encode2(data) {
  return data.reduce((p, c) => {
    p += alphabetBytesToChars[c];
    return p;
  }, "");
}
function decode2(str) {
  const byts = [];
  for (const char of str) {
    const byt = alphabetCharsToBytes[
      /** @type {number} */
      char.codePointAt(0)
    ];
    if (byt === void 0) {
      throw new Error(`Non-base256emoji character: ${char}`);
    }
    byts.push(byt);
  }
  return new Uint8Array(byts);
}
var base256emoji = from({
  prefix: "\u{1F680}",
  name: "base256emoji",
  encode: encode2,
  decode: decode2
});

// ../../node_modules/uint8arrays/node_modules/multiformats/src/hashes/sha2-browser.js
var sha2_browser_exports = {};
__export(sha2_browser_exports, {
  sha256: () => sha256,
  sha512: () => sha512
});

// ../../node_modules/uint8arrays/node_modules/multiformats/vendor/varint.js
var encode_1 = encode3;
var MSB = 128;
var REST = 127;
var MSBALL = ~REST;
var INT = Math.pow(2, 31);
function encode3(num, out, offset) {
  out = out || [];
  offset = offset || 0;
  var oldOffset = offset;
  while (num >= INT) {
    out[offset++] = num & 255 | MSB;
    num /= 128;
  }
  while (num & MSBALL) {
    out[offset++] = num & 255 | MSB;
    num >>>= 7;
  }
  out[offset] = num | 0;
  encode3.bytes = offset - oldOffset + 1;
  return out;
}
var decode3 = read;
var MSB$1 = 128;
var REST$1 = 127;
function read(buf, offset) {
  var res = 0, offset = offset || 0, shift = 0, counter = offset, b, l = buf.length;
  do {
    if (counter >= l) {
      read.bytes = 0;
      throw new RangeError("Could not decode varint");
    }
    b = buf[counter++];
    res += shift < 28 ? (b & REST$1) << shift : (b & REST$1) * Math.pow(2, shift);
    shift += 7;
  } while (b >= MSB$1);
  read.bytes = counter - offset;
  return res;
}
var N1 = Math.pow(2, 7);
var N2 = Math.pow(2, 14);
var N3 = Math.pow(2, 21);
var N4 = Math.pow(2, 28);
var N5 = Math.pow(2, 35);
var N6 = Math.pow(2, 42);
var N7 = Math.pow(2, 49);
var N8 = Math.pow(2, 56);
var N9 = Math.pow(2, 63);
var length = function(value) {
  return value < N1 ? 1 : value < N2 ? 2 : value < N3 ? 3 : value < N4 ? 4 : value < N5 ? 5 : value < N6 ? 6 : value < N7 ? 7 : value < N8 ? 8 : value < N9 ? 9 : 10;
};
var varint = {
  encode: encode_1,
  decode: decode3,
  encodingLength: length
};
var _brrp_varint = varint;
var varint_default = _brrp_varint;

// ../../node_modules/uint8arrays/node_modules/multiformats/src/varint.js
var decode4 = (data, offset = 0) => {
  const code3 = varint_default.decode(data, offset);
  return [code3, varint_default.decode.bytes];
};
var encodeTo = (int, target, offset = 0) => {
  varint_default.encode(int, target, offset);
  return target;
};
var encodingLength = (int) => {
  return varint_default.encodingLength(int);
};

// ../../node_modules/uint8arrays/node_modules/multiformats/src/hashes/digest.js
var create = (code3, digest3) => {
  const size = digest3.byteLength;
  const sizeOffset = encodingLength(code3);
  const digestOffset = sizeOffset + encodingLength(size);
  const bytes = new Uint8Array(digestOffset + size);
  encodeTo(code3, bytes, 0);
  encodeTo(size, bytes, sizeOffset);
  bytes.set(digest3, digestOffset);
  return new Digest(code3, size, digest3, bytes);
};
var decode5 = (multihash) => {
  const bytes = coerce(multihash);
  const [code3, sizeOffset] = decode4(bytes);
  const [size, digestOffset] = decode4(bytes.subarray(sizeOffset));
  const digest3 = bytes.subarray(sizeOffset + digestOffset);
  if (digest3.byteLength !== size) {
    throw new Error("Incorrect length");
  }
  return new Digest(code3, size, digest3, bytes);
};
var equals2 = (a, b) => {
  if (a === b) {
    return true;
  } else {
    const data = (
      /** @type {{code?:unknown, size?:unknown, bytes?:unknown}} */
      b
    );
    return a.code === data.code && a.size === data.size && data.bytes instanceof Uint8Array && equals(a.bytes, data.bytes);
  }
};
var Digest = class {
  /**
   * Creates a multihash digest.
   *
   * @param {Code} code
   * @param {Size} size
   * @param {Uint8Array} digest
   * @param {Uint8Array} bytes
   */
  constructor(code3, size, digest3, bytes) {
    this.code = code3;
    this.size = size;
    this.digest = digest3;
    this.bytes = bytes;
  }
};

// ../../node_modules/uint8arrays/node_modules/multiformats/src/hashes/hasher.js
var from2 = ({ name: name3, code: code3, encode: encode9 }) => new Hasher(name3, code3, encode9);
var Hasher = class {
  /**
   *
   * @param {Name} name
   * @param {Code} code
   * @param {(input: Uint8Array) => Await<Uint8Array>} encode
   */
  constructor(name3, code3, encode9) {
    this.name = name3;
    this.code = code3;
    this.encode = encode9;
  }
  /**
   * @param {Uint8Array} input
   * @returns {Await<Digest.Digest<Code, number>>}
   */
  digest(input) {
    if (input instanceof Uint8Array) {
      const result = this.encode(input);
      return result instanceof Uint8Array ? create(this.code, result) : result.then((digest3) => create(this.code, digest3));
    } else {
      throw Error("Unknown type, must be binary type");
    }
  }
};

// ../../node_modules/uint8arrays/node_modules/multiformats/src/hashes/sha2-browser.js
var sha = (name3) => (
  /**
   * @param {Uint8Array} data
   */
  async (data) => new Uint8Array(await crypto.subtle.digest(name3, data))
);
var sha256 = from2({
  name: "sha2-256",
  code: 18,
  encode: sha("SHA-256")
});
var sha512 = from2({
  name: "sha2-512",
  code: 19,
  encode: sha("SHA-512")
});

// ../../node_modules/uint8arrays/node_modules/multiformats/src/hashes/identity.js
var identity_exports2 = {};
__export(identity_exports2, {
  identity: () => identity2
});
var code = 0;
var name = "identity";
var encode4 = coerce;
var digest = (input) => create(code, encode4(input));
var identity2 = { code, name, encode: encode4, digest };

// ../../node_modules/uint8arrays/node_modules/multiformats/src/codecs/json.js
var textEncoder = new TextEncoder();
var textDecoder = new TextDecoder();

// ../../node_modules/uint8arrays/node_modules/multiformats/src/cid.js
var format = (link, base4) => {
  const { bytes, version } = link;
  switch (version) {
    case 0:
      return toStringV0(
        bytes,
        baseCache(link),
        /** @type {API.MultibaseEncoder<"z">} */
        base4 || base58btc.encoder
      );
    default:
      return toStringV1(
        bytes,
        baseCache(link),
        /** @type {API.MultibaseEncoder<Prefix>} */
        base4 || base32.encoder
      );
  }
};
var cache = /* @__PURE__ */ new WeakMap();
var baseCache = (cid) => {
  const baseCache3 = cache.get(cid);
  if (baseCache3 == null) {
    const baseCache4 = /* @__PURE__ */ new Map();
    cache.set(cid, baseCache4);
    return baseCache4;
  }
  return baseCache3;
};
var CID = class _CID {
  /**
   * @param {Version} version - Version of the CID
   * @param {Format} code - Code of the codec content is encoded in, see https://github.com/multiformats/multicodec/blob/master/table.csv
   * @param {API.MultihashDigest<Alg>} multihash - (Multi)hash of the of the content.
   * @param {Uint8Array} bytes
   *
   */
  constructor(version, code3, multihash, bytes) {
    this.code = code3;
    this.version = version;
    this.multihash = multihash;
    this.bytes = bytes;
    this["/"] = bytes;
  }
  /**
   * Signalling `cid.asCID === cid` has been replaced with `cid['/'] === cid.bytes`
   * please either use `CID.asCID(cid)` or switch to new signalling mechanism
   *
   * @deprecated
   */
  get asCID() {
    return this;
  }
  // ArrayBufferView
  get byteOffset() {
    return this.bytes.byteOffset;
  }
  // ArrayBufferView
  get byteLength() {
    return this.bytes.byteLength;
  }
  /**
   * @returns {CID<Data, API.DAG_PB, API.SHA_256, 0>}
   */
  toV0() {
    switch (this.version) {
      case 0: {
        return (
          /** @type {CID<Data, API.DAG_PB, API.SHA_256, 0>} */
          this
        );
      }
      case 1: {
        const { code: code3, multihash } = this;
        if (code3 !== DAG_PB_CODE) {
          throw new Error("Cannot convert a non dag-pb CID to CIDv0");
        }
        if (multihash.code !== SHA_256_CODE) {
          throw new Error("Cannot convert non sha2-256 multihash CID to CIDv0");
        }
        return (
          /** @type {CID<Data, API.DAG_PB, API.SHA_256, 0>} */
          _CID.createV0(
            /** @type {API.MultihashDigest<API.SHA_256>} */
            multihash
          )
        );
      }
      default: {
        throw Error(
          `Can not convert CID version ${this.version} to version 0. This is a bug please report`
        );
      }
    }
  }
  /**
   * @returns {CID<Data, Format, Alg, 1>}
   */
  toV1() {
    switch (this.version) {
      case 0: {
        const { code: code3, digest: digest3 } = this.multihash;
        const multihash = create(code3, digest3);
        return (
          /** @type {CID<Data, Format, Alg, 1>} */
          _CID.createV1(this.code, multihash)
        );
      }
      case 1: {
        return (
          /** @type {CID<Data, Format, Alg, 1>} */
          this
        );
      }
      default: {
        throw Error(
          `Can not convert CID version ${this.version} to version 1. This is a bug please report`
        );
      }
    }
  }
  /**
   * @param {unknown} other
   * @returns {other is CID<Data, Format, Alg, Version>}
   */
  equals(other) {
    return _CID.equals(this, other);
  }
  /**
   * @template {unknown} Data
   * @template {number} Format
   * @template {number} Alg
   * @template {API.Version} Version
   * @param {API.Link<Data, Format, Alg, Version>} self
   * @param {unknown} other
   * @returns {other is CID}
   */
  static equals(self2, other) {
    const unknown = (
      /** @type {{code?:unknown, version?:unknown, multihash?:unknown}} */
      other
    );
    return unknown && self2.code === unknown.code && self2.version === unknown.version && equals2(self2.multihash, unknown.multihash);
  }
  /**
   * @param {API.MultibaseEncoder<string>} [base]
   * @returns {string}
   */
  toString(base4) {
    return format(this, base4);
  }
  toJSON() {
    return { "/": format(this) };
  }
  link() {
    return this;
  }
  get [Symbol.toStringTag]() {
    return "CID";
  }
  // Legacy
  [Symbol.for("nodejs.util.inspect.custom")]() {
    return `CID(${this.toString()})`;
  }
  /**
   * Takes any input `value` and returns a `CID` instance if it was
   * a `CID` otherwise returns `null`. If `value` is instanceof `CID`
   * it will return value back. If `value` is not instance of this CID
   * class, but is compatible CID it will return new instance of this
   * `CID` class. Otherwise returns null.
   *
   * This allows two different incompatible versions of CID library to
   * co-exist and interop as long as binary interface is compatible.
   *
   * @template {unknown} Data
   * @template {number} Format
   * @template {number} Alg
   * @template {API.Version} Version
   * @template {unknown} U
   * @param {API.Link<Data, Format, Alg, Version>|U} input
   * @returns {CID<Data, Format, Alg, Version>|null}
   */
  static asCID(input) {
    if (input == null) {
      return null;
    }
    const value = (
      /** @type {any} */
      input
    );
    if (value instanceof _CID) {
      return value;
    } else if (value["/"] != null && value["/"] === value.bytes || value.asCID === value) {
      const { version, code: code3, multihash, bytes } = value;
      return new _CID(
        version,
        code3,
        /** @type {API.MultihashDigest<Alg>} */
        multihash,
        bytes || encodeCID(version, code3, multihash.bytes)
      );
    } else if (value[cidSymbol] === true) {
      const { version, multihash, code: code3 } = value;
      const digest3 = (
        /** @type {API.MultihashDigest<Alg>} */
        decode5(multihash)
      );
      return _CID.create(version, code3, digest3);
    } else {
      return null;
    }
  }
  /**
   *
   * @template {unknown} Data
   * @template {number} Format
   * @template {number} Alg
   * @template {API.Version} Version
   * @param {Version} version - Version of the CID
   * @param {Format} code - Code of the codec content is encoded in, see https://github.com/multiformats/multicodec/blob/master/table.csv
   * @param {API.MultihashDigest<Alg>} digest - (Multi)hash of the of the content.
   * @returns {CID<Data, Format, Alg, Version>}
   */
  static create(version, code3, digest3) {
    if (typeof code3 !== "number") {
      throw new Error("String codecs are no longer supported");
    }
    if (!(digest3.bytes instanceof Uint8Array)) {
      throw new Error("Invalid digest");
    }
    switch (version) {
      case 0: {
        if (code3 !== DAG_PB_CODE) {
          throw new Error(
            `Version 0 CID must use dag-pb (code: ${DAG_PB_CODE}) block encoding`
          );
        } else {
          return new _CID(version, code3, digest3, digest3.bytes);
        }
      }
      case 1: {
        const bytes = encodeCID(version, code3, digest3.bytes);
        return new _CID(version, code3, digest3, bytes);
      }
      default: {
        throw new Error("Invalid version");
      }
    }
  }
  /**
   * Simplified version of `create` for CIDv0.
   *
   * @template {unknown} [T=unknown]
   * @param {API.MultihashDigest<typeof SHA_256_CODE>} digest - Multihash.
   * @returns {CID<T, typeof DAG_PB_CODE, typeof SHA_256_CODE, 0>}
   */
  static createV0(digest3) {
    return _CID.create(0, DAG_PB_CODE, digest3);
  }
  /**
   * Simplified version of `create` for CIDv1.
   *
   * @template {unknown} Data
   * @template {number} Code
   * @template {number} Alg
   * @param {Code} code - Content encoding format code.
   * @param {API.MultihashDigest<Alg>} digest - Miltihash of the content.
   * @returns {CID<Data, Code, Alg, 1>}
   */
  static createV1(code3, digest3) {
    return _CID.create(1, code3, digest3);
  }
  /**
   * Decoded a CID from its binary representation. The byte array must contain
   * only the CID with no additional bytes.
   *
   * An error will be thrown if the bytes provided do not contain a valid
   * binary representation of a CID.
   *
   * @template {unknown} Data
   * @template {number} Code
   * @template {number} Alg
   * @template {API.Version} Ver
   * @param {API.ByteView<API.Link<Data, Code, Alg, Ver>>} bytes
   * @returns {CID<Data, Code, Alg, Ver>}
   */
  static decode(bytes) {
    const [cid, remainder] = _CID.decodeFirst(bytes);
    if (remainder.length) {
      throw new Error("Incorrect length");
    }
    return cid;
  }
  /**
   * Decoded a CID from its binary representation at the beginning of a byte
   * array.
   *
   * Returns an array with the first element containing the CID and the second
   * element containing the remainder of the original byte array. The remainder
   * will be a zero-length byte array if the provided bytes only contained a
   * binary CID representation.
   *
   * @template {unknown} T
   * @template {number} C
   * @template {number} A
   * @template {API.Version} V
   * @param {API.ByteView<API.Link<T, C, A, V>>} bytes
   * @returns {[CID<T, C, A, V>, Uint8Array]}
   */
  static decodeFirst(bytes) {
    const specs = _CID.inspectBytes(bytes);
    const prefixSize = specs.size - specs.multihashSize;
    const multihashBytes = coerce(
      bytes.subarray(prefixSize, prefixSize + specs.multihashSize)
    );
    if (multihashBytes.byteLength !== specs.multihashSize) {
      throw new Error("Incorrect length");
    }
    const digestBytes = multihashBytes.subarray(
      specs.multihashSize - specs.digestSize
    );
    const digest3 = new Digest(
      specs.multihashCode,
      specs.digestSize,
      digestBytes,
      multihashBytes
    );
    const cid = specs.version === 0 ? _CID.createV0(
      /** @type {API.MultihashDigest<API.SHA_256>} */
      digest3
    ) : _CID.createV1(specs.codec, digest3);
    return [
      /** @type {CID<T, C, A, V>} */
      cid,
      bytes.subarray(specs.size)
    ];
  }
  /**
   * Inspect the initial bytes of a CID to determine its properties.
   *
   * Involves decoding up to 4 varints. Typically this will require only 4 to 6
   * bytes but for larger multicodec code values and larger multihash digest
   * lengths these varints can be quite large. It is recommended that at least
   * 10 bytes be made available in the `initialBytes` argument for a complete
   * inspection.
   *
   * @template {unknown} T
   * @template {number} C
   * @template {number} A
   * @template {API.Version} V
   * @param {API.ByteView<API.Link<T, C, A, V>>} initialBytes
   * @returns {{ version:V, codec:C, multihashCode:A, digestSize:number, multihashSize:number, size:number }}
   */
  static inspectBytes(initialBytes) {
    let offset = 0;
    const next = () => {
      const [i, length3] = decode4(initialBytes.subarray(offset));
      offset += length3;
      return i;
    };
    let version = (
      /** @type {V} */
      next()
    );
    let codec = (
      /** @type {C} */
      DAG_PB_CODE
    );
    if (
      /** @type {number} */
      version === 18
    ) {
      version = /** @type {V} */
      0;
      offset = 0;
    } else {
      codec = /** @type {C} */
      next();
    }
    if (version !== 0 && version !== 1) {
      throw new RangeError(`Invalid CID version ${version}`);
    }
    const prefixSize = offset;
    const multihashCode = (
      /** @type {A} */
      next()
    );
    const digestSize = next();
    const size = offset + digestSize;
    const multihashSize = size - prefixSize;
    return { version, codec, multihashCode, digestSize, multihashSize, size };
  }
  /**
   * Takes cid in a string representation and creates an instance. If `base`
   * decoder is not provided will use a default from the configuration. It will
   * throw an error if encoding of the CID is not compatible with supplied (or
   * a default decoder).
   *
   * @template {string} Prefix
   * @template {unknown} Data
   * @template {number} Code
   * @template {number} Alg
   * @template {API.Version} Ver
   * @param {API.ToString<API.Link<Data, Code, Alg, Ver>, Prefix>} source
   * @param {API.MultibaseDecoder<Prefix>} [base]
   * @returns {CID<Data, Code, Alg, Ver>}
   */
  static parse(source, base4) {
    const [prefix, bytes] = parseCIDtoBytes(source, base4);
    const cid = _CID.decode(bytes);
    if (cid.version === 0 && source[0] !== "Q") {
      throw Error("Version 0 CID string must not include multibase prefix");
    }
    baseCache(cid).set(prefix, source);
    return cid;
  }
};
var parseCIDtoBytes = (source, base4) => {
  switch (source[0]) {
    case "Q": {
      const decoder = base4 || base58btc;
      return [
        /** @type {Prefix} */
        base58btc.prefix,
        decoder.decode(`${base58btc.prefix}${source}`)
      ];
    }
    case base58btc.prefix: {
      const decoder = base4 || base58btc;
      return [
        /** @type {Prefix} */
        base58btc.prefix,
        decoder.decode(source)
      ];
    }
    case base32.prefix: {
      const decoder = base4 || base32;
      return [
        /** @type {Prefix} */
        base32.prefix,
        decoder.decode(source)
      ];
    }
    default: {
      if (base4 == null) {
        throw Error(
          "To parse non base32 or base58btc encoded CID multibase decoder must be provided"
        );
      }
      return [
        /** @type {Prefix} */
        source[0],
        base4.decode(source)
      ];
    }
  }
};
var toStringV0 = (bytes, cache3, base4) => {
  const { prefix } = base4;
  if (prefix !== base58btc.prefix) {
    throw Error(`Cannot string encode V0 in ${base4.name} encoding`);
  }
  const cid = cache3.get(prefix);
  if (cid == null) {
    const cid2 = base4.encode(bytes).slice(1);
    cache3.set(prefix, cid2);
    return cid2;
  } else {
    return cid;
  }
};
var toStringV1 = (bytes, cache3, base4) => {
  const { prefix } = base4;
  const cid = cache3.get(prefix);
  if (cid == null) {
    const cid2 = base4.encode(bytes);
    cache3.set(prefix, cid2);
    return cid2;
  } else {
    return cid;
  }
};
var DAG_PB_CODE = 112;
var SHA_256_CODE = 18;
var encodeCID = (version, code3, multihash) => {
  const codeOffset = encodingLength(version);
  const hashOffset = codeOffset + encodingLength(code3);
  const bytes = new Uint8Array(hashOffset + multihash.byteLength);
  encodeTo(version, bytes, 0);
  encodeTo(code3, bytes, codeOffset);
  bytes.set(multihash, hashOffset);
  return bytes;
};
var cidSymbol = Symbol.for("@ipld/js-cid/CID");

// ../../node_modules/uint8arrays/node_modules/multiformats/src/basics.js
var bases = { ...identity_exports, ...base2_exports, ...base8_exports, ...base10_exports, ...base16_exports, ...base32_exports, ...base36_exports, ...base58_exports, ...base64_exports, ...base256emoji_exports };
var hashes = { ...sha2_browser_exports, ...identity_exports2 };

// ../../node_modules/uint8arrays/dist/src/alloc.js
function allocUnsafe(size = 0) {
  if (globalThis.Buffer?.allocUnsafe != null) {
    return asUint8Array(globalThis.Buffer.allocUnsafe(size));
  }
  return new Uint8Array(size);
}

// ../../node_modules/uint8arrays/dist/src/util/bases.js
function createCodec(name3, prefix, encode9, decode11) {
  return {
    name: name3,
    prefix,
    encoder: {
      name: name3,
      prefix,
      encode: encode9
    },
    decoder: {
      decode: decode11
    }
  };
}
var string = createCodec("utf8", "u", (buf) => {
  const decoder = new TextDecoder("utf8");
  return "u" + decoder.decode(buf);
}, (str) => {
  const encoder = new TextEncoder();
  return encoder.encode(str.substring(1));
});
var ascii = createCodec("ascii", "a", (buf) => {
  let string2 = "a";
  for (let i = 0; i < buf.length; i++) {
    string2 += String.fromCharCode(buf[i]);
  }
  return string2;
}, (str) => {
  str = str.substring(1);
  const buf = allocUnsafe(str.length);
  for (let i = 0; i < str.length; i++) {
    buf[i] = str.charCodeAt(i);
  }
  return buf;
});
var BASES = {
  utf8: string,
  "utf-8": string,
  hex: bases.base16,
  latin1: ascii,
  ascii,
  binary: ascii,
  ...bases
};
var bases_default = BASES;

// ../../node_modules/uint8arrays/dist/src/from-string.js
function fromString2(string2, encoding = "utf8") {
  const base4 = bases_default[encoding];
  if (base4 == null) {
    throw new Error(`Unsupported encoding "${encoding}"`);
  }
  if ((encoding === "utf8" || encoding === "utf-8") && globalThis.Buffer != null && globalThis.Buffer.from != null) {
    return asUint8Array(globalThis.Buffer.from(string2, "utf-8"));
  }
  return base4.decoder.decode(`${base4.prefix}${string2}`);
}

// ../crypto/dist/src/keys/ed25519-class.js
var ed25519_class_exports = {};
__export(ed25519_class_exports, {
  Ed25519PrivateKey: () => Ed25519PrivateKey,
  Ed25519PublicKey: () => Ed25519PublicKey,
  generateKeyPair: () => generateKeyPair,
  generateKeyPairFromSeed: () => generateKeyPairFromSeed,
  unmarshalEd25519PrivateKey: () => unmarshalEd25519PrivateKey,
  unmarshalEd25519PublicKey: () => unmarshalEd25519PublicKey
});

// ../../node_modules/multiformats/src/bases/base58.js
var base58_exports2 = {};
__export(base58_exports2, {
  base58btc: () => base58btc2,
  base58flickr: () => base58flickr2
});

// ../../node_modules/multiformats/vendor/base-x.js
function base3(ALPHABET, name3) {
  if (ALPHABET.length >= 255) {
    throw new TypeError("Alphabet too long");
  }
  var BASE_MAP = new Uint8Array(256);
  for (var j = 0; j < BASE_MAP.length; j++) {
    BASE_MAP[j] = 255;
  }
  for (var i = 0; i < ALPHABET.length; i++) {
    var x = ALPHABET.charAt(i);
    var xc = x.charCodeAt(0);
    if (BASE_MAP[xc] !== 255) {
      throw new TypeError(x + " is ambiguous");
    }
    BASE_MAP[xc] = i;
  }
  var BASE = ALPHABET.length;
  var LEADER = ALPHABET.charAt(0);
  var FACTOR = Math.log(BASE) / Math.log(256);
  var iFACTOR = Math.log(256) / Math.log(BASE);
  function encode9(source) {
    if (source instanceof Uint8Array)
      ;
    else if (ArrayBuffer.isView(source)) {
      source = new Uint8Array(source.buffer, source.byteOffset, source.byteLength);
    } else if (Array.isArray(source)) {
      source = Uint8Array.from(source);
    }
    if (!(source instanceof Uint8Array)) {
      throw new TypeError("Expected Uint8Array");
    }
    if (source.length === 0) {
      return "";
    }
    var zeroes = 0;
    var length3 = 0;
    var pbegin = 0;
    var pend = source.length;
    while (pbegin !== pend && source[pbegin] === 0) {
      pbegin++;
      zeroes++;
    }
    var size = (pend - pbegin) * iFACTOR + 1 >>> 0;
    var b58 = new Uint8Array(size);
    while (pbegin !== pend) {
      var carry = source[pbegin];
      var i2 = 0;
      for (var it1 = size - 1; (carry !== 0 || i2 < length3) && it1 !== -1; it1--, i2++) {
        carry += 256 * b58[it1] >>> 0;
        b58[it1] = carry % BASE >>> 0;
        carry = carry / BASE >>> 0;
      }
      if (carry !== 0) {
        throw new Error("Non-zero carry");
      }
      length3 = i2;
      pbegin++;
    }
    var it2 = size - length3;
    while (it2 !== size && b58[it2] === 0) {
      it2++;
    }
    var str = LEADER.repeat(zeroes);
    for (; it2 < size; ++it2) {
      str += ALPHABET.charAt(b58[it2]);
    }
    return str;
  }
  function decodeUnsafe(source) {
    if (typeof source !== "string") {
      throw new TypeError("Expected String");
    }
    if (source.length === 0) {
      return new Uint8Array();
    }
    var psz = 0;
    if (source[psz] === " ") {
      return;
    }
    var zeroes = 0;
    var length3 = 0;
    while (source[psz] === LEADER) {
      zeroes++;
      psz++;
    }
    var size = (source.length - psz) * FACTOR + 1 >>> 0;
    var b256 = new Uint8Array(size);
    while (source[psz]) {
      var carry = BASE_MAP[source.charCodeAt(psz)];
      if (carry === 255) {
        return;
      }
      var i2 = 0;
      for (var it3 = size - 1; (carry !== 0 || i2 < length3) && it3 !== -1; it3--, i2++) {
        carry += BASE * b256[it3] >>> 0;
        b256[it3] = carry % 256 >>> 0;
        carry = carry / 256 >>> 0;
      }
      if (carry !== 0) {
        throw new Error("Non-zero carry");
      }
      length3 = i2;
      psz++;
    }
    if (source[psz] === " ") {
      return;
    }
    var it4 = size - length3;
    while (it4 !== size && b256[it4] === 0) {
      it4++;
    }
    var vch = new Uint8Array(zeroes + (size - it4));
    var j2 = zeroes;
    while (it4 !== size) {
      vch[j2++] = b256[it4++];
    }
    return vch;
  }
  function decode11(string2) {
    var buffer = decodeUnsafe(string2);
    if (buffer) {
      return buffer;
    }
    throw new Error(`Non-${name3} character`);
  }
  return {
    encode: encode9,
    decodeUnsafe,
    decode: decode11
  };
}
var src2 = base3;
var _brrp__multiformats_scope_baseX2 = src2;
var base_x_default2 = _brrp__multiformats_scope_baseX2;

// ../../node_modules/multiformats/src/bytes.js
var empty2 = new Uint8Array(0);
var equals3 = (aa, bb) => {
  if (aa === bb)
    return true;
  if (aa.byteLength !== bb.byteLength) {
    return false;
  }
  for (let ii = 0; ii < aa.byteLength; ii++) {
    if (aa[ii] !== bb[ii]) {
      return false;
    }
  }
  return true;
};
var coerce2 = (o) => {
  if (o instanceof Uint8Array && o.constructor.name === "Uint8Array")
    return o;
  if (o instanceof ArrayBuffer)
    return new Uint8Array(o);
  if (ArrayBuffer.isView(o)) {
    return new Uint8Array(o.buffer, o.byteOffset, o.byteLength);
  }
  throw new Error("Unknown type, must be binary type");
};
var fromString3 = (str) => new TextEncoder().encode(str);
var toString2 = (b) => new TextDecoder().decode(b);

// ../../node_modules/multiformats/src/bases/base.js
var Encoder2 = class {
  /**
   * @param {Base} name
   * @param {Prefix} prefix
   * @param {(bytes:Uint8Array) => string} baseEncode
   */
  constructor(name3, prefix, baseEncode) {
    this.name = name3;
    this.prefix = prefix;
    this.baseEncode = baseEncode;
  }
  /**
   * @param {Uint8Array} bytes
   * @returns {API.Multibase<Prefix>}
   */
  encode(bytes) {
    if (bytes instanceof Uint8Array) {
      return `${this.prefix}${this.baseEncode(bytes)}`;
    } else {
      throw Error("Unknown type, must be binary type");
    }
  }
};
var Decoder2 = class {
  /**
   * @param {Base} name
   * @param {Prefix} prefix
   * @param {(text:string) => Uint8Array} baseDecode
   */
  constructor(name3, prefix, baseDecode) {
    this.name = name3;
    this.prefix = prefix;
    if (prefix.codePointAt(0) === void 0) {
      throw new Error("Invalid prefix character");
    }
    this.prefixCodePoint = /** @type {number} */
    prefix.codePointAt(0);
    this.baseDecode = baseDecode;
  }
  /**
   * @param {string} text
   */
  decode(text) {
    if (typeof text === "string") {
      if (text.codePointAt(0) !== this.prefixCodePoint) {
        throw Error(`Unable to decode multibase string ${JSON.stringify(text)}, ${this.name} decoder only supports inputs prefixed with ${this.prefix}`);
      }
      return this.baseDecode(text.slice(this.prefix.length));
    } else {
      throw Error("Can only multibase decode strings");
    }
  }
  /**
   * @template {string} OtherPrefix
   * @param {API.UnibaseDecoder<OtherPrefix>|ComposedDecoder<OtherPrefix>} decoder
   * @returns {ComposedDecoder<Prefix|OtherPrefix>}
   */
  or(decoder) {
    return or2(this, decoder);
  }
};
var ComposedDecoder2 = class {
  /**
   * @param {Decoders<Prefix>} decoders
   */
  constructor(decoders) {
    this.decoders = decoders;
  }
  /**
   * @template {string} OtherPrefix
   * @param {API.UnibaseDecoder<OtherPrefix>|ComposedDecoder<OtherPrefix>} decoder
   * @returns {ComposedDecoder<Prefix|OtherPrefix>}
   */
  or(decoder) {
    return or2(this, decoder);
  }
  /**
   * @param {string} input
   * @returns {Uint8Array}
   */
  decode(input) {
    const prefix = (
      /** @type {Prefix} */
      input[0]
    );
    const decoder = this.decoders[prefix];
    if (decoder) {
      return decoder.decode(input);
    } else {
      throw RangeError(`Unable to decode multibase string ${JSON.stringify(input)}, only inputs prefixed with ${Object.keys(this.decoders)} are supported`);
    }
  }
};
var or2 = (left, right) => new ComposedDecoder2(
  /** @type {Decoders<L|R>} */
  {
    ...left.decoders || { [
      /** @type API.UnibaseDecoder<L> */
      left.prefix
    ]: left },
    ...right.decoders || { [
      /** @type API.UnibaseDecoder<R> */
      right.prefix
    ]: right }
  }
);
var Codec2 = class {
  /**
   * @param {Base} name
   * @param {Prefix} prefix
   * @param {(bytes:Uint8Array) => string} baseEncode
   * @param {(text:string) => Uint8Array} baseDecode
   */
  constructor(name3, prefix, baseEncode, baseDecode) {
    this.name = name3;
    this.prefix = prefix;
    this.baseEncode = baseEncode;
    this.baseDecode = baseDecode;
    this.encoder = new Encoder2(name3, prefix, baseEncode);
    this.decoder = new Decoder2(name3, prefix, baseDecode);
  }
  /**
   * @param {Uint8Array} input
   */
  encode(input) {
    return this.encoder.encode(input);
  }
  /**
   * @param {string} input
   */
  decode(input) {
    return this.decoder.decode(input);
  }
};
var from3 = ({ name: name3, prefix, encode: encode9, decode: decode11 }) => new Codec2(name3, prefix, encode9, decode11);
var baseX2 = ({ prefix, name: name3, alphabet: alphabet3 }) => {
  const { encode: encode9, decode: decode11 } = base_x_default2(alphabet3, name3);
  return from3({
    prefix,
    name: name3,
    encode: encode9,
    /**
     * @param {string} text
     */
    decode: (text) => coerce2(decode11(text))
  });
};
var decode6 = (string2, alphabet3, bitsPerChar, name3) => {
  const codes = {};
  for (let i = 0; i < alphabet3.length; ++i) {
    codes[alphabet3[i]] = i;
  }
  let end = string2.length;
  while (string2[end - 1] === "=") {
    --end;
  }
  const out = new Uint8Array(end * bitsPerChar / 8 | 0);
  let bits2 = 0;
  let buffer = 0;
  let written = 0;
  for (let i = 0; i < end; ++i) {
    const value = codes[string2[i]];
    if (value === void 0) {
      throw new SyntaxError(`Non-${name3} character`);
    }
    buffer = buffer << bitsPerChar | value;
    bits2 += bitsPerChar;
    if (bits2 >= 8) {
      bits2 -= 8;
      out[written++] = 255 & buffer >> bits2;
    }
  }
  if (bits2 >= bitsPerChar || 255 & buffer << 8 - bits2) {
    throw new SyntaxError("Unexpected end of data");
  }
  return out;
};
var encode5 = (data, alphabet3, bitsPerChar) => {
  const pad = alphabet3[alphabet3.length - 1] === "=";
  const mask = (1 << bitsPerChar) - 1;
  let out = "";
  let bits2 = 0;
  let buffer = 0;
  for (let i = 0; i < data.length; ++i) {
    buffer = buffer << 8 | data[i];
    bits2 += 8;
    while (bits2 > bitsPerChar) {
      bits2 -= bitsPerChar;
      out += alphabet3[mask & buffer >> bits2];
    }
  }
  if (bits2) {
    out += alphabet3[mask & buffer << bitsPerChar - bits2];
  }
  if (pad) {
    while (out.length * bitsPerChar & 7) {
      out += "=";
    }
  }
  return out;
};
var rfc46482 = ({ name: name3, prefix, bitsPerChar, alphabet: alphabet3 }) => {
  return from3({
    prefix,
    name: name3,
    encode(input) {
      return encode5(input, alphabet3, bitsPerChar);
    },
    decode(input) {
      return decode6(input, alphabet3, bitsPerChar, name3);
    }
  });
};

// ../../node_modules/multiformats/src/bases/base58.js
var base58btc2 = baseX2({
  name: "base58btc",
  prefix: "z",
  alphabet: "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
});
var base58flickr2 = baseX2({
  name: "base58flickr",
  prefix: "Z",
  alphabet: "123456789abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ"
});

// ../../node_modules/multiformats/src/hashes/identity.js
var identity_exports3 = {};
__export(identity_exports3, {
  identity: () => identity3
});

// ../../node_modules/multiformats/vendor/varint.js
var encode_12 = encode6;
var MSB2 = 128;
var REST2 = 127;
var MSBALL2 = ~REST2;
var INT2 = Math.pow(2, 31);
function encode6(num, out, offset) {
  out = out || [];
  offset = offset || 0;
  var oldOffset = offset;
  while (num >= INT2) {
    out[offset++] = num & 255 | MSB2;
    num /= 128;
  }
  while (num & MSBALL2) {
    out[offset++] = num & 255 | MSB2;
    num >>>= 7;
  }
  out[offset] = num | 0;
  encode6.bytes = offset - oldOffset + 1;
  return out;
}
var decode7 = read2;
var MSB$12 = 128;
var REST$12 = 127;
function read2(buf, offset) {
  var res = 0, offset = offset || 0, shift = 0, counter = offset, b, l = buf.length;
  do {
    if (counter >= l) {
      read2.bytes = 0;
      throw new RangeError("Could not decode varint");
    }
    b = buf[counter++];
    res += shift < 28 ? (b & REST$12) << shift : (b & REST$12) * Math.pow(2, shift);
    shift += 7;
  } while (b >= MSB$12);
  read2.bytes = counter - offset;
  return res;
}
var N12 = Math.pow(2, 7);
var N22 = Math.pow(2, 14);
var N32 = Math.pow(2, 21);
var N42 = Math.pow(2, 28);
var N52 = Math.pow(2, 35);
var N62 = Math.pow(2, 42);
var N72 = Math.pow(2, 49);
var N82 = Math.pow(2, 56);
var N92 = Math.pow(2, 63);
var length2 = function(value) {
  return value < N12 ? 1 : value < N22 ? 2 : value < N32 ? 3 : value < N42 ? 4 : value < N52 ? 5 : value < N62 ? 6 : value < N72 ? 7 : value < N82 ? 8 : value < N92 ? 9 : 10;
};
var varint2 = {
  encode: encode_12,
  decode: decode7,
  encodingLength: length2
};
var _brrp_varint2 = varint2;
var varint_default2 = _brrp_varint2;

// ../../node_modules/multiformats/src/varint.js
var decode8 = (data, offset = 0) => {
  const code3 = varint_default2.decode(data, offset);
  return [code3, varint_default2.decode.bytes];
};
var encodeTo2 = (int, target, offset = 0) => {
  varint_default2.encode(int, target, offset);
  return target;
};
var encodingLength2 = (int) => {
  return varint_default2.encodingLength(int);
};

// ../../node_modules/multiformats/src/hashes/digest.js
var create2 = (code3, digest3) => {
  const size = digest3.byteLength;
  const sizeOffset = encodingLength2(code3);
  const digestOffset = sizeOffset + encodingLength2(size);
  const bytes = new Uint8Array(digestOffset + size);
  encodeTo2(code3, bytes, 0);
  encodeTo2(size, bytes, sizeOffset);
  bytes.set(digest3, digestOffset);
  return new Digest2(code3, size, digest3, bytes);
};
var decode9 = (multihash) => {
  const bytes = coerce2(multihash);
  const [code3, sizeOffset] = decode8(bytes);
  const [size, digestOffset] = decode8(bytes.subarray(sizeOffset));
  const digest3 = bytes.subarray(sizeOffset + digestOffset);
  if (digest3.byteLength !== size) {
    throw new Error("Incorrect length");
  }
  return new Digest2(code3, size, digest3, bytes);
};
var equals4 = (a, b) => {
  if (a === b) {
    return true;
  } else {
    const data = (
      /** @type {{code?:unknown, size?:unknown, bytes?:unknown}} */
      b
    );
    return a.code === data.code && a.size === data.size && data.bytes instanceof Uint8Array && equals3(a.bytes, data.bytes);
  }
};
var Digest2 = class {
  /**
   * Creates a multihash digest.
   *
   * @param {Code} code
   * @param {Size} size
   * @param {Uint8Array} digest
   * @param {Uint8Array} bytes
   */
  constructor(code3, size, digest3, bytes) {
    this.code = code3;
    this.size = size;
    this.digest = digest3;
    this.bytes = bytes;
  }
};

// ../../node_modules/multiformats/src/hashes/identity.js
var code2 = 0;
var name2 = "identity";
var encode7 = coerce2;
var digest2 = (input) => create2(code2, encode7(input));
var identity3 = { code: code2, name: name2, encode: encode7, digest: digest2 };

// ../../node_modules/multiformats/src/hashes/sha2-browser.js
var sha2_browser_exports2 = {};
__export(sha2_browser_exports2, {
  sha256: () => sha2562,
  sha512: () => sha5122
});

// ../../node_modules/multiformats/src/hashes/hasher.js
var from4 = ({ name: name3, code: code3, encode: encode9 }) => new Hasher2(name3, code3, encode9);
var Hasher2 = class {
  /**
   *
   * @param {Name} name
   * @param {Code} code
   * @param {(input: Uint8Array) => Await<Uint8Array>} encode
   */
  constructor(name3, code3, encode9) {
    this.name = name3;
    this.code = code3;
    this.encode = encode9;
  }
  /**
   * @param {Uint8Array} input
   * @returns {Await<Digest.Digest<Code, number>>}
   */
  digest(input) {
    if (input instanceof Uint8Array) {
      const result = this.encode(input);
      return result instanceof Uint8Array ? create2(this.code, result) : result.then((digest3) => create2(this.code, digest3));
    } else {
      throw Error("Unknown type, must be binary type");
    }
  }
};

// ../../node_modules/multiformats/src/hashes/sha2-browser.js
var sha2 = (name3) => (
  /**
   * @param {Uint8Array} data
   */
  async (data) => new Uint8Array(await crypto.subtle.digest(name3, data))
);
var sha2562 = from4({
  name: "sha2-256",
  code: 18,
  encode: sha2("SHA-256")
});
var sha5122 = from4({
  name: "sha2-512",
  code: 19,
  encode: sha2("SHA-512")
});

// ../../node_modules/uint8arrays/dist/src/equals.js
function equals5(a, b) {
  if (a === b) {
    return true;
  }
  if (a.byteLength !== b.byteLength) {
    return false;
  }
  for (let i = 0; i < a.byteLength; i++) {
    if (a[i] !== b[i]) {
      return false;
    }
  }
  return true;
}

// ../../node_modules/@noble/ed25519/lib/esm/index.js
var nodeCrypto = __toESM(require_crypto(), 1);
var _0n = BigInt(0);
var _1n = BigInt(1);
var _2n = BigInt(2);
var _8n = BigInt(8);
var CU_O = BigInt("7237005577332262213973186563042994240857116359379907606001950938285454250989");
var CURVE = Object.freeze({
  a: BigInt(-1),
  d: BigInt("37095705934669439343138083508754565189542113879843219016388785533085940283555"),
  P: BigInt("57896044618658097711785492504343953926634992332820282019728792003956564819949"),
  l: CU_O,
  n: CU_O,
  h: BigInt(8),
  Gx: BigInt("15112221349535400772501151409588531511454012693041857206046113283949847762202"),
  Gy: BigInt("46316835694926478169428394003475163141307993866256225615783033603165251855960")
});
var POW_2_256 = BigInt("0x10000000000000000000000000000000000000000000000000000000000000000");
var SQRT_M1 = BigInt("19681161376707505956807079304988542015446066515923890162744021073123829784752");
var SQRT_D = BigInt("6853475219497561581579357271197624642482790079785650197046958215289687604742");
var SQRT_AD_MINUS_ONE = BigInt("25063068953384623474111414158702152701244531502492656460079210482610430750235");
var INVSQRT_A_MINUS_D = BigInt("54469307008909316920995813868745141605393597292927456921205312896311721017578");
var ONE_MINUS_D_SQ = BigInt("1159843021668779879193775521855586647937357759715417654439879720876111806838");
var D_MINUS_ONE_SQ = BigInt("40440834346308536858101042469323190826248399146238708352240133220865137265952");
var ExtendedPoint = class _ExtendedPoint {
  constructor(x, y, z, t) {
    this.x = x;
    this.y = y;
    this.z = z;
    this.t = t;
  }
  static fromAffine(p) {
    if (!(p instanceof Point)) {
      throw new TypeError("ExtendedPoint#fromAffine: expected Point");
    }
    if (p.equals(Point.ZERO))
      return _ExtendedPoint.ZERO;
    return new _ExtendedPoint(p.x, p.y, _1n, mod2(p.x * p.y));
  }
  static toAffineBatch(points) {
    const toInv = invertBatch(points.map((p) => p.z));
    return points.map((p, i) => p.toAffine(toInv[i]));
  }
  static normalizeZ(points) {
    return this.toAffineBatch(points).map(this.fromAffine);
  }
  equals(other) {
    assertExtPoint(other);
    const { x: X1, y: Y1, z: Z1 } = this;
    const { x: X2, y: Y2, z: Z2 } = other;
    const X1Z2 = mod2(X1 * Z2);
    const X2Z1 = mod2(X2 * Z1);
    const Y1Z2 = mod2(Y1 * Z2);
    const Y2Z1 = mod2(Y2 * Z1);
    return X1Z2 === X2Z1 && Y1Z2 === Y2Z1;
  }
  negate() {
    return new _ExtendedPoint(mod2(-this.x), this.y, this.z, mod2(-this.t));
  }
  double() {
    const { x: X1, y: Y1, z: Z1 } = this;
    const { a } = CURVE;
    const A = mod2(X1 * X1);
    const B = mod2(Y1 * Y1);
    const C = mod2(_2n * mod2(Z1 * Z1));
    const D = mod2(a * A);
    const x1y1 = X1 + Y1;
    const E = mod2(mod2(x1y1 * x1y1) - A - B);
    const G = D + B;
    const F = G - C;
    const H = D - B;
    const X3 = mod2(E * F);
    const Y3 = mod2(G * H);
    const T3 = mod2(E * H);
    const Z3 = mod2(F * G);
    return new _ExtendedPoint(X3, Y3, Z3, T3);
  }
  add(other) {
    assertExtPoint(other);
    const { x: X1, y: Y1, z: Z1, t: T1 } = this;
    const { x: X2, y: Y2, z: Z2, t: T2 } = other;
    const A = mod2((Y1 - X1) * (Y2 + X2));
    const B = mod2((Y1 + X1) * (Y2 - X2));
    const F = mod2(B - A);
    if (F === _0n)
      return this.double();
    const C = mod2(Z1 * _2n * T2);
    const D = mod2(T1 * _2n * Z2);
    const E = D + C;
    const G = B + A;
    const H = D - C;
    const X3 = mod2(E * F);
    const Y3 = mod2(G * H);
    const T3 = mod2(E * H);
    const Z3 = mod2(F * G);
    return new _ExtendedPoint(X3, Y3, Z3, T3);
  }
  subtract(other) {
    return this.add(other.negate());
  }
  precomputeWindow(W) {
    const windows = 1 + 256 / W;
    const points = [];
    let p = this;
    let base4 = p;
    for (let window2 = 0; window2 < windows; window2++) {
      base4 = p;
      points.push(base4);
      for (let i = 1; i < 2 ** (W - 1); i++) {
        base4 = base4.add(p);
        points.push(base4);
      }
      p = base4.double();
    }
    return points;
  }
  wNAF(n, affinePoint) {
    if (!affinePoint && this.equals(_ExtendedPoint.BASE))
      affinePoint = Point.BASE;
    const W = affinePoint && affinePoint._WINDOW_SIZE || 1;
    if (256 % W) {
      throw new Error("Point#wNAF: Invalid precomputation window, must be power of 2");
    }
    let precomputes = affinePoint && pointPrecomputes.get(affinePoint);
    if (!precomputes) {
      precomputes = this.precomputeWindow(W);
      if (affinePoint && W !== 1) {
        precomputes = _ExtendedPoint.normalizeZ(precomputes);
        pointPrecomputes.set(affinePoint, precomputes);
      }
    }
    let p = _ExtendedPoint.ZERO;
    let f = _ExtendedPoint.BASE;
    const windows = 1 + 256 / W;
    const windowSize = 2 ** (W - 1);
    const mask = BigInt(2 ** W - 1);
    const maxNumber = 2 ** W;
    const shiftBy = BigInt(W);
    for (let window2 = 0; window2 < windows; window2++) {
      const offset = window2 * windowSize;
      let wbits = Number(n & mask);
      n >>= shiftBy;
      if (wbits > windowSize) {
        wbits -= maxNumber;
        n += _1n;
      }
      const offset1 = offset;
      const offset2 = offset + Math.abs(wbits) - 1;
      const cond1 = window2 % 2 !== 0;
      const cond2 = wbits < 0;
      if (wbits === 0) {
        f = f.add(constTimeNegate(cond1, precomputes[offset1]));
      } else {
        p = p.add(constTimeNegate(cond2, precomputes[offset2]));
      }
    }
    return _ExtendedPoint.normalizeZ([p, f])[0];
  }
  multiply(scalar, affinePoint) {
    return this.wNAF(normalizeScalar(scalar, CURVE.l), affinePoint);
  }
  multiplyUnsafe(scalar) {
    let n = normalizeScalar(scalar, CURVE.l, false);
    const G = _ExtendedPoint.BASE;
    const P0 = _ExtendedPoint.ZERO;
    if (n === _0n)
      return P0;
    if (this.equals(P0) || n === _1n)
      return this;
    if (this.equals(G))
      return this.wNAF(n);
    let p = P0;
    let d = this;
    while (n > _0n) {
      if (n & _1n)
        p = p.add(d);
      d = d.double();
      n >>= _1n;
    }
    return p;
  }
  isSmallOrder() {
    return this.multiplyUnsafe(CURVE.h).equals(_ExtendedPoint.ZERO);
  }
  isTorsionFree() {
    let p = this.multiplyUnsafe(CURVE.l / _2n).double();
    if (CURVE.l % _2n)
      p = p.add(this);
    return p.equals(_ExtendedPoint.ZERO);
  }
  toAffine(invZ) {
    const { x, y, z } = this;
    const is0 = this.equals(_ExtendedPoint.ZERO);
    if (invZ == null)
      invZ = is0 ? _8n : invert(z);
    const ax = mod2(x * invZ);
    const ay = mod2(y * invZ);
    const zz = mod2(z * invZ);
    if (is0)
      return Point.ZERO;
    if (zz !== _1n)
      throw new Error("invZ was invalid");
    return new Point(ax, ay);
  }
  fromRistrettoBytes() {
    legacyRist();
  }
  toRistrettoBytes() {
    legacyRist();
  }
  fromRistrettoHash() {
    legacyRist();
  }
};
ExtendedPoint.BASE = new ExtendedPoint(CURVE.Gx, CURVE.Gy, _1n, mod2(CURVE.Gx * CURVE.Gy));
ExtendedPoint.ZERO = new ExtendedPoint(_0n, _1n, _1n, _0n);
function constTimeNegate(condition, item) {
  const neg = item.negate();
  return condition ? neg : item;
}
function assertExtPoint(other) {
  if (!(other instanceof ExtendedPoint))
    throw new TypeError("ExtendedPoint expected");
}
function assertRstPoint(other) {
  if (!(other instanceof RistrettoPoint))
    throw new TypeError("RistrettoPoint expected");
}
function legacyRist() {
  throw new Error("Legacy method: switch to RistrettoPoint");
}
var RistrettoPoint = class _RistrettoPoint {
  constructor(ep) {
    this.ep = ep;
  }
  static calcElligatorRistrettoMap(r0) {
    const { d } = CURVE;
    const r = mod2(SQRT_M1 * r0 * r0);
    const Ns = mod2((r + _1n) * ONE_MINUS_D_SQ);
    let c = BigInt(-1);
    const D = mod2((c - d * r) * mod2(r + d));
    let { isValid: Ns_D_is_sq, value: s } = uvRatio(Ns, D);
    let s_ = mod2(s * r0);
    if (!edIsNegative(s_))
      s_ = mod2(-s_);
    if (!Ns_D_is_sq)
      s = s_;
    if (!Ns_D_is_sq)
      c = r;
    const Nt = mod2(c * (r - _1n) * D_MINUS_ONE_SQ - D);
    const s2 = s * s;
    const W0 = mod2((s + s) * D);
    const W1 = mod2(Nt * SQRT_AD_MINUS_ONE);
    const W2 = mod2(_1n - s2);
    const W3 = mod2(_1n + s2);
    return new ExtendedPoint(mod2(W0 * W3), mod2(W2 * W1), mod2(W1 * W3), mod2(W0 * W2));
  }
  static hashToCurve(hex) {
    hex = ensureBytes(hex, 64);
    const r1 = bytes255ToNumberLE(hex.slice(0, 32));
    const R1 = this.calcElligatorRistrettoMap(r1);
    const r2 = bytes255ToNumberLE(hex.slice(32, 64));
    const R2 = this.calcElligatorRistrettoMap(r2);
    return new _RistrettoPoint(R1.add(R2));
  }
  static fromHex(hex) {
    hex = ensureBytes(hex, 32);
    const { a, d } = CURVE;
    const emsg = "RistrettoPoint.fromHex: the hex is not valid encoding of RistrettoPoint";
    const s = bytes255ToNumberLE(hex);
    if (!equalBytes(numberTo32BytesLE(s), hex) || edIsNegative(s))
      throw new Error(emsg);
    const s2 = mod2(s * s);
    const u1 = mod2(_1n + a * s2);
    const u2 = mod2(_1n - a * s2);
    const u1_2 = mod2(u1 * u1);
    const u2_2 = mod2(u2 * u2);
    const v = mod2(a * d * u1_2 - u2_2);
    const { isValid, value: I } = invertSqrt(mod2(v * u2_2));
    const Dx = mod2(I * u2);
    const Dy = mod2(I * Dx * v);
    let x = mod2((s + s) * Dx);
    if (edIsNegative(x))
      x = mod2(-x);
    const y = mod2(u1 * Dy);
    const t = mod2(x * y);
    if (!isValid || edIsNegative(t) || y === _0n)
      throw new Error(emsg);
    return new _RistrettoPoint(new ExtendedPoint(x, y, _1n, t));
  }
  toRawBytes() {
    let { x, y, z, t } = this.ep;
    const u1 = mod2(mod2(z + y) * mod2(z - y));
    const u2 = mod2(x * y);
    const u2sq = mod2(u2 * u2);
    const { value: invsqrt } = invertSqrt(mod2(u1 * u2sq));
    const D1 = mod2(invsqrt * u1);
    const D2 = mod2(invsqrt * u2);
    const zInv = mod2(D1 * D2 * t);
    let D;
    if (edIsNegative(t * zInv)) {
      let _x = mod2(y * SQRT_M1);
      let _y = mod2(x * SQRT_M1);
      x = _x;
      y = _y;
      D = mod2(D1 * INVSQRT_A_MINUS_D);
    } else {
      D = D2;
    }
    if (edIsNegative(x * zInv))
      y = mod2(-y);
    let s = mod2((z - y) * D);
    if (edIsNegative(s))
      s = mod2(-s);
    return numberTo32BytesLE(s);
  }
  toHex() {
    return bytesToHex(this.toRawBytes());
  }
  toString() {
    return this.toHex();
  }
  equals(other) {
    assertRstPoint(other);
    const a = this.ep;
    const b = other.ep;
    const one = mod2(a.x * b.y) === mod2(a.y * b.x);
    const two = mod2(a.y * b.y) === mod2(a.x * b.x);
    return one || two;
  }
  add(other) {
    assertRstPoint(other);
    return new _RistrettoPoint(this.ep.add(other.ep));
  }
  subtract(other) {
    assertRstPoint(other);
    return new _RistrettoPoint(this.ep.subtract(other.ep));
  }
  multiply(scalar) {
    return new _RistrettoPoint(this.ep.multiply(scalar));
  }
  multiplyUnsafe(scalar) {
    return new _RistrettoPoint(this.ep.multiplyUnsafe(scalar));
  }
};
RistrettoPoint.BASE = new RistrettoPoint(ExtendedPoint.BASE);
RistrettoPoint.ZERO = new RistrettoPoint(ExtendedPoint.ZERO);
var pointPrecomputes = /* @__PURE__ */ new WeakMap();
var Point = class _Point {
  constructor(x, y) {
    this.x = x;
    this.y = y;
  }
  _setWindowSize(windowSize) {
    this._WINDOW_SIZE = windowSize;
    pointPrecomputes.delete(this);
  }
  static fromHex(hex, strict = true) {
    const { d, P } = CURVE;
    hex = ensureBytes(hex, 32);
    const normed = hex.slice();
    normed[31] = hex[31] & ~128;
    const y = bytesToNumberLE(normed);
    if (strict && y >= P)
      throw new Error("Expected 0 < hex < P");
    if (!strict && y >= POW_2_256)
      throw new Error("Expected 0 < hex < 2**256");
    const y2 = mod2(y * y);
    const u = mod2(y2 - _1n);
    const v = mod2(d * y2 + _1n);
    let { isValid, value: x } = uvRatio(u, v);
    if (!isValid)
      throw new Error("Point.fromHex: invalid y coordinate");
    const isXOdd = (x & _1n) === _1n;
    const isLastByteOdd = (hex[31] & 128) !== 0;
    if (isLastByteOdd !== isXOdd) {
      x = mod2(-x);
    }
    return new _Point(x, y);
  }
  static async fromPrivateKey(privateKey) {
    return (await getExtendedPublicKey(privateKey)).point;
  }
  toRawBytes() {
    const bytes = numberTo32BytesLE(this.y);
    bytes[31] |= this.x & _1n ? 128 : 0;
    return bytes;
  }
  toHex() {
    return bytesToHex(this.toRawBytes());
  }
  toX25519() {
    const { y } = this;
    const u = mod2((_1n + y) * invert(_1n - y));
    return numberTo32BytesLE(u);
  }
  isTorsionFree() {
    return ExtendedPoint.fromAffine(this).isTorsionFree();
  }
  equals(other) {
    return this.x === other.x && this.y === other.y;
  }
  negate() {
    return new _Point(mod2(-this.x), this.y);
  }
  add(other) {
    return ExtendedPoint.fromAffine(this).add(ExtendedPoint.fromAffine(other)).toAffine();
  }
  subtract(other) {
    return this.add(other.negate());
  }
  multiply(scalar) {
    return ExtendedPoint.fromAffine(this).multiply(scalar, this).toAffine();
  }
};
Point.BASE = new Point(CURVE.Gx, CURVE.Gy);
Point.ZERO = new Point(_0n, _1n);
var Signature = class _Signature {
  constructor(r, s) {
    this.r = r;
    this.s = s;
    this.assertValidity();
  }
  static fromHex(hex) {
    const bytes = ensureBytes(hex, 64);
    const r = Point.fromHex(bytes.slice(0, 32), false);
    const s = bytesToNumberLE(bytes.slice(32, 64));
    return new _Signature(r, s);
  }
  assertValidity() {
    const { r, s } = this;
    if (!(r instanceof Point))
      throw new Error("Expected Point instance");
    normalizeScalar(s, CURVE.l, false);
    return this;
  }
  toRawBytes() {
    const u8 = new Uint8Array(64);
    u8.set(this.r.toRawBytes());
    u8.set(numberTo32BytesLE(this.s), 32);
    return u8;
  }
  toHex() {
    return bytesToHex(this.toRawBytes());
  }
};
function concatBytes(...arrays) {
  if (!arrays.every((a) => a instanceof Uint8Array))
    throw new Error("Expected Uint8Array list");
  if (arrays.length === 1)
    return arrays[0];
  const length3 = arrays.reduce((a, arr) => a + arr.length, 0);
  const result = new Uint8Array(length3);
  for (let i = 0, pad = 0; i < arrays.length; i++) {
    const arr = arrays[i];
    result.set(arr, pad);
    pad += arr.length;
  }
  return result;
}
var hexes = Array.from({ length: 256 }, (v, i) => i.toString(16).padStart(2, "0"));
function bytesToHex(uint8a) {
  if (!(uint8a instanceof Uint8Array))
    throw new Error("Uint8Array expected");
  let hex = "";
  for (let i = 0; i < uint8a.length; i++) {
    hex += hexes[uint8a[i]];
  }
  return hex;
}
function hexToBytes(hex) {
  if (typeof hex !== "string") {
    throw new TypeError("hexToBytes: expected string, got " + typeof hex);
  }
  if (hex.length % 2)
    throw new Error("hexToBytes: received invalid unpadded hex");
  const array = new Uint8Array(hex.length / 2);
  for (let i = 0; i < array.length; i++) {
    const j = i * 2;
    const hexByte = hex.slice(j, j + 2);
    const byte = Number.parseInt(hexByte, 16);
    if (Number.isNaN(byte) || byte < 0)
      throw new Error("Invalid byte sequence");
    array[i] = byte;
  }
  return array;
}
function numberTo32BytesBE(num) {
  const length3 = 32;
  const hex = num.toString(16).padStart(length3 * 2, "0");
  return hexToBytes(hex);
}
function numberTo32BytesLE(num) {
  return numberTo32BytesBE(num).reverse();
}
function edIsNegative(num) {
  return (mod2(num) & _1n) === _1n;
}
function bytesToNumberLE(uint8a) {
  if (!(uint8a instanceof Uint8Array))
    throw new Error("Expected Uint8Array");
  return BigInt("0x" + bytesToHex(Uint8Array.from(uint8a).reverse()));
}
var MAX_255B = BigInt("0x7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff");
function bytes255ToNumberLE(bytes) {
  return mod2(bytesToNumberLE(bytes) & MAX_255B);
}
function mod2(a, b = CURVE.P) {
  const res = a % b;
  return res >= _0n ? res : b + res;
}
function invert(number, modulo = CURVE.P) {
  if (number === _0n || modulo <= _0n) {
    throw new Error(`invert: expected positive integers, got n=${number} mod=${modulo}`);
  }
  let a = mod2(number, modulo);
  let b = modulo;
  let x = _0n, y = _1n, u = _1n, v = _0n;
  while (a !== _0n) {
    const q = b / a;
    const r = b % a;
    const m = x - u * q;
    const n = y - v * q;
    b = a, a = r, x = u, y = v, u = m, v = n;
  }
  const gcd = b;
  if (gcd !== _1n)
    throw new Error("invert: does not exist");
  return mod2(x, modulo);
}
function invertBatch(nums, p = CURVE.P) {
  const tmp = new Array(nums.length);
  const lastMultiplied = nums.reduce((acc, num, i) => {
    if (num === _0n)
      return acc;
    tmp[i] = acc;
    return mod2(acc * num, p);
  }, _1n);
  const inverted = invert(lastMultiplied, p);
  nums.reduceRight((acc, num, i) => {
    if (num === _0n)
      return acc;
    tmp[i] = mod2(acc * tmp[i], p);
    return mod2(acc * num, p);
  }, inverted);
  return tmp;
}
function pow2(x, power) {
  const { P } = CURVE;
  let res = x;
  while (power-- > _0n) {
    res *= res;
    res %= P;
  }
  return res;
}
function pow_2_252_3(x) {
  const { P } = CURVE;
  const _5n = BigInt(5);
  const _10n = BigInt(10);
  const _20n = BigInt(20);
  const _40n = BigInt(40);
  const _80n = BigInt(80);
  const x2 = x * x % P;
  const b2 = x2 * x % P;
  const b4 = pow2(b2, _2n) * b2 % P;
  const b5 = pow2(b4, _1n) * x % P;
  const b10 = pow2(b5, _5n) * b5 % P;
  const b20 = pow2(b10, _10n) * b10 % P;
  const b40 = pow2(b20, _20n) * b20 % P;
  const b80 = pow2(b40, _40n) * b40 % P;
  const b160 = pow2(b80, _80n) * b80 % P;
  const b240 = pow2(b160, _80n) * b80 % P;
  const b250 = pow2(b240, _10n) * b10 % P;
  const pow_p_5_8 = pow2(b250, _2n) * x % P;
  return { pow_p_5_8, b2 };
}
function uvRatio(u, v) {
  const v3 = mod2(v * v * v);
  const v7 = mod2(v3 * v3 * v);
  const pow = pow_2_252_3(u * v7).pow_p_5_8;
  let x = mod2(u * v3 * pow);
  const vx2 = mod2(v * x * x);
  const root1 = x;
  const root2 = mod2(x * SQRT_M1);
  const useRoot1 = vx2 === u;
  const useRoot2 = vx2 === mod2(-u);
  const noRoot = vx2 === mod2(-u * SQRT_M1);
  if (useRoot1)
    x = root1;
  if (useRoot2 || noRoot)
    x = root2;
  if (edIsNegative(x))
    x = mod2(-x);
  return { isValid: useRoot1 || useRoot2, value: x };
}
function invertSqrt(number) {
  return uvRatio(_1n, number);
}
function modlLE(hash) {
  return mod2(bytesToNumberLE(hash), CURVE.l);
}
function equalBytes(b1, b2) {
  if (b1.length !== b2.length) {
    return false;
  }
  for (let i = 0; i < b1.length; i++) {
    if (b1[i] !== b2[i]) {
      return false;
    }
  }
  return true;
}
function ensureBytes(hex, expectedLength) {
  const bytes = hex instanceof Uint8Array ? Uint8Array.from(hex) : hexToBytes(hex);
  if (typeof expectedLength === "number" && bytes.length !== expectedLength)
    throw new Error(`Expected ${expectedLength} bytes`);
  return bytes;
}
function normalizeScalar(num, max, strict = true) {
  if (!max)
    throw new TypeError("Specify max value");
  if (typeof num === "number" && Number.isSafeInteger(num))
    num = BigInt(num);
  if (typeof num === "bigint" && num < max) {
    if (strict) {
      if (_0n < num)
        return num;
    } else {
      if (_0n <= num)
        return num;
    }
  }
  throw new TypeError("Expected valid scalar: 0 < scalar < max");
}
function adjustBytes25519(bytes) {
  bytes[0] &= 248;
  bytes[31] &= 127;
  bytes[31] |= 64;
  return bytes;
}
function checkPrivateKey(key) {
  key = typeof key === "bigint" || typeof key === "number" ? numberTo32BytesBE(normalizeScalar(key, POW_2_256)) : ensureBytes(key);
  if (key.length !== 32)
    throw new Error(`Expected 32 bytes`);
  return key;
}
function getKeyFromHash(hashed) {
  const head = adjustBytes25519(hashed.slice(0, 32));
  const prefix = hashed.slice(32, 64);
  const scalar = modlLE(head);
  const point = Point.BASE.multiply(scalar);
  const pointBytes = point.toRawBytes();
  return { head, prefix, scalar, point, pointBytes };
}
var _sha512Sync;
async function getExtendedPublicKey(key) {
  return getKeyFromHash(await utils.sha512(checkPrivateKey(key)));
}
async function getPublicKey(privateKey) {
  return (await getExtendedPublicKey(privateKey)).pointBytes;
}
async function sign(message2, privateKey) {
  message2 = ensureBytes(message2);
  const { prefix, scalar, pointBytes } = await getExtendedPublicKey(privateKey);
  const r = modlLE(await utils.sha512(prefix, message2));
  const R = Point.BASE.multiply(r);
  const k = modlLE(await utils.sha512(R.toRawBytes(), pointBytes, message2));
  const s = mod2(r + k * scalar, CURVE.l);
  return new Signature(R, s).toRawBytes();
}
function prepareVerification(sig, message2, publicKey) {
  message2 = ensureBytes(message2);
  if (!(publicKey instanceof Point))
    publicKey = Point.fromHex(publicKey, false);
  const { r, s } = sig instanceof Signature ? sig.assertValidity() : Signature.fromHex(sig);
  const SB = ExtendedPoint.BASE.multiplyUnsafe(s);
  return { r, s, SB, pub: publicKey, msg: message2 };
}
function finishVerification(publicKey, r, SB, hashed) {
  const k = modlLE(hashed);
  const kA = ExtendedPoint.fromAffine(publicKey).multiplyUnsafe(k);
  const RkA = ExtendedPoint.fromAffine(r).add(kA);
  return RkA.subtract(SB).multiplyUnsafe(CURVE.h).equals(ExtendedPoint.ZERO);
}
async function verify(sig, message2, publicKey) {
  const { r, SB, msg, pub } = prepareVerification(sig, message2, publicKey);
  const hashed = await utils.sha512(r.toRawBytes(), pub.toRawBytes(), msg);
  return finishVerification(pub, r, SB, hashed);
}
Point.BASE._setWindowSize(8);
var crypto2 = {
  node: nodeCrypto,
  web: typeof self === "object" && "crypto" in self ? self.crypto : void 0
};
var utils = {
  bytesToHex,
  hexToBytes,
  concatBytes,
  getExtendedPublicKey,
  mod: mod2,
  invert,
  TORSION_SUBGROUP: [
    "0100000000000000000000000000000000000000000000000000000000000000",
    "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac037a",
    "0000000000000000000000000000000000000000000000000000000000000080",
    "26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc05",
    "ecffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff7f",
    "26e8958fc2b227b045c3f489f2ef98f0d5dfac05d3c63339b13802886d53fc85",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "c7176a703d4dd84fba3c0b760d10670f2a2053fa2c39ccc64ec7fd7792ac03fa"
  ],
  hashToPrivateScalar: (hash) => {
    hash = ensureBytes(hash);
    if (hash.length < 40 || hash.length > 1024)
      throw new Error("Expected 40-1024 bytes of private key as per FIPS 186");
    return mod2(bytesToNumberLE(hash), CURVE.l - _1n) + _1n;
  },
  randomBytes: (bytesLength = 32) => {
    if (crypto2.web) {
      return crypto2.web.getRandomValues(new Uint8Array(bytesLength));
    } else if (crypto2.node) {
      const { randomBytes: randomBytes2 } = crypto2.node;
      return new Uint8Array(randomBytes2(bytesLength).buffer);
    } else {
      throw new Error("The environment doesn't have randomBytes function");
    }
  },
  randomPrivateKey: () => {
    return utils.randomBytes(32);
  },
  sha512: async (...messages) => {
    const message2 = concatBytes(...messages);
    if (crypto2.web) {
      const buffer = await crypto2.web.subtle.digest("SHA-512", message2.buffer);
      return new Uint8Array(buffer);
    } else if (crypto2.node) {
      return Uint8Array.from(crypto2.node.createHash("sha512").update(message2).digest());
    } else {
      throw new Error("The environment doesn't have sha512 function");
    }
  },
  precompute(windowSize = 8, point = Point.BASE) {
    const cached = point.equals(Point.BASE) ? point : new Point(point.x, point.y);
    cached._setWindowSize(windowSize);
    cached.multiply(_2n);
    return cached;
  },
  sha512Sync: void 0
};
Object.defineProperties(utils, {
  sha512Sync: {
    configurable: false,
    get() {
      return _sha512Sync;
    },
    set(val) {
      if (!_sha512Sync)
        _sha512Sync = val;
    }
  }
});

// ../crypto/dist/src/keys/ed25519-browser.js
var PUBLIC_KEY_BYTE_LENGTH = 32;
var PRIVATE_KEY_BYTE_LENGTH = 64;
var KEYS_BYTE_LENGTH = 32;
async function generateKey() {
  const privateKeyRaw = utils.randomPrivateKey();
  const publicKey = await getPublicKey(privateKeyRaw);
  const privateKey = concatKeys(privateKeyRaw, publicKey);
  return {
    privateKey,
    publicKey
  };
}
async function generateKeyFromSeed(seed) {
  if (seed.length !== KEYS_BYTE_LENGTH) {
    throw new TypeError('"seed" must be 32 bytes in length.');
  } else if (!(seed instanceof Uint8Array)) {
    throw new TypeError('"seed" must be a node.js Buffer, or Uint8Array.');
  }
  const privateKeyRaw = seed;
  const publicKey = await getPublicKey(privateKeyRaw);
  const privateKey = concatKeys(privateKeyRaw, publicKey);
  return {
    privateKey,
    publicKey
  };
}
async function hashAndSign(privateKey, msg) {
  const privateKeyRaw = privateKey.subarray(0, KEYS_BYTE_LENGTH);
  return sign(msg, privateKeyRaw);
}
async function hashAndVerify(publicKey, sig, msg) {
  return verify(sig, msg, publicKey);
}
function concatKeys(privateKeyRaw, publicKey) {
  const privateKey = new Uint8Array(PRIVATE_KEY_BYTE_LENGTH);
  for (let i = 0; i < KEYS_BYTE_LENGTH; i++) {
    privateKey[i] = privateKeyRaw[i];
    privateKey[KEYS_BYTE_LENGTH + i] = publicKey[i];
  }
  return privateKey;
}

// ../../node_modules/multiformats/src/bases/base64.js
var base64_exports2 = {};
__export(base64_exports2, {
  base64: () => base642,
  base64pad: () => base64pad2,
  base64url: () => base64url2,
  base64urlpad: () => base64urlpad2
});
var base642 = rfc46482({
  prefix: "m",
  name: "base64",
  alphabet: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/",
  bitsPerChar: 6
});
var base64pad2 = rfc46482({
  prefix: "M",
  name: "base64pad",
  alphabet: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=",
  bitsPerChar: 6
});
var base64url2 = rfc46482({
  prefix: "u",
  name: "base64url",
  alphabet: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_",
  bitsPerChar: 6
});
var base64urlpad2 = rfc46482({
  prefix: "U",
  name: "base64urlpad",
  alphabet: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_=",
  bitsPerChar: 6
});

// ../../node_modules/uint8arrays/dist/src/concat.js
function concat(arrays, length3) {
  if (length3 == null) {
    length3 = arrays.reduce((acc, curr) => acc + curr.length, 0);
  }
  const output = allocUnsafe(length3);
  let offset = 0;
  for (const arr of arrays) {
    output.set(arr, offset);
    offset += arr.length;
  }
  return asUint8Array(output);
}

// ../crypto/dist/src/webcrypto.js
var webcrypto_default = {
  get(win = globalThis) {
    const nativeCrypto = win.crypto;
    if (nativeCrypto == null || nativeCrypto.subtle == null) {
      throw Object.assign(new Error("Missing Web Crypto API. The most likely cause of this error is that this page is being accessed from an insecure context (i.e. not HTTPS). For more information and possible resolutions see https://github.com/libp2p/js-libp2p-crypto/blob/master/README.md#web-crypto-api"), { code: "ERR_MISSING_WEB_CRYPTO" });
    }
    return nativeCrypto;
  }
};

// ../crypto/dist/src/ciphers/aes-gcm.browser.js
var derivedEmptyPasswordKey = { alg: "A128GCM", ext: true, k: "scm9jmO_4BJAgdwWGVulLg", key_ops: ["encrypt", "decrypt"], kty: "oct" };
function create3(opts) {
  const algorithm = opts?.algorithm ?? "AES-GCM";
  let keyLength = opts?.keyLength ?? 16;
  const nonceLength = opts?.nonceLength ?? 12;
  const digest3 = opts?.digest ?? "SHA-256";
  const saltLength = opts?.saltLength ?? 16;
  const iterations = opts?.iterations ?? 32767;
  const crypto4 = webcrypto_default.get();
  keyLength *= 8;
  async function encrypt2(data, password) {
    const salt = crypto4.getRandomValues(new Uint8Array(saltLength));
    const nonce = crypto4.getRandomValues(new Uint8Array(nonceLength));
    const aesGcm = { name: algorithm, iv: nonce };
    if (typeof password === "string") {
      password = fromString2(password);
    }
    let cryptoKey;
    if (password.length === 0) {
      cryptoKey = await crypto4.subtle.importKey("jwk", derivedEmptyPasswordKey, { name: "AES-GCM" }, true, ["encrypt"]);
      try {
        const deriveParams = { name: "PBKDF2", salt, iterations, hash: { name: digest3 } };
        const runtimeDerivedEmptyPassword = await crypto4.subtle.importKey("raw", password, { name: "PBKDF2" }, false, ["deriveKey"]);
        cryptoKey = await crypto4.subtle.deriveKey(deriveParams, runtimeDerivedEmptyPassword, { name: algorithm, length: keyLength }, true, ["encrypt"]);
      } catch {
        cryptoKey = await crypto4.subtle.importKey("jwk", derivedEmptyPasswordKey, { name: "AES-GCM" }, true, ["encrypt"]);
      }
    } else {
      const deriveParams = { name: "PBKDF2", salt, iterations, hash: { name: digest3 } };
      const rawKey = await crypto4.subtle.importKey("raw", password, { name: "PBKDF2" }, false, ["deriveKey"]);
      cryptoKey = await crypto4.subtle.deriveKey(deriveParams, rawKey, { name: algorithm, length: keyLength }, true, ["encrypt"]);
    }
    const ciphertext = await crypto4.subtle.encrypt(aesGcm, cryptoKey, data);
    return concat([salt, aesGcm.iv, new Uint8Array(ciphertext)]);
  }
  async function decrypt2(data, password) {
    const salt = data.subarray(0, saltLength);
    const nonce = data.subarray(saltLength, saltLength + nonceLength);
    const ciphertext = data.subarray(saltLength + nonceLength);
    const aesGcm = { name: algorithm, iv: nonce };
    if (typeof password === "string") {
      password = fromString2(password);
    }
    let cryptoKey;
    if (password.length === 0) {
      try {
        const deriveParams = { name: "PBKDF2", salt, iterations, hash: { name: digest3 } };
        const runtimeDerivedEmptyPassword = await crypto4.subtle.importKey("raw", password, { name: "PBKDF2" }, false, ["deriveKey"]);
        cryptoKey = await crypto4.subtle.deriveKey(deriveParams, runtimeDerivedEmptyPassword, { name: algorithm, length: keyLength }, true, ["decrypt"]);
      } catch {
        cryptoKey = await crypto4.subtle.importKey("jwk", derivedEmptyPasswordKey, { name: "AES-GCM" }, true, ["decrypt"]);
      }
    } else {
      const deriveParams = { name: "PBKDF2", salt, iterations, hash: { name: digest3 } };
      const rawKey = await crypto4.subtle.importKey("raw", password, { name: "PBKDF2" }, false, ["deriveKey"]);
      cryptoKey = await crypto4.subtle.deriveKey(deriveParams, rawKey, { name: algorithm, length: keyLength }, true, ["decrypt"]);
    }
    const plaintext = await crypto4.subtle.decrypt(aesGcm, cryptoKey, ciphertext);
    return new Uint8Array(plaintext);
  }
  const cipher = {
    encrypt: encrypt2,
    decrypt: decrypt2
  };
  return cipher;
}

// ../crypto/dist/src/keys/exporter.js
async function exporter(privateKey, password) {
  const cipher = create3();
  const encryptedKey = await cipher.encrypt(privateKey, password);
  return base642.encode(encryptedKey);
}

// ../../node_modules/protons-runtime/dist/src/utils.js
var import_reader = __toESM(require_reader(), 1);
var import_reader_buffer = __toESM(require_reader_buffer(), 1);
var import_minimal = __toESM(require_minimal(), 1);
var import_writer = __toESM(require_writer(), 1);
var import_writer_buffer = __toESM(require_writer_buffer(), 1);
function configure() {
  import_minimal.default._configure();
  import_reader.default._configure(import_reader_buffer.default);
  import_writer.default._configure(import_writer_buffer.default);
}
configure();
var methods = [
  "uint64",
  "int64",
  "sint64",
  "fixed64",
  "sfixed64"
];
function patchReader(obj) {
  for (const method of methods) {
    if (obj[method] == null) {
      continue;
    }
    const original = obj[method];
    obj[method] = function() {
      return BigInt(original.call(this).toString());
    };
  }
  return obj;
}
function reader(buf) {
  return patchReader(new import_reader.default(buf));
}
function patchWriter(obj) {
  for (const method of methods) {
    if (obj[method] == null) {
      continue;
    }
    const original = obj[method];
    obj[method] = function(val) {
      return original.call(this, val.toString());
    };
  }
  return obj;
}
function writer() {
  return patchWriter(import_writer.default.create());
}

// ../../node_modules/protons-runtime/dist/src/decode.js
function decodeMessage(buf, codec) {
  const r = reader(buf instanceof Uint8Array ? buf : buf.subarray());
  return codec.decode(r);
}

// ../../node_modules/protons-runtime/dist/src/encode.js
function encodeMessage(message2, codec) {
  const w = writer();
  codec.encode(message2, w, {
    lengthDelimited: false
  });
  return w.finish();
}

// ../../node_modules/protons-runtime/dist/src/codec.js
var CODEC_TYPES;
(function(CODEC_TYPES2) {
  CODEC_TYPES2[CODEC_TYPES2["VARINT"] = 0] = "VARINT";
  CODEC_TYPES2[CODEC_TYPES2["BIT64"] = 1] = "BIT64";
  CODEC_TYPES2[CODEC_TYPES2["LENGTH_DELIMITED"] = 2] = "LENGTH_DELIMITED";
  CODEC_TYPES2[CODEC_TYPES2["START_GROUP"] = 3] = "START_GROUP";
  CODEC_TYPES2[CODEC_TYPES2["END_GROUP"] = 4] = "END_GROUP";
  CODEC_TYPES2[CODEC_TYPES2["BIT32"] = 5] = "BIT32";
})(CODEC_TYPES || (CODEC_TYPES = {}));
function createCodec2(name3, type, encode9, decode11) {
  return {
    name: name3,
    type,
    encode: encode9,
    decode: decode11
  };
}

// ../../node_modules/protons-runtime/dist/src/codecs/enum.js
function enumeration(v) {
  function findValue(val) {
    if (v[val.toString()] == null) {
      throw new Error("Invalid enum value");
    }
    return v[val];
  }
  const encode9 = function enumEncode(val, writer2) {
    const enumValue = findValue(val);
    writer2.int32(enumValue);
  };
  const decode11 = function enumDecode(reader2) {
    const val = reader2.int32();
    return findValue(val);
  };
  return createCodec2("enum", CODEC_TYPES.VARINT, encode9, decode11);
}

// ../../node_modules/protons-runtime/dist/src/codecs/message.js
function message(encode9, decode11) {
  return createCodec2("message", CODEC_TYPES.LENGTH_DELIMITED, encode9, decode11);
}

// ../crypto/dist/src/keys/keys.js
var KeyType;
(function(KeyType2) {
  KeyType2["RSA"] = "RSA";
  KeyType2["Ed25519"] = "Ed25519";
  KeyType2["Secp256k1"] = "Secp256k1";
})(KeyType || (KeyType = {}));
var __KeyTypeValues;
(function(__KeyTypeValues2) {
  __KeyTypeValues2[__KeyTypeValues2["RSA"] = 0] = "RSA";
  __KeyTypeValues2[__KeyTypeValues2["Ed25519"] = 1] = "Ed25519";
  __KeyTypeValues2[__KeyTypeValues2["Secp256k1"] = 2] = "Secp256k1";
})(__KeyTypeValues || (__KeyTypeValues = {}));
(function(KeyType2) {
  KeyType2.codec = () => {
    return enumeration(__KeyTypeValues);
  };
})(KeyType || (KeyType = {}));
var PublicKey;
(function(PublicKey2) {
  let _codec;
  PublicKey2.codec = () => {
    if (_codec == null) {
      _codec = message((obj, w, opts = {}) => {
        if (opts.lengthDelimited !== false) {
          w.fork();
        }
        if (obj.Type != null) {
          w.uint32(8);
          KeyType.codec().encode(obj.Type, w);
        }
        if (obj.Data != null) {
          w.uint32(18);
          w.bytes(obj.Data);
        }
        if (opts.lengthDelimited !== false) {
          w.ldelim();
        }
      }, (reader2, length3) => {
        const obj = {};
        const end = length3 == null ? reader2.len : reader2.pos + length3;
        while (reader2.pos < end) {
          const tag = reader2.uint32();
          switch (tag >>> 3) {
            case 1:
              obj.Type = KeyType.codec().decode(reader2);
              break;
            case 2:
              obj.Data = reader2.bytes();
              break;
            default:
              reader2.skipType(tag & 7);
              break;
          }
        }
        return obj;
      });
    }
    return _codec;
  };
  PublicKey2.encode = (obj) => {
    return encodeMessage(obj, PublicKey2.codec());
  };
  PublicKey2.decode = (buf) => {
    return decodeMessage(buf, PublicKey2.codec());
  };
})(PublicKey || (PublicKey = {}));
var PrivateKey;
(function(PrivateKey2) {
  let _codec;
  PrivateKey2.codec = () => {
    if (_codec == null) {
      _codec = message((obj, w, opts = {}) => {
        if (opts.lengthDelimited !== false) {
          w.fork();
        }
        if (obj.Type != null) {
          w.uint32(8);
          KeyType.codec().encode(obj.Type, w);
        }
        if (obj.Data != null) {
          w.uint32(18);
          w.bytes(obj.Data);
        }
        if (opts.lengthDelimited !== false) {
          w.ldelim();
        }
      }, (reader2, length3) => {
        const obj = {};
        const end = length3 == null ? reader2.len : reader2.pos + length3;
        while (reader2.pos < end) {
          const tag = reader2.uint32();
          switch (tag >>> 3) {
            case 1:
              obj.Type = KeyType.codec().decode(reader2);
              break;
            case 2:
              obj.Data = reader2.bytes();
              break;
            default:
              reader2.skipType(tag & 7);
              break;
          }
        }
        return obj;
      });
    }
    return _codec;
  };
  PrivateKey2.encode = (obj) => {
    return encodeMessage(obj, PrivateKey2.codec());
  };
  PrivateKey2.decode = (buf) => {
    return decodeMessage(buf, PrivateKey2.codec());
  };
})(PrivateKey || (PrivateKey = {}));

// ../crypto/dist/src/keys/ed25519-class.js
var Ed25519PublicKey = class {
  _key;
  constructor(key) {
    this._key = ensureKey(key, PUBLIC_KEY_BYTE_LENGTH);
  }
  async verify(data, sig) {
    return hashAndVerify(this._key, sig, data);
  }
  marshal() {
    return this._key;
  }
  get bytes() {
    return PublicKey.encode({
      Type: KeyType.Ed25519,
      Data: this.marshal()
    }).subarray();
  }
  equals(key) {
    return equals5(this.bytes, key.bytes);
  }
  async hash() {
    const { bytes } = await sha2562.digest(this.bytes);
    return bytes;
  }
};
var Ed25519PrivateKey = class {
  _key;
  _publicKey;
  // key       - 64 byte Uint8Array containing private key
  // publicKey - 32 byte Uint8Array containing public key
  constructor(key, publicKey) {
    this._key = ensureKey(key, PRIVATE_KEY_BYTE_LENGTH);
    this._publicKey = ensureKey(publicKey, PUBLIC_KEY_BYTE_LENGTH);
  }
  async sign(message2) {
    return hashAndSign(this._key, message2);
  }
  get public() {
    return new Ed25519PublicKey(this._publicKey);
  }
  marshal() {
    return this._key;
  }
  get bytes() {
    return PrivateKey.encode({
      Type: KeyType.Ed25519,
      Data: this.marshal()
    }).subarray();
  }
  equals(key) {
    return equals5(this.bytes, key.bytes);
  }
  async hash() {
    const { bytes } = await sha2562.digest(this.bytes);
    return bytes;
  }
  /**
   * Gets the ID of the key.
   *
   * The key id is the base58 encoding of the identity multihash containing its public key.
   * The public key is a protobuf encoding containing a type and the DER encoding
   * of the PKCS SubjectPublicKeyInfo.
   *
   * @returns {Promise<string>}
   */
  async id() {
    const encoding = identity3.digest(this.public.bytes);
    return base58btc2.encode(encoding.bytes).substring(1);
  }
  /**
   * Exports the key into a password protected `format`
   */
  async export(password, format3 = "libp2p-key") {
    if (format3 === "libp2p-key") {
      return exporter(this.bytes, password);
    } else {
      throw new CodeError(`export format '${format3}' is not supported`, "ERR_INVALID_EXPORT_FORMAT");
    }
  }
};
function unmarshalEd25519PrivateKey(bytes) {
  if (bytes.length > PRIVATE_KEY_BYTE_LENGTH) {
    bytes = ensureKey(bytes, PRIVATE_KEY_BYTE_LENGTH + PUBLIC_KEY_BYTE_LENGTH);
    const privateKeyBytes2 = bytes.subarray(0, PRIVATE_KEY_BYTE_LENGTH);
    const publicKeyBytes2 = bytes.subarray(PRIVATE_KEY_BYTE_LENGTH, bytes.length);
    return new Ed25519PrivateKey(privateKeyBytes2, publicKeyBytes2);
  }
  bytes = ensureKey(bytes, PRIVATE_KEY_BYTE_LENGTH);
  const privateKeyBytes = bytes.subarray(0, PRIVATE_KEY_BYTE_LENGTH);
  const publicKeyBytes = bytes.subarray(PUBLIC_KEY_BYTE_LENGTH);
  return new Ed25519PrivateKey(privateKeyBytes, publicKeyBytes);
}
function unmarshalEd25519PublicKey(bytes) {
  bytes = ensureKey(bytes, PUBLIC_KEY_BYTE_LENGTH);
  return new Ed25519PublicKey(bytes);
}
async function generateKeyPair() {
  const { privateKey, publicKey } = await generateKey();
  return new Ed25519PrivateKey(privateKey, publicKey);
}
async function generateKeyPairFromSeed(seed) {
  const { privateKey, publicKey } = await generateKeyFromSeed(seed);
  return new Ed25519PrivateKey(privateKey, publicKey);
}
function ensureKey(key, length3) {
  key = Uint8Array.from(key ?? []);
  if (key.length !== length3) {
    throw new CodeError(`Key must be a Uint8Array of length ${length3}, got ${key.length}`, "ERR_INVALID_KEY_TYPE");
  }
  return key;
}

// ../../node_modules/uint8arrays/dist/src/to-string.js
function toString3(array, encoding = "utf8") {
  const base4 = bases_default[encoding];
  if (base4 == null) {
    throw new Error(`Unsupported encoding "${encoding}"`);
  }
  if ((encoding === "utf8" || encoding === "utf-8") && globalThis.Buffer != null && globalThis.Buffer.from != null) {
    return globalThis.Buffer.from(array.buffer, array.byteOffset, array.byteLength).toString("utf8");
  }
  return base4.encoder.encode(array).substring(1);
}

// ../crypto/dist/src/util.js
var import_util = __toESM(require_util(), 1);
var import_jsbn = __toESM(require_jsbn(), 1);
var import_forge = __toESM(require_forge(), 1);
function bigIntegerToUintBase64url(num, len) {
  let buf = Uint8Array.from(num.abs().toByteArray());
  buf = buf[0] === 0 ? buf.subarray(1) : buf;
  if (len != null) {
    if (buf.length > len)
      throw new Error("byte array longer than desired length");
    buf = concat([new Uint8Array(len - buf.length), buf]);
  }
  return toString3(buf, "base64url");
}
function base64urlToBigInteger(str) {
  const buf = base64urlToBuffer(str);
  return new import_forge.default.jsbn.BigInteger(toString3(buf, "base16"), 16);
}
function base64urlToBuffer(str, len) {
  let buf = fromString2(str, "base64urlpad");
  if (len != null) {
    if (buf.length > len)
      throw new Error("byte array longer than desired length");
    buf = concat([new Uint8Array(len - buf.length), buf]);
  }
  return buf;
}

// ../crypto/dist/src/keys/ecdh-browser.js
var bits = {
  "P-256": 256,
  "P-384": 384,
  "P-521": 521
};
var curveTypes = Object.keys(bits);
var names = curveTypes.join(" / ");

// ../crypto/dist/src/keys/rsa-class.js
var rsa_class_exports = {};
__export(rsa_class_exports, {
  RsaPrivateKey: () => RsaPrivateKey,
  RsaPublicKey: () => RsaPublicKey,
  fromJwk: () => fromJwk,
  generateKeyPair: () => generateKeyPair2,
  unmarshalRsaPrivateKey: () => unmarshalRsaPrivateKey,
  unmarshalRsaPublicKey: () => unmarshalRsaPublicKey
});
var import_forge4 = __toESM(require_forge(), 1);
var import_sha512 = __toESM(require_sha512(), 1);

// ../../node_modules/@noble/secp256k1/lib/esm/index.js
var nodeCrypto2 = __toESM(require_crypto(), 1);
var _0n2 = BigInt(0);
var _1n2 = BigInt(1);
var _2n2 = BigInt(2);
var _3n = BigInt(3);
var _8n2 = BigInt(8);
var CURVE2 = Object.freeze({
  a: _0n2,
  b: BigInt(7),
  P: BigInt("0xfffffffffffffffffffffffffffffffffffffffffffffffffffffffefffffc2f"),
  n: BigInt("0xfffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141"),
  h: _1n2,
  Gx: BigInt("55066263022277343669578718895168534326250603453777594175500187360389116729240"),
  Gy: BigInt("32670510020758816978083085130507043184471273380659243275938904335757337482424"),
  beta: BigInt("0x7ae96a2b657c07106e64479eac3434e99cf0497512f58995c1396c28719501ee")
});
var divNearest = (a, b) => (a + b / _2n2) / b;
var endo = {
  beta: BigInt("0x7ae96a2b657c07106e64479eac3434e99cf0497512f58995c1396c28719501ee"),
  splitScalar(k) {
    const { n } = CURVE2;
    const a1 = BigInt("0x3086d221a7d46bcde86c90e49284eb15");
    const b1 = -_1n2 * BigInt("0xe4437ed6010e88286f547fa90abfe4c3");
    const a2 = BigInt("0x114ca50f7a8e2f3f657c1108d9d44cfd8");
    const b2 = a1;
    const POW_2_128 = BigInt("0x100000000000000000000000000000000");
    const c1 = divNearest(b2 * k, n);
    const c2 = divNearest(-b1 * k, n);
    let k1 = mod3(k - c1 * a1 - c2 * a2, n);
    let k2 = mod3(-c1 * b1 - c2 * b2, n);
    const k1neg = k1 > POW_2_128;
    const k2neg = k2 > POW_2_128;
    if (k1neg)
      k1 = n - k1;
    if (k2neg)
      k2 = n - k2;
    if (k1 > POW_2_128 || k2 > POW_2_128) {
      throw new Error("splitScalarEndo: Endomorphism failed, k=" + k);
    }
    return { k1neg, k1, k2neg, k2 };
  }
};
var fieldLen = 32;
var groupLen = 32;
var hashLen = 32;
var compressedLen = fieldLen + 1;
var uncompressedLen = 2 * fieldLen + 1;
function weierstrass(x) {
  const { a, b } = CURVE2;
  const x2 = mod3(x * x);
  const x3 = mod3(x2 * x);
  return mod3(x3 + a * x + b);
}
var USE_ENDOMORPHISM = CURVE2.a === _0n2;
var ShaError = class extends Error {
  constructor(message2) {
    super(message2);
  }
};
function assertJacPoint(other) {
  if (!(other instanceof JacobianPoint))
    throw new TypeError("JacobianPoint expected");
}
var JacobianPoint = class _JacobianPoint {
  constructor(x, y, z) {
    this.x = x;
    this.y = y;
    this.z = z;
  }
  static fromAffine(p) {
    if (!(p instanceof Point2)) {
      throw new TypeError("JacobianPoint#fromAffine: expected Point");
    }
    if (p.equals(Point2.ZERO))
      return _JacobianPoint.ZERO;
    return new _JacobianPoint(p.x, p.y, _1n2);
  }
  static toAffineBatch(points) {
    const toInv = invertBatch2(points.map((p) => p.z));
    return points.map((p, i) => p.toAffine(toInv[i]));
  }
  static normalizeZ(points) {
    return _JacobianPoint.toAffineBatch(points).map(_JacobianPoint.fromAffine);
  }
  equals(other) {
    assertJacPoint(other);
    const { x: X1, y: Y1, z: Z1 } = this;
    const { x: X2, y: Y2, z: Z2 } = other;
    const Z1Z1 = mod3(Z1 * Z1);
    const Z2Z2 = mod3(Z2 * Z2);
    const U1 = mod3(X1 * Z2Z2);
    const U2 = mod3(X2 * Z1Z1);
    const S1 = mod3(mod3(Y1 * Z2) * Z2Z2);
    const S2 = mod3(mod3(Y2 * Z1) * Z1Z1);
    return U1 === U2 && S1 === S2;
  }
  negate() {
    return new _JacobianPoint(this.x, mod3(-this.y), this.z);
  }
  double() {
    const { x: X1, y: Y1, z: Z1 } = this;
    const A = mod3(X1 * X1);
    const B = mod3(Y1 * Y1);
    const C = mod3(B * B);
    const x1b = X1 + B;
    const D = mod3(_2n2 * (mod3(x1b * x1b) - A - C));
    const E = mod3(_3n * A);
    const F = mod3(E * E);
    const X3 = mod3(F - _2n2 * D);
    const Y3 = mod3(E * (D - X3) - _8n2 * C);
    const Z3 = mod3(_2n2 * Y1 * Z1);
    return new _JacobianPoint(X3, Y3, Z3);
  }
  add(other) {
    assertJacPoint(other);
    const { x: X1, y: Y1, z: Z1 } = this;
    const { x: X2, y: Y2, z: Z2 } = other;
    if (X2 === _0n2 || Y2 === _0n2)
      return this;
    if (X1 === _0n2 || Y1 === _0n2)
      return other;
    const Z1Z1 = mod3(Z1 * Z1);
    const Z2Z2 = mod3(Z2 * Z2);
    const U1 = mod3(X1 * Z2Z2);
    const U2 = mod3(X2 * Z1Z1);
    const S1 = mod3(mod3(Y1 * Z2) * Z2Z2);
    const S2 = mod3(mod3(Y2 * Z1) * Z1Z1);
    const H = mod3(U2 - U1);
    const r = mod3(S2 - S1);
    if (H === _0n2) {
      if (r === _0n2) {
        return this.double();
      } else {
        return _JacobianPoint.ZERO;
      }
    }
    const HH = mod3(H * H);
    const HHH = mod3(H * HH);
    const V = mod3(U1 * HH);
    const X3 = mod3(r * r - HHH - _2n2 * V);
    const Y3 = mod3(r * (V - X3) - S1 * HHH);
    const Z3 = mod3(Z1 * Z2 * H);
    return new _JacobianPoint(X3, Y3, Z3);
  }
  subtract(other) {
    return this.add(other.negate());
  }
  multiplyUnsafe(scalar) {
    const P0 = _JacobianPoint.ZERO;
    if (typeof scalar === "bigint" && scalar === _0n2)
      return P0;
    let n = normalizeScalar2(scalar);
    if (n === _1n2)
      return this;
    if (!USE_ENDOMORPHISM) {
      let p = P0;
      let d2 = this;
      while (n > _0n2) {
        if (n & _1n2)
          p = p.add(d2);
        d2 = d2.double();
        n >>= _1n2;
      }
      return p;
    }
    let { k1neg, k1, k2neg, k2 } = endo.splitScalar(n);
    let k1p = P0;
    let k2p = P0;
    let d = this;
    while (k1 > _0n2 || k2 > _0n2) {
      if (k1 & _1n2)
        k1p = k1p.add(d);
      if (k2 & _1n2)
        k2p = k2p.add(d);
      d = d.double();
      k1 >>= _1n2;
      k2 >>= _1n2;
    }
    if (k1neg)
      k1p = k1p.negate();
    if (k2neg)
      k2p = k2p.negate();
    k2p = new _JacobianPoint(mod3(k2p.x * endo.beta), k2p.y, k2p.z);
    return k1p.add(k2p);
  }
  precomputeWindow(W) {
    const windows = USE_ENDOMORPHISM ? 128 / W + 1 : 256 / W + 1;
    const points = [];
    let p = this;
    let base4 = p;
    for (let window2 = 0; window2 < windows; window2++) {
      base4 = p;
      points.push(base4);
      for (let i = 1; i < 2 ** (W - 1); i++) {
        base4 = base4.add(p);
        points.push(base4);
      }
      p = base4.double();
    }
    return points;
  }
  wNAF(n, affinePoint) {
    if (!affinePoint && this.equals(_JacobianPoint.BASE))
      affinePoint = Point2.BASE;
    const W = affinePoint && affinePoint._WINDOW_SIZE || 1;
    if (256 % W) {
      throw new Error("Point#wNAF: Invalid precomputation window, must be power of 2");
    }
    let precomputes = affinePoint && pointPrecomputes2.get(affinePoint);
    if (!precomputes) {
      precomputes = this.precomputeWindow(W);
      if (affinePoint && W !== 1) {
        precomputes = _JacobianPoint.normalizeZ(precomputes);
        pointPrecomputes2.set(affinePoint, precomputes);
      }
    }
    let p = _JacobianPoint.ZERO;
    let f = _JacobianPoint.BASE;
    const windows = 1 + (USE_ENDOMORPHISM ? 128 / W : 256 / W);
    const windowSize = 2 ** (W - 1);
    const mask = BigInt(2 ** W - 1);
    const maxNumber = 2 ** W;
    const shiftBy = BigInt(W);
    for (let window2 = 0; window2 < windows; window2++) {
      const offset = window2 * windowSize;
      let wbits = Number(n & mask);
      n >>= shiftBy;
      if (wbits > windowSize) {
        wbits -= maxNumber;
        n += _1n2;
      }
      const offset1 = offset;
      const offset2 = offset + Math.abs(wbits) - 1;
      const cond1 = window2 % 2 !== 0;
      const cond2 = wbits < 0;
      if (wbits === 0) {
        f = f.add(constTimeNegate2(cond1, precomputes[offset1]));
      } else {
        p = p.add(constTimeNegate2(cond2, precomputes[offset2]));
      }
    }
    return { p, f };
  }
  multiply(scalar, affinePoint) {
    let n = normalizeScalar2(scalar);
    let point;
    let fake;
    if (USE_ENDOMORPHISM) {
      const { k1neg, k1, k2neg, k2 } = endo.splitScalar(n);
      let { p: k1p, f: f1p } = this.wNAF(k1, affinePoint);
      let { p: k2p, f: f2p } = this.wNAF(k2, affinePoint);
      k1p = constTimeNegate2(k1neg, k1p);
      k2p = constTimeNegate2(k2neg, k2p);
      k2p = new _JacobianPoint(mod3(k2p.x * endo.beta), k2p.y, k2p.z);
      point = k1p.add(k2p);
      fake = f1p.add(f2p);
    } else {
      const { p, f } = this.wNAF(n, affinePoint);
      point = p;
      fake = f;
    }
    return _JacobianPoint.normalizeZ([point, fake])[0];
  }
  toAffine(invZ) {
    const { x, y, z } = this;
    const is0 = this.equals(_JacobianPoint.ZERO);
    if (invZ == null)
      invZ = is0 ? _8n2 : invert2(z);
    const iz1 = invZ;
    const iz2 = mod3(iz1 * iz1);
    const iz3 = mod3(iz2 * iz1);
    const ax = mod3(x * iz2);
    const ay = mod3(y * iz3);
    const zz = mod3(z * iz1);
    if (is0)
      return Point2.ZERO;
    if (zz !== _1n2)
      throw new Error("invZ was invalid");
    return new Point2(ax, ay);
  }
};
JacobianPoint.BASE = new JacobianPoint(CURVE2.Gx, CURVE2.Gy, _1n2);
JacobianPoint.ZERO = new JacobianPoint(_0n2, _1n2, _0n2);
function constTimeNegate2(condition, item) {
  const neg = item.negate();
  return condition ? neg : item;
}
var pointPrecomputes2 = /* @__PURE__ */ new WeakMap();
var Point2 = class _Point {
  constructor(x, y) {
    this.x = x;
    this.y = y;
  }
  _setWindowSize(windowSize) {
    this._WINDOW_SIZE = windowSize;
    pointPrecomputes2.delete(this);
  }
  hasEvenY() {
    return this.y % _2n2 === _0n2;
  }
  static fromCompressedHex(bytes) {
    const isShort = bytes.length === 32;
    const x = bytesToNumber(isShort ? bytes : bytes.subarray(1));
    if (!isValidFieldElement(x))
      throw new Error("Point is not on curve");
    const y2 = weierstrass(x);
    let y = sqrtMod(y2);
    const isYOdd = (y & _1n2) === _1n2;
    if (isShort) {
      if (isYOdd)
        y = mod3(-y);
    } else {
      const isFirstByteOdd = (bytes[0] & 1) === 1;
      if (isFirstByteOdd !== isYOdd)
        y = mod3(-y);
    }
    const point = new _Point(x, y);
    point.assertValidity();
    return point;
  }
  static fromUncompressedHex(bytes) {
    const x = bytesToNumber(bytes.subarray(1, fieldLen + 1));
    const y = bytesToNumber(bytes.subarray(fieldLen + 1, fieldLen * 2 + 1));
    const point = new _Point(x, y);
    point.assertValidity();
    return point;
  }
  static fromHex(hex) {
    const bytes = ensureBytes2(hex);
    const len = bytes.length;
    const header = bytes[0];
    if (len === fieldLen)
      return this.fromCompressedHex(bytes);
    if (len === compressedLen && (header === 2 || header === 3)) {
      return this.fromCompressedHex(bytes);
    }
    if (len === uncompressedLen && header === 4)
      return this.fromUncompressedHex(bytes);
    throw new Error(`Point.fromHex: received invalid point. Expected 32-${compressedLen} compressed bytes or ${uncompressedLen} uncompressed bytes, not ${len}`);
  }
  static fromPrivateKey(privateKey) {
    return _Point.BASE.multiply(normalizePrivateKey(privateKey));
  }
  static fromSignature(msgHash, signature, recovery) {
    const { r, s } = normalizeSignature(signature);
    if (![0, 1, 2, 3].includes(recovery))
      throw new Error("Cannot recover: invalid recovery bit");
    const h = truncateHash(ensureBytes2(msgHash));
    const { n } = CURVE2;
    const radj = recovery === 2 || recovery === 3 ? r + n : r;
    const rinv = invert2(radj, n);
    const u1 = mod3(-h * rinv, n);
    const u2 = mod3(s * rinv, n);
    const prefix = recovery & 1 ? "03" : "02";
    const R = _Point.fromHex(prefix + numTo32bStr(radj));
    const Q = _Point.BASE.multiplyAndAddUnsafe(R, u1, u2);
    if (!Q)
      throw new Error("Cannot recover signature: point at infinify");
    Q.assertValidity();
    return Q;
  }
  toRawBytes(isCompressed = false) {
    return hexToBytes2(this.toHex(isCompressed));
  }
  toHex(isCompressed = false) {
    const x = numTo32bStr(this.x);
    if (isCompressed) {
      const prefix = this.hasEvenY() ? "02" : "03";
      return `${prefix}${x}`;
    } else {
      return `04${x}${numTo32bStr(this.y)}`;
    }
  }
  toHexX() {
    return this.toHex(true).slice(2);
  }
  toRawX() {
    return this.toRawBytes(true).slice(1);
  }
  assertValidity() {
    const msg = "Point is not on elliptic curve";
    const { x, y } = this;
    if (!isValidFieldElement(x) || !isValidFieldElement(y))
      throw new Error(msg);
    const left = mod3(y * y);
    const right = weierstrass(x);
    if (mod3(left - right) !== _0n2)
      throw new Error(msg);
  }
  equals(other) {
    return this.x === other.x && this.y === other.y;
  }
  negate() {
    return new _Point(this.x, mod3(-this.y));
  }
  double() {
    return JacobianPoint.fromAffine(this).double().toAffine();
  }
  add(other) {
    return JacobianPoint.fromAffine(this).add(JacobianPoint.fromAffine(other)).toAffine();
  }
  subtract(other) {
    return this.add(other.negate());
  }
  multiply(scalar) {
    return JacobianPoint.fromAffine(this).multiply(scalar, this).toAffine();
  }
  multiplyAndAddUnsafe(Q, a, b) {
    const P = JacobianPoint.fromAffine(this);
    const aP = a === _0n2 || a === _1n2 || this !== _Point.BASE ? P.multiplyUnsafe(a) : P.multiply(a);
    const bQ = JacobianPoint.fromAffine(Q).multiplyUnsafe(b);
    const sum = aP.add(bQ);
    return sum.equals(JacobianPoint.ZERO) ? void 0 : sum.toAffine();
  }
};
Point2.BASE = new Point2(CURVE2.Gx, CURVE2.Gy);
Point2.ZERO = new Point2(_0n2, _0n2);
function sliceDER(s) {
  return Number.parseInt(s[0], 16) >= 8 ? "00" + s : s;
}
function parseDERInt(data) {
  if (data.length < 2 || data[0] !== 2) {
    throw new Error(`Invalid signature integer tag: ${bytesToHex2(data)}`);
  }
  const len = data[1];
  const res = data.subarray(2, len + 2);
  if (!len || res.length !== len) {
    throw new Error(`Invalid signature integer: wrong length`);
  }
  if (res[0] === 0 && res[1] <= 127) {
    throw new Error("Invalid signature integer: trailing length");
  }
  return { data: bytesToNumber(res), left: data.subarray(len + 2) };
}
function parseDERSignature(data) {
  if (data.length < 2 || data[0] != 48) {
    throw new Error(`Invalid signature tag: ${bytesToHex2(data)}`);
  }
  if (data[1] !== data.length - 2) {
    throw new Error("Invalid signature: incorrect length");
  }
  const { data: r, left: sBytes } = parseDERInt(data.subarray(2));
  const { data: s, left: rBytesLeft } = parseDERInt(sBytes);
  if (rBytesLeft.length) {
    throw new Error(`Invalid signature: left bytes after parsing: ${bytesToHex2(rBytesLeft)}`);
  }
  return { r, s };
}
var Signature2 = class _Signature {
  constructor(r, s) {
    this.r = r;
    this.s = s;
    this.assertValidity();
  }
  static fromCompact(hex) {
    const arr = hex instanceof Uint8Array;
    const name3 = "Signature.fromCompact";
    if (typeof hex !== "string" && !arr)
      throw new TypeError(`${name3}: Expected string or Uint8Array`);
    const str = arr ? bytesToHex2(hex) : hex;
    if (str.length !== 128)
      throw new Error(`${name3}: Expected 64-byte hex`);
    return new _Signature(hexToNumber(str.slice(0, 64)), hexToNumber(str.slice(64, 128)));
  }
  static fromDER(hex) {
    const arr = hex instanceof Uint8Array;
    if (typeof hex !== "string" && !arr)
      throw new TypeError(`Signature.fromDER: Expected string or Uint8Array`);
    const { r, s } = parseDERSignature(arr ? hex : hexToBytes2(hex));
    return new _Signature(r, s);
  }
  static fromHex(hex) {
    return this.fromDER(hex);
  }
  assertValidity() {
    const { r, s } = this;
    if (!isWithinCurveOrder(r))
      throw new Error("Invalid Signature: r must be 0 < r < n");
    if (!isWithinCurveOrder(s))
      throw new Error("Invalid Signature: s must be 0 < s < n");
  }
  hasHighS() {
    const HALF = CURVE2.n >> _1n2;
    return this.s > HALF;
  }
  normalizeS() {
    return this.hasHighS() ? new _Signature(this.r, mod3(-this.s, CURVE2.n)) : this;
  }
  toDERRawBytes() {
    return hexToBytes2(this.toDERHex());
  }
  toDERHex() {
    const sHex = sliceDER(numberToHexUnpadded(this.s));
    const rHex = sliceDER(numberToHexUnpadded(this.r));
    const sHexL = sHex.length / 2;
    const rHexL = rHex.length / 2;
    const sLen = numberToHexUnpadded(sHexL);
    const rLen = numberToHexUnpadded(rHexL);
    const length3 = numberToHexUnpadded(rHexL + sHexL + 4);
    return `30${length3}02${rLen}${rHex}02${sLen}${sHex}`;
  }
  toRawBytes() {
    return this.toDERRawBytes();
  }
  toHex() {
    return this.toDERHex();
  }
  toCompactRawBytes() {
    return hexToBytes2(this.toCompactHex());
  }
  toCompactHex() {
    return numTo32bStr(this.r) + numTo32bStr(this.s);
  }
};
function concatBytes2(...arrays) {
  if (!arrays.every((b) => b instanceof Uint8Array))
    throw new Error("Uint8Array list expected");
  if (arrays.length === 1)
    return arrays[0];
  const length3 = arrays.reduce((a, arr) => a + arr.length, 0);
  const result = new Uint8Array(length3);
  for (let i = 0, pad = 0; i < arrays.length; i++) {
    const arr = arrays[i];
    result.set(arr, pad);
    pad += arr.length;
  }
  return result;
}
var hexes2 = Array.from({ length: 256 }, (v, i) => i.toString(16).padStart(2, "0"));
function bytesToHex2(uint8a) {
  if (!(uint8a instanceof Uint8Array))
    throw new Error("Expected Uint8Array");
  let hex = "";
  for (let i = 0; i < uint8a.length; i++) {
    hex += hexes2[uint8a[i]];
  }
  return hex;
}
var POW_2_2562 = BigInt("0x10000000000000000000000000000000000000000000000000000000000000000");
function numTo32bStr(num) {
  if (typeof num !== "bigint")
    throw new Error("Expected bigint");
  if (!(_0n2 <= num && num < POW_2_2562))
    throw new Error("Expected number 0 <= n < 2^256");
  return num.toString(16).padStart(64, "0");
}
function numTo32b(num) {
  const b = hexToBytes2(numTo32bStr(num));
  if (b.length !== 32)
    throw new Error("Error: expected 32 bytes");
  return b;
}
function numberToHexUnpadded(num) {
  const hex = num.toString(16);
  return hex.length & 1 ? `0${hex}` : hex;
}
function hexToNumber(hex) {
  if (typeof hex !== "string") {
    throw new TypeError("hexToNumber: expected string, got " + typeof hex);
  }
  return BigInt(`0x${hex}`);
}
function hexToBytes2(hex) {
  if (typeof hex !== "string") {
    throw new TypeError("hexToBytes: expected string, got " + typeof hex);
  }
  if (hex.length % 2)
    throw new Error("hexToBytes: received invalid unpadded hex" + hex.length);
  const array = new Uint8Array(hex.length / 2);
  for (let i = 0; i < array.length; i++) {
    const j = i * 2;
    const hexByte = hex.slice(j, j + 2);
    const byte = Number.parseInt(hexByte, 16);
    if (Number.isNaN(byte) || byte < 0)
      throw new Error("Invalid byte sequence");
    array[i] = byte;
  }
  return array;
}
function bytesToNumber(bytes) {
  return hexToNumber(bytesToHex2(bytes));
}
function ensureBytes2(hex) {
  return hex instanceof Uint8Array ? Uint8Array.from(hex) : hexToBytes2(hex);
}
function normalizeScalar2(num) {
  if (typeof num === "number" && Number.isSafeInteger(num) && num > 0)
    return BigInt(num);
  if (typeof num === "bigint" && isWithinCurveOrder(num))
    return num;
  throw new TypeError("Expected valid private scalar: 0 < scalar < curve.n");
}
function mod3(a, b = CURVE2.P) {
  const result = a % b;
  return result >= _0n2 ? result : b + result;
}
function pow22(x, power) {
  const { P } = CURVE2;
  let res = x;
  while (power-- > _0n2) {
    res *= res;
    res %= P;
  }
  return res;
}
function sqrtMod(x) {
  const { P } = CURVE2;
  const _6n = BigInt(6);
  const _11n = BigInt(11);
  const _22n = BigInt(22);
  const _23n = BigInt(23);
  const _44n = BigInt(44);
  const _88n = BigInt(88);
  const b2 = x * x * x % P;
  const b3 = b2 * b2 * x % P;
  const b6 = pow22(b3, _3n) * b3 % P;
  const b9 = pow22(b6, _3n) * b3 % P;
  const b11 = pow22(b9, _2n2) * b2 % P;
  const b22 = pow22(b11, _11n) * b11 % P;
  const b44 = pow22(b22, _22n) * b22 % P;
  const b88 = pow22(b44, _44n) * b44 % P;
  const b176 = pow22(b88, _88n) * b88 % P;
  const b220 = pow22(b176, _44n) * b44 % P;
  const b223 = pow22(b220, _3n) * b3 % P;
  const t1 = pow22(b223, _23n) * b22 % P;
  const t2 = pow22(t1, _6n) * b2 % P;
  const rt = pow22(t2, _2n2);
  const xc = rt * rt % P;
  if (xc !== x)
    throw new Error("Cannot find square root");
  return rt;
}
function invert2(number, modulo = CURVE2.P) {
  if (number === _0n2 || modulo <= _0n2) {
    throw new Error(`invert: expected positive integers, got n=${number} mod=${modulo}`);
  }
  let a = mod3(number, modulo);
  let b = modulo;
  let x = _0n2, y = _1n2, u = _1n2, v = _0n2;
  while (a !== _0n2) {
    const q = b / a;
    const r = b % a;
    const m = x - u * q;
    const n = y - v * q;
    b = a, a = r, x = u, y = v, u = m, v = n;
  }
  const gcd = b;
  if (gcd !== _1n2)
    throw new Error("invert: does not exist");
  return mod3(x, modulo);
}
function invertBatch2(nums, p = CURVE2.P) {
  const scratch = new Array(nums.length);
  const lastMultiplied = nums.reduce((acc, num, i) => {
    if (num === _0n2)
      return acc;
    scratch[i] = acc;
    return mod3(acc * num, p);
  }, _1n2);
  const inverted = invert2(lastMultiplied, p);
  nums.reduceRight((acc, num, i) => {
    if (num === _0n2)
      return acc;
    scratch[i] = mod3(acc * scratch[i], p);
    return mod3(acc * num, p);
  }, inverted);
  return scratch;
}
function bits2int_2(bytes) {
  const delta = bytes.length * 8 - groupLen * 8;
  const num = bytesToNumber(bytes);
  return delta > 0 ? num >> BigInt(delta) : num;
}
function truncateHash(hash, truncateOnly = false) {
  const h = bits2int_2(hash);
  if (truncateOnly)
    return h;
  const { n } = CURVE2;
  return h >= n ? h - n : h;
}
var _sha256Sync;
var _hmacSha256Sync;
var HmacDrbg = class {
  constructor(hashLen2, qByteLen) {
    this.hashLen = hashLen2;
    this.qByteLen = qByteLen;
    if (typeof hashLen2 !== "number" || hashLen2 < 2)
      throw new Error("hashLen must be a number");
    if (typeof qByteLen !== "number" || qByteLen < 2)
      throw new Error("qByteLen must be a number");
    this.v = new Uint8Array(hashLen2).fill(1);
    this.k = new Uint8Array(hashLen2).fill(0);
    this.counter = 0;
  }
  hmac(...values) {
    return utils2.hmacSha256(this.k, ...values);
  }
  hmacSync(...values) {
    return _hmacSha256Sync(this.k, ...values);
  }
  checkSync() {
    if (typeof _hmacSha256Sync !== "function")
      throw new ShaError("hmacSha256Sync needs to be set");
  }
  incr() {
    if (this.counter >= 1e3)
      throw new Error("Tried 1,000 k values for sign(), all were invalid");
    this.counter += 1;
  }
  async reseed(seed = new Uint8Array()) {
    this.k = await this.hmac(this.v, Uint8Array.from([0]), seed);
    this.v = await this.hmac(this.v);
    if (seed.length === 0)
      return;
    this.k = await this.hmac(this.v, Uint8Array.from([1]), seed);
    this.v = await this.hmac(this.v);
  }
  reseedSync(seed = new Uint8Array()) {
    this.checkSync();
    this.k = this.hmacSync(this.v, Uint8Array.from([0]), seed);
    this.v = this.hmacSync(this.v);
    if (seed.length === 0)
      return;
    this.k = this.hmacSync(this.v, Uint8Array.from([1]), seed);
    this.v = this.hmacSync(this.v);
  }
  async generate() {
    this.incr();
    let len = 0;
    const out = [];
    while (len < this.qByteLen) {
      this.v = await this.hmac(this.v);
      const sl = this.v.slice();
      out.push(sl);
      len += this.v.length;
    }
    return concatBytes2(...out);
  }
  generateSync() {
    this.checkSync();
    this.incr();
    let len = 0;
    const out = [];
    while (len < this.qByteLen) {
      this.v = this.hmacSync(this.v);
      const sl = this.v.slice();
      out.push(sl);
      len += this.v.length;
    }
    return concatBytes2(...out);
  }
};
function isWithinCurveOrder(num) {
  return _0n2 < num && num < CURVE2.n;
}
function isValidFieldElement(num) {
  return _0n2 < num && num < CURVE2.P;
}
function kmdToSig(kBytes, m, d, lowS = true) {
  const { n } = CURVE2;
  const k = truncateHash(kBytes, true);
  if (!isWithinCurveOrder(k))
    return;
  const kinv = invert2(k, n);
  const q = Point2.BASE.multiply(k);
  const r = mod3(q.x, n);
  if (r === _0n2)
    return;
  const s = mod3(kinv * mod3(m + d * r, n), n);
  if (s === _0n2)
    return;
  let sig = new Signature2(r, s);
  let recovery = (q.x === sig.r ? 0 : 2) | Number(q.y & _1n2);
  if (lowS && sig.hasHighS()) {
    sig = sig.normalizeS();
    recovery ^= 1;
  }
  return { sig, recovery };
}
function normalizePrivateKey(key) {
  let num;
  if (typeof key === "bigint") {
    num = key;
  } else if (typeof key === "number" && Number.isSafeInteger(key) && key > 0) {
    num = BigInt(key);
  } else if (typeof key === "string") {
    if (key.length !== 2 * groupLen)
      throw new Error("Expected 32 bytes of private key");
    num = hexToNumber(key);
  } else if (key instanceof Uint8Array) {
    if (key.length !== groupLen)
      throw new Error("Expected 32 bytes of private key");
    num = bytesToNumber(key);
  } else {
    throw new TypeError("Expected valid private key");
  }
  if (!isWithinCurveOrder(num))
    throw new Error("Expected private key: 0 < key < n");
  return num;
}
function normalizePublicKey(publicKey) {
  if (publicKey instanceof Point2) {
    publicKey.assertValidity();
    return publicKey;
  } else {
    return Point2.fromHex(publicKey);
  }
}
function normalizeSignature(signature) {
  if (signature instanceof Signature2) {
    signature.assertValidity();
    return signature;
  }
  try {
    return Signature2.fromDER(signature);
  } catch (error) {
    return Signature2.fromCompact(signature);
  }
}
function getPublicKey2(privateKey, isCompressed = false) {
  return Point2.fromPrivateKey(privateKey).toRawBytes(isCompressed);
}
function bits2int(bytes) {
  const slice = bytes.length > fieldLen ? bytes.slice(0, fieldLen) : bytes;
  return bytesToNumber(slice);
}
function bits2octets(bytes) {
  const z1 = bits2int(bytes);
  const z2 = mod3(z1, CURVE2.n);
  return int2octets(z2 < _0n2 ? z1 : z2);
}
function int2octets(num) {
  return numTo32b(num);
}
function initSigArgs(msgHash, privateKey, extraEntropy) {
  if (msgHash == null)
    throw new Error(`sign: expected valid message hash, not "${msgHash}"`);
  const h1 = ensureBytes2(msgHash);
  const d = normalizePrivateKey(privateKey);
  const seedArgs = [int2octets(d), bits2octets(h1)];
  if (extraEntropy != null) {
    if (extraEntropy === true)
      extraEntropy = utils2.randomBytes(fieldLen);
    const e = ensureBytes2(extraEntropy);
    if (e.length !== fieldLen)
      throw new Error(`sign: Expected ${fieldLen} bytes of extra data`);
    seedArgs.push(e);
  }
  const seed = concatBytes2(...seedArgs);
  const m = bits2int(h1);
  return { seed, m, d };
}
function finalizeSig(recSig, opts) {
  const { sig, recovery } = recSig;
  const { der, recovered } = Object.assign({ canonical: true, der: true }, opts);
  const hashed = der ? sig.toDERRawBytes() : sig.toCompactRawBytes();
  return recovered ? [hashed, recovery] : hashed;
}
async function sign2(msgHash, privKey, opts = {}) {
  const { seed, m, d } = initSigArgs(msgHash, privKey, opts.extraEntropy);
  const drbg = new HmacDrbg(hashLen, groupLen);
  await drbg.reseed(seed);
  let sig;
  while (!(sig = kmdToSig(await drbg.generate(), m, d, opts.canonical)))
    await drbg.reseed();
  return finalizeSig(sig, opts);
}
var vopts = { strict: true };
function verify2(signature, msgHash, publicKey, opts = vopts) {
  let sig;
  try {
    sig = normalizeSignature(signature);
    msgHash = ensureBytes2(msgHash);
  } catch (error) {
    return false;
  }
  const { r, s } = sig;
  if (opts.strict && sig.hasHighS())
    return false;
  const h = truncateHash(msgHash);
  let P;
  try {
    P = normalizePublicKey(publicKey);
  } catch (error) {
    return false;
  }
  const { n } = CURVE2;
  const sinv = invert2(s, n);
  const u1 = mod3(h * sinv, n);
  const u2 = mod3(r * sinv, n);
  const R = Point2.BASE.multiplyAndAddUnsafe(P, u1, u2);
  if (!R)
    return false;
  const v = mod3(R.x, n);
  return v === r;
}
Point2.BASE._setWindowSize(8);
var crypto3 = {
  node: nodeCrypto2,
  web: typeof self === "object" && "crypto" in self ? self.crypto : void 0
};
var TAGGED_HASH_PREFIXES = {};
var utils2 = {
  bytesToHex: bytesToHex2,
  hexToBytes: hexToBytes2,
  concatBytes: concatBytes2,
  mod: mod3,
  invert: invert2,
  isValidPrivateKey(privateKey) {
    try {
      normalizePrivateKey(privateKey);
      return true;
    } catch (error) {
      return false;
    }
  },
  _bigintTo32Bytes: numTo32b,
  _normalizePrivateKey: normalizePrivateKey,
  hashToPrivateKey: (hash) => {
    hash = ensureBytes2(hash);
    const minLen = groupLen + 8;
    if (hash.length < minLen || hash.length > 1024) {
      throw new Error(`Expected valid bytes of private key as per FIPS 186`);
    }
    const num = mod3(bytesToNumber(hash), CURVE2.n - _1n2) + _1n2;
    return numTo32b(num);
  },
  randomBytes: (bytesLength = 32) => {
    if (crypto3.web) {
      return crypto3.web.getRandomValues(new Uint8Array(bytesLength));
    } else if (crypto3.node) {
      const { randomBytes: randomBytes2 } = crypto3.node;
      return Uint8Array.from(randomBytes2(bytesLength));
    } else {
      throw new Error("The environment doesn't have randomBytes function");
    }
  },
  randomPrivateKey: () => utils2.hashToPrivateKey(utils2.randomBytes(groupLen + 8)),
  precompute(windowSize = 8, point = Point2.BASE) {
    const cached = point === Point2.BASE ? point : new Point2(point.x, point.y);
    cached._setWindowSize(windowSize);
    cached.multiply(_3n);
    return cached;
  },
  sha256: async (...messages) => {
    if (crypto3.web) {
      const buffer = await crypto3.web.subtle.digest("SHA-256", concatBytes2(...messages));
      return new Uint8Array(buffer);
    } else if (crypto3.node) {
      const { createHash } = crypto3.node;
      const hash = createHash("sha256");
      messages.forEach((m) => hash.update(m));
      return Uint8Array.from(hash.digest());
    } else {
      throw new Error("The environment doesn't have sha256 function");
    }
  },
  hmacSha256: async (key, ...messages) => {
    if (crypto3.web) {
      const ckey = await crypto3.web.subtle.importKey("raw", key, { name: "HMAC", hash: { name: "SHA-256" } }, false, ["sign"]);
      const message2 = concatBytes2(...messages);
      const buffer = await crypto3.web.subtle.sign("HMAC", ckey, message2);
      return new Uint8Array(buffer);
    } else if (crypto3.node) {
      const { createHmac } = crypto3.node;
      const hash = createHmac("sha256", key);
      messages.forEach((m) => hash.update(m));
      return Uint8Array.from(hash.digest());
    } else {
      throw new Error("The environment doesn't have hmac-sha256 function");
    }
  },
  sha256Sync: void 0,
  hmacSha256Sync: void 0,
  taggedHash: async (tag, ...messages) => {
    let tagP = TAGGED_HASH_PREFIXES[tag];
    if (tagP === void 0) {
      const tagH = await utils2.sha256(Uint8Array.from(tag, (c) => c.charCodeAt(0)));
      tagP = concatBytes2(tagH, tagH);
      TAGGED_HASH_PREFIXES[tag] = tagP;
    }
    return utils2.sha256(tagP, ...messages);
  },
  taggedHashSync: (tag, ...messages) => {
    if (typeof _sha256Sync !== "function")
      throw new ShaError("sha256Sync is undefined, you need to set it");
    let tagP = TAGGED_HASH_PREFIXES[tag];
    if (tagP === void 0) {
      const tagH = _sha256Sync(Uint8Array.from(tag, (c) => c.charCodeAt(0)));
      tagP = concatBytes2(tagH, tagH);
      TAGGED_HASH_PREFIXES[tag] = tagP;
    }
    return _sha256Sync(tagP, ...messages);
  },
  _JacobianPoint: JacobianPoint
};
Object.defineProperties(utils2, {
  sha256Sync: {
    configurable: false,
    get() {
      return _sha256Sync;
    },
    set(val) {
      if (!_sha256Sync)
        _sha256Sync = val;
    }
  },
  hmacSha256Sync: {
    configurable: false,
    get() {
      return _hmacSha256Sync;
    },
    set(val) {
      if (!_hmacSha256Sync)
        _hmacSha256Sync = val;
    }
  }
});

// ../crypto/dist/src/random-bytes.js
function randomBytes(length3) {
  if (isNaN(length3) || length3 <= 0) {
    throw new CodeError("random bytes length must be a Number bigger than 0", "ERR_INVALID_LENGTH");
  }
  return utils2.randomBytes(length3);
}

// ../crypto/dist/src/keys/jwk2pem.js
var import_rsa = __toESM(require_rsa(), 1);
var import_forge2 = __toESM(require_forge(), 1);
function convert(key, types) {
  return types.map((t) => base64urlToBigInteger(key[t]));
}
function jwk2priv(key) {
  return import_forge2.default.pki.setRsaPrivateKey(...convert(key, ["n", "e", "d", "p", "q", "dp", "dq", "qi"]));
}
function jwk2pub(key) {
  return import_forge2.default.pki.setRsaPublicKey(...convert(key, ["n", "e"]));
}

// ../crypto/dist/src/keys/rsa-utils.js
var rsa_utils_exports = {};
__export(rsa_utils_exports, {
  jwkToPkcs1: () => jwkToPkcs1,
  jwkToPkix: () => jwkToPkix,
  pkcs1ToJwk: () => pkcs1ToJwk,
  pkixToJwk: () => pkixToJwk
});
var import_asn1 = __toESM(require_asn1(), 1);
var import_rsa2 = __toESM(require_rsa(), 1);
var import_forge3 = __toESM(require_forge(), 1);
function pkcs1ToJwk(bytes) {
  const asn1 = import_forge3.default.asn1.fromDer(toString3(bytes, "ascii"));
  const privateKey = import_forge3.default.pki.privateKeyFromAsn1(asn1);
  return {
    kty: "RSA",
    n: bigIntegerToUintBase64url(privateKey.n),
    e: bigIntegerToUintBase64url(privateKey.e),
    d: bigIntegerToUintBase64url(privateKey.d),
    p: bigIntegerToUintBase64url(privateKey.p),
    q: bigIntegerToUintBase64url(privateKey.q),
    dp: bigIntegerToUintBase64url(privateKey.dP),
    dq: bigIntegerToUintBase64url(privateKey.dQ),
    qi: bigIntegerToUintBase64url(privateKey.qInv),
    alg: "RS256"
  };
}
function jwkToPkcs1(jwk) {
  if (jwk.n == null || jwk.e == null || jwk.d == null || jwk.p == null || jwk.q == null || jwk.dp == null || jwk.dq == null || jwk.qi == null) {
    throw new CodeError("JWK was missing components", "ERR_INVALID_PARAMETERS");
  }
  const asn1 = import_forge3.default.pki.privateKeyToAsn1({
    n: base64urlToBigInteger(jwk.n),
    e: base64urlToBigInteger(jwk.e),
    d: base64urlToBigInteger(jwk.d),
    p: base64urlToBigInteger(jwk.p),
    q: base64urlToBigInteger(jwk.q),
    dP: base64urlToBigInteger(jwk.dp),
    dQ: base64urlToBigInteger(jwk.dq),
    qInv: base64urlToBigInteger(jwk.qi)
  });
  return fromString2(import_forge3.default.asn1.toDer(asn1).getBytes(), "ascii");
}
function pkixToJwk(bytes) {
  const asn1 = import_forge3.default.asn1.fromDer(toString3(bytes, "ascii"));
  const publicKey = import_forge3.default.pki.publicKeyFromAsn1(asn1);
  return {
    kty: "RSA",
    n: bigIntegerToUintBase64url(publicKey.n),
    e: bigIntegerToUintBase64url(publicKey.e)
  };
}
function jwkToPkix(jwk) {
  if (jwk.n == null || jwk.e == null) {
    throw new CodeError("JWK was missing components", "ERR_INVALID_PARAMETERS");
  }
  const asn1 = import_forge3.default.pki.publicKeyToAsn1({
    n: base64urlToBigInteger(jwk.n),
    e: base64urlToBigInteger(jwk.e)
  });
  return fromString2(import_forge3.default.asn1.toDer(asn1).getBytes(), "ascii");
}

// ../crypto/dist/src/keys/rsa-browser.js
async function generateKey2(bits2) {
  const pair = await webcrypto_default.get().subtle.generateKey({
    name: "RSASSA-PKCS1-v1_5",
    modulusLength: bits2,
    publicExponent: new Uint8Array([1, 0, 1]),
    hash: { name: "SHA-256" }
  }, true, ["sign", "verify"]);
  const keys = await exportKey(pair);
  return {
    privateKey: keys[0],
    publicKey: keys[1]
  };
}
async function unmarshalPrivateKey(key) {
  const privateKey = await webcrypto_default.get().subtle.importKey("jwk", key, {
    name: "RSASSA-PKCS1-v1_5",
    hash: { name: "SHA-256" }
  }, true, ["sign"]);
  const pair = [
    privateKey,
    await derivePublicFromPrivate(key)
  ];
  const keys = await exportKey({
    privateKey: pair[0],
    publicKey: pair[1]
  });
  return {
    privateKey: keys[0],
    publicKey: keys[1]
  };
}
async function hashAndSign2(key, msg) {
  const privateKey = await webcrypto_default.get().subtle.importKey("jwk", key, {
    name: "RSASSA-PKCS1-v1_5",
    hash: { name: "SHA-256" }
  }, false, ["sign"]);
  const sig = await webcrypto_default.get().subtle.sign({ name: "RSASSA-PKCS1-v1_5" }, privateKey, Uint8Array.from(msg));
  return new Uint8Array(sig, 0, sig.byteLength);
}
async function hashAndVerify2(key, sig, msg) {
  const publicKey = await webcrypto_default.get().subtle.importKey("jwk", key, {
    name: "RSASSA-PKCS1-v1_5",
    hash: { name: "SHA-256" }
  }, false, ["verify"]);
  return webcrypto_default.get().subtle.verify({ name: "RSASSA-PKCS1-v1_5" }, publicKey, sig, msg);
}
async function exportKey(pair) {
  if (pair.privateKey == null || pair.publicKey == null) {
    throw new CodeError("Private and public key are required", "ERR_INVALID_PARAMETERS");
  }
  return Promise.all([
    webcrypto_default.get().subtle.exportKey("jwk", pair.privateKey),
    webcrypto_default.get().subtle.exportKey("jwk", pair.publicKey)
  ]);
}
async function derivePublicFromPrivate(jwKey) {
  return webcrypto_default.get().subtle.importKey("jwk", {
    kty: jwKey.kty,
    n: jwKey.n,
    e: jwKey.e
  }, {
    name: "RSASSA-PKCS1-v1_5",
    hash: { name: "SHA-256" }
  }, true, ["verify"]);
}
function convertKey(key, pub, msg, handle) {
  const fkey = pub ? jwk2pub(key) : jwk2priv(key);
  const fmsg = toString3(Uint8Array.from(msg), "ascii");
  const fomsg = handle(fmsg, fkey);
  return fromString2(fomsg, "ascii");
}
function encrypt(key, msg) {
  return convertKey(key, true, msg, (msg2, key2) => key2.encrypt(msg2));
}
function decrypt(key, msg) {
  return convertKey(key, false, msg, (msg2, key2) => key2.decrypt(msg2));
}

// ../crypto/dist/src/keys/rsa-class.js
var RsaPublicKey = class {
  _key;
  constructor(key) {
    this._key = key;
  }
  async verify(data, sig) {
    return hashAndVerify2(this._key, sig, data);
  }
  marshal() {
    return rsa_utils_exports.jwkToPkix(this._key);
  }
  get bytes() {
    return PublicKey.encode({
      Type: KeyType.RSA,
      Data: this.marshal()
    }).subarray();
  }
  encrypt(bytes) {
    return encrypt(this._key, bytes);
  }
  equals(key) {
    return equals5(this.bytes, key.bytes);
  }
  async hash() {
    const { bytes } = await sha2562.digest(this.bytes);
    return bytes;
  }
};
var RsaPrivateKey = class {
  _key;
  _publicKey;
  constructor(key, publicKey) {
    this._key = key;
    this._publicKey = publicKey;
  }
  genSecret() {
    return randomBytes(16);
  }
  async sign(message2) {
    return hashAndSign2(this._key, message2);
  }
  get public() {
    if (this._publicKey == null) {
      throw new CodeError("public key not provided", "ERR_PUBKEY_NOT_PROVIDED");
    }
    return new RsaPublicKey(this._publicKey);
  }
  decrypt(bytes) {
    return decrypt(this._key, bytes);
  }
  marshal() {
    return rsa_utils_exports.jwkToPkcs1(this._key);
  }
  get bytes() {
    return PrivateKey.encode({
      Type: KeyType.RSA,
      Data: this.marshal()
    }).subarray();
  }
  equals(key) {
    return equals5(this.bytes, key.bytes);
  }
  async hash() {
    const { bytes } = await sha2562.digest(this.bytes);
    return bytes;
  }
  /**
   * Gets the ID of the key.
   *
   * The key id is the base58 encoding of the SHA-256 multihash of its public key.
   * The public key is a protobuf encoding containing a type and the DER encoding
   * of the PKCS SubjectPublicKeyInfo.
   */
  async id() {
    const hash = await this.public.hash();
    return toString3(hash, "base58btc");
  }
  /**
   * Exports the key into a password protected PEM format
   */
  async export(password, format3 = "pkcs-8") {
    if (format3 === "pkcs-8") {
      const buffer = new import_forge4.default.util.ByteBuffer(this.marshal());
      const asn1 = import_forge4.default.asn1.fromDer(buffer);
      const privateKey = import_forge4.default.pki.privateKeyFromAsn1(asn1);
      const options = {
        algorithm: "aes256",
        count: 1e4,
        saltSize: 128 / 8,
        prfAlgorithm: "sha512"
      };
      return import_forge4.default.pki.encryptRsaPrivateKey(privateKey, password, options);
    } else if (format3 === "libp2p-key") {
      return exporter(this.bytes, password);
    } else {
      throw new CodeError(`export format '${format3}' is not supported`, "ERR_INVALID_EXPORT_FORMAT");
    }
  }
};
async function unmarshalRsaPrivateKey(bytes) {
  const jwk = rsa_utils_exports.pkcs1ToJwk(bytes);
  const keys = await unmarshalPrivateKey(jwk);
  return new RsaPrivateKey(keys.privateKey, keys.publicKey);
}
function unmarshalRsaPublicKey(bytes) {
  const jwk = rsa_utils_exports.pkixToJwk(bytes);
  return new RsaPublicKey(jwk);
}
async function fromJwk(jwk) {
  const keys = await unmarshalPrivateKey(jwk);
  return new RsaPrivateKey(keys.privateKey, keys.publicKey);
}
async function generateKeyPair2(bits2) {
  const keys = await generateKey2(bits2);
  return new RsaPrivateKey(keys.privateKey, keys.publicKey);
}

// ../crypto/dist/src/keys/secp256k1-class.js
var secp256k1_class_exports = {};
__export(secp256k1_class_exports, {
  Secp256k1PrivateKey: () => Secp256k1PrivateKey,
  Secp256k1PublicKey: () => Secp256k1PublicKey,
  generateKeyPair: () => generateKeyPair3,
  unmarshalSecp256k1PrivateKey: () => unmarshalSecp256k1PrivateKey,
  unmarshalSecp256k1PublicKey: () => unmarshalSecp256k1PublicKey
});

// ../crypto/dist/src/keys/secp256k1.js
function generateKey3() {
  return utils2.randomPrivateKey();
}
async function hashAndSign3(key, msg) {
  const { digest: digest3 } = await sha2562.digest(msg);
  try {
    return await sign2(digest3, key);
  } catch (err) {
    throw new CodeError(String(err), "ERR_INVALID_INPUT");
  }
}
async function hashAndVerify3(key, sig, msg) {
  try {
    const { digest: digest3 } = await sha2562.digest(msg);
    return verify2(sig, digest3, key);
  } catch (err) {
    throw new CodeError(String(err), "ERR_INVALID_INPUT");
  }
}
function compressPublicKey(key) {
  const point = Point2.fromHex(key).toRawBytes(true);
  return point;
}
function validatePrivateKey(key) {
  try {
    getPublicKey2(key, true);
  } catch (err) {
    throw new CodeError(String(err), "ERR_INVALID_PRIVATE_KEY");
  }
}
function validatePublicKey(key) {
  try {
    Point2.fromHex(key);
  } catch (err) {
    throw new CodeError(String(err), "ERR_INVALID_PUBLIC_KEY");
  }
}
function computePublicKey(privateKey) {
  try {
    return getPublicKey2(privateKey, true);
  } catch (err) {
    throw new CodeError(String(err), "ERR_INVALID_PRIVATE_KEY");
  }
}

// ../crypto/dist/src/keys/secp256k1-class.js
var Secp256k1PublicKey = class {
  _key;
  constructor(key) {
    validatePublicKey(key);
    this._key = key;
  }
  async verify(data, sig) {
    return hashAndVerify3(this._key, sig, data);
  }
  marshal() {
    return compressPublicKey(this._key);
  }
  get bytes() {
    return PublicKey.encode({
      Type: KeyType.Secp256k1,
      Data: this.marshal()
    }).subarray();
  }
  equals(key) {
    return equals5(this.bytes, key.bytes);
  }
  async hash() {
    const { bytes } = await sha2562.digest(this.bytes);
    return bytes;
  }
};
var Secp256k1PrivateKey = class {
  _key;
  _publicKey;
  constructor(key, publicKey) {
    this._key = key;
    this._publicKey = publicKey ?? computePublicKey(key);
    validatePrivateKey(this._key);
    validatePublicKey(this._publicKey);
  }
  async sign(message2) {
    return hashAndSign3(this._key, message2);
  }
  get public() {
    return new Secp256k1PublicKey(this._publicKey);
  }
  marshal() {
    return this._key;
  }
  get bytes() {
    return PrivateKey.encode({
      Type: KeyType.Secp256k1,
      Data: this.marshal()
    }).subarray();
  }
  equals(key) {
    return equals5(this.bytes, key.bytes);
  }
  async hash() {
    const { bytes } = await sha2562.digest(this.bytes);
    return bytes;
  }
  /**
   * Gets the ID of the key.
   *
   * The key id is the base58 encoding of the SHA-256 multihash of its public key.
   * The public key is a protobuf encoding containing a type and the DER encoding
   * of the PKCS SubjectPublicKeyInfo.
   */
  async id() {
    const hash = await this.public.hash();
    return toString3(hash, "base58btc");
  }
  /**
   * Exports the key into a password protected `format`
   */
  async export(password, format3 = "libp2p-key") {
    if (format3 === "libp2p-key") {
      return exporter(this.bytes, password);
    } else {
      throw new CodeError(`export format '${format3}' is not supported`, "ERR_INVALID_EXPORT_FORMAT");
    }
  }
};
function unmarshalSecp256k1PrivateKey(bytes) {
  return new Secp256k1PrivateKey(bytes);
}
function unmarshalSecp256k1PublicKey(bytes) {
  return new Secp256k1PublicKey(bytes);
}
async function generateKeyPair3() {
  const privateKeyBytes = generateKey3();
  return new Secp256k1PrivateKey(privateKeyBytes);
}

// ../crypto/dist/src/keys/index.js
var supportedKeys = {
  rsa: rsa_class_exports,
  ed25519: ed25519_class_exports,
  secp256k1: secp256k1_class_exports
};
function unsupportedKey(type) {
  const supported = Object.keys(supportedKeys).join(" / ");
  return new CodeError(`invalid or unsupported key type ${type}. Must be ${supported}`, "ERR_UNSUPPORTED_KEY_TYPE");
}
function typeToKey(type) {
  type = type.toLowerCase();
  if (type === "rsa" || type === "ed25519" || type === "secp256k1") {
    return supportedKeys[type];
  }
  throw unsupportedKey(type);
}
async function generateKeyPair4(type, bits2) {
  return typeToKey(type).generateKeyPair(bits2 ?? 2048);
}
function unmarshalPublicKey(buf) {
  const decoded = PublicKey.decode(buf);
  const data = decoded.Data ?? new Uint8Array();
  switch (decoded.Type) {
    case KeyType.RSA:
      return supportedKeys.rsa.unmarshalRsaPublicKey(data);
    case KeyType.Ed25519:
      return supportedKeys.ed25519.unmarshalEd25519PublicKey(data);
    case KeyType.Secp256k1:
      return supportedKeys.secp256k1.unmarshalSecp256k1PublicKey(data);
    default:
      throw unsupportedKey(decoded.Type ?? "RSA");
  }
}
function marshalPublicKey(key, type) {
  type = (type ?? "rsa").toLowerCase();
  typeToKey(type);
  return key.bytes;
}
async function unmarshalPrivateKey2(buf) {
  const decoded = PrivateKey.decode(buf);
  const data = decoded.Data ?? new Uint8Array();
  switch (decoded.Type) {
    case KeyType.RSA:
      return supportedKeys.rsa.unmarshalRsaPrivateKey(data);
    case KeyType.Ed25519:
      return supportedKeys.ed25519.unmarshalEd25519PrivateKey(data);
    case KeyType.Secp256k1:
      return supportedKeys.secp256k1.unmarshalSecp256k1PrivateKey(data);
    default:
      throw unsupportedKey(decoded.Type ?? "RSA");
  }
}
function marshalPrivateKey(key, type) {
  type = (type ?? "rsa").toLowerCase();
  typeToKey(type);
  return key.bytes;
}

// ../interface/dist/src/peer-id/index.js
var symbol = Symbol.for("@libp2p/peer-id");

// ../../node_modules/multiformats/src/bases/identity.js
var identity_exports4 = {};
__export(identity_exports4, {
  identity: () => identity4
});
var identity4 = from3({
  prefix: "\0",
  name: "identity",
  encode: (buf) => toString2(buf),
  decode: (str) => fromString3(str)
});

// ../../node_modules/multiformats/src/bases/base2.js
var base2_exports2 = {};
__export(base2_exports2, {
  base2: () => base22
});
var base22 = rfc46482({
  prefix: "0",
  name: "base2",
  alphabet: "01",
  bitsPerChar: 1
});

// ../../node_modules/multiformats/src/bases/base8.js
var base8_exports2 = {};
__export(base8_exports2, {
  base8: () => base82
});
var base82 = rfc46482({
  prefix: "7",
  name: "base8",
  alphabet: "01234567",
  bitsPerChar: 3
});

// ../../node_modules/multiformats/src/bases/base10.js
var base10_exports2 = {};
__export(base10_exports2, {
  base10: () => base102
});
var base102 = baseX2({
  prefix: "9",
  name: "base10",
  alphabet: "0123456789"
});

// ../../node_modules/multiformats/src/bases/base16.js
var base16_exports2 = {};
__export(base16_exports2, {
  base16: () => base162,
  base16upper: () => base16upper2
});
var base162 = rfc46482({
  prefix: "f",
  name: "base16",
  alphabet: "0123456789abcdef",
  bitsPerChar: 4
});
var base16upper2 = rfc46482({
  prefix: "F",
  name: "base16upper",
  alphabet: "0123456789ABCDEF",
  bitsPerChar: 4
});

// ../../node_modules/multiformats/src/bases/base32.js
var base32_exports2 = {};
__export(base32_exports2, {
  base32: () => base322,
  base32hex: () => base32hex2,
  base32hexpad: () => base32hexpad2,
  base32hexpadupper: () => base32hexpadupper2,
  base32hexupper: () => base32hexupper2,
  base32pad: () => base32pad2,
  base32padupper: () => base32padupper2,
  base32upper: () => base32upper2,
  base32z: () => base32z2
});
var base322 = rfc46482({
  prefix: "b",
  name: "base32",
  alphabet: "abcdefghijklmnopqrstuvwxyz234567",
  bitsPerChar: 5
});
var base32upper2 = rfc46482({
  prefix: "B",
  name: "base32upper",
  alphabet: "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567",
  bitsPerChar: 5
});
var base32pad2 = rfc46482({
  prefix: "c",
  name: "base32pad",
  alphabet: "abcdefghijklmnopqrstuvwxyz234567=",
  bitsPerChar: 5
});
var base32padupper2 = rfc46482({
  prefix: "C",
  name: "base32padupper",
  alphabet: "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567=",
  bitsPerChar: 5
});
var base32hex2 = rfc46482({
  prefix: "v",
  name: "base32hex",
  alphabet: "0123456789abcdefghijklmnopqrstuv",
  bitsPerChar: 5
});
var base32hexupper2 = rfc46482({
  prefix: "V",
  name: "base32hexupper",
  alphabet: "0123456789ABCDEFGHIJKLMNOPQRSTUV",
  bitsPerChar: 5
});
var base32hexpad2 = rfc46482({
  prefix: "t",
  name: "base32hexpad",
  alphabet: "0123456789abcdefghijklmnopqrstuv=",
  bitsPerChar: 5
});
var base32hexpadupper2 = rfc46482({
  prefix: "T",
  name: "base32hexpadupper",
  alphabet: "0123456789ABCDEFGHIJKLMNOPQRSTUV=",
  bitsPerChar: 5
});
var base32z2 = rfc46482({
  prefix: "h",
  name: "base32z",
  alphabet: "ybndrfg8ejkmcpqxot1uwisza345h769",
  bitsPerChar: 5
});

// ../../node_modules/multiformats/src/bases/base36.js
var base36_exports2 = {};
__export(base36_exports2, {
  base36: () => base362,
  base36upper: () => base36upper2
});
var base362 = baseX2({
  prefix: "k",
  name: "base36",
  alphabet: "0123456789abcdefghijklmnopqrstuvwxyz"
});
var base36upper2 = baseX2({
  prefix: "K",
  name: "base36upper",
  alphabet: "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ"
});

// ../../node_modules/multiformats/src/bases/base256emoji.js
var base256emoji_exports2 = {};
__export(base256emoji_exports2, {
  base256emoji: () => base256emoji2
});
var alphabet2 = Array.from("\u{1F680}\u{1FA90}\u2604\u{1F6F0}\u{1F30C}\u{1F311}\u{1F312}\u{1F313}\u{1F314}\u{1F315}\u{1F316}\u{1F317}\u{1F318}\u{1F30D}\u{1F30F}\u{1F30E}\u{1F409}\u2600\u{1F4BB}\u{1F5A5}\u{1F4BE}\u{1F4BF}\u{1F602}\u2764\u{1F60D}\u{1F923}\u{1F60A}\u{1F64F}\u{1F495}\u{1F62D}\u{1F618}\u{1F44D}\u{1F605}\u{1F44F}\u{1F601}\u{1F525}\u{1F970}\u{1F494}\u{1F496}\u{1F499}\u{1F622}\u{1F914}\u{1F606}\u{1F644}\u{1F4AA}\u{1F609}\u263A\u{1F44C}\u{1F917}\u{1F49C}\u{1F614}\u{1F60E}\u{1F607}\u{1F339}\u{1F926}\u{1F389}\u{1F49E}\u270C\u2728\u{1F937}\u{1F631}\u{1F60C}\u{1F338}\u{1F64C}\u{1F60B}\u{1F497}\u{1F49A}\u{1F60F}\u{1F49B}\u{1F642}\u{1F493}\u{1F929}\u{1F604}\u{1F600}\u{1F5A4}\u{1F603}\u{1F4AF}\u{1F648}\u{1F447}\u{1F3B6}\u{1F612}\u{1F92D}\u2763\u{1F61C}\u{1F48B}\u{1F440}\u{1F62A}\u{1F611}\u{1F4A5}\u{1F64B}\u{1F61E}\u{1F629}\u{1F621}\u{1F92A}\u{1F44A}\u{1F973}\u{1F625}\u{1F924}\u{1F449}\u{1F483}\u{1F633}\u270B\u{1F61A}\u{1F61D}\u{1F634}\u{1F31F}\u{1F62C}\u{1F643}\u{1F340}\u{1F337}\u{1F63B}\u{1F613}\u2B50\u2705\u{1F97A}\u{1F308}\u{1F608}\u{1F918}\u{1F4A6}\u2714\u{1F623}\u{1F3C3}\u{1F490}\u2639\u{1F38A}\u{1F498}\u{1F620}\u261D\u{1F615}\u{1F33A}\u{1F382}\u{1F33B}\u{1F610}\u{1F595}\u{1F49D}\u{1F64A}\u{1F639}\u{1F5E3}\u{1F4AB}\u{1F480}\u{1F451}\u{1F3B5}\u{1F91E}\u{1F61B}\u{1F534}\u{1F624}\u{1F33C}\u{1F62B}\u26BD\u{1F919}\u2615\u{1F3C6}\u{1F92B}\u{1F448}\u{1F62E}\u{1F646}\u{1F37B}\u{1F343}\u{1F436}\u{1F481}\u{1F632}\u{1F33F}\u{1F9E1}\u{1F381}\u26A1\u{1F31E}\u{1F388}\u274C\u270A\u{1F44B}\u{1F630}\u{1F928}\u{1F636}\u{1F91D}\u{1F6B6}\u{1F4B0}\u{1F353}\u{1F4A2}\u{1F91F}\u{1F641}\u{1F6A8}\u{1F4A8}\u{1F92C}\u2708\u{1F380}\u{1F37A}\u{1F913}\u{1F619}\u{1F49F}\u{1F331}\u{1F616}\u{1F476}\u{1F974}\u25B6\u27A1\u2753\u{1F48E}\u{1F4B8}\u2B07\u{1F628}\u{1F31A}\u{1F98B}\u{1F637}\u{1F57A}\u26A0\u{1F645}\u{1F61F}\u{1F635}\u{1F44E}\u{1F932}\u{1F920}\u{1F927}\u{1F4CC}\u{1F535}\u{1F485}\u{1F9D0}\u{1F43E}\u{1F352}\u{1F617}\u{1F911}\u{1F30A}\u{1F92F}\u{1F437}\u260E\u{1F4A7}\u{1F62F}\u{1F486}\u{1F446}\u{1F3A4}\u{1F647}\u{1F351}\u2744\u{1F334}\u{1F4A3}\u{1F438}\u{1F48C}\u{1F4CD}\u{1F940}\u{1F922}\u{1F445}\u{1F4A1}\u{1F4A9}\u{1F450}\u{1F4F8}\u{1F47B}\u{1F910}\u{1F92E}\u{1F3BC}\u{1F975}\u{1F6A9}\u{1F34E}\u{1F34A}\u{1F47C}\u{1F48D}\u{1F4E3}\u{1F942}");
var alphabetBytesToChars2 = (
  /** @type {string[]} */
  alphabet2.reduce(
    (p, c, i) => {
      p[i] = c;
      return p;
    },
    /** @type {string[]} */
    []
  )
);
var alphabetCharsToBytes2 = (
  /** @type {number[]} */
  alphabet2.reduce(
    (p, c, i) => {
      p[
        /** @type {number} */
        c.codePointAt(0)
      ] = i;
      return p;
    },
    /** @type {number[]} */
    []
  )
);
function encode8(data) {
  return data.reduce((p, c) => {
    p += alphabetBytesToChars2[c];
    return p;
  }, "");
}
function decode10(str) {
  const byts = [];
  for (const char of str) {
    const byt = alphabetCharsToBytes2[
      /** @type {number} */
      char.codePointAt(0)
    ];
    if (byt === void 0) {
      throw new Error(`Non-base256emoji character: ${char}`);
    }
    byts.push(byt);
  }
  return new Uint8Array(byts);
}
var base256emoji2 = from3({
  prefix: "\u{1F680}",
  name: "base256emoji",
  encode: encode8,
  decode: decode10
});

// ../../node_modules/multiformats/src/codecs/json.js
var textEncoder2 = new TextEncoder();
var textDecoder2 = new TextDecoder();

// ../../node_modules/multiformats/src/cid.js
var format2 = (link, base4) => {
  const { bytes, version } = link;
  switch (version) {
    case 0:
      return toStringV02(
        bytes,
        baseCache2(link),
        /** @type {API.MultibaseEncoder<"z">} */
        base4 || base58btc2.encoder
      );
    default:
      return toStringV12(
        bytes,
        baseCache2(link),
        /** @type {API.MultibaseEncoder<Prefix>} */
        base4 || base322.encoder
      );
  }
};
var cache2 = /* @__PURE__ */ new WeakMap();
var baseCache2 = (cid) => {
  const baseCache3 = cache2.get(cid);
  if (baseCache3 == null) {
    const baseCache4 = /* @__PURE__ */ new Map();
    cache2.set(cid, baseCache4);
    return baseCache4;
  }
  return baseCache3;
};
var CID2 = class _CID {
  /**
   * @param {Version} version - Version of the CID
   * @param {Format} code - Code of the codec content is encoded in, see https://github.com/multiformats/multicodec/blob/master/table.csv
   * @param {API.MultihashDigest<Alg>} multihash - (Multi)hash of the of the content.
   * @param {Uint8Array} bytes
   *
   */
  constructor(version, code3, multihash, bytes) {
    this.code = code3;
    this.version = version;
    this.multihash = multihash;
    this.bytes = bytes;
    this["/"] = bytes;
  }
  /**
   * Signalling `cid.asCID === cid` has been replaced with `cid['/'] === cid.bytes`
   * please either use `CID.asCID(cid)` or switch to new signalling mechanism
   *
   * @deprecated
   */
  get asCID() {
    return this;
  }
  // ArrayBufferView
  get byteOffset() {
    return this.bytes.byteOffset;
  }
  // ArrayBufferView
  get byteLength() {
    return this.bytes.byteLength;
  }
  /**
   * @returns {CID<Data, API.DAG_PB, API.SHA_256, 0>}
   */
  toV0() {
    switch (this.version) {
      case 0: {
        return (
          /** @type {CID<Data, API.DAG_PB, API.SHA_256, 0>} */
          this
        );
      }
      case 1: {
        const { code: code3, multihash } = this;
        if (code3 !== DAG_PB_CODE2) {
          throw new Error("Cannot convert a non dag-pb CID to CIDv0");
        }
        if (multihash.code !== SHA_256_CODE2) {
          throw new Error("Cannot convert non sha2-256 multihash CID to CIDv0");
        }
        return (
          /** @type {CID<Data, API.DAG_PB, API.SHA_256, 0>} */
          _CID.createV0(
            /** @type {API.MultihashDigest<API.SHA_256>} */
            multihash
          )
        );
      }
      default: {
        throw Error(
          `Can not convert CID version ${this.version} to version 0. This is a bug please report`
        );
      }
    }
  }
  /**
   * @returns {CID<Data, Format, Alg, 1>}
   */
  toV1() {
    switch (this.version) {
      case 0: {
        const { code: code3, digest: digest3 } = this.multihash;
        const multihash = create2(code3, digest3);
        return (
          /** @type {CID<Data, Format, Alg, 1>} */
          _CID.createV1(this.code, multihash)
        );
      }
      case 1: {
        return (
          /** @type {CID<Data, Format, Alg, 1>} */
          this
        );
      }
      default: {
        throw Error(
          `Can not convert CID version ${this.version} to version 1. This is a bug please report`
        );
      }
    }
  }
  /**
   * @param {unknown} other
   * @returns {other is CID<Data, Format, Alg, Version>}
   */
  equals(other) {
    return _CID.equals(this, other);
  }
  /**
   * @template {unknown} Data
   * @template {number} Format
   * @template {number} Alg
   * @template {API.Version} Version
   * @param {API.Link<Data, Format, Alg, Version>} self
   * @param {unknown} other
   * @returns {other is CID}
   */
  static equals(self2, other) {
    const unknown = (
      /** @type {{code?:unknown, version?:unknown, multihash?:unknown}} */
      other
    );
    return unknown && self2.code === unknown.code && self2.version === unknown.version && equals4(self2.multihash, unknown.multihash);
  }
  /**
   * @param {API.MultibaseEncoder<string>} [base]
   * @returns {string}
   */
  toString(base4) {
    return format2(this, base4);
  }
  toJSON() {
    return { "/": format2(this) };
  }
  link() {
    return this;
  }
  get [Symbol.toStringTag]() {
    return "CID";
  }
  // Legacy
  [Symbol.for("nodejs.util.inspect.custom")]() {
    return `CID(${this.toString()})`;
  }
  /**
   * Takes any input `value` and returns a `CID` instance if it was
   * a `CID` otherwise returns `null`. If `value` is instanceof `CID`
   * it will return value back. If `value` is not instance of this CID
   * class, but is compatible CID it will return new instance of this
   * `CID` class. Otherwise returns null.
   *
   * This allows two different incompatible versions of CID library to
   * co-exist and interop as long as binary interface is compatible.
   *
   * @template {unknown} Data
   * @template {number} Format
   * @template {number} Alg
   * @template {API.Version} Version
   * @template {unknown} U
   * @param {API.Link<Data, Format, Alg, Version>|U} input
   * @returns {CID<Data, Format, Alg, Version>|null}
   */
  static asCID(input) {
    if (input == null) {
      return null;
    }
    const value = (
      /** @type {any} */
      input
    );
    if (value instanceof _CID) {
      return value;
    } else if (value["/"] != null && value["/"] === value.bytes || value.asCID === value) {
      const { version, code: code3, multihash, bytes } = value;
      return new _CID(
        version,
        code3,
        /** @type {API.MultihashDigest<Alg>} */
        multihash,
        bytes || encodeCID2(version, code3, multihash.bytes)
      );
    } else if (value[cidSymbol2] === true) {
      const { version, multihash, code: code3 } = value;
      const digest3 = (
        /** @type {API.MultihashDigest<Alg>} */
        decode9(multihash)
      );
      return _CID.create(version, code3, digest3);
    } else {
      return null;
    }
  }
  /**
   *
   * @template {unknown} Data
   * @template {number} Format
   * @template {number} Alg
   * @template {API.Version} Version
   * @param {Version} version - Version of the CID
   * @param {Format} code - Code of the codec content is encoded in, see https://github.com/multiformats/multicodec/blob/master/table.csv
   * @param {API.MultihashDigest<Alg>} digest - (Multi)hash of the of the content.
   * @returns {CID<Data, Format, Alg, Version>}
   */
  static create(version, code3, digest3) {
    if (typeof code3 !== "number") {
      throw new Error("String codecs are no longer supported");
    }
    if (!(digest3.bytes instanceof Uint8Array)) {
      throw new Error("Invalid digest");
    }
    switch (version) {
      case 0: {
        if (code3 !== DAG_PB_CODE2) {
          throw new Error(
            `Version 0 CID must use dag-pb (code: ${DAG_PB_CODE2}) block encoding`
          );
        } else {
          return new _CID(version, code3, digest3, digest3.bytes);
        }
      }
      case 1: {
        const bytes = encodeCID2(version, code3, digest3.bytes);
        return new _CID(version, code3, digest3, bytes);
      }
      default: {
        throw new Error("Invalid version");
      }
    }
  }
  /**
   * Simplified version of `create` for CIDv0.
   *
   * @template {unknown} [T=unknown]
   * @param {API.MultihashDigest<typeof SHA_256_CODE>} digest - Multihash.
   * @returns {CID<T, typeof DAG_PB_CODE, typeof SHA_256_CODE, 0>}
   */
  static createV0(digest3) {
    return _CID.create(0, DAG_PB_CODE2, digest3);
  }
  /**
   * Simplified version of `create` for CIDv1.
   *
   * @template {unknown} Data
   * @template {number} Code
   * @template {number} Alg
   * @param {Code} code - Content encoding format code.
   * @param {API.MultihashDigest<Alg>} digest - Miltihash of the content.
   * @returns {CID<Data, Code, Alg, 1>}
   */
  static createV1(code3, digest3) {
    return _CID.create(1, code3, digest3);
  }
  /**
   * Decoded a CID from its binary representation. The byte array must contain
   * only the CID with no additional bytes.
   *
   * An error will be thrown if the bytes provided do not contain a valid
   * binary representation of a CID.
   *
   * @template {unknown} Data
   * @template {number} Code
   * @template {number} Alg
   * @template {API.Version} Ver
   * @param {API.ByteView<API.Link<Data, Code, Alg, Ver>>} bytes
   * @returns {CID<Data, Code, Alg, Ver>}
   */
  static decode(bytes) {
    const [cid, remainder] = _CID.decodeFirst(bytes);
    if (remainder.length) {
      throw new Error("Incorrect length");
    }
    return cid;
  }
  /**
   * Decoded a CID from its binary representation at the beginning of a byte
   * array.
   *
   * Returns an array with the first element containing the CID and the second
   * element containing the remainder of the original byte array. The remainder
   * will be a zero-length byte array if the provided bytes only contained a
   * binary CID representation.
   *
   * @template {unknown} T
   * @template {number} C
   * @template {number} A
   * @template {API.Version} V
   * @param {API.ByteView<API.Link<T, C, A, V>>} bytes
   * @returns {[CID<T, C, A, V>, Uint8Array]}
   */
  static decodeFirst(bytes) {
    const specs = _CID.inspectBytes(bytes);
    const prefixSize = specs.size - specs.multihashSize;
    const multihashBytes = coerce2(
      bytes.subarray(prefixSize, prefixSize + specs.multihashSize)
    );
    if (multihashBytes.byteLength !== specs.multihashSize) {
      throw new Error("Incorrect length");
    }
    const digestBytes = multihashBytes.subarray(
      specs.multihashSize - specs.digestSize
    );
    const digest3 = new Digest2(
      specs.multihashCode,
      specs.digestSize,
      digestBytes,
      multihashBytes
    );
    const cid = specs.version === 0 ? _CID.createV0(
      /** @type {API.MultihashDigest<API.SHA_256>} */
      digest3
    ) : _CID.createV1(specs.codec, digest3);
    return [
      /** @type {CID<T, C, A, V>} */
      cid,
      bytes.subarray(specs.size)
    ];
  }
  /**
   * Inspect the initial bytes of a CID to determine its properties.
   *
   * Involves decoding up to 4 varints. Typically this will require only 4 to 6
   * bytes but for larger multicodec code values and larger multihash digest
   * lengths these varints can be quite large. It is recommended that at least
   * 10 bytes be made available in the `initialBytes` argument for a complete
   * inspection.
   *
   * @template {unknown} T
   * @template {number} C
   * @template {number} A
   * @template {API.Version} V
   * @param {API.ByteView<API.Link<T, C, A, V>>} initialBytes
   * @returns {{ version:V, codec:C, multihashCode:A, digestSize:number, multihashSize:number, size:number }}
   */
  static inspectBytes(initialBytes) {
    let offset = 0;
    const next = () => {
      const [i, length3] = decode8(initialBytes.subarray(offset));
      offset += length3;
      return i;
    };
    let version = (
      /** @type {V} */
      next()
    );
    let codec = (
      /** @type {C} */
      DAG_PB_CODE2
    );
    if (
      /** @type {number} */
      version === 18
    ) {
      version = /** @type {V} */
      0;
      offset = 0;
    } else {
      codec = /** @type {C} */
      next();
    }
    if (version !== 0 && version !== 1) {
      throw new RangeError(`Invalid CID version ${version}`);
    }
    const prefixSize = offset;
    const multihashCode = (
      /** @type {A} */
      next()
    );
    const digestSize = next();
    const size = offset + digestSize;
    const multihashSize = size - prefixSize;
    return { version, codec, multihashCode, digestSize, multihashSize, size };
  }
  /**
   * Takes cid in a string representation and creates an instance. If `base`
   * decoder is not provided will use a default from the configuration. It will
   * throw an error if encoding of the CID is not compatible with supplied (or
   * a default decoder).
   *
   * @template {string} Prefix
   * @template {unknown} Data
   * @template {number} Code
   * @template {number} Alg
   * @template {API.Version} Ver
   * @param {API.ToString<API.Link<Data, Code, Alg, Ver>, Prefix>} source
   * @param {API.MultibaseDecoder<Prefix>} [base]
   * @returns {CID<Data, Code, Alg, Ver>}
   */
  static parse(source, base4) {
    const [prefix, bytes] = parseCIDtoBytes2(source, base4);
    const cid = _CID.decode(bytes);
    if (cid.version === 0 && source[0] !== "Q") {
      throw Error("Version 0 CID string must not include multibase prefix");
    }
    baseCache2(cid).set(prefix, source);
    return cid;
  }
};
var parseCIDtoBytes2 = (source, base4) => {
  switch (source[0]) {
    case "Q": {
      const decoder = base4 || base58btc2;
      return [
        /** @type {Prefix} */
        base58btc2.prefix,
        decoder.decode(`${base58btc2.prefix}${source}`)
      ];
    }
    case base58btc2.prefix: {
      const decoder = base4 || base58btc2;
      return [
        /** @type {Prefix} */
        base58btc2.prefix,
        decoder.decode(source)
      ];
    }
    case base322.prefix: {
      const decoder = base4 || base322;
      return [
        /** @type {Prefix} */
        base322.prefix,
        decoder.decode(source)
      ];
    }
    default: {
      if (base4 == null) {
        throw Error(
          "To parse non base32 or base58btc encoded CID multibase decoder must be provided"
        );
      }
      return [
        /** @type {Prefix} */
        source[0],
        base4.decode(source)
      ];
    }
  }
};
var toStringV02 = (bytes, cache3, base4) => {
  const { prefix } = base4;
  if (prefix !== base58btc2.prefix) {
    throw Error(`Cannot string encode V0 in ${base4.name} encoding`);
  }
  const cid = cache3.get(prefix);
  if (cid == null) {
    const cid2 = base4.encode(bytes).slice(1);
    cache3.set(prefix, cid2);
    return cid2;
  } else {
    return cid;
  }
};
var toStringV12 = (bytes, cache3, base4) => {
  const { prefix } = base4;
  const cid = cache3.get(prefix);
  if (cid == null) {
    const cid2 = base4.encode(bytes);
    cache3.set(prefix, cid2);
    return cid2;
  } else {
    return cid;
  }
};
var DAG_PB_CODE2 = 112;
var SHA_256_CODE2 = 18;
var encodeCID2 = (version, code3, multihash) => {
  const codeOffset = encodingLength2(version);
  const hashOffset = codeOffset + encodingLength2(code3);
  const bytes = new Uint8Array(hashOffset + multihash.byteLength);
  encodeTo2(version, bytes, 0);
  encodeTo2(code3, bytes, codeOffset);
  bytes.set(multihash, hashOffset);
  return bytes;
};
var cidSymbol2 = Symbol.for("@ipld/js-cid/CID");

// ../../node_modules/multiformats/src/basics.js
var bases2 = { ...identity_exports4, ...base2_exports2, ...base8_exports2, ...base10_exports2, ...base16_exports2, ...base32_exports2, ...base36_exports2, ...base58_exports2, ...base64_exports2, ...base256emoji_exports2 };
var hashes2 = { ...sha2_browser_exports2, ...identity_exports3 };

// ../peer-id/dist/src/index.js
var inspect = Symbol.for("nodejs.util.inspect.custom");
var baseDecoder = Object.values(bases2).map((codec) => codec.decoder).reduce((acc, curr) => acc.or(curr), bases2.identity.decoder);
var LIBP2P_KEY_CODE = 114;
var MARSHALLED_ED225519_PUBLIC_KEY_LENGTH = 36;
var MARSHALLED_SECP256K1_PUBLIC_KEY_LENGTH = 37;
var PeerIdImpl = class {
  type;
  multihash;
  privateKey;
  publicKey;
  string;
  constructor(init) {
    this.type = init.type;
    this.multihash = init.multihash;
    this.privateKey = init.privateKey;
    Object.defineProperty(this, "string", {
      enumerable: false,
      writable: true
    });
  }
  get [Symbol.toStringTag]() {
    return `PeerId(${this.toString()})`;
  }
  [symbol] = true;
  toString() {
    if (this.string == null) {
      this.string = base58btc2.encode(this.multihash.bytes).slice(1);
    }
    return this.string;
  }
  // return self-describing String representation
  // in default format from RFC 0001: https://github.com/libp2p/specs/pull/209
  toCID() {
    return CID2.createV1(LIBP2P_KEY_CODE, this.multihash);
  }
  toBytes() {
    return this.multihash.bytes;
  }
  /**
   * Returns Multiaddr as a JSON string
   */
  toJSON() {
    return this.toString();
  }
  /**
   * Checks the equality of `this` peer against a given PeerId
   */
  equals(id) {
    if (id instanceof Uint8Array) {
      return equals5(this.multihash.bytes, id);
    } else if (typeof id === "string") {
      return peerIdFromString(id).equals(this);
    } else if (id?.multihash?.bytes != null) {
      return equals5(this.multihash.bytes, id.multihash.bytes);
    } else {
      throw new Error("not valid Id");
    }
  }
  /**
   * Returns PeerId as a human-readable string
   * https://nodejs.org/api/util.html#utilinspectcustom
   *
   * @example
   * ```js
   * import { peerIdFromString } from '@libp2p/peer-id'
   *
   * console.info(peerIdFromString('QmFoo'))
   * // 'PeerId(QmFoo)'
   * ```
   */
  [inspect]() {
    return `PeerId(${this.toString()})`;
  }
};
var RSAPeerIdImpl = class extends PeerIdImpl {
  type = "RSA";
  publicKey;
  constructor(init) {
    super({ ...init, type: "RSA" });
    this.publicKey = init.publicKey;
  }
};
var Ed25519PeerIdImpl = class extends PeerIdImpl {
  type = "Ed25519";
  publicKey;
  constructor(init) {
    super({ ...init, type: "Ed25519" });
    this.publicKey = init.multihash.digest;
  }
};
var Secp256k1PeerIdImpl = class extends PeerIdImpl {
  type = "secp256k1";
  publicKey;
  constructor(init) {
    super({ ...init, type: "secp256k1" });
    this.publicKey = init.multihash.digest;
  }
};
function peerIdFromString(str, decoder) {
  decoder = decoder ?? baseDecoder;
  if (str.charAt(0) === "1" || str.charAt(0) === "Q") {
    const multihash = decode9(base58btc2.decode(`z${str}`));
    if (str.startsWith("12D")) {
      return new Ed25519PeerIdImpl({ multihash });
    } else if (str.startsWith("16U")) {
      return new Secp256k1PeerIdImpl({ multihash });
    } else {
      return new RSAPeerIdImpl({ multihash });
    }
  }
  return peerIdFromBytes(baseDecoder.decode(str));
}
function peerIdFromBytes(buf) {
  try {
    const multihash = decode9(buf);
    if (multihash.code === identity3.code) {
      if (multihash.digest.length === MARSHALLED_ED225519_PUBLIC_KEY_LENGTH) {
        return new Ed25519PeerIdImpl({ multihash });
      } else if (multihash.digest.length === MARSHALLED_SECP256K1_PUBLIC_KEY_LENGTH) {
        return new Secp256k1PeerIdImpl({ multihash });
      }
    }
    if (multihash.code === sha2562.code) {
      return new RSAPeerIdImpl({ multihash });
    }
  } catch {
    return peerIdFromCID(CID2.decode(buf));
  }
  throw new Error("Supplied PeerID CID is invalid");
}
function peerIdFromCID(cid) {
  if (cid == null || cid.multihash == null || cid.version == null || cid.version === 1 && cid.code !== LIBP2P_KEY_CODE) {
    throw new Error("Supplied PeerID CID is invalid");
  }
  const multihash = cid.multihash;
  if (multihash.code === sha2562.code) {
    return new RSAPeerIdImpl({ multihash: cid.multihash });
  } else if (multihash.code === identity3.code) {
    if (multihash.digest.length === MARSHALLED_ED225519_PUBLIC_KEY_LENGTH) {
      return new Ed25519PeerIdImpl({ multihash: cid.multihash });
    } else if (multihash.digest.length === MARSHALLED_SECP256K1_PUBLIC_KEY_LENGTH) {
      return new Secp256k1PeerIdImpl({ multihash: cid.multihash });
    }
  }
  throw new Error("Supplied PeerID CID is invalid");
}
async function peerIdFromKeys(publicKey, privateKey) {
  if (publicKey.length === MARSHALLED_ED225519_PUBLIC_KEY_LENGTH) {
    return new Ed25519PeerIdImpl({ multihash: create2(identity3.code, publicKey), privateKey });
  }
  if (publicKey.length === MARSHALLED_SECP256K1_PUBLIC_KEY_LENGTH) {
    return new Secp256k1PeerIdImpl({ multihash: create2(identity3.code, publicKey), privateKey });
  }
  return new RSAPeerIdImpl({ multihash: await sha2562.digest(publicKey), publicKey, privateKey });
}

// src/proto.ts
var PeerIdProto;
((PeerIdProto2) => {
  let _codec;
  PeerIdProto2.codec = () => {
    if (_codec == null) {
      _codec = message((obj, w, opts = {}) => {
        if (opts.lengthDelimited !== false) {
          w.fork();
        }
        if (obj.id != null) {
          w.uint32(10);
          w.bytes(obj.id);
        }
        if (obj.pubKey != null) {
          w.uint32(18);
          w.bytes(obj.pubKey);
        }
        if (obj.privKey != null) {
          w.uint32(26);
          w.bytes(obj.privKey);
        }
        if (opts.lengthDelimited !== false) {
          w.ldelim();
        }
      }, (reader2, length3) => {
        const obj = {};
        const end = length3 == null ? reader2.len : reader2.pos + length3;
        while (reader2.pos < end) {
          const tag = reader2.uint32();
          switch (tag >>> 3) {
            case 1:
              obj.id = reader2.bytes();
              break;
            case 2:
              obj.pubKey = reader2.bytes();
              break;
            case 3:
              obj.privKey = reader2.bytes();
              break;
            default:
              reader2.skipType(tag & 7);
              break;
          }
        }
        return obj;
      });
    }
    return _codec;
  };
  PeerIdProto2.encode = (obj) => {
    return encodeMessage(obj, PeerIdProto2.codec());
  };
  PeerIdProto2.decode = (buf) => {
    return decodeMessage(buf, PeerIdProto2.codec());
  };
})(PeerIdProto || (PeerIdProto = {}));

// src/index.ts
var createEd25519PeerId = async () => {
  const key = await generateKeyPair4("Ed25519");
  const id = await createFromPrivKey(key);
  if (id.type === "Ed25519") {
    return id;
  }
  throw new Error(`Generated unexpected PeerId type "${id.type}"`);
};
var createSecp256k1PeerId = async () => {
  const key = await generateKeyPair4("secp256k1");
  const id = await createFromPrivKey(key);
  if (id.type === "secp256k1") {
    return id;
  }
  throw new Error(`Generated unexpected PeerId type "${id.type}"`);
};
var createRSAPeerId = async (opts) => {
  const key = await generateKeyPair4("RSA", opts?.bits ?? 2048);
  const id = await createFromPrivKey(key);
  if (id.type === "RSA") {
    return id;
  }
  throw new Error(`Generated unexpected PeerId type "${id.type}"`);
};
async function createFromPubKey(publicKey) {
  return peerIdFromKeys(marshalPublicKey(publicKey));
}
async function createFromPrivKey(privateKey) {
  return peerIdFromKeys(marshalPublicKey(privateKey.public), marshalPrivateKey(privateKey));
}
function exportToProtobuf(peerId, excludePrivateKey) {
  return PeerIdProto.encode({
    id: peerId.multihash.bytes,
    pubKey: peerId.publicKey,
    privKey: excludePrivateKey === true || peerId.privateKey == null ? void 0 : peerId.privateKey
  });
}
async function createFromProtobuf(buf) {
  const {
    id,
    privKey,
    pubKey
  } = PeerIdProto.decode(buf);
  return createFromParts(
    id ?? new Uint8Array(0),
    privKey,
    pubKey
  );
}
async function createFromJSON(obj) {
  return createFromParts(
    fromString2(obj.id, "base58btc"),
    obj.privKey != null ? fromString2(obj.privKey, "base64pad") : void 0,
    obj.pubKey != null ? fromString2(obj.pubKey, "base64pad") : void 0
  );
}
async function createFromParts(multihash, privKey, pubKey) {
  if (privKey != null) {
    const key = await unmarshalPrivateKey2(privKey);
    return createFromPrivKey(key);
  } else if (pubKey != null) {
    const key = unmarshalPublicKey(pubKey);
    return createFromPubKey(key);
  }
  return peerIdFromBytes(multihash);
}
export {
  createEd25519PeerId,
  createFromJSON,
  createFromPrivKey,
  createFromProtobuf,
  createFromPubKey,
  createRSAPeerId,
  createSecp256k1PeerId,
  exportToProtobuf
};
/*! Bundled license information:

@noble/ed25519/lib/esm/index.js:
  (*! noble-ed25519 - MIT License (c) 2019 Paul Miller (paulmillr.com) *)

@noble/secp256k1/lib/esm/index.js:
  (*! noble-secp256k1 - MIT License (c) 2019 Paul Miller (paulmillr.com) *)
*/
