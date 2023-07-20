var __create = Object.create;
var __defProp = Object.defineProperty;
var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __getProtoOf = Object.getPrototypeOf;
var __hasOwnProp = Object.prototype.hasOwnProperty;
var __require = /* @__PURE__ */ ((x) => typeof require !== "undefined" ? require : typeof Proxy !== "undefined" ? new Proxy(x, {
  get: (a, b) => (typeof require !== "undefined" ? require : a)[b]
}) : x)(function(x) {
  if (typeof require !== "undefined")
    return require.apply(this, arguments);
  throw new Error('Dynamic require of "' + x + '" is not supported');
});
var __esm = (fn, res) => function __init() {
  return fn && (res = (0, fn[__getOwnPropNames(fn)[0]])(fn = 0)), res;
};
var __commonJS = (cb, mod4) => function __require2() {
  return mod4 || (0, cb[__getOwnPropNames(cb)[0]])((mod4 = { exports: {} }).exports, mod4), mod4.exports;
};
var __export = (target, all) => {
  for (var name5 in all)
    __defProp(target, name5, { get: all[name5], enumerable: true });
};
var __copyProps = (to, from9, except, desc) => {
  if (from9 && typeof from9 === "object" || typeof from9 === "function") {
    for (let key of __getOwnPropNames(from9))
      if (!__hasOwnProp.call(to, key) && key !== except)
        __defProp(to, key, { get: () => from9[key], enumerable: !(desc = __getOwnPropDesc(from9, key)) || desc.enumerable });
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
var __toCommonJS = (mod4) => __copyProps(__defProp({}, "__esModule", { value: true }), mod4);

// ../../../node_modules/ms/index.js
var require_ms = __commonJS({
  "../../../node_modules/ms/index.js"(exports2, module2) {
    var s = 1e3;
    var m = s * 60;
    var h = m * 60;
    var d = h * 24;
    var w = d * 7;
    var y = d * 365.25;
    module2.exports = function(val, options) {
      options = options || {};
      var type = typeof val;
      if (type === "string" && val.length > 0) {
        return parse(val);
      } else if (type === "number" && isFinite(val)) {
        return options.long ? fmtLong(val) : fmtShort(val);
      }
      throw new Error(
        "val is not a non-empty string or a valid number. val=" + JSON.stringify(val)
      );
    };
    function parse(str) {
      str = String(str);
      if (str.length > 100) {
        return;
      }
      var match = /^(-?(?:\d+)?\.?\d+) *(milliseconds?|msecs?|ms|seconds?|secs?|s|minutes?|mins?|m|hours?|hrs?|h|days?|d|weeks?|w|years?|yrs?|y)?$/i.exec(
        str
      );
      if (!match) {
        return;
      }
      var n = parseFloat(match[1]);
      var type = (match[2] || "ms").toLowerCase();
      switch (type) {
        case "years":
        case "year":
        case "yrs":
        case "yr":
        case "y":
          return n * y;
        case "weeks":
        case "week":
        case "w":
          return n * w;
        case "days":
        case "day":
        case "d":
          return n * d;
        case "hours":
        case "hour":
        case "hrs":
        case "hr":
        case "h":
          return n * h;
        case "minutes":
        case "minute":
        case "mins":
        case "min":
        case "m":
          return n * m;
        case "seconds":
        case "second":
        case "secs":
        case "sec":
        case "s":
          return n * s;
        case "milliseconds":
        case "millisecond":
        case "msecs":
        case "msec":
        case "ms":
          return n;
        default:
          return void 0;
      }
    }
    function fmtShort(ms) {
      var msAbs = Math.abs(ms);
      if (msAbs >= d) {
        return Math.round(ms / d) + "d";
      }
      if (msAbs >= h) {
        return Math.round(ms / h) + "h";
      }
      if (msAbs >= m) {
        return Math.round(ms / m) + "m";
      }
      if (msAbs >= s) {
        return Math.round(ms / s) + "s";
      }
      return ms + "ms";
    }
    function fmtLong(ms) {
      var msAbs = Math.abs(ms);
      if (msAbs >= d) {
        return plural(ms, msAbs, d, "day");
      }
      if (msAbs >= h) {
        return plural(ms, msAbs, h, "hour");
      }
      if (msAbs >= m) {
        return plural(ms, msAbs, m, "minute");
      }
      if (msAbs >= s) {
        return plural(ms, msAbs, s, "second");
      }
      return ms + " ms";
    }
    function plural(ms, msAbs, n, name5) {
      var isPlural = msAbs >= n * 1.5;
      return Math.round(ms / n) + " " + name5 + (isPlural ? "s" : "");
    }
  }
});

// ../../../node_modules/debug/src/common.js
var require_common = __commonJS({
  "../../../node_modules/debug/src/common.js"(exports2, module2) {
    function setup(env) {
      createDebug.debug = createDebug;
      createDebug.default = createDebug;
      createDebug.coerce = coerce5;
      createDebug.disable = disable;
      createDebug.enable = enable;
      createDebug.enabled = enabled;
      createDebug.humanize = require_ms();
      createDebug.destroy = destroy;
      Object.keys(env).forEach((key) => {
        createDebug[key] = env[key];
      });
      createDebug.names = [];
      createDebug.skips = [];
      createDebug.formatters = {};
      function selectColor(namespace) {
        let hash2 = 0;
        for (let i = 0; i < namespace.length; i++) {
          hash2 = (hash2 << 5) - hash2 + namespace.charCodeAt(i);
          hash2 |= 0;
        }
        return createDebug.colors[Math.abs(hash2) % createDebug.colors.length];
      }
      createDebug.selectColor = selectColor;
      function createDebug(namespace) {
        let prevTime;
        let enableOverride = null;
        let namespacesCache;
        let enabledCache;
        function debug2(...args) {
          if (!debug2.enabled) {
            return;
          }
          const self2 = debug2;
          const curr = Number(/* @__PURE__ */ new Date());
          const ms = curr - (prevTime || curr);
          self2.diff = ms;
          self2.prev = prevTime;
          self2.curr = curr;
          prevTime = curr;
          args[0] = createDebug.coerce(args[0]);
          if (typeof args[0] !== "string") {
            args.unshift("%O");
          }
          let index = 0;
          args[0] = args[0].replace(/%([a-zA-Z%])/g, (match, format4) => {
            if (match === "%%") {
              return "%";
            }
            index++;
            const formatter = createDebug.formatters[format4];
            if (typeof formatter === "function") {
              const val = args[index];
              match = formatter.call(self2, val);
              args.splice(index, 1);
              index--;
            }
            return match;
          });
          createDebug.formatArgs.call(self2, args);
          const logFn = self2.log || createDebug.log;
          logFn.apply(self2, args);
        }
        debug2.namespace = namespace;
        debug2.useColors = createDebug.useColors();
        debug2.color = createDebug.selectColor(namespace);
        debug2.extend = extend;
        debug2.destroy = createDebug.destroy;
        Object.defineProperty(debug2, "enabled", {
          enumerable: true,
          configurable: false,
          get: () => {
            if (enableOverride !== null) {
              return enableOverride;
            }
            if (namespacesCache !== createDebug.namespaces) {
              namespacesCache = createDebug.namespaces;
              enabledCache = createDebug.enabled(namespace);
            }
            return enabledCache;
          },
          set: (v) => {
            enableOverride = v;
          }
        });
        if (typeof createDebug.init === "function") {
          createDebug.init(debug2);
        }
        return debug2;
      }
      function extend(namespace, delimiter) {
        const newDebug = createDebug(this.namespace + (typeof delimiter === "undefined" ? ":" : delimiter) + namespace);
        newDebug.log = this.log;
        return newDebug;
      }
      function enable(namespaces) {
        createDebug.save(namespaces);
        createDebug.namespaces = namespaces;
        createDebug.names = [];
        createDebug.skips = [];
        let i;
        const split = (typeof namespaces === "string" ? namespaces : "").split(/[\s,]+/);
        const len = split.length;
        for (i = 0; i < len; i++) {
          if (!split[i]) {
            continue;
          }
          namespaces = split[i].replace(/\*/g, ".*?");
          if (namespaces[0] === "-") {
            createDebug.skips.push(new RegExp("^" + namespaces.slice(1) + "$"));
          } else {
            createDebug.names.push(new RegExp("^" + namespaces + "$"));
          }
        }
      }
      function disable() {
        const namespaces = [
          ...createDebug.names.map(toNamespace),
          ...createDebug.skips.map(toNamespace).map((namespace) => "-" + namespace)
        ].join(",");
        createDebug.enable("");
        return namespaces;
      }
      function enabled(name5) {
        if (name5[name5.length - 1] === "*") {
          return true;
        }
        let i;
        let len;
        for (i = 0, len = createDebug.skips.length; i < len; i++) {
          if (createDebug.skips[i].test(name5)) {
            return false;
          }
        }
        for (i = 0, len = createDebug.names.length; i < len; i++) {
          if (createDebug.names[i].test(name5)) {
            return true;
          }
        }
        return false;
      }
      function toNamespace(regexp) {
        return regexp.toString().substring(2, regexp.toString().length - 2).replace(/\.\*\?$/, "*");
      }
      function coerce5(val) {
        if (val instanceof Error) {
          return val.stack || val.message;
        }
        return val;
      }
      function destroy() {
        console.warn("Instance method `debug.destroy()` is deprecated and no longer does anything. It will be removed in the next major version of `debug`.");
      }
      createDebug.enable(createDebug.load());
      return createDebug;
    }
    module2.exports = setup;
  }
});

// ../../../node_modules/debug/src/browser.js
var require_browser = __commonJS({
  "../../../node_modules/debug/src/browser.js"(exports2, module2) {
    exports2.formatArgs = formatArgs;
    exports2.save = save;
    exports2.load = load;
    exports2.useColors = useColors;
    exports2.storage = localstorage();
    exports2.destroy = (() => {
      let warned = false;
      return () => {
        if (!warned) {
          warned = true;
          console.warn("Instance method `debug.destroy()` is deprecated and no longer does anything. It will be removed in the next major version of `debug`.");
        }
      };
    })();
    exports2.colors = [
      "#0000CC",
      "#0000FF",
      "#0033CC",
      "#0033FF",
      "#0066CC",
      "#0066FF",
      "#0099CC",
      "#0099FF",
      "#00CC00",
      "#00CC33",
      "#00CC66",
      "#00CC99",
      "#00CCCC",
      "#00CCFF",
      "#3300CC",
      "#3300FF",
      "#3333CC",
      "#3333FF",
      "#3366CC",
      "#3366FF",
      "#3399CC",
      "#3399FF",
      "#33CC00",
      "#33CC33",
      "#33CC66",
      "#33CC99",
      "#33CCCC",
      "#33CCFF",
      "#6600CC",
      "#6600FF",
      "#6633CC",
      "#6633FF",
      "#66CC00",
      "#66CC33",
      "#9900CC",
      "#9900FF",
      "#9933CC",
      "#9933FF",
      "#99CC00",
      "#99CC33",
      "#CC0000",
      "#CC0033",
      "#CC0066",
      "#CC0099",
      "#CC00CC",
      "#CC00FF",
      "#CC3300",
      "#CC3333",
      "#CC3366",
      "#CC3399",
      "#CC33CC",
      "#CC33FF",
      "#CC6600",
      "#CC6633",
      "#CC9900",
      "#CC9933",
      "#CCCC00",
      "#CCCC33",
      "#FF0000",
      "#FF0033",
      "#FF0066",
      "#FF0099",
      "#FF00CC",
      "#FF00FF",
      "#FF3300",
      "#FF3333",
      "#FF3366",
      "#FF3399",
      "#FF33CC",
      "#FF33FF",
      "#FF6600",
      "#FF6633",
      "#FF9900",
      "#FF9933",
      "#FFCC00",
      "#FFCC33"
    ];
    function useColors() {
      if (typeof window !== "undefined" && window.process && (window.process.type === "renderer" || window.process.__nwjs)) {
        return true;
      }
      if (typeof navigator !== "undefined" && navigator.userAgent && navigator.userAgent.toLowerCase().match(/(edge|trident)\/(\d+)/)) {
        return false;
      }
      return typeof document !== "undefined" && document.documentElement && document.documentElement.style && document.documentElement.style.WebkitAppearance || // Is firebug? http://stackoverflow.com/a/398120/376773
      typeof window !== "undefined" && window.console && (window.console.firebug || window.console.exception && window.console.table) || // Is firefox >= v31?
      // https://developer.mozilla.org/en-US/docs/Tools/Web_Console#Styling_messages
      typeof navigator !== "undefined" && navigator.userAgent && navigator.userAgent.toLowerCase().match(/firefox\/(\d+)/) && parseInt(RegExp.$1, 10) >= 31 || // Double check webkit in userAgent just in case we are in a worker
      typeof navigator !== "undefined" && navigator.userAgent && navigator.userAgent.toLowerCase().match(/applewebkit\/(\d+)/);
    }
    function formatArgs(args) {
      args[0] = (this.useColors ? "%c" : "") + this.namespace + (this.useColors ? " %c" : " ") + args[0] + (this.useColors ? "%c " : " ") + "+" + module2.exports.humanize(this.diff);
      if (!this.useColors) {
        return;
      }
      const c = "color: " + this.color;
      args.splice(1, 0, c, "color: inherit");
      let index = 0;
      let lastC = 0;
      args[0].replace(/%[a-zA-Z%]/g, (match) => {
        if (match === "%%") {
          return;
        }
        index++;
        if (match === "%c") {
          lastC = index;
        }
      });
      args.splice(lastC, 0, c);
    }
    exports2.log = console.debug || console.log || (() => {
    });
    function save(namespaces) {
      try {
        if (namespaces) {
          exports2.storage.setItem("debug", namespaces);
        } else {
          exports2.storage.removeItem("debug");
        }
      } catch (error) {
      }
    }
    function load() {
      let r;
      try {
        r = exports2.storage.getItem("debug");
      } catch (error) {
      }
      if (!r && typeof process !== "undefined" && "env" in process) {
        r = process.env.DEBUG;
      }
      return r;
    }
    function localstorage() {
      try {
        return localStorage;
      } catch (error) {
      }
    }
    module2.exports = require_common()(exports2);
    var { formatters } = module2.exports;
    formatters.j = function(v) {
      try {
        return JSON.stringify(v);
      } catch (error) {
        return "[UnexpectedJSONParseError]: " + error.message;
      }
    };
  }
});

// ../../../node_modules/varint/encode.js
var require_encode = __commonJS({
  "../../../node_modules/varint/encode.js"(exports2, module2) {
    module2.exports = encode19;
    var MSB5 = 128;
    var REST5 = 127;
    var MSBALL5 = ~REST5;
    var INT5 = Math.pow(2, 31);
    function encode19(num, out, offset) {
      if (Number.MAX_SAFE_INTEGER && num > Number.MAX_SAFE_INTEGER) {
        encode19.bytes = 0;
        throw new RangeError("Could not encode varint");
      }
      out = out || [];
      offset = offset || 0;
      var oldOffset = offset;
      while (num >= INT5) {
        out[offset++] = num & 255 | MSB5;
        num /= 128;
      }
      while (num & MSBALL5) {
        out[offset++] = num & 255 | MSB5;
        num >>>= 7;
      }
      out[offset] = num | 0;
      encode19.bytes = offset - oldOffset + 1;
      return out;
    }
  }
});

// ../../../node_modules/varint/decode.js
var require_decode = __commonJS({
  "../../../node_modules/varint/decode.js"(exports2, module2) {
    module2.exports = read5;
    var MSB5 = 128;
    var REST5 = 127;
    function read5(buf, offset) {
      var res = 0, offset = offset || 0, shift = 0, counter = offset, b, l = buf.length;
      do {
        if (counter >= l || shift > 49) {
          read5.bytes = 0;
          throw new RangeError("Could not decode varint");
        }
        b = buf[counter++];
        res += shift < 28 ? (b & REST5) << shift : (b & REST5) * Math.pow(2, shift);
        shift += 7;
      } while (b >= MSB5);
      read5.bytes = counter - offset;
      return res;
    }
  }
});

// ../../../node_modules/varint/length.js
var require_length = __commonJS({
  "../../../node_modules/varint/length.js"(exports2, module2) {
    var N16 = Math.pow(2, 7);
    var N26 = Math.pow(2, 14);
    var N36 = Math.pow(2, 21);
    var N46 = Math.pow(2, 28);
    var N56 = Math.pow(2, 35);
    var N66 = Math.pow(2, 42);
    var N76 = Math.pow(2, 49);
    var N86 = Math.pow(2, 56);
    var N96 = Math.pow(2, 63);
    module2.exports = function(value) {
      return value < N16 ? 1 : value < N26 ? 2 : value < N36 ? 3 : value < N46 ? 4 : value < N56 ? 5 : value < N66 ? 6 : value < N76 ? 7 : value < N86 ? 8 : value < N96 ? 9 : 10;
    };
  }
});

// ../../../node_modules/varint/index.js
var require_varint = __commonJS({
  "../../../node_modules/varint/index.js"(exports2, module2) {
    module2.exports = {
      encode: require_encode(),
      decode: require_decode(),
      encodingLength: require_length()
    };
  }
});

// ../../../node_modules/err-code/index.js
var require_err_code = __commonJS({
  "../../../node_modules/err-code/index.js"(exports2, module2) {
    "use strict";
    function assign(obj, props) {
      for (const key in props) {
        Object.defineProperty(obj, key, {
          value: props[key],
          enumerable: true,
          configurable: true
        });
      }
      return obj;
    }
    function createError(err, code5, props) {
      if (!err || typeof err === "string") {
        throw new TypeError("Please pass an Error to err-code");
      }
      if (!props) {
        props = {};
      }
      if (typeof code5 === "object") {
        props = code5;
        code5 = "";
      }
      if (code5) {
        props.code = code5;
      }
      try {
        return assign(err, props);
      } catch (_) {
        props.message = err.message;
        props.stack = err.stack;
        const ErrClass = function() {
        };
        ErrClass.prototype = Object.create(Object.getPrototypeOf(err));
        const output2 = assign(new ErrClass(), props);
        return output2;
      }
    }
    module2.exports = createError;
  }
});

// ../../../node_modules/@protobufjs/aspromise/index.js
var require_aspromise = __commonJS({
  "../../../node_modules/@protobufjs/aspromise/index.js"(exports2, module2) {
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

// ../../../node_modules/@protobufjs/base64/index.js
var require_base64 = __commonJS({
  "../../../node_modules/@protobufjs/base64/index.js"(exports2) {
    "use strict";
    var base645 = exports2;
    base645.length = function length5(string3) {
      var p = string3.length;
      if (!p)
        return 0;
      var n = 0;
      while (--p % 4 > 1 && string3.charAt(p) === "=")
        ++n;
      return Math.ceil(string3.length * 3) / 4 - n;
    };
    var b64 = new Array(64);
    var s64 = new Array(123);
    for (i = 0; i < 64; )
      s64[b64[i] = i < 26 ? i + 65 : i < 52 ? i + 71 : i < 62 ? i - 4 : i - 59 | 43] = i++;
    var i;
    base645.encode = function encode19(buffer, start, end) {
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
    base645.decode = function decode24(string3, buffer, offset) {
      var start = offset;
      var j = 0, t;
      for (var i2 = 0; i2 < string3.length; ) {
        var c = string3.charCodeAt(i2++);
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
    base645.test = function test(string3) {
      return /^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(string3);
    };
  }
});

// ../../../node_modules/@protobufjs/eventemitter/index.js
var require_eventemitter = __commonJS({
  "../../../node_modules/@protobufjs/eventemitter/index.js"(exports2, module2) {
    "use strict";
    module2.exports = EventEmitter2;
    function EventEmitter2() {
      this._listeners = {};
    }
    EventEmitter2.prototype.on = function on(evt, fn, ctx) {
      (this._listeners[evt] || (this._listeners[evt] = [])).push({
        fn,
        ctx: ctx || this
      });
      return this;
    };
    EventEmitter2.prototype.off = function off(evt, fn) {
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
    EventEmitter2.prototype.emit = function emit(evt) {
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

// ../../../node_modules/@protobufjs/float/index.js
var require_float = __commonJS({
  "../../../node_modules/@protobufjs/float/index.js"(exports2, module2) {
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

// ../../../node_modules/@protobufjs/inquire/index.js
var require_inquire = __commonJS({
  "../../../node_modules/@protobufjs/inquire/index.js"(exports, module) {
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

// ../../../node_modules/@protobufjs/utf8/index.js
var require_utf8 = __commonJS({
  "../../../node_modules/@protobufjs/utf8/index.js"(exports2) {
    "use strict";
    var utf8 = exports2;
    utf8.length = function utf8_length(string3) {
      var len = 0, c = 0;
      for (var i = 0; i < string3.length; ++i) {
        c = string3.charCodeAt(i);
        if (c < 128)
          len += 1;
        else if (c < 2048)
          len += 2;
        else if ((c & 64512) === 55296 && (string3.charCodeAt(i + 1) & 64512) === 56320) {
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
    utf8.write = function utf8_write(string3, buffer, offset) {
      var start = offset, c1, c2;
      for (var i = 0; i < string3.length; ++i) {
        c1 = string3.charCodeAt(i);
        if (c1 < 128) {
          buffer[offset++] = c1;
        } else if (c1 < 2048) {
          buffer[offset++] = c1 >> 6 | 192;
          buffer[offset++] = c1 & 63 | 128;
        } else if ((c1 & 64512) === 55296 && ((c2 = string3.charCodeAt(i + 1)) & 64512) === 56320) {
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

// ../../../node_modules/@protobufjs/pool/index.js
var require_pool = __commonJS({
  "../../../node_modules/@protobufjs/pool/index.js"(exports2, module2) {
    "use strict";
    module2.exports = pool;
    function pool(alloc2, slice, size) {
      var SIZE = size || 8192;
      var MAX = SIZE >>> 1;
      var slab = null;
      var offset = SIZE;
      return function pool_alloc(size2) {
        if (size2 < 1 || size2 > MAX)
          return alloc2(size2);
        if (offset + size2 > SIZE) {
          slab = alloc2(SIZE);
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

// ../../../node_modules/protobufjs/src/util/longbits.js
var require_longbits = __commonJS({
  "../../../node_modules/protobufjs/src/util/longbits.js"(exports2, module2) {
    "use strict";
    module2.exports = LongBits2;
    var util2 = require_minimal();
    function LongBits2(lo, hi) {
      this.lo = lo >>> 0;
      this.hi = hi >>> 0;
    }
    var zero = LongBits2.zero = new LongBits2(0, 0);
    zero.toNumber = function() {
      return 0;
    };
    zero.zzEncode = zero.zzDecode = function() {
      return this;
    };
    zero.length = function() {
      return 1;
    };
    var zeroHash = LongBits2.zeroHash = "\0\0\0\0\0\0\0\0";
    LongBits2.fromNumber = function fromNumber(value) {
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
      return new LongBits2(lo, hi);
    };
    LongBits2.from = function from9(value) {
      if (typeof value === "number")
        return LongBits2.fromNumber(value);
      if (util2.isString(value)) {
        if (util2.Long)
          value = util2.Long.fromString(value);
        else
          return LongBits2.fromNumber(parseInt(value, 10));
      }
      return value.low || value.high ? new LongBits2(value.low >>> 0, value.high >>> 0) : zero;
    };
    LongBits2.prototype.toNumber = function toNumber(unsigned2) {
      if (!unsigned2 && this.hi >>> 31) {
        var lo = ~this.lo + 1 >>> 0, hi = ~this.hi >>> 0;
        if (!lo)
          hi = hi + 1 >>> 0;
        return -(lo + hi * 4294967296);
      }
      return this.lo + this.hi * 4294967296;
    };
    LongBits2.prototype.toLong = function toLong(unsigned2) {
      return util2.Long ? new util2.Long(this.lo | 0, this.hi | 0, Boolean(unsigned2)) : { low: this.lo | 0, high: this.hi | 0, unsigned: Boolean(unsigned2) };
    };
    var charCodeAt = String.prototype.charCodeAt;
    LongBits2.fromHash = function fromHash(hash2) {
      if (hash2 === zeroHash)
        return zero;
      return new LongBits2(
        (charCodeAt.call(hash2, 0) | charCodeAt.call(hash2, 1) << 8 | charCodeAt.call(hash2, 2) << 16 | charCodeAt.call(hash2, 3) << 24) >>> 0,
        (charCodeAt.call(hash2, 4) | charCodeAt.call(hash2, 5) << 8 | charCodeAt.call(hash2, 6) << 16 | charCodeAt.call(hash2, 7) << 24) >>> 0
      );
    };
    LongBits2.prototype.toHash = function toHash() {
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
    LongBits2.prototype.zzEncode = function zzEncode() {
      var mask = this.hi >> 31;
      this.hi = ((this.hi << 1 | this.lo >>> 31) ^ mask) >>> 0;
      this.lo = (this.lo << 1 ^ mask) >>> 0;
      return this;
    };
    LongBits2.prototype.zzDecode = function zzDecode() {
      var mask = -(this.lo & 1);
      this.lo = ((this.lo >>> 1 | this.hi << 31) ^ mask) >>> 0;
      this.hi = (this.hi >>> 1 ^ mask) >>> 0;
      return this;
    };
    LongBits2.prototype.length = function length5() {
      var part0 = this.lo, part1 = (this.lo >>> 28 | this.hi << 4) >>> 0, part2 = this.hi >>> 24;
      return part2 === 0 ? part1 === 0 ? part0 < 16384 ? part0 < 128 ? 1 : 2 : part0 < 2097152 ? 3 : 4 : part1 < 16384 ? part1 < 128 ? 5 : 6 : part1 < 2097152 ? 7 : 8 : part2 < 128 ? 9 : 10;
    };
  }
});

// ../../../node_modules/protobufjs/src/util/minimal.js
var require_minimal = __commonJS({
  "../../../node_modules/protobufjs/src/util/minimal.js"(exports2) {
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
    util2.longFromHash = function longFromHash(hash2, unsigned2) {
      var bits2 = util2.LongBits.fromHash(hash2);
      if (util2.Long)
        return util2.Long.fromBits(bits2.lo, bits2.hi, unsigned2);
      return bits2.toNumber(Boolean(unsigned2));
    };
    function merge2(dst, src5, ifNotSet) {
      for (var keys = Object.keys(src5), i = 0; i < keys.length; ++i)
        if (dst[keys[i]] === void 0 || !ifNotSet)
          dst[keys[i]] = src5[keys[i]];
      return dst;
    }
    util2.merge = merge2;
    util2.lcFirst = function lcFirst(str) {
      return str.charAt(0).toLowerCase() + str.substring(1);
    };
    function newError(name5) {
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
          merge2(this, properties);
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
            return name5;
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
      return function(name5) {
        for (var i = 0; i < fieldNames.length; ++i)
          if (fieldNames[i] !== name5)
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

// ../../../node_modules/protobufjs/src/reader.js
var require_reader = __commonJS({
  "../../../node_modules/protobufjs/src/reader.js"(exports2, module2) {
    "use strict";
    module2.exports = Reader;
    var util2 = require_minimal();
    var BufferReader;
    var LongBits2 = util2.LongBits;
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
    var create7 = function create8() {
      return util2.Buffer ? function create_buffer_setup(buffer) {
        return (Reader.create = function create_buffer(buffer2) {
          return util2.Buffer.isBuffer(buffer2) ? new BufferReader(buffer2) : create_array(buffer2);
        })(buffer);
      } : create_array;
    };
    Reader.create = create7();
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
      var bits2 = new LongBits2(0, 0);
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
      return new LongBits2(readFixed32_end(this.buf, this.pos += 4), readFixed32_end(this.buf, this.pos += 4));
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
      var length5 = this.uint32(), start = this.pos, end = this.pos + length5;
      if (end > this.len)
        throw indexOutOfRange(this, length5);
      this.pos += length5;
      if (Array.isArray(this.buf))
        return this.buf.slice(start, end);
      return start === end ? new this.buf.constructor(0) : this._slice.call(this.buf, start, end);
    };
    Reader.prototype.string = function read_string() {
      var bytes2 = this.bytes();
      return utf8.read(bytes2, 0, bytes2.length);
    };
    Reader.prototype.skip = function skip(length5) {
      if (typeof length5 === "number") {
        if (this.pos + length5 > this.len)
          throw indexOutOfRange(this, length5);
        this.pos += length5;
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
      Reader.create = create7();
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

// ../../../node_modules/protobufjs/src/reader_buffer.js
var require_reader_buffer = __commonJS({
  "../../../node_modules/protobufjs/src/reader_buffer.js"(exports2, module2) {
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

// ../../../node_modules/protobufjs/src/writer.js
var require_writer = __commonJS({
  "../../../node_modules/protobufjs/src/writer.js"(exports2, module2) {
    "use strict";
    module2.exports = Writer;
    var util2 = require_minimal();
    var BufferWriter;
    var LongBits2 = util2.LongBits;
    var base645 = util2.base64;
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
    var create7 = function create8() {
      return util2.Buffer ? function create_buffer_setup() {
        return (Writer.create = function create_buffer() {
          return new BufferWriter();
        })();
      } : function create_array() {
        return new Writer();
      };
    };
    Writer.create = create7();
    Writer.alloc = function alloc2(size) {
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
      return value < 0 ? this._push(writeVarint64, 10, LongBits2.fromNumber(value)) : this.uint32(value);
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
      var bits2 = LongBits2.from(value);
      return this._push(writeVarint64, bits2.length(), bits2);
    };
    Writer.prototype.int64 = Writer.prototype.uint64;
    Writer.prototype.sint64 = function write_sint64(value) {
      var bits2 = LongBits2.from(value).zzEncode();
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
      var bits2 = LongBits2.from(value);
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
        var buf = Writer.alloc(len = base645.length(value));
        base645.decode(value, buf, 0);
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
      Writer.create = create7();
      BufferWriter._configure();
    };
  }
});

// ../../../node_modules/protobufjs/src/writer_buffer.js
var require_writer_buffer = __commonJS({
  "../../../node_modules/protobufjs/src/writer_buffer.js"(exports2, module2) {
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

// ../../../node_modules/@stablelib/int/lib/int.js
var require_int = __commonJS({
  "../../../node_modules/@stablelib/int/lib/int.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    function imulShim(a, b) {
      var ah = a >>> 16 & 65535, al = a & 65535;
      var bh = b >>> 16 & 65535, bl = b & 65535;
      return al * bl + (ah * bl + al * bh << 16 >>> 0) | 0;
    }
    exports2.mul = Math.imul || imulShim;
    function add(a, b) {
      return a + b | 0;
    }
    exports2.add = add;
    function sub(a, b) {
      return a - b | 0;
    }
    exports2.sub = sub;
    function rotl(x, n) {
      return x << n | x >>> 32 - n;
    }
    exports2.rotl = rotl;
    function rotr2(x, n) {
      return x << 32 - n | x >>> n;
    }
    exports2.rotr = rotr2;
    function isIntegerShim(n) {
      return typeof n === "number" && isFinite(n) && Math.floor(n) === n;
    }
    exports2.isInteger = Number.isInteger || isIntegerShim;
    exports2.MAX_SAFE_INTEGER = 9007199254740991;
    exports2.isSafeInteger = function(n) {
      return exports2.isInteger(n) && (n >= -exports2.MAX_SAFE_INTEGER && n <= exports2.MAX_SAFE_INTEGER);
    };
  }
});

// ../../../node_modules/@stablelib/binary/lib/binary.js
var require_binary = __commonJS({
  "../../../node_modules/@stablelib/binary/lib/binary.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    var int_1 = require_int();
    function readInt16BE(array, offset) {
      if (offset === void 0) {
        offset = 0;
      }
      return (array[offset + 0] << 8 | array[offset + 1]) << 16 >> 16;
    }
    exports2.readInt16BE = readInt16BE;
    function readUint16BE(array, offset) {
      if (offset === void 0) {
        offset = 0;
      }
      return (array[offset + 0] << 8 | array[offset + 1]) >>> 0;
    }
    exports2.readUint16BE = readUint16BE;
    function readInt16LE(array, offset) {
      if (offset === void 0) {
        offset = 0;
      }
      return (array[offset + 1] << 8 | array[offset]) << 16 >> 16;
    }
    exports2.readInt16LE = readInt16LE;
    function readUint16LE(array, offset) {
      if (offset === void 0) {
        offset = 0;
      }
      return (array[offset + 1] << 8 | array[offset]) >>> 0;
    }
    exports2.readUint16LE = readUint16LE;
    function writeUint16BE(value, out, offset) {
      if (out === void 0) {
        out = new Uint8Array(2);
      }
      if (offset === void 0) {
        offset = 0;
      }
      out[offset + 0] = value >>> 8;
      out[offset + 1] = value >>> 0;
      return out;
    }
    exports2.writeUint16BE = writeUint16BE;
    exports2.writeInt16BE = writeUint16BE;
    function writeUint16LE(value, out, offset) {
      if (out === void 0) {
        out = new Uint8Array(2);
      }
      if (offset === void 0) {
        offset = 0;
      }
      out[offset + 0] = value >>> 0;
      out[offset + 1] = value >>> 8;
      return out;
    }
    exports2.writeUint16LE = writeUint16LE;
    exports2.writeInt16LE = writeUint16LE;
    function readInt32BE(array, offset) {
      if (offset === void 0) {
        offset = 0;
      }
      return array[offset] << 24 | array[offset + 1] << 16 | array[offset + 2] << 8 | array[offset + 3];
    }
    exports2.readInt32BE = readInt32BE;
    function readUint32BE(array, offset) {
      if (offset === void 0) {
        offset = 0;
      }
      return (array[offset] << 24 | array[offset + 1] << 16 | array[offset + 2] << 8 | array[offset + 3]) >>> 0;
    }
    exports2.readUint32BE = readUint32BE;
    function readInt32LE(array, offset) {
      if (offset === void 0) {
        offset = 0;
      }
      return array[offset + 3] << 24 | array[offset + 2] << 16 | array[offset + 1] << 8 | array[offset];
    }
    exports2.readInt32LE = readInt32LE;
    function readUint32LE(array, offset) {
      if (offset === void 0) {
        offset = 0;
      }
      return (array[offset + 3] << 24 | array[offset + 2] << 16 | array[offset + 1] << 8 | array[offset]) >>> 0;
    }
    exports2.readUint32LE = readUint32LE;
    function writeUint32BE(value, out, offset) {
      if (out === void 0) {
        out = new Uint8Array(4);
      }
      if (offset === void 0) {
        offset = 0;
      }
      out[offset + 0] = value >>> 24;
      out[offset + 1] = value >>> 16;
      out[offset + 2] = value >>> 8;
      out[offset + 3] = value >>> 0;
      return out;
    }
    exports2.writeUint32BE = writeUint32BE;
    exports2.writeInt32BE = writeUint32BE;
    function writeUint32LE(value, out, offset) {
      if (out === void 0) {
        out = new Uint8Array(4);
      }
      if (offset === void 0) {
        offset = 0;
      }
      out[offset + 0] = value >>> 0;
      out[offset + 1] = value >>> 8;
      out[offset + 2] = value >>> 16;
      out[offset + 3] = value >>> 24;
      return out;
    }
    exports2.writeUint32LE = writeUint32LE;
    exports2.writeInt32LE = writeUint32LE;
    function readInt64BE(array, offset) {
      if (offset === void 0) {
        offset = 0;
      }
      var hi = readInt32BE(array, offset);
      var lo = readInt32BE(array, offset + 4);
      return hi * 4294967296 + lo - (lo >> 31) * 4294967296;
    }
    exports2.readInt64BE = readInt64BE;
    function readUint64BE(array, offset) {
      if (offset === void 0) {
        offset = 0;
      }
      var hi = readUint32BE(array, offset);
      var lo = readUint32BE(array, offset + 4);
      return hi * 4294967296 + lo;
    }
    exports2.readUint64BE = readUint64BE;
    function readInt64LE(array, offset) {
      if (offset === void 0) {
        offset = 0;
      }
      var lo = readInt32LE(array, offset);
      var hi = readInt32LE(array, offset + 4);
      return hi * 4294967296 + lo - (lo >> 31) * 4294967296;
    }
    exports2.readInt64LE = readInt64LE;
    function readUint64LE(array, offset) {
      if (offset === void 0) {
        offset = 0;
      }
      var lo = readUint32LE(array, offset);
      var hi = readUint32LE(array, offset + 4);
      return hi * 4294967296 + lo;
    }
    exports2.readUint64LE = readUint64LE;
    function writeUint64BE(value, out, offset) {
      if (out === void 0) {
        out = new Uint8Array(8);
      }
      if (offset === void 0) {
        offset = 0;
      }
      writeUint32BE(value / 4294967296 >>> 0, out, offset);
      writeUint32BE(value >>> 0, out, offset + 4);
      return out;
    }
    exports2.writeUint64BE = writeUint64BE;
    exports2.writeInt64BE = writeUint64BE;
    function writeUint64LE(value, out, offset) {
      if (out === void 0) {
        out = new Uint8Array(8);
      }
      if (offset === void 0) {
        offset = 0;
      }
      writeUint32LE(value >>> 0, out, offset);
      writeUint32LE(value / 4294967296 >>> 0, out, offset + 4);
      return out;
    }
    exports2.writeUint64LE = writeUint64LE;
    exports2.writeInt64LE = writeUint64LE;
    function readUintBE(bitLength, array, offset) {
      if (offset === void 0) {
        offset = 0;
      }
      if (bitLength % 8 !== 0) {
        throw new Error("readUintBE supports only bitLengths divisible by 8");
      }
      if (bitLength / 8 > array.length - offset) {
        throw new Error("readUintBE: array is too short for the given bitLength");
      }
      var result = 0;
      var mul = 1;
      for (var i = bitLength / 8 + offset - 1; i >= offset; i--) {
        result += array[i] * mul;
        mul *= 256;
      }
      return result;
    }
    exports2.readUintBE = readUintBE;
    function readUintLE(bitLength, array, offset) {
      if (offset === void 0) {
        offset = 0;
      }
      if (bitLength % 8 !== 0) {
        throw new Error("readUintLE supports only bitLengths divisible by 8");
      }
      if (bitLength / 8 > array.length - offset) {
        throw new Error("readUintLE: array is too short for the given bitLength");
      }
      var result = 0;
      var mul = 1;
      for (var i = offset; i < offset + bitLength / 8; i++) {
        result += array[i] * mul;
        mul *= 256;
      }
      return result;
    }
    exports2.readUintLE = readUintLE;
    function writeUintBE(bitLength, value, out, offset) {
      if (out === void 0) {
        out = new Uint8Array(bitLength / 8);
      }
      if (offset === void 0) {
        offset = 0;
      }
      if (bitLength % 8 !== 0) {
        throw new Error("writeUintBE supports only bitLengths divisible by 8");
      }
      if (!int_1.isSafeInteger(value)) {
        throw new Error("writeUintBE value must be an integer");
      }
      var div = 1;
      for (var i = bitLength / 8 + offset - 1; i >= offset; i--) {
        out[i] = value / div & 255;
        div *= 256;
      }
      return out;
    }
    exports2.writeUintBE = writeUintBE;
    function writeUintLE(bitLength, value, out, offset) {
      if (out === void 0) {
        out = new Uint8Array(bitLength / 8);
      }
      if (offset === void 0) {
        offset = 0;
      }
      if (bitLength % 8 !== 0) {
        throw new Error("writeUintLE supports only bitLengths divisible by 8");
      }
      if (!int_1.isSafeInteger(value)) {
        throw new Error("writeUintLE value must be an integer");
      }
      var div = 1;
      for (var i = offset; i < offset + bitLength / 8; i++) {
        out[i] = value / div & 255;
        div *= 256;
      }
      return out;
    }
    exports2.writeUintLE = writeUintLE;
    function readFloat32BE(array, offset) {
      if (offset === void 0) {
        offset = 0;
      }
      var view = new DataView(array.buffer, array.byteOffset, array.byteLength);
      return view.getFloat32(offset);
    }
    exports2.readFloat32BE = readFloat32BE;
    function readFloat32LE(array, offset) {
      if (offset === void 0) {
        offset = 0;
      }
      var view = new DataView(array.buffer, array.byteOffset, array.byteLength);
      return view.getFloat32(offset, true);
    }
    exports2.readFloat32LE = readFloat32LE;
    function readFloat64BE(array, offset) {
      if (offset === void 0) {
        offset = 0;
      }
      var view = new DataView(array.buffer, array.byteOffset, array.byteLength);
      return view.getFloat64(offset);
    }
    exports2.readFloat64BE = readFloat64BE;
    function readFloat64LE(array, offset) {
      if (offset === void 0) {
        offset = 0;
      }
      var view = new DataView(array.buffer, array.byteOffset, array.byteLength);
      return view.getFloat64(offset, true);
    }
    exports2.readFloat64LE = readFloat64LE;
    function writeFloat32BE(value, out, offset) {
      if (out === void 0) {
        out = new Uint8Array(4);
      }
      if (offset === void 0) {
        offset = 0;
      }
      var view = new DataView(out.buffer, out.byteOffset, out.byteLength);
      view.setFloat32(offset, value);
      return out;
    }
    exports2.writeFloat32BE = writeFloat32BE;
    function writeFloat32LE(value, out, offset) {
      if (out === void 0) {
        out = new Uint8Array(4);
      }
      if (offset === void 0) {
        offset = 0;
      }
      var view = new DataView(out.buffer, out.byteOffset, out.byteLength);
      view.setFloat32(offset, value, true);
      return out;
    }
    exports2.writeFloat32LE = writeFloat32LE;
    function writeFloat64BE(value, out, offset) {
      if (out === void 0) {
        out = new Uint8Array(8);
      }
      if (offset === void 0) {
        offset = 0;
      }
      var view = new DataView(out.buffer, out.byteOffset, out.byteLength);
      view.setFloat64(offset, value);
      return out;
    }
    exports2.writeFloat64BE = writeFloat64BE;
    function writeFloat64LE(value, out, offset) {
      if (out === void 0) {
        out = new Uint8Array(8);
      }
      if (offset === void 0) {
        offset = 0;
      }
      var view = new DataView(out.buffer, out.byteOffset, out.byteLength);
      view.setFloat64(offset, value, true);
      return out;
    }
    exports2.writeFloat64LE = writeFloat64LE;
  }
});

// ../../../node_modules/@stablelib/wipe/lib/wipe.js
var require_wipe = __commonJS({
  "../../../node_modules/@stablelib/wipe/lib/wipe.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    function wipe(array) {
      for (var i = 0; i < array.length; i++) {
        array[i] = 0;
      }
      return array;
    }
    exports2.wipe = wipe;
  }
});

// ../../../node_modules/@stablelib/chacha/lib/chacha.js
var require_chacha = __commonJS({
  "../../../node_modules/@stablelib/chacha/lib/chacha.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    var binary_1 = require_binary();
    var wipe_1 = require_wipe();
    var ROUNDS = 20;
    function core(out, input, key) {
      var j0 = 1634760805;
      var j1 = 857760878;
      var j2 = 2036477234;
      var j3 = 1797285236;
      var j4 = key[3] << 24 | key[2] << 16 | key[1] << 8 | key[0];
      var j5 = key[7] << 24 | key[6] << 16 | key[5] << 8 | key[4];
      var j6 = key[11] << 24 | key[10] << 16 | key[9] << 8 | key[8];
      var j7 = key[15] << 24 | key[14] << 16 | key[13] << 8 | key[12];
      var j8 = key[19] << 24 | key[18] << 16 | key[17] << 8 | key[16];
      var j9 = key[23] << 24 | key[22] << 16 | key[21] << 8 | key[20];
      var j10 = key[27] << 24 | key[26] << 16 | key[25] << 8 | key[24];
      var j11 = key[31] << 24 | key[30] << 16 | key[29] << 8 | key[28];
      var j12 = input[3] << 24 | input[2] << 16 | input[1] << 8 | input[0];
      var j13 = input[7] << 24 | input[6] << 16 | input[5] << 8 | input[4];
      var j14 = input[11] << 24 | input[10] << 16 | input[9] << 8 | input[8];
      var j15 = input[15] << 24 | input[14] << 16 | input[13] << 8 | input[12];
      var x0 = j0;
      var x1 = j1;
      var x2 = j2;
      var x3 = j3;
      var x4 = j4;
      var x5 = j5;
      var x6 = j6;
      var x7 = j7;
      var x8 = j8;
      var x9 = j9;
      var x10 = j10;
      var x11 = j11;
      var x12 = j12;
      var x13 = j13;
      var x14 = j14;
      var x15 = j15;
      for (var i = 0; i < ROUNDS; i += 2) {
        x0 = x0 + x4 | 0;
        x12 ^= x0;
        x12 = x12 >>> 32 - 16 | x12 << 16;
        x8 = x8 + x12 | 0;
        x4 ^= x8;
        x4 = x4 >>> 32 - 12 | x4 << 12;
        x1 = x1 + x5 | 0;
        x13 ^= x1;
        x13 = x13 >>> 32 - 16 | x13 << 16;
        x9 = x9 + x13 | 0;
        x5 ^= x9;
        x5 = x5 >>> 32 - 12 | x5 << 12;
        x2 = x2 + x6 | 0;
        x14 ^= x2;
        x14 = x14 >>> 32 - 16 | x14 << 16;
        x10 = x10 + x14 | 0;
        x6 ^= x10;
        x6 = x6 >>> 32 - 12 | x6 << 12;
        x3 = x3 + x7 | 0;
        x15 ^= x3;
        x15 = x15 >>> 32 - 16 | x15 << 16;
        x11 = x11 + x15 | 0;
        x7 ^= x11;
        x7 = x7 >>> 32 - 12 | x7 << 12;
        x2 = x2 + x6 | 0;
        x14 ^= x2;
        x14 = x14 >>> 32 - 8 | x14 << 8;
        x10 = x10 + x14 | 0;
        x6 ^= x10;
        x6 = x6 >>> 32 - 7 | x6 << 7;
        x3 = x3 + x7 | 0;
        x15 ^= x3;
        x15 = x15 >>> 32 - 8 | x15 << 8;
        x11 = x11 + x15 | 0;
        x7 ^= x11;
        x7 = x7 >>> 32 - 7 | x7 << 7;
        x1 = x1 + x5 | 0;
        x13 ^= x1;
        x13 = x13 >>> 32 - 8 | x13 << 8;
        x9 = x9 + x13 | 0;
        x5 ^= x9;
        x5 = x5 >>> 32 - 7 | x5 << 7;
        x0 = x0 + x4 | 0;
        x12 ^= x0;
        x12 = x12 >>> 32 - 8 | x12 << 8;
        x8 = x8 + x12 | 0;
        x4 ^= x8;
        x4 = x4 >>> 32 - 7 | x4 << 7;
        x0 = x0 + x5 | 0;
        x15 ^= x0;
        x15 = x15 >>> 32 - 16 | x15 << 16;
        x10 = x10 + x15 | 0;
        x5 ^= x10;
        x5 = x5 >>> 32 - 12 | x5 << 12;
        x1 = x1 + x6 | 0;
        x12 ^= x1;
        x12 = x12 >>> 32 - 16 | x12 << 16;
        x11 = x11 + x12 | 0;
        x6 ^= x11;
        x6 = x6 >>> 32 - 12 | x6 << 12;
        x2 = x2 + x7 | 0;
        x13 ^= x2;
        x13 = x13 >>> 32 - 16 | x13 << 16;
        x8 = x8 + x13 | 0;
        x7 ^= x8;
        x7 = x7 >>> 32 - 12 | x7 << 12;
        x3 = x3 + x4 | 0;
        x14 ^= x3;
        x14 = x14 >>> 32 - 16 | x14 << 16;
        x9 = x9 + x14 | 0;
        x4 ^= x9;
        x4 = x4 >>> 32 - 12 | x4 << 12;
        x2 = x2 + x7 | 0;
        x13 ^= x2;
        x13 = x13 >>> 32 - 8 | x13 << 8;
        x8 = x8 + x13 | 0;
        x7 ^= x8;
        x7 = x7 >>> 32 - 7 | x7 << 7;
        x3 = x3 + x4 | 0;
        x14 ^= x3;
        x14 = x14 >>> 32 - 8 | x14 << 8;
        x9 = x9 + x14 | 0;
        x4 ^= x9;
        x4 = x4 >>> 32 - 7 | x4 << 7;
        x1 = x1 + x6 | 0;
        x12 ^= x1;
        x12 = x12 >>> 32 - 8 | x12 << 8;
        x11 = x11 + x12 | 0;
        x6 ^= x11;
        x6 = x6 >>> 32 - 7 | x6 << 7;
        x0 = x0 + x5 | 0;
        x15 ^= x0;
        x15 = x15 >>> 32 - 8 | x15 << 8;
        x10 = x10 + x15 | 0;
        x5 ^= x10;
        x5 = x5 >>> 32 - 7 | x5 << 7;
      }
      binary_1.writeUint32LE(x0 + j0 | 0, out, 0);
      binary_1.writeUint32LE(x1 + j1 | 0, out, 4);
      binary_1.writeUint32LE(x2 + j2 | 0, out, 8);
      binary_1.writeUint32LE(x3 + j3 | 0, out, 12);
      binary_1.writeUint32LE(x4 + j4 | 0, out, 16);
      binary_1.writeUint32LE(x5 + j5 | 0, out, 20);
      binary_1.writeUint32LE(x6 + j6 | 0, out, 24);
      binary_1.writeUint32LE(x7 + j7 | 0, out, 28);
      binary_1.writeUint32LE(x8 + j8 | 0, out, 32);
      binary_1.writeUint32LE(x9 + j9 | 0, out, 36);
      binary_1.writeUint32LE(x10 + j10 | 0, out, 40);
      binary_1.writeUint32LE(x11 + j11 | 0, out, 44);
      binary_1.writeUint32LE(x12 + j12 | 0, out, 48);
      binary_1.writeUint32LE(x13 + j13 | 0, out, 52);
      binary_1.writeUint32LE(x14 + j14 | 0, out, 56);
      binary_1.writeUint32LE(x15 + j15 | 0, out, 60);
    }
    function streamXOR(key, nonce, src5, dst, nonceInplaceCounterLength) {
      if (nonceInplaceCounterLength === void 0) {
        nonceInplaceCounterLength = 0;
      }
      if (key.length !== 32) {
        throw new Error("ChaCha: key size must be 32 bytes");
      }
      if (dst.length < src5.length) {
        throw new Error("ChaCha: destination is shorter than source");
      }
      var nc;
      var counterLength;
      if (nonceInplaceCounterLength === 0) {
        if (nonce.length !== 8 && nonce.length !== 12) {
          throw new Error("ChaCha nonce must be 8 or 12 bytes");
        }
        nc = new Uint8Array(16);
        counterLength = nc.length - nonce.length;
        nc.set(nonce, counterLength);
      } else {
        if (nonce.length !== 16) {
          throw new Error("ChaCha nonce with counter must be 16 bytes");
        }
        nc = nonce;
        counterLength = nonceInplaceCounterLength;
      }
      var block = new Uint8Array(64);
      for (var i = 0; i < src5.length; i += 64) {
        core(block, nc, key);
        for (var j = i; j < i + 64 && j < src5.length; j++) {
          dst[j] = src5[j] ^ block[j - i];
        }
        incrementCounter(nc, 0, counterLength);
      }
      wipe_1.wipe(block);
      if (nonceInplaceCounterLength === 0) {
        wipe_1.wipe(nc);
      }
      return dst;
    }
    exports2.streamXOR = streamXOR;
    function stream(key, nonce, dst, nonceInplaceCounterLength) {
      if (nonceInplaceCounterLength === void 0) {
        nonceInplaceCounterLength = 0;
      }
      wipe_1.wipe(dst);
      return streamXOR(key, nonce, dst, dst, nonceInplaceCounterLength);
    }
    exports2.stream = stream;
    function incrementCounter(counter, pos, len) {
      var carry = 1;
      while (len--) {
        carry = carry + (counter[pos] & 255) | 0;
        counter[pos] = carry & 255;
        carry >>>= 8;
        pos++;
      }
      if (carry > 0) {
        throw new Error("ChaCha: counter overflow");
      }
    }
  }
});

// ../../../node_modules/@stablelib/constant-time/lib/constant-time.js
var require_constant_time = __commonJS({
  "../../../node_modules/@stablelib/constant-time/lib/constant-time.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    function select(subject, resultIfOne, resultIfZero) {
      return ~(subject - 1) & resultIfOne | subject - 1 & resultIfZero;
    }
    exports2.select = select;
    function lessOrEqual(a, b) {
      return (a | 0) - (b | 0) - 1 >>> 31 & 1;
    }
    exports2.lessOrEqual = lessOrEqual;
    function compare2(a, b) {
      if (a.length !== b.length) {
        return 0;
      }
      var result = 0;
      for (var i = 0; i < a.length; i++) {
        result |= a[i] ^ b[i];
      }
      return 1 & result - 1 >>> 8;
    }
    exports2.compare = compare2;
    function equal(a, b) {
      if (a.length === 0 || b.length === 0) {
        return false;
      }
      return compare2(a, b) !== 0;
    }
    exports2.equal = equal;
  }
});

// ../../../node_modules/@stablelib/poly1305/lib/poly1305.js
var require_poly1305 = __commonJS({
  "../../../node_modules/@stablelib/poly1305/lib/poly1305.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    var constant_time_1 = require_constant_time();
    var wipe_1 = require_wipe();
    exports2.DIGEST_LENGTH = 16;
    var Poly1305 = (
      /** @class */
      function() {
        function Poly13052(key) {
          this.digestLength = exports2.DIGEST_LENGTH;
          this._buffer = new Uint8Array(16);
          this._r = new Uint16Array(10);
          this._h = new Uint16Array(10);
          this._pad = new Uint16Array(8);
          this._leftover = 0;
          this._fin = 0;
          this._finished = false;
          var t0 = key[0] | key[1] << 8;
          this._r[0] = t0 & 8191;
          var t1 = key[2] | key[3] << 8;
          this._r[1] = (t0 >>> 13 | t1 << 3) & 8191;
          var t2 = key[4] | key[5] << 8;
          this._r[2] = (t1 >>> 10 | t2 << 6) & 7939;
          var t3 = key[6] | key[7] << 8;
          this._r[3] = (t2 >>> 7 | t3 << 9) & 8191;
          var t4 = key[8] | key[9] << 8;
          this._r[4] = (t3 >>> 4 | t4 << 12) & 255;
          this._r[5] = t4 >>> 1 & 8190;
          var t5 = key[10] | key[11] << 8;
          this._r[6] = (t4 >>> 14 | t5 << 2) & 8191;
          var t6 = key[12] | key[13] << 8;
          this._r[7] = (t5 >>> 11 | t6 << 5) & 8065;
          var t7 = key[14] | key[15] << 8;
          this._r[8] = (t6 >>> 8 | t7 << 8) & 8191;
          this._r[9] = t7 >>> 5 & 127;
          this._pad[0] = key[16] | key[17] << 8;
          this._pad[1] = key[18] | key[19] << 8;
          this._pad[2] = key[20] | key[21] << 8;
          this._pad[3] = key[22] | key[23] << 8;
          this._pad[4] = key[24] | key[25] << 8;
          this._pad[5] = key[26] | key[27] << 8;
          this._pad[6] = key[28] | key[29] << 8;
          this._pad[7] = key[30] | key[31] << 8;
        }
        Poly13052.prototype._blocks = function(m, mpos, bytes2) {
          var hibit = this._fin ? 0 : 1 << 11;
          var h0 = this._h[0], h1 = this._h[1], h2 = this._h[2], h3 = this._h[3], h4 = this._h[4], h5 = this._h[5], h6 = this._h[6], h7 = this._h[7], h8 = this._h[8], h9 = this._h[9];
          var r0 = this._r[0], r1 = this._r[1], r2 = this._r[2], r3 = this._r[3], r4 = this._r[4], r5 = this._r[5], r6 = this._r[6], r7 = this._r[7], r8 = this._r[8], r9 = this._r[9];
          while (bytes2 >= 16) {
            var t0 = m[mpos + 0] | m[mpos + 1] << 8;
            h0 += t0 & 8191;
            var t1 = m[mpos + 2] | m[mpos + 3] << 8;
            h1 += (t0 >>> 13 | t1 << 3) & 8191;
            var t2 = m[mpos + 4] | m[mpos + 5] << 8;
            h2 += (t1 >>> 10 | t2 << 6) & 8191;
            var t3 = m[mpos + 6] | m[mpos + 7] << 8;
            h3 += (t2 >>> 7 | t3 << 9) & 8191;
            var t4 = m[mpos + 8] | m[mpos + 9] << 8;
            h4 += (t3 >>> 4 | t4 << 12) & 8191;
            h5 += t4 >>> 1 & 8191;
            var t5 = m[mpos + 10] | m[mpos + 11] << 8;
            h6 += (t4 >>> 14 | t5 << 2) & 8191;
            var t6 = m[mpos + 12] | m[mpos + 13] << 8;
            h7 += (t5 >>> 11 | t6 << 5) & 8191;
            var t7 = m[mpos + 14] | m[mpos + 15] << 8;
            h8 += (t6 >>> 8 | t7 << 8) & 8191;
            h9 += t7 >>> 5 | hibit;
            var c = 0;
            var d0 = c;
            d0 += h0 * r0;
            d0 += h1 * (5 * r9);
            d0 += h2 * (5 * r8);
            d0 += h3 * (5 * r7);
            d0 += h4 * (5 * r6);
            c = d0 >>> 13;
            d0 &= 8191;
            d0 += h5 * (5 * r5);
            d0 += h6 * (5 * r4);
            d0 += h7 * (5 * r3);
            d0 += h8 * (5 * r2);
            d0 += h9 * (5 * r1);
            c += d0 >>> 13;
            d0 &= 8191;
            var d1 = c;
            d1 += h0 * r1;
            d1 += h1 * r0;
            d1 += h2 * (5 * r9);
            d1 += h3 * (5 * r8);
            d1 += h4 * (5 * r7);
            c = d1 >>> 13;
            d1 &= 8191;
            d1 += h5 * (5 * r6);
            d1 += h6 * (5 * r5);
            d1 += h7 * (5 * r4);
            d1 += h8 * (5 * r3);
            d1 += h9 * (5 * r2);
            c += d1 >>> 13;
            d1 &= 8191;
            var d2 = c;
            d2 += h0 * r2;
            d2 += h1 * r1;
            d2 += h2 * r0;
            d2 += h3 * (5 * r9);
            d2 += h4 * (5 * r8);
            c = d2 >>> 13;
            d2 &= 8191;
            d2 += h5 * (5 * r7);
            d2 += h6 * (5 * r6);
            d2 += h7 * (5 * r5);
            d2 += h8 * (5 * r4);
            d2 += h9 * (5 * r3);
            c += d2 >>> 13;
            d2 &= 8191;
            var d3 = c;
            d3 += h0 * r3;
            d3 += h1 * r2;
            d3 += h2 * r1;
            d3 += h3 * r0;
            d3 += h4 * (5 * r9);
            c = d3 >>> 13;
            d3 &= 8191;
            d3 += h5 * (5 * r8);
            d3 += h6 * (5 * r7);
            d3 += h7 * (5 * r6);
            d3 += h8 * (5 * r5);
            d3 += h9 * (5 * r4);
            c += d3 >>> 13;
            d3 &= 8191;
            var d4 = c;
            d4 += h0 * r4;
            d4 += h1 * r3;
            d4 += h2 * r2;
            d4 += h3 * r1;
            d4 += h4 * r0;
            c = d4 >>> 13;
            d4 &= 8191;
            d4 += h5 * (5 * r9);
            d4 += h6 * (5 * r8);
            d4 += h7 * (5 * r7);
            d4 += h8 * (5 * r6);
            d4 += h9 * (5 * r5);
            c += d4 >>> 13;
            d4 &= 8191;
            var d5 = c;
            d5 += h0 * r5;
            d5 += h1 * r4;
            d5 += h2 * r3;
            d5 += h3 * r2;
            d5 += h4 * r1;
            c = d5 >>> 13;
            d5 &= 8191;
            d5 += h5 * r0;
            d5 += h6 * (5 * r9);
            d5 += h7 * (5 * r8);
            d5 += h8 * (5 * r7);
            d5 += h9 * (5 * r6);
            c += d5 >>> 13;
            d5 &= 8191;
            var d6 = c;
            d6 += h0 * r6;
            d6 += h1 * r5;
            d6 += h2 * r4;
            d6 += h3 * r3;
            d6 += h4 * r2;
            c = d6 >>> 13;
            d6 &= 8191;
            d6 += h5 * r1;
            d6 += h6 * r0;
            d6 += h7 * (5 * r9);
            d6 += h8 * (5 * r8);
            d6 += h9 * (5 * r7);
            c += d6 >>> 13;
            d6 &= 8191;
            var d7 = c;
            d7 += h0 * r7;
            d7 += h1 * r6;
            d7 += h2 * r5;
            d7 += h3 * r4;
            d7 += h4 * r3;
            c = d7 >>> 13;
            d7 &= 8191;
            d7 += h5 * r2;
            d7 += h6 * r1;
            d7 += h7 * r0;
            d7 += h8 * (5 * r9);
            d7 += h9 * (5 * r8);
            c += d7 >>> 13;
            d7 &= 8191;
            var d8 = c;
            d8 += h0 * r8;
            d8 += h1 * r7;
            d8 += h2 * r6;
            d8 += h3 * r5;
            d8 += h4 * r4;
            c = d8 >>> 13;
            d8 &= 8191;
            d8 += h5 * r3;
            d8 += h6 * r2;
            d8 += h7 * r1;
            d8 += h8 * r0;
            d8 += h9 * (5 * r9);
            c += d8 >>> 13;
            d8 &= 8191;
            var d9 = c;
            d9 += h0 * r9;
            d9 += h1 * r8;
            d9 += h2 * r7;
            d9 += h3 * r6;
            d9 += h4 * r5;
            c = d9 >>> 13;
            d9 &= 8191;
            d9 += h5 * r4;
            d9 += h6 * r3;
            d9 += h7 * r2;
            d9 += h8 * r1;
            d9 += h9 * r0;
            c += d9 >>> 13;
            d9 &= 8191;
            c = (c << 2) + c | 0;
            c = c + d0 | 0;
            d0 = c & 8191;
            c = c >>> 13;
            d1 += c;
            h0 = d0;
            h1 = d1;
            h2 = d2;
            h3 = d3;
            h4 = d4;
            h5 = d5;
            h6 = d6;
            h7 = d7;
            h8 = d8;
            h9 = d9;
            mpos += 16;
            bytes2 -= 16;
          }
          this._h[0] = h0;
          this._h[1] = h1;
          this._h[2] = h2;
          this._h[3] = h3;
          this._h[4] = h4;
          this._h[5] = h5;
          this._h[6] = h6;
          this._h[7] = h7;
          this._h[8] = h8;
          this._h[9] = h9;
        };
        Poly13052.prototype.finish = function(mac, macpos) {
          if (macpos === void 0) {
            macpos = 0;
          }
          var g = new Uint16Array(10);
          var c;
          var mask;
          var f;
          var i;
          if (this._leftover) {
            i = this._leftover;
            this._buffer[i++] = 1;
            for (; i < 16; i++) {
              this._buffer[i] = 0;
            }
            this._fin = 1;
            this._blocks(this._buffer, 0, 16);
          }
          c = this._h[1] >>> 13;
          this._h[1] &= 8191;
          for (i = 2; i < 10; i++) {
            this._h[i] += c;
            c = this._h[i] >>> 13;
            this._h[i] &= 8191;
          }
          this._h[0] += c * 5;
          c = this._h[0] >>> 13;
          this._h[0] &= 8191;
          this._h[1] += c;
          c = this._h[1] >>> 13;
          this._h[1] &= 8191;
          this._h[2] += c;
          g[0] = this._h[0] + 5;
          c = g[0] >>> 13;
          g[0] &= 8191;
          for (i = 1; i < 10; i++) {
            g[i] = this._h[i] + c;
            c = g[i] >>> 13;
            g[i] &= 8191;
          }
          g[9] -= 1 << 13;
          mask = (c ^ 1) - 1;
          for (i = 0; i < 10; i++) {
            g[i] &= mask;
          }
          mask = ~mask;
          for (i = 0; i < 10; i++) {
            this._h[i] = this._h[i] & mask | g[i];
          }
          this._h[0] = (this._h[0] | this._h[1] << 13) & 65535;
          this._h[1] = (this._h[1] >>> 3 | this._h[2] << 10) & 65535;
          this._h[2] = (this._h[2] >>> 6 | this._h[3] << 7) & 65535;
          this._h[3] = (this._h[3] >>> 9 | this._h[4] << 4) & 65535;
          this._h[4] = (this._h[4] >>> 12 | this._h[5] << 1 | this._h[6] << 14) & 65535;
          this._h[5] = (this._h[6] >>> 2 | this._h[7] << 11) & 65535;
          this._h[6] = (this._h[7] >>> 5 | this._h[8] << 8) & 65535;
          this._h[7] = (this._h[8] >>> 8 | this._h[9] << 5) & 65535;
          f = this._h[0] + this._pad[0];
          this._h[0] = f & 65535;
          for (i = 1; i < 8; i++) {
            f = (this._h[i] + this._pad[i] | 0) + (f >>> 16) | 0;
            this._h[i] = f & 65535;
          }
          mac[macpos + 0] = this._h[0] >>> 0;
          mac[macpos + 1] = this._h[0] >>> 8;
          mac[macpos + 2] = this._h[1] >>> 0;
          mac[macpos + 3] = this._h[1] >>> 8;
          mac[macpos + 4] = this._h[2] >>> 0;
          mac[macpos + 5] = this._h[2] >>> 8;
          mac[macpos + 6] = this._h[3] >>> 0;
          mac[macpos + 7] = this._h[3] >>> 8;
          mac[macpos + 8] = this._h[4] >>> 0;
          mac[macpos + 9] = this._h[4] >>> 8;
          mac[macpos + 10] = this._h[5] >>> 0;
          mac[macpos + 11] = this._h[5] >>> 8;
          mac[macpos + 12] = this._h[6] >>> 0;
          mac[macpos + 13] = this._h[6] >>> 8;
          mac[macpos + 14] = this._h[7] >>> 0;
          mac[macpos + 15] = this._h[7] >>> 8;
          this._finished = true;
          return this;
        };
        Poly13052.prototype.update = function(m) {
          var mpos = 0;
          var bytes2 = m.length;
          var want;
          if (this._leftover) {
            want = 16 - this._leftover;
            if (want > bytes2) {
              want = bytes2;
            }
            for (var i = 0; i < want; i++) {
              this._buffer[this._leftover + i] = m[mpos + i];
            }
            bytes2 -= want;
            mpos += want;
            this._leftover += want;
            if (this._leftover < 16) {
              return this;
            }
            this._blocks(this._buffer, 0, 16);
            this._leftover = 0;
          }
          if (bytes2 >= 16) {
            want = bytes2 - bytes2 % 16;
            this._blocks(m, mpos, want);
            mpos += want;
            bytes2 -= want;
          }
          if (bytes2) {
            for (var i = 0; i < bytes2; i++) {
              this._buffer[this._leftover + i] = m[mpos + i];
            }
            this._leftover += bytes2;
          }
          return this;
        };
        Poly13052.prototype.digest = function() {
          if (this._finished) {
            throw new Error("Poly1305 was finished");
          }
          var mac = new Uint8Array(16);
          this.finish(mac);
          return mac;
        };
        Poly13052.prototype.clean = function() {
          wipe_1.wipe(this._buffer);
          wipe_1.wipe(this._r);
          wipe_1.wipe(this._h);
          wipe_1.wipe(this._pad);
          this._leftover = 0;
          this._fin = 0;
          this._finished = true;
          return this;
        };
        return Poly13052;
      }()
    );
    exports2.Poly1305 = Poly1305;
    function oneTimeAuth(key, data) {
      var h = new Poly1305(key);
      h.update(data);
      var digest5 = h.digest();
      h.clean();
      return digest5;
    }
    exports2.oneTimeAuth = oneTimeAuth;
    function equal(a, b) {
      if (a.length !== exports2.DIGEST_LENGTH || b.length !== exports2.DIGEST_LENGTH) {
        return false;
      }
      return constant_time_1.equal(a, b);
    }
    exports2.equal = equal;
  }
});

// ../../../node_modules/@stablelib/chacha20poly1305/lib/chacha20poly1305.js
var require_chacha20poly1305 = __commonJS({
  "../../../node_modules/@stablelib/chacha20poly1305/lib/chacha20poly1305.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    var chacha_1 = require_chacha();
    var poly1305_1 = require_poly1305();
    var wipe_1 = require_wipe();
    var binary_1 = require_binary();
    var constant_time_1 = require_constant_time();
    exports2.KEY_LENGTH = 32;
    exports2.NONCE_LENGTH = 12;
    exports2.TAG_LENGTH = 16;
    var ZEROS = new Uint8Array(16);
    var ChaCha20Poly13052 = (
      /** @class */
      function() {
        function ChaCha20Poly13053(key) {
          this.nonceLength = exports2.NONCE_LENGTH;
          this.tagLength = exports2.TAG_LENGTH;
          if (key.length !== exports2.KEY_LENGTH) {
            throw new Error("ChaCha20Poly1305 needs 32-byte key");
          }
          this._key = new Uint8Array(key);
        }
        ChaCha20Poly13053.prototype.seal = function(nonce, plaintext, associatedData, dst) {
          if (nonce.length > 16) {
            throw new Error("ChaCha20Poly1305: incorrect nonce length");
          }
          var counter = new Uint8Array(16);
          counter.set(nonce, counter.length - nonce.length);
          var authKey = new Uint8Array(32);
          chacha_1.stream(this._key, counter, authKey, 4);
          var resultLength = plaintext.length + this.tagLength;
          var result;
          if (dst) {
            if (dst.length !== resultLength) {
              throw new Error("ChaCha20Poly1305: incorrect destination length");
            }
            result = dst;
          } else {
            result = new Uint8Array(resultLength);
          }
          chacha_1.streamXOR(this._key, counter, plaintext, result, 4);
          this._authenticate(result.subarray(result.length - this.tagLength, result.length), authKey, result.subarray(0, result.length - this.tagLength), associatedData);
          wipe_1.wipe(counter);
          return result;
        };
        ChaCha20Poly13053.prototype.open = function(nonce, sealed, associatedData, dst) {
          if (nonce.length > 16) {
            throw new Error("ChaCha20Poly1305: incorrect nonce length");
          }
          if (sealed.length < this.tagLength) {
            return null;
          }
          var counter = new Uint8Array(16);
          counter.set(nonce, counter.length - nonce.length);
          var authKey = new Uint8Array(32);
          chacha_1.stream(this._key, counter, authKey, 4);
          var calculatedTag = new Uint8Array(this.tagLength);
          this._authenticate(calculatedTag, authKey, sealed.subarray(0, sealed.length - this.tagLength), associatedData);
          if (!constant_time_1.equal(calculatedTag, sealed.subarray(sealed.length - this.tagLength, sealed.length))) {
            return null;
          }
          var resultLength = sealed.length - this.tagLength;
          var result;
          if (dst) {
            if (dst.length !== resultLength) {
              throw new Error("ChaCha20Poly1305: incorrect destination length");
            }
            result = dst;
          } else {
            result = new Uint8Array(resultLength);
          }
          chacha_1.streamXOR(this._key, counter, sealed.subarray(0, sealed.length - this.tagLength), result, 4);
          wipe_1.wipe(counter);
          return result;
        };
        ChaCha20Poly13053.prototype.clean = function() {
          wipe_1.wipe(this._key);
          return this;
        };
        ChaCha20Poly13053.prototype._authenticate = function(tagOut, authKey, ciphertext, associatedData) {
          var h = new poly1305_1.Poly1305(authKey);
          if (associatedData) {
            h.update(associatedData);
            if (associatedData.length % 16 > 0) {
              h.update(ZEROS.subarray(associatedData.length % 16));
            }
          }
          h.update(ciphertext);
          if (ciphertext.length % 16 > 0) {
            h.update(ZEROS.subarray(ciphertext.length % 16));
          }
          var length5 = new Uint8Array(8);
          if (associatedData) {
            binary_1.writeUint64LE(associatedData.length, length5);
          }
          h.update(length5);
          binary_1.writeUint64LE(ciphertext.length, length5);
          h.update(length5);
          var tag = h.digest();
          for (var i = 0; i < tag.length; i++) {
            tagOut[i] = tag[i];
          }
          h.clean();
          wipe_1.wipe(tag);
          wipe_1.wipe(length5);
        };
        return ChaCha20Poly13053;
      }()
    );
    exports2.ChaCha20Poly1305 = ChaCha20Poly13052;
  }
});

// ../../../node_modules/@stablelib/random/lib/source/browser.js
var require_browser2 = __commonJS({
  "../../../node_modules/@stablelib/random/lib/source/browser.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.BrowserRandomSource = void 0;
    var QUOTA = 65536;
    var BrowserRandomSource = class {
      constructor() {
        this.isAvailable = false;
        this.isInstantiated = false;
        const browserCrypto = typeof self !== "undefined" ? self.crypto || self.msCrypto : null;
        if (browserCrypto && browserCrypto.getRandomValues !== void 0) {
          this._crypto = browserCrypto;
          this.isAvailable = true;
          this.isInstantiated = true;
        }
      }
      randomBytes(length5) {
        if (!this.isAvailable || !this._crypto) {
          throw new Error("Browser random byte generator is not available.");
        }
        const out = new Uint8Array(length5);
        for (let i = 0; i < out.length; i += QUOTA) {
          this._crypto.getRandomValues(out.subarray(i, i + Math.min(out.length - i, QUOTA)));
        }
        return out;
      }
    };
    exports2.BrowserRandomSource = BrowserRandomSource;
  }
});

// (disabled):crypto
var require_crypto = __commonJS({
  "(disabled):crypto"() {
  }
});

// ../../../node_modules/@stablelib/random/lib/source/node.js
var require_node = __commonJS({
  "../../../node_modules/@stablelib/random/lib/source/node.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.NodeRandomSource = void 0;
    var wipe_1 = require_wipe();
    var NodeRandomSource = class {
      constructor() {
        this.isAvailable = false;
        this.isInstantiated = false;
        if (typeof __require !== "undefined") {
          const nodeCrypto3 = require_crypto();
          if (nodeCrypto3 && nodeCrypto3.randomBytes) {
            this._crypto = nodeCrypto3;
            this.isAvailable = true;
            this.isInstantiated = true;
          }
        }
      }
      randomBytes(length5) {
        if (!this.isAvailable || !this._crypto) {
          throw new Error("Node.js random byte generator is not available.");
        }
        let buffer = this._crypto.randomBytes(length5);
        if (buffer.length !== length5) {
          throw new Error("NodeRandomSource: got fewer bytes than requested");
        }
        const out = new Uint8Array(length5);
        for (let i = 0; i < out.length; i++) {
          out[i] = buffer[i];
        }
        (0, wipe_1.wipe)(buffer);
        return out;
      }
    };
    exports2.NodeRandomSource = NodeRandomSource;
  }
});

// ../../../node_modules/@stablelib/random/lib/source/system.js
var require_system = __commonJS({
  "../../../node_modules/@stablelib/random/lib/source/system.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.SystemRandomSource = void 0;
    var browser_1 = require_browser2();
    var node_1 = require_node();
    var SystemRandomSource = class {
      constructor() {
        this.isAvailable = false;
        this.name = "";
        this._source = new browser_1.BrowserRandomSource();
        if (this._source.isAvailable) {
          this.isAvailable = true;
          this.name = "Browser";
          return;
        }
        this._source = new node_1.NodeRandomSource();
        if (this._source.isAvailable) {
          this.isAvailable = true;
          this.name = "Node";
          return;
        }
      }
      randomBytes(length5) {
        if (!this.isAvailable) {
          throw new Error("System random byte generator is not available.");
        }
        return this._source.randomBytes(length5);
      }
    };
    exports2.SystemRandomSource = SystemRandomSource;
  }
});

// ../../../node_modules/@stablelib/random/lib/random.js
var require_random = __commonJS({
  "../../../node_modules/@stablelib/random/lib/random.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.randomStringForEntropy = exports2.randomString = exports2.randomUint32 = exports2.randomBytes = exports2.defaultRandomSource = void 0;
    var system_1 = require_system();
    var binary_1 = require_binary();
    var wipe_1 = require_wipe();
    exports2.defaultRandomSource = new system_1.SystemRandomSource();
    function randomBytes2(length5, prng = exports2.defaultRandomSource) {
      return prng.randomBytes(length5);
    }
    exports2.randomBytes = randomBytes2;
    function randomUint32(prng = exports2.defaultRandomSource) {
      const buf = randomBytes2(4, prng);
      const result = (0, binary_1.readUint32LE)(buf);
      (0, wipe_1.wipe)(buf);
      return result;
    }
    exports2.randomUint32 = randomUint32;
    var ALPHANUMERIC = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    function randomString(length5, charset2 = ALPHANUMERIC, prng = exports2.defaultRandomSource) {
      if (charset2.length < 2) {
        throw new Error("randomString charset is too short");
      }
      if (charset2.length > 256) {
        throw new Error("randomString charset is too long");
      }
      let out = "";
      const charsLen = charset2.length;
      const maxByte = 256 - 256 % charsLen;
      while (length5 > 0) {
        const buf = randomBytes2(Math.ceil(length5 * 256 / maxByte), prng);
        for (let i = 0; i < buf.length && length5 > 0; i++) {
          const randomByte = buf[i];
          if (randomByte < maxByte) {
            out += charset2.charAt(randomByte % charsLen);
            length5--;
          }
        }
        (0, wipe_1.wipe)(buf);
      }
      return out;
    }
    exports2.randomString = randomString;
    function randomStringForEntropy(bits2, charset2 = ALPHANUMERIC, prng = exports2.defaultRandomSource) {
      const length5 = Math.ceil(bits2 / (Math.log(charset2.length) / Math.LN2));
      return randomString(length5, charset2, prng);
    }
    exports2.randomStringForEntropy = randomStringForEntropy;
  }
});

// ../../../node_modules/@stablelib/x25519/lib/x25519.js
var require_x25519 = __commonJS({
  "../../../node_modules/@stablelib/x25519/lib/x25519.js"(exports2) {
    "use strict";
    Object.defineProperty(exports2, "__esModule", { value: true });
    exports2.sharedKey = exports2.generateKeyPair = exports2.generateKeyPairFromSeed = exports2.scalarMultBase = exports2.scalarMult = exports2.SHARED_KEY_LENGTH = exports2.SECRET_KEY_LENGTH = exports2.PUBLIC_KEY_LENGTH = void 0;
    var random_1 = require_random();
    var wipe_1 = require_wipe();
    exports2.PUBLIC_KEY_LENGTH = 32;
    exports2.SECRET_KEY_LENGTH = 32;
    exports2.SHARED_KEY_LENGTH = 32;
    function gf(init) {
      const r = new Float64Array(16);
      if (init) {
        for (let i = 0; i < init.length; i++) {
          r[i] = init[i];
        }
      }
      return r;
    }
    var _9 = new Uint8Array(32);
    _9[0] = 9;
    var _121665 = gf([56129, 1]);
    function car25519(o) {
      let c = 1;
      for (let i = 0; i < 16; i++) {
        let v = o[i] + c + 65535;
        c = Math.floor(v / 65536);
        o[i] = v - c * 65536;
      }
      o[0] += c - 1 + 37 * (c - 1);
    }
    function sel25519(p, q, b) {
      const c = ~(b - 1);
      for (let i = 0; i < 16; i++) {
        const t = c & (p[i] ^ q[i]);
        p[i] ^= t;
        q[i] ^= t;
      }
    }
    function pack25519(o, n) {
      const m = gf();
      const t = gf();
      for (let i = 0; i < 16; i++) {
        t[i] = n[i];
      }
      car25519(t);
      car25519(t);
      car25519(t);
      for (let j = 0; j < 2; j++) {
        m[0] = t[0] - 65517;
        for (let i = 1; i < 15; i++) {
          m[i] = t[i] - 65535 - (m[i - 1] >> 16 & 1);
          m[i - 1] &= 65535;
        }
        m[15] = t[15] - 32767 - (m[14] >> 16 & 1);
        const b = m[15] >> 16 & 1;
        m[14] &= 65535;
        sel25519(t, m, 1 - b);
      }
      for (let i = 0; i < 16; i++) {
        o[2 * i] = t[i] & 255;
        o[2 * i + 1] = t[i] >> 8;
      }
    }
    function unpack25519(o, n) {
      for (let i = 0; i < 16; i++) {
        o[i] = n[2 * i] + (n[2 * i + 1] << 8);
      }
      o[15] &= 32767;
    }
    function add(o, a, b) {
      for (let i = 0; i < 16; i++) {
        o[i] = a[i] + b[i];
      }
    }
    function sub(o, a, b) {
      for (let i = 0; i < 16; i++) {
        o[i] = a[i] - b[i];
      }
    }
    function mul(o, a, b) {
      let v, c, t0 = 0, t1 = 0, t2 = 0, t3 = 0, t4 = 0, t5 = 0, t6 = 0, t7 = 0, t8 = 0, t9 = 0, t10 = 0, t11 = 0, t12 = 0, t13 = 0, t14 = 0, t15 = 0, t16 = 0, t17 = 0, t18 = 0, t19 = 0, t20 = 0, t21 = 0, t22 = 0, t23 = 0, t24 = 0, t25 = 0, t26 = 0, t27 = 0, t28 = 0, t29 = 0, t30 = 0, b0 = b[0], b1 = b[1], b2 = b[2], b3 = b[3], b4 = b[4], b5 = b[5], b6 = b[6], b7 = b[7], b8 = b[8], b9 = b[9], b10 = b[10], b11 = b[11], b12 = b[12], b13 = b[13], b14 = b[14], b15 = b[15];
      v = a[0];
      t0 += v * b0;
      t1 += v * b1;
      t2 += v * b2;
      t3 += v * b3;
      t4 += v * b4;
      t5 += v * b5;
      t6 += v * b6;
      t7 += v * b7;
      t8 += v * b8;
      t9 += v * b9;
      t10 += v * b10;
      t11 += v * b11;
      t12 += v * b12;
      t13 += v * b13;
      t14 += v * b14;
      t15 += v * b15;
      v = a[1];
      t1 += v * b0;
      t2 += v * b1;
      t3 += v * b2;
      t4 += v * b3;
      t5 += v * b4;
      t6 += v * b5;
      t7 += v * b6;
      t8 += v * b7;
      t9 += v * b8;
      t10 += v * b9;
      t11 += v * b10;
      t12 += v * b11;
      t13 += v * b12;
      t14 += v * b13;
      t15 += v * b14;
      t16 += v * b15;
      v = a[2];
      t2 += v * b0;
      t3 += v * b1;
      t4 += v * b2;
      t5 += v * b3;
      t6 += v * b4;
      t7 += v * b5;
      t8 += v * b6;
      t9 += v * b7;
      t10 += v * b8;
      t11 += v * b9;
      t12 += v * b10;
      t13 += v * b11;
      t14 += v * b12;
      t15 += v * b13;
      t16 += v * b14;
      t17 += v * b15;
      v = a[3];
      t3 += v * b0;
      t4 += v * b1;
      t5 += v * b2;
      t6 += v * b3;
      t7 += v * b4;
      t8 += v * b5;
      t9 += v * b6;
      t10 += v * b7;
      t11 += v * b8;
      t12 += v * b9;
      t13 += v * b10;
      t14 += v * b11;
      t15 += v * b12;
      t16 += v * b13;
      t17 += v * b14;
      t18 += v * b15;
      v = a[4];
      t4 += v * b0;
      t5 += v * b1;
      t6 += v * b2;
      t7 += v * b3;
      t8 += v * b4;
      t9 += v * b5;
      t10 += v * b6;
      t11 += v * b7;
      t12 += v * b8;
      t13 += v * b9;
      t14 += v * b10;
      t15 += v * b11;
      t16 += v * b12;
      t17 += v * b13;
      t18 += v * b14;
      t19 += v * b15;
      v = a[5];
      t5 += v * b0;
      t6 += v * b1;
      t7 += v * b2;
      t8 += v * b3;
      t9 += v * b4;
      t10 += v * b5;
      t11 += v * b6;
      t12 += v * b7;
      t13 += v * b8;
      t14 += v * b9;
      t15 += v * b10;
      t16 += v * b11;
      t17 += v * b12;
      t18 += v * b13;
      t19 += v * b14;
      t20 += v * b15;
      v = a[6];
      t6 += v * b0;
      t7 += v * b1;
      t8 += v * b2;
      t9 += v * b3;
      t10 += v * b4;
      t11 += v * b5;
      t12 += v * b6;
      t13 += v * b7;
      t14 += v * b8;
      t15 += v * b9;
      t16 += v * b10;
      t17 += v * b11;
      t18 += v * b12;
      t19 += v * b13;
      t20 += v * b14;
      t21 += v * b15;
      v = a[7];
      t7 += v * b0;
      t8 += v * b1;
      t9 += v * b2;
      t10 += v * b3;
      t11 += v * b4;
      t12 += v * b5;
      t13 += v * b6;
      t14 += v * b7;
      t15 += v * b8;
      t16 += v * b9;
      t17 += v * b10;
      t18 += v * b11;
      t19 += v * b12;
      t20 += v * b13;
      t21 += v * b14;
      t22 += v * b15;
      v = a[8];
      t8 += v * b0;
      t9 += v * b1;
      t10 += v * b2;
      t11 += v * b3;
      t12 += v * b4;
      t13 += v * b5;
      t14 += v * b6;
      t15 += v * b7;
      t16 += v * b8;
      t17 += v * b9;
      t18 += v * b10;
      t19 += v * b11;
      t20 += v * b12;
      t21 += v * b13;
      t22 += v * b14;
      t23 += v * b15;
      v = a[9];
      t9 += v * b0;
      t10 += v * b1;
      t11 += v * b2;
      t12 += v * b3;
      t13 += v * b4;
      t14 += v * b5;
      t15 += v * b6;
      t16 += v * b7;
      t17 += v * b8;
      t18 += v * b9;
      t19 += v * b10;
      t20 += v * b11;
      t21 += v * b12;
      t22 += v * b13;
      t23 += v * b14;
      t24 += v * b15;
      v = a[10];
      t10 += v * b0;
      t11 += v * b1;
      t12 += v * b2;
      t13 += v * b3;
      t14 += v * b4;
      t15 += v * b5;
      t16 += v * b6;
      t17 += v * b7;
      t18 += v * b8;
      t19 += v * b9;
      t20 += v * b10;
      t21 += v * b11;
      t22 += v * b12;
      t23 += v * b13;
      t24 += v * b14;
      t25 += v * b15;
      v = a[11];
      t11 += v * b0;
      t12 += v * b1;
      t13 += v * b2;
      t14 += v * b3;
      t15 += v * b4;
      t16 += v * b5;
      t17 += v * b6;
      t18 += v * b7;
      t19 += v * b8;
      t20 += v * b9;
      t21 += v * b10;
      t22 += v * b11;
      t23 += v * b12;
      t24 += v * b13;
      t25 += v * b14;
      t26 += v * b15;
      v = a[12];
      t12 += v * b0;
      t13 += v * b1;
      t14 += v * b2;
      t15 += v * b3;
      t16 += v * b4;
      t17 += v * b5;
      t18 += v * b6;
      t19 += v * b7;
      t20 += v * b8;
      t21 += v * b9;
      t22 += v * b10;
      t23 += v * b11;
      t24 += v * b12;
      t25 += v * b13;
      t26 += v * b14;
      t27 += v * b15;
      v = a[13];
      t13 += v * b0;
      t14 += v * b1;
      t15 += v * b2;
      t16 += v * b3;
      t17 += v * b4;
      t18 += v * b5;
      t19 += v * b6;
      t20 += v * b7;
      t21 += v * b8;
      t22 += v * b9;
      t23 += v * b10;
      t24 += v * b11;
      t25 += v * b12;
      t26 += v * b13;
      t27 += v * b14;
      t28 += v * b15;
      v = a[14];
      t14 += v * b0;
      t15 += v * b1;
      t16 += v * b2;
      t17 += v * b3;
      t18 += v * b4;
      t19 += v * b5;
      t20 += v * b6;
      t21 += v * b7;
      t22 += v * b8;
      t23 += v * b9;
      t24 += v * b10;
      t25 += v * b11;
      t26 += v * b12;
      t27 += v * b13;
      t28 += v * b14;
      t29 += v * b15;
      v = a[15];
      t15 += v * b0;
      t16 += v * b1;
      t17 += v * b2;
      t18 += v * b3;
      t19 += v * b4;
      t20 += v * b5;
      t21 += v * b6;
      t22 += v * b7;
      t23 += v * b8;
      t24 += v * b9;
      t25 += v * b10;
      t26 += v * b11;
      t27 += v * b12;
      t28 += v * b13;
      t29 += v * b14;
      t30 += v * b15;
      t0 += 38 * t16;
      t1 += 38 * t17;
      t2 += 38 * t18;
      t3 += 38 * t19;
      t4 += 38 * t20;
      t5 += 38 * t21;
      t6 += 38 * t22;
      t7 += 38 * t23;
      t8 += 38 * t24;
      t9 += 38 * t25;
      t10 += 38 * t26;
      t11 += 38 * t27;
      t12 += 38 * t28;
      t13 += 38 * t29;
      t14 += 38 * t30;
      c = 1;
      v = t0 + c + 65535;
      c = Math.floor(v / 65536);
      t0 = v - c * 65536;
      v = t1 + c + 65535;
      c = Math.floor(v / 65536);
      t1 = v - c * 65536;
      v = t2 + c + 65535;
      c = Math.floor(v / 65536);
      t2 = v - c * 65536;
      v = t3 + c + 65535;
      c = Math.floor(v / 65536);
      t3 = v - c * 65536;
      v = t4 + c + 65535;
      c = Math.floor(v / 65536);
      t4 = v - c * 65536;
      v = t5 + c + 65535;
      c = Math.floor(v / 65536);
      t5 = v - c * 65536;
      v = t6 + c + 65535;
      c = Math.floor(v / 65536);
      t6 = v - c * 65536;
      v = t7 + c + 65535;
      c = Math.floor(v / 65536);
      t7 = v - c * 65536;
      v = t8 + c + 65535;
      c = Math.floor(v / 65536);
      t8 = v - c * 65536;
      v = t9 + c + 65535;
      c = Math.floor(v / 65536);
      t9 = v - c * 65536;
      v = t10 + c + 65535;
      c = Math.floor(v / 65536);
      t10 = v - c * 65536;
      v = t11 + c + 65535;
      c = Math.floor(v / 65536);
      t11 = v - c * 65536;
      v = t12 + c + 65535;
      c = Math.floor(v / 65536);
      t12 = v - c * 65536;
      v = t13 + c + 65535;
      c = Math.floor(v / 65536);
      t13 = v - c * 65536;
      v = t14 + c + 65535;
      c = Math.floor(v / 65536);
      t14 = v - c * 65536;
      v = t15 + c + 65535;
      c = Math.floor(v / 65536);
      t15 = v - c * 65536;
      t0 += c - 1 + 37 * (c - 1);
      c = 1;
      v = t0 + c + 65535;
      c = Math.floor(v / 65536);
      t0 = v - c * 65536;
      v = t1 + c + 65535;
      c = Math.floor(v / 65536);
      t1 = v - c * 65536;
      v = t2 + c + 65535;
      c = Math.floor(v / 65536);
      t2 = v - c * 65536;
      v = t3 + c + 65535;
      c = Math.floor(v / 65536);
      t3 = v - c * 65536;
      v = t4 + c + 65535;
      c = Math.floor(v / 65536);
      t4 = v - c * 65536;
      v = t5 + c + 65535;
      c = Math.floor(v / 65536);
      t5 = v - c * 65536;
      v = t6 + c + 65535;
      c = Math.floor(v / 65536);
      t6 = v - c * 65536;
      v = t7 + c + 65535;
      c = Math.floor(v / 65536);
      t7 = v - c * 65536;
      v = t8 + c + 65535;
      c = Math.floor(v / 65536);
      t8 = v - c * 65536;
      v = t9 + c + 65535;
      c = Math.floor(v / 65536);
      t9 = v - c * 65536;
      v = t10 + c + 65535;
      c = Math.floor(v / 65536);
      t10 = v - c * 65536;
      v = t11 + c + 65535;
      c = Math.floor(v / 65536);
      t11 = v - c * 65536;
      v = t12 + c + 65535;
      c = Math.floor(v / 65536);
      t12 = v - c * 65536;
      v = t13 + c + 65535;
      c = Math.floor(v / 65536);
      t13 = v - c * 65536;
      v = t14 + c + 65535;
      c = Math.floor(v / 65536);
      t14 = v - c * 65536;
      v = t15 + c + 65535;
      c = Math.floor(v / 65536);
      t15 = v - c * 65536;
      t0 += c - 1 + 37 * (c - 1);
      o[0] = t0;
      o[1] = t1;
      o[2] = t2;
      o[3] = t3;
      o[4] = t4;
      o[5] = t5;
      o[6] = t6;
      o[7] = t7;
      o[8] = t8;
      o[9] = t9;
      o[10] = t10;
      o[11] = t11;
      o[12] = t12;
      o[13] = t13;
      o[14] = t14;
      o[15] = t15;
    }
    function square(o, a) {
      mul(o, a, a);
    }
    function inv25519(o, inp) {
      const c = gf();
      for (let i = 0; i < 16; i++) {
        c[i] = inp[i];
      }
      for (let i = 253; i >= 0; i--) {
        square(c, c);
        if (i !== 2 && i !== 4) {
          mul(c, c, inp);
        }
      }
      for (let i = 0; i < 16; i++) {
        o[i] = c[i];
      }
    }
    function scalarMult(n, p) {
      const z = new Uint8Array(32);
      const x = new Float64Array(80);
      const a = gf(), b = gf(), c = gf(), d = gf(), e = gf(), f = gf();
      for (let i = 0; i < 31; i++) {
        z[i] = n[i];
      }
      z[31] = n[31] & 127 | 64;
      z[0] &= 248;
      unpack25519(x, p);
      for (let i = 0; i < 16; i++) {
        b[i] = x[i];
      }
      a[0] = d[0] = 1;
      for (let i = 254; i >= 0; --i) {
        const r = z[i >>> 3] >>> (i & 7) & 1;
        sel25519(a, b, r);
        sel25519(c, d, r);
        add(e, a, c);
        sub(a, a, c);
        add(c, b, d);
        sub(b, b, d);
        square(d, e);
        square(f, a);
        mul(a, c, a);
        mul(c, b, e);
        add(e, a, c);
        sub(a, a, c);
        square(b, a);
        sub(c, d, f);
        mul(a, c, _121665);
        add(a, a, d);
        mul(c, c, a);
        mul(a, d, f);
        mul(d, b, x);
        square(b, e);
        sel25519(a, b, r);
        sel25519(c, d, r);
      }
      for (let i = 0; i < 16; i++) {
        x[i + 16] = a[i];
        x[i + 32] = c[i];
        x[i + 48] = b[i];
        x[i + 64] = d[i];
      }
      const x32 = x.subarray(32);
      const x16 = x.subarray(16);
      inv25519(x32, x32);
      mul(x16, x16, x32);
      const q = new Uint8Array(32);
      pack25519(q, x16);
      return q;
    }
    exports2.scalarMult = scalarMult;
    function scalarMultBase(n) {
      return scalarMult(n, _9);
    }
    exports2.scalarMultBase = scalarMultBase;
    function generateKeyPairFromSeed3(seed) {
      if (seed.length !== exports2.SECRET_KEY_LENGTH) {
        throw new Error(`x25519: seed must be ${exports2.SECRET_KEY_LENGTH} bytes`);
      }
      const secretKey = new Uint8Array(seed);
      const publicKey = scalarMultBase(secretKey);
      return {
        publicKey,
        secretKey
      };
    }
    exports2.generateKeyPairFromSeed = generateKeyPairFromSeed3;
    function generateKeyPair5(prng) {
      const seed = (0, random_1.randomBytes)(32, prng);
      const result = generateKeyPairFromSeed3(seed);
      (0, wipe_1.wipe)(seed);
      return result;
    }
    exports2.generateKeyPair = generateKeyPair5;
    function sharedKey2(mySecretKey, theirPublicKey, rejectZero = false) {
      if (mySecretKey.length !== exports2.PUBLIC_KEY_LENGTH) {
        throw new Error("X25519: incorrect secret key length");
      }
      if (theirPublicKey.length !== exports2.PUBLIC_KEY_LENGTH) {
        throw new Error("X25519: incorrect public key length");
      }
      const result = scalarMult(mySecretKey, theirPublicKey);
      if (rejectZero) {
        let zeros = 0;
        for (let i = 0; i < result.length; i++) {
          zeros |= result[i];
        }
        if (zeros === 0) {
          throw new Error("X25519: invalid shared key");
        }
      }
      return result;
    }
    exports2.sharedKey = sharedKey2;
  }
});

// ../../../node_modules/node-forge/lib/forge.js
var require_forge = __commonJS({
  "../../../node_modules/node-forge/lib/forge.js"(exports2, module2) {
    module2.exports = {
      // default options
      options: {
        usePureJavaScript: false
      }
    };
  }
});

// ../../../node_modules/node-forge/lib/baseN.js
var require_baseN = __commonJS({
  "../../../node_modules/node-forge/lib/baseN.js"(exports2, module2) {
    var api = {};
    module2.exports = api;
    var _reverseAlphabets = {};
    api.encode = function(input, alphabet5, maxline) {
      if (typeof alphabet5 !== "string") {
        throw new TypeError('"alphabet" must be a string.');
      }
      if (maxline !== void 0 && typeof maxline !== "number") {
        throw new TypeError('"maxline" must be a number.');
      }
      var output2 = "";
      if (!(input instanceof Uint8Array)) {
        output2 = _encodeWithByteBuffer(input, alphabet5);
      } else {
        var i = 0;
        var base7 = alphabet5.length;
        var first = alphabet5.charAt(0);
        var digits = [0];
        for (i = 0; i < input.length; ++i) {
          for (var j = 0, carry = input[i]; j < digits.length; ++j) {
            carry += digits[j] << 8;
            digits[j] = carry % base7;
            carry = carry / base7 | 0;
          }
          while (carry > 0) {
            digits.push(carry % base7);
            carry = carry / base7 | 0;
          }
        }
        for (i = 0; input[i] === 0 && i < input.length - 1; ++i) {
          output2 += first;
        }
        for (i = digits.length - 1; i >= 0; --i) {
          output2 += alphabet5[digits[i]];
        }
      }
      if (maxline) {
        var regex = new RegExp(".{1," + maxline + "}", "g");
        output2 = output2.match(regex).join("\r\n");
      }
      return output2;
    };
    api.decode = function(input, alphabet5) {
      if (typeof input !== "string") {
        throw new TypeError('"input" must be a string.');
      }
      if (typeof alphabet5 !== "string") {
        throw new TypeError('"alphabet" must be a string.');
      }
      var table2 = _reverseAlphabets[alphabet5];
      if (!table2) {
        table2 = _reverseAlphabets[alphabet5] = [];
        for (var i = 0; i < alphabet5.length; ++i) {
          table2[alphabet5.charCodeAt(i)] = i;
        }
      }
      input = input.replace(/\s/g, "");
      var base7 = alphabet5.length;
      var first = alphabet5.charAt(0);
      var bytes2 = [0];
      for (var i = 0; i < input.length; i++) {
        var value = table2[input.charCodeAt(i)];
        if (value === void 0) {
          return;
        }
        for (var j = 0, carry = value; j < bytes2.length; ++j) {
          carry += bytes2[j] * base7;
          bytes2[j] = carry & 255;
          carry >>= 8;
        }
        while (carry > 0) {
          bytes2.push(carry & 255);
          carry >>= 8;
        }
      }
      for (var k = 0; input[k] === first && k < input.length - 1; ++k) {
        bytes2.push(0);
      }
      if (typeof Buffer !== "undefined") {
        return Buffer.from(bytes2.reverse());
      }
      return new Uint8Array(bytes2.reverse());
    };
    function _encodeWithByteBuffer(input, alphabet5) {
      var i = 0;
      var base7 = alphabet5.length;
      var first = alphabet5.charAt(0);
      var digits = [0];
      for (i = 0; i < input.length(); ++i) {
        for (var j = 0, carry = input.at(i); j < digits.length; ++j) {
          carry += digits[j] << 8;
          digits[j] = carry % base7;
          carry = carry / base7 | 0;
        }
        while (carry > 0) {
          digits.push(carry % base7);
          carry = carry / base7 | 0;
        }
      }
      var output2 = "";
      for (i = 0; input.at(i) === 0 && i < input.length() - 1; ++i) {
        output2 += first;
      }
      for (i = digits.length - 1; i >= 0; --i) {
        output2 += alphabet5[digits[i]];
      }
      return output2;
    }
  }
});

// ../../../node_modules/node-forge/lib/util.js
var require_util = __commonJS({
  "../../../node_modules/node-forge/lib/util.js"(exports2, module2) {
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
    util2.ByteStringBuffer.prototype.putBytes = function(bytes2) {
      this.data += bytes2;
      this._optimizeConstructedString(bytes2.length);
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
      var bytes2 = "";
      do {
        n -= 8;
        bytes2 += String.fromCharCode(i >> n & 255);
      } while (n > 0);
      return this.putBytes(bytes2);
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
      var src5 = new Uint8Array(
        this.data.buffer,
        this.data.byteOffset,
        this.data.byteLength
      );
      var dst = new Uint8Array(this.length() + growSize);
      dst.set(src5);
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
    util2.DataBuffer.prototype.putBytes = function(bytes2, encoding) {
      if (util2.isArrayBufferView(bytes2)) {
        var src5 = new Uint8Array(bytes2.buffer, bytes2.byteOffset, bytes2.byteLength);
        var len = src5.byteLength - src5.byteOffset;
        this.accommodate(len);
        var dst = new Uint8Array(this.data.buffer, this.write);
        dst.set(src5);
        this.write += len;
        return this;
      }
      if (util2.isArrayBuffer(bytes2)) {
        var src5 = new Uint8Array(bytes2);
        this.accommodate(src5.byteLength);
        var dst = new Uint8Array(this.data.buffer);
        dst.set(src5, this.write);
        this.write += src5.byteLength;
        return this;
      }
      if (bytes2 instanceof util2.DataBuffer || typeof bytes2 === "object" && typeof bytes2.read === "number" && typeof bytes2.write === "number" && util2.isArrayBufferView(bytes2.data)) {
        var src5 = new Uint8Array(bytes2.data.byteLength, bytes2.read, bytes2.length());
        this.accommodate(src5.byteLength);
        var dst = new Uint8Array(bytes2.data.byteLength, this.write);
        dst.set(src5);
        this.write += src5.byteLength;
        return this;
      }
      if (bytes2 instanceof util2.ByteStringBuffer) {
        bytes2 = bytes2.data;
        encoding = "binary";
      }
      encoding = encoding || "binary";
      if (typeof bytes2 === "string") {
        var view;
        if (encoding === "hex") {
          this.accommodate(Math.ceil(bytes2.length / 2));
          view = new Uint8Array(this.data.buffer, this.write);
          this.write += util2.binary.hex.decode(bytes2, view, this.write);
          return this;
        }
        if (encoding === "base64") {
          this.accommodate(Math.ceil(bytes2.length / 4) * 3);
          view = new Uint8Array(this.data.buffer, this.write);
          this.write += util2.binary.base64.decode(bytes2, view, this.write);
          return this;
        }
        if (encoding === "utf8") {
          bytes2 = util2.encodeUtf8(bytes2);
          encoding = "binary";
        }
        if (encoding === "binary" || encoding === "raw") {
          this.accommodate(bytes2.length);
          view = new Uint8Array(this.data.buffer, this.write);
          this.write += util2.binary.raw.decode(view);
          return this;
        }
        if (encoding === "utf16") {
          this.accommodate(bytes2.length * 2);
          view = new Uint16Array(this.data.buffer, this.write);
          this.write += util2.text.utf16.encode(view);
          return this;
        }
        throw new Error("Invalid encoding: " + encoding);
      }
      throw Error("Invalid parameter: " + bytes2);
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
        var src5 = new Uint8Array(this.data.buffer, this.read);
        var dst = new Uint8Array(src5.byteLength);
        dst.set(src5);
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
    util2.bytesToHex = function(bytes2) {
      return util2.createBuffer(bytes2).toHex();
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
      var output2 = "";
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
          output2 += line.substr(0, maxline) + "\r\n";
          line = line.substr(maxline);
        }
      }
      output2 += line;
      return output2;
    };
    util2.decode64 = function(input) {
      input = input.replace(/[^A-Za-z0-9\+\/\=]/g, "");
      var output2 = "";
      var enc1, enc2, enc3, enc4;
      var i = 0;
      while (i < input.length) {
        enc1 = _base64Idx[input.charCodeAt(i++) - 43];
        enc2 = _base64Idx[input.charCodeAt(i++) - 43];
        enc3 = _base64Idx[input.charCodeAt(i++) - 43];
        enc4 = _base64Idx[input.charCodeAt(i++) - 43];
        output2 += String.fromCharCode(enc1 << 2 | enc2 >> 4);
        if (enc3 !== 64) {
          output2 += String.fromCharCode((enc2 & 15) << 4 | enc3 >> 2);
          if (enc4 !== 64) {
            output2 += String.fromCharCode((enc3 & 3) << 6 | enc4);
          }
        }
      }
      return output2;
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
    util2.binary.raw.encode = function(bytes2) {
      return String.fromCharCode.apply(null, bytes2);
    };
    util2.binary.raw.decode = function(str, output2, offset) {
      var out = output2;
      if (!out) {
        out = new Uint8Array(str.length);
      }
      offset = offset || 0;
      var j = offset;
      for (var i = 0; i < str.length; ++i) {
        out[j++] = str.charCodeAt(i);
      }
      return output2 ? j - offset : out;
    };
    util2.binary.hex.encode = util2.bytesToHex;
    util2.binary.hex.decode = function(hex, output2, offset) {
      var out = output2;
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
      return output2 ? j - offset : out;
    };
    util2.binary.base64.encode = function(input, maxline) {
      var line = "";
      var output2 = "";
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
          output2 += line.substr(0, maxline) + "\r\n";
          line = line.substr(maxline);
        }
      }
      output2 += line;
      return output2;
    };
    util2.binary.base64.decode = function(input, output2, offset) {
      var out = output2;
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
      return output2 ? j - offset : out.subarray(0, j);
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
    util2.text.utf8.encode = function(str, output2, offset) {
      str = util2.encodeUtf8(str);
      var out = output2;
      if (!out) {
        out = new Uint8Array(str.length);
      }
      offset = offset || 0;
      var j = offset;
      for (var i = 0; i < str.length; ++i) {
        out[j++] = str.charCodeAt(i);
      }
      return output2 ? j - offset : out;
    };
    util2.text.utf8.decode = function(bytes2) {
      return util2.decodeUtf8(String.fromCharCode.apply(null, bytes2));
    };
    util2.text.utf16.encode = function(str, output2, offset) {
      var out = output2;
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
      return output2 ? j - offset : out;
    };
    util2.text.utf16.decode = function(bytes2) {
      return String.fromCharCode.apply(null, new Uint16Array(bytes2.buffer));
    };
    util2.deflate = function(api, bytes2, raw) {
      bytes2 = util2.decode64(api.deflate(util2.encode64(bytes2)).rval);
      if (raw) {
        var start = 2;
        var flg = bytes2.charCodeAt(1);
        if (flg & 32) {
          start = 6;
        }
        bytes2 = bytes2.substring(start, bytes2.length - 4);
      }
      return bytes2;
    };
    util2.inflate = function(api, bytes2, raw) {
      var rval = api.inflate(util2.encode64(bytes2)).rval;
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
        var empty5 = true;
        for (var prop in obj) {
          empty5 = false;
          break;
        }
        if (empty5) {
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
    util2.format = function(format4) {
      var re = /%./g;
      var match;
      var part;
      var argi = 0;
      var parts = [];
      var last = 0;
      while (match = re.exec(format4)) {
        part = format4.substring(last, re.lastIndex - 2);
        if (part.length > 0) {
          parts.push(part);
        }
        last = re.lastIndex;
        var code5 = match[0][1];
        switch (code5) {
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
            parts.push("<%" + code5 + "?>");
        }
      }
      parts.push(format4.substring(last));
      return parts.join("");
    };
    util2.formatNumber = function(number2, decimals, dec_point, thousands_sep) {
      var n = number2, c = isNaN(decimals = Math.abs(decimals)) ? 2 : decimals;
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
        var bytes2 = util2.hexToBytes(ip[i]);
        if (bytes2.length < 2) {
          b.putByte(0);
        }
        b.putBytes(bytes2);
      }
      return b.getBytes();
    };
    util2.bytesToIP = function(bytes2) {
      if (bytes2.length === 4) {
        return util2.bytesToIPv4(bytes2);
      }
      if (bytes2.length === 16) {
        return util2.bytesToIPv6(bytes2);
      }
      return null;
    };
    util2.bytesToIPv4 = function(bytes2) {
      if (bytes2.length !== 4) {
        return null;
      }
      var ip = [];
      for (var i = 0; i < bytes2.length; ++i) {
        ip.push(bytes2.charCodeAt(i));
      }
      return ip.join(".");
    };
    util2.bytesToIPv6 = function(bytes2) {
      if (bytes2.length !== 16) {
        return null;
      }
      var ip = [];
      var zeroGroups = [];
      var zeroMaxGroup = 0;
      for (var i = 0; i < bytes2.length; i += 2) {
        var hex = util2.bytesToHex(bytes2[i] + bytes2[i + 1]);
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

// ../../../node_modules/node-forge/lib/oids.js
var require_oids = __commonJS({
  "../../../node_modules/node-forge/lib/oids.js"(exports2, module2) {
    var forge6 = require_forge();
    forge6.pki = forge6.pki || {};
    var oids = module2.exports = forge6.pki.oids = forge6.oids = forge6.oids || {};
    function _IN(id, name5) {
      oids[id] = name5;
      oids[name5] = id;
    }
    function _I_(id, name5) {
      oids[id] = name5;
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

// ../../../node_modules/node-forge/lib/asn1.js
var require_asn1 = __commonJS({
  "../../../node_modules/node-forge/lib/asn1.js"(exports2, module2) {
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
      var length5;
      var longForm = b2 & 128;
      if (!longForm) {
        length5 = b2;
      } else {
        length5 = b.getInt((b2 & 127) << 3);
      }
      return length5;
    };
    function _checkBufferLength(bytes2, remaining, n) {
      if (n > remaining) {
        var error = new Error("Too few bytes to parse DER.");
        error.available = bytes2.length();
        error.remaining = remaining;
        error.requested = n;
        throw error;
      }
    }
    var _getValueLength = function(bytes2, remaining) {
      var b2 = bytes2.getByte();
      remaining--;
      if (b2 === 128) {
        return void 0;
      }
      var length5;
      var longForm = b2 & 128;
      if (!longForm) {
        length5 = b2;
      } else {
        var longFormBytes = b2 & 127;
        _checkBufferLength(bytes2, remaining, longFormBytes);
        length5 = bytes2.getInt(longFormBytes << 3);
      }
      if (length5 < 0) {
        throw new Error("Negative length: " + length5);
      }
      return length5;
    };
    asn1.fromDer = function(bytes2, options) {
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
      if (typeof bytes2 === "string") {
        bytes2 = forge6.util.createBuffer(bytes2);
      }
      var byteCount = bytes2.length();
      var value = _fromDer(bytes2, bytes2.length(), 0, options);
      if (options.parseAllBytes && bytes2.length() !== 0) {
        var error = new Error("Unparsed DER bytes remain after ASN.1 parsing.");
        error.byteCount = byteCount;
        error.remaining = bytes2.length();
        throw error;
      }
      return value;
    };
    function _fromDer(bytes2, remaining, depth, options) {
      var start;
      _checkBufferLength(bytes2, remaining, 2);
      var b1 = bytes2.getByte();
      remaining--;
      var tagClass = b1 & 192;
      var type = b1 & 31;
      start = bytes2.length();
      var length5 = _getValueLength(bytes2, remaining);
      remaining -= start - bytes2.length();
      if (length5 !== void 0 && length5 > remaining) {
        if (options.strict) {
          var error = new Error("Too few bytes to read ASN.1 value.");
          error.available = bytes2.length();
          error.remaining = remaining;
          error.requested = length5;
          throw error;
        }
        length5 = remaining;
      }
      var value;
      var bitStringContents;
      var constructed = (b1 & 32) === 32;
      if (constructed) {
        value = [];
        if (length5 === void 0) {
          for (; ; ) {
            _checkBufferLength(bytes2, remaining, 2);
            if (bytes2.bytes(2) === String.fromCharCode(0, 0)) {
              bytes2.getBytes(2);
              remaining -= 2;
              break;
            }
            start = bytes2.length();
            value.push(_fromDer(bytes2, remaining, depth + 1, options));
            remaining -= start - bytes2.length();
          }
        } else {
          while (length5 > 0) {
            start = bytes2.length();
            value.push(_fromDer(bytes2, length5, depth + 1, options));
            remaining -= start - bytes2.length();
            length5 -= start - bytes2.length();
          }
        }
      }
      if (value === void 0 && tagClass === asn1.Class.UNIVERSAL && type === asn1.Type.BITSTRING) {
        bitStringContents = bytes2.bytes(length5);
      }
      if (value === void 0 && options.decodeBitStrings && tagClass === asn1.Class.UNIVERSAL && // FIXME: OCTET STRINGs not yet supported here
      // .. other parts of forge expect to decode OCTET STRINGs manually
      type === asn1.Type.BITSTRING && length5 > 1) {
        var savedRead = bytes2.read;
        var savedRemaining = remaining;
        var unused = 0;
        if (type === asn1.Type.BITSTRING) {
          _checkBufferLength(bytes2, remaining, 1);
          unused = bytes2.getByte();
          remaining--;
        }
        if (unused === 0) {
          try {
            start = bytes2.length();
            var subOptions = {
              // enforce strict mode to avoid parsing ASN.1 from plain data
              strict: true,
              decodeBitStrings: true
            };
            var composed = _fromDer(bytes2, remaining, depth + 1, subOptions);
            var used = start - bytes2.length();
            remaining -= used;
            if (type == asn1.Type.BITSTRING) {
              used++;
            }
            var tc = composed.tagClass;
            if (used === length5 && (tc === asn1.Class.UNIVERSAL || tc === asn1.Class.CONTEXT_SPECIFIC)) {
              value = [composed];
            }
          } catch (ex) {
          }
        }
        if (value === void 0) {
          bytes2.read = savedRead;
          remaining = savedRemaining;
        }
      }
      if (value === void 0) {
        if (length5 === void 0) {
          if (options.strict) {
            throw new Error("Non-constructed ASN.1 object of indefinite length.");
          }
          length5 = remaining;
        }
        if (type === asn1.Type.BMPSTRING) {
          value = "";
          for (; length5 > 0; length5 -= 2) {
            _checkBufferLength(bytes2, remaining, 2);
            value += String.fromCharCode(bytes2.getInt16());
            remaining -= 2;
          }
        } else {
          value = bytes2.getBytes(length5);
          remaining -= length5;
        }
      }
      var asn1Options = bitStringContents === void 0 ? null : {
        bitStringContents
      };
      return asn1.create(tagClass, type, constructed, value, asn1Options);
    }
    asn1.toDer = function(obj) {
      var bytes2 = forge6.util.createBuffer();
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
      bytes2.putByte(b1);
      if (value.length() <= 127) {
        bytes2.putByte(value.length() & 127);
      } else {
        var len = value.length();
        var lenBytes = "";
        do {
          lenBytes += String.fromCharCode(len & 255);
          len = len >>> 8;
        } while (len > 0);
        bytes2.putByte(lenBytes.length | 128);
        for (var i = lenBytes.length - 1; i >= 0; --i) {
          bytes2.putByte(lenBytes.charCodeAt(i));
        }
      }
      bytes2.putBuffer(value);
      return bytes2;
    };
    asn1.oidToDer = function(oid) {
      var values = oid.split(".");
      var bytes2 = forge6.util.createBuffer();
      bytes2.putByte(40 * parseInt(values[0], 10) + parseInt(values[1], 10));
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
          bytes2.putByte(valueBytes[n]);
        }
      }
      return bytes2;
    };
    asn1.derToOid = function(bytes2) {
      var oid;
      if (typeof bytes2 === "string") {
        bytes2 = forge6.util.createBuffer(bytes2);
      }
      var b = bytes2.getByte();
      oid = Math.floor(b / 40) + "." + b % 40;
      var value = 0;
      while (bytes2.length() > 0) {
        b = bytes2.getByte();
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
      var format4 = [];
      format4.push(("" + date.getUTCFullYear()).substr(2));
      format4.push("" + (date.getUTCMonth() + 1));
      format4.push("" + date.getUTCDate());
      format4.push("" + date.getUTCHours());
      format4.push("" + date.getUTCMinutes());
      format4.push("" + date.getUTCSeconds());
      for (var i = 0; i < format4.length; ++i) {
        if (format4[i].length < 2) {
          rval += "0";
        }
        rval += format4[i];
      }
      rval += "Z";
      return rval;
    };
    asn1.dateToGeneralizedTime = function(date) {
      if (typeof date === "string") {
        return date;
      }
      var rval = "";
      var format4 = [];
      format4.push("" + date.getUTCFullYear());
      format4.push("" + (date.getUTCMonth() + 1));
      format4.push("" + date.getUTCDate());
      format4.push("" + date.getUTCHours());
      format4.push("" + date.getUTCMinutes());
      format4.push("" + date.getUTCSeconds());
      for (var i = 0; i < format4.length; ++i) {
        if (format4[i].length < 2) {
          rval += "0";
        }
        rval += format4[i];
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
    asn1.derToInteger = function(bytes2) {
      if (typeof bytes2 === "string") {
        bytes2 = forge6.util.createBuffer(bytes2);
      }
      var n = bytes2.length() * 8;
      if (n > 32) {
        throw new Error("Integer too large; max is 32-bits.");
      }
      return bytes2.getSignedInt(n);
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

// ../../../node_modules/node-forge/lib/cipher.js
var require_cipher = __commonJS({
  "../../../node_modules/node-forge/lib/cipher.js"(exports2, module2) {
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
    forge6.cipher.registerAlgorithm = function(name5, algorithm) {
      name5 = name5.toUpperCase();
      forge6.cipher.algorithms[name5] = algorithm;
    };
    forge6.cipher.getAlgorithm = function(name5) {
      name5 = name5.toUpperCase();
      if (name5 in forge6.cipher.algorithms) {
        return forge6.cipher.algorithms[name5];
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
        this.mode.unpad = function(output2) {
          return pad(this.blockSize, output2, true);
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

// ../../../node_modules/node-forge/lib/cipherModes.js
var require_cipherModes = __commonJS({
  "../../../node_modules/node-forge/lib/cipherModes.js"(exports2, module2) {
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
    modes.ecb.prototype.encrypt = function(input, output2, finish) {
      if (input.length() < this.blockSize && !(finish && input.length() > 0)) {
        return true;
      }
      for (var i = 0; i < this._ints; ++i) {
        this._inBlock[i] = input.getInt32();
      }
      this.cipher.encrypt(this._inBlock, this._outBlock);
      for (var i = 0; i < this._ints; ++i) {
        output2.putInt32(this._outBlock[i]);
      }
    };
    modes.ecb.prototype.decrypt = function(input, output2, finish) {
      if (input.length() < this.blockSize && !(finish && input.length() > 0)) {
        return true;
      }
      for (var i = 0; i < this._ints; ++i) {
        this._inBlock[i] = input.getInt32();
      }
      this.cipher.decrypt(this._inBlock, this._outBlock);
      for (var i = 0; i < this._ints; ++i) {
        output2.putInt32(this._outBlock[i]);
      }
    };
    modes.ecb.prototype.pad = function(input, options) {
      var padding = input.length() === this.blockSize ? this.blockSize : this.blockSize - input.length();
      input.fillWithByte(padding, padding);
      return true;
    };
    modes.ecb.prototype.unpad = function(output2, options) {
      if (options.overflow > 0) {
        return false;
      }
      var len = output2.length();
      var count = output2.at(len - 1);
      if (count > this.blockSize << 2) {
        return false;
      }
      output2.truncate(count);
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
    modes.cbc.prototype.encrypt = function(input, output2, finish) {
      if (input.length() < this.blockSize && !(finish && input.length() > 0)) {
        return true;
      }
      for (var i = 0; i < this._ints; ++i) {
        this._inBlock[i] = this._prev[i] ^ input.getInt32();
      }
      this.cipher.encrypt(this._inBlock, this._outBlock);
      for (var i = 0; i < this._ints; ++i) {
        output2.putInt32(this._outBlock[i]);
      }
      this._prev = this._outBlock;
    };
    modes.cbc.prototype.decrypt = function(input, output2, finish) {
      if (input.length() < this.blockSize && !(finish && input.length() > 0)) {
        return true;
      }
      for (var i = 0; i < this._ints; ++i) {
        this._inBlock[i] = input.getInt32();
      }
      this.cipher.decrypt(this._inBlock, this._outBlock);
      for (var i = 0; i < this._ints; ++i) {
        output2.putInt32(this._prev[i] ^ this._outBlock[i]);
      }
      this._prev = this._inBlock.slice(0);
    };
    modes.cbc.prototype.pad = function(input, options) {
      var padding = input.length() === this.blockSize ? this.blockSize : this.blockSize - input.length();
      input.fillWithByte(padding, padding);
      return true;
    };
    modes.cbc.prototype.unpad = function(output2, options) {
      if (options.overflow > 0) {
        return false;
      }
      var len = output2.length();
      var count = output2.at(len - 1);
      if (count > this.blockSize << 2) {
        return false;
      }
      output2.truncate(count);
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
    modes.cfb.prototype.encrypt = function(input, output2, finish) {
      var inputLength = input.length();
      if (inputLength === 0) {
        return true;
      }
      this.cipher.encrypt(this._inBlock, this._outBlock);
      if (this._partialBytes === 0 && inputLength >= this.blockSize) {
        for (var i = 0; i < this._ints; ++i) {
          this._inBlock[i] = input.getInt32() ^ this._outBlock[i];
          output2.putInt32(this._inBlock[i]);
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
        output2.putBytes(this._partialOutput.getBytes(
          partialBytes - this._partialBytes
        ));
        this._partialBytes = partialBytes;
        return true;
      }
      output2.putBytes(this._partialOutput.getBytes(
        inputLength - this._partialBytes
      ));
      this._partialBytes = 0;
    };
    modes.cfb.prototype.decrypt = function(input, output2, finish) {
      var inputLength = input.length();
      if (inputLength === 0) {
        return true;
      }
      this.cipher.encrypt(this._inBlock, this._outBlock);
      if (this._partialBytes === 0 && inputLength >= this.blockSize) {
        for (var i = 0; i < this._ints; ++i) {
          this._inBlock[i] = input.getInt32();
          output2.putInt32(this._inBlock[i] ^ this._outBlock[i]);
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
        output2.putBytes(this._partialOutput.getBytes(
          partialBytes - this._partialBytes
        ));
        this._partialBytes = partialBytes;
        return true;
      }
      output2.putBytes(this._partialOutput.getBytes(
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
    modes.ofb.prototype.encrypt = function(input, output2, finish) {
      var inputLength = input.length();
      if (input.length() === 0) {
        return true;
      }
      this.cipher.encrypt(this._inBlock, this._outBlock);
      if (this._partialBytes === 0 && inputLength >= this.blockSize) {
        for (var i = 0; i < this._ints; ++i) {
          output2.putInt32(input.getInt32() ^ this._outBlock[i]);
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
        output2.putBytes(this._partialOutput.getBytes(
          partialBytes - this._partialBytes
        ));
        this._partialBytes = partialBytes;
        return true;
      }
      output2.putBytes(this._partialOutput.getBytes(
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
    modes.ctr.prototype.encrypt = function(input, output2, finish) {
      var inputLength = input.length();
      if (inputLength === 0) {
        return true;
      }
      this.cipher.encrypt(this._inBlock, this._outBlock);
      if (this._partialBytes === 0 && inputLength >= this.blockSize) {
        for (var i = 0; i < this._ints; ++i) {
          output2.putInt32(input.getInt32() ^ this._outBlock[i]);
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
          output2.putBytes(this._partialOutput.getBytes(
            partialBytes - this._partialBytes
          ));
          this._partialBytes = partialBytes;
          return true;
        }
        output2.putBytes(this._partialOutput.getBytes(
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
    modes.gcm.prototype.encrypt = function(input, output2, finish) {
      var inputLength = input.length();
      if (inputLength === 0) {
        return true;
      }
      this.cipher.encrypt(this._inBlock, this._outBlock);
      if (this._partialBytes === 0 && inputLength >= this.blockSize) {
        for (var i = 0; i < this._ints; ++i) {
          output2.putInt32(this._outBlock[i] ^= input.getInt32());
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
          output2.putBytes(this._partialOutput.getBytes(
            partialBytes - this._partialBytes
          ));
          this._partialBytes = partialBytes;
          return true;
        }
        output2.putBytes(this._partialOutput.getBytes(
          inputLength - this._partialBytes
        ));
        this._partialBytes = 0;
      }
      this._s = this.ghash(this._hashSubkey, this._s, this._outBlock);
      inc32(this._inBlock);
    };
    modes.gcm.prototype.decrypt = function(input, output2, finish) {
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
        output2.putInt32(this._outBlock[i] ^ this._hashBlock[i]);
      }
      if (inputLength < this.blockSize) {
        this._cipherLength += inputLength % this.blockSize;
      } else {
        this._cipherLength += this.blockSize;
      }
    };
    modes.gcm.prototype.afterFinish = function(output2, options) {
      var rval = true;
      if (options.decrypt && options.overflow) {
        output2.truncate(this.blockSize - options.overflow);
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

// ../../../node_modules/node-forge/lib/aes.js
var require_aes = __commonJS({
  "../../../node_modules/node-forge/lib/aes.js"(exports2, module2) {
    var forge6 = require_forge();
    require_cipher();
    require_cipherModes();
    require_util();
    module2.exports = forge6.aes = forge6.aes || {};
    forge6.aes.startEncrypting = function(key, iv, output2, mode) {
      var cipher = _createCipher({
        key,
        output: output2,
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
    forge6.aes.startDecrypting = function(key, iv, output2, mode) {
      var cipher = _createCipher({
        key,
        output: output2,
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
    forge6.aes.Algorithm = function(name5, mode) {
      if (!init) {
        initialize();
      }
      var self2 = this;
      self2.name = name5;
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
    function registerAlgorithm(name5, mode) {
      var factory = function() {
        return new forge6.aes.Algorithm(name5, mode);
      };
      forge6.cipher.registerAlgorithm(name5, factory);
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
    function _updateBlock(w, input, output2, decrypt2) {
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
      output2[0] = sub[a >>> 24] << 24 ^ sub[b >>> 16 & 255] << 16 ^ sub[c >>> 8 & 255] << 8 ^ sub[d & 255] ^ w[++i];
      output2[decrypt2 ? 3 : 1] = sub[b >>> 24] << 24 ^ sub[c >>> 16 & 255] << 16 ^ sub[d >>> 8 & 255] << 8 ^ sub[a & 255] ^ w[++i];
      output2[2] = sub[c >>> 24] << 24 ^ sub[d >>> 16 & 255] << 16 ^ sub[a >>> 8 & 255] << 8 ^ sub[b & 255] ^ w[++i];
      output2[decrypt2 ? 1 : 3] = sub[d >>> 24] << 24 ^ sub[a >>> 16 & 255] << 16 ^ sub[b >>> 8 & 255] << 8 ^ sub[c & 255] ^ w[++i];
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
        var output2 = null;
        if (options2 instanceof forge6.util.ByteBuffer) {
          output2 = options2;
          options2 = {};
        }
        options2 = options2 || {};
        options2.output = output2;
        options2.iv = iv;
        start.call(cipher, options2);
      };
      return cipher;
    }
  }
});

// ../../../node_modules/node-forge/lib/des.js
var require_des = __commonJS({
  "../../../node_modules/node-forge/lib/des.js"(exports2, module2) {
    var forge6 = require_forge();
    require_cipher();
    require_cipherModes();
    require_util();
    module2.exports = forge6.des = forge6.des || {};
    forge6.des.startEncrypting = function(key, iv, output2, mode) {
      var cipher = _createCipher({
        key,
        output: output2,
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
    forge6.des.startDecrypting = function(key, iv, output2, mode) {
      var cipher = _createCipher({
        key,
        output: output2,
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
    forge6.des.Algorithm = function(name5, mode) {
      var self2 = this;
      self2.name = name5;
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
    function registerAlgorithm(name5, mode) {
      var factory = function() {
        return new forge6.des.Algorithm(name5, mode);
      };
      forge6.cipher.registerAlgorithm(name5, factory);
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
    function _updateBlock(keys, input, output2, decrypt2) {
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
      output2[0] = left;
      output2[1] = right;
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
        var output2 = null;
        if (options2 instanceof forge6.util.ByteBuffer) {
          output2 = options2;
          options2 = {};
        }
        options2 = options2 || {};
        options2.output = output2;
        options2.iv = iv;
        start.call(cipher, options2);
      };
      return cipher;
    }
  }
});

// ../../../node_modules/node-forge/lib/md.js
var require_md = __commonJS({
  "../../../node_modules/node-forge/lib/md.js"(exports2, module2) {
    var forge6 = require_forge();
    module2.exports = forge6.md = forge6.md || {};
    forge6.md.algorithms = forge6.md.algorithms || {};
  }
});

// ../../../node_modules/node-forge/lib/hmac.js
var require_hmac = __commonJS({
  "../../../node_modules/node-forge/lib/hmac.js"(exports2, module2) {
    var forge6 = require_forge();
    require_md();
    require_util();
    var hmac2 = module2.exports = forge6.hmac = forge6.hmac || {};
    hmac2.create = function() {
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
      ctx.update = function(bytes2) {
        _md.update(bytes2);
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

// ../../../node_modules/node-forge/lib/pbkdf2.js
var require_pbkdf2 = __commonJS({
  "../../../node_modules/node-forge/lib/pbkdf2.js"(exports2, module2) {
    var forge6 = require_forge();
    require_hmac();
    require_md();
    require_util();
    var pkcs5 = forge6.pkcs5 = forge6.pkcs5 || {};
    var crypto5;
    if (forge6.util.isNodejs && !forge6.options.usePureJavaScript) {
      crypto5 = require_crypto();
    }
    module2.exports = forge6.pbkdf2 = pkcs5.pbkdf2 = function(p, s, c, dkLen, md, callback) {
      if (typeof md === "function") {
        callback = md;
        md = null;
      }
      if (forge6.util.isNodejs && !forge6.options.usePureJavaScript && crypto5.pbkdf2 && (md === null || typeof md !== "object") && (crypto5.pbkdf2Sync.length > 4 || (!md || md === "sha1"))) {
        if (typeof md !== "string") {
          md = "sha1";
        }
        p = Buffer.from(p, "binary");
        s = Buffer.from(s, "binary");
        if (!callback) {
          if (crypto5.pbkdf2Sync.length === 4) {
            return crypto5.pbkdf2Sync(p, s, c, dkLen).toString("binary");
          }
          return crypto5.pbkdf2Sync(p, s, c, dkLen, md).toString("binary");
        }
        if (crypto5.pbkdf2Sync.length === 4) {
          return crypto5.pbkdf2(p, s, c, dkLen, function(err2, key) {
            if (err2) {
              return callback(err2);
            }
            callback(null, key.toString("binary"));
          });
        }
        return crypto5.pbkdf2(p, s, c, dkLen, md, function(err2, key) {
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
      var xor2, u_c, u_c1;
      if (!callback) {
        for (var i = 1; i <= len; ++i) {
          prf.start(null, null);
          prf.update(s);
          prf.update(forge6.util.int32ToBytes(i));
          xor2 = u_c1 = prf.digest().getBytes();
          for (var j = 2; j <= c; ++j) {
            prf.start(null, null);
            prf.update(u_c1);
            u_c = prf.digest().getBytes();
            xor2 = forge6.util.xorBytes(xor2, u_c, hLen);
            u_c1 = u_c;
          }
          dk += i < len ? xor2 : xor2.substr(0, r);
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
        xor2 = u_c1 = prf.digest().getBytes();
        j = 2;
        inner();
      }
      function inner() {
        if (j <= c) {
          prf.start(null, null);
          prf.update(u_c1);
          u_c = prf.digest().getBytes();
          xor2 = forge6.util.xorBytes(xor2, u_c, hLen);
          u_c1 = u_c;
          ++j;
          return forge6.util.setImmediate(inner);
        }
        dk += i < len ? xor2 : xor2.substr(0, r);
        ++i;
        outer();
      }
      outer();
    };
  }
});

// ../../../node_modules/node-forge/lib/pem.js
var require_pem = __commonJS({
  "../../../node_modules/node-forge/lib/pem.js"(exports2, module2) {
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
      var length5 = 0;
      var candidate = -1;
      for (var i = 0; i < rval.length; ++i, ++length5) {
        if (length5 > 65 && candidate !== -1) {
          var insert = rval[candidate];
          if (insert === ",") {
            ++candidate;
            rval = rval.substr(0, candidate) + "\r\n " + rval.substr(candidate);
          } else {
            rval = rval.substr(0, candidate) + "\r\n" + insert + rval.substr(candidate + 1);
          }
          length5 = i - candidate - 1;
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

// ../../../node_modules/node-forge/lib/sha256.js
var require_sha256 = __commonJS({
  "../../../node_modules/node-forge/lib/sha256.js"(exports2, module2) {
    var forge6 = require_forge();
    require_md();
    require_util();
    var sha2566 = module2.exports = forge6.sha256 = forge6.sha256 || {};
    forge6.md.sha256 = forge6.md.algorithms.sha256 = sha2566;
    sha2566.create = function() {
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
    function _update(s, w, bytes2) {
      var t1, t2, s0, s1, ch, maj, i, a, b, c, d, e, f, g, h;
      var len = bytes2.length();
      while (len >= 64) {
        for (i = 0; i < 16; ++i) {
          w[i] = bytes2.getInt32();
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

// ../../../node_modules/node-forge/lib/prng.js
var require_prng = __commonJS({
  "../../../node_modules/node-forge/lib/prng.js"(exports2, module2) {
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
          var bytes2 = cipher(ctx.key, ctx.seed);
          ctx.generated += bytes2.length;
          b.putBytes(bytes2);
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
          var bytes2 = cipher(ctx.key, ctx.seed);
          ctx.generated += bytes2.length;
          b.putBytes(bytes2);
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
        ctx.seedFile(needed, function(err, bytes2) {
          if (err) {
            return callback(err);
          }
          ctx.collect(bytes2);
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
          _crypto.randomBytes(needed, function(err, bytes2) {
            if (err) {
              return callback(err);
            }
            callback(null, bytes2.toString());
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
      ctx.collect = function(bytes2) {
        var count = bytes2.length;
        for (var i2 = 0; i2 < count; ++i2) {
          ctx.pools[ctx.pool].update(bytes2.substr(i2, 1));
          ctx.pool = ctx.pool === 31 ? 0 : ctx.pool + 1;
        }
      };
      ctx.collectInt = function(i2, n) {
        var bytes2 = "";
        for (var x = 0; x < n; x += 8) {
          bytes2 += String.fromCharCode(i2 >> x & 255);
        }
        ctx.collect(bytes2);
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
              ctx.seedFile(data.forge.prng.needed, function(err, bytes2) {
                worker.postMessage({ forge: { prng: { err, bytes: bytes2 } } });
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

// ../../../node_modules/node-forge/lib/random.js
var require_random2 = __commonJS({
  "../../../node_modules/node-forge/lib/random.js"(exports2, module2) {
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

// ../../../node_modules/node-forge/lib/rc2.js
var require_rc2 = __commonJS({
  "../../../node_modules/node-forge/lib/rc2.js"(exports2, module2) {
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
        start: function(iv, output2) {
          if (iv) {
            if (typeof iv === "string") {
              iv = forge6.util.createBuffer(iv);
            }
          }
          _finish = false;
          _input = forge6.util.createBuffer();
          _output = output2 || new forge6.util.createBuffer();
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
    forge6.rc2.startEncrypting = function(key, iv, output2) {
      var cipher = forge6.rc2.createEncryptionCipher(key, 128);
      cipher.start(iv, output2);
      return cipher;
    };
    forge6.rc2.createEncryptionCipher = function(key, bits2) {
      return createCipher(key, bits2, true);
    };
    forge6.rc2.startDecrypting = function(key, iv, output2) {
      var cipher = forge6.rc2.createDecryptionCipher(key, 128);
      cipher.start(iv, output2);
      return cipher;
    };
    forge6.rc2.createDecryptionCipher = function(key, bits2) {
      return createCipher(key, bits2, false);
    };
  }
});

// ../../../node_modules/node-forge/lib/jsbn.js
var require_jsbn = __commonJS({
  "../../../node_modules/node-forge/lib/jsbn.js"(exports2, module2) {
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

// ../../../node_modules/node-forge/lib/sha1.js
var require_sha1 = __commonJS({
  "../../../node_modules/node-forge/lib/sha1.js"(exports2, module2) {
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
    function _update(s, w, bytes2) {
      var t, a, b, c, d, e, f, i;
      var len = bytes2.length();
      while (len >= 64) {
        a = s.h0;
        b = s.h1;
        c = s.h2;
        d = s.h3;
        e = s.h4;
        for (i = 0; i < 16; ++i) {
          t = bytes2.getInt32();
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

// ../../../node_modules/node-forge/lib/pkcs1.js
var require_pkcs1 = __commonJS({
  "../../../node_modules/node-forge/lib/pkcs1.js"(exports2, module2) {
    var forge6 = require_forge();
    require_util();
    require_random2();
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
        var code5 = db.charCodeAt(j);
        var is_0 = code5 & 1 ^ 1;
        var error_mask = in_ps ? 65534 : 0;
        error |= code5 & error_mask;
        in_ps = in_ps & is_0;
        index += in_ps;
      }
      if (error || db.charCodeAt(index) !== 1) {
        throw new Error("Invalid RSAES-OAEP padding.");
      }
      return db.substring(index + 1);
    };
    function rsa_mgf1(seed, maskLength, hash2) {
      if (!hash2) {
        hash2 = forge6.md.sha1.create();
      }
      var t = "";
      var count = Math.ceil(maskLength / hash2.digestLength);
      for (var i = 0; i < count; ++i) {
        var c = String.fromCharCode(
          i >> 24 & 255,
          i >> 16 & 255,
          i >> 8 & 255,
          i & 255
        );
        hash2.start();
        hash2.update(seed + c);
        t += hash2.digest().getBytes();
      }
      return t.substring(0, maskLength);
    }
  }
});

// ../../../node_modules/node-forge/lib/prime.js
var require_prime = __commonJS({
  "../../../node_modules/node-forge/lib/prime.js"(exports2, module2) {
    var forge6 = require_forge();
    require_util();
    require_jsbn();
    require_random2();
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

// ../../../node_modules/node-forge/lib/rsa.js
var require_rsa = __commonJS({
  "../../../node_modules/node-forge/lib/rsa.js"(exports2, module2) {
    var forge6 = require_forge();
    require_asn1();
    require_jsbn();
    require_oids();
    require_pkcs1();
    require_prime();
    require_random2();
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
      var digest5 = asn1.create(
        asn1.Class.UNIVERSAL,
        asn1.Type.OCTETSTRING,
        false,
        md.digest().getBytes()
      );
      digestInfo.value.push(digestAlgorithm);
      digestInfo.value.push(digest5);
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
            }, true, ["sign", "verify"]).then(function(pair2) {
              return util2.globalScope.crypto.subtle.exportKey(
                "pkcs8",
                pair2.privateKey
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
              var pair2 = e2.target.result;
              var exportOp = util2.globalScope.msCrypto.subtle.exportKey(
                "pkcs8",
                pair2.privateKey
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
      key.verify = function(digest5, signature, scheme, options) {
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
            verify: function(digest6, d2) {
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
              return digest6 === capture.digest;
            }
          };
        } else if (scheme === "NONE" || scheme === "NULL" || scheme === null) {
          scheme = {
            verify: function(digest6, d2) {
              d2 = _decodePkcs1_v1_5(d2, key, true);
              return digest6 === d2;
            }
          };
        }
        var d = pki.rsa.decrypt(signature, key, true, false);
        return scheme.verify(digest5, d, key.n.bitLength());
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
      var bytes2 = forge6.util.hexToBytes(hex);
      if (bytes2.length > 1 && // leading 0x00 for positive integer
      (bytes2.charCodeAt(0) === 0 && (bytes2.charCodeAt(1) & 128) === 0 || // leading 0xFF for negative integer
      bytes2.charCodeAt(0) === 255 && (bytes2.charCodeAt(1) & 128) === 128)) {
        return bytes2.substr(1);
      }
      return bytes2;
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
      var bytes2 = forge6.util.hexToBytes(x.toString(16));
      var buffer = new Uint8Array(bytes2.length);
      for (var i = 0; i < bytes2.length; ++i) {
        buffer[i] = bytes2.charCodeAt(i);
      }
      return buffer;
    }
  }
});

// ../../../node_modules/node-forge/lib/pbe.js
var require_pbe = __commonJS({
  "../../../node_modules/node-forge/lib/pbe.js"(exports2, module2) {
    var forge6 = require_forge();
    require_aes();
    require_asn1();
    require_des();
    require_md();
    require_oids();
    require_pbkdf2();
    require_pem();
    require_random2();
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
      var digests = [hash2(md, password + salt)];
      for (var length5 = 16, i = 1; length5 < dkLen; ++i, length5 += 16) {
        digests.push(hash2(md, digests[i - 1] + password + salt));
      }
      return digests.join("").substr(0, dkLen);
    };
    function hash2(md, bytes2) {
      return md.start().update(bytes2).digest().getBytes();
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

// ../../../node_modules/node-forge/lib/sha512.js
var require_sha512 = __commonJS({
  "../../../node_modules/node-forge/lib/sha512.js"(exports2, module2) {
    var forge6 = require_forge();
    require_md();
    require_util();
    var sha5125 = module2.exports = forge6.sha512 = forge6.sha512 || {};
    forge6.md.sha512 = forge6.md.algorithms.sha512 = sha5125;
    var sha384 = forge6.sha384 = forge6.sha512.sha384 = forge6.sha512.sha384 || {};
    sha384.create = function() {
      return sha5125.create("SHA-384");
    };
    forge6.md.sha384 = forge6.md.algorithms.sha384 = sha384;
    forge6.sha512.sha256 = forge6.sha512.sha256 || {
      create: function() {
        return sha5125.create("SHA-512/256");
      }
    };
    forge6.md["sha512/256"] = forge6.md.algorithms["sha512/256"] = forge6.sha512.sha256;
    forge6.sha512.sha224 = forge6.sha512.sha224 || {
      create: function() {
        return sha5125.create("SHA-512/224");
      }
    };
    forge6.md["sha512/224"] = forge6.md.algorithms["sha512/224"] = forge6.sha512.sha224;
    sha5125.create = function(algorithm) {
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
    function _update(s, w, bytes2) {
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
      var len = bytes2.length();
      while (len >= 128) {
        for (i = 0; i < 16; ++i) {
          w[i][0] = bytes2.getInt32() >>> 0;
          w[i][1] = bytes2.getInt32() >>> 0;
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

// ../../../node_modules/@multiformats/base-x/src/index.js
var require_src = __commonJS({
  "../../../node_modules/@multiformats/base-x/src/index.js"(exports2, module2) {
    "use strict";
    function base7(ALPHABET) {
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
      function encode19(source) {
        if (source instanceof Uint8Array) {
        } else if (ArrayBuffer.isView(source)) {
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
        var length5 = 0;
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
          for (var it1 = size - 1; (carry !== 0 || i2 < length5) && it1 !== -1; it1--, i2++) {
            carry += 256 * b58[it1] >>> 0;
            b58[it1] = carry % BASE >>> 0;
            carry = carry / BASE >>> 0;
          }
          if (carry !== 0) {
            throw new Error("Non-zero carry");
          }
          length5 = i2;
          pbegin++;
        }
        var it2 = size - length5;
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
        var length5 = 0;
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
          for (var it3 = size - 1; (carry !== 0 || i2 < length5) && it3 !== -1; it3--, i2++) {
            carry += BASE * b256[it3] >>> 0;
            b256[it3] = carry % 256 >>> 0;
            carry = carry / 256 >>> 0;
          }
          if (carry !== 0) {
            throw new Error("Non-zero carry");
          }
          length5 = i2;
          psz++;
        }
        if (source[psz] === " ") {
          return;
        }
        var it4 = size - length5;
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
      function decode24(string3) {
        var buffer = decodeUnsafe(string3);
        if (buffer) {
          return buffer;
        }
        throw new Error("Non-base" + BASE + " character");
      }
      return {
        encode: encode19,
        decodeUnsafe,
        decode: decode24
      };
    }
    module2.exports = base7;
  }
});

// ../../../node_modules/multibase/src/util.js
var require_util2 = __commonJS({
  "../../../node_modules/multibase/src/util.js"(exports2, module2) {
    "use strict";
    var textDecoder5 = new TextDecoder();
    var decodeText = (bytes2) => textDecoder5.decode(bytes2);
    var textEncoder5 = new TextEncoder();
    var encodeText = (text) => textEncoder5.encode(text);
    function concat3(arrs, length5) {
      const output2 = new Uint8Array(length5);
      let offset = 0;
      for (const arr of arrs) {
        output2.set(arr, offset);
        offset += arr.length;
      }
      return output2;
    }
    module2.exports = { decodeText, encodeText, concat: concat3 };
  }
});

// ../../../node_modules/multibase/src/base.js
var require_base = __commonJS({
  "../../../node_modules/multibase/src/base.js"(exports2, module2) {
    "use strict";
    var { encodeText } = require_util2();
    var Base = class {
      /**
       * @param {BaseName} name
       * @param {BaseCode} code
       * @param {CodecFactory} factory
       * @param {string} alphabet
       */
      constructor(name5, code5, factory, alphabet5) {
        this.name = name5;
        this.code = code5;
        this.codeBuf = encodeText(this.code);
        this.alphabet = alphabet5;
        this.codec = factory(alphabet5);
      }
      /**
       * @param {Uint8Array} buf
       * @returns {string}
       */
      encode(buf) {
        return this.codec.encode(buf);
      }
      /**
       * @param {string} string
       * @returns {Uint8Array}
       */
      decode(string3) {
        for (const char of string3) {
          if (this.alphabet && this.alphabet.indexOf(char) < 0) {
            throw new Error(`invalid character '${char}' in '${string3}'`);
          }
        }
        return this.codec.decode(string3);
      }
    };
    module2.exports = Base;
  }
});

// ../../../node_modules/multibase/src/rfc4648.js
var require_rfc4648 = __commonJS({
  "../../../node_modules/multibase/src/rfc4648.js"(exports2, module2) {
    "use strict";
    var decode24 = (string3, alphabet5, bitsPerChar) => {
      const codes3 = {};
      for (let i = 0; i < alphabet5.length; ++i) {
        codes3[alphabet5[i]] = i;
      }
      let end = string3.length;
      while (string3[end - 1] === "=") {
        --end;
      }
      const out = new Uint8Array(end * bitsPerChar / 8 | 0);
      let bits2 = 0;
      let buffer = 0;
      let written = 0;
      for (let i = 0; i < end; ++i) {
        const value = codes3[string3[i]];
        if (value === void 0) {
          throw new SyntaxError("Invalid character " + string3[i]);
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
    var encode19 = (data, alphabet5, bitsPerChar) => {
      const pad = alphabet5[alphabet5.length - 1] === "=";
      const mask = (1 << bitsPerChar) - 1;
      let out = "";
      let bits2 = 0;
      let buffer = 0;
      for (let i = 0; i < data.length; ++i) {
        buffer = buffer << 8 | data[i];
        bits2 += 8;
        while (bits2 > bitsPerChar) {
          bits2 -= bitsPerChar;
          out += alphabet5[mask & buffer >> bits2];
        }
      }
      if (bits2) {
        out += alphabet5[mask & buffer << bitsPerChar - bits2];
      }
      if (pad) {
        while (out.length * bitsPerChar & 7) {
          out += "=";
        }
      }
      return out;
    };
    var rfc46485 = (bitsPerChar) => (alphabet5) => {
      return {
        /**
         * @param {Uint8Array} input
         * @returns {string}
         */
        encode(input) {
          return encode19(input, alphabet5, bitsPerChar);
        },
        /**
         * @param {string} input
         * @returns {Uint8Array}
         */
        decode(input) {
          return decode24(input, alphabet5, bitsPerChar);
        }
      };
    };
    module2.exports = { rfc4648: rfc46485 };
  }
});

// ../../../node_modules/multibase/src/constants.js
var require_constants = __commonJS({
  "../../../node_modules/multibase/src/constants.js"(exports2, module2) {
    "use strict";
    var baseX5 = require_src();
    var Base = require_base();
    var { rfc4648: rfc46485 } = require_rfc4648();
    var { decodeText, encodeText } = require_util2();
    var identity9 = () => {
      return {
        encode: decodeText,
        decode: encodeText
      };
    };
    var constants = [
      ["identity", "\0", identity9, ""],
      ["base2", "0", rfc46485(1), "01"],
      ["base8", "7", rfc46485(3), "01234567"],
      ["base10", "9", baseX5, "0123456789"],
      ["base16", "f", rfc46485(4), "0123456789abcdef"],
      ["base16upper", "F", rfc46485(4), "0123456789ABCDEF"],
      ["base32hex", "v", rfc46485(5), "0123456789abcdefghijklmnopqrstuv"],
      ["base32hexupper", "V", rfc46485(5), "0123456789ABCDEFGHIJKLMNOPQRSTUV"],
      ["base32hexpad", "t", rfc46485(5), "0123456789abcdefghijklmnopqrstuv="],
      ["base32hexpadupper", "T", rfc46485(5), "0123456789ABCDEFGHIJKLMNOPQRSTUV="],
      ["base32", "b", rfc46485(5), "abcdefghijklmnopqrstuvwxyz234567"],
      ["base32upper", "B", rfc46485(5), "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567"],
      ["base32pad", "c", rfc46485(5), "abcdefghijklmnopqrstuvwxyz234567="],
      ["base32padupper", "C", rfc46485(5), "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567="],
      ["base32z", "h", rfc46485(5), "ybndrfg8ejkmcpqxot1uwisza345h769"],
      ["base36", "k", baseX5, "0123456789abcdefghijklmnopqrstuvwxyz"],
      ["base36upper", "K", baseX5, "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ"],
      ["base58btc", "z", baseX5, "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"],
      ["base58flickr", "Z", baseX5, "123456789abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ"],
      ["base64", "m", rfc46485(6), "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"],
      ["base64pad", "M", rfc46485(6), "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/="],
      ["base64url", "u", rfc46485(6), "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"],
      ["base64urlpad", "U", rfc46485(6), "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_="]
    ];
    var names3 = constants.reduce(
      (prev, tupple) => {
        prev[tupple[0]] = new Base(tupple[0], tupple[1], tupple[2], tupple[3]);
        return prev;
      },
      /** @type {Record<BaseName,Base>} */
      {}
    );
    var codes3 = constants.reduce(
      (prev, tupple) => {
        prev[tupple[1]] = names3[tupple[0]];
        return prev;
      },
      /** @type {Record<BaseCode,Base>} */
      {}
    );
    module2.exports = {
      names: names3,
      codes: codes3
    };
  }
});

// ../../../node_modules/multibase/src/index.js
var require_src2 = __commonJS({
  "../../../node_modules/multibase/src/index.js"(exports2, module2) {
    "use strict";
    var constants = require_constants();
    var { encodeText, decodeText, concat: concat3 } = require_util2();
    function multibase(nameOrCode, buf) {
      if (!buf) {
        throw new Error("requires an encoded Uint8Array");
      }
      const { name: name5, codeBuf } = encoding(nameOrCode);
      validEncode(name5, buf);
      return concat3([codeBuf, buf], codeBuf.length + buf.length);
    }
    function encode19(nameOrCode, buf) {
      const enc = encoding(nameOrCode);
      const data = encodeText(enc.encode(buf));
      return concat3([enc.codeBuf, data], enc.codeBuf.length + data.length);
    }
    function decode24(data) {
      if (data instanceof Uint8Array) {
        data = decodeText(data);
      }
      const prefix = data[0];
      if (["f", "F", "v", "V", "t", "T", "b", "B", "c", "C", "h", "k", "K"].includes(prefix)) {
        data = data.toLowerCase();
      }
      const enc = encoding(
        /** @type {BaseCode} */
        data[0]
      );
      return enc.decode(data.substring(1));
    }
    function isEncoded(data) {
      if (data instanceof Uint8Array) {
        data = decodeText(data);
      }
      if (Object.prototype.toString.call(data) !== "[object String]") {
        return false;
      }
      try {
        const enc = encoding(
          /** @type {BaseCode} */
          data[0]
        );
        return enc.name;
      } catch (err) {
        return false;
      }
    }
    function validEncode(name5, buf) {
      const enc = encoding(name5);
      enc.decode(decodeText(buf));
    }
    function encoding(nameOrCode) {
      if (Object.prototype.hasOwnProperty.call(
        constants.names,
        /** @type {BaseName} */
        nameOrCode
      )) {
        return constants.names[
          /** @type {BaseName} */
          nameOrCode
        ];
      } else if (Object.prototype.hasOwnProperty.call(
        constants.codes,
        /** @type {BaseCode} */
        nameOrCode
      )) {
        return constants.codes[
          /** @type {BaseCode} */
          nameOrCode
        ];
      } else {
        throw new Error(`Unsupported encoding: ${nameOrCode}`);
      }
    }
    function encodingFromData(data) {
      if (data instanceof Uint8Array) {
        data = decodeText(data);
      }
      return encoding(
        /** @type {BaseCode} */
        data[0]
      );
    }
    exports2 = module2.exports = multibase;
    exports2.encode = encode19;
    exports2.decode = decode24;
    exports2.isEncoded = isEncoded;
    exports2.encoding = encoding;
    exports2.encodingFromData = encodingFromData;
    var names3 = Object.freeze(constants.names);
    var codes3 = Object.freeze(constants.codes);
    exports2.names = names3;
    exports2.codes = codes3;
  }
});

// ../../../node_modules/multihashes/node_modules/varint/encode.js
var require_encode2 = __commonJS({
  "../../../node_modules/multihashes/node_modules/varint/encode.js"(exports2, module2) {
    module2.exports = encode19;
    var MSB5 = 128;
    var REST5 = 127;
    var MSBALL5 = ~REST5;
    var INT5 = Math.pow(2, 31);
    function encode19(num, out, offset) {
      out = out || [];
      offset = offset || 0;
      var oldOffset = offset;
      while (num >= INT5) {
        out[offset++] = num & 255 | MSB5;
        num /= 128;
      }
      while (num & MSBALL5) {
        out[offset++] = num & 255 | MSB5;
        num >>>= 7;
      }
      out[offset] = num | 0;
      encode19.bytes = offset - oldOffset + 1;
      return out;
    }
  }
});

// ../../../node_modules/multihashes/node_modules/varint/decode.js
var require_decode2 = __commonJS({
  "../../../node_modules/multihashes/node_modules/varint/decode.js"(exports2, module2) {
    module2.exports = read5;
    var MSB5 = 128;
    var REST5 = 127;
    function read5(buf, offset) {
      var res = 0, offset = offset || 0, shift = 0, counter = offset, b, l = buf.length;
      do {
        if (counter >= l) {
          read5.bytes = 0;
          throw new RangeError("Could not decode varint");
        }
        b = buf[counter++];
        res += shift < 28 ? (b & REST5) << shift : (b & REST5) * Math.pow(2, shift);
        shift += 7;
      } while (b >= MSB5);
      read5.bytes = counter - offset;
      return res;
    }
  }
});

// ../../../node_modules/multihashes/node_modules/varint/length.js
var require_length2 = __commonJS({
  "../../../node_modules/multihashes/node_modules/varint/length.js"(exports2, module2) {
    var N16 = Math.pow(2, 7);
    var N26 = Math.pow(2, 14);
    var N36 = Math.pow(2, 21);
    var N46 = Math.pow(2, 28);
    var N56 = Math.pow(2, 35);
    var N66 = Math.pow(2, 42);
    var N76 = Math.pow(2, 49);
    var N86 = Math.pow(2, 56);
    var N96 = Math.pow(2, 63);
    module2.exports = function(value) {
      return value < N16 ? 1 : value < N26 ? 2 : value < N36 ? 3 : value < N46 ? 4 : value < N56 ? 5 : value < N66 ? 6 : value < N76 ? 7 : value < N86 ? 8 : value < N96 ? 9 : 10;
    };
  }
});

// ../../../node_modules/multihashes/node_modules/varint/index.js
var require_varint2 = __commonJS({
  "../../../node_modules/multihashes/node_modules/varint/index.js"(exports2, module2) {
    module2.exports = {
      encode: require_encode2(),
      decode: require_decode2(),
      encodingLength: require_length2()
    };
  }
});

// ../../../node_modules/multihashes/src/constants.js
var require_constants2 = __commonJS({
  "../../../node_modules/multihashes/src/constants.js"(exports2, module2) {
    "use strict";
    var names3 = Object.freeze({
      "identity": 0,
      "sha1": 17,
      "sha2-256": 18,
      "sha2-512": 19,
      "sha3-512": 20,
      "sha3-384": 21,
      "sha3-256": 22,
      "sha3-224": 23,
      "shake-128": 24,
      "shake-256": 25,
      "keccak-224": 26,
      "keccak-256": 27,
      "keccak-384": 28,
      "keccak-512": 29,
      "blake3": 30,
      "murmur3-128": 34,
      "murmur3-32": 35,
      "dbl-sha2-256": 86,
      "md4": 212,
      "md5": 213,
      "bmt": 214,
      "sha2-256-trunc254-padded": 4114,
      "ripemd-128": 4178,
      "ripemd-160": 4179,
      "ripemd-256": 4180,
      "ripemd-320": 4181,
      "x11": 4352,
      "kangarootwelve": 7425,
      "sm3-256": 21325,
      "blake2b-8": 45569,
      "blake2b-16": 45570,
      "blake2b-24": 45571,
      "blake2b-32": 45572,
      "blake2b-40": 45573,
      "blake2b-48": 45574,
      "blake2b-56": 45575,
      "blake2b-64": 45576,
      "blake2b-72": 45577,
      "blake2b-80": 45578,
      "blake2b-88": 45579,
      "blake2b-96": 45580,
      "blake2b-104": 45581,
      "blake2b-112": 45582,
      "blake2b-120": 45583,
      "blake2b-128": 45584,
      "blake2b-136": 45585,
      "blake2b-144": 45586,
      "blake2b-152": 45587,
      "blake2b-160": 45588,
      "blake2b-168": 45589,
      "blake2b-176": 45590,
      "blake2b-184": 45591,
      "blake2b-192": 45592,
      "blake2b-200": 45593,
      "blake2b-208": 45594,
      "blake2b-216": 45595,
      "blake2b-224": 45596,
      "blake2b-232": 45597,
      "blake2b-240": 45598,
      "blake2b-248": 45599,
      "blake2b-256": 45600,
      "blake2b-264": 45601,
      "blake2b-272": 45602,
      "blake2b-280": 45603,
      "blake2b-288": 45604,
      "blake2b-296": 45605,
      "blake2b-304": 45606,
      "blake2b-312": 45607,
      "blake2b-320": 45608,
      "blake2b-328": 45609,
      "blake2b-336": 45610,
      "blake2b-344": 45611,
      "blake2b-352": 45612,
      "blake2b-360": 45613,
      "blake2b-368": 45614,
      "blake2b-376": 45615,
      "blake2b-384": 45616,
      "blake2b-392": 45617,
      "blake2b-400": 45618,
      "blake2b-408": 45619,
      "blake2b-416": 45620,
      "blake2b-424": 45621,
      "blake2b-432": 45622,
      "blake2b-440": 45623,
      "blake2b-448": 45624,
      "blake2b-456": 45625,
      "blake2b-464": 45626,
      "blake2b-472": 45627,
      "blake2b-480": 45628,
      "blake2b-488": 45629,
      "blake2b-496": 45630,
      "blake2b-504": 45631,
      "blake2b-512": 45632,
      "blake2s-8": 45633,
      "blake2s-16": 45634,
      "blake2s-24": 45635,
      "blake2s-32": 45636,
      "blake2s-40": 45637,
      "blake2s-48": 45638,
      "blake2s-56": 45639,
      "blake2s-64": 45640,
      "blake2s-72": 45641,
      "blake2s-80": 45642,
      "blake2s-88": 45643,
      "blake2s-96": 45644,
      "blake2s-104": 45645,
      "blake2s-112": 45646,
      "blake2s-120": 45647,
      "blake2s-128": 45648,
      "blake2s-136": 45649,
      "blake2s-144": 45650,
      "blake2s-152": 45651,
      "blake2s-160": 45652,
      "blake2s-168": 45653,
      "blake2s-176": 45654,
      "blake2s-184": 45655,
      "blake2s-192": 45656,
      "blake2s-200": 45657,
      "blake2s-208": 45658,
      "blake2s-216": 45659,
      "blake2s-224": 45660,
      "blake2s-232": 45661,
      "blake2s-240": 45662,
      "blake2s-248": 45663,
      "blake2s-256": 45664,
      "skein256-8": 45825,
      "skein256-16": 45826,
      "skein256-24": 45827,
      "skein256-32": 45828,
      "skein256-40": 45829,
      "skein256-48": 45830,
      "skein256-56": 45831,
      "skein256-64": 45832,
      "skein256-72": 45833,
      "skein256-80": 45834,
      "skein256-88": 45835,
      "skein256-96": 45836,
      "skein256-104": 45837,
      "skein256-112": 45838,
      "skein256-120": 45839,
      "skein256-128": 45840,
      "skein256-136": 45841,
      "skein256-144": 45842,
      "skein256-152": 45843,
      "skein256-160": 45844,
      "skein256-168": 45845,
      "skein256-176": 45846,
      "skein256-184": 45847,
      "skein256-192": 45848,
      "skein256-200": 45849,
      "skein256-208": 45850,
      "skein256-216": 45851,
      "skein256-224": 45852,
      "skein256-232": 45853,
      "skein256-240": 45854,
      "skein256-248": 45855,
      "skein256-256": 45856,
      "skein512-8": 45857,
      "skein512-16": 45858,
      "skein512-24": 45859,
      "skein512-32": 45860,
      "skein512-40": 45861,
      "skein512-48": 45862,
      "skein512-56": 45863,
      "skein512-64": 45864,
      "skein512-72": 45865,
      "skein512-80": 45866,
      "skein512-88": 45867,
      "skein512-96": 45868,
      "skein512-104": 45869,
      "skein512-112": 45870,
      "skein512-120": 45871,
      "skein512-128": 45872,
      "skein512-136": 45873,
      "skein512-144": 45874,
      "skein512-152": 45875,
      "skein512-160": 45876,
      "skein512-168": 45877,
      "skein512-176": 45878,
      "skein512-184": 45879,
      "skein512-192": 45880,
      "skein512-200": 45881,
      "skein512-208": 45882,
      "skein512-216": 45883,
      "skein512-224": 45884,
      "skein512-232": 45885,
      "skein512-240": 45886,
      "skein512-248": 45887,
      "skein512-256": 45888,
      "skein512-264": 45889,
      "skein512-272": 45890,
      "skein512-280": 45891,
      "skein512-288": 45892,
      "skein512-296": 45893,
      "skein512-304": 45894,
      "skein512-312": 45895,
      "skein512-320": 45896,
      "skein512-328": 45897,
      "skein512-336": 45898,
      "skein512-344": 45899,
      "skein512-352": 45900,
      "skein512-360": 45901,
      "skein512-368": 45902,
      "skein512-376": 45903,
      "skein512-384": 45904,
      "skein512-392": 45905,
      "skein512-400": 45906,
      "skein512-408": 45907,
      "skein512-416": 45908,
      "skein512-424": 45909,
      "skein512-432": 45910,
      "skein512-440": 45911,
      "skein512-448": 45912,
      "skein512-456": 45913,
      "skein512-464": 45914,
      "skein512-472": 45915,
      "skein512-480": 45916,
      "skein512-488": 45917,
      "skein512-496": 45918,
      "skein512-504": 45919,
      "skein512-512": 45920,
      "skein1024-8": 45921,
      "skein1024-16": 45922,
      "skein1024-24": 45923,
      "skein1024-32": 45924,
      "skein1024-40": 45925,
      "skein1024-48": 45926,
      "skein1024-56": 45927,
      "skein1024-64": 45928,
      "skein1024-72": 45929,
      "skein1024-80": 45930,
      "skein1024-88": 45931,
      "skein1024-96": 45932,
      "skein1024-104": 45933,
      "skein1024-112": 45934,
      "skein1024-120": 45935,
      "skein1024-128": 45936,
      "skein1024-136": 45937,
      "skein1024-144": 45938,
      "skein1024-152": 45939,
      "skein1024-160": 45940,
      "skein1024-168": 45941,
      "skein1024-176": 45942,
      "skein1024-184": 45943,
      "skein1024-192": 45944,
      "skein1024-200": 45945,
      "skein1024-208": 45946,
      "skein1024-216": 45947,
      "skein1024-224": 45948,
      "skein1024-232": 45949,
      "skein1024-240": 45950,
      "skein1024-248": 45951,
      "skein1024-256": 45952,
      "skein1024-264": 45953,
      "skein1024-272": 45954,
      "skein1024-280": 45955,
      "skein1024-288": 45956,
      "skein1024-296": 45957,
      "skein1024-304": 45958,
      "skein1024-312": 45959,
      "skein1024-320": 45960,
      "skein1024-328": 45961,
      "skein1024-336": 45962,
      "skein1024-344": 45963,
      "skein1024-352": 45964,
      "skein1024-360": 45965,
      "skein1024-368": 45966,
      "skein1024-376": 45967,
      "skein1024-384": 45968,
      "skein1024-392": 45969,
      "skein1024-400": 45970,
      "skein1024-408": 45971,
      "skein1024-416": 45972,
      "skein1024-424": 45973,
      "skein1024-432": 45974,
      "skein1024-440": 45975,
      "skein1024-448": 45976,
      "skein1024-456": 45977,
      "skein1024-464": 45978,
      "skein1024-472": 45979,
      "skein1024-480": 45980,
      "skein1024-488": 45981,
      "skein1024-496": 45982,
      "skein1024-504": 45983,
      "skein1024-512": 45984,
      "skein1024-520": 45985,
      "skein1024-528": 45986,
      "skein1024-536": 45987,
      "skein1024-544": 45988,
      "skein1024-552": 45989,
      "skein1024-560": 45990,
      "skein1024-568": 45991,
      "skein1024-576": 45992,
      "skein1024-584": 45993,
      "skein1024-592": 45994,
      "skein1024-600": 45995,
      "skein1024-608": 45996,
      "skein1024-616": 45997,
      "skein1024-624": 45998,
      "skein1024-632": 45999,
      "skein1024-640": 46e3,
      "skein1024-648": 46001,
      "skein1024-656": 46002,
      "skein1024-664": 46003,
      "skein1024-672": 46004,
      "skein1024-680": 46005,
      "skein1024-688": 46006,
      "skein1024-696": 46007,
      "skein1024-704": 46008,
      "skein1024-712": 46009,
      "skein1024-720": 46010,
      "skein1024-728": 46011,
      "skein1024-736": 46012,
      "skein1024-744": 46013,
      "skein1024-752": 46014,
      "skein1024-760": 46015,
      "skein1024-768": 46016,
      "skein1024-776": 46017,
      "skein1024-784": 46018,
      "skein1024-792": 46019,
      "skein1024-800": 46020,
      "skein1024-808": 46021,
      "skein1024-816": 46022,
      "skein1024-824": 46023,
      "skein1024-832": 46024,
      "skein1024-840": 46025,
      "skein1024-848": 46026,
      "skein1024-856": 46027,
      "skein1024-864": 46028,
      "skein1024-872": 46029,
      "skein1024-880": 46030,
      "skein1024-888": 46031,
      "skein1024-896": 46032,
      "skein1024-904": 46033,
      "skein1024-912": 46034,
      "skein1024-920": 46035,
      "skein1024-928": 46036,
      "skein1024-936": 46037,
      "skein1024-944": 46038,
      "skein1024-952": 46039,
      "skein1024-960": 46040,
      "skein1024-968": 46041,
      "skein1024-976": 46042,
      "skein1024-984": 46043,
      "skein1024-992": 46044,
      "skein1024-1000": 46045,
      "skein1024-1008": 46046,
      "skein1024-1016": 46047,
      "skein1024-1024": 46048,
      "poseidon-bls12_381-a2-fc1": 46081,
      "poseidon-bls12_381-a2-fc1-sc": 46082
    });
    module2.exports = { names: names3 };
  }
});

// ../../../node_modules/multihashes/node_modules/multiformats/esm/vendor/base-x.js
function base6(ALPHABET, name5) {
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
  function encode19(source) {
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
    var length5 = 0;
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
      for (var it1 = size - 1; (carry !== 0 || i2 < length5) && it1 !== -1; it1--, i2++) {
        carry += 256 * b58[it1] >>> 0;
        b58[it1] = carry % BASE >>> 0;
        carry = carry / BASE >>> 0;
      }
      if (carry !== 0) {
        throw new Error("Non-zero carry");
      }
      length5 = i2;
      pbegin++;
    }
    var it2 = size - length5;
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
    var length5 = 0;
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
      for (var it3 = size - 1; (carry !== 0 || i2 < length5) && it3 !== -1; it3--, i2++) {
        carry += BASE * b256[it3] >>> 0;
        b256[it3] = carry % 256 >>> 0;
        carry = carry / 256 >>> 0;
      }
      if (carry !== 0) {
        throw new Error("Non-zero carry");
      }
      length5 = i2;
      psz++;
    }
    if (source[psz] === " ") {
      return;
    }
    var it4 = size - length5;
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
  function decode24(string3) {
    var buffer = decodeUnsafe(string3);
    if (buffer) {
      return buffer;
    }
    throw new Error(`Non-${name5} character`);
  }
  return {
    encode: encode19,
    decodeUnsafe,
    decode: decode24
  };
}
var src4, _brrp__multiformats_scope_baseX4, base_x_default4;
var init_base_x = __esm({
  "../../../node_modules/multihashes/node_modules/multiformats/esm/vendor/base-x.js"() {
    src4 = base6;
    _brrp__multiformats_scope_baseX4 = src4;
    base_x_default4 = _brrp__multiformats_scope_baseX4;
  }
});

// ../../../node_modules/multihashes/node_modules/multiformats/esm/src/bytes.js
var empty4, equals8, coerce4, fromString6, toString6;
var init_bytes = __esm({
  "../../../node_modules/multihashes/node_modules/multiformats/esm/src/bytes.js"() {
    empty4 = new Uint8Array(0);
    equals8 = (aa, bb) => {
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
    coerce4 = (o) => {
      if (o instanceof Uint8Array && o.constructor.name === "Uint8Array")
        return o;
      if (o instanceof ArrayBuffer)
        return new Uint8Array(o);
      if (ArrayBuffer.isView(o)) {
        return new Uint8Array(o.buffer, o.byteOffset, o.byteLength);
      }
      throw new Error("Unknown type, must be binary type");
    };
    fromString6 = (str) => new TextEncoder().encode(str);
    toString6 = (b) => new TextDecoder().decode(b);
  }
});

// ../../../node_modules/multihashes/node_modules/multiformats/esm/src/bases/base.js
var Encoder4, Decoder4, ComposedDecoder4, or5, Codec4, from7, baseX4, decode17, encode14, rfc46484;
var init_base = __esm({
  "../../../node_modules/multihashes/node_modules/multiformats/esm/src/bases/base.js"() {
    init_base_x();
    init_bytes();
    Encoder4 = class {
      constructor(name5, prefix, baseEncode) {
        this.name = name5;
        this.prefix = prefix;
        this.baseEncode = baseEncode;
      }
      encode(bytes2) {
        if (bytes2 instanceof Uint8Array) {
          return `${this.prefix}${this.baseEncode(bytes2)}`;
        } else {
          throw Error("Unknown type, must be binary type");
        }
      }
    };
    Decoder4 = class {
      constructor(name5, prefix, baseDecode) {
        this.name = name5;
        this.prefix = prefix;
        if (prefix.codePointAt(0) === void 0) {
          throw new Error("Invalid prefix character");
        }
        this.prefixCodePoint = prefix.codePointAt(0);
        this.baseDecode = baseDecode;
      }
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
      or(decoder) {
        return or5(this, decoder);
      }
    };
    ComposedDecoder4 = class {
      constructor(decoders2) {
        this.decoders = decoders2;
      }
      or(decoder) {
        return or5(this, decoder);
      }
      decode(input) {
        const prefix = input[0];
        const decoder = this.decoders[prefix];
        if (decoder) {
          return decoder.decode(input);
        } else {
          throw RangeError(`Unable to decode multibase string ${JSON.stringify(input)}, only inputs prefixed with ${Object.keys(this.decoders)} are supported`);
        }
      }
    };
    or5 = (left, right) => new ComposedDecoder4({
      ...left.decoders || { [left.prefix]: left },
      ...right.decoders || { [right.prefix]: right }
    });
    Codec4 = class {
      constructor(name5, prefix, baseEncode, baseDecode) {
        this.name = name5;
        this.prefix = prefix;
        this.baseEncode = baseEncode;
        this.baseDecode = baseDecode;
        this.encoder = new Encoder4(name5, prefix, baseEncode);
        this.decoder = new Decoder4(name5, prefix, baseDecode);
      }
      encode(input) {
        return this.encoder.encode(input);
      }
      decode(input) {
        return this.decoder.decode(input);
      }
    };
    from7 = ({ name: name5, prefix, encode: encode19, decode: decode24 }) => new Codec4(name5, prefix, encode19, decode24);
    baseX4 = ({ prefix, name: name5, alphabet: alphabet5 }) => {
      const { encode: encode19, decode: decode24 } = base_x_default4(alphabet5, name5);
      return from7({
        prefix,
        name: name5,
        encode: encode19,
        decode: (text) => coerce4(decode24(text))
      });
    };
    decode17 = (string3, alphabet5, bitsPerChar, name5) => {
      const codes3 = {};
      for (let i = 0; i < alphabet5.length; ++i) {
        codes3[alphabet5[i]] = i;
      }
      let end = string3.length;
      while (string3[end - 1] === "=") {
        --end;
      }
      const out = new Uint8Array(end * bitsPerChar / 8 | 0);
      let bits2 = 0;
      let buffer = 0;
      let written = 0;
      for (let i = 0; i < end; ++i) {
        const value = codes3[string3[i]];
        if (value === void 0) {
          throw new SyntaxError(`Non-${name5} character`);
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
    encode14 = (data, alphabet5, bitsPerChar) => {
      const pad = alphabet5[alphabet5.length - 1] === "=";
      const mask = (1 << bitsPerChar) - 1;
      let out = "";
      let bits2 = 0;
      let buffer = 0;
      for (let i = 0; i < data.length; ++i) {
        buffer = buffer << 8 | data[i];
        bits2 += 8;
        while (bits2 > bitsPerChar) {
          bits2 -= bitsPerChar;
          out += alphabet5[mask & buffer >> bits2];
        }
      }
      if (bits2) {
        out += alphabet5[mask & buffer << bitsPerChar - bits2];
      }
      if (pad) {
        while (out.length * bitsPerChar & 7) {
          out += "=";
        }
      }
      return out;
    };
    rfc46484 = ({ name: name5, prefix, bitsPerChar, alphabet: alphabet5 }) => {
      return from7({
        prefix,
        name: name5,
        encode(input) {
          return encode14(input, alphabet5, bitsPerChar);
        },
        decode(input) {
          return decode17(input, alphabet5, bitsPerChar, name5);
        }
      });
    };
  }
});

// ../../../node_modules/multihashes/node_modules/multiformats/esm/src/bases/identity.js
var identity_exports7 = {};
__export(identity_exports7, {
  identity: () => identity7
});
var identity7;
var init_identity = __esm({
  "../../../node_modules/multihashes/node_modules/multiformats/esm/src/bases/identity.js"() {
    init_base();
    init_bytes();
    identity7 = from7({
      prefix: "\0",
      name: "identity",
      encode: (buf) => toString6(buf),
      decode: (str) => fromString6(str)
    });
  }
});

// ../../../node_modules/multihashes/node_modules/multiformats/esm/src/bases/base2.js
var base2_exports4 = {};
__export(base2_exports4, {
  base2: () => base24
});
var base24;
var init_base2 = __esm({
  "../../../node_modules/multihashes/node_modules/multiformats/esm/src/bases/base2.js"() {
    init_base();
    base24 = rfc46484({
      prefix: "0",
      name: "base2",
      alphabet: "01",
      bitsPerChar: 1
    });
  }
});

// ../../../node_modules/multihashes/node_modules/multiformats/esm/src/bases/base8.js
var base8_exports4 = {};
__export(base8_exports4, {
  base8: () => base84
});
var base84;
var init_base8 = __esm({
  "../../../node_modules/multihashes/node_modules/multiformats/esm/src/bases/base8.js"() {
    init_base();
    base84 = rfc46484({
      prefix: "7",
      name: "base8",
      alphabet: "01234567",
      bitsPerChar: 3
    });
  }
});

// ../../../node_modules/multihashes/node_modules/multiformats/esm/src/bases/base10.js
var base10_exports4 = {};
__export(base10_exports4, {
  base10: () => base104
});
var base104;
var init_base10 = __esm({
  "../../../node_modules/multihashes/node_modules/multiformats/esm/src/bases/base10.js"() {
    init_base();
    base104 = baseX4({
      prefix: "9",
      name: "base10",
      alphabet: "0123456789"
    });
  }
});

// ../../../node_modules/multihashes/node_modules/multiformats/esm/src/bases/base16.js
var base16_exports4 = {};
__export(base16_exports4, {
  base16: () => base164,
  base16upper: () => base16upper4
});
var base164, base16upper4;
var init_base16 = __esm({
  "../../../node_modules/multihashes/node_modules/multiformats/esm/src/bases/base16.js"() {
    init_base();
    base164 = rfc46484({
      prefix: "f",
      name: "base16",
      alphabet: "0123456789abcdef",
      bitsPerChar: 4
    });
    base16upper4 = rfc46484({
      prefix: "F",
      name: "base16upper",
      alphabet: "0123456789ABCDEF",
      bitsPerChar: 4
    });
  }
});

// ../../../node_modules/multihashes/node_modules/multiformats/esm/src/bases/base32.js
var base32_exports4 = {};
__export(base32_exports4, {
  base32: () => base324,
  base32hex: () => base32hex4,
  base32hexpad: () => base32hexpad4,
  base32hexpadupper: () => base32hexpadupper4,
  base32hexupper: () => base32hexupper4,
  base32pad: () => base32pad4,
  base32padupper: () => base32padupper4,
  base32upper: () => base32upper4,
  base32z: () => base32z4
});
var base324, base32upper4, base32pad4, base32padupper4, base32hex4, base32hexupper4, base32hexpad4, base32hexpadupper4, base32z4;
var init_base32 = __esm({
  "../../../node_modules/multihashes/node_modules/multiformats/esm/src/bases/base32.js"() {
    init_base();
    base324 = rfc46484({
      prefix: "b",
      name: "base32",
      alphabet: "abcdefghijklmnopqrstuvwxyz234567",
      bitsPerChar: 5
    });
    base32upper4 = rfc46484({
      prefix: "B",
      name: "base32upper",
      alphabet: "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567",
      bitsPerChar: 5
    });
    base32pad4 = rfc46484({
      prefix: "c",
      name: "base32pad",
      alphabet: "abcdefghijklmnopqrstuvwxyz234567=",
      bitsPerChar: 5
    });
    base32padupper4 = rfc46484({
      prefix: "C",
      name: "base32padupper",
      alphabet: "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567=",
      bitsPerChar: 5
    });
    base32hex4 = rfc46484({
      prefix: "v",
      name: "base32hex",
      alphabet: "0123456789abcdefghijklmnopqrstuv",
      bitsPerChar: 5
    });
    base32hexupper4 = rfc46484({
      prefix: "V",
      name: "base32hexupper",
      alphabet: "0123456789ABCDEFGHIJKLMNOPQRSTUV",
      bitsPerChar: 5
    });
    base32hexpad4 = rfc46484({
      prefix: "t",
      name: "base32hexpad",
      alphabet: "0123456789abcdefghijklmnopqrstuv=",
      bitsPerChar: 5
    });
    base32hexpadupper4 = rfc46484({
      prefix: "T",
      name: "base32hexpadupper",
      alphabet: "0123456789ABCDEFGHIJKLMNOPQRSTUV=",
      bitsPerChar: 5
    });
    base32z4 = rfc46484({
      prefix: "h",
      name: "base32z",
      alphabet: "ybndrfg8ejkmcpqxot1uwisza345h769",
      bitsPerChar: 5
    });
  }
});

// ../../../node_modules/multihashes/node_modules/multiformats/esm/src/bases/base36.js
var base36_exports4 = {};
__export(base36_exports4, {
  base36: () => base364,
  base36upper: () => base36upper4
});
var base364, base36upper4;
var init_base36 = __esm({
  "../../../node_modules/multihashes/node_modules/multiformats/esm/src/bases/base36.js"() {
    init_base();
    base364 = baseX4({
      prefix: "k",
      name: "base36",
      alphabet: "0123456789abcdefghijklmnopqrstuvwxyz"
    });
    base36upper4 = baseX4({
      prefix: "K",
      name: "base36upper",
      alphabet: "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ"
    });
  }
});

// ../../../node_modules/multihashes/node_modules/multiformats/esm/src/bases/base58.js
var base58_exports4 = {};
__export(base58_exports4, {
  base58btc: () => base58btc4,
  base58flickr: () => base58flickr4
});
var base58btc4, base58flickr4;
var init_base58 = __esm({
  "../../../node_modules/multihashes/node_modules/multiformats/esm/src/bases/base58.js"() {
    init_base();
    base58btc4 = baseX4({
      name: "base58btc",
      prefix: "z",
      alphabet: "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
    });
    base58flickr4 = baseX4({
      name: "base58flickr",
      prefix: "Z",
      alphabet: "123456789abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ"
    });
  }
});

// ../../../node_modules/multihashes/node_modules/multiformats/esm/src/bases/base64.js
var base64_exports4 = {};
__export(base64_exports4, {
  base64: () => base644,
  base64pad: () => base64pad4,
  base64url: () => base64url4,
  base64urlpad: () => base64urlpad4
});
var base644, base64pad4, base64url4, base64urlpad4;
var init_base64 = __esm({
  "../../../node_modules/multihashes/node_modules/multiformats/esm/src/bases/base64.js"() {
    init_base();
    base644 = rfc46484({
      prefix: "m",
      name: "base64",
      alphabet: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/",
      bitsPerChar: 6
    });
    base64pad4 = rfc46484({
      prefix: "M",
      name: "base64pad",
      alphabet: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=",
      bitsPerChar: 6
    });
    base64url4 = rfc46484({
      prefix: "u",
      name: "base64url",
      alphabet: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_",
      bitsPerChar: 6
    });
    base64urlpad4 = rfc46484({
      prefix: "U",
      name: "base64urlpad",
      alphabet: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_=",
      bitsPerChar: 6
    });
  }
});

// ../../../node_modules/multihashes/node_modules/multiformats/esm/src/bases/base256emoji.js
var base256emoji_exports4 = {};
__export(base256emoji_exports4, {
  base256emoji: () => base256emoji4
});
function encode15(data) {
  return data.reduce((p, c) => {
    p += alphabetBytesToChars4[c];
    return p;
  }, "");
}
function decode18(str) {
  const byts = [];
  for (const char of str) {
    const byt = alphabetCharsToBytes4[char.codePointAt(0)];
    if (byt === void 0) {
      throw new Error(`Non-base256emoji character: ${char}`);
    }
    byts.push(byt);
  }
  return new Uint8Array(byts);
}
var alphabet4, alphabetBytesToChars4, alphabetCharsToBytes4, base256emoji4;
var init_base256emoji = __esm({
  "../../../node_modules/multihashes/node_modules/multiformats/esm/src/bases/base256emoji.js"() {
    init_base();
    alphabet4 = Array.from("\u{1F680}\u{1FA90}\u2604\u{1F6F0}\u{1F30C}\u{1F311}\u{1F312}\u{1F313}\u{1F314}\u{1F315}\u{1F316}\u{1F317}\u{1F318}\u{1F30D}\u{1F30F}\u{1F30E}\u{1F409}\u2600\u{1F4BB}\u{1F5A5}\u{1F4BE}\u{1F4BF}\u{1F602}\u2764\u{1F60D}\u{1F923}\u{1F60A}\u{1F64F}\u{1F495}\u{1F62D}\u{1F618}\u{1F44D}\u{1F605}\u{1F44F}\u{1F601}\u{1F525}\u{1F970}\u{1F494}\u{1F496}\u{1F499}\u{1F622}\u{1F914}\u{1F606}\u{1F644}\u{1F4AA}\u{1F609}\u263A\u{1F44C}\u{1F917}\u{1F49C}\u{1F614}\u{1F60E}\u{1F607}\u{1F339}\u{1F926}\u{1F389}\u{1F49E}\u270C\u2728\u{1F937}\u{1F631}\u{1F60C}\u{1F338}\u{1F64C}\u{1F60B}\u{1F497}\u{1F49A}\u{1F60F}\u{1F49B}\u{1F642}\u{1F493}\u{1F929}\u{1F604}\u{1F600}\u{1F5A4}\u{1F603}\u{1F4AF}\u{1F648}\u{1F447}\u{1F3B6}\u{1F612}\u{1F92D}\u2763\u{1F61C}\u{1F48B}\u{1F440}\u{1F62A}\u{1F611}\u{1F4A5}\u{1F64B}\u{1F61E}\u{1F629}\u{1F621}\u{1F92A}\u{1F44A}\u{1F973}\u{1F625}\u{1F924}\u{1F449}\u{1F483}\u{1F633}\u270B\u{1F61A}\u{1F61D}\u{1F634}\u{1F31F}\u{1F62C}\u{1F643}\u{1F340}\u{1F337}\u{1F63B}\u{1F613}\u2B50\u2705\u{1F97A}\u{1F308}\u{1F608}\u{1F918}\u{1F4A6}\u2714\u{1F623}\u{1F3C3}\u{1F490}\u2639\u{1F38A}\u{1F498}\u{1F620}\u261D\u{1F615}\u{1F33A}\u{1F382}\u{1F33B}\u{1F610}\u{1F595}\u{1F49D}\u{1F64A}\u{1F639}\u{1F5E3}\u{1F4AB}\u{1F480}\u{1F451}\u{1F3B5}\u{1F91E}\u{1F61B}\u{1F534}\u{1F624}\u{1F33C}\u{1F62B}\u26BD\u{1F919}\u2615\u{1F3C6}\u{1F92B}\u{1F448}\u{1F62E}\u{1F646}\u{1F37B}\u{1F343}\u{1F436}\u{1F481}\u{1F632}\u{1F33F}\u{1F9E1}\u{1F381}\u26A1\u{1F31E}\u{1F388}\u274C\u270A\u{1F44B}\u{1F630}\u{1F928}\u{1F636}\u{1F91D}\u{1F6B6}\u{1F4B0}\u{1F353}\u{1F4A2}\u{1F91F}\u{1F641}\u{1F6A8}\u{1F4A8}\u{1F92C}\u2708\u{1F380}\u{1F37A}\u{1F913}\u{1F619}\u{1F49F}\u{1F331}\u{1F616}\u{1F476}\u{1F974}\u25B6\u27A1\u2753\u{1F48E}\u{1F4B8}\u2B07\u{1F628}\u{1F31A}\u{1F98B}\u{1F637}\u{1F57A}\u26A0\u{1F645}\u{1F61F}\u{1F635}\u{1F44E}\u{1F932}\u{1F920}\u{1F927}\u{1F4CC}\u{1F535}\u{1F485}\u{1F9D0}\u{1F43E}\u{1F352}\u{1F617}\u{1F911}\u{1F30A}\u{1F92F}\u{1F437}\u260E\u{1F4A7}\u{1F62F}\u{1F486}\u{1F446}\u{1F3A4}\u{1F647}\u{1F351}\u2744\u{1F334}\u{1F4A3}\u{1F438}\u{1F48C}\u{1F4CD}\u{1F940}\u{1F922}\u{1F445}\u{1F4A1}\u{1F4A9}\u{1F450}\u{1F4F8}\u{1F47B}\u{1F910}\u{1F92E}\u{1F3BC}\u{1F975}\u{1F6A9}\u{1F34E}\u{1F34A}\u{1F47C}\u{1F48D}\u{1F4E3}\u{1F942}");
    alphabetBytesToChars4 = alphabet4.reduce((p, c, i) => {
      p[i] = c;
      return p;
    }, []);
    alphabetCharsToBytes4 = alphabet4.reduce((p, c, i) => {
      p[c.codePointAt(0)] = i;
      return p;
    }, []);
    base256emoji4 = from7({
      prefix: "\u{1F680}",
      name: "base256emoji",
      encode: encode15,
      decode: decode18
    });
  }
});

// ../../../node_modules/multihashes/node_modules/multiformats/esm/vendor/varint.js
function encode16(num, out, offset) {
  out = out || [];
  offset = offset || 0;
  var oldOffset = offset;
  while (num >= INT4) {
    out[offset++] = num & 255 | MSB4;
    num /= 128;
  }
  while (num & MSBALL4) {
    out[offset++] = num & 255 | MSB4;
    num >>>= 7;
  }
  out[offset] = num | 0;
  encode16.bytes = offset - oldOffset + 1;
  return out;
}
function read4(buf, offset) {
  var res = 0, offset = offset || 0, shift = 0, counter = offset, b, l = buf.length;
  do {
    if (counter >= l) {
      read4.bytes = 0;
      throw new RangeError("Could not decode varint");
    }
    b = buf[counter++];
    res += shift < 28 ? (b & REST$14) << shift : (b & REST$14) * Math.pow(2, shift);
    shift += 7;
  } while (b >= MSB$14);
  read4.bytes = counter - offset;
  return res;
}
var encode_14, MSB4, REST4, MSBALL4, INT4, decode19, MSB$14, REST$14, N15, N25, N35, N45, N55, N65, N75, N85, N95, length4, varint7, _brrp_varint4, varint_default4;
var init_varint = __esm({
  "../../../node_modules/multihashes/node_modules/multiformats/esm/vendor/varint.js"() {
    encode_14 = encode16;
    MSB4 = 128;
    REST4 = 127;
    MSBALL4 = ~REST4;
    INT4 = Math.pow(2, 31);
    decode19 = read4;
    MSB$14 = 128;
    REST$14 = 127;
    N15 = Math.pow(2, 7);
    N25 = Math.pow(2, 14);
    N35 = Math.pow(2, 21);
    N45 = Math.pow(2, 28);
    N55 = Math.pow(2, 35);
    N65 = Math.pow(2, 42);
    N75 = Math.pow(2, 49);
    N85 = Math.pow(2, 56);
    N95 = Math.pow(2, 63);
    length4 = function(value) {
      return value < N15 ? 1 : value < N25 ? 2 : value < N35 ? 3 : value < N45 ? 4 : value < N55 ? 5 : value < N65 ? 6 : value < N75 ? 7 : value < N85 ? 8 : value < N95 ? 9 : 10;
    };
    varint7 = {
      encode: encode_14,
      decode: decode19,
      encodingLength: length4
    };
    _brrp_varint4 = varint7;
    varint_default4 = _brrp_varint4;
  }
});

// ../../../node_modules/multihashes/node_modules/multiformats/esm/src/varint.js
var decode20, encodeTo4, encodingLength4;
var init_varint2 = __esm({
  "../../../node_modules/multihashes/node_modules/multiformats/esm/src/varint.js"() {
    init_varint();
    decode20 = (data, offset = 0) => {
      const code5 = varint_default4.decode(data, offset);
      return [
        code5,
        varint_default4.decode.bytes
      ];
    };
    encodeTo4 = (int, target, offset = 0) => {
      varint_default4.encode(int, target, offset);
      return target;
    };
    encodingLength4 = (int) => {
      return varint_default4.encodingLength(int);
    };
  }
});

// ../../../node_modules/multihashes/node_modules/multiformats/esm/src/hashes/digest.js
var create6, decode21, equals9, Digest4;
var init_digest = __esm({
  "../../../node_modules/multihashes/node_modules/multiformats/esm/src/hashes/digest.js"() {
    init_bytes();
    init_varint2();
    create6 = (code5, digest5) => {
      const size = digest5.byteLength;
      const sizeOffset = encodingLength4(code5);
      const digestOffset = sizeOffset + encodingLength4(size);
      const bytes2 = new Uint8Array(digestOffset + size);
      encodeTo4(code5, bytes2, 0);
      encodeTo4(size, bytes2, sizeOffset);
      bytes2.set(digest5, digestOffset);
      return new Digest4(code5, size, digest5, bytes2);
    };
    decode21 = (multihash) => {
      const bytes2 = coerce4(multihash);
      const [code5, sizeOffset] = decode20(bytes2);
      const [size, digestOffset] = decode20(bytes2.subarray(sizeOffset));
      const digest5 = bytes2.subarray(sizeOffset + digestOffset);
      if (digest5.byteLength !== size) {
        throw new Error("Incorrect length");
      }
      return new Digest4(code5, size, digest5, bytes2);
    };
    equals9 = (a, b) => {
      if (a === b) {
        return true;
      } else {
        return a.code === b.code && a.size === b.size && equals8(a.bytes, b.bytes);
      }
    };
    Digest4 = class {
      constructor(code5, size, digest5, bytes2) {
        this.code = code5;
        this.size = size;
        this.digest = digest5;
        this.bytes = bytes2;
      }
    };
  }
});

// ../../../node_modules/multihashes/node_modules/multiformats/esm/src/hashes/hasher.js
var from8, Hasher4;
var init_hasher = __esm({
  "../../../node_modules/multihashes/node_modules/multiformats/esm/src/hashes/hasher.js"() {
    init_digest();
    from8 = ({ name: name5, code: code5, encode: encode19 }) => new Hasher4(name5, code5, encode19);
    Hasher4 = class {
      constructor(name5, code5, encode19) {
        this.name = name5;
        this.code = code5;
        this.encode = encode19;
      }
      digest(input) {
        if (input instanceof Uint8Array) {
          const result = this.encode(input);
          return result instanceof Uint8Array ? create6(this.code, result) : result.then((digest5) => create6(this.code, digest5));
        } else {
          throw Error("Unknown type, must be binary type");
        }
      }
    };
  }
});

// ../../../node_modules/multihashes/node_modules/multiformats/esm/src/hashes/sha2-browser.js
var sha2_browser_exports4 = {};
__export(sha2_browser_exports4, {
  sha256: () => sha2565,
  sha512: () => sha5124
});
var sha4, sha2565, sha5124;
var init_sha2_browser = __esm({
  "../../../node_modules/multihashes/node_modules/multiformats/esm/src/hashes/sha2-browser.js"() {
    init_hasher();
    sha4 = (name5) => async (data) => new Uint8Array(await crypto.subtle.digest(name5, data));
    sha2565 = from8({
      name: "sha2-256",
      code: 18,
      encode: sha4("SHA-256")
    });
    sha5124 = from8({
      name: "sha2-512",
      code: 19,
      encode: sha4("SHA-512")
    });
  }
});

// ../../../node_modules/multihashes/node_modules/multiformats/esm/src/hashes/identity.js
var identity_exports8 = {};
__export(identity_exports8, {
  identity: () => identity8
});
var code4, name4, encode17, digest4, identity8;
var init_identity2 = __esm({
  "../../../node_modules/multihashes/node_modules/multiformats/esm/src/hashes/identity.js"() {
    init_bytes();
    init_digest();
    code4 = 0;
    name4 = "identity";
    encode17 = coerce4;
    digest4 = (input) => create6(code4, encode17(input));
    identity8 = {
      code: code4,
      name: name4,
      encode: encode17,
      digest: digest4
    };
  }
});

// ../../../node_modules/multihashes/node_modules/multiformats/esm/src/codecs/raw.js
var init_raw = __esm({
  "../../../node_modules/multihashes/node_modules/multiformats/esm/src/codecs/raw.js"() {
    init_bytes();
  }
});

// ../../../node_modules/multihashes/node_modules/multiformats/esm/src/codecs/json.js
var textEncoder4, textDecoder4;
var init_json = __esm({
  "../../../node_modules/multihashes/node_modules/multiformats/esm/src/codecs/json.js"() {
    textEncoder4 = new TextEncoder();
    textDecoder4 = new TextDecoder();
  }
});

// ../../../node_modules/multihashes/node_modules/multiformats/esm/src/cid.js
var CID4, parseCIDtoBytes4, toStringV04, toStringV14, DAG_PB_CODE4, SHA_256_CODE4, encodeCID4, cidSymbol4, readonly, hidden, version, deprecate, IS_CID_DEPRECATION;
var init_cid = __esm({
  "../../../node_modules/multihashes/node_modules/multiformats/esm/src/cid.js"() {
    init_varint2();
    init_digest();
    init_base58();
    init_base32();
    init_bytes();
    CID4 = class _CID {
      constructor(version2, code5, multihash, bytes2) {
        this.code = code5;
        this.version = version2;
        this.multihash = multihash;
        this.bytes = bytes2;
        this.byteOffset = bytes2.byteOffset;
        this.byteLength = bytes2.byteLength;
        this.asCID = this;
        this._baseCache = /* @__PURE__ */ new Map();
        Object.defineProperties(this, {
          byteOffset: hidden,
          byteLength: hidden,
          code: readonly,
          version: readonly,
          multihash: readonly,
          bytes: readonly,
          _baseCache: hidden,
          asCID: hidden
        });
      }
      toV0() {
        switch (this.version) {
          case 0: {
            return this;
          }
          default: {
            const { code: code5, multihash } = this;
            if (code5 !== DAG_PB_CODE4) {
              throw new Error("Cannot convert a non dag-pb CID to CIDv0");
            }
            if (multihash.code !== SHA_256_CODE4) {
              throw new Error("Cannot convert non sha2-256 multihash CID to CIDv0");
            }
            return _CID.createV0(multihash);
          }
        }
      }
      toV1() {
        switch (this.version) {
          case 0: {
            const { code: code5, digest: digest5 } = this.multihash;
            const multihash = create6(code5, digest5);
            return _CID.createV1(this.code, multihash);
          }
          case 1: {
            return this;
          }
          default: {
            throw Error(`Can not convert CID version ${this.version} to version 0. This is a bug please report`);
          }
        }
      }
      equals(other) {
        return other && this.code === other.code && this.version === other.version && equals9(this.multihash, other.multihash);
      }
      toString(base7) {
        const { bytes: bytes2, version: version2, _baseCache } = this;
        switch (version2) {
          case 0:
            return toStringV04(bytes2, _baseCache, base7 || base58btc4.encoder);
          default:
            return toStringV14(bytes2, _baseCache, base7 || base324.encoder);
        }
      }
      toJSON() {
        return {
          code: this.code,
          version: this.version,
          hash: this.multihash.bytes
        };
      }
      get [Symbol.toStringTag]() {
        return "CID";
      }
      [Symbol.for("nodejs.util.inspect.custom")]() {
        return "CID(" + this.toString() + ")";
      }
      static isCID(value) {
        deprecate(/^0\.0/, IS_CID_DEPRECATION);
        return !!(value && (value[cidSymbol4] || value.asCID === value));
      }
      get toBaseEncodedString() {
        throw new Error("Deprecated, use .toString()");
      }
      get codec() {
        throw new Error('"codec" property is deprecated, use integer "code" property instead');
      }
      get buffer() {
        throw new Error("Deprecated .buffer property, use .bytes to get Uint8Array instead");
      }
      get multibaseName() {
        throw new Error('"multibaseName" property is deprecated');
      }
      get prefix() {
        throw new Error('"prefix" property is deprecated');
      }
      static asCID(value) {
        if (value instanceof _CID) {
          return value;
        } else if (value != null && value.asCID === value) {
          const { version: version2, code: code5, multihash, bytes: bytes2 } = value;
          return new _CID(version2, code5, multihash, bytes2 || encodeCID4(version2, code5, multihash.bytes));
        } else if (value != null && value[cidSymbol4] === true) {
          const { version: version2, multihash, code: code5 } = value;
          const digest5 = decode21(multihash);
          return _CID.create(version2, code5, digest5);
        } else {
          return null;
        }
      }
      static create(version2, code5, digest5) {
        if (typeof code5 !== "number") {
          throw new Error("String codecs are no longer supported");
        }
        switch (version2) {
          case 0: {
            if (code5 !== DAG_PB_CODE4) {
              throw new Error(`Version 0 CID must use dag-pb (code: ${DAG_PB_CODE4}) block encoding`);
            } else {
              return new _CID(version2, code5, digest5, digest5.bytes);
            }
          }
          case 1: {
            const bytes2 = encodeCID4(version2, code5, digest5.bytes);
            return new _CID(version2, code5, digest5, bytes2);
          }
          default: {
            throw new Error("Invalid version");
          }
        }
      }
      static createV0(digest5) {
        return _CID.create(0, DAG_PB_CODE4, digest5);
      }
      static createV1(code5, digest5) {
        return _CID.create(1, code5, digest5);
      }
      static decode(bytes2) {
        const [cid, remainder] = _CID.decodeFirst(bytes2);
        if (remainder.length) {
          throw new Error("Incorrect length");
        }
        return cid;
      }
      static decodeFirst(bytes2) {
        const specs = _CID.inspectBytes(bytes2);
        const prefixSize = specs.size - specs.multihashSize;
        const multihashBytes = coerce4(bytes2.subarray(prefixSize, prefixSize + specs.multihashSize));
        if (multihashBytes.byteLength !== specs.multihashSize) {
          throw new Error("Incorrect length");
        }
        const digestBytes = multihashBytes.subarray(specs.multihashSize - specs.digestSize);
        const digest5 = new Digest4(specs.multihashCode, specs.digestSize, digestBytes, multihashBytes);
        const cid = specs.version === 0 ? _CID.createV0(digest5) : _CID.createV1(specs.codec, digest5);
        return [
          cid,
          bytes2.subarray(specs.size)
        ];
      }
      static inspectBytes(initialBytes) {
        let offset = 0;
        const next = () => {
          const [i, length5] = decode20(initialBytes.subarray(offset));
          offset += length5;
          return i;
        };
        let version2 = next();
        let codec = DAG_PB_CODE4;
        if (version2 === 18) {
          version2 = 0;
          offset = 0;
        } else if (version2 === 1) {
          codec = next();
        }
        if (version2 !== 0 && version2 !== 1) {
          throw new RangeError(`Invalid CID version ${version2}`);
        }
        const prefixSize = offset;
        const multihashCode = next();
        const digestSize = next();
        const size = offset + digestSize;
        const multihashSize = size - prefixSize;
        return {
          version: version2,
          codec,
          multihashCode,
          digestSize,
          multihashSize,
          size
        };
      }
      static parse(source, base7) {
        const [prefix, bytes2] = parseCIDtoBytes4(source, base7);
        const cid = _CID.decode(bytes2);
        cid._baseCache.set(prefix, source);
        return cid;
      }
    };
    parseCIDtoBytes4 = (source, base7) => {
      switch (source[0]) {
        case "Q": {
          const decoder = base7 || base58btc4;
          return [
            base58btc4.prefix,
            decoder.decode(`${base58btc4.prefix}${source}`)
          ];
        }
        case base58btc4.prefix: {
          const decoder = base7 || base58btc4;
          return [
            base58btc4.prefix,
            decoder.decode(source)
          ];
        }
        case base324.prefix: {
          const decoder = base7 || base324;
          return [
            base324.prefix,
            decoder.decode(source)
          ];
        }
        default: {
          if (base7 == null) {
            throw Error("To parse non base32 or base58btc encoded CID multibase decoder must be provided");
          }
          return [
            source[0],
            base7.decode(source)
          ];
        }
      }
    };
    toStringV04 = (bytes2, cache4, base7) => {
      const { prefix } = base7;
      if (prefix !== base58btc4.prefix) {
        throw Error(`Cannot string encode V0 in ${base7.name} encoding`);
      }
      const cid = cache4.get(prefix);
      if (cid == null) {
        const cid2 = base7.encode(bytes2).slice(1);
        cache4.set(prefix, cid2);
        return cid2;
      } else {
        return cid;
      }
    };
    toStringV14 = (bytes2, cache4, base7) => {
      const { prefix } = base7;
      const cid = cache4.get(prefix);
      if (cid == null) {
        const cid2 = base7.encode(bytes2);
        cache4.set(prefix, cid2);
        return cid2;
      } else {
        return cid;
      }
    };
    DAG_PB_CODE4 = 112;
    SHA_256_CODE4 = 18;
    encodeCID4 = (version2, code5, multihash) => {
      const codeOffset = encodingLength4(version2);
      const hashOffset = codeOffset + encodingLength4(code5);
      const bytes2 = new Uint8Array(hashOffset + multihash.byteLength);
      encodeTo4(version2, bytes2, 0);
      encodeTo4(code5, bytes2, codeOffset);
      bytes2.set(multihash, hashOffset);
      return bytes2;
    };
    cidSymbol4 = Symbol.for("@ipld/js-cid/CID");
    readonly = {
      writable: false,
      configurable: false,
      enumerable: true
    };
    hidden = {
      writable: false,
      enumerable: false,
      configurable: false
    };
    version = "0.0.0-dev";
    deprecate = (range, message2) => {
      if (range.test(version)) {
        console.warn(message2);
      } else {
        throw new Error(message2);
      }
    };
    IS_CID_DEPRECATION = `CID.isCID(v) is deprecated and will be removed in the next major release.
Following code pattern:

if (CID.isCID(value)) {
  doSomethingWithCID(value)
}

Is replaced with:

const cid = CID.asCID(value)
if (cid) {
  // Make sure to use cid instead of value
  doSomethingWithCID(cid)
}
`;
  }
});

// ../../../node_modules/multihashes/node_modules/multiformats/esm/src/index.js
var init_src = __esm({
  "../../../node_modules/multihashes/node_modules/multiformats/esm/src/index.js"() {
    init_cid();
    init_varint2();
    init_bytes();
    init_hasher();
    init_digest();
  }
});

// ../../../node_modules/multihashes/node_modules/multiformats/esm/src/basics.js
var bases4, hashes4;
var init_basics = __esm({
  "../../../node_modules/multihashes/node_modules/multiformats/esm/src/basics.js"() {
    init_identity();
    init_base2();
    init_base8();
    init_base10();
    init_base16();
    init_base32();
    init_base36();
    init_base58();
    init_base64();
    init_base256emoji();
    init_sha2_browser();
    init_identity2();
    init_raw();
    init_json();
    init_src();
    bases4 = {
      ...identity_exports7,
      ...base2_exports4,
      ...base8_exports4,
      ...base10_exports4,
      ...base16_exports4,
      ...base32_exports4,
      ...base36_exports4,
      ...base58_exports4,
      ...base64_exports4,
      ...base256emoji_exports4
    };
    hashes4 = {
      ...sha2_browser_exports4,
      ...identity_exports8
    };
  }
});

// ../../../node_modules/multihashes/node_modules/uint8arrays/esm/src/util/as-uint8array.js
function asUint8Array2(buf) {
  if (globalThis.Buffer != null) {
    return new Uint8Array(buf.buffer, buf.byteOffset, buf.byteLength);
  }
  return buf;
}
var init_as_uint8array = __esm({
  "../../../node_modules/multihashes/node_modules/uint8arrays/esm/src/util/as-uint8array.js"() {
  }
});

// ../../../node_modules/multihashes/node_modules/uint8arrays/esm/src/alloc.js
function allocUnsafe3(size = 0) {
  if (globalThis.Buffer != null && globalThis.Buffer.allocUnsafe != null) {
    return asUint8Array2(globalThis.Buffer.allocUnsafe(size));
  }
  return new Uint8Array(size);
}
var init_alloc = __esm({
  "../../../node_modules/multihashes/node_modules/uint8arrays/esm/src/alloc.js"() {
    init_as_uint8array();
  }
});

// ../../../node_modules/multihashes/node_modules/uint8arrays/esm/src/util/bases.js
function createCodec3(name5, prefix, encode19, decode24) {
  return {
    name: name5,
    prefix,
    encoder: {
      name: name5,
      prefix,
      encode: encode19
    },
    decoder: { decode: decode24 }
  };
}
var string2, ascii2, BASES2, bases_default2;
var init_bases = __esm({
  "../../../node_modules/multihashes/node_modules/uint8arrays/esm/src/util/bases.js"() {
    init_basics();
    init_alloc();
    string2 = createCodec3("utf8", "u", (buf) => {
      const decoder = new TextDecoder("utf8");
      return "u" + decoder.decode(buf);
    }, (str) => {
      const encoder = new TextEncoder();
      return encoder.encode(str.substring(1));
    });
    ascii2 = createCodec3("ascii", "a", (buf) => {
      let string3 = "a";
      for (let i = 0; i < buf.length; i++) {
        string3 += String.fromCharCode(buf[i]);
      }
      return string3;
    }, (str) => {
      str = str.substring(1);
      const buf = allocUnsafe3(str.length);
      for (let i = 0; i < str.length; i++) {
        buf[i] = str.charCodeAt(i);
      }
      return buf;
    });
    BASES2 = {
      utf8: string2,
      "utf-8": string2,
      hex: bases4.base16,
      latin1: ascii2,
      ascii: ascii2,
      binary: ascii2,
      ...bases4
    };
    bases_default2 = BASES2;
  }
});

// ../../../node_modules/multihashes/node_modules/uint8arrays/esm/src/to-string.js
var to_string_exports = {};
__export(to_string_exports, {
  toString: () => toString7
});
function toString7(array, encoding = "utf8") {
  const base7 = bases_default2[encoding];
  if (!base7) {
    throw new Error(`Unsupported encoding "${encoding}"`);
  }
  if ((encoding === "utf8" || encoding === "utf-8") && globalThis.Buffer != null && globalThis.Buffer.from != null) {
    return globalThis.Buffer.from(array.buffer, array.byteOffset, array.byteLength).toString("utf8");
  }
  return base7.encoder.encode(array).substring(1);
}
var init_to_string = __esm({
  "../../../node_modules/multihashes/node_modules/uint8arrays/esm/src/to-string.js"() {
    init_bases();
  }
});

// ../../../node_modules/multihashes/node_modules/uint8arrays/esm/src/from-string.js
var from_string_exports = {};
__export(from_string_exports, {
  fromString: () => fromString7
});
function fromString7(string3, encoding = "utf8") {
  const base7 = bases_default2[encoding];
  if (!base7) {
    throw new Error(`Unsupported encoding "${encoding}"`);
  }
  if ((encoding === "utf8" || encoding === "utf-8") && globalThis.Buffer != null && globalThis.Buffer.from != null) {
    return asUint8Array2(globalThis.Buffer.from(string3, "utf-8"));
  }
  return base7.decoder.decode(`${base7.prefix}${string3}`);
}
var init_from_string = __esm({
  "../../../node_modules/multihashes/node_modules/uint8arrays/esm/src/from-string.js"() {
    init_bases();
    init_as_uint8array();
  }
});

// ../../../node_modules/multihashes/node_modules/uint8arrays/esm/src/concat.js
var concat_exports = {};
__export(concat_exports, {
  concat: () => concat2
});
function concat2(arrays, length5) {
  if (!length5) {
    length5 = arrays.reduce((acc, curr) => acc + curr.length, 0);
  }
  const output2 = allocUnsafe3(length5);
  let offset = 0;
  for (const arr of arrays) {
    output2.set(arr, offset);
    offset += arr.length;
  }
  return asUint8Array2(output2);
}
var init_concat = __esm({
  "../../../node_modules/multihashes/node_modules/uint8arrays/esm/src/concat.js"() {
    init_alloc();
    init_as_uint8array();
  }
});

// ../../../node_modules/multihashes/src/index.js
var require_src3 = __commonJS({
  "../../../node_modules/multihashes/src/index.js"(exports2, module2) {
    "use strict";
    var multibase = require_src2();
    var varint8 = require_varint2();
    var { names: names3 } = require_constants2();
    var { toString: uint8ArrayToString } = (init_to_string(), __toCommonJS(to_string_exports));
    var { fromString: uint8ArrayFromString } = (init_from_string(), __toCommonJS(from_string_exports));
    var { concat: uint8ArrayConcat } = (init_concat(), __toCommonJS(concat_exports));
    var codes3 = (
      /** @type {import('./types').CodeNameMap} */
      {}
    );
    for (const key in names3) {
      const name5 = (
        /** @type {HashName} */
        key
      );
      codes3[names3[name5]] = name5;
    }
    Object.freeze(codes3);
    function toHexString(hash2) {
      if (!(hash2 instanceof Uint8Array)) {
        throw new Error("must be passed a Uint8Array");
      }
      return uint8ArrayToString(hash2, "base16");
    }
    function fromHexString(hash2) {
      return uint8ArrayFromString(hash2, "base16");
    }
    function toB58String(hash2) {
      if (!(hash2 instanceof Uint8Array)) {
        throw new Error("must be passed a Uint8Array");
      }
      return uint8ArrayToString(multibase.encode("base58btc", hash2)).slice(1);
    }
    function fromB58String(hash2) {
      const encoded = hash2 instanceof Uint8Array ? uint8ArrayToString(hash2) : hash2;
      return multibase.decode("z" + encoded);
    }
    function decode24(bytes2) {
      if (!(bytes2 instanceof Uint8Array)) {
        throw new Error("multihash must be a Uint8Array");
      }
      if (bytes2.length < 2) {
        throw new Error("multihash too short. must be > 2 bytes.");
      }
      const code5 = (
        /** @type {HashCode} */
        varint8.decode(bytes2)
      );
      if (!isValidCode(code5)) {
        throw new Error(`multihash unknown function code: 0x${code5.toString(16)}`);
      }
      bytes2 = bytes2.slice(varint8.decode.bytes);
      const len = varint8.decode(bytes2);
      if (len < 0) {
        throw new Error(`multihash invalid length: ${len}`);
      }
      bytes2 = bytes2.slice(varint8.decode.bytes);
      if (bytes2.length !== len) {
        throw new Error(`multihash length inconsistent: 0x${uint8ArrayToString(bytes2, "base16")}`);
      }
      return {
        code: code5,
        name: codes3[code5],
        length: len,
        digest: bytes2
      };
    }
    function encode19(digest5, code5, length5) {
      if (!digest5 || code5 === void 0) {
        throw new Error("multihash encode requires at least two args: digest, code");
      }
      const hashfn = coerceCode(code5);
      if (!(digest5 instanceof Uint8Array)) {
        throw new Error("digest should be a Uint8Array");
      }
      if (length5 == null) {
        length5 = digest5.length;
      }
      if (length5 && digest5.length !== length5) {
        throw new Error("digest length should be equal to specified length.");
      }
      const hash2 = varint8.encode(hashfn);
      const len = varint8.encode(length5);
      return uint8ArrayConcat([hash2, len, digest5], hash2.length + len.length + digest5.length);
    }
    function coerceCode(name5) {
      let code5 = name5;
      if (typeof name5 === "string") {
        if (names3[name5] === void 0) {
          throw new Error(`Unrecognized hash function named: ${name5}`);
        }
        code5 = names3[name5];
      }
      if (typeof code5 !== "number") {
        throw new Error(`Hash function code should be a number. Got: ${code5}`);
      }
      if (codes3[code5] === void 0 && !isAppCode(code5)) {
        throw new Error(`Unrecognized function code: ${code5}`);
      }
      return code5;
    }
    function isAppCode(code5) {
      return code5 > 0 && code5 < 16;
    }
    function isValidCode(code5) {
      if (isAppCode(code5)) {
        return true;
      }
      if (codes3[code5]) {
        return true;
      }
      return false;
    }
    function validate(multihash) {
      decode24(multihash);
    }
    function prefix(multihash) {
      validate(multihash);
      return multihash.subarray(0, 2);
    }
    module2.exports = {
      names: names3,
      codes: codes3,
      toHexString,
      fromHexString,
      toB58String,
      fromB58String,
      decode: decode24,
      encode: encode19,
      coerceCode,
      isAppCode,
      validate,
      prefix,
      isValidCode
    };
  }
});

// ../../interface/dist/src/errors.js
var CodeError = class extends Error {
  code;
  props;
  constructor(message2, code5, props) {
    super(message2);
    this.code = code5;
    this.name = props?.name ?? "CodeError";
    this.props = props ?? {};
  }
};

// ../../interface/dist/src/transport/index.js
var symbol = Symbol.for("@libp2p/transport");
var FaultTolerance;
(function(FaultTolerance2) {
  FaultTolerance2[FaultTolerance2["FATAL_ALL"] = 0] = "FATAL_ALL";
  FaultTolerance2[FaultTolerance2["NO_FATAL"] = 1] = "NO_FATAL";
})(FaultTolerance || (FaultTolerance = {}));

// ../../logger/dist/src/index.js
var import_debug = __toESM(require_browser(), 1);

// ../../../node_modules/multiformats/src/bases/base32.js
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

// ../../../node_modules/multiformats/vendor/base-x.js
function base(ALPHABET, name5) {
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
  function encode19(source) {
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
    var length5 = 0;
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
      for (var it1 = size - 1; (carry !== 0 || i2 < length5) && it1 !== -1; it1--, i2++) {
        carry += 256 * b58[it1] >>> 0;
        b58[it1] = carry % BASE >>> 0;
        carry = carry / BASE >>> 0;
      }
      if (carry !== 0) {
        throw new Error("Non-zero carry");
      }
      length5 = i2;
      pbegin++;
    }
    var it2 = size - length5;
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
    var length5 = 0;
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
      for (var it3 = size - 1; (carry !== 0 || i2 < length5) && it3 !== -1; it3--, i2++) {
        carry += BASE * b256[it3] >>> 0;
        b256[it3] = carry % 256 >>> 0;
        carry = carry / 256 >>> 0;
      }
      if (carry !== 0) {
        throw new Error("Non-zero carry");
      }
      length5 = i2;
      psz++;
    }
    if (source[psz] === " ") {
      return;
    }
    var it4 = size - length5;
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
  function decode24(string3) {
    var buffer = decodeUnsafe(string3);
    if (buffer) {
      return buffer;
    }
    throw new Error(`Non-${name5} character`);
  }
  return {
    encode: encode19,
    decodeUnsafe,
    decode: decode24
  };
}
var src = base;
var _brrp__multiformats_scope_baseX = src;
var base_x_default = _brrp__multiformats_scope_baseX;

// ../../../node_modules/multiformats/src/bytes.js
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

// ../../../node_modules/multiformats/src/bases/base.js
var Encoder = class {
  /**
   * @param {Base} name
   * @param {Prefix} prefix
   * @param {(bytes:Uint8Array) => string} baseEncode
   */
  constructor(name5, prefix, baseEncode) {
    this.name = name5;
    this.prefix = prefix;
    this.baseEncode = baseEncode;
  }
  /**
   * @param {Uint8Array} bytes
   * @returns {API.Multibase<Prefix>}
   */
  encode(bytes2) {
    if (bytes2 instanceof Uint8Array) {
      return `${this.prefix}${this.baseEncode(bytes2)}`;
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
  constructor(name5, prefix, baseDecode) {
    this.name = name5;
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
  constructor(decoders2) {
    this.decoders = decoders2;
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
  constructor(name5, prefix, baseEncode, baseDecode) {
    this.name = name5;
    this.prefix = prefix;
    this.baseEncode = baseEncode;
    this.baseDecode = baseDecode;
    this.encoder = new Encoder(name5, prefix, baseEncode);
    this.decoder = new Decoder(name5, prefix, baseDecode);
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
var from = ({ name: name5, prefix, encode: encode19, decode: decode24 }) => new Codec(name5, prefix, encode19, decode24);
var baseX = ({ prefix, name: name5, alphabet: alphabet5 }) => {
  const { encode: encode19, decode: decode24 } = base_x_default(alphabet5, name5);
  return from({
    prefix,
    name: name5,
    encode: encode19,
    /**
     * @param {string} text
     */
    decode: (text) => coerce(decode24(text))
  });
};
var decode = (string3, alphabet5, bitsPerChar, name5) => {
  const codes3 = {};
  for (let i = 0; i < alphabet5.length; ++i) {
    codes3[alphabet5[i]] = i;
  }
  let end = string3.length;
  while (string3[end - 1] === "=") {
    --end;
  }
  const out = new Uint8Array(end * bitsPerChar / 8 | 0);
  let bits2 = 0;
  let buffer = 0;
  let written = 0;
  for (let i = 0; i < end; ++i) {
    const value = codes3[string3[i]];
    if (value === void 0) {
      throw new SyntaxError(`Non-${name5} character`);
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
var encode = (data, alphabet5, bitsPerChar) => {
  const pad = alphabet5[alphabet5.length - 1] === "=";
  const mask = (1 << bitsPerChar) - 1;
  let out = "";
  let bits2 = 0;
  let buffer = 0;
  for (let i = 0; i < data.length; ++i) {
    buffer = buffer << 8 | data[i];
    bits2 += 8;
    while (bits2 > bitsPerChar) {
      bits2 -= bitsPerChar;
      out += alphabet5[mask & buffer >> bits2];
    }
  }
  if (bits2) {
    out += alphabet5[mask & buffer << bitsPerChar - bits2];
  }
  if (pad) {
    while (out.length * bitsPerChar & 7) {
      out += "=";
    }
  }
  return out;
};
var rfc4648 = ({ name: name5, prefix, bitsPerChar, alphabet: alphabet5 }) => {
  return from({
    prefix,
    name: name5,
    encode(input) {
      return encode(input, alphabet5, bitsPerChar);
    },
    decode(input) {
      return decode(input, alphabet5, bitsPerChar, name5);
    }
  });
};

// ../../../node_modules/multiformats/src/bases/base32.js
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

// ../../../node_modules/multiformats/src/bases/base58.js
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

// ../../../node_modules/multiformats/src/bases/base64.js
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

// ../../logger/dist/src/index.js
import_debug.default.formatters.b = (v) => {
  return v == null ? "undefined" : base58btc.baseEncode(v);
};
import_debug.default.formatters.t = (v) => {
  return v == null ? "undefined" : base32.baseEncode(v);
};
import_debug.default.formatters.m = (v) => {
  return v == null ? "undefined" : base64.baseEncode(v);
};
import_debug.default.formatters.p = (v) => {
  return v == null ? "undefined" : v.toString();
};
import_debug.default.formatters.c = (v) => {
  return v == null ? "undefined" : v.toString();
};
import_debug.default.formatters.k = (v) => {
  return v == null ? "undefined" : v.toString();
};
import_debug.default.formatters.a = (v) => {
  return v == null ? "undefined" : v.toString();
};
function createDisabledLogger(namespace) {
  const logger2 = () => {
  };
  logger2.enabled = false;
  logger2.color = "";
  logger2.diff = 0;
  logger2.log = () => {
  };
  logger2.namespace = namespace;
  logger2.destroy = () => true;
  logger2.extend = () => logger2;
  return logger2;
}
function logger(name5) {
  let trace = createDisabledLogger(`${name5}:trace`);
  if (import_debug.default.enabled(`${name5}:trace`) && import_debug.default.names.map((r) => r.toString()).find((n) => n.includes(":trace")) != null) {
    trace = (0, import_debug.default)(`${name5}:trace`);
  }
  return Object.assign((0, import_debug.default)(name5), {
    error: (0, import_debug.default)(`${name5}:error`),
    trace
  });
}

// ../../interface/dist/src/peer-id/index.js
var symbol2 = Symbol.for("@libp2p/peer-id");

// ../../../node_modules/multiformats/src/bases/identity.js
var identity_exports = {};
__export(identity_exports, {
  identity: () => identity
});
var identity = from({
  prefix: "\0",
  name: "identity",
  encode: (buf) => toString(buf),
  decode: (str) => fromString(str)
});

// ../../../node_modules/multiformats/src/bases/base2.js
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

// ../../../node_modules/multiformats/src/bases/base8.js
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

// ../../../node_modules/multiformats/src/bases/base10.js
var base10_exports = {};
__export(base10_exports, {
  base10: () => base10
});
var base10 = baseX({
  prefix: "9",
  name: "base10",
  alphabet: "0123456789"
});

// ../../../node_modules/multiformats/src/bases/base16.js
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

// ../../../node_modules/multiformats/src/bases/base36.js
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

// ../../../node_modules/multiformats/src/bases/base256emoji.js
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

// ../../../node_modules/multiformats/src/hashes/sha2-browser.js
var sha2_browser_exports = {};
__export(sha2_browser_exports, {
  sha256: () => sha256,
  sha512: () => sha512
});

// ../../../node_modules/multiformats/vendor/varint.js
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

// ../../../node_modules/multiformats/src/varint.js
var decode4 = (data, offset = 0) => {
  const code5 = varint_default.decode(data, offset);
  return [code5, varint_default.decode.bytes];
};
var encodeTo = (int, target, offset = 0) => {
  varint_default.encode(int, target, offset);
  return target;
};
var encodingLength = (int) => {
  return varint_default.encodingLength(int);
};

// ../../../node_modules/multiformats/src/hashes/digest.js
var create = (code5, digest5) => {
  const size = digest5.byteLength;
  const sizeOffset = encodingLength(code5);
  const digestOffset = sizeOffset + encodingLength(size);
  const bytes2 = new Uint8Array(digestOffset + size);
  encodeTo(code5, bytes2, 0);
  encodeTo(size, bytes2, sizeOffset);
  bytes2.set(digest5, digestOffset);
  return new Digest(code5, size, digest5, bytes2);
};
var decode5 = (multihash) => {
  const bytes2 = coerce(multihash);
  const [code5, sizeOffset] = decode4(bytes2);
  const [size, digestOffset] = decode4(bytes2.subarray(sizeOffset));
  const digest5 = bytes2.subarray(sizeOffset + digestOffset);
  if (digest5.byteLength !== size) {
    throw new Error("Incorrect length");
  }
  return new Digest(code5, size, digest5, bytes2);
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
  constructor(code5, size, digest5, bytes2) {
    this.code = code5;
    this.size = size;
    this.digest = digest5;
    this.bytes = bytes2;
  }
};

// ../../../node_modules/multiformats/src/hashes/hasher.js
var from2 = ({ name: name5, code: code5, encode: encode19 }) => new Hasher(name5, code5, encode19);
var Hasher = class {
  /**
   *
   * @param {Name} name
   * @param {Code} code
   * @param {(input: Uint8Array) => Await<Uint8Array>} encode
   */
  constructor(name5, code5, encode19) {
    this.name = name5;
    this.code = code5;
    this.encode = encode19;
  }
  /**
   * @param {Uint8Array} input
   * @returns {Await<Digest.Digest<Code, number>>}
   */
  digest(input) {
    if (input instanceof Uint8Array) {
      const result = this.encode(input);
      return result instanceof Uint8Array ? create(this.code, result) : result.then((digest5) => create(this.code, digest5));
    } else {
      throw Error("Unknown type, must be binary type");
    }
  }
};

// ../../../node_modules/multiformats/src/hashes/sha2-browser.js
var sha = (name5) => (
  /**
   * @param {Uint8Array} data
   */
  async (data) => new Uint8Array(await crypto.subtle.digest(name5, data))
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

// ../../../node_modules/multiformats/src/hashes/identity.js
var identity_exports2 = {};
__export(identity_exports2, {
  identity: () => identity2
});
var code = 0;
var name = "identity";
var encode4 = coerce;
var digest = (input) => create(code, encode4(input));
var identity2 = { code, name, encode: encode4, digest };

// ../../../node_modules/multiformats/src/codecs/json.js
var textEncoder = new TextEncoder();
var textDecoder = new TextDecoder();

// ../../../node_modules/multiformats/src/cid.js
var format = (link, base7) => {
  const { bytes: bytes2, version: version2 } = link;
  switch (version2) {
    case 0:
      return toStringV0(
        bytes2,
        baseCache(link),
        /** @type {API.MultibaseEncoder<"z">} */
        base7 || base58btc.encoder
      );
    default:
      return toStringV1(
        bytes2,
        baseCache(link),
        /** @type {API.MultibaseEncoder<Prefix>} */
        base7 || base32.encoder
      );
  }
};
var cache = /* @__PURE__ */ new WeakMap();
var baseCache = (cid) => {
  const baseCache4 = cache.get(cid);
  if (baseCache4 == null) {
    const baseCache5 = /* @__PURE__ */ new Map();
    cache.set(cid, baseCache5);
    return baseCache5;
  }
  return baseCache4;
};
var CID = class _CID {
  /**
   * @param {Version} version - Version of the CID
   * @param {Format} code - Code of the codec content is encoded in, see https://github.com/multiformats/multicodec/blob/master/table.csv
   * @param {API.MultihashDigest<Alg>} multihash - (Multi)hash of the of the content.
   * @param {Uint8Array} bytes
   *
   */
  constructor(version2, code5, multihash, bytes2) {
    this.code = code5;
    this.version = version2;
    this.multihash = multihash;
    this.bytes = bytes2;
    this["/"] = bytes2;
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
        const { code: code5, multihash } = this;
        if (code5 !== DAG_PB_CODE) {
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
        const { code: code5, digest: digest5 } = this.multihash;
        const multihash = create(code5, digest5);
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
  toString(base7) {
    return format(this, base7);
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
      const { version: version2, code: code5, multihash, bytes: bytes2 } = value;
      return new _CID(
        version2,
        code5,
        /** @type {API.MultihashDigest<Alg>} */
        multihash,
        bytes2 || encodeCID(version2, code5, multihash.bytes)
      );
    } else if (value[cidSymbol] === true) {
      const { version: version2, multihash, code: code5 } = value;
      const digest5 = (
        /** @type {API.MultihashDigest<Alg>} */
        decode5(multihash)
      );
      return _CID.create(version2, code5, digest5);
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
  static create(version2, code5, digest5) {
    if (typeof code5 !== "number") {
      throw new Error("String codecs are no longer supported");
    }
    if (!(digest5.bytes instanceof Uint8Array)) {
      throw new Error("Invalid digest");
    }
    switch (version2) {
      case 0: {
        if (code5 !== DAG_PB_CODE) {
          throw new Error(
            `Version 0 CID must use dag-pb (code: ${DAG_PB_CODE}) block encoding`
          );
        } else {
          return new _CID(version2, code5, digest5, digest5.bytes);
        }
      }
      case 1: {
        const bytes2 = encodeCID(version2, code5, digest5.bytes);
        return new _CID(version2, code5, digest5, bytes2);
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
  static createV0(digest5) {
    return _CID.create(0, DAG_PB_CODE, digest5);
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
  static createV1(code5, digest5) {
    return _CID.create(1, code5, digest5);
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
  static decode(bytes2) {
    const [cid, remainder] = _CID.decodeFirst(bytes2);
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
  static decodeFirst(bytes2) {
    const specs = _CID.inspectBytes(bytes2);
    const prefixSize = specs.size - specs.multihashSize;
    const multihashBytes = coerce(
      bytes2.subarray(prefixSize, prefixSize + specs.multihashSize)
    );
    if (multihashBytes.byteLength !== specs.multihashSize) {
      throw new Error("Incorrect length");
    }
    const digestBytes = multihashBytes.subarray(
      specs.multihashSize - specs.digestSize
    );
    const digest5 = new Digest(
      specs.multihashCode,
      specs.digestSize,
      digestBytes,
      multihashBytes
    );
    const cid = specs.version === 0 ? _CID.createV0(
      /** @type {API.MultihashDigest<API.SHA_256>} */
      digest5
    ) : _CID.createV1(specs.codec, digest5);
    return [
      /** @type {CID<T, C, A, V>} */
      cid,
      bytes2.subarray(specs.size)
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
      const [i, length5] = decode4(initialBytes.subarray(offset));
      offset += length5;
      return i;
    };
    let version2 = (
      /** @type {V} */
      next()
    );
    let codec = (
      /** @type {C} */
      DAG_PB_CODE
    );
    if (
      /** @type {number} */
      version2 === 18
    ) {
      version2 = /** @type {V} */
      0;
      offset = 0;
    } else {
      codec = /** @type {C} */
      next();
    }
    if (version2 !== 0 && version2 !== 1) {
      throw new RangeError(`Invalid CID version ${version2}`);
    }
    const prefixSize = offset;
    const multihashCode = (
      /** @type {A} */
      next()
    );
    const digestSize = next();
    const size = offset + digestSize;
    const multihashSize = size - prefixSize;
    return { version: version2, codec, multihashCode, digestSize, multihashSize, size };
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
  static parse(source, base7) {
    const [prefix, bytes2] = parseCIDtoBytes(source, base7);
    const cid = _CID.decode(bytes2);
    if (cid.version === 0 && source[0] !== "Q") {
      throw Error("Version 0 CID string must not include multibase prefix");
    }
    baseCache(cid).set(prefix, source);
    return cid;
  }
};
var parseCIDtoBytes = (source, base7) => {
  switch (source[0]) {
    case "Q": {
      const decoder = base7 || base58btc;
      return [
        /** @type {Prefix} */
        base58btc.prefix,
        decoder.decode(`${base58btc.prefix}${source}`)
      ];
    }
    case base58btc.prefix: {
      const decoder = base7 || base58btc;
      return [
        /** @type {Prefix} */
        base58btc.prefix,
        decoder.decode(source)
      ];
    }
    case base32.prefix: {
      const decoder = base7 || base32;
      return [
        /** @type {Prefix} */
        base32.prefix,
        decoder.decode(source)
      ];
    }
    default: {
      if (base7 == null) {
        throw Error(
          "To parse non base32 or base58btc encoded CID multibase decoder must be provided"
        );
      }
      return [
        /** @type {Prefix} */
        source[0],
        base7.decode(source)
      ];
    }
  }
};
var toStringV0 = (bytes2, cache4, base7) => {
  const { prefix } = base7;
  if (prefix !== base58btc.prefix) {
    throw Error(`Cannot string encode V0 in ${base7.name} encoding`);
  }
  const cid = cache4.get(prefix);
  if (cid == null) {
    const cid2 = base7.encode(bytes2).slice(1);
    cache4.set(prefix, cid2);
    return cid2;
  } else {
    return cid;
  }
};
var toStringV1 = (bytes2, cache4, base7) => {
  const { prefix } = base7;
  const cid = cache4.get(prefix);
  if (cid == null) {
    const cid2 = base7.encode(bytes2);
    cache4.set(prefix, cid2);
    return cid2;
  } else {
    return cid;
  }
};
var DAG_PB_CODE = 112;
var SHA_256_CODE = 18;
var encodeCID = (version2, code5, multihash) => {
  const codeOffset = encodingLength(version2);
  const hashOffset = codeOffset + encodingLength(code5);
  const bytes2 = new Uint8Array(hashOffset + multihash.byteLength);
  encodeTo(version2, bytes2, 0);
  encodeTo(code5, bytes2, codeOffset);
  bytes2.set(multihash, hashOffset);
  return bytes2;
};
var cidSymbol = Symbol.for("@ipld/js-cid/CID");

// ../../../node_modules/multiformats/src/basics.js
var bases = { ...identity_exports, ...base2_exports, ...base8_exports, ...base10_exports, ...base16_exports, ...base32_exports, ...base36_exports, ...base58_exports, ...base64_exports, ...base256emoji_exports };
var hashes = { ...sha2_browser_exports, ...identity_exports2 };

// ../../../node_modules/uint8arrays/dist/src/equals.js
function equals3(a, b) {
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

// ../../peer-id/dist/src/index.js
var inspect = Symbol.for("nodejs.util.inspect.custom");
var baseDecoder = Object.values(bases).map((codec) => codec.decoder).reduce((acc, curr) => acc.or(curr), bases.identity.decoder);
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
  [symbol2] = true;
  toString() {
    if (this.string == null) {
      this.string = base58btc.encode(this.multihash.bytes).slice(1);
    }
    return this.string;
  }
  // return self-describing String representation
  // in default format from RFC 0001: https://github.com/libp2p/specs/pull/209
  toCID() {
    return CID.createV1(LIBP2P_KEY_CODE, this.multihash);
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
      return equals3(this.multihash.bytes, id);
    } else if (typeof id === "string") {
      return peerIdFromString(id).equals(this);
    } else if (id?.multihash?.bytes != null) {
      return equals3(this.multihash.bytes, id.multihash.bytes);
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
    const multihash = decode5(base58btc.decode(`z${str}`));
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
    const multihash = decode5(buf);
    if (multihash.code === identity2.code) {
      if (multihash.digest.length === MARSHALLED_ED225519_PUBLIC_KEY_LENGTH) {
        return new Ed25519PeerIdImpl({ multihash });
      } else if (multihash.digest.length === MARSHALLED_SECP256K1_PUBLIC_KEY_LENGTH) {
        return new Secp256k1PeerIdImpl({ multihash });
      }
    }
    if (multihash.code === sha256.code) {
      return new RSAPeerIdImpl({ multihash });
    }
  } catch {
    return peerIdFromCID(CID.decode(buf));
  }
  throw new Error("Supplied PeerID CID is invalid");
}
function peerIdFromCID(cid) {
  if (cid == null || cid.multihash == null || cid.version == null || cid.version === 1 && cid.code !== LIBP2P_KEY_CODE) {
    throw new Error("Supplied PeerID CID is invalid");
  }
  const multihash = cid.multihash;
  if (multihash.code === sha256.code) {
    return new RSAPeerIdImpl({ multihash: cid.multihash });
  } else if (multihash.code === identity2.code) {
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
    return new Ed25519PeerIdImpl({ multihash: create(identity2.code, publicKey), privateKey });
  }
  if (publicKey.length === MARSHALLED_SECP256K1_PUBLIC_KEY_LENGTH) {
    return new Secp256k1PeerIdImpl({ multihash: create(identity2.code, publicKey), privateKey });
  }
  return new RSAPeerIdImpl({ multihash: await sha256.digest(publicKey), publicKey, privateKey });
}

// ../../../node_modules/@libp2p/interfaces/dist/src/errors.js
var CodeError2 = class extends Error {
  code;
  props;
  constructor(message2, code5, props) {
    super(message2);
    this.code = code5;
    this.name = props?.name ?? "CodeError";
    this.props = props ?? {};
  }
};

// ../../../node_modules/@multiformats/multiaddr/node_modules/multiformats/src/bases/base58.js
var base58_exports2 = {};
__export(base58_exports2, {
  base58btc: () => base58btc2,
  base58flickr: () => base58flickr2
});

// ../../../node_modules/@multiformats/multiaddr/node_modules/multiformats/vendor/base-x.js
function base3(ALPHABET, name5) {
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
  function encode19(source) {
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
    var length5 = 0;
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
      for (var it1 = size - 1; (carry !== 0 || i2 < length5) && it1 !== -1; it1--, i2++) {
        carry += 256 * b58[it1] >>> 0;
        b58[it1] = carry % BASE >>> 0;
        carry = carry / BASE >>> 0;
      }
      if (carry !== 0) {
        throw new Error("Non-zero carry");
      }
      length5 = i2;
      pbegin++;
    }
    var it2 = size - length5;
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
    var length5 = 0;
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
      for (var it3 = size - 1; (carry !== 0 || i2 < length5) && it3 !== -1; it3--, i2++) {
        carry += BASE * b256[it3] >>> 0;
        b256[it3] = carry % 256 >>> 0;
        carry = carry / 256 >>> 0;
      }
      if (carry !== 0) {
        throw new Error("Non-zero carry");
      }
      length5 = i2;
      psz++;
    }
    if (source[psz] === " ") {
      return;
    }
    var it4 = size - length5;
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
  function decode24(string3) {
    var buffer = decodeUnsafe(string3);
    if (buffer) {
      return buffer;
    }
    throw new Error(`Non-${name5} character`);
  }
  return {
    encode: encode19,
    decodeUnsafe,
    decode: decode24
  };
}
var src2 = base3;
var _brrp__multiformats_scope_baseX2 = src2;
var base_x_default2 = _brrp__multiformats_scope_baseX2;

// ../../../node_modules/@multiformats/multiaddr/node_modules/multiformats/src/bytes.js
var empty2 = new Uint8Array(0);
var equals4 = (aa, bb) => {
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
var fromString2 = (str) => new TextEncoder().encode(str);
var toString2 = (b) => new TextDecoder().decode(b);

// ../../../node_modules/@multiformats/multiaddr/node_modules/multiformats/src/bases/base.js
var Encoder2 = class {
  /**
   * @param {Base} name
   * @param {Prefix} prefix
   * @param {(bytes:Uint8Array) => string} baseEncode
   */
  constructor(name5, prefix, baseEncode) {
    this.name = name5;
    this.prefix = prefix;
    this.baseEncode = baseEncode;
  }
  /**
   * @param {Uint8Array} bytes
   * @returns {API.Multibase<Prefix>}
   */
  encode(bytes2) {
    if (bytes2 instanceof Uint8Array) {
      return `${this.prefix}${this.baseEncode(bytes2)}`;
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
  constructor(name5, prefix, baseDecode) {
    this.name = name5;
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
  constructor(decoders2) {
    this.decoders = decoders2;
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
  constructor(name5, prefix, baseEncode, baseDecode) {
    this.name = name5;
    this.prefix = prefix;
    this.baseEncode = baseEncode;
    this.baseDecode = baseDecode;
    this.encoder = new Encoder2(name5, prefix, baseEncode);
    this.decoder = new Decoder2(name5, prefix, baseDecode);
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
var from3 = ({ name: name5, prefix, encode: encode19, decode: decode24 }) => new Codec2(name5, prefix, encode19, decode24);
var baseX2 = ({ prefix, name: name5, alphabet: alphabet5 }) => {
  const { encode: encode19, decode: decode24 } = base_x_default2(alphabet5, name5);
  return from3({
    prefix,
    name: name5,
    encode: encode19,
    /**
     * @param {string} text
     */
    decode: (text) => coerce2(decode24(text))
  });
};
var decode6 = (string3, alphabet5, bitsPerChar, name5) => {
  const codes3 = {};
  for (let i = 0; i < alphabet5.length; ++i) {
    codes3[alphabet5[i]] = i;
  }
  let end = string3.length;
  while (string3[end - 1] === "=") {
    --end;
  }
  const out = new Uint8Array(end * bitsPerChar / 8 | 0);
  let bits2 = 0;
  let buffer = 0;
  let written = 0;
  for (let i = 0; i < end; ++i) {
    const value = codes3[string3[i]];
    if (value === void 0) {
      throw new SyntaxError(`Non-${name5} character`);
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
var encode5 = (data, alphabet5, bitsPerChar) => {
  const pad = alphabet5[alphabet5.length - 1] === "=";
  const mask = (1 << bitsPerChar) - 1;
  let out = "";
  let bits2 = 0;
  let buffer = 0;
  for (let i = 0; i < data.length; ++i) {
    buffer = buffer << 8 | data[i];
    bits2 += 8;
    while (bits2 > bitsPerChar) {
      bits2 -= bitsPerChar;
      out += alphabet5[mask & buffer >> bits2];
    }
  }
  if (bits2) {
    out += alphabet5[mask & buffer << bitsPerChar - bits2];
  }
  if (pad) {
    while (out.length * bitsPerChar & 7) {
      out += "=";
    }
  }
  return out;
};
var rfc46482 = ({ name: name5, prefix, bitsPerChar, alphabet: alphabet5 }) => {
  return from3({
    prefix,
    name: name5,
    encode(input) {
      return encode5(input, alphabet5, bitsPerChar);
    },
    decode(input) {
      return decode6(input, alphabet5, bitsPerChar, name5);
    }
  });
};

// ../../../node_modules/@multiformats/multiaddr/node_modules/multiformats/src/bases/base58.js
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

// ../../../node_modules/@multiformats/multiaddr/node_modules/multiformats/vendor/varint.js
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

// ../../../node_modules/@multiformats/multiaddr/node_modules/multiformats/src/varint.js
var decode8 = (data, offset = 0) => {
  const code5 = varint_default2.decode(data, offset);
  return [code5, varint_default2.decode.bytes];
};
var encodeTo2 = (int, target, offset = 0) => {
  varint_default2.encode(int, target, offset);
  return target;
};
var encodingLength2 = (int) => {
  return varint_default2.encodingLength(int);
};

// ../../../node_modules/@multiformats/multiaddr/node_modules/multiformats/src/hashes/digest.js
var create2 = (code5, digest5) => {
  const size = digest5.byteLength;
  const sizeOffset = encodingLength2(code5);
  const digestOffset = sizeOffset + encodingLength2(size);
  const bytes2 = new Uint8Array(digestOffset + size);
  encodeTo2(code5, bytes2, 0);
  encodeTo2(size, bytes2, sizeOffset);
  bytes2.set(digest5, digestOffset);
  return new Digest2(code5, size, digest5, bytes2);
};
var decode9 = (multihash) => {
  const bytes2 = coerce2(multihash);
  const [code5, sizeOffset] = decode8(bytes2);
  const [size, digestOffset] = decode8(bytes2.subarray(sizeOffset));
  const digest5 = bytes2.subarray(sizeOffset + digestOffset);
  if (digest5.byteLength !== size) {
    throw new Error("Incorrect length");
  }
  return new Digest2(code5, size, digest5, bytes2);
};
var equals5 = (a, b) => {
  if (a === b) {
    return true;
  } else {
    const data = (
      /** @type {{code?:unknown, size?:unknown, bytes?:unknown}} */
      b
    );
    return a.code === data.code && a.size === data.size && data.bytes instanceof Uint8Array && equals4(a.bytes, data.bytes);
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
  constructor(code5, size, digest5, bytes2) {
    this.code = code5;
    this.size = size;
    this.digest = digest5;
    this.bytes = bytes2;
  }
};

// ../../../node_modules/@multiformats/multiaddr/node_modules/multiformats/src/bases/base32.js
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

// ../../../node_modules/@multiformats/multiaddr/node_modules/multiformats/src/cid.js
var format2 = (link, base7) => {
  const { bytes: bytes2, version: version2 } = link;
  switch (version2) {
    case 0:
      return toStringV02(
        bytes2,
        baseCache2(link),
        /** @type {API.MultibaseEncoder<"z">} */
        base7 || base58btc2.encoder
      );
    default:
      return toStringV12(
        bytes2,
        baseCache2(link),
        /** @type {API.MultibaseEncoder<Prefix>} */
        base7 || base322.encoder
      );
  }
};
var cache2 = /* @__PURE__ */ new WeakMap();
var baseCache2 = (cid) => {
  const baseCache4 = cache2.get(cid);
  if (baseCache4 == null) {
    const baseCache5 = /* @__PURE__ */ new Map();
    cache2.set(cid, baseCache5);
    return baseCache5;
  }
  return baseCache4;
};
var CID2 = class _CID {
  /**
   * @param {Version} version - Version of the CID
   * @param {Format} code - Code of the codec content is encoded in, see https://github.com/multiformats/multicodec/blob/master/table.csv
   * @param {API.MultihashDigest<Alg>} multihash - (Multi)hash of the of the content.
   * @param {Uint8Array} bytes
   *
   */
  constructor(version2, code5, multihash, bytes2) {
    this.code = code5;
    this.version = version2;
    this.multihash = multihash;
    this.bytes = bytes2;
    this["/"] = bytes2;
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
        const { code: code5, multihash } = this;
        if (code5 !== DAG_PB_CODE2) {
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
        const { code: code5, digest: digest5 } = this.multihash;
        const multihash = create2(code5, digest5);
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
    return unknown && self2.code === unknown.code && self2.version === unknown.version && equals5(self2.multihash, unknown.multihash);
  }
  /**
   * @param {API.MultibaseEncoder<string>} [base]
   * @returns {string}
   */
  toString(base7) {
    return format2(this, base7);
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
      const { version: version2, code: code5, multihash, bytes: bytes2 } = value;
      return new _CID(
        version2,
        code5,
        /** @type {API.MultihashDigest<Alg>} */
        multihash,
        bytes2 || encodeCID2(version2, code5, multihash.bytes)
      );
    } else if (value[cidSymbol2] === true) {
      const { version: version2, multihash, code: code5 } = value;
      const digest5 = (
        /** @type {API.MultihashDigest<Alg>} */
        decode9(multihash)
      );
      return _CID.create(version2, code5, digest5);
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
  static create(version2, code5, digest5) {
    if (typeof code5 !== "number") {
      throw new Error("String codecs are no longer supported");
    }
    if (!(digest5.bytes instanceof Uint8Array)) {
      throw new Error("Invalid digest");
    }
    switch (version2) {
      case 0: {
        if (code5 !== DAG_PB_CODE2) {
          throw new Error(
            `Version 0 CID must use dag-pb (code: ${DAG_PB_CODE2}) block encoding`
          );
        } else {
          return new _CID(version2, code5, digest5, digest5.bytes);
        }
      }
      case 1: {
        const bytes2 = encodeCID2(version2, code5, digest5.bytes);
        return new _CID(version2, code5, digest5, bytes2);
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
  static createV0(digest5) {
    return _CID.create(0, DAG_PB_CODE2, digest5);
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
  static createV1(code5, digest5) {
    return _CID.create(1, code5, digest5);
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
  static decode(bytes2) {
    const [cid, remainder] = _CID.decodeFirst(bytes2);
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
  static decodeFirst(bytes2) {
    const specs = _CID.inspectBytes(bytes2);
    const prefixSize = specs.size - specs.multihashSize;
    const multihashBytes = coerce2(
      bytes2.subarray(prefixSize, prefixSize + specs.multihashSize)
    );
    if (multihashBytes.byteLength !== specs.multihashSize) {
      throw new Error("Incorrect length");
    }
    const digestBytes = multihashBytes.subarray(
      specs.multihashSize - specs.digestSize
    );
    const digest5 = new Digest2(
      specs.multihashCode,
      specs.digestSize,
      digestBytes,
      multihashBytes
    );
    const cid = specs.version === 0 ? _CID.createV0(
      /** @type {API.MultihashDigest<API.SHA_256>} */
      digest5
    ) : _CID.createV1(specs.codec, digest5);
    return [
      /** @type {CID<T, C, A, V>} */
      cid,
      bytes2.subarray(specs.size)
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
      const [i, length5] = decode8(initialBytes.subarray(offset));
      offset += length5;
      return i;
    };
    let version2 = (
      /** @type {V} */
      next()
    );
    let codec = (
      /** @type {C} */
      DAG_PB_CODE2
    );
    if (
      /** @type {number} */
      version2 === 18
    ) {
      version2 = /** @type {V} */
      0;
      offset = 0;
    } else {
      codec = /** @type {C} */
      next();
    }
    if (version2 !== 0 && version2 !== 1) {
      throw new RangeError(`Invalid CID version ${version2}`);
    }
    const prefixSize = offset;
    const multihashCode = (
      /** @type {A} */
      next()
    );
    const digestSize = next();
    const size = offset + digestSize;
    const multihashSize = size - prefixSize;
    return { version: version2, codec, multihashCode, digestSize, multihashSize, size };
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
  static parse(source, base7) {
    const [prefix, bytes2] = parseCIDtoBytes2(source, base7);
    const cid = _CID.decode(bytes2);
    if (cid.version === 0 && source[0] !== "Q") {
      throw Error("Version 0 CID string must not include multibase prefix");
    }
    baseCache2(cid).set(prefix, source);
    return cid;
  }
};
var parseCIDtoBytes2 = (source, base7) => {
  switch (source[0]) {
    case "Q": {
      const decoder = base7 || base58btc2;
      return [
        /** @type {Prefix} */
        base58btc2.prefix,
        decoder.decode(`${base58btc2.prefix}${source}`)
      ];
    }
    case base58btc2.prefix: {
      const decoder = base7 || base58btc2;
      return [
        /** @type {Prefix} */
        base58btc2.prefix,
        decoder.decode(source)
      ];
    }
    case base322.prefix: {
      const decoder = base7 || base322;
      return [
        /** @type {Prefix} */
        base322.prefix,
        decoder.decode(source)
      ];
    }
    default: {
      if (base7 == null) {
        throw Error(
          "To parse non base32 or base58btc encoded CID multibase decoder must be provided"
        );
      }
      return [
        /** @type {Prefix} */
        source[0],
        base7.decode(source)
      ];
    }
  }
};
var toStringV02 = (bytes2, cache4, base7) => {
  const { prefix } = base7;
  if (prefix !== base58btc2.prefix) {
    throw Error(`Cannot string encode V0 in ${base7.name} encoding`);
  }
  const cid = cache4.get(prefix);
  if (cid == null) {
    const cid2 = base7.encode(bytes2).slice(1);
    cache4.set(prefix, cid2);
    return cid2;
  } else {
    return cid;
  }
};
var toStringV12 = (bytes2, cache4, base7) => {
  const { prefix } = base7;
  const cid = cache4.get(prefix);
  if (cid == null) {
    const cid2 = base7.encode(bytes2);
    cache4.set(prefix, cid2);
    return cid2;
  } else {
    return cid;
  }
};
var DAG_PB_CODE2 = 112;
var SHA_256_CODE2 = 18;
var encodeCID2 = (version2, code5, multihash) => {
  const codeOffset = encodingLength2(version2);
  const hashOffset = codeOffset + encodingLength2(code5);
  const bytes2 = new Uint8Array(hashOffset + multihash.byteLength);
  encodeTo2(version2, bytes2, 0);
  encodeTo2(code5, bytes2, codeOffset);
  bytes2.set(multihash, hashOffset);
  return bytes2;
};
var cidSymbol2 = Symbol.for("@ipld/js-cid/CID");

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/bases/identity.js
var identity_exports3 = {};
__export(identity_exports3, {
  identity: () => identity3
});

// ../../../node_modules/uint8arrays/node_modules/multiformats/vendor/base-x.js
function base4(ALPHABET, name5) {
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
  function encode19(source) {
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
    var length5 = 0;
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
      for (var it1 = size - 1; (carry !== 0 || i2 < length5) && it1 !== -1; it1--, i2++) {
        carry += 256 * b58[it1] >>> 0;
        b58[it1] = carry % BASE >>> 0;
        carry = carry / BASE >>> 0;
      }
      if (carry !== 0) {
        throw new Error("Non-zero carry");
      }
      length5 = i2;
      pbegin++;
    }
    var it2 = size - length5;
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
    var length5 = 0;
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
      for (var it3 = size - 1; (carry !== 0 || i2 < length5) && it3 !== -1; it3--, i2++) {
        carry += BASE * b256[it3] >>> 0;
        b256[it3] = carry % 256 >>> 0;
        carry = carry / 256 >>> 0;
      }
      if (carry !== 0) {
        throw new Error("Non-zero carry");
      }
      length5 = i2;
      psz++;
    }
    if (source[psz] === " ") {
      return;
    }
    var it4 = size - length5;
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
  function decode24(string3) {
    var buffer = decodeUnsafe(string3);
    if (buffer) {
      return buffer;
    }
    throw new Error(`Non-${name5} character`);
  }
  return {
    encode: encode19,
    decodeUnsafe,
    decode: decode24
  };
}
var src3 = base4;
var _brrp__multiformats_scope_baseX3 = src3;
var base_x_default3 = _brrp__multiformats_scope_baseX3;

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/bytes.js
var empty3 = new Uint8Array(0);
var equals6 = (aa, bb) => {
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
var coerce3 = (o) => {
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
var toString3 = (b) => new TextDecoder().decode(b);

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/bases/base.js
var Encoder3 = class {
  /**
   * @param {Base} name
   * @param {Prefix} prefix
   * @param {(bytes:Uint8Array) => string} baseEncode
   */
  constructor(name5, prefix, baseEncode) {
    this.name = name5;
    this.prefix = prefix;
    this.baseEncode = baseEncode;
  }
  /**
   * @param {Uint8Array} bytes
   * @returns {API.Multibase<Prefix>}
   */
  encode(bytes2) {
    if (bytes2 instanceof Uint8Array) {
      return `${this.prefix}${this.baseEncode(bytes2)}`;
    } else {
      throw Error("Unknown type, must be binary type");
    }
  }
};
var Decoder3 = class {
  /**
   * @param {Base} name
   * @param {Prefix} prefix
   * @param {(text:string) => Uint8Array} baseDecode
   */
  constructor(name5, prefix, baseDecode) {
    this.name = name5;
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
    return or3(this, decoder);
  }
};
var ComposedDecoder3 = class {
  /**
   * @param {Decoders<Prefix>} decoders
   */
  constructor(decoders2) {
    this.decoders = decoders2;
  }
  /**
   * @template {string} OtherPrefix
   * @param {API.UnibaseDecoder<OtherPrefix>|ComposedDecoder<OtherPrefix>} decoder
   * @returns {ComposedDecoder<Prefix|OtherPrefix>}
   */
  or(decoder) {
    return or3(this, decoder);
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
var or3 = (left, right) => new ComposedDecoder3(
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
var Codec3 = class {
  /**
   * @param {Base} name
   * @param {Prefix} prefix
   * @param {(bytes:Uint8Array) => string} baseEncode
   * @param {(text:string) => Uint8Array} baseDecode
   */
  constructor(name5, prefix, baseEncode, baseDecode) {
    this.name = name5;
    this.prefix = prefix;
    this.baseEncode = baseEncode;
    this.baseDecode = baseDecode;
    this.encoder = new Encoder3(name5, prefix, baseEncode);
    this.decoder = new Decoder3(name5, prefix, baseDecode);
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
var from4 = ({ name: name5, prefix, encode: encode19, decode: decode24 }) => new Codec3(name5, prefix, encode19, decode24);
var baseX3 = ({ prefix, name: name5, alphabet: alphabet5 }) => {
  const { encode: encode19, decode: decode24 } = base_x_default3(alphabet5, name5);
  return from4({
    prefix,
    name: name5,
    encode: encode19,
    /**
     * @param {string} text
     */
    decode: (text) => coerce3(decode24(text))
  });
};
var decode10 = (string3, alphabet5, bitsPerChar, name5) => {
  const codes3 = {};
  for (let i = 0; i < alphabet5.length; ++i) {
    codes3[alphabet5[i]] = i;
  }
  let end = string3.length;
  while (string3[end - 1] === "=") {
    --end;
  }
  const out = new Uint8Array(end * bitsPerChar / 8 | 0);
  let bits2 = 0;
  let buffer = 0;
  let written = 0;
  for (let i = 0; i < end; ++i) {
    const value = codes3[string3[i]];
    if (value === void 0) {
      throw new SyntaxError(`Non-${name5} character`);
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
var encode7 = (data, alphabet5, bitsPerChar) => {
  const pad = alphabet5[alphabet5.length - 1] === "=";
  const mask = (1 << bitsPerChar) - 1;
  let out = "";
  let bits2 = 0;
  let buffer = 0;
  for (let i = 0; i < data.length; ++i) {
    buffer = buffer << 8 | data[i];
    bits2 += 8;
    while (bits2 > bitsPerChar) {
      bits2 -= bitsPerChar;
      out += alphabet5[mask & buffer >> bits2];
    }
  }
  if (bits2) {
    out += alphabet5[mask & buffer << bitsPerChar - bits2];
  }
  if (pad) {
    while (out.length * bitsPerChar & 7) {
      out += "=";
    }
  }
  return out;
};
var rfc46483 = ({ name: name5, prefix, bitsPerChar, alphabet: alphabet5 }) => {
  return from4({
    prefix,
    name: name5,
    encode(input) {
      return encode7(input, alphabet5, bitsPerChar);
    },
    decode(input) {
      return decode10(input, alphabet5, bitsPerChar, name5);
    }
  });
};

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/bases/identity.js
var identity3 = from4({
  prefix: "\0",
  name: "identity",
  encode: (buf) => toString3(buf),
  decode: (str) => fromString3(str)
});

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/bases/base2.js
var base2_exports2 = {};
__export(base2_exports2, {
  base2: () => base22
});
var base22 = rfc46483({
  prefix: "0",
  name: "base2",
  alphabet: "01",
  bitsPerChar: 1
});

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/bases/base8.js
var base8_exports2 = {};
__export(base8_exports2, {
  base8: () => base82
});
var base82 = rfc46483({
  prefix: "7",
  name: "base8",
  alphabet: "01234567",
  bitsPerChar: 3
});

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/bases/base10.js
var base10_exports2 = {};
__export(base10_exports2, {
  base10: () => base102
});
var base102 = baseX3({
  prefix: "9",
  name: "base10",
  alphabet: "0123456789"
});

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/bases/base16.js
var base16_exports2 = {};
__export(base16_exports2, {
  base16: () => base162,
  base16upper: () => base16upper2
});
var base162 = rfc46483({
  prefix: "f",
  name: "base16",
  alphabet: "0123456789abcdef",
  bitsPerChar: 4
});
var base16upper2 = rfc46483({
  prefix: "F",
  name: "base16upper",
  alphabet: "0123456789ABCDEF",
  bitsPerChar: 4
});

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/bases/base32.js
var base32_exports3 = {};
__export(base32_exports3, {
  base32: () => base323,
  base32hex: () => base32hex3,
  base32hexpad: () => base32hexpad3,
  base32hexpadupper: () => base32hexpadupper3,
  base32hexupper: () => base32hexupper3,
  base32pad: () => base32pad3,
  base32padupper: () => base32padupper3,
  base32upper: () => base32upper3,
  base32z: () => base32z3
});
var base323 = rfc46483({
  prefix: "b",
  name: "base32",
  alphabet: "abcdefghijklmnopqrstuvwxyz234567",
  bitsPerChar: 5
});
var base32upper3 = rfc46483({
  prefix: "B",
  name: "base32upper",
  alphabet: "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567",
  bitsPerChar: 5
});
var base32pad3 = rfc46483({
  prefix: "c",
  name: "base32pad",
  alphabet: "abcdefghijklmnopqrstuvwxyz234567=",
  bitsPerChar: 5
});
var base32padupper3 = rfc46483({
  prefix: "C",
  name: "base32padupper",
  alphabet: "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567=",
  bitsPerChar: 5
});
var base32hex3 = rfc46483({
  prefix: "v",
  name: "base32hex",
  alphabet: "0123456789abcdefghijklmnopqrstuv",
  bitsPerChar: 5
});
var base32hexupper3 = rfc46483({
  prefix: "V",
  name: "base32hexupper",
  alphabet: "0123456789ABCDEFGHIJKLMNOPQRSTUV",
  bitsPerChar: 5
});
var base32hexpad3 = rfc46483({
  prefix: "t",
  name: "base32hexpad",
  alphabet: "0123456789abcdefghijklmnopqrstuv=",
  bitsPerChar: 5
});
var base32hexpadupper3 = rfc46483({
  prefix: "T",
  name: "base32hexpadupper",
  alphabet: "0123456789ABCDEFGHIJKLMNOPQRSTUV=",
  bitsPerChar: 5
});
var base32z3 = rfc46483({
  prefix: "h",
  name: "base32z",
  alphabet: "ybndrfg8ejkmcpqxot1uwisza345h769",
  bitsPerChar: 5
});

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/bases/base36.js
var base36_exports2 = {};
__export(base36_exports2, {
  base36: () => base362,
  base36upper: () => base36upper2
});
var base362 = baseX3({
  prefix: "k",
  name: "base36",
  alphabet: "0123456789abcdefghijklmnopqrstuvwxyz"
});
var base36upper2 = baseX3({
  prefix: "K",
  name: "base36upper",
  alphabet: "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ"
});

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/bases/base58.js
var base58_exports3 = {};
__export(base58_exports3, {
  base58btc: () => base58btc3,
  base58flickr: () => base58flickr3
});
var base58btc3 = baseX3({
  name: "base58btc",
  prefix: "z",
  alphabet: "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
});
var base58flickr3 = baseX3({
  name: "base58flickr",
  prefix: "Z",
  alphabet: "123456789abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ"
});

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/bases/base64.js
var base64_exports2 = {};
__export(base64_exports2, {
  base64: () => base642,
  base64pad: () => base64pad2,
  base64url: () => base64url2,
  base64urlpad: () => base64urlpad2
});
var base642 = rfc46483({
  prefix: "m",
  name: "base64",
  alphabet: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/",
  bitsPerChar: 6
});
var base64pad2 = rfc46483({
  prefix: "M",
  name: "base64pad",
  alphabet: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=",
  bitsPerChar: 6
});
var base64url2 = rfc46483({
  prefix: "u",
  name: "base64url",
  alphabet: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_",
  bitsPerChar: 6
});
var base64urlpad2 = rfc46483({
  prefix: "U",
  name: "base64urlpad",
  alphabet: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_=",
  bitsPerChar: 6
});

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/bases/base256emoji.js
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
function decode11(str) {
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
var base256emoji2 = from4({
  prefix: "\u{1F680}",
  name: "base256emoji",
  encode: encode8,
  decode: decode11
});

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/hashes/sha2-browser.js
var sha2_browser_exports2 = {};
__export(sha2_browser_exports2, {
  sha256: () => sha2562,
  sha512: () => sha5122
});

// ../../../node_modules/uint8arrays/node_modules/multiformats/vendor/varint.js
var encode_13 = encode9;
var MSB3 = 128;
var REST3 = 127;
var MSBALL3 = ~REST3;
var INT3 = Math.pow(2, 31);
function encode9(num, out, offset) {
  out = out || [];
  offset = offset || 0;
  var oldOffset = offset;
  while (num >= INT3) {
    out[offset++] = num & 255 | MSB3;
    num /= 128;
  }
  while (num & MSBALL3) {
    out[offset++] = num & 255 | MSB3;
    num >>>= 7;
  }
  out[offset] = num | 0;
  encode9.bytes = offset - oldOffset + 1;
  return out;
}
var decode12 = read3;
var MSB$13 = 128;
var REST$13 = 127;
function read3(buf, offset) {
  var res = 0, offset = offset || 0, shift = 0, counter = offset, b, l = buf.length;
  do {
    if (counter >= l) {
      read3.bytes = 0;
      throw new RangeError("Could not decode varint");
    }
    b = buf[counter++];
    res += shift < 28 ? (b & REST$13) << shift : (b & REST$13) * Math.pow(2, shift);
    shift += 7;
  } while (b >= MSB$13);
  read3.bytes = counter - offset;
  return res;
}
var N13 = Math.pow(2, 7);
var N23 = Math.pow(2, 14);
var N33 = Math.pow(2, 21);
var N43 = Math.pow(2, 28);
var N53 = Math.pow(2, 35);
var N63 = Math.pow(2, 42);
var N73 = Math.pow(2, 49);
var N83 = Math.pow(2, 56);
var N93 = Math.pow(2, 63);
var length3 = function(value) {
  return value < N13 ? 1 : value < N23 ? 2 : value < N33 ? 3 : value < N43 ? 4 : value < N53 ? 5 : value < N63 ? 6 : value < N73 ? 7 : value < N83 ? 8 : value < N93 ? 9 : 10;
};
var varint3 = {
  encode: encode_13,
  decode: decode12,
  encodingLength: length3
};
var _brrp_varint3 = varint3;
var varint_default3 = _brrp_varint3;

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/varint.js
var decode13 = (data, offset = 0) => {
  const code5 = varint_default3.decode(data, offset);
  return [code5, varint_default3.decode.bytes];
};
var encodeTo3 = (int, target, offset = 0) => {
  varint_default3.encode(int, target, offset);
  return target;
};
var encodingLength3 = (int) => {
  return varint_default3.encodingLength(int);
};

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/hashes/digest.js
var create3 = (code5, digest5) => {
  const size = digest5.byteLength;
  const sizeOffset = encodingLength3(code5);
  const digestOffset = sizeOffset + encodingLength3(size);
  const bytes2 = new Uint8Array(digestOffset + size);
  encodeTo3(code5, bytes2, 0);
  encodeTo3(size, bytes2, sizeOffset);
  bytes2.set(digest5, digestOffset);
  return new Digest3(code5, size, digest5, bytes2);
};
var decode14 = (multihash) => {
  const bytes2 = coerce3(multihash);
  const [code5, sizeOffset] = decode13(bytes2);
  const [size, digestOffset] = decode13(bytes2.subarray(sizeOffset));
  const digest5 = bytes2.subarray(sizeOffset + digestOffset);
  if (digest5.byteLength !== size) {
    throw new Error("Incorrect length");
  }
  return new Digest3(code5, size, digest5, bytes2);
};
var equals7 = (a, b) => {
  if (a === b) {
    return true;
  } else {
    const data = (
      /** @type {{code?:unknown, size?:unknown, bytes?:unknown}} */
      b
    );
    return a.code === data.code && a.size === data.size && data.bytes instanceof Uint8Array && equals6(a.bytes, data.bytes);
  }
};
var Digest3 = class {
  /**
   * Creates a multihash digest.
   *
   * @param {Code} code
   * @param {Size} size
   * @param {Uint8Array} digest
   * @param {Uint8Array} bytes
   */
  constructor(code5, size, digest5, bytes2) {
    this.code = code5;
    this.size = size;
    this.digest = digest5;
    this.bytes = bytes2;
  }
};

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/hashes/hasher.js
var from5 = ({ name: name5, code: code5, encode: encode19 }) => new Hasher2(name5, code5, encode19);
var Hasher2 = class {
  /**
   *
   * @param {Name} name
   * @param {Code} code
   * @param {(input: Uint8Array) => Await<Uint8Array>} encode
   */
  constructor(name5, code5, encode19) {
    this.name = name5;
    this.code = code5;
    this.encode = encode19;
  }
  /**
   * @param {Uint8Array} input
   * @returns {Await<Digest.Digest<Code, number>>}
   */
  digest(input) {
    if (input instanceof Uint8Array) {
      const result = this.encode(input);
      return result instanceof Uint8Array ? create3(this.code, result) : result.then((digest5) => create3(this.code, digest5));
    } else {
      throw Error("Unknown type, must be binary type");
    }
  }
};

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/hashes/sha2-browser.js
var sha2 = (name5) => (
  /**
   * @param {Uint8Array} data
   */
  async (data) => new Uint8Array(await crypto.subtle.digest(name5, data))
);
var sha2562 = from5({
  name: "sha2-256",
  code: 18,
  encode: sha2("SHA-256")
});
var sha5122 = from5({
  name: "sha2-512",
  code: 19,
  encode: sha2("SHA-512")
});

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/hashes/identity.js
var identity_exports4 = {};
__export(identity_exports4, {
  identity: () => identity4
});
var code2 = 0;
var name2 = "identity";
var encode10 = coerce3;
var digest2 = (input) => create3(code2, encode10(input));
var identity4 = { code: code2, name: name2, encode: encode10, digest: digest2 };

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/codecs/json.js
var textEncoder2 = new TextEncoder();
var textDecoder2 = new TextDecoder();

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/cid.js
var format3 = (link, base7) => {
  const { bytes: bytes2, version: version2 } = link;
  switch (version2) {
    case 0:
      return toStringV03(
        bytes2,
        baseCache3(link),
        /** @type {API.MultibaseEncoder<"z">} */
        base7 || base58btc3.encoder
      );
    default:
      return toStringV13(
        bytes2,
        baseCache3(link),
        /** @type {API.MultibaseEncoder<Prefix>} */
        base7 || base323.encoder
      );
  }
};
var cache3 = /* @__PURE__ */ new WeakMap();
var baseCache3 = (cid) => {
  const baseCache4 = cache3.get(cid);
  if (baseCache4 == null) {
    const baseCache5 = /* @__PURE__ */ new Map();
    cache3.set(cid, baseCache5);
    return baseCache5;
  }
  return baseCache4;
};
var CID3 = class _CID {
  /**
   * @param {Version} version - Version of the CID
   * @param {Format} code - Code of the codec content is encoded in, see https://github.com/multiformats/multicodec/blob/master/table.csv
   * @param {API.MultihashDigest<Alg>} multihash - (Multi)hash of the of the content.
   * @param {Uint8Array} bytes
   *
   */
  constructor(version2, code5, multihash, bytes2) {
    this.code = code5;
    this.version = version2;
    this.multihash = multihash;
    this.bytes = bytes2;
    this["/"] = bytes2;
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
        const { code: code5, multihash } = this;
        if (code5 !== DAG_PB_CODE3) {
          throw new Error("Cannot convert a non dag-pb CID to CIDv0");
        }
        if (multihash.code !== SHA_256_CODE3) {
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
        const { code: code5, digest: digest5 } = this.multihash;
        const multihash = create3(code5, digest5);
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
    return unknown && self2.code === unknown.code && self2.version === unknown.version && equals7(self2.multihash, unknown.multihash);
  }
  /**
   * @param {API.MultibaseEncoder<string>} [base]
   * @returns {string}
   */
  toString(base7) {
    return format3(this, base7);
  }
  toJSON() {
    return { "/": format3(this) };
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
      const { version: version2, code: code5, multihash, bytes: bytes2 } = value;
      return new _CID(
        version2,
        code5,
        /** @type {API.MultihashDigest<Alg>} */
        multihash,
        bytes2 || encodeCID3(version2, code5, multihash.bytes)
      );
    } else if (value[cidSymbol3] === true) {
      const { version: version2, multihash, code: code5 } = value;
      const digest5 = (
        /** @type {API.MultihashDigest<Alg>} */
        decode14(multihash)
      );
      return _CID.create(version2, code5, digest5);
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
  static create(version2, code5, digest5) {
    if (typeof code5 !== "number") {
      throw new Error("String codecs are no longer supported");
    }
    if (!(digest5.bytes instanceof Uint8Array)) {
      throw new Error("Invalid digest");
    }
    switch (version2) {
      case 0: {
        if (code5 !== DAG_PB_CODE3) {
          throw new Error(
            `Version 0 CID must use dag-pb (code: ${DAG_PB_CODE3}) block encoding`
          );
        } else {
          return new _CID(version2, code5, digest5, digest5.bytes);
        }
      }
      case 1: {
        const bytes2 = encodeCID3(version2, code5, digest5.bytes);
        return new _CID(version2, code5, digest5, bytes2);
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
  static createV0(digest5) {
    return _CID.create(0, DAG_PB_CODE3, digest5);
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
  static createV1(code5, digest5) {
    return _CID.create(1, code5, digest5);
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
  static decode(bytes2) {
    const [cid, remainder] = _CID.decodeFirst(bytes2);
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
  static decodeFirst(bytes2) {
    const specs = _CID.inspectBytes(bytes2);
    const prefixSize = specs.size - specs.multihashSize;
    const multihashBytes = coerce3(
      bytes2.subarray(prefixSize, prefixSize + specs.multihashSize)
    );
    if (multihashBytes.byteLength !== specs.multihashSize) {
      throw new Error("Incorrect length");
    }
    const digestBytes = multihashBytes.subarray(
      specs.multihashSize - specs.digestSize
    );
    const digest5 = new Digest3(
      specs.multihashCode,
      specs.digestSize,
      digestBytes,
      multihashBytes
    );
    const cid = specs.version === 0 ? _CID.createV0(
      /** @type {API.MultihashDigest<API.SHA_256>} */
      digest5
    ) : _CID.createV1(specs.codec, digest5);
    return [
      /** @type {CID<T, C, A, V>} */
      cid,
      bytes2.subarray(specs.size)
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
      const [i, length5] = decode13(initialBytes.subarray(offset));
      offset += length5;
      return i;
    };
    let version2 = (
      /** @type {V} */
      next()
    );
    let codec = (
      /** @type {C} */
      DAG_PB_CODE3
    );
    if (
      /** @type {number} */
      version2 === 18
    ) {
      version2 = /** @type {V} */
      0;
      offset = 0;
    } else {
      codec = /** @type {C} */
      next();
    }
    if (version2 !== 0 && version2 !== 1) {
      throw new RangeError(`Invalid CID version ${version2}`);
    }
    const prefixSize = offset;
    const multihashCode = (
      /** @type {A} */
      next()
    );
    const digestSize = next();
    const size = offset + digestSize;
    const multihashSize = size - prefixSize;
    return { version: version2, codec, multihashCode, digestSize, multihashSize, size };
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
  static parse(source, base7) {
    const [prefix, bytes2] = parseCIDtoBytes3(source, base7);
    const cid = _CID.decode(bytes2);
    if (cid.version === 0 && source[0] !== "Q") {
      throw Error("Version 0 CID string must not include multibase prefix");
    }
    baseCache3(cid).set(prefix, source);
    return cid;
  }
};
var parseCIDtoBytes3 = (source, base7) => {
  switch (source[0]) {
    case "Q": {
      const decoder = base7 || base58btc3;
      return [
        /** @type {Prefix} */
        base58btc3.prefix,
        decoder.decode(`${base58btc3.prefix}${source}`)
      ];
    }
    case base58btc3.prefix: {
      const decoder = base7 || base58btc3;
      return [
        /** @type {Prefix} */
        base58btc3.prefix,
        decoder.decode(source)
      ];
    }
    case base323.prefix: {
      const decoder = base7 || base323;
      return [
        /** @type {Prefix} */
        base323.prefix,
        decoder.decode(source)
      ];
    }
    default: {
      if (base7 == null) {
        throw Error(
          "To parse non base32 or base58btc encoded CID multibase decoder must be provided"
        );
      }
      return [
        /** @type {Prefix} */
        source[0],
        base7.decode(source)
      ];
    }
  }
};
var toStringV03 = (bytes2, cache4, base7) => {
  const { prefix } = base7;
  if (prefix !== base58btc3.prefix) {
    throw Error(`Cannot string encode V0 in ${base7.name} encoding`);
  }
  const cid = cache4.get(prefix);
  if (cid == null) {
    const cid2 = base7.encode(bytes2).slice(1);
    cache4.set(prefix, cid2);
    return cid2;
  } else {
    return cid;
  }
};
var toStringV13 = (bytes2, cache4, base7) => {
  const { prefix } = base7;
  const cid = cache4.get(prefix);
  if (cid == null) {
    const cid2 = base7.encode(bytes2);
    cache4.set(prefix, cid2);
    return cid2;
  } else {
    return cid;
  }
};
var DAG_PB_CODE3 = 112;
var SHA_256_CODE3 = 18;
var encodeCID3 = (version2, code5, multihash) => {
  const codeOffset = encodingLength3(version2);
  const hashOffset = codeOffset + encodingLength3(code5);
  const bytes2 = new Uint8Array(hashOffset + multihash.byteLength);
  encodeTo3(version2, bytes2, 0);
  encodeTo3(code5, bytes2, codeOffset);
  bytes2.set(multihash, hashOffset);
  return bytes2;
};
var cidSymbol3 = Symbol.for("@ipld/js-cid/CID");

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/basics.js
var bases2 = { ...identity_exports3, ...base2_exports2, ...base8_exports2, ...base10_exports2, ...base16_exports2, ...base32_exports3, ...base36_exports2, ...base58_exports3, ...base64_exports2, ...base256emoji_exports2 };
var hashes2 = { ...sha2_browser_exports2, ...identity_exports4 };

// ../../../node_modules/uint8arrays/dist/src/util/as-uint8array.js
function asUint8Array(buf) {
  if (globalThis.Buffer != null) {
    return new Uint8Array(buf.buffer, buf.byteOffset, buf.byteLength);
  }
  return buf;
}

// ../../../node_modules/uint8arrays/dist/src/alloc.js
function alloc(size = 0) {
  if (globalThis.Buffer?.alloc != null) {
    return asUint8Array(globalThis.Buffer.alloc(size));
  }
  return new Uint8Array(size);
}
function allocUnsafe(size = 0) {
  if (globalThis.Buffer?.allocUnsafe != null) {
    return asUint8Array(globalThis.Buffer.allocUnsafe(size));
  }
  return new Uint8Array(size);
}

// ../../../node_modules/uint8arrays/dist/src/util/bases.js
function createCodec(name5, prefix, encode19, decode24) {
  return {
    name: name5,
    prefix,
    encoder: {
      name: name5,
      prefix,
      encode: encode19
    },
    decoder: {
      decode: decode24
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
  let string3 = "a";
  for (let i = 0; i < buf.length; i++) {
    string3 += String.fromCharCode(buf[i]);
  }
  return string3;
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
  hex: bases2.base16,
  latin1: ascii,
  ascii,
  binary: ascii,
  ...bases2
};
var bases_default = BASES;

// ../../../node_modules/uint8arrays/dist/src/to-string.js
function toString4(array, encoding = "utf8") {
  const base7 = bases_default[encoding];
  if (base7 == null) {
    throw new Error(`Unsupported encoding "${encoding}"`);
  }
  if ((encoding === "utf8" || encoding === "utf-8") && globalThis.Buffer != null && globalThis.Buffer.from != null) {
    return globalThis.Buffer.from(array.buffer, array.byteOffset, array.byteLength).toString("utf8");
  }
  return base7.encoder.encode(array).substring(1);
}

// ../../../node_modules/@multiformats/multiaddr/dist/src/index.js
var import_varint6 = __toESM(require_varint(), 1);

// ../../../node_modules/uint8arrays/dist/src/concat.js
function concat(arrays, length5) {
  if (length5 == null) {
    length5 = arrays.reduce((acc, curr) => acc + curr.length, 0);
  }
  const output2 = allocUnsafe(length5);
  let offset = 0;
  for (const arr of arrays) {
    output2.set(arr, offset);
    offset += arr.length;
  }
  return asUint8Array(output2);
}

// ../../../node_modules/@multiformats/multiaddr/dist/src/codec.js
var import_varint5 = __toESM(require_varint(), 1);

// ../../../node_modules/@chainsafe/is-ip/lib/parser.js
var Parser = class {
  index = 0;
  input = "";
  new(input) {
    this.index = 0;
    this.input = input;
    return this;
  }
  /** Run a parser, and restore the pre-parse state if it fails. */
  readAtomically(fn) {
    const index = this.index;
    const result = fn();
    if (result === void 0) {
      this.index = index;
    }
    return result;
  }
  /** Run a parser, but fail if the entire input wasn't consumed. Doesn't run atomically. */
  parseWith(fn) {
    const result = fn();
    if (this.index !== this.input.length) {
      return void 0;
    }
    return result;
  }
  /** Peek the next character from the input */
  peekChar() {
    if (this.index >= this.input.length) {
      return void 0;
    }
    return this.input[this.index];
  }
  /** Read the next character from the input */
  readChar() {
    if (this.index >= this.input.length) {
      return void 0;
    }
    return this.input[this.index++];
  }
  /** Read the next character from the input if it matches the target. */
  readGivenChar(target) {
    return this.readAtomically(() => {
      const char = this.readChar();
      if (char !== target) {
        return void 0;
      }
      return char;
    });
  }
  /**
   * Helper for reading separators in an indexed loop. Reads the separator
   * character iff index > 0, then runs the parser. When used in a loop,
   * the separator character will only be read on index > 0 (see
   * readIPv4Addr for an example)
   */
  readSeparator(sep, index, inner) {
    return this.readAtomically(() => {
      if (index > 0) {
        if (this.readGivenChar(sep) === void 0) {
          return void 0;
        }
      }
      return inner();
    });
  }
  /**
   * Read a number off the front of the input in the given radix, stopping
   * at the first non-digit character or eof. Fails if the number has more
   * digits than max_digits or if there is no number.
   */
  readNumber(radix, maxDigits, allowZeroPrefix, maxBytes) {
    return this.readAtomically(() => {
      let result = 0;
      let digitCount = 0;
      const leadingChar = this.peekChar();
      if (leadingChar === void 0) {
        return void 0;
      }
      const hasLeadingZero = leadingChar === "0";
      const maxValue = 2 ** (8 * maxBytes) - 1;
      while (true) {
        const digit = this.readAtomically(() => {
          const char = this.readChar();
          if (char === void 0) {
            return void 0;
          }
          const num = Number.parseInt(char, radix);
          if (Number.isNaN(num)) {
            return void 0;
          }
          return num;
        });
        if (digit === void 0) {
          break;
        }
        result *= radix;
        result += digit;
        if (result > maxValue) {
          return void 0;
        }
        digitCount += 1;
        if (maxDigits !== void 0) {
          if (digitCount > maxDigits) {
            return void 0;
          }
        }
      }
      if (digitCount === 0) {
        return void 0;
      } else if (!allowZeroPrefix && hasLeadingZero && digitCount > 1) {
        return void 0;
      } else {
        return result;
      }
    });
  }
  /** Read an IPv4 address. */
  readIPv4Addr() {
    return this.readAtomically(() => {
      const out = new Uint8Array(4);
      for (let i = 0; i < out.length; i++) {
        const ix = this.readSeparator(".", i, () => this.readNumber(10, 3, false, 1));
        if (ix === void 0) {
          return void 0;
        }
        out[i] = ix;
      }
      return out;
    });
  }
  /** Read an IPv6 Address. */
  readIPv6Addr() {
    const readGroups = (groups) => {
      for (let i = 0; i < groups.length / 2; i++) {
        const ix = i * 2;
        if (i < groups.length - 3) {
          const ipv4 = this.readSeparator(":", i, () => this.readIPv4Addr());
          if (ipv4 !== void 0) {
            groups[ix] = ipv4[0];
            groups[ix + 1] = ipv4[1];
            groups[ix + 2] = ipv4[2];
            groups[ix + 3] = ipv4[3];
            return [ix + 4, true];
          }
        }
        const group = this.readSeparator(":", i, () => this.readNumber(16, 4, true, 2));
        if (group === void 0) {
          return [ix, false];
        }
        groups[ix] = group >> 8;
        groups[ix + 1] = group & 255;
      }
      return [groups.length, false];
    };
    return this.readAtomically(() => {
      const head = new Uint8Array(16);
      const [headSize, headIp4] = readGroups(head);
      if (headSize === 16) {
        return head;
      }
      if (headIp4) {
        return void 0;
      }
      if (this.readGivenChar(":") === void 0) {
        return void 0;
      }
      if (this.readGivenChar(":") === void 0) {
        return void 0;
      }
      const tail = new Uint8Array(14);
      const limit = 16 - (headSize + 2);
      const [tailSize] = readGroups(tail.subarray(0, limit));
      head.set(tail.subarray(0, tailSize), 16 - tailSize);
      return head;
    });
  }
  /** Read an IP Address, either IPv4 or IPv6. */
  readIPAddr() {
    return this.readIPv4Addr() ?? this.readIPv6Addr();
  }
};

// ../../../node_modules/@chainsafe/is-ip/lib/parse.js
var MAX_IPV6_LENGTH = 45;
var MAX_IPV4_LENGTH = 15;
var parser = new Parser();
function parseIPv4(input) {
  if (input.length > MAX_IPV4_LENGTH) {
    return void 0;
  }
  return parser.new(input).parseWith(() => parser.readIPv4Addr());
}
function parseIPv6(input) {
  if (input.length > MAX_IPV6_LENGTH) {
    return void 0;
  }
  return parser.new(input).parseWith(() => parser.readIPv6Addr());
}
function parseIP(input) {
  if (input.length > MAX_IPV6_LENGTH) {
    return void 0;
  }
  return parser.new(input).parseWith(() => parser.readIPAddr());
}

// ../../../node_modules/@chainsafe/netmask/dist/src/ip.js
var maxIPv6Octet = parseInt("0xFFFF", 16);
var ipv4Prefix = new Uint8Array([
  0,
  0,
  0,
  0,
  0,
  0,
  0,
  0,
  0,
  0,
  255,
  255
]);

// ../../../node_modules/@multiformats/multiaddr/node_modules/multiformats/src/bases/identity.js
var identity_exports5 = {};
__export(identity_exports5, {
  identity: () => identity5
});
var identity5 = from3({
  prefix: "\0",
  name: "identity",
  encode: (buf) => toString2(buf),
  decode: (str) => fromString2(str)
});

// ../../../node_modules/@multiformats/multiaddr/node_modules/multiformats/src/bases/base2.js
var base2_exports3 = {};
__export(base2_exports3, {
  base2: () => base23
});
var base23 = rfc46482({
  prefix: "0",
  name: "base2",
  alphabet: "01",
  bitsPerChar: 1
});

// ../../../node_modules/@multiformats/multiaddr/node_modules/multiformats/src/bases/base8.js
var base8_exports3 = {};
__export(base8_exports3, {
  base8: () => base83
});
var base83 = rfc46482({
  prefix: "7",
  name: "base8",
  alphabet: "01234567",
  bitsPerChar: 3
});

// ../../../node_modules/@multiformats/multiaddr/node_modules/multiformats/src/bases/base10.js
var base10_exports3 = {};
__export(base10_exports3, {
  base10: () => base103
});
var base103 = baseX2({
  prefix: "9",
  name: "base10",
  alphabet: "0123456789"
});

// ../../../node_modules/@multiformats/multiaddr/node_modules/multiformats/src/bases/base16.js
var base16_exports3 = {};
__export(base16_exports3, {
  base16: () => base163,
  base16upper: () => base16upper3
});
var base163 = rfc46482({
  prefix: "f",
  name: "base16",
  alphabet: "0123456789abcdef",
  bitsPerChar: 4
});
var base16upper3 = rfc46482({
  prefix: "F",
  name: "base16upper",
  alphabet: "0123456789ABCDEF",
  bitsPerChar: 4
});

// ../../../node_modules/@multiformats/multiaddr/node_modules/multiformats/src/bases/base36.js
var base36_exports3 = {};
__export(base36_exports3, {
  base36: () => base363,
  base36upper: () => base36upper3
});
var base363 = baseX2({
  prefix: "k",
  name: "base36",
  alphabet: "0123456789abcdefghijklmnopqrstuvwxyz"
});
var base36upper3 = baseX2({
  prefix: "K",
  name: "base36upper",
  alphabet: "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ"
});

// ../../../node_modules/@multiformats/multiaddr/node_modules/multiformats/src/bases/base64.js
var base64_exports3 = {};
__export(base64_exports3, {
  base64: () => base643,
  base64pad: () => base64pad3,
  base64url: () => base64url3,
  base64urlpad: () => base64urlpad3
});
var base643 = rfc46482({
  prefix: "m",
  name: "base64",
  alphabet: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/",
  bitsPerChar: 6
});
var base64pad3 = rfc46482({
  prefix: "M",
  name: "base64pad",
  alphabet: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=",
  bitsPerChar: 6
});
var base64url3 = rfc46482({
  prefix: "u",
  name: "base64url",
  alphabet: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_",
  bitsPerChar: 6
});
var base64urlpad3 = rfc46482({
  prefix: "U",
  name: "base64urlpad",
  alphabet: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_=",
  bitsPerChar: 6
});

// ../../../node_modules/@multiformats/multiaddr/node_modules/multiformats/src/bases/base256emoji.js
var base256emoji_exports3 = {};
__export(base256emoji_exports3, {
  base256emoji: () => base256emoji3
});
var alphabet3 = Array.from("\u{1F680}\u{1FA90}\u2604\u{1F6F0}\u{1F30C}\u{1F311}\u{1F312}\u{1F313}\u{1F314}\u{1F315}\u{1F316}\u{1F317}\u{1F318}\u{1F30D}\u{1F30F}\u{1F30E}\u{1F409}\u2600\u{1F4BB}\u{1F5A5}\u{1F4BE}\u{1F4BF}\u{1F602}\u2764\u{1F60D}\u{1F923}\u{1F60A}\u{1F64F}\u{1F495}\u{1F62D}\u{1F618}\u{1F44D}\u{1F605}\u{1F44F}\u{1F601}\u{1F525}\u{1F970}\u{1F494}\u{1F496}\u{1F499}\u{1F622}\u{1F914}\u{1F606}\u{1F644}\u{1F4AA}\u{1F609}\u263A\u{1F44C}\u{1F917}\u{1F49C}\u{1F614}\u{1F60E}\u{1F607}\u{1F339}\u{1F926}\u{1F389}\u{1F49E}\u270C\u2728\u{1F937}\u{1F631}\u{1F60C}\u{1F338}\u{1F64C}\u{1F60B}\u{1F497}\u{1F49A}\u{1F60F}\u{1F49B}\u{1F642}\u{1F493}\u{1F929}\u{1F604}\u{1F600}\u{1F5A4}\u{1F603}\u{1F4AF}\u{1F648}\u{1F447}\u{1F3B6}\u{1F612}\u{1F92D}\u2763\u{1F61C}\u{1F48B}\u{1F440}\u{1F62A}\u{1F611}\u{1F4A5}\u{1F64B}\u{1F61E}\u{1F629}\u{1F621}\u{1F92A}\u{1F44A}\u{1F973}\u{1F625}\u{1F924}\u{1F449}\u{1F483}\u{1F633}\u270B\u{1F61A}\u{1F61D}\u{1F634}\u{1F31F}\u{1F62C}\u{1F643}\u{1F340}\u{1F337}\u{1F63B}\u{1F613}\u2B50\u2705\u{1F97A}\u{1F308}\u{1F608}\u{1F918}\u{1F4A6}\u2714\u{1F623}\u{1F3C3}\u{1F490}\u2639\u{1F38A}\u{1F498}\u{1F620}\u261D\u{1F615}\u{1F33A}\u{1F382}\u{1F33B}\u{1F610}\u{1F595}\u{1F49D}\u{1F64A}\u{1F639}\u{1F5E3}\u{1F4AB}\u{1F480}\u{1F451}\u{1F3B5}\u{1F91E}\u{1F61B}\u{1F534}\u{1F624}\u{1F33C}\u{1F62B}\u26BD\u{1F919}\u2615\u{1F3C6}\u{1F92B}\u{1F448}\u{1F62E}\u{1F646}\u{1F37B}\u{1F343}\u{1F436}\u{1F481}\u{1F632}\u{1F33F}\u{1F9E1}\u{1F381}\u26A1\u{1F31E}\u{1F388}\u274C\u270A\u{1F44B}\u{1F630}\u{1F928}\u{1F636}\u{1F91D}\u{1F6B6}\u{1F4B0}\u{1F353}\u{1F4A2}\u{1F91F}\u{1F641}\u{1F6A8}\u{1F4A8}\u{1F92C}\u2708\u{1F380}\u{1F37A}\u{1F913}\u{1F619}\u{1F49F}\u{1F331}\u{1F616}\u{1F476}\u{1F974}\u25B6\u27A1\u2753\u{1F48E}\u{1F4B8}\u2B07\u{1F628}\u{1F31A}\u{1F98B}\u{1F637}\u{1F57A}\u26A0\u{1F645}\u{1F61F}\u{1F635}\u{1F44E}\u{1F932}\u{1F920}\u{1F927}\u{1F4CC}\u{1F535}\u{1F485}\u{1F9D0}\u{1F43E}\u{1F352}\u{1F617}\u{1F911}\u{1F30A}\u{1F92F}\u{1F437}\u260E\u{1F4A7}\u{1F62F}\u{1F486}\u{1F446}\u{1F3A4}\u{1F647}\u{1F351}\u2744\u{1F334}\u{1F4A3}\u{1F438}\u{1F48C}\u{1F4CD}\u{1F940}\u{1F922}\u{1F445}\u{1F4A1}\u{1F4A9}\u{1F450}\u{1F4F8}\u{1F47B}\u{1F910}\u{1F92E}\u{1F3BC}\u{1F975}\u{1F6A9}\u{1F34E}\u{1F34A}\u{1F47C}\u{1F48D}\u{1F4E3}\u{1F942}");
var alphabetBytesToChars3 = (
  /** @type {string[]} */
  alphabet3.reduce(
    (p, c, i) => {
      p[i] = c;
      return p;
    },
    /** @type {string[]} */
    []
  )
);
var alphabetCharsToBytes3 = (
  /** @type {number[]} */
  alphabet3.reduce(
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
function encode11(data) {
  return data.reduce((p, c) => {
    p += alphabetBytesToChars3[c];
    return p;
  }, "");
}
function decode15(str) {
  const byts = [];
  for (const char of str) {
    const byt = alphabetCharsToBytes3[
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
var base256emoji3 = from3({
  prefix: "\u{1F680}",
  name: "base256emoji",
  encode: encode11,
  decode: decode15
});

// ../../../node_modules/@multiformats/multiaddr/node_modules/multiformats/src/hashes/sha2-browser.js
var sha2_browser_exports3 = {};
__export(sha2_browser_exports3, {
  sha256: () => sha2563,
  sha512: () => sha5123
});

// ../../../node_modules/@multiformats/multiaddr/node_modules/multiformats/src/hashes/hasher.js
var from6 = ({ name: name5, code: code5, encode: encode19 }) => new Hasher3(name5, code5, encode19);
var Hasher3 = class {
  /**
   *
   * @param {Name} name
   * @param {Code} code
   * @param {(input: Uint8Array) => Await<Uint8Array>} encode
   */
  constructor(name5, code5, encode19) {
    this.name = name5;
    this.code = code5;
    this.encode = encode19;
  }
  /**
   * @param {Uint8Array} input
   * @returns {Await<Digest.Digest<Code, number>>}
   */
  digest(input) {
    if (input instanceof Uint8Array) {
      const result = this.encode(input);
      return result instanceof Uint8Array ? create2(this.code, result) : result.then((digest5) => create2(this.code, digest5));
    } else {
      throw Error("Unknown type, must be binary type");
    }
  }
};

// ../../../node_modules/@multiformats/multiaddr/node_modules/multiformats/src/hashes/sha2-browser.js
var sha3 = (name5) => (
  /**
   * @param {Uint8Array} data
   */
  async (data) => new Uint8Array(await crypto.subtle.digest(name5, data))
);
var sha2563 = from6({
  name: "sha2-256",
  code: 18,
  encode: sha3("SHA-256")
});
var sha5123 = from6({
  name: "sha2-512",
  code: 19,
  encode: sha3("SHA-512")
});

// ../../../node_modules/@multiformats/multiaddr/node_modules/multiformats/src/hashes/identity.js
var identity_exports6 = {};
__export(identity_exports6, {
  identity: () => identity6
});
var code3 = 0;
var name3 = "identity";
var encode12 = coerce2;
var digest3 = (input) => create2(code3, encode12(input));
var identity6 = { code: code3, name: name3, encode: encode12, digest: digest3 };

// ../../../node_modules/@multiformats/multiaddr/node_modules/multiformats/src/codecs/json.js
var textEncoder3 = new TextEncoder();
var textDecoder3 = new TextDecoder();

// ../../../node_modules/@multiformats/multiaddr/node_modules/multiformats/src/basics.js
var bases3 = { ...identity_exports5, ...base2_exports3, ...base8_exports3, ...base10_exports3, ...base16_exports3, ...base32_exports2, ...base36_exports3, ...base58_exports2, ...base64_exports3, ...base256emoji_exports3 };
var hashes3 = { ...sha2_browser_exports3, ...identity_exports6 };

// ../../../node_modules/uint8arrays/dist/src/from-string.js
function fromString4(string3, encoding = "utf8") {
  const base7 = bases_default[encoding];
  if (base7 == null) {
    throw new Error(`Unsupported encoding "${encoding}"`);
  }
  if ((encoding === "utf8" || encoding === "utf-8") && globalThis.Buffer != null && globalThis.Buffer.from != null) {
    return asUint8Array(globalThis.Buffer.from(string3, "utf-8"));
  }
  return base7.decoder.decode(`${base7.prefix}${string3}`);
}

// ../../../node_modules/@multiformats/multiaddr/dist/src/convert.js
var import_varint4 = __toESM(require_varint(), 1);

// ../../../node_modules/@chainsafe/is-ip/lib/is-ip.js
function isIPv4(input) {
  return Boolean(parseIPv4(input));
}
function isIPv6(input) {
  return Boolean(parseIPv6(input));
}
function isIP(input) {
  return Boolean(parseIP(input));
}

// ../../../node_modules/@multiformats/multiaddr/dist/src/ip.js
var isV4 = isIPv4;
var isV6 = isIPv6;
var toBytes = function(ip) {
  let offset = 0;
  ip = ip.toString().trim();
  if (isV4(ip)) {
    const bytes2 = new Uint8Array(offset + 4);
    ip.split(/\./g).forEach((byte) => {
      bytes2[offset++] = parseInt(byte, 10) & 255;
    });
    return bytes2;
  }
  if (isV6(ip)) {
    const sections = ip.split(":", 8);
    let i;
    for (i = 0; i < sections.length; i++) {
      const isv4 = isV4(sections[i]);
      let v4Buffer;
      if (isv4) {
        v4Buffer = toBytes(sections[i]);
        sections[i] = toString4(v4Buffer.slice(0, 2), "base16");
      }
      if (v4Buffer != null && ++i < 8) {
        sections.splice(i, 0, toString4(v4Buffer.slice(2, 4), "base16"));
      }
    }
    if (sections[0] === "") {
      while (sections.length < 8)
        sections.unshift("0");
    } else if (sections[sections.length - 1] === "") {
      while (sections.length < 8)
        sections.push("0");
    } else if (sections.length < 8) {
      for (i = 0; i < sections.length && sections[i] !== ""; i++)
        ;
      const argv = [i, 1];
      for (i = 9 - sections.length; i > 0; i--) {
        argv.push("0");
      }
      sections.splice.apply(sections, argv);
    }
    const bytes2 = new Uint8Array(offset + 16);
    for (i = 0; i < sections.length; i++) {
      const word = parseInt(sections[i], 16);
      bytes2[offset++] = word >> 8 & 255;
      bytes2[offset++] = word & 255;
    }
    return bytes2;
  }
  throw new Error("invalid ip address");
};
var toString5 = function(buf, offset = 0, length5) {
  offset = ~~offset;
  length5 = length5 ?? buf.length - offset;
  const view = new DataView(buf.buffer);
  if (length5 === 4) {
    const result = [];
    for (let i = 0; i < length5; i++) {
      result.push(buf[offset + i]);
    }
    return result.join(".");
  }
  if (length5 === 16) {
    const result = [];
    for (let i = 0; i < length5; i += 2) {
      result.push(view.getUint16(offset + i).toString(16));
    }
    return result.join(":").replace(/(^|:)0(:0)*:0(:|$)/, "$1::$3").replace(/:{3,4}/, "::");
  }
  return "";
};

// ../../../node_modules/@multiformats/multiaddr/dist/src/protocols-table.js
var V = -1;
var names = {};
var codes = {};
var table = [
  [4, 32, "ip4"],
  [6, 16, "tcp"],
  [33, 16, "dccp"],
  [41, 128, "ip6"],
  [42, V, "ip6zone"],
  [43, 8, "ipcidr"],
  [53, V, "dns", true],
  [54, V, "dns4", true],
  [55, V, "dns6", true],
  [56, V, "dnsaddr", true],
  [132, 16, "sctp"],
  [273, 16, "udp"],
  [275, 0, "p2p-webrtc-star"],
  [276, 0, "p2p-webrtc-direct"],
  [277, 0, "p2p-stardust"],
  [280, 0, "webrtc-direct"],
  [281, 0, "webrtc"],
  [290, 0, "p2p-circuit"],
  [301, 0, "udt"],
  [302, 0, "utp"],
  [400, V, "unix", false, true],
  // `ipfs` is added before `p2p` for legacy support.
  // All text representations will default to `p2p`, but `ipfs` will
  // still be supported
  [421, V, "ipfs"],
  // `p2p` is the preferred name for 421, and is now the default
  [421, V, "p2p"],
  [443, 0, "https"],
  [444, 96, "onion"],
  [445, 296, "onion3"],
  [446, V, "garlic64"],
  [448, 0, "tls"],
  [449, V, "sni"],
  [460, 0, "quic"],
  [461, 0, "quic-v1"],
  [465, 0, "webtransport"],
  [466, V, "certhash"],
  [477, 0, "ws"],
  [478, 0, "wss"],
  [479, 0, "p2p-websocket-star"],
  [480, 0, "http"],
  [777, V, "memory"]
];
table.forEach((row) => {
  const proto = createProtocol(...row);
  codes[proto.code] = proto;
  names[proto.name] = proto;
});
function createProtocol(code5, size, name5, resolvable, path) {
  return {
    code: code5,
    size,
    name: name5,
    resolvable: Boolean(resolvable),
    path: Boolean(path)
  };
}
function getProtocol(proto) {
  if (typeof proto === "number") {
    if (codes[proto] != null) {
      return codes[proto];
    }
    throw new Error(`no protocol with code: ${proto}`);
  } else if (typeof proto === "string") {
    if (names[proto] != null) {
      return names[proto];
    }
    throw new Error(`no protocol with name: ${proto}`);
  }
  throw new Error(`invalid protocol id type: ${typeof proto}`);
}

// ../../../node_modules/@multiformats/multiaddr/dist/src/convert.js
var ip4Protocol = getProtocol("ip4");
var ip6Protocol = getProtocol("ip6");
var ipcidrProtocol = getProtocol("ipcidr");
function convertToString(proto, buf) {
  const protocol = getProtocol(proto);
  switch (protocol.code) {
    case 4:
    case 41:
      return bytes2ip(buf);
    case 42:
      return bytes2str(buf);
    case 6:
    case 273:
    case 33:
    case 132:
      return bytes2port(buf).toString();
    case 53:
    case 54:
    case 55:
    case 56:
    case 400:
    case 449:
    case 777:
      return bytes2str(buf);
    case 421:
      return bytes2mh(buf);
    case 444:
      return bytes2onion(buf);
    case 445:
      return bytes2onion(buf);
    case 466:
      return bytes2mb(buf);
    default:
      return toString4(buf, "base16");
  }
}
function convertToBytes(proto, str) {
  const protocol = getProtocol(proto);
  switch (protocol.code) {
    case 4:
      return ip2bytes(str);
    case 41:
      return ip2bytes(str);
    case 42:
      return str2bytes(str);
    case 6:
    case 273:
    case 33:
    case 132:
      return port2bytes(parseInt(str, 10));
    case 53:
    case 54:
    case 55:
    case 56:
    case 400:
    case 449:
    case 777:
      return str2bytes(str);
    case 421:
      return mh2bytes(str);
    case 444:
      return onion2bytes(str);
    case 445:
      return onion32bytes(str);
    case 466:
      return mb2bytes(str);
    default:
      return fromString4(str, "base16");
  }
}
var decoders = Object.values(bases3).map((c) => c.decoder);
var anybaseDecoder = function() {
  let acc = decoders[0].or(decoders[1]);
  decoders.slice(2).forEach((d) => acc = acc.or(d));
  return acc;
}();
function ip2bytes(ipString) {
  if (!isIP(ipString)) {
    throw new Error("invalid ip address");
  }
  return toBytes(ipString);
}
function bytes2ip(ipBuff) {
  const ipString = toString5(ipBuff, 0, ipBuff.length);
  if (ipString == null) {
    throw new Error("ipBuff is required");
  }
  if (!isIP(ipString)) {
    throw new Error("invalid ip address");
  }
  return ipString;
}
function port2bytes(port) {
  const buf = new ArrayBuffer(2);
  const view = new DataView(buf);
  view.setUint16(0, port);
  return new Uint8Array(buf);
}
function bytes2port(buf) {
  const view = new DataView(buf.buffer);
  return view.getUint16(buf.byteOffset);
}
function str2bytes(str) {
  const buf = fromString4(str);
  const size = Uint8Array.from(import_varint4.default.encode(buf.length));
  return concat([size, buf], size.length + buf.length);
}
function bytes2str(buf) {
  const size = import_varint4.default.decode(buf);
  buf = buf.slice(import_varint4.default.decode.bytes);
  if (buf.length !== size) {
    throw new Error("inconsistent lengths");
  }
  return toString4(buf);
}
function mh2bytes(hash2) {
  let mh;
  if (hash2[0] === "Q" || hash2[0] === "1") {
    mh = decode9(base58btc2.decode(`z${hash2}`)).bytes;
  } else {
    mh = CID2.parse(hash2).multihash.bytes;
  }
  const size = Uint8Array.from(import_varint4.default.encode(mh.length));
  return concat([size, mh], size.length + mh.length);
}
function mb2bytes(mbstr) {
  const mb = anybaseDecoder.decode(mbstr);
  const size = Uint8Array.from(import_varint4.default.encode(mb.length));
  return concat([size, mb], size.length + mb.length);
}
function bytes2mb(buf) {
  const size = import_varint4.default.decode(buf);
  const hash2 = buf.slice(import_varint4.default.decode.bytes);
  if (hash2.length !== size) {
    throw new Error("inconsistent lengths");
  }
  return "u" + toString4(hash2, "base64url");
}
function bytes2mh(buf) {
  const size = import_varint4.default.decode(buf);
  const address = buf.slice(import_varint4.default.decode.bytes);
  if (address.length !== size) {
    throw new Error("inconsistent lengths");
  }
  return toString4(address, "base58btc");
}
function onion2bytes(str) {
  const addr = str.split(":");
  if (addr.length !== 2) {
    throw new Error(`failed to parse onion addr: ["'${addr.join('", "')}'"]' does not contain a port number`);
  }
  if (addr[0].length !== 16) {
    throw new Error(`failed to parse onion addr: ${addr[0]} not a Tor onion address.`);
  }
  const buf = base322.decode("b" + addr[0]);
  const port = parseInt(addr[1], 10);
  if (port < 1 || port > 65536) {
    throw new Error("Port number is not in range(1, 65536)");
  }
  const portBuf = port2bytes(port);
  return concat([buf, portBuf], buf.length + portBuf.length);
}
function onion32bytes(str) {
  const addr = str.split(":");
  if (addr.length !== 2) {
    throw new Error(`failed to parse onion addr: ["'${addr.join('", "')}'"]' does not contain a port number`);
  }
  if (addr[0].length !== 56) {
    throw new Error(`failed to parse onion addr: ${addr[0]} not a Tor onion3 address.`);
  }
  const buf = base322.decode(`b${addr[0]}`);
  const port = parseInt(addr[1], 10);
  if (port < 1 || port > 65536) {
    throw new Error("Port number is not in range(1, 65536)");
  }
  const portBuf = port2bytes(port);
  return concat([buf, portBuf], buf.length + portBuf.length);
}
function bytes2onion(buf) {
  const addrBytes = buf.slice(0, buf.length - 2);
  const portBytes = buf.slice(buf.length - 2);
  const addr = toString4(addrBytes, "base32");
  const port = bytes2port(portBytes);
  return `${addr}:${port}`;
}

// ../../../node_modules/@multiformats/multiaddr/dist/src/codec.js
function stringToStringTuples(str) {
  const tuples = [];
  const parts = str.split("/").slice(1);
  if (parts.length === 1 && parts[0] === "") {
    return [];
  }
  for (let p = 0; p < parts.length; p++) {
    const part = parts[p];
    const proto = getProtocol(part);
    if (proto.size === 0) {
      tuples.push([part]);
      continue;
    }
    p++;
    if (p >= parts.length) {
      throw ParseError("invalid address: " + str);
    }
    if (proto.path === true) {
      tuples.push([
        part,
        // should we need to check each path part to see if it's a proto?
        // This would allow for other protocols to be added after a unix path,
        // however it would have issues if the path had a protocol name in the path
        cleanPath(parts.slice(p).join("/"))
      ]);
      break;
    }
    tuples.push([part, parts[p]]);
  }
  return tuples;
}
function stringTuplesToString(tuples) {
  const parts = [];
  tuples.map((tup) => {
    const proto = protoFromTuple(tup);
    parts.push(proto.name);
    if (tup.length > 1 && tup[1] != null) {
      parts.push(tup[1]);
    }
    return null;
  });
  return cleanPath(parts.join("/"));
}
function stringTuplesToTuples(tuples) {
  return tuples.map((tup) => {
    if (!Array.isArray(tup)) {
      tup = [tup];
    }
    const proto = protoFromTuple(tup);
    if (tup.length > 1) {
      return [proto.code, convertToBytes(proto.code, tup[1])];
    }
    return [proto.code];
  });
}
function tuplesToStringTuples(tuples) {
  return tuples.map((tup) => {
    const proto = protoFromTuple(tup);
    if (tup[1] != null) {
      return [proto.code, convertToString(proto.code, tup[1])];
    }
    return [proto.code];
  });
}
function tuplesToBytes(tuples) {
  return fromBytes(concat(tuples.map((tup) => {
    const proto = protoFromTuple(tup);
    let buf = Uint8Array.from(import_varint5.default.encode(proto.code));
    if (tup.length > 1 && tup[1] != null) {
      buf = concat([buf, tup[1]]);
    }
    return buf;
  })));
}
function sizeForAddr(p, addr) {
  if (p.size > 0) {
    return p.size / 8;
  } else if (p.size === 0) {
    return 0;
  } else {
    const size = import_varint5.default.decode(addr);
    return size + (import_varint5.default.decode.bytes ?? 0);
  }
}
function bytesToTuples(buf) {
  const tuples = [];
  let i = 0;
  while (i < buf.length) {
    const code5 = import_varint5.default.decode(buf, i);
    const n = import_varint5.default.decode.bytes ?? 0;
    const p = getProtocol(code5);
    const size = sizeForAddr(p, buf.slice(i + n));
    if (size === 0) {
      tuples.push([code5]);
      i += n;
      continue;
    }
    const addr = buf.slice(i + n, i + n + size);
    i += size + n;
    if (i > buf.length) {
      throw ParseError("Invalid address Uint8Array: " + toString4(buf, "base16"));
    }
    tuples.push([code5, addr]);
  }
  return tuples;
}
function bytesToString(buf) {
  const a = bytesToTuples(buf);
  const b = tuplesToStringTuples(a);
  return stringTuplesToString(b);
}
function stringToBytes(str) {
  str = cleanPath(str);
  const a = stringToStringTuples(str);
  const b = stringTuplesToTuples(a);
  return tuplesToBytes(b);
}
function fromString5(str) {
  return stringToBytes(str);
}
function fromBytes(buf) {
  const err = validateBytes(buf);
  if (err != null) {
    throw err;
  }
  return Uint8Array.from(buf);
}
function validateBytes(buf) {
  try {
    bytesToTuples(buf);
  } catch (err) {
    return err;
  }
}
function cleanPath(str) {
  return "/" + str.trim().split("/").filter((a) => a).join("/");
}
function ParseError(str) {
  return new Error("Error parsing address: " + str);
}
function protoFromTuple(tup) {
  const proto = getProtocol(tup[0]);
  return proto;
}

// ../../../node_modules/@multiformats/multiaddr/dist/src/index.js
var inspect2 = Symbol.for("nodejs.util.inspect.custom");
var DNS_CODES = [
  getProtocol("dns").code,
  getProtocol("dns4").code,
  getProtocol("dns6").code,
  getProtocol("dnsaddr").code
];
var resolvers = /* @__PURE__ */ new Map();
var symbol3 = Symbol.for("@multiformats/js-multiaddr/multiaddr");
function isMultiaddr(value) {
  return Boolean(value?.[symbol3]);
}
var DefaultMultiaddr = class _DefaultMultiaddr {
  bytes;
  #string;
  #tuples;
  #stringTuples;
  #path;
  [symbol3] = true;
  constructor(addr) {
    if (addr == null) {
      addr = "";
    }
    if (addr instanceof Uint8Array) {
      this.bytes = fromBytes(addr);
    } else if (typeof addr === "string") {
      if (addr.length > 0 && addr.charAt(0) !== "/") {
        throw new Error(`multiaddr "${addr}" must start with a "/"`);
      }
      this.bytes = fromString5(addr);
    } else if (isMultiaddr(addr)) {
      this.bytes = fromBytes(addr.bytes);
    } else {
      throw new Error("addr must be a string, Buffer, or another Multiaddr");
    }
  }
  toString() {
    if (this.#string == null) {
      this.#string = bytesToString(this.bytes);
    }
    return this.#string;
  }
  toJSON() {
    return this.toString();
  }
  toOptions() {
    let family;
    let transport;
    let host;
    let port;
    let zone = "";
    const tcp = getProtocol("tcp");
    const udp = getProtocol("udp");
    const ip4 = getProtocol("ip4");
    const ip6 = getProtocol("ip6");
    const dns6 = getProtocol("dns6");
    const ip6zone = getProtocol("ip6zone");
    for (const [code5, value] of this.stringTuples()) {
      if (code5 === ip6zone.code) {
        zone = `%${value ?? ""}`;
      }
      if (DNS_CODES.includes(code5)) {
        transport = tcp.name;
        port = 443;
        host = `${value ?? ""}${zone}`;
        family = code5 === dns6.code ? 6 : 4;
      }
      if (code5 === tcp.code || code5 === udp.code) {
        transport = getProtocol(code5).name;
        port = parseInt(value ?? "");
      }
      if (code5 === ip4.code || code5 === ip6.code) {
        transport = getProtocol(code5).name;
        host = `${value ?? ""}${zone}`;
        family = code5 === ip6.code ? 6 : 4;
      }
    }
    if (family == null || transport == null || host == null || port == null) {
      throw new Error('multiaddr must have a valid format: "/{ip4, ip6, dns4, dns6, dnsaddr}/{address}/{tcp, udp}/{port}".');
    }
    const opts = {
      family,
      host,
      transport,
      port
    };
    return opts;
  }
  protos() {
    return this.protoCodes().map((code5) => Object.assign({}, getProtocol(code5)));
  }
  protoCodes() {
    const codes3 = [];
    const buf = this.bytes;
    let i = 0;
    while (i < buf.length) {
      const code5 = import_varint6.default.decode(buf, i);
      const n = import_varint6.default.decode.bytes ?? 0;
      const p = getProtocol(code5);
      const size = sizeForAddr(p, buf.slice(i + n));
      i += size + n;
      codes3.push(code5);
    }
    return codes3;
  }
  protoNames() {
    return this.protos().map((proto) => proto.name);
  }
  tuples() {
    if (this.#tuples == null) {
      this.#tuples = bytesToTuples(this.bytes);
    }
    return this.#tuples;
  }
  stringTuples() {
    if (this.#stringTuples == null) {
      this.#stringTuples = tuplesToStringTuples(this.tuples());
    }
    return this.#stringTuples;
  }
  encapsulate(addr) {
    addr = new _DefaultMultiaddr(addr);
    return new _DefaultMultiaddr(this.toString() + addr.toString());
  }
  decapsulate(addr) {
    const addrString = addr.toString();
    const s = this.toString();
    const i = s.lastIndexOf(addrString);
    if (i < 0) {
      throw new Error(`Address ${this.toString()} does not contain subaddress: ${addr.toString()}`);
    }
    return new _DefaultMultiaddr(s.slice(0, i));
  }
  decapsulateCode(code5) {
    const tuples = this.tuples();
    for (let i = tuples.length - 1; i >= 0; i--) {
      if (tuples[i][0] === code5) {
        return new _DefaultMultiaddr(tuplesToBytes(tuples.slice(0, i)));
      }
    }
    return this;
  }
  getPeerId() {
    try {
      const tuples = this.stringTuples().filter((tuple2) => {
        if (tuple2[0] === names.ipfs.code) {
          return true;
        }
        return false;
      });
      const tuple = tuples.pop();
      if (tuple?.[1] != null) {
        const peerIdStr = tuple[1];
        if (peerIdStr[0] === "Q" || peerIdStr[0] === "1") {
          return toString4(base58btc2.decode(`z${peerIdStr}`), "base58btc");
        }
        return toString4(CID2.parse(peerIdStr).multihash.bytes, "base58btc");
      }
      return null;
    } catch (e) {
      return null;
    }
  }
  getPath() {
    if (this.#path === void 0) {
      try {
        this.#path = this.stringTuples().filter((tuple) => {
          const proto = getProtocol(tuple[0]);
          if (proto.path === true) {
            return true;
          }
          return false;
        })[0][1];
        if (this.#path == null) {
          this.#path = null;
        }
      } catch {
        this.#path = null;
      }
    }
    return this.#path;
  }
  equals(addr) {
    return equals3(this.bytes, addr.bytes);
  }
  async resolve(options) {
    const resolvableProto = this.protos().find((p) => p.resolvable);
    if (resolvableProto == null) {
      return [this];
    }
    const resolver = resolvers.get(resolvableProto.name);
    if (resolver == null) {
      throw new CodeError2(`no available resolver for ${resolvableProto.name}`, "ERR_NO_AVAILABLE_RESOLVER");
    }
    const addresses = await resolver(this, options);
    return addresses.map((a) => new _DefaultMultiaddr(a));
  }
  nodeAddress() {
    const options = this.toOptions();
    if (options.transport !== "tcp" && options.transport !== "udp") {
      throw new Error(`multiaddr must have a valid format - no protocol with name: "${options.transport}". Must have a valid transport protocol: "{tcp, udp}"`);
    }
    return {
      family: options.family,
      address: options.host,
      port: options.port
    };
  }
  isThinWaistAddress(addr) {
    const protos = (addr ?? this).protos();
    if (protos.length !== 2) {
      return false;
    }
    if (protos[0].code !== 4 && protos[0].code !== 41) {
      return false;
    }
    if (protos[1].code !== 6 && protos[1].code !== 273) {
      return false;
    }
    return true;
  }
  /**
   * Returns Multiaddr as a human-readable string
   * https://nodejs.org/api/util.html#utilinspectcustom
   *
   * @example
   * ```js
   * import { multiaddr } from '@multiformats/multiaddr'
   *
   * console.info(multiaddr('/ip4/127.0.0.1/tcp/4001'))
   * // 'Multiaddr(/ip4/127.0.0.1/tcp/4001)'
   * ```
   */
  [inspect2]() {
    return `Multiaddr(${bytesToString(this.bytes)})`;
  }
};
function multiaddr(addr) {
  return new DefaultMultiaddr(addr);
}

// error.ts
var WebRTCTransportError = class extends CodeError {
  constructor(msg, code5) {
    super(`WebRTC transport error: ${msg}`, code5 ?? "");
    this.name = "WebRTCTransportError";
  }
};
var DataChannelError = class extends WebRTCTransportError {
  constructor(streamLabel, msg) {
    super(`[stream: ${streamLabel}] data channel error: ${msg}`, "ERR_DATA_CHANNEL" /* ERR_DATA_CHANNEL */);
    this.name = "WebRTC/DataChannelError";
  }
};
function dataChannelError(streamLabel, msg) {
  return new DataChannelError(streamLabel, msg);
}
var InappropriateMultiaddrError = class extends WebRTCTransportError {
  constructor(msg) {
    super(`There was a problem with the Multiaddr which was passed in: ${msg}`, "ERR_INVALID_MULTIADDR" /* ERR_INVALID_MULTIADDR */);
    this.name = "WebRTC/InappropriateMultiaddrError";
  }
};
function inappropriateMultiaddr(msg) {
  return new InappropriateMultiaddrError(msg);
}
var InvalidArgumentError = class extends WebRTCTransportError {
  constructor(msg) {
    super(`There was a problem with a provided argument: ${msg}`, "ERR_INVALID_PARAMETERS" /* ERR_INVALID_PARAMETERS */);
    this.name = "WebRTC/InvalidArgumentError";
  }
};
function invalidArgument(msg) {
  return new InvalidArgumentError(msg);
}
var InvalidFingerprintError = class extends WebRTCTransportError {
  constructor(fingerprint, source) {
    super(`Invalid fingerprint "${fingerprint}" within ${source}`, "ERR_INVALID_FINGERPRINT" /* ERR_INVALID_FINGERPRINT */);
    this.name = "WebRTC/InvalidFingerprintError";
  }
};
function invalidFingerprint(fingerprint, source) {
  return new InvalidFingerprintError(fingerprint, source);
}
var UnimplementedError = class extends WebRTCTransportError {
  constructor(methodName) {
    super(`A method (${methodName}) was called though it has been intentionally left unimplemented.`, "ERR_NOT_IMPLEMENTED" /* ERR_NOT_IMPLEMENTED */);
    this.name = "WebRTC/UnimplementedError";
  }
};
function unimplemented(methodName) {
  return new UnimplementedError(methodName);
}
var UnsupportedHashAlgorithmError = class extends WebRTCTransportError {
  constructor(algo) {
    super(`unsupported hash algorithm: ${algo}`, "ERR_HASH_NOT_SUPPORTED" /* ERR_HASH_NOT_SUPPORTED */);
    this.name = "WebRTC/UnsupportedHashAlgorithmError";
  }
};
function unsupportedHashAlgorithm(algorithm) {
  return new UnsupportedHashAlgorithmError(algorithm);
}

// ../../../node_modules/detect-browser/es/index.js
var __spreadArray = function(to, from9, pack) {
  if (pack || arguments.length === 2)
    for (var i = 0, l = from9.length, ar; i < l; i++) {
      if (ar || !(i in from9)) {
        if (!ar)
          ar = Array.prototype.slice.call(from9, 0, i);
        ar[i] = from9[i];
      }
    }
  return to.concat(ar || Array.prototype.slice.call(from9));
};
var BrowserInfo = (
  /** @class */
  function() {
    function BrowserInfo2(name5, version2, os) {
      this.name = name5;
      this.version = version2;
      this.os = os;
      this.type = "browser";
    }
    return BrowserInfo2;
  }()
);
var NodeInfo = (
  /** @class */
  function() {
    function NodeInfo2(version2) {
      this.version = version2;
      this.type = "node";
      this.name = "node";
      this.os = process.platform;
    }
    return NodeInfo2;
  }()
);
var SearchBotDeviceInfo = (
  /** @class */
  function() {
    function SearchBotDeviceInfo2(name5, version2, os, bot) {
      this.name = name5;
      this.version = version2;
      this.os = os;
      this.bot = bot;
      this.type = "bot-device";
    }
    return SearchBotDeviceInfo2;
  }()
);
var BotInfo = (
  /** @class */
  function() {
    function BotInfo2() {
      this.type = "bot";
      this.bot = true;
      this.name = "bot";
      this.version = null;
      this.os = null;
    }
    return BotInfo2;
  }()
);
var ReactNativeInfo = (
  /** @class */
  function() {
    function ReactNativeInfo2() {
      this.type = "react-native";
      this.name = "react-native";
      this.version = null;
      this.os = null;
    }
    return ReactNativeInfo2;
  }()
);
var SEARCHBOX_UA_REGEX = /alexa|bot|crawl(er|ing)|facebookexternalhit|feedburner|google web preview|nagios|postrank|pingdom|slurp|spider|yahoo!|yandex/;
var SEARCHBOT_OS_REGEX = /(nuhk|curl|Googlebot|Yammybot|Openbot|Slurp|MSNBot|Ask\ Jeeves\/Teoma|ia_archiver)/;
var REQUIRED_VERSION_PARTS = 3;
var userAgentRules = [
  ["aol", /AOLShield\/([0-9\._]+)/],
  ["edge", /Edge\/([0-9\._]+)/],
  ["edge-ios", /EdgiOS\/([0-9\._]+)/],
  ["yandexbrowser", /YaBrowser\/([0-9\._]+)/],
  ["kakaotalk", /KAKAOTALK\s([0-9\.]+)/],
  ["samsung", /SamsungBrowser\/([0-9\.]+)/],
  ["silk", /\bSilk\/([0-9._-]+)\b/],
  ["miui", /MiuiBrowser\/([0-9\.]+)$/],
  ["beaker", /BeakerBrowser\/([0-9\.]+)/],
  ["edge-chromium", /EdgA?\/([0-9\.]+)/],
  [
    "chromium-webview",
    /(?!Chrom.*OPR)wv\).*Chrom(?:e|ium)\/([0-9\.]+)(:?\s|$)/
  ],
  ["chrome", /(?!Chrom.*OPR)Chrom(?:e|ium)\/([0-9\.]+)(:?\s|$)/],
  ["phantomjs", /PhantomJS\/([0-9\.]+)(:?\s|$)/],
  ["crios", /CriOS\/([0-9\.]+)(:?\s|$)/],
  ["firefox", /Firefox\/([0-9\.]+)(?:\s|$)/],
  ["fxios", /FxiOS\/([0-9\.]+)/],
  ["opera-mini", /Opera Mini.*Version\/([0-9\.]+)/],
  ["opera", /Opera\/([0-9\.]+)(?:\s|$)/],
  ["opera", /OPR\/([0-9\.]+)(:?\s|$)/],
  ["pie", /^Microsoft Pocket Internet Explorer\/(\d+\.\d+)$/],
  ["pie", /^Mozilla\/\d\.\d+\s\(compatible;\s(?:MSP?IE|MSInternet Explorer) (\d+\.\d+);.*Windows CE.*\)$/],
  ["netfront", /^Mozilla\/\d\.\d+.*NetFront\/(\d.\d)/],
  ["ie", /Trident\/7\.0.*rv\:([0-9\.]+).*\).*Gecko$/],
  ["ie", /MSIE\s([0-9\.]+);.*Trident\/[4-7].0/],
  ["ie", /MSIE\s(7\.0)/],
  ["bb10", /BB10;\sTouch.*Version\/([0-9\.]+)/],
  ["android", /Android\s([0-9\.]+)/],
  ["ios", /Version\/([0-9\._]+).*Mobile.*Safari.*/],
  ["safari", /Version\/([0-9\._]+).*Safari/],
  ["facebook", /FB[AS]V\/([0-9\.]+)/],
  ["instagram", /Instagram\s([0-9\.]+)/],
  ["ios-webview", /AppleWebKit\/([0-9\.]+).*Mobile/],
  ["ios-webview", /AppleWebKit\/([0-9\.]+).*Gecko\)$/],
  ["curl", /^curl\/([0-9\.]+)$/],
  ["searchbot", SEARCHBOX_UA_REGEX]
];
var operatingSystemRules = [
  ["iOS", /iP(hone|od|ad)/],
  ["Android OS", /Android/],
  ["BlackBerry OS", /BlackBerry|BB10/],
  ["Windows Mobile", /IEMobile/],
  ["Amazon OS", /Kindle/],
  ["Windows 3.11", /Win16/],
  ["Windows 95", /(Windows 95)|(Win95)|(Windows_95)/],
  ["Windows 98", /(Windows 98)|(Win98)/],
  ["Windows 2000", /(Windows NT 5.0)|(Windows 2000)/],
  ["Windows XP", /(Windows NT 5.1)|(Windows XP)/],
  ["Windows Server 2003", /(Windows NT 5.2)/],
  ["Windows Vista", /(Windows NT 6.0)/],
  ["Windows 7", /(Windows NT 6.1)/],
  ["Windows 8", /(Windows NT 6.2)/],
  ["Windows 8.1", /(Windows NT 6.3)/],
  ["Windows 10", /(Windows NT 10.0)/],
  ["Windows ME", /Windows ME/],
  ["Windows CE", /Windows CE|WinCE|Microsoft Pocket Internet Explorer/],
  ["Open BSD", /OpenBSD/],
  ["Sun OS", /SunOS/],
  ["Chrome OS", /CrOS/],
  ["Linux", /(Linux)|(X11)/],
  ["Mac OS", /(Mac_PowerPC)|(Macintosh)/],
  ["QNX", /QNX/],
  ["BeOS", /BeOS/],
  ["OS/2", /OS\/2/]
];
function detect(userAgent) {
  if (!!userAgent) {
    return parseUserAgent(userAgent);
  }
  if (typeof document === "undefined" && typeof navigator !== "undefined" && navigator.product === "ReactNative") {
    return new ReactNativeInfo();
  }
  if (typeof navigator !== "undefined") {
    return parseUserAgent(navigator.userAgent);
  }
  return getNodeVersion();
}
function matchUserAgent(ua) {
  return ua !== "" && userAgentRules.reduce(function(matched, _a) {
    var browser2 = _a[0], regex = _a[1];
    if (matched) {
      return matched;
    }
    var uaMatch = regex.exec(ua);
    return !!uaMatch && [browser2, uaMatch];
  }, false);
}
function parseUserAgent(ua) {
  var matchedRule = matchUserAgent(ua);
  if (!matchedRule) {
    return null;
  }
  var name5 = matchedRule[0], match = matchedRule[1];
  if (name5 === "searchbot") {
    return new BotInfo();
  }
  var versionParts = match[1] && match[1].split(".").join("_").split("_").slice(0, 3);
  if (versionParts) {
    if (versionParts.length < REQUIRED_VERSION_PARTS) {
      versionParts = __spreadArray(__spreadArray([], versionParts, true), createVersionParts(REQUIRED_VERSION_PARTS - versionParts.length), true);
    }
  } else {
    versionParts = [];
  }
  var version2 = versionParts.join(".");
  var os = detectOS(ua);
  var searchBotMatch = SEARCHBOT_OS_REGEX.exec(ua);
  if (searchBotMatch && searchBotMatch[1]) {
    return new SearchBotDeviceInfo(name5, version2, os, searchBotMatch[1]);
  }
  return new BrowserInfo(name5, version2, os);
}
function detectOS(ua) {
  for (var ii = 0, count = operatingSystemRules.length; ii < count; ii++) {
    var _a = operatingSystemRules[ii], os = _a[0], regex = _a[1];
    var match = regex.exec(ua);
    if (match) {
      return os;
    }
  }
  return null;
}
function getNodeVersion() {
  var isNode = typeof process !== "undefined" && process.version;
  return isNode ? new NodeInfo(process.version.slice(1)) : null;
}
function createVersionParts(count) {
  var output2 = [];
  for (var ii = 0; ii < count; ii++) {
    output2.push("0");
  }
  return output2;
}

// util.ts
var browser = detect();
var isFirefox = browser != null && browser.name === "firefox";
var nopSource = async function* nop() {
};
var nopSink = async (_) => {
};

// maconn.ts
var log = logger("libp2p:webrtc:connection");
var WebRTCMultiaddrConnection = class {
  /**
   * WebRTC Peer Connection
   */
  peerConnection;
  /**
   * The multiaddr address used to communicate with the remote peer
   */
  remoteAddr;
  /**
   * Holds the lifecycle times of the connection
   */
  timeline;
  /**
   * Optional metrics counter group for this connection
   */
  metrics;
  /**
   * The stream source, a no-op as the transport natively supports multiplexing
   */
  source = nopSource();
  /**
   * The stream destination, a no-op as the transport natively supports multiplexing
   */
  sink = nopSink;
  constructor(init) {
    this.remoteAddr = init.remoteAddr;
    this.timeline = init.timeline;
    this.peerConnection = init.peerConnection;
    this.peerConnection.onconnectionstatechange = () => {
      if (this.peerConnection.connectionState === "closed" || this.peerConnection.connectionState === "disconnected" || this.peerConnection.connectionState === "failed") {
        this.timeline.close = Date.now();
      }
    };
  }
  async close(err) {
    if (err !== void 0) {
      log.error("error closing connection", err);
    }
    log.trace("closing connection");
    this.timeline.close = Date.now();
    this.peerConnection.close();
    this.metrics?.increment({ close: true });
  }
};

// ../../../node_modules/abortable-iterator/dist/src/abort-error.js
var AbortError = class extends Error {
  constructor(message2, code5) {
    super(message2 ?? "The operation was aborted");
    this.type = "aborted";
    this.code = code5 ?? "ABORT_ERR";
  }
};

// ../../../node_modules/get-iterator/dist/src/index.js
function getIterator(obj) {
  if (obj != null) {
    if (typeof obj[Symbol.iterator] === "function") {
      return obj[Symbol.iterator]();
    }
    if (typeof obj[Symbol.asyncIterator] === "function") {
      return obj[Symbol.asyncIterator]();
    }
    if (typeof obj.next === "function") {
      return obj;
    }
  }
  throw new Error("argument is not an iterator or iterable");
}

// ../../../node_modules/abortable-iterator/dist/src/index.js
function abortableSource(source, signal, options) {
  const opts = options ?? {};
  const iterator = getIterator(source);
  async function* abortable() {
    let nextAbortHandler;
    const abortHandler = () => {
      if (nextAbortHandler != null)
        nextAbortHandler();
    };
    signal.addEventListener("abort", abortHandler);
    while (true) {
      let result;
      try {
        if (signal.aborted) {
          const { abortMessage, abortCode } = opts;
          throw new AbortError(abortMessage, abortCode);
        }
        const abort = new Promise((resolve, reject) => {
          nextAbortHandler = () => {
            const { abortMessage, abortCode } = opts;
            reject(new AbortError(abortMessage, abortCode));
          };
        });
        result = await Promise.race([abort, iterator.next()]);
        nextAbortHandler = null;
      } catch (err) {
        signal.removeEventListener("abort", abortHandler);
        const isKnownAborter = err.type === "aborted" && signal.aborted;
        if (isKnownAborter && opts.onAbort != null) {
          opts.onAbort(source);
        }
        if (typeof iterator.return === "function") {
          try {
            const p = iterator.return();
            if (p instanceof Promise) {
              p.catch((err2) => {
                if (opts.onReturnError != null) {
                  opts.onReturnError(err2);
                }
              });
            }
          } catch (err2) {
            if (opts.onReturnError != null) {
              opts.onReturnError(err2);
            }
          }
        }
        if (isKnownAborter && opts.returnOnAbort === true) {
          return;
        }
        throw err;
      }
      if (result.done === true) {
        break;
      }
      yield result.value;
    }
    signal.removeEventListener("abort", abortHandler);
  }
  return abortable();
}
function abortableSink(sink, signal, options) {
  return (source) => sink(abortableSource(source, signal, options));
}
function abortableDuplex(duplex, signal, options) {
  return {
    sink: abortableSink(duplex.sink, signal, {
      ...options,
      onAbort: void 0
    }),
    source: abortableSource(duplex.source, signal, options)
  };
}

// ../../../node_modules/it-pb-stream/dist/src/index.js
var import_err_code2 = __toESM(require_err_code(), 1);

// ../../../node_modules/uint8arraylist/dist/src/index.js
var symbol4 = Symbol.for("@achingbrain/uint8arraylist");
function findBufAndOffset(bufs, index) {
  if (index == null || index < 0) {
    throw new RangeError("index is out of bounds");
  }
  let offset = 0;
  for (const buf of bufs) {
    const bufEnd = offset + buf.byteLength;
    if (index < bufEnd) {
      return {
        buf,
        index: index - offset
      };
    }
    offset = bufEnd;
  }
  throw new RangeError("index is out of bounds");
}
function isUint8ArrayList(value) {
  return Boolean(value?.[symbol4]);
}
var Uint8ArrayList = class _Uint8ArrayList {
  constructor(...data) {
    Object.defineProperty(this, symbol4, { value: true });
    this.bufs = [];
    this.length = 0;
    if (data.length > 0) {
      this.appendAll(data);
    }
  }
  *[Symbol.iterator]() {
    yield* this.bufs;
  }
  get byteLength() {
    return this.length;
  }
  /**
   * Add one or more `bufs` to the end of this Uint8ArrayList
   */
  append(...bufs) {
    this.appendAll(bufs);
  }
  /**
   * Add all `bufs` to the end of this Uint8ArrayList
   */
  appendAll(bufs) {
    let length5 = 0;
    for (const buf of bufs) {
      if (buf instanceof Uint8Array) {
        length5 += buf.byteLength;
        this.bufs.push(buf);
      } else if (isUint8ArrayList(buf)) {
        length5 += buf.byteLength;
        this.bufs.push(...buf.bufs);
      } else {
        throw new Error("Could not append value, must be an Uint8Array or a Uint8ArrayList");
      }
    }
    this.length += length5;
  }
  /**
   * Add one or more `bufs` to the start of this Uint8ArrayList
   */
  prepend(...bufs) {
    this.prependAll(bufs);
  }
  /**
   * Add all `bufs` to the start of this Uint8ArrayList
   */
  prependAll(bufs) {
    let length5 = 0;
    for (const buf of bufs.reverse()) {
      if (buf instanceof Uint8Array) {
        length5 += buf.byteLength;
        this.bufs.unshift(buf);
      } else if (isUint8ArrayList(buf)) {
        length5 += buf.byteLength;
        this.bufs.unshift(...buf.bufs);
      } else {
        throw new Error("Could not prepend value, must be an Uint8Array or a Uint8ArrayList");
      }
    }
    this.length += length5;
  }
  /**
   * Read the value at `index`
   */
  get(index) {
    const res = findBufAndOffset(this.bufs, index);
    return res.buf[res.index];
  }
  /**
   * Set the value at `index` to `value`
   */
  set(index, value) {
    const res = findBufAndOffset(this.bufs, index);
    res.buf[res.index] = value;
  }
  /**
   * Copy bytes from `buf` to the index specified by `offset`
   */
  write(buf, offset = 0) {
    if (buf instanceof Uint8Array) {
      for (let i = 0; i < buf.length; i++) {
        this.set(offset + i, buf[i]);
      }
    } else if (isUint8ArrayList(buf)) {
      for (let i = 0; i < buf.length; i++) {
        this.set(offset + i, buf.get(i));
      }
    } else {
      throw new Error("Could not write value, must be an Uint8Array or a Uint8ArrayList");
    }
  }
  /**
   * Remove bytes from the front of the pool
   */
  consume(bytes2) {
    bytes2 = Math.trunc(bytes2);
    if (Number.isNaN(bytes2) || bytes2 <= 0) {
      return;
    }
    if (bytes2 === this.byteLength) {
      this.bufs = [];
      this.length = 0;
      return;
    }
    while (this.bufs.length > 0) {
      if (bytes2 >= this.bufs[0].byteLength) {
        bytes2 -= this.bufs[0].byteLength;
        this.length -= this.bufs[0].byteLength;
        this.bufs.shift();
      } else {
        this.bufs[0] = this.bufs[0].subarray(bytes2);
        this.length -= bytes2;
        break;
      }
    }
  }
  /**
   * Extracts a section of an array and returns a new array.
   *
   * This is a copy operation as it is with Uint8Arrays and Arrays
   * - note this is different to the behaviour of Node Buffers.
   */
  slice(beginInclusive, endExclusive) {
    const { bufs, length: length5 } = this._subList(beginInclusive, endExclusive);
    return concat(bufs, length5);
  }
  /**
   * Returns a alloc from the given start and end element index.
   *
   * In the best case where the data extracted comes from a single Uint8Array
   * internally this is a no-copy operation otherwise it is a copy operation.
   */
  subarray(beginInclusive, endExclusive) {
    const { bufs, length: length5 } = this._subList(beginInclusive, endExclusive);
    if (bufs.length === 1) {
      return bufs[0];
    }
    return concat(bufs, length5);
  }
  /**
   * Returns a allocList from the given start and end element index.
   *
   * This is a no-copy operation.
   */
  sublist(beginInclusive, endExclusive) {
    const { bufs, length: length5 } = this._subList(beginInclusive, endExclusive);
    const list = new _Uint8ArrayList();
    list.length = length5;
    list.bufs = bufs;
    return list;
  }
  _subList(beginInclusive, endExclusive) {
    beginInclusive = beginInclusive ?? 0;
    endExclusive = endExclusive ?? this.length;
    if (beginInclusive < 0) {
      beginInclusive = this.length + beginInclusive;
    }
    if (endExclusive < 0) {
      endExclusive = this.length + endExclusive;
    }
    if (beginInclusive < 0 || endExclusive > this.length) {
      throw new RangeError("index is out of bounds");
    }
    if (beginInclusive === endExclusive) {
      return { bufs: [], length: 0 };
    }
    if (beginInclusive === 0 && endExclusive === this.length) {
      return { bufs: [...this.bufs], length: this.length };
    }
    const bufs = [];
    let offset = 0;
    for (let i = 0; i < this.bufs.length; i++) {
      const buf = this.bufs[i];
      const bufStart = offset;
      const bufEnd = bufStart + buf.byteLength;
      offset = bufEnd;
      if (beginInclusive >= bufEnd) {
        continue;
      }
      const sliceStartInBuf = beginInclusive >= bufStart && beginInclusive < bufEnd;
      const sliceEndsInBuf = endExclusive > bufStart && endExclusive <= bufEnd;
      if (sliceStartInBuf && sliceEndsInBuf) {
        if (beginInclusive === bufStart && endExclusive === bufEnd) {
          bufs.push(buf);
          break;
        }
        const start = beginInclusive - bufStart;
        bufs.push(buf.subarray(start, start + (endExclusive - beginInclusive)));
        break;
      }
      if (sliceStartInBuf) {
        if (beginInclusive === 0) {
          bufs.push(buf);
          continue;
        }
        bufs.push(buf.subarray(beginInclusive - bufStart));
        continue;
      }
      if (sliceEndsInBuf) {
        if (endExclusive === bufEnd) {
          bufs.push(buf);
          break;
        }
        bufs.push(buf.subarray(0, endExclusive - bufStart));
        break;
      }
      bufs.push(buf);
    }
    return { bufs, length: endExclusive - beginInclusive };
  }
  indexOf(search, offset = 0) {
    if (!isUint8ArrayList(search) && !(search instanceof Uint8Array)) {
      throw new TypeError('The "value" argument must be a Uint8ArrayList or Uint8Array');
    }
    const needle = search instanceof Uint8Array ? search : search.subarray();
    offset = Number(offset ?? 0);
    if (isNaN(offset)) {
      offset = 0;
    }
    if (offset < 0) {
      offset = this.length + offset;
    }
    if (offset < 0) {
      offset = 0;
    }
    if (search.length === 0) {
      return offset > this.length ? this.length : offset;
    }
    const M = needle.byteLength;
    if (M === 0) {
      throw new TypeError("search must be at least 1 byte long");
    }
    const radix = 256;
    const rightmostPositions = new Int32Array(radix);
    for (let c = 0; c < radix; c++) {
      rightmostPositions[c] = -1;
    }
    for (let j = 0; j < M; j++) {
      rightmostPositions[needle[j]] = j;
    }
    const right = rightmostPositions;
    const lastIndex = this.byteLength - needle.byteLength;
    const lastPatIndex = needle.byteLength - 1;
    let skip;
    for (let i = offset; i <= lastIndex; i += skip) {
      skip = 0;
      for (let j = lastPatIndex; j >= 0; j--) {
        const char = this.get(i + j);
        if (needle[j] !== char) {
          skip = Math.max(1, j - right[char]);
          break;
        }
      }
      if (skip === 0) {
        return i;
      }
    }
    return -1;
  }
  getInt8(byteOffset) {
    const buf = this.subarray(byteOffset, byteOffset + 1);
    const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
    return view.getInt8(0);
  }
  setInt8(byteOffset, value) {
    const buf = allocUnsafe(1);
    const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
    view.setInt8(0, value);
    this.write(buf, byteOffset);
  }
  getInt16(byteOffset, littleEndian) {
    const buf = this.subarray(byteOffset, byteOffset + 2);
    const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
    return view.getInt16(0, littleEndian);
  }
  setInt16(byteOffset, value, littleEndian) {
    const buf = alloc(2);
    const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
    view.setInt16(0, value, littleEndian);
    this.write(buf, byteOffset);
  }
  getInt32(byteOffset, littleEndian) {
    const buf = this.subarray(byteOffset, byteOffset + 4);
    const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
    return view.getInt32(0, littleEndian);
  }
  setInt32(byteOffset, value, littleEndian) {
    const buf = alloc(4);
    const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
    view.setInt32(0, value, littleEndian);
    this.write(buf, byteOffset);
  }
  getBigInt64(byteOffset, littleEndian) {
    const buf = this.subarray(byteOffset, byteOffset + 8);
    const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
    return view.getBigInt64(0, littleEndian);
  }
  setBigInt64(byteOffset, value, littleEndian) {
    const buf = alloc(8);
    const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
    view.setBigInt64(0, value, littleEndian);
    this.write(buf, byteOffset);
  }
  getUint8(byteOffset) {
    const buf = this.subarray(byteOffset, byteOffset + 1);
    const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
    return view.getUint8(0);
  }
  setUint8(byteOffset, value) {
    const buf = allocUnsafe(1);
    const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
    view.setUint8(0, value);
    this.write(buf, byteOffset);
  }
  getUint16(byteOffset, littleEndian) {
    const buf = this.subarray(byteOffset, byteOffset + 2);
    const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
    return view.getUint16(0, littleEndian);
  }
  setUint16(byteOffset, value, littleEndian) {
    const buf = alloc(2);
    const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
    view.setUint16(0, value, littleEndian);
    this.write(buf, byteOffset);
  }
  getUint32(byteOffset, littleEndian) {
    const buf = this.subarray(byteOffset, byteOffset + 4);
    const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
    return view.getUint32(0, littleEndian);
  }
  setUint32(byteOffset, value, littleEndian) {
    const buf = alloc(4);
    const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
    view.setUint32(0, value, littleEndian);
    this.write(buf, byteOffset);
  }
  getBigUint64(byteOffset, littleEndian) {
    const buf = this.subarray(byteOffset, byteOffset + 8);
    const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
    return view.getBigUint64(0, littleEndian);
  }
  setBigUint64(byteOffset, value, littleEndian) {
    const buf = alloc(8);
    const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
    view.setBigUint64(0, value, littleEndian);
    this.write(buf, byteOffset);
  }
  getFloat32(byteOffset, littleEndian) {
    const buf = this.subarray(byteOffset, byteOffset + 4);
    const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
    return view.getFloat32(0, littleEndian);
  }
  setFloat32(byteOffset, value, littleEndian) {
    const buf = alloc(4);
    const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
    view.setFloat32(0, value, littleEndian);
    this.write(buf, byteOffset);
  }
  getFloat64(byteOffset, littleEndian) {
    const buf = this.subarray(byteOffset, byteOffset + 8);
    const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
    return view.getFloat64(0, littleEndian);
  }
  setFloat64(byteOffset, value, littleEndian) {
    const buf = alloc(8);
    const view = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
    view.setFloat64(0, value, littleEndian);
    this.write(buf, byteOffset);
  }
  equals(other) {
    if (other == null) {
      return false;
    }
    if (!(other instanceof _Uint8ArrayList)) {
      return false;
    }
    if (other.bufs.length !== this.bufs.length) {
      return false;
    }
    for (let i = 0; i < this.bufs.length; i++) {
      if (!equals3(this.bufs[i], other.bufs[i])) {
        return false;
      }
    }
    return true;
  }
  /**
   * Create a Uint8ArrayList from a pre-existing list of Uint8Arrays.  Use this
   * method if you know the total size of all the Uint8Arrays ahead of time.
   */
  static fromUint8Arrays(bufs, length5) {
    const list = new _Uint8ArrayList();
    list.bufs = bufs;
    if (length5 == null) {
      length5 = bufs.reduce((acc, curr) => acc + curr.byteLength, 0);
    }
    list.length = length5;
    return list;
  }
};

// ../../../node_modules/byte-access/dist/src/index.js
function accessor(buf) {
  if (buf instanceof Uint8Array) {
    return {
      get(index) {
        return buf[index];
      },
      set(index, value) {
        buf[index] = value;
      }
    };
  }
  return {
    get(index) {
      return buf.get(index);
    },
    set(index, value) {
      buf.set(index, value);
    }
  };
}

// ../../../node_modules/longbits/dist/src/index.js
var TWO_32 = 4294967296;
var LongBits = class _LongBits {
  constructor(hi = 0, lo = 0) {
    this.hi = hi;
    this.lo = lo;
  }
  /**
   * Returns these hi/lo bits as a BigInt
   */
  toBigInt(unsigned2) {
    if (unsigned2 === true) {
      return BigInt(this.lo >>> 0) + (BigInt(this.hi >>> 0) << 32n);
    }
    if (this.hi >>> 31 !== 0) {
      const lo = ~this.lo + 1 >>> 0;
      let hi = ~this.hi >>> 0;
      if (lo === 0) {
        hi = hi + 1 >>> 0;
      }
      return -(BigInt(lo) + (BigInt(hi) << 32n));
    }
    return BigInt(this.lo >>> 0) + (BigInt(this.hi >>> 0) << 32n);
  }
  /**
   * Returns these hi/lo bits as a Number - this may overflow, toBigInt
   * should be preferred
   */
  toNumber(unsigned2) {
    return Number(this.toBigInt(unsigned2));
  }
  /**
   * ZigZag decode a LongBits object
   */
  zzDecode() {
    const mask = -(this.lo & 1);
    const lo = ((this.lo >>> 1 | this.hi << 31) ^ mask) >>> 0;
    const hi = (this.hi >>> 1 ^ mask) >>> 0;
    return new _LongBits(hi, lo);
  }
  /**
   * ZigZag encode a LongBits object
   */
  zzEncode() {
    const mask = this.hi >> 31;
    const hi = ((this.hi << 1 | this.lo >>> 31) ^ mask) >>> 0;
    const lo = (this.lo << 1 ^ mask) >>> 0;
    return new _LongBits(hi, lo);
  }
  /**
   * Encode a LongBits object as a varint byte array
   */
  toBytes(buf, offset = 0) {
    const access = accessor(buf);
    while (this.hi > 0) {
      access.set(offset++, this.lo & 127 | 128);
      this.lo = (this.lo >>> 7 | this.hi << 25) >>> 0;
      this.hi >>>= 7;
    }
    while (this.lo > 127) {
      access.set(offset++, this.lo & 127 | 128);
      this.lo = this.lo >>> 7;
    }
    access.set(offset++, this.lo);
  }
  /**
   * Parse a LongBits object from a BigInt
   */
  static fromBigInt(value) {
    if (value === 0n) {
      return new _LongBits();
    }
    const negative = value < 0;
    if (negative) {
      value = -value;
    }
    let hi = Number(value >> 32n) | 0;
    let lo = Number(value - (BigInt(hi) << 32n)) | 0;
    if (negative) {
      hi = ~hi >>> 0;
      lo = ~lo >>> 0;
      if (++lo > TWO_32) {
        lo = 0;
        if (++hi > TWO_32) {
          hi = 0;
        }
      }
    }
    return new _LongBits(hi, lo);
  }
  /**
   * Parse a LongBits object from a Number
   */
  static fromNumber(value) {
    if (value === 0) {
      return new _LongBits();
    }
    const sign3 = value < 0;
    if (sign3) {
      value = -value;
    }
    let lo = value >>> 0;
    let hi = (value - lo) / 4294967296 >>> 0;
    if (sign3) {
      hi = ~hi >>> 0;
      lo = ~lo >>> 0;
      if (++lo > 4294967295) {
        lo = 0;
        if (++hi > 4294967295) {
          hi = 0;
        }
      }
    }
    return new _LongBits(hi, lo);
  }
  /**
   * Parse a LongBits object from a varint byte array
   */
  static fromBytes(buf, offset = 0) {
    const access = accessor(buf);
    const bits2 = new _LongBits();
    let i = 0;
    if (buf.length - offset > 4) {
      for (; i < 4; ++i) {
        bits2.lo = (bits2.lo | (access.get(offset) & 127) << i * 7) >>> 0;
        if (access.get(offset++) < 128) {
          return bits2;
        }
      }
      bits2.lo = (bits2.lo | (access.get(offset) & 127) << 28) >>> 0;
      bits2.hi = (bits2.hi | (access.get(offset) & 127) >> 4) >>> 0;
      if (access.get(offset++) < 128) {
        return bits2;
      }
      i = 0;
    } else {
      for (; i < 4; ++i) {
        if (offset >= buf.length) {
          throw RangeError(`index out of range: ${offset} > ${buf.length}`);
        }
        bits2.lo = (bits2.lo | (access.get(offset) & 127) << i * 7) >>> 0;
        if (access.get(offset++) < 128) {
          return bits2;
        }
      }
    }
    if (buf.length - offset > 4) {
      for (; i < 5; ++i) {
        bits2.hi = (bits2.hi | (access.get(offset) & 127) << i * 7 + 3) >>> 0;
        if (access.get(offset++) < 128) {
          return bits2;
        }
      }
    } else if (offset < buf.byteLength) {
      for (; i < 5; ++i) {
        if (offset >= buf.length) {
          throw RangeError(`index out of range: ${offset} > ${buf.length}`);
        }
        bits2.hi = (bits2.hi | (access.get(offset) & 127) << i * 7 + 3) >>> 0;
        if (access.get(offset++) < 128) {
          return bits2;
        }
      }
    }
    throw RangeError("invalid varint encoding");
  }
};

// ../../../node_modules/uint8-varint/dist/src/index.js
var N14 = Math.pow(2, 7);
var N24 = Math.pow(2, 14);
var N34 = Math.pow(2, 21);
var N44 = Math.pow(2, 28);
var N54 = Math.pow(2, 35);
var N64 = Math.pow(2, 42);
var N74 = Math.pow(2, 49);
var N84 = Math.pow(2, 56);
var N94 = Math.pow(2, 63);
var unsigned = {
  encodingLength(value) {
    if (value < N14) {
      return 1;
    }
    if (value < N24) {
      return 2;
    }
    if (value < N34) {
      return 3;
    }
    if (value < N44) {
      return 4;
    }
    if (value < N54) {
      return 5;
    }
    if (value < N64) {
      return 6;
    }
    if (value < N74) {
      return 7;
    }
    if (value < N84) {
      return 8;
    }
    if (value < N94) {
      return 9;
    }
    return 10;
  },
  encode(value, buf, offset = 0) {
    if (Number.MAX_SAFE_INTEGER != null && value > Number.MAX_SAFE_INTEGER) {
      throw new RangeError("Could not encode varint");
    }
    if (buf == null) {
      buf = allocUnsafe(unsigned.encodingLength(value));
    }
    LongBits.fromNumber(value).toBytes(buf, offset);
    return buf;
  },
  decode(buf, offset = 0) {
    return LongBits.fromBytes(buf, offset).toNumber(true);
  }
};

// ../../../node_modules/it-length-prefixed/dist/src/utils.js
function isAsyncIterable(thing) {
  return thing[Symbol.asyncIterator] != null;
}

// ../../../node_modules/it-length-prefixed/dist/src/encode.js
var defaultEncoder = (length5) => {
  const lengthLength = unsigned.encodingLength(length5);
  const lengthBuf = allocUnsafe(lengthLength);
  unsigned.encode(length5, lengthBuf);
  defaultEncoder.bytes = lengthLength;
  return lengthBuf;
};
defaultEncoder.bytes = 0;
function encode13(source, options) {
  options = options ?? {};
  const encodeLength = options.lengthEncoder ?? defaultEncoder;
  function* maybeYield(chunk) {
    const length5 = encodeLength(chunk.byteLength);
    if (length5 instanceof Uint8Array) {
      yield length5;
    } else {
      yield* length5;
    }
    if (chunk instanceof Uint8Array) {
      yield chunk;
    } else {
      yield* chunk;
    }
  }
  if (isAsyncIterable(source)) {
    return async function* () {
      for await (const chunk of source) {
        yield* maybeYield(chunk);
      }
    }();
  }
  return function* () {
    for (const chunk of source) {
      yield* maybeYield(chunk);
    }
  }();
}
encode13.single = (chunk, options) => {
  options = options ?? {};
  const encodeLength = options.lengthEncoder ?? defaultEncoder;
  return new Uint8ArrayList(encodeLength(chunk.byteLength), chunk);
};

// ../../../node_modules/it-length-prefixed/dist/src/decode.js
var import_err_code = __toESM(require_err_code(), 1);
var MAX_LENGTH_LENGTH = 8;
var MAX_DATA_LENGTH = 1024 * 1024 * 4;
var ReadMode;
(function(ReadMode2) {
  ReadMode2[ReadMode2["LENGTH"] = 0] = "LENGTH";
  ReadMode2[ReadMode2["DATA"] = 1] = "DATA";
})(ReadMode || (ReadMode = {}));
var defaultDecoder = (buf) => {
  const length5 = unsigned.decode(buf);
  defaultDecoder.bytes = unsigned.encodingLength(length5);
  return length5;
};
defaultDecoder.bytes = 0;
function decode16(source, options) {
  const buffer = new Uint8ArrayList();
  let mode = ReadMode.LENGTH;
  let dataLength = -1;
  const lengthDecoder = options?.lengthDecoder ?? defaultDecoder;
  const maxLengthLength = options?.maxLengthLength ?? MAX_LENGTH_LENGTH;
  const maxDataLength = options?.maxDataLength ?? MAX_DATA_LENGTH;
  function* maybeYield() {
    while (buffer.byteLength > 0) {
      if (mode === ReadMode.LENGTH) {
        try {
          dataLength = lengthDecoder(buffer);
          if (dataLength < 0) {
            throw (0, import_err_code.default)(new Error("invalid message length"), "ERR_INVALID_MSG_LENGTH");
          }
          if (dataLength > maxDataLength) {
            throw (0, import_err_code.default)(new Error("message length too long"), "ERR_MSG_DATA_TOO_LONG");
          }
          const dataLengthLength = lengthDecoder.bytes;
          buffer.consume(dataLengthLength);
          if (options?.onLength != null) {
            options.onLength(dataLength);
          }
          mode = ReadMode.DATA;
        } catch (err) {
          if (err instanceof RangeError) {
            if (buffer.byteLength > maxLengthLength) {
              throw (0, import_err_code.default)(new Error("message length length too long"), "ERR_MSG_LENGTH_TOO_LONG");
            }
            break;
          }
          throw err;
        }
      }
      if (mode === ReadMode.DATA) {
        if (buffer.byteLength < dataLength) {
          break;
        }
        const data = buffer.sublist(0, dataLength);
        buffer.consume(dataLength);
        if (options?.onData != null) {
          options.onData(data);
        }
        yield data;
        mode = ReadMode.LENGTH;
      }
    }
  }
  if (isAsyncIterable(source)) {
    return async function* () {
      for await (const buf of source) {
        buffer.append(buf);
        yield* maybeYield();
      }
      if (buffer.byteLength > 0) {
        throw (0, import_err_code.default)(new Error("unexpected end of input"), "ERR_UNEXPECTED_EOF");
      }
    }();
  }
  return function* () {
    for (const buf of source) {
      buffer.append(buf);
      yield* maybeYield();
    }
    if (buffer.byteLength > 0) {
      throw (0, import_err_code.default)(new Error("unexpected end of input"), "ERR_UNEXPECTED_EOF");
    }
  }();
}
decode16.fromReader = (reader2, options) => {
  let byteLength = 1;
  const varByteSource = async function* () {
    while (true) {
      try {
        const { done, value } = await reader2.next(byteLength);
        if (done === true) {
          return;
        }
        if (value != null) {
          yield value;
        }
      } catch (err) {
        if (err.code === "ERR_UNDER_READ") {
          return { done: true, value: null };
        }
        throw err;
      } finally {
        byteLength = 1;
      }
    }
  }();
  const onLength = (l) => {
    byteLength = l;
  };
  return decode16(varByteSource, {
    ...options ?? {},
    onLength
  });
};

// ../../../node_modules/p-defer/index.js
function pDefer() {
  const deferred = {};
  deferred.promise = new Promise((resolve, reject) => {
    deferred.resolve = resolve;
    deferred.reject = reject;
  });
  return deferred;
}

// ../../../node_modules/it-pushable/dist/src/fifo.js
var FixedFIFO = class {
  buffer;
  mask;
  top;
  btm;
  next;
  constructor(hwm) {
    if (!(hwm > 0) || (hwm - 1 & hwm) !== 0) {
      throw new Error("Max size for a FixedFIFO should be a power of two");
    }
    this.buffer = new Array(hwm);
    this.mask = hwm - 1;
    this.top = 0;
    this.btm = 0;
    this.next = null;
  }
  push(data) {
    if (this.buffer[this.top] !== void 0) {
      return false;
    }
    this.buffer[this.top] = data;
    this.top = this.top + 1 & this.mask;
    return true;
  }
  shift() {
    const last = this.buffer[this.btm];
    if (last === void 0) {
      return void 0;
    }
    this.buffer[this.btm] = void 0;
    this.btm = this.btm + 1 & this.mask;
    return last;
  }
  isEmpty() {
    return this.buffer[this.btm] === void 0;
  }
};
var FIFO = class {
  size;
  hwm;
  head;
  tail;
  constructor(options = {}) {
    this.hwm = options.splitLimit ?? 16;
    this.head = new FixedFIFO(this.hwm);
    this.tail = this.head;
    this.size = 0;
  }
  calculateSize(obj) {
    if (obj?.byteLength != null) {
      return obj.byteLength;
    }
    return 1;
  }
  push(val) {
    if (val?.value != null) {
      this.size += this.calculateSize(val.value);
    }
    if (!this.head.push(val)) {
      const prev = this.head;
      this.head = prev.next = new FixedFIFO(2 * this.head.buffer.length);
      this.head.push(val);
    }
  }
  shift() {
    let val = this.tail.shift();
    if (val === void 0 && this.tail.next != null) {
      const next = this.tail.next;
      this.tail.next = null;
      this.tail = next;
      val = this.tail.shift();
    }
    if (val?.value != null) {
      this.size -= this.calculateSize(val.value);
    }
    return val;
  }
  isEmpty() {
    return this.head.isEmpty();
  }
};

// ../../../node_modules/it-pushable/dist/src/index.js
var AbortError2 = class extends Error {
  type;
  code;
  constructor(message2, code5) {
    super(message2 ?? "The operation was aborted");
    this.type = "aborted";
    this.code = code5 ?? "ABORT_ERR";
  }
};
function pushable(options = {}) {
  const getNext = (buffer) => {
    const next = buffer.shift();
    if (next == null) {
      return { done: true };
    }
    if (next.error != null) {
      throw next.error;
    }
    return {
      done: next.done === true,
      // @ts-expect-error if done is false, value will be present
      value: next.value
    };
  };
  return _pushable(getNext, options);
}
function _pushable(getNext, options) {
  options = options ?? {};
  let onEnd = options.onEnd;
  let buffer = new FIFO();
  let pushable2;
  let onNext;
  let ended;
  let drain = pDefer();
  const waitNext = async () => {
    try {
      if (!buffer.isEmpty()) {
        return getNext(buffer);
      }
      if (ended) {
        return { done: true };
      }
      return await new Promise((resolve, reject) => {
        onNext = (next) => {
          onNext = null;
          buffer.push(next);
          try {
            resolve(getNext(buffer));
          } catch (err) {
            reject(err);
          }
          return pushable2;
        };
      });
    } finally {
      if (buffer.isEmpty()) {
        queueMicrotask(() => {
          drain.resolve();
          drain = pDefer();
        });
      }
    }
  };
  const bufferNext = (next) => {
    if (onNext != null) {
      return onNext(next);
    }
    buffer.push(next);
    return pushable2;
  };
  const bufferError = (err) => {
    buffer = new FIFO();
    if (onNext != null) {
      return onNext({ error: err });
    }
    buffer.push({ error: err });
    return pushable2;
  };
  const push = (value) => {
    if (ended) {
      return pushable2;
    }
    if (options?.objectMode !== true && value?.byteLength == null) {
      throw new Error("objectMode was not true but tried to push non-Uint8Array value");
    }
    return bufferNext({ done: false, value });
  };
  const end = (err) => {
    if (ended)
      return pushable2;
    ended = true;
    return err != null ? bufferError(err) : bufferNext({ done: true });
  };
  const _return = () => {
    buffer = new FIFO();
    end();
    return { done: true };
  };
  const _throw = (err) => {
    end(err);
    return { done: true };
  };
  pushable2 = {
    [Symbol.asyncIterator]() {
      return this;
    },
    next: waitNext,
    return: _return,
    throw: _throw,
    push,
    end,
    get readableLength() {
      return buffer.size;
    },
    onEmpty: async (options2) => {
      const signal = options2?.signal;
      signal?.throwIfAborted();
      if (buffer.isEmpty()) {
        return;
      }
      let cancel;
      let listener;
      if (signal != null) {
        cancel = new Promise((resolve, reject) => {
          listener = () => {
            reject(new AbortError2());
          };
          signal.addEventListener("abort", listener);
        });
      }
      try {
        await Promise.race([
          drain.promise,
          cancel
        ]);
      } finally {
        if (listener != null && signal != null) {
          signal?.removeEventListener("abort", listener);
        }
      }
    }
  };
  if (onEnd == null) {
    return pushable2;
  }
  const _pushable2 = pushable2;
  pushable2 = {
    [Symbol.asyncIterator]() {
      return this;
    },
    next() {
      return _pushable2.next();
    },
    throw(err) {
      _pushable2.throw(err);
      if (onEnd != null) {
        onEnd(err);
        onEnd = void 0;
      }
      return { done: true };
    },
    return() {
      _pushable2.return();
      if (onEnd != null) {
        onEnd();
        onEnd = void 0;
      }
      return { done: true };
    },
    push,
    end(err) {
      _pushable2.end(err);
      if (onEnd != null) {
        onEnd(err);
        onEnd = void 0;
      }
      return pushable2;
    },
    get readableLength() {
      return _pushable2.readableLength;
    }
  };
  return pushable2;
}

// ../../../node_modules/it-pb-stream/dist/src/index.js
var defaultLengthDecoder = (buf) => {
  return unsigned.decode(buf);
};
defaultLengthDecoder.bytes = 0;
function pbStream(duplex, opts) {
  const write = pushable();
  duplex.sink(write).catch((err) => {
    write.end(err);
  });
  duplex.sink = async (source2) => {
    for await (const buf of source2) {
      write.push(buf);
    }
    write.end();
  };
  let source = duplex.source;
  if (duplex.source[Symbol.iterator] != null) {
    source = duplex.source[Symbol.iterator]();
  } else if (duplex.source[Symbol.asyncIterator] != null) {
    source = duplex.source[Symbol.asyncIterator]();
  }
  const readBuffer = new Uint8ArrayList();
  const W = {
    read: async (bytes2) => {
      if (bytes2 == null) {
        const { done, value } = await source.next();
        if (done === true) {
          return new Uint8ArrayList();
        }
        return value;
      }
      while (readBuffer.byteLength < bytes2) {
        const { value, done } = await source.next();
        if (done === true) {
          throw (0, import_err_code2.default)(new Error("unexpected end of input"), "ERR_UNEXPECTED_EOF");
        }
        readBuffer.append(value);
      }
      const buf = readBuffer.sublist(0, bytes2);
      readBuffer.consume(bytes2);
      return buf;
    },
    readLP: async () => {
      let dataLength = -1;
      const lengthBuffer = new Uint8ArrayList();
      const decodeLength = opts?.lengthDecoder ?? defaultLengthDecoder;
      while (true) {
        lengthBuffer.append(await W.read(1));
        try {
          dataLength = decodeLength(lengthBuffer);
        } catch (err) {
          if (err instanceof RangeError) {
            continue;
          }
          throw err;
        }
        if (dataLength > -1) {
          break;
        }
        if (opts?.maxLengthLength != null && lengthBuffer.byteLength > opts.maxLengthLength) {
          throw (0, import_err_code2.default)(new Error("message length length too long"), "ERR_MSG_LENGTH_TOO_LONG");
        }
      }
      if (opts?.maxDataLength != null && dataLength > opts.maxDataLength) {
        throw (0, import_err_code2.default)(new Error("message length too long"), "ERR_MSG_DATA_TOO_LONG");
      }
      return W.read(dataLength);
    },
    readPB: async (proto) => {
      const value = await W.readLP();
      if (value == null) {
        throw new Error("Value is null");
      }
      const buf = value instanceof Uint8Array ? value : value.subarray();
      return proto.decode(buf);
    },
    write: (data) => {
      if (data instanceof Uint8Array) {
        write.push(data);
      } else {
        write.push(data.subarray());
      }
    },
    writeLP: (data) => {
      W.write(encode13.single(data, opts));
    },
    writePB: (data, proto) => {
      W.writeLP(proto.encode(data));
    },
    pb: (proto) => {
      return {
        read: async () => W.readPB(proto),
        write: (d) => {
          W.writePB(d, proto);
        },
        unwrap: () => W
      };
    },
    unwrap: () => {
      const originalStream = duplex.source;
      duplex.source = async function* () {
        yield* readBuffer;
        yield* originalStream;
      }();
      return duplex;
    }
  };
  return W;
}

// ../../../node_modules/any-signal/dist/src/index.js
function anySignal(signals) {
  const controller = new globalThis.AbortController();
  function onAbort() {
    controller.abort();
    for (const signal2 of signals) {
      if (signal2?.removeEventListener != null) {
        signal2.removeEventListener("abort", onAbort);
      }
    }
  }
  for (const signal2 of signals) {
    if (signal2?.aborted === true) {
      onAbort();
      break;
    }
    if (signal2?.addEventListener != null) {
      signal2.addEventListener("abort", onAbort);
    }
  }
  function clear() {
    for (const signal2 of signals) {
      if (signal2?.removeEventListener != null) {
        signal2.removeEventListener("abort", onAbort);
      }
    }
  }
  const signal = controller.signal;
  signal.clear = clear;
  return signal;
}

// ../../interface/dist/src/stream-muxer/stream.js
var log2 = () => {
};
log2.trace = () => {
};
log2.error = () => {
};
var ERR_STREAM_RESET = "ERR_STREAM_RESET";
var ERR_STREAM_ABORT = "ERR_STREAM_ABORT";
var ERR_SINK_ENDED = "ERR_SINK_ENDED";
var ERR_DOUBLE_SINK = "ERR_DOUBLE_SINK";
function isPromise(res) {
  return res != null && typeof res.then === "function";
}
var AbstractStream = class {
  id;
  stat;
  metadata;
  source;
  abortController;
  resetController;
  closeController;
  sourceEnded;
  sinkEnded;
  sinkSunk;
  endErr;
  streamSource;
  onEnd;
  maxDataSize;
  constructor(init) {
    this.abortController = new AbortController();
    this.resetController = new AbortController();
    this.closeController = new AbortController();
    this.sourceEnded = false;
    this.sinkEnded = false;
    this.sinkSunk = false;
    this.id = init.id;
    this.metadata = init.metadata ?? {};
    this.stat = {
      direction: init.direction,
      timeline: {
        open: Date.now()
      }
    };
    this.maxDataSize = init.maxDataSize;
    this.onEnd = init.onEnd;
    this.source = this.streamSource = pushable({
      onEnd: () => {
        if (this.stat.timeline.reset !== null) {
          const res = this.sendCloseRead();
          if (isPromise(res)) {
            res.catch((err) => {
              log2.error("error while sending close read", err);
            });
          }
        }
        this.onSourceEnd();
      }
    });
    this.sink = this.sink.bind(this);
  }
  onSourceEnd(err) {
    if (this.sourceEnded) {
      return;
    }
    this.stat.timeline.closeRead = Date.now();
    this.sourceEnded = true;
    log2.trace("%s stream %s source end - err: %o", this.stat.direction, this.id, err);
    if (err != null && this.endErr == null) {
      this.endErr = err;
    }
    if (this.sinkEnded) {
      this.stat.timeline.close = Date.now();
      if (this.onEnd != null) {
        this.onEnd(this.endErr);
      }
    }
  }
  onSinkEnd(err) {
    if (this.sinkEnded) {
      return;
    }
    this.stat.timeline.closeWrite = Date.now();
    this.sinkEnded = true;
    log2.trace("%s stream %s sink end - err: %o", this.stat.direction, this.id, err);
    if (err != null && this.endErr == null) {
      this.endErr = err;
    }
    if (this.sourceEnded) {
      this.stat.timeline.close = Date.now();
      if (this.onEnd != null) {
        this.onEnd(this.endErr);
      }
    }
  }
  // Close for both Reading and Writing
  close() {
    log2.trace("%s stream %s close", this.stat.direction, this.id);
    this.closeRead();
    this.closeWrite();
  }
  // Close for reading
  closeRead() {
    log2.trace("%s stream %s closeRead", this.stat.direction, this.id);
    if (this.sourceEnded) {
      return;
    }
    this.streamSource.end();
  }
  // Close for writing
  closeWrite() {
    log2.trace("%s stream %s closeWrite", this.stat.direction, this.id);
    if (this.sinkEnded) {
      return;
    }
    this.closeController.abort();
    try {
      const res = this.sendCloseWrite();
      if (isPromise(res)) {
        res.catch((err) => {
          log2.error("error while sending close write", err);
        });
      }
    } catch (err) {
      log2.trace("%s stream %s error sending close", this.stat.direction, this.id, err);
    }
    this.onSinkEnd();
  }
  // Close for reading and writing (local error)
  abort(err) {
    log2.trace("%s stream %s abort", this.stat.direction, this.id, err);
    this.streamSource.end(err);
    this.abortController.abort();
    this.onSinkEnd(err);
  }
  // Close immediately for reading and writing (remote error)
  reset() {
    const err = new CodeError("stream reset", ERR_STREAM_RESET);
    this.resetController.abort();
    this.streamSource.end(err);
    this.onSinkEnd(err);
  }
  async sink(source) {
    if (this.sinkSunk) {
      throw new CodeError("sink already called on stream", ERR_DOUBLE_SINK);
    }
    this.sinkSunk = true;
    if (this.sinkEnded) {
      throw new CodeError("stream closed for writing", ERR_SINK_ENDED);
    }
    const signal = anySignal([
      this.abortController.signal,
      this.resetController.signal,
      this.closeController.signal
    ]);
    try {
      source = abortableSource(source, signal);
      if (this.stat.direction === "outbound") {
        const res = this.sendNewStream();
        if (isPromise(res)) {
          await res;
        }
      }
      for await (let data of source) {
        while (data.length > 0) {
          if (data.length <= this.maxDataSize) {
            const res2 = this.sendData(data instanceof Uint8Array ? new Uint8ArrayList(data) : data);
            if (isPromise(res2)) {
              await res2;
            }
            break;
          }
          data = data instanceof Uint8Array ? new Uint8ArrayList(data) : data;
          const res = this.sendData(data.sublist(0, this.maxDataSize));
          if (isPromise(res)) {
            await res;
          }
          data.consume(this.maxDataSize);
        }
      }
    } catch (err) {
      if (err.type === "aborted" && err.message === "The operation was aborted") {
        if (this.closeController.signal.aborted) {
          return;
        }
        if (this.resetController.signal.aborted) {
          err.message = "stream reset";
          err.code = ERR_STREAM_RESET;
        }
        if (this.abortController.signal.aborted) {
          err.message = "stream aborted";
          err.code = ERR_STREAM_ABORT;
        }
      }
      if (err.code === ERR_STREAM_RESET) {
        log2.trace("%s stream %s reset", this.stat.direction, this.id);
      } else {
        log2.trace("%s stream %s error", this.stat.direction, this.id, err);
        try {
          const res = this.sendReset();
          if (isPromise(res)) {
            await res;
          }
          this.stat.timeline.reset = Date.now();
        } catch (err2) {
          log2.trace("%s stream %s error sending reset", this.stat.direction, this.id, err2);
        }
      }
      this.streamSource.end(err);
      this.onSinkEnd(err);
      throw err;
    } finally {
      signal.clear();
    }
    try {
      const res = this.sendCloseWrite();
      if (isPromise(res)) {
        await res;
      }
    } catch (err) {
      log2.trace("%s stream %s error sending close", this.stat.direction, this.id, err);
    }
    this.onSinkEnd();
  }
  /**
   * When an extending class reads data from it's implementation-specific source,
   * call this method to allow the stream consumer to read the data.
   */
  sourcePush(data) {
    this.streamSource.push(data);
  }
  /**
   * Returns the amount of unread data - can be used to prevent large amounts of
   * data building up when the stream consumer is too slow.
   */
  sourceReadableLength() {
    return this.streamSource.readableLength;
  }
};

// ../../../node_modules/p-timeout/index.js
var TimeoutError = class extends Error {
  constructor(message2) {
    super(message2);
    this.name = "TimeoutError";
  }
};
var AbortError3 = class extends Error {
  constructor(message2) {
    super();
    this.name = "AbortError";
    this.message = message2;
  }
};
var getDOMException = (errorMessage) => globalThis.DOMException === void 0 ? new AbortError3(errorMessage) : new DOMException(errorMessage);
var getAbortedReason = (signal) => {
  const reason = signal.reason === void 0 ? getDOMException("This operation was aborted.") : signal.reason;
  return reason instanceof Error ? reason : getDOMException(reason);
};
function pTimeout(promise, options) {
  const {
    milliseconds,
    fallback,
    message: message2,
    customTimers = { setTimeout, clearTimeout }
  } = options;
  let timer;
  const wrappedPromise = new Promise((resolve, reject) => {
    if (typeof milliseconds !== "number" || Math.sign(milliseconds) !== 1) {
      throw new TypeError(`Expected \`milliseconds\` to be a positive number, got \`${milliseconds}\``);
    }
    if (options.signal) {
      const { signal } = options;
      if (signal.aborted) {
        reject(getAbortedReason(signal));
      }
      signal.addEventListener("abort", () => {
        reject(getAbortedReason(signal));
      });
    }
    if (milliseconds === Number.POSITIVE_INFINITY) {
      promise.then(resolve, reject);
      return;
    }
    const timeoutError = new TimeoutError();
    timer = customTimers.setTimeout.call(void 0, () => {
      if (fallback) {
        try {
          resolve(fallback());
        } catch (error) {
          reject(error);
        }
        return;
      }
      if (typeof promise.cancel === "function") {
        promise.cancel();
      }
      if (message2 === false) {
        resolve();
      } else if (message2 instanceof Error) {
        reject(message2);
      } else {
        timeoutError.message = message2 ?? `Promise timed out after ${milliseconds} milliseconds`;
        reject(timeoutError);
      }
    }, milliseconds);
    (async () => {
      try {
        resolve(await promise);
      } catch (error) {
        reject(error);
      }
    })();
  });
  const cancelablePromise = wrappedPromise.finally(() => {
    cancelablePromise.clear();
  });
  cancelablePromise.clear = () => {
    customTimers.clearTimeout.call(void 0, timer);
    timer = void 0;
  };
  return cancelablePromise;
}

// ../../../node_modules/p-event/index.js
var normalizeEmitter = (emitter) => {
  const addListener = emitter.on || emitter.addListener || emitter.addEventListener;
  const removeListener = emitter.off || emitter.removeListener || emitter.removeEventListener;
  if (!addListener || !removeListener) {
    throw new TypeError("Emitter is not compatible");
  }
  return {
    addListener: addListener.bind(emitter),
    removeListener: removeListener.bind(emitter)
  };
};
function pEventMultiple(emitter, event, options) {
  let cancel;
  const returnValue = new Promise((resolve, reject) => {
    options = {
      rejectionEvents: ["error"],
      multiArgs: false,
      resolveImmediately: false,
      ...options
    };
    if (!(options.count >= 0 && (options.count === Number.POSITIVE_INFINITY || Number.isInteger(options.count)))) {
      throw new TypeError("The `count` option should be at least 0 or more");
    }
    options.signal?.throwIfAborted();
    const events = [event].flat();
    const items = [];
    const { addListener, removeListener } = normalizeEmitter(emitter);
    const onItem = (...arguments_) => {
      const value = options.multiArgs ? arguments_ : arguments_[0];
      if (options.filter && !options.filter(value)) {
        return;
      }
      items.push(value);
      if (options.count === items.length) {
        cancel();
        resolve(items);
      }
    };
    const rejectHandler = (error) => {
      cancel();
      reject(error);
    };
    cancel = () => {
      for (const event2 of events) {
        removeListener(event2, onItem);
      }
      for (const rejectionEvent of options.rejectionEvents) {
        removeListener(rejectionEvent, rejectHandler);
      }
    };
    for (const event2 of events) {
      addListener(event2, onItem);
    }
    for (const rejectionEvent of options.rejectionEvents) {
      addListener(rejectionEvent, rejectHandler);
    }
    if (options.signal) {
      options.signal.addEventListener("abort", () => {
        rejectHandler(options.signal.reason);
      }, { once: true });
    }
    if (options.resolveImmediately) {
      resolve(items);
    }
  });
  returnValue.cancel = cancel;
  if (typeof options.timeout === "number") {
    const timeout = pTimeout(returnValue, { milliseconds: options.timeout });
    timeout.cancel = cancel;
    return timeout;
  }
  return returnValue;
}
function pEvent(emitter, event, options) {
  if (typeof options === "function") {
    options = { filter: options };
  }
  options = {
    ...options,
    count: 1,
    resolveImmediately: false
  };
  const arrayPromise = pEventMultiple(emitter, event, options);
  const promise = arrayPromise.then((array) => array[0]);
  promise.cancel = arrayPromise.cancel;
  return promise;
}

// ../../../node_modules/protons-runtime/dist/src/utils.js
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

// ../../../node_modules/protons-runtime/dist/src/decode.js
function decodeMessage(buf, codec) {
  const r = reader(buf instanceof Uint8Array ? buf : buf.subarray());
  return codec.decode(r);
}

// ../../../node_modules/protons-runtime/dist/src/encode.js
function encodeMessage(message2, codec) {
  const w = writer();
  codec.encode(message2, w, {
    lengthDelimited: false
  });
  return w.finish();
}

// ../../../node_modules/protons-runtime/dist/src/codec.js
var CODEC_TYPES;
(function(CODEC_TYPES2) {
  CODEC_TYPES2[CODEC_TYPES2["VARINT"] = 0] = "VARINT";
  CODEC_TYPES2[CODEC_TYPES2["BIT64"] = 1] = "BIT64";
  CODEC_TYPES2[CODEC_TYPES2["LENGTH_DELIMITED"] = 2] = "LENGTH_DELIMITED";
  CODEC_TYPES2[CODEC_TYPES2["START_GROUP"] = 3] = "START_GROUP";
  CODEC_TYPES2[CODEC_TYPES2["END_GROUP"] = 4] = "END_GROUP";
  CODEC_TYPES2[CODEC_TYPES2["BIT32"] = 5] = "BIT32";
})(CODEC_TYPES || (CODEC_TYPES = {}));
function createCodec2(name5, type, encode19, decode24) {
  return {
    name: name5,
    type,
    encode: encode19,
    decode: decode24
  };
}

// ../../../node_modules/protons-runtime/dist/src/codecs/enum.js
function enumeration(v) {
  function findValue(val) {
    if (v[val.toString()] == null) {
      throw new Error("Invalid enum value");
    }
    return v[val];
  }
  const encode19 = function enumEncode(val, writer2) {
    const enumValue = findValue(val);
    writer2.int32(enumValue);
  };
  const decode24 = function enumDecode(reader2) {
    const val = reader2.int32();
    return findValue(val);
  };
  return createCodec2("enum", CODEC_TYPES.VARINT, encode19, decode24);
}

// ../../../node_modules/protons-runtime/dist/src/codecs/message.js
function message(encode19, decode24) {
  return createCodec2("message", CODEC_TYPES.LENGTH_DELIMITED, encode19, decode24);
}

// pb/message.ts
var Message;
((Message3) => {
  let Flag;
  ((Flag2) => {
    Flag2["FIN"] = "FIN";
    Flag2["STOP_SENDING"] = "STOP_SENDING";
    Flag2["RESET"] = "RESET";
  })(Flag = Message3.Flag || (Message3.Flag = {}));
  let __FlagValues;
  ((__FlagValues2) => {
    __FlagValues2[__FlagValues2["FIN"] = 0] = "FIN";
    __FlagValues2[__FlagValues2["STOP_SENDING"] = 1] = "STOP_SENDING";
    __FlagValues2[__FlagValues2["RESET"] = 2] = "RESET";
  })(__FlagValues || (__FlagValues = {}));
  ((Flag2) => {
    Flag2.codec = () => {
      return enumeration(__FlagValues);
    };
  })(Flag = Message3.Flag || (Message3.Flag = {}));
  let _codec;
  Message3.codec = () => {
    if (_codec == null) {
      _codec = message((obj, w, opts = {}) => {
        if (opts.lengthDelimited !== false) {
          w.fork();
        }
        if (obj.flag != null) {
          w.uint32(8);
          Message3.Flag.codec().encode(obj.flag, w);
        }
        if (obj.message != null) {
          w.uint32(18);
          w.bytes(obj.message);
        }
        if (opts.lengthDelimited !== false) {
          w.ldelim();
        }
      }, (reader2, length5) => {
        const obj = {};
        const end = length5 == null ? reader2.len : reader2.pos + length5;
        while (reader2.pos < end) {
          const tag = reader2.uint32();
          switch (tag >>> 3) {
            case 1:
              obj.flag = Message3.Flag.codec().decode(reader2);
              break;
            case 2:
              obj.message = reader2.bytes();
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
  Message3.encode = (obj) => {
    return encodeMessage(obj, Message3.codec());
  };
  Message3.decode = (buf) => {
    return decodeMessage(buf, Message3.codec());
  };
})(Message || (Message = {}));

// stream.ts
var log3 = logger("libp2p:webrtc:stream");
var MAX_MESSAGE_SIZE = 16 * 1024;
var MAX_BUFFERED_AMOUNT = 16 * 1024 * 1024;
var BUFFERED_AMOUNT_LOW_TIMEOUT = 30 * 1e3;
var PROTOBUF_OVERHEAD = 3;
var WebRTCStream = class extends AbstractStream {
  /**
   * The data channel used to send and receive data
   */
  channel;
  /**
   * Data channel options
   */
  dataChannelOptions;
  /**
   * push data from the underlying datachannel to the length prefix decoder
   * and then the protobuf decoder.
   */
  incomingData;
  messageQueue;
  constructor(init) {
    super(init);
    this.channel = init.channel;
    this.channel.binaryType = "arraybuffer";
    this.incomingData = pushable();
    this.messageQueue = new Uint8ArrayList();
    this.dataChannelOptions = {
      bufferedAmountLowEventTimeout: init.dataChannelOptions?.bufferedAmountLowEventTimeout ?? BUFFERED_AMOUNT_LOW_TIMEOUT,
      maxBufferedAmount: init.dataChannelOptions?.maxBufferedAmount ?? MAX_BUFFERED_AMOUNT,
      maxMessageSize: init.dataChannelOptions?.maxMessageSize ?? MAX_MESSAGE_SIZE
    };
    switch (this.channel.readyState) {
      case "open":
        break;
      case "closed":
      case "closing":
        if (this.stat.timeline.close === void 0 || this.stat.timeline.close === 0) {
          this.stat.timeline.close = Date.now();
        }
        break;
      case "connecting":
        break;
      default:
        log3.error("unknown datachannel state %s", this.channel.readyState);
        throw new CodeError("Unknown datachannel state", "ERR_INVALID_STATE");
    }
    this.channel.onopen = (_evt) => {
      this.stat.timeline.open = (/* @__PURE__ */ new Date()).getTime();
      if (this.messageQueue != null) {
        this._sendMessage(this.messageQueue).catch((err) => {
          this.abort(err);
        });
        this.messageQueue = void 0;
      }
    };
    this.channel.onclose = (_evt) => {
      this.close();
    };
    this.channel.onerror = (evt) => {
      const err = evt.error;
      this.abort(err);
    };
    const self2 = this;
    this.channel.onmessage = async (event) => {
      const { data } = event;
      if (data === null || data.byteLength === 0) {
        return;
      }
      this.incomingData.push(new Uint8Array(data, 0, data.byteLength));
    };
    Promise.resolve().then(async () => {
      for await (const buf of decode16(this.incomingData)) {
        const message2 = self2.processIncomingProtobuf(buf.subarray());
        if (message2 != null) {
          self2.sourcePush(new Uint8ArrayList(message2));
        }
      }
    }).catch((err) => {
      log3.error("error processing incoming data channel messages", err);
    });
  }
  sendNewStream() {
  }
  async _sendMessage(data, checkBuffer = true) {
    if (checkBuffer && this.channel.bufferedAmount > this.dataChannelOptions.maxBufferedAmount) {
      try {
        await pEvent(this.channel, "bufferedamountlow", { timeout: this.dataChannelOptions.bufferedAmountLowEventTimeout });
      } catch (err) {
        if (err instanceof TimeoutError) {
          this.abort(err);
          throw new Error("Timed out waiting for DataChannel buffer to clear");
        }
        throw err;
      }
    }
    if (this.channel.readyState === "closed" || this.channel.readyState === "closing") {
      throw new CodeError("Invalid datachannel state - closed or closing", "ERR_INVALID_STATE");
    }
    if (this.channel.readyState === "open") {
      for (const buf of data) {
        this.channel.send(buf);
      }
    } else if (this.channel.readyState === "connecting") {
      if (this.messageQueue == null) {
        this.messageQueue = new Uint8ArrayList();
      }
      this.messageQueue.append(data);
    } else {
      log3.error("unknown datachannel state %s", this.channel.readyState);
      throw new CodeError("Unknown datachannel state", "ERR_INVALID_STATE");
    }
  }
  async sendData(data) {
    const msgbuf = Message.encode({ message: data.subarray() });
    const sendbuf = encode13.single(msgbuf);
    await this._sendMessage(sendbuf);
  }
  async sendReset() {
    await this._sendFlag(Message.Flag.RESET);
  }
  async sendCloseWrite() {
    await this._sendFlag(Message.Flag.FIN);
  }
  async sendCloseRead() {
    await this._sendFlag(Message.Flag.STOP_SENDING);
  }
  /**
   * Handle incoming
   */
  processIncomingProtobuf(buffer) {
    const message2 = Message.decode(buffer);
    if (message2.flag !== void 0) {
      if (message2.flag === Message.Flag.FIN) {
        this.incomingData.end();
        this.closeRead();
      }
      if (message2.flag === Message.Flag.RESET) {
        this.reset();
      }
      if (message2.flag === Message.Flag.STOP_SENDING) {
        this.closeWrite();
      }
    }
    return message2.message;
  }
  async _sendFlag(flag) {
    log3.trace("Sending flag: %s", flag.toString());
    const msgbuf = Message.encode({ flag });
    const prefixedBuf = encode13.single(msgbuf);
    await this._sendMessage(prefixedBuf, false);
  }
};
function createStream(options) {
  const { channel, direction, onEnd, dataChannelOptions } = options;
  return new WebRTCStream({
    id: direction === "inbound" ? `i${channel.id}` : `r${channel.id}`,
    direction,
    maxDataSize: (dataChannelOptions?.maxMessageSize ?? MAX_MESSAGE_SIZE) - PROTOBUF_OVERHEAD,
    dataChannelOptions,
    onEnd,
    channel
  });
}

// muxer.ts
var PROTOCOL = "/webrtc";
var DataChannelMuxerFactory = class {
  protocol;
  /**
   * WebRTC Peer Connection
   */
  peerConnection;
  streamBuffer = [];
  metrics;
  dataChannelOptions;
  constructor(init) {
    this.peerConnection = init.peerConnection;
    this.metrics = init.metrics;
    this.protocol = init.protocol ?? PROTOCOL;
    this.dataChannelOptions = init.dataChannelOptions;
    this.peerConnection.ondatachannel = ({ channel }) => {
      const stream = createStream({
        channel,
        direction: "inbound",
        dataChannelOptions: init.dataChannelOptions,
        onEnd: () => {
          this.streamBuffer = this.streamBuffer.filter((s) => s.id !== stream.id);
        }
      });
      this.streamBuffer.push(stream);
    };
  }
  createStreamMuxer(init) {
    return new DataChannelMuxer({
      ...init,
      peerConnection: this.peerConnection,
      dataChannelOptions: this.dataChannelOptions,
      metrics: this.metrics,
      streams: this.streamBuffer,
      protocol: this.protocol
    });
  }
};
var DataChannelMuxer = class {
  constructor(init) {
    this.init = init;
    this.streams = init.streams;
    this.peerConnection = init.peerConnection;
    this.protocol = init.protocol ?? PROTOCOL;
    this.metrics = init.metrics;
    this.peerConnection.ondatachannel = ({ channel }) => {
      const stream = createStream({
        channel,
        direction: "inbound",
        dataChannelOptions: this.dataChannelOptions,
        onEnd: () => {
          this.streams = this.streams.filter((s) => s.id !== stream.id);
          this.metrics?.increment({ stream_end: true });
          init?.onStreamEnd?.(stream);
        }
      });
      this.streams.push(stream);
      if (init?.onIncomingStream != null) {
        this.metrics?.increment({ incoming_stream: true });
        init.onIncomingStream(stream);
      }
    };
    const onIncomingStream = init?.onIncomingStream;
    if (onIncomingStream != null) {
      this.streams.forEach((s) => {
        onIncomingStream(s);
      });
    }
  }
  /**
   * Array of streams in the data channel
   */
  streams;
  protocol;
  peerConnection;
  dataChannelOptions;
  metrics;
  /**
   * Close or abort all tracked streams and stop the muxer
   */
  close = () => {
  };
  /**
   * The stream source, a no-op as the transport natively supports multiplexing
   */
  source = nopSource();
  /**
   * The stream destination, a no-op as the transport natively supports multiplexing
   */
  sink = nopSink;
  newStream() {
    const channel = this.peerConnection.createDataChannel("");
    const stream = createStream({
      channel,
      direction: "outbound",
      dataChannelOptions: this.dataChannelOptions,
      onEnd: () => {
        this.streams = this.streams.filter((s) => s.id !== stream.id);
        this.metrics?.increment({ stream_end: true });
        this.init?.onStreamEnd?.(stream);
      }
    });
    this.streams.push(stream);
    this.metrics?.increment({ outgoing_stream: true });
    return stream;
  }
};

// private-to-private/pb/message.ts
var Message2;
((Message3) => {
  let Type;
  ((Type2) => {
    Type2["SDP_OFFER"] = "SDP_OFFER";
    Type2["SDP_ANSWER"] = "SDP_ANSWER";
    Type2["ICE_CANDIDATE"] = "ICE_CANDIDATE";
  })(Type = Message3.Type || (Message3.Type = {}));
  let __TypeValues;
  ((__TypeValues2) => {
    __TypeValues2[__TypeValues2["SDP_OFFER"] = 0] = "SDP_OFFER";
    __TypeValues2[__TypeValues2["SDP_ANSWER"] = 1] = "SDP_ANSWER";
    __TypeValues2[__TypeValues2["ICE_CANDIDATE"] = 2] = "ICE_CANDIDATE";
  })(__TypeValues || (__TypeValues = {}));
  ((Type2) => {
    Type2.codec = () => {
      return enumeration(__TypeValues);
    };
  })(Type = Message3.Type || (Message3.Type = {}));
  let _codec;
  Message3.codec = () => {
    if (_codec == null) {
      _codec = message((obj, w, opts = {}) => {
        if (opts.lengthDelimited !== false) {
          w.fork();
        }
        if (obj.type != null) {
          w.uint32(8);
          Message3.Type.codec().encode(obj.type, w);
        }
        if (obj.data != null) {
          w.uint32(18);
          w.string(obj.data);
        }
        if (opts.lengthDelimited !== false) {
          w.ldelim();
        }
      }, (reader2, length5) => {
        const obj = {};
        const end = length5 == null ? reader2.len : reader2.pos + length5;
        while (reader2.pos < end) {
          const tag = reader2.uint32();
          switch (tag >>> 3) {
            case 1:
              obj.type = Message3.Type.codec().decode(reader2);
              break;
            case 2:
              obj.data = reader2.string();
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
  Message3.encode = (obj) => {
    return encodeMessage(obj, Message3.codec());
  };
  Message3.decode = (buf) => {
    return decodeMessage(buf, Message3.codec());
  };
})(Message2 || (Message2 = {}));

// private-to-private/util.ts
var log4 = logger("libp2p:webrtc:peer:util");
var readCandidatesUntilConnected = async (connectedPromise, pc, stream) => {
  while (true) {
    const readResult = await Promise.race([connectedPromise.promise, stream.read()]);
    if (readResult instanceof Object) {
      const message2 = readResult;
      if (message2.type !== Message2.Type.ICE_CANDIDATE) {
        throw new Error("expected only ice candidates");
      }
      if (message2.data == null || message2.data === "") {
        log4.trace("end-of-candidates received");
        break;
      }
      log4.trace("received new ICE candidate: %s", message2.data);
      try {
        await pc.addIceCandidate(new RTCIceCandidate(JSON.parse(message2.data)));
      } catch (err) {
        log4.error("bad candidate received: ", err);
        throw new Error("bad candidate received");
      }
    } else {
      break;
    }
  }
  await connectedPromise.promise;
};
function resolveOnConnected(pc, promise) {
  pc[isFirefox ? "oniceconnectionstatechange" : "onconnectionstatechange"] = (_) => {
    log4.trace("receiver peerConnectionState state: ", pc.connectionState);
    switch (isFirefox ? pc.iceConnectionState : pc.connectionState) {
      case "connected":
        promise.resolve();
        break;
      case "failed":
      case "disconnected":
      case "closed":
        promise.reject(new Error("RTCPeerConnection was closed"));
        break;
      default:
        break;
    }
  };
}

// private-to-private/handler.ts
var DEFAULT_TIMEOUT = 30 * 1e3;
var log5 = logger("libp2p:webrtc:peer");
async function handleIncomingStream({ rtcConfiguration, dataChannelOptions, stream: rawStream }) {
  const signal = AbortSignal.timeout(DEFAULT_TIMEOUT);
  const stream = pbStream(abortableDuplex(rawStream, signal)).pb(Message2);
  const pc = new RTCPeerConnection(rtcConfiguration);
  const muxerFactory = new DataChannelMuxerFactory({ peerConnection: pc, dataChannelOptions });
  const connectedPromise = pDefer();
  const answerSentPromise = pDefer();
  signal.onabort = () => {
    connectedPromise.reject();
  };
  pc.onicecandidate = ({ candidate }) => {
    answerSentPromise.promise.then(
      () => {
        stream.write({
          type: Message2.Type.ICE_CANDIDATE,
          data: candidate != null ? JSON.stringify(candidate.toJSON()) : ""
        });
      },
      (err) => {
        log5.error("cannot set candidate since sending answer failed", err);
      }
    );
  };
  resolveOnConnected(pc, connectedPromise);
  const pbOffer = await stream.read();
  if (pbOffer.type !== Message2.Type.SDP_OFFER) {
    throw new Error(`expected message type SDP_OFFER, received: ${pbOffer.type ?? "undefined"} `);
  }
  const offer = new RTCSessionDescription({
    type: "offer",
    sdp: pbOffer.data
  });
  await pc.setRemoteDescription(offer).catch((err) => {
    log5.error("could not execute setRemoteDescription", err);
    throw new Error("Failed to set remoteDescription");
  });
  const answer = await pc.createAnswer().catch((err) => {
    log5.error("could not execute createAnswer", err);
    answerSentPromise.reject(err);
    throw new Error("Failed to create answer");
  });
  stream.write({ type: Message2.Type.SDP_ANSWER, data: answer.sdp });
  await pc.setLocalDescription(answer).catch((err) => {
    log5.error("could not execute setLocalDescription", err);
    answerSentPromise.reject(err);
    throw new Error("Failed to set localDescription");
  });
  answerSentPromise.resolve();
  await readCandidatesUntilConnected(connectedPromise, pc, stream);
  const remoteAddress = parseRemoteAddress(pc.currentRemoteDescription?.sdp ?? "");
  return { pc, muxerFactory, remoteAddress };
}
async function initiateConnection({ rtcConfiguration, dataChannelOptions, signal, stream: rawStream }) {
  const stream = pbStream(abortableDuplex(rawStream, signal)).pb(Message2);
  const pc = new RTCPeerConnection(rtcConfiguration);
  const muxerFactory = new DataChannelMuxerFactory({ peerConnection: pc, dataChannelOptions });
  const connectedPromise = pDefer();
  resolveOnConnected(pc, connectedPromise);
  signal.onabort = connectedPromise.reject;
  const channel = pc.createDataChannel("init");
  pc.onicecandidate = ({ candidate }) => {
    stream.write({
      type: Message2.Type.ICE_CANDIDATE,
      data: candidate != null ? JSON.stringify(candidate.toJSON()) : ""
    });
  };
  const offerSdp = await pc.createOffer();
  stream.write({ type: Message2.Type.SDP_OFFER, data: offerSdp.sdp });
  await pc.setLocalDescription(offerSdp).catch((err) => {
    log5.error("could not execute setLocalDescription", err);
    throw new Error("Failed to set localDescription");
  });
  const answerMessage = await stream.read();
  if (answerMessage.type !== Message2.Type.SDP_ANSWER) {
    throw new Error("remote should send an SDP answer");
  }
  const answerSdp = new RTCSessionDescription({ type: "answer", sdp: answerMessage.data });
  await pc.setRemoteDescription(answerSdp).catch((err) => {
    log5.error("could not execute setRemoteDescription", err);
    throw new Error("Failed to set remoteDescription");
  });
  await readCandidatesUntilConnected(connectedPromise, pc, stream);
  channel.close();
  const remoteAddress = parseRemoteAddress(pc.currentRemoteDescription?.sdp ?? "");
  return { pc, muxerFactory, remoteAddress };
}
function parseRemoteAddress(sdp) {
  const candidateLine = sdp.split("\r\n").filter((line) => line.startsWith("a=candidate")).pop();
  const candidateParts = candidateLine?.split(" ");
  if (candidateLine == null || candidateParts == null || candidateParts.length < 5) {
    log5("could not parse remote address from", candidateLine);
    return "/webrtc";
  }
  return `/dnsaddr/${candidateParts[4]}/${candidateParts[2].toLowerCase()}/${candidateParts[3]}/webrtc`;
}

// ../../interface/dist/src/events.js
var EventEmitter = class extends EventTarget {
  #listeners = /* @__PURE__ */ new Map();
  listenerCount(type) {
    const listeners = this.#listeners.get(type);
    if (listeners == null) {
      return 0;
    }
    return listeners.length;
  }
  addEventListener(type, listener, options) {
    super.addEventListener(type, listener, options);
    let list = this.#listeners.get(type);
    if (list == null) {
      list = [];
      this.#listeners.set(type, list);
    }
    list.push({
      callback: listener,
      once: (options !== true && options !== false && options?.once) ?? false
    });
  }
  removeEventListener(type, listener, options) {
    super.removeEventListener(type.toString(), listener ?? null, options);
    let list = this.#listeners.get(type);
    if (list == null) {
      return;
    }
    list = list.filter(({ callback }) => callback !== listener);
    this.#listeners.set(type, list);
  }
  dispatchEvent(event) {
    const result = super.dispatchEvent(event);
    let list = this.#listeners.get(event.type);
    if (list == null) {
      return result;
    }
    list = list.filter(({ once }) => !once);
    this.#listeners.set(event.type, list);
    return result;
  }
  safeDispatchEvent(type, detail) {
    return this.dispatchEvent(new CustomEvent(type, detail));
  }
};
var CustomEventPolyfill = class extends Event {
  /** Returns any custom data event was created with. Typically used for synthetic events. */
  detail;
  constructor(message2, data) {
    super(message2, data);
    this.detail = data?.detail;
  }
};
var CustomEvent = globalThis.CustomEvent ?? CustomEventPolyfill;

// ../../../node_modules/@multiformats/mafmt/dist/src/index.js
var DNS4 = base5("dns4");
var DNS6 = base5("dns6");
var DNSADDR = base5("dnsaddr");
var DNS = or4(base5("dns"), DNSADDR, DNS4, DNS6);
var IP = or4(base5("ip4"), base5("ip6"));
var TCP = or4(and(IP, base5("tcp")), and(DNS, base5("tcp")));
var UDP = and(IP, base5("udp"));
var UTP = and(UDP, base5("utp"));
var QUIC = and(UDP, base5("quic"));
var QUICV1 = and(UDP, base5("quic-v1"));
var _WebSockets = or4(and(TCP, base5("ws")), and(DNS, base5("ws")));
var WebSockets = or4(and(_WebSockets, base5("p2p")), _WebSockets);
var _WebSocketsSecure = or4(and(TCP, base5("wss")), and(DNS, base5("wss")), and(TCP, base5("tls"), base5("ws")), and(DNS, base5("tls"), base5("ws")));
var WebSocketsSecure = or4(and(_WebSocketsSecure, base5("p2p")), _WebSocketsSecure);
var HTTP = or4(and(TCP, base5("http")), and(IP, base5("http")), and(DNS, base5("http")));
var HTTPS = or4(and(TCP, base5("https")), and(IP, base5("https")), and(DNS, base5("https")));
var _WebRTCDirect = and(UDP, base5("webrtc-direct"), base5("certhash"));
var WebRTCDirect = or4(and(_WebRTCDirect, base5("p2p")), _WebRTCDirect);
var _WebTransport = and(QUICV1, base5("webtransport"), base5("certhash"), base5("certhash"));
var WebTransport = or4(and(_WebTransport, base5("p2p")), _WebTransport);
var P2PWebRTCStar = or4(and(WebSockets, base5("p2p-webrtc-star"), base5("p2p")), and(WebSocketsSecure, base5("p2p-webrtc-star"), base5("p2p")), and(WebSockets, base5("p2p-webrtc-star")), and(WebSocketsSecure, base5("p2p-webrtc-star")));
var WebSocketStar = or4(and(WebSockets, base5("p2p-websocket-star"), base5("p2p")), and(WebSocketsSecure, base5("p2p-websocket-star"), base5("p2p")), and(WebSockets, base5("p2p-websocket-star")), and(WebSocketsSecure, base5("p2p-websocket-star")));
var P2PWebRTCDirect = or4(and(HTTP, base5("p2p-webrtc-direct"), base5("p2p")), and(HTTPS, base5("p2p-webrtc-direct"), base5("p2p")), and(HTTP, base5("p2p-webrtc-direct")), and(HTTPS, base5("p2p-webrtc-direct")));
var Reliable = or4(_WebSockets, _WebSocketsSecure, HTTP, HTTPS, P2PWebRTCStar, P2PWebRTCDirect, TCP, UTP, QUIC, DNS, WebRTCDirect, WebTransport);
var Stardust = or4(and(Reliable, base5("p2p-stardust"), base5("p2p")), and(Reliable, base5("p2p-stardust")));
var _P2P = or4(and(Reliable, base5("p2p")), P2PWebRTCStar, P2PWebRTCDirect, WebRTCDirect, WebTransport, base5("p2p"));
var _Circuit = or4(and(_P2P, base5("p2p-circuit"), _P2P), and(_P2P, base5("p2p-circuit")), and(base5("p2p-circuit"), _P2P), and(Reliable, base5("p2p-circuit")), and(base5("p2p-circuit"), Reliable), base5("p2p-circuit"));
var CircuitRecursive = () => or4(and(_Circuit, CircuitRecursive), _Circuit);
var Circuit = CircuitRecursive();
var P2P = or4(and(Circuit, _P2P, Circuit), and(_P2P, Circuit), and(Circuit, _P2P), Circuit, _P2P);
var WebRTC = or4(and(Circuit, base5("webrtc"), base5("p2p")), and(Circuit, base5("webrtc")), and(Reliable, base5("webrtc"), base5("p2p")), and(Reliable, base5("webrtc")), base5("webrtc"));
function makeMatchesFunction(partialMatch) {
  function matches(a) {
    let ma;
    try {
      ma = multiaddr(a);
    } catch (err) {
      return false;
    }
    const out = partialMatch(ma.protoNames());
    if (out === null) {
      return false;
    }
    if (out === true || out === false) {
      return out;
    }
    return out.length === 0;
  }
  return matches;
}
function and(...args) {
  function partialMatch(a) {
    if (a.length < args.length) {
      return null;
    }
    let out = a;
    args.some((arg) => {
      out = typeof arg === "function" ? arg().partialMatch(a) : arg.partialMatch(a);
      if (Array.isArray(out)) {
        a = out;
      }
      if (out === null) {
        return true;
      }
      return false;
    });
    return out;
  }
  return {
    toString: function() {
      return "{ " + args.join(" ") + " }";
    },
    input: args,
    matches: makeMatchesFunction(partialMatch),
    partialMatch
  };
}
function or4(...args) {
  function partialMatch(a) {
    let out = null;
    args.some((arg) => {
      const res = typeof arg === "function" ? arg().partialMatch(a) : arg.partialMatch(a);
      if (res != null) {
        out = res;
        return true;
      }
      return false;
    });
    return out;
  }
  const result = {
    toString: function() {
      return "{ " + args.join(" ") + " }";
    },
    input: args,
    matches: makeMatchesFunction(partialMatch),
    partialMatch
  };
  return result;
}
function base5(n) {
  const name5 = n;
  function matches(a) {
    let ma;
    try {
      ma = multiaddr(a);
    } catch (err) {
      return false;
    }
    const pnames = ma.protoNames();
    if (pnames.length === 1 && pnames[0] === name5) {
      return true;
    }
    return false;
  }
  function partialMatch(protos) {
    if (protos.length === 0) {
      return null;
    }
    if (protos[0] === name5) {
      return protos.slice(1);
    }
    return null;
  }
  return {
    toString: function() {
      return name5;
    },
    matches,
    partialMatch
  };
}

// private-to-private/listener.ts
var WebRTCPeerListener = class extends EventEmitter {
  peerId;
  transportManager;
  constructor(opts) {
    super();
    this.peerId = opts.peerId;
    this.transportManager = opts.transportManager;
  }
  async listen() {
    this.safeDispatchEvent("listening", {});
  }
  getAddrs() {
    return this.transportManager.getListeners().filter((l) => l !== this).map(
      (l) => l.getAddrs().filter((ma) => Circuit.matches(ma)).map((ma) => {
        return ma.encapsulate(`/webrtc/p2p/${this.peerId}`);
      })
    ).flat();
  }
  async close() {
    this.safeDispatchEvent("close", {});
  }
};

// private-to-private/transport.ts
var log6 = logger("libp2p:webrtc:peer");
var WEBRTC_TRANSPORT = "/webrtc";
var CIRCUIT_RELAY_TRANSPORT = "/p2p-circuit";
var SIGNALING_PROTO_ID = "/webrtc-signaling/0.0.1";
var WEBRTC_CODE = getProtocol("webrtc").code;
var WebRTCTransport = class {
  constructor(components, init = {}) {
    this.components = components;
    this.init = init;
  }
  _started = false;
  isStarted() {
    return this._started;
  }
  async start() {
    await this.components.registrar.handle(SIGNALING_PROTO_ID, (data) => {
      this._onProtocol(data).catch((err) => {
        log6.error("failed to handle incoming connect from %p", data.connection.remotePeer, err);
      });
    });
    this._started = true;
  }
  async stop() {
    await this.components.registrar.unhandle(SIGNALING_PROTO_ID);
    this._started = false;
  }
  createListener(options) {
    return new WebRTCPeerListener(this.components);
  }
  [Symbol.toStringTag] = "@libp2p/webrtc";
  [symbol] = true;
  filter(multiaddrs) {
    return multiaddrs.filter((ma) => {
      const codes3 = ma.protoCodes();
      return codes3.includes(WEBRTC_CODE);
    });
  }
  /*
   * dial connects to a remote via the circuit relay or any other protocol
   * and proceeds to upgrade to a webrtc connection.
   * multiaddr of the form: <multiaddr>/webrtc/p2p/<destination-peer>
   * For a circuit relay, this will be of the form
   * <relay address>/p2p/<relay-peer>/p2p-circuit/webrtc/p2p/<destination-peer>
  */
  async dial(ma, options) {
    log6.trace("dialing address: ", ma);
    const { baseAddr, peerId } = splitAddr(ma);
    if (options.signal == null) {
      const controller = new AbortController();
      options.signal = controller.signal;
    }
    const connection = await this.components.transportManager.dial(baseAddr, options);
    const signalingStream = await connection.newStream([SIGNALING_PROTO_ID], options);
    try {
      const { pc, muxerFactory, remoteAddress } = await initiateConnection({
        stream: signalingStream,
        rtcConfiguration: this.init.rtcConfiguration,
        dataChannelOptions: this.init.dataChannel,
        signal: options.signal
      });
      const result = await options.upgrader.upgradeOutbound(
        new WebRTCMultiaddrConnection({
          peerConnection: pc,
          timeline: { open: Date.now() },
          remoteAddr: multiaddr(remoteAddress).encapsulate(`/p2p/${peerId.toString()}`)
        }),
        {
          skipProtection: true,
          skipEncryption: true,
          muxerFactory
        }
      );
      signalingStream.close();
      return result;
    } catch (err) {
      signalingStream.reset();
      throw err;
    } finally {
      await connection.close();
    }
  }
  async _onProtocol({ connection, stream }) {
    try {
      const { pc, muxerFactory, remoteAddress } = await handleIncomingStream({
        rtcConfiguration: this.init.rtcConfiguration,
        connection,
        stream,
        dataChannelOptions: this.init.dataChannel
      });
      await this.components.upgrader.upgradeInbound(new WebRTCMultiaddrConnection({
        peerConnection: pc,
        timeline: { open: (/* @__PURE__ */ new Date()).getTime() },
        remoteAddr: multiaddr(remoteAddress).encapsulate(`/p2p/${connection.remotePeer.toString()}`)
      }), {
        skipEncryption: true,
        skipProtection: true,
        muxerFactory
      });
    } catch (err) {
      stream.reset();
      throw err;
    } finally {
      await connection.close();
    }
  }
};
function splitAddr(ma) {
  const addrs = ma.toString().split(WEBRTC_TRANSPORT + "/");
  if (addrs.length !== 2) {
    throw new CodeError("webrtc protocol was not present in multiaddr", "ERR_INVALID_MULTIADDR" /* ERR_INVALID_MULTIADDR */);
  }
  if (!addrs[0].includes(CIRCUIT_RELAY_TRANSPORT)) {
    throw new CodeError("p2p-circuit protocol was not present in multiaddr", "ERR_INVALID_MULTIADDR" /* ERR_INVALID_MULTIADDR */);
  }
  let remoteAddr = multiaddr(addrs[0]);
  const destination = multiaddr("/" + addrs[1]);
  const destinationIdString = destination.getPeerId();
  if (destinationIdString == null) {
    throw new CodeError("destination peer id was missing", "ERR_INVALID_MULTIADDR" /* ERR_INVALID_MULTIADDR */);
  }
  const lastProtoInRemote = remoteAddr.protos().pop();
  if (lastProtoInRemote === void 0) {
    throw new CodeError("invalid multiaddr", "ERR_INVALID_MULTIADDR" /* ERR_INVALID_MULTIADDR */);
  }
  if (lastProtoInRemote.name !== "p2p") {
    remoteAddr = remoteAddr.encapsulate(`/p2p/${destinationIdString}`);
  }
  return { baseAddr: remoteAddr, peerId: peerIdFromString(destinationIdString) };
}

// ../../../node_modules/it-pair/dist/src/index.js
function pair() {
  const deferred = pDefer();
  let piped = false;
  return {
    sink: async (source) => {
      if (piped) {
        throw new Error("already piped");
      }
      piped = true;
      deferred.resolve(source);
    },
    source: async function* () {
      const source = await deferred.promise;
      yield* source;
    }()
  };
}

// ../../../node_modules/it-pair/dist/src/duplex.js
function duplexPair() {
  const a = pair();
  const b = pair();
  return [
    {
      source: a.source,
      sink: b.sink
    },
    {
      source: b.source,
      sink: a.sink
    }
  ];
}

// ../../../node_modules/it-merge/dist/src/index.js
function isAsyncIterable2(thing) {
  return thing[Symbol.asyncIterator] != null;
}
function merge(...sources) {
  const syncSources = [];
  for (const source of sources) {
    if (!isAsyncIterable2(source)) {
      syncSources.push(source);
    }
  }
  if (syncSources.length === sources.length) {
    return function* () {
      for (const source of syncSources) {
        yield* source;
      }
    }();
  }
  return async function* () {
    const output2 = pushable({
      objectMode: true
    });
    void Promise.resolve().then(async () => {
      try {
        await Promise.all(sources.map(async (source) => {
          for await (const item of source) {
            output2.push(item);
          }
        }));
        output2.end();
      } catch (err) {
        output2.end(err);
      }
    });
    yield* output2;
  }();
}
var src_default = merge;

// ../../../node_modules/it-pipe/dist/src/index.js
function pipe(first, ...rest) {
  if (first == null) {
    throw new Error("Empty pipeline");
  }
  if (isDuplex(first)) {
    const duplex = first;
    first = () => duplex.source;
  } else if (isIterable(first) || isAsyncIterable3(first)) {
    const source = first;
    first = () => source;
  }
  const fns = [first, ...rest];
  if (fns.length > 1) {
    if (isDuplex(fns[fns.length - 1])) {
      fns[fns.length - 1] = fns[fns.length - 1].sink;
    }
  }
  if (fns.length > 2) {
    for (let i = 1; i < fns.length - 1; i++) {
      if (isDuplex(fns[i])) {
        fns[i] = duplexPipelineFn(fns[i]);
      }
    }
  }
  return rawPipe(...fns);
}
var rawPipe = (...fns) => {
  let res;
  while (fns.length > 0) {
    res = fns.shift()(res);
  }
  return res;
};
var isAsyncIterable3 = (obj) => {
  return obj?.[Symbol.asyncIterator] != null;
};
var isIterable = (obj) => {
  return obj?.[Symbol.iterator] != null;
};
var isDuplex = (obj) => {
  if (obj == null) {
    return false;
  }
  return obj.sink != null && obj.source != null;
};
var duplexPipelineFn = (duplex) => {
  return (source) => {
    const p = duplex.sink(source);
    if (p?.then != null) {
      const stream = pushable({
        objectMode: true
      });
      p.then(() => {
        stream.end();
      }, (err) => {
        stream.end(err);
      });
      let sourceWrap;
      const source2 = duplex.source;
      if (isAsyncIterable3(source2)) {
        sourceWrap = async function* () {
          yield* source2;
          stream.end();
        };
      } else if (isIterable(source2)) {
        sourceWrap = function* () {
          yield* source2;
          stream.end();
        };
      } else {
        throw new Error("Unknown duplex source type - must be Iterable or AsyncIterable");
      }
      return src_default(stream, sourceWrap());
    }
    return duplex.source;
  };
};

// ../../../node_modules/@chainsafe/libp2p-noise/dist/src/constants.js
var NOISE_MSG_MAX_LENGTH_BYTES = 65535;
var NOISE_MSG_MAX_LENGTH_BYTES_WITHOUT_TAG = NOISE_MSG_MAX_LENGTH_BYTES - 16;
var DUMP_SESSION_KEYS = Boolean(globalThis.process?.env?.DUMP_SESSION_KEYS);

// ../../../node_modules/@noble/hashes/esm/_assert.js
function number(n) {
  if (!Number.isSafeInteger(n) || n < 0)
    throw new Error(`Wrong positive integer: ${n}`);
}
function bool(b) {
  if (typeof b !== "boolean")
    throw new Error(`Expected boolean, not ${b}`);
}
function bytes(b, ...lengths) {
  if (!(b instanceof Uint8Array))
    throw new Error("Expected Uint8Array");
  if (lengths.length > 0 && !lengths.includes(b.length))
    throw new Error(`Expected Uint8Array of length ${lengths}, not of length=${b.length}`);
}
function hash(hash2) {
  if (typeof hash2 !== "function" || typeof hash2.create !== "function")
    throw new Error("Hash should be wrapped by utils.wrapConstructor");
  number(hash2.outputLen);
  number(hash2.blockLen);
}
function exists(instance, checkFinished = true) {
  if (instance.destroyed)
    throw new Error("Hash instance has been destroyed");
  if (checkFinished && instance.finished)
    throw new Error("Hash#digest() has already been called");
}
function output(out, instance) {
  bytes(out);
  const min = instance.outputLen;
  if (out.length < min) {
    throw new Error(`digestInto() expects output buffer of length at least ${min}`);
  }
}
var assert = {
  number,
  bool,
  bytes,
  hash,
  exists,
  output
};
var assert_default = assert;

// ../../../node_modules/@noble/hashes/esm/crypto.js
var crypto2 = typeof globalThis === "object" && "crypto" in globalThis ? globalThis.crypto : void 0;

// ../../../node_modules/@noble/hashes/esm/utils.js
var u8a = (a) => a instanceof Uint8Array;
var createView = (arr) => new DataView(arr.buffer, arr.byteOffset, arr.byteLength);
var rotr = (word, shift) => word << 32 - shift | word >>> shift;
var isLE = new Uint8Array(new Uint32Array([287454020]).buffer)[0] === 68;
if (!isLE)
  throw new Error("Non little-endian hardware is not supported");
var hexes = Array.from({ length: 256 }, (v, i) => i.toString(16).padStart(2, "0"));
function utf8ToBytes(str) {
  if (typeof str !== "string")
    throw new Error(`utf8ToBytes expected string, got ${typeof str}`);
  return new Uint8Array(new TextEncoder().encode(str));
}
function toBytes2(data) {
  if (typeof data === "string")
    data = utf8ToBytes(data);
  if (!u8a(data))
    throw new Error(`expected Uint8Array, got ${typeof data}`);
  return data;
}
var Hash = class {
  // Safe version that clones internal state
  clone() {
    return this._cloneInto();
  }
};
function wrapConstructor(hashCons) {
  const hashC = (msg) => hashCons().update(toBytes2(msg)).digest();
  const tmp = hashCons();
  hashC.outputLen = tmp.outputLen;
  hashC.blockLen = tmp.blockLen;
  hashC.create = () => hashCons();
  return hashC;
}

// ../../../node_modules/@noble/hashes/esm/hmac.js
var HMAC = class extends Hash {
  constructor(hash2, _key) {
    super();
    this.finished = false;
    this.destroyed = false;
    assert_default.hash(hash2);
    const key = toBytes2(_key);
    this.iHash = hash2.create();
    if (typeof this.iHash.update !== "function")
      throw new Error("Expected instance of class which extends utils.Hash");
    this.blockLen = this.iHash.blockLen;
    this.outputLen = this.iHash.outputLen;
    const blockLen = this.blockLen;
    const pad = new Uint8Array(blockLen);
    pad.set(key.length > blockLen ? hash2.create().update(key).digest() : key);
    for (let i = 0; i < pad.length; i++)
      pad[i] ^= 54;
    this.iHash.update(pad);
    this.oHash = hash2.create();
    for (let i = 0; i < pad.length; i++)
      pad[i] ^= 54 ^ 92;
    this.oHash.update(pad);
    pad.fill(0);
  }
  update(buf) {
    assert_default.exists(this);
    this.iHash.update(buf);
    return this;
  }
  digestInto(out) {
    assert_default.exists(this);
    assert_default.bytes(out, this.outputLen);
    this.finished = true;
    this.iHash.digestInto(out);
    this.oHash.update(out);
    this.oHash.digestInto(out);
    this.destroy();
  }
  digest() {
    const out = new Uint8Array(this.oHash.outputLen);
    this.digestInto(out);
    return out;
  }
  _cloneInto(to) {
    to || (to = Object.create(Object.getPrototypeOf(this), {}));
    const { oHash, iHash, finished, destroyed, blockLen, outputLen } = this;
    to = to;
    to.finished = finished;
    to.destroyed = destroyed;
    to.blockLen = blockLen;
    to.outputLen = outputLen;
    to.oHash = oHash._cloneInto(to.oHash);
    to.iHash = iHash._cloneInto(to.iHash);
    return to;
  }
  destroy() {
    this.destroyed = true;
    this.oHash.destroy();
    this.iHash.destroy();
  }
};
var hmac = (hash2, key, message2) => new HMAC(hash2, key).update(message2).digest();
hmac.create = (hash2, key) => new HMAC(hash2, key);

// ../../../node_modules/@noble/hashes/esm/hkdf.js
function extract(hash2, ikm, salt) {
  assert_default.hash(hash2);
  if (salt === void 0)
    salt = new Uint8Array(hash2.outputLen);
  return hmac(hash2, toBytes2(salt), toBytes2(ikm));
}
var HKDF_COUNTER = new Uint8Array([0]);
var EMPTY_BUFFER = new Uint8Array();
function expand(hash2, prk, info, length5 = 32) {
  assert_default.hash(hash2);
  assert_default.number(length5);
  if (length5 > 255 * hash2.outputLen)
    throw new Error("Length should be <= 255*HashLen");
  const blocks = Math.ceil(length5 / hash2.outputLen);
  if (info === void 0)
    info = EMPTY_BUFFER;
  const okm = new Uint8Array(blocks * hash2.outputLen);
  const HMAC2 = hmac.create(hash2, prk);
  const HMACTmp = HMAC2._cloneInto();
  const T = new Uint8Array(HMAC2.outputLen);
  for (let counter = 0; counter < blocks; counter++) {
    HKDF_COUNTER[0] = counter + 1;
    HMACTmp.update(counter === 0 ? EMPTY_BUFFER : T).update(info).update(HKDF_COUNTER).digestInto(T);
    okm.set(T, hash2.outputLen * counter);
    HMAC2._cloneInto(HMACTmp);
  }
  HMAC2.destroy();
  HMACTmp.destroy();
  T.fill(0);
  HKDF_COUNTER.fill(0);
  return okm.slice(0, length5);
}
var hkdf = (hash2, ikm, salt, info, length5) => expand(hash2, extract(hash2, ikm, salt), info, length5);

// ../../../node_modules/@noble/hashes/esm/_sha2.js
function setBigUint64(view, byteOffset, value, isLE2) {
  if (typeof view.setBigUint64 === "function")
    return view.setBigUint64(byteOffset, value, isLE2);
  const _32n = BigInt(32);
  const _u32_max = BigInt(4294967295);
  const wh = Number(value >> _32n & _u32_max);
  const wl = Number(value & _u32_max);
  const h = isLE2 ? 4 : 0;
  const l = isLE2 ? 0 : 4;
  view.setUint32(byteOffset + h, wh, isLE2);
  view.setUint32(byteOffset + l, wl, isLE2);
}
var SHA2 = class extends Hash {
  constructor(blockLen, outputLen, padOffset, isLE2) {
    super();
    this.blockLen = blockLen;
    this.outputLen = outputLen;
    this.padOffset = padOffset;
    this.isLE = isLE2;
    this.finished = false;
    this.length = 0;
    this.pos = 0;
    this.destroyed = false;
    this.buffer = new Uint8Array(blockLen);
    this.view = createView(this.buffer);
  }
  update(data) {
    assert_default.exists(this);
    const { view, buffer, blockLen } = this;
    data = toBytes2(data);
    const len = data.length;
    for (let pos = 0; pos < len; ) {
      const take = Math.min(blockLen - this.pos, len - pos);
      if (take === blockLen) {
        const dataView = createView(data);
        for (; blockLen <= len - pos; pos += blockLen)
          this.process(dataView, pos);
        continue;
      }
      buffer.set(data.subarray(pos, pos + take), this.pos);
      this.pos += take;
      pos += take;
      if (this.pos === blockLen) {
        this.process(view, 0);
        this.pos = 0;
      }
    }
    this.length += data.length;
    this.roundClean();
    return this;
  }
  digestInto(out) {
    assert_default.exists(this);
    assert_default.output(out, this);
    this.finished = true;
    const { buffer, view, blockLen, isLE: isLE2 } = this;
    let { pos } = this;
    buffer[pos++] = 128;
    this.buffer.subarray(pos).fill(0);
    if (this.padOffset > blockLen - pos) {
      this.process(view, 0);
      pos = 0;
    }
    for (let i = pos; i < blockLen; i++)
      buffer[i] = 0;
    setBigUint64(view, blockLen - 8, BigInt(this.length * 8), isLE2);
    this.process(view, 0);
    const oview = createView(out);
    const len = this.outputLen;
    if (len % 4)
      throw new Error("_sha2: outputLen should be aligned to 32bit");
    const outLen = len / 4;
    const state = this.get();
    if (outLen > state.length)
      throw new Error("_sha2: outputLen bigger than state");
    for (let i = 0; i < outLen; i++)
      oview.setUint32(4 * i, state[i], isLE2);
  }
  digest() {
    const { buffer, outputLen } = this;
    this.digestInto(buffer);
    const res = buffer.slice(0, outputLen);
    this.destroy();
    return res;
  }
  _cloneInto(to) {
    to || (to = new this.constructor());
    to.set(...this.get());
    const { blockLen, buffer, length: length5, finished, destroyed, pos } = this;
    to.length = length5;
    to.pos = pos;
    to.finished = finished;
    to.destroyed = destroyed;
    if (length5 % blockLen)
      to.buffer.set(buffer);
    return to;
  }
};

// ../../../node_modules/@noble/hashes/esm/sha256.js
var Chi = (a, b, c) => a & b ^ ~a & c;
var Maj = (a, b, c) => a & b ^ a & c ^ b & c;
var SHA256_K = new Uint32Array([
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
]);
var IV = new Uint32Array([
  1779033703,
  3144134277,
  1013904242,
  2773480762,
  1359893119,
  2600822924,
  528734635,
  1541459225
]);
var SHA256_W = new Uint32Array(64);
var SHA256 = class extends SHA2 {
  constructor() {
    super(64, 32, 8, false);
    this.A = IV[0] | 0;
    this.B = IV[1] | 0;
    this.C = IV[2] | 0;
    this.D = IV[3] | 0;
    this.E = IV[4] | 0;
    this.F = IV[5] | 0;
    this.G = IV[6] | 0;
    this.H = IV[7] | 0;
  }
  get() {
    const { A, B, C, D, E, F, G, H } = this;
    return [A, B, C, D, E, F, G, H];
  }
  // prettier-ignore
  set(A, B, C, D, E, F, G, H) {
    this.A = A | 0;
    this.B = B | 0;
    this.C = C | 0;
    this.D = D | 0;
    this.E = E | 0;
    this.F = F | 0;
    this.G = G | 0;
    this.H = H | 0;
  }
  process(view, offset) {
    for (let i = 0; i < 16; i++, offset += 4)
      SHA256_W[i] = view.getUint32(offset, false);
    for (let i = 16; i < 64; i++) {
      const W15 = SHA256_W[i - 15];
      const W2 = SHA256_W[i - 2];
      const s0 = rotr(W15, 7) ^ rotr(W15, 18) ^ W15 >>> 3;
      const s1 = rotr(W2, 17) ^ rotr(W2, 19) ^ W2 >>> 10;
      SHA256_W[i] = s1 + SHA256_W[i - 7] + s0 + SHA256_W[i - 16] | 0;
    }
    let { A, B, C, D, E, F, G, H } = this;
    for (let i = 0; i < 64; i++) {
      const sigma1 = rotr(E, 6) ^ rotr(E, 11) ^ rotr(E, 25);
      const T1 = H + sigma1 + Chi(E, F, G) + SHA256_K[i] + SHA256_W[i] | 0;
      const sigma0 = rotr(A, 2) ^ rotr(A, 13) ^ rotr(A, 22);
      const T2 = sigma0 + Maj(A, B, C) | 0;
      H = G;
      G = F;
      F = E;
      E = D + T1 | 0;
      D = C;
      C = B;
      B = A;
      A = T1 + T2 | 0;
    }
    A = A + this.A | 0;
    B = B + this.B | 0;
    C = C + this.C | 0;
    D = D + this.D | 0;
    E = E + this.E | 0;
    F = F + this.F | 0;
    G = G + this.G | 0;
    H = H + this.H | 0;
    this.set(A, B, C, D, E, F, G, H);
  }
  roundClean() {
    SHA256_W.fill(0);
  }
  destroy() {
    this.set(0, 0, 0, 0, 0, 0, 0, 0);
    this.buffer.fill(0);
  }
};
var SHA224 = class extends SHA256 {
  constructor() {
    super();
    this.A = 3238371032 | 0;
    this.B = 914150663 | 0;
    this.C = 812702999 | 0;
    this.D = 4144912697 | 0;
    this.E = 4290775857 | 0;
    this.F = 1750603025 | 0;
    this.G = 1694076839 | 0;
    this.H = 3204075428 | 0;
    this.outputLen = 28;
  }
};
var sha2564 = wrapConstructor(() => new SHA256());
var sha224 = wrapConstructor(() => new SHA224());

// ../../../node_modules/@chainsafe/libp2p-noise/dist/src/crypto/js.js
var import_chacha20poly1305 = __toESM(require_chacha20poly1305(), 1);
var x25519 = __toESM(require_x25519(), 1);
var pureJsCrypto = {
  hashSHA256(data) {
    return sha2564(data);
  },
  getHKDF(ck, ikm) {
    const okm = hkdf(sha2564, ikm, ck, void 0, 96);
    const k1 = okm.subarray(0, 32);
    const k2 = okm.subarray(32, 64);
    const k3 = okm.subarray(64, 96);
    return [k1, k2, k3];
  },
  generateX25519KeyPair() {
    const keypair = x25519.generateKeyPair();
    return {
      publicKey: keypair.publicKey,
      privateKey: keypair.secretKey
    };
  },
  generateX25519KeyPairFromSeed(seed) {
    const keypair = x25519.generateKeyPairFromSeed(seed);
    return {
      publicKey: keypair.publicKey,
      privateKey: keypair.secretKey
    };
  },
  generateX25519SharedKey(privateKey, publicKey) {
    return x25519.sharedKey(privateKey, publicKey);
  },
  chaCha20Poly1305Encrypt(plaintext, nonce, ad, k) {
    const ctx = new import_chacha20poly1305.ChaCha20Poly1305(k);
    return ctx.seal(nonce, plaintext, ad);
  },
  chaCha20Poly1305Decrypt(ciphertext, nonce, ad, k, dst) {
    const ctx = new import_chacha20poly1305.ChaCha20Poly1305(k);
    return ctx.open(nonce, ciphertext, ad, dst);
  }
};

// ../../../node_modules/@chainsafe/libp2p-noise/dist/src/crypto/streaming.js
var import_chacha20poly13052 = __toESM(require_chacha20poly1305(), 1);

// ../../../node_modules/@chainsafe/libp2p-noise/dist/src/encoder.js
var allocUnsafe2 = (len) => {
  if (globalThis.Buffer) {
    return globalThis.Buffer.allocUnsafe(len);
  }
  return new Uint8Array(len);
};
var uint16BEEncode = (value) => {
  const target = allocUnsafe2(2);
  new DataView(target.buffer, target.byteOffset, target.byteLength).setUint16(0, value, false);
  return target;
};
uint16BEEncode.bytes = 2;
var uint16BEDecode = (data) => {
  if (data.length < 2)
    throw RangeError("Could not decode int16BE");
  if (data instanceof Uint8Array) {
    return new DataView(data.buffer, data.byteOffset, data.byteLength).getUint16(0, false);
  }
  return data.getUint16(0);
};
uint16BEDecode.bytes = 2;
function encode0(message2) {
  return concat([message2.ne, message2.ciphertext], message2.ne.length + message2.ciphertext.length);
}
function encode1(message2) {
  return concat([message2.ne, message2.ns, message2.ciphertext], message2.ne.length + message2.ns.length + message2.ciphertext.length);
}
function encode22(message2) {
  return concat([message2.ns, message2.ciphertext], message2.ns.length + message2.ciphertext.length);
}
function decode0(input) {
  if (input.length < 32) {
    throw new Error("Cannot decode stage 0 MessageBuffer: length less than 32 bytes.");
  }
  return {
    ne: input.subarray(0, 32),
    ciphertext: input.subarray(32, input.length),
    ns: new Uint8Array(0)
  };
}
function decode1(input) {
  if (input.length < 80) {
    throw new Error("Cannot decode stage 1 MessageBuffer: length less than 80 bytes.");
  }
  return {
    ne: input.subarray(0, 32),
    ns: input.subarray(32, 80),
    ciphertext: input.subarray(80, input.length)
  };
}
function decode22(input) {
  if (input.length < 48) {
    throw new Error("Cannot decode stage 2 MessageBuffer: length less than 48 bytes.");
  }
  return {
    ne: new Uint8Array(0),
    ns: input.subarray(0, 48),
    ciphertext: input.subarray(48, input.length)
  };
}

// ../../../node_modules/@chainsafe/libp2p-noise/dist/src/crypto/streaming.js
function encryptStream(handshake, metrics) {
  return async function* (source) {
    for await (const chunk of source) {
      for (let i = 0; i < chunk.length; i += NOISE_MSG_MAX_LENGTH_BYTES_WITHOUT_TAG) {
        let end = i + NOISE_MSG_MAX_LENGTH_BYTES_WITHOUT_TAG;
        if (end > chunk.length) {
          end = chunk.length;
        }
        const data = handshake.encrypt(chunk.subarray(i, end), handshake.session);
        metrics?.encryptedPackets.increment();
        yield uint16BEEncode(data.byteLength);
        yield data;
      }
    }
  };
}
function decryptStream(handshake, metrics) {
  return async function* (source) {
    for await (const chunk of source) {
      for (let i = 0; i < chunk.length; i += NOISE_MSG_MAX_LENGTH_BYTES) {
        let end = i + NOISE_MSG_MAX_LENGTH_BYTES;
        if (end > chunk.length) {
          end = chunk.length;
        }
        if (end - import_chacha20poly13052.TAG_LENGTH < i) {
          throw new Error("Invalid chunk");
        }
        const encrypted = chunk.subarray(i, end);
        const dst = chunk.subarray(i, end - import_chacha20poly13052.TAG_LENGTH);
        const { plaintext: decrypted, valid } = handshake.decrypt(encrypted, handshake.session, dst);
        if (!valid) {
          metrics?.decryptErrors.increment();
          throw new Error("Failed to validate decrypted chunk");
        }
        metrics?.decryptedPackets.increment();
        yield decrypted;
      }
    }
  };
}

// ../../../node_modules/@libp2p/interface-connection-encrypter/dist/src/errors.js
var UnexpectedPeerError = class _UnexpectedPeerError extends Error {
  code;
  constructor(message2 = "Unexpected Peer") {
    super(message2);
    this.code = _UnexpectedPeerError.code;
  }
  static code = "ERR_UNEXPECTED_PEER";
};
var InvalidCryptoExchangeError = class _InvalidCryptoExchangeError extends Error {
  code;
  constructor(message2 = "Invalid crypto exchange") {
    super(message2);
    this.code = _InvalidCryptoExchangeError.code;
  }
  static code = "ERR_INVALID_CRYPTO_EXCHANGE";
};

// ../../crypto/dist/src/keys/index.js
var import_asn12 = __toESM(require_asn1(), 1);
var import_pbe = __toESM(require_pbe(), 1);
var import_forge5 = __toESM(require_forge(), 1);

// ../../crypto/dist/src/keys/ed25519-class.js
var ed25519_class_exports = {};
__export(ed25519_class_exports, {
  Ed25519PrivateKey: () => Ed25519PrivateKey,
  Ed25519PublicKey: () => Ed25519PublicKey,
  generateKeyPair: () => generateKeyPair2,
  generateKeyPairFromSeed: () => generateKeyPairFromSeed2,
  unmarshalEd25519PrivateKey: () => unmarshalEd25519PrivateKey,
  unmarshalEd25519PublicKey: () => unmarshalEd25519PublicKey
});

// ../../../node_modules/@noble/ed25519/lib/esm/index.js
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
    let base7 = p;
    for (let window2 = 0; window2 < windows; window2++) {
      base7 = p;
      points.push(base7);
      for (let i = 1; i < 2 ** (W - 1); i++) {
        base7 = base7.add(p);
        points.push(base7);
      }
      p = base7.double();
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
    const bytes2 = numberTo32BytesLE(this.y);
    bytes2[31] |= this.x & _1n ? 128 : 0;
    return bytes2;
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
    const bytes2 = ensureBytes(hex, 64);
    const r = Point.fromHex(bytes2.slice(0, 32), false);
    const s = bytesToNumberLE(bytes2.slice(32, 64));
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
  const length5 = arrays.reduce((a, arr) => a + arr.length, 0);
  const result = new Uint8Array(length5);
  for (let i = 0, pad = 0; i < arrays.length; i++) {
    const arr = arrays[i];
    result.set(arr, pad);
    pad += arr.length;
  }
  return result;
}
var hexes2 = Array.from({ length: 256 }, (v, i) => i.toString(16).padStart(2, "0"));
function bytesToHex(uint8a) {
  if (!(uint8a instanceof Uint8Array))
    throw new Error("Uint8Array expected");
  let hex = "";
  for (let i = 0; i < uint8a.length; i++) {
    hex += hexes2[uint8a[i]];
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
  const length5 = 32;
  const hex = num.toString(16).padStart(length5 * 2, "0");
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
function bytes255ToNumberLE(bytes2) {
  return mod2(bytesToNumberLE(bytes2) & MAX_255B);
}
function mod2(a, b = CURVE.P) {
  const res = a % b;
  return res >= _0n ? res : b + res;
}
function invert(number2, modulo = CURVE.P) {
  if (number2 === _0n || modulo <= _0n) {
    throw new Error(`invert: expected positive integers, got n=${number2} mod=${modulo}`);
  }
  let a = mod2(number2, modulo);
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
function invertSqrt(number2) {
  return uvRatio(_1n, number2);
}
function modlLE(hash2) {
  return mod2(bytesToNumberLE(hash2), CURVE.l);
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
  const bytes2 = hex instanceof Uint8Array ? Uint8Array.from(hex) : hexToBytes(hex);
  if (typeof expectedLength === "number" && bytes2.length !== expectedLength)
    throw new Error(`Expected ${expectedLength} bytes`);
  return bytes2;
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
function adjustBytes25519(bytes2) {
  bytes2[0] &= 248;
  bytes2[31] &= 127;
  bytes2[31] |= 64;
  return bytes2;
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
var crypto3 = {
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
  hashToPrivateScalar: (hash2) => {
    hash2 = ensureBytes(hash2);
    if (hash2.length < 40 || hash2.length > 1024)
      throw new Error("Expected 40-1024 bytes of private key as per FIPS 186");
    return mod2(bytesToNumberLE(hash2), CURVE.l - _1n) + _1n;
  },
  randomBytes: (bytesLength = 32) => {
    if (crypto3.web) {
      return crypto3.web.getRandomValues(new Uint8Array(bytesLength));
    } else if (crypto3.node) {
      const { randomBytes: randomBytes2 } = crypto3.node;
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
    if (crypto3.web) {
      const buffer = await crypto3.web.subtle.digest("SHA-512", message2.buffer);
      return new Uint8Array(buffer);
    } else if (crypto3.node) {
      return Uint8Array.from(crypto3.node.createHash("sha512").update(message2).digest());
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

// ../../crypto/dist/src/keys/ed25519-browser.js
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

// ../../crypto/dist/src/webcrypto.js
var webcrypto_default = {
  get(win = globalThis) {
    const nativeCrypto = win.crypto;
    if (nativeCrypto == null || nativeCrypto.subtle == null) {
      throw Object.assign(new Error("Missing Web Crypto API. The most likely cause of this error is that this page is being accessed from an insecure context (i.e. not HTTPS). For more information and possible resolutions see https://github.com/libp2p/js-libp2p-crypto/blob/master/README.md#web-crypto-api"), { code: "ERR_MISSING_WEB_CRYPTO" });
    }
    return nativeCrypto;
  }
};

// ../../crypto/dist/src/ciphers/aes-gcm.browser.js
var derivedEmptyPasswordKey = { alg: "A128GCM", ext: true, k: "scm9jmO_4BJAgdwWGVulLg", key_ops: ["encrypt", "decrypt"], kty: "oct" };
function create4(opts) {
  const algorithm = opts?.algorithm ?? "AES-GCM";
  let keyLength = opts?.keyLength ?? 16;
  const nonceLength = opts?.nonceLength ?? 12;
  const digest5 = opts?.digest ?? "SHA-256";
  const saltLength = opts?.saltLength ?? 16;
  const iterations = opts?.iterations ?? 32767;
  const crypto5 = webcrypto_default.get();
  keyLength *= 8;
  async function encrypt2(data, password) {
    const salt = crypto5.getRandomValues(new Uint8Array(saltLength));
    const nonce = crypto5.getRandomValues(new Uint8Array(nonceLength));
    const aesGcm = { name: algorithm, iv: nonce };
    if (typeof password === "string") {
      password = fromString4(password);
    }
    let cryptoKey;
    if (password.length === 0) {
      cryptoKey = await crypto5.subtle.importKey("jwk", derivedEmptyPasswordKey, { name: "AES-GCM" }, true, ["encrypt"]);
      try {
        const deriveParams = { name: "PBKDF2", salt, iterations, hash: { name: digest5 } };
        const runtimeDerivedEmptyPassword = await crypto5.subtle.importKey("raw", password, { name: "PBKDF2" }, false, ["deriveKey"]);
        cryptoKey = await crypto5.subtle.deriveKey(deriveParams, runtimeDerivedEmptyPassword, { name: algorithm, length: keyLength }, true, ["encrypt"]);
      } catch {
        cryptoKey = await crypto5.subtle.importKey("jwk", derivedEmptyPasswordKey, { name: "AES-GCM" }, true, ["encrypt"]);
      }
    } else {
      const deriveParams = { name: "PBKDF2", salt, iterations, hash: { name: digest5 } };
      const rawKey = await crypto5.subtle.importKey("raw", password, { name: "PBKDF2" }, false, ["deriveKey"]);
      cryptoKey = await crypto5.subtle.deriveKey(deriveParams, rawKey, { name: algorithm, length: keyLength }, true, ["encrypt"]);
    }
    const ciphertext = await crypto5.subtle.encrypt(aesGcm, cryptoKey, data);
    return concat([salt, aesGcm.iv, new Uint8Array(ciphertext)]);
  }
  async function decrypt2(data, password) {
    const salt = data.subarray(0, saltLength);
    const nonce = data.subarray(saltLength, saltLength + nonceLength);
    const ciphertext = data.subarray(saltLength + nonceLength);
    const aesGcm = { name: algorithm, iv: nonce };
    if (typeof password === "string") {
      password = fromString4(password);
    }
    let cryptoKey;
    if (password.length === 0) {
      try {
        const deriveParams = { name: "PBKDF2", salt, iterations, hash: { name: digest5 } };
        const runtimeDerivedEmptyPassword = await crypto5.subtle.importKey("raw", password, { name: "PBKDF2" }, false, ["deriveKey"]);
        cryptoKey = await crypto5.subtle.deriveKey(deriveParams, runtimeDerivedEmptyPassword, { name: algorithm, length: keyLength }, true, ["decrypt"]);
      } catch {
        cryptoKey = await crypto5.subtle.importKey("jwk", derivedEmptyPasswordKey, { name: "AES-GCM" }, true, ["decrypt"]);
      }
    } else {
      const deriveParams = { name: "PBKDF2", salt, iterations, hash: { name: digest5 } };
      const rawKey = await crypto5.subtle.importKey("raw", password, { name: "PBKDF2" }, false, ["deriveKey"]);
      cryptoKey = await crypto5.subtle.deriveKey(deriveParams, rawKey, { name: algorithm, length: keyLength }, true, ["decrypt"]);
    }
    const plaintext = await crypto5.subtle.decrypt(aesGcm, cryptoKey, ciphertext);
    return new Uint8Array(plaintext);
  }
  const cipher = {
    encrypt: encrypt2,
    decrypt: decrypt2
  };
  return cipher;
}

// ../../crypto/dist/src/keys/exporter.js
async function exporter(privateKey, password) {
  const cipher = create4();
  const encryptedKey = await cipher.encrypt(privateKey, password);
  return base64.encode(encryptedKey);
}

// ../../crypto/dist/src/keys/keys.js
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
      }, (reader2, length5) => {
        const obj = {};
        const end = length5 == null ? reader2.len : reader2.pos + length5;
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
      }, (reader2, length5) => {
        const obj = {};
        const end = length5 == null ? reader2.len : reader2.pos + length5;
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

// ../../crypto/dist/src/keys/ed25519-class.js
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
    return equals3(this.bytes, key.bytes);
  }
  async hash() {
    const { bytes: bytes2 } = await sha256.digest(this.bytes);
    return bytes2;
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
    return equals3(this.bytes, key.bytes);
  }
  async hash() {
    const { bytes: bytes2 } = await sha256.digest(this.bytes);
    return bytes2;
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
    const encoding = identity2.digest(this.public.bytes);
    return base58btc.encode(encoding.bytes).substring(1);
  }
  /**
   * Exports the key into a password protected `format`
   */
  async export(password, format4 = "libp2p-key") {
    if (format4 === "libp2p-key") {
      return exporter(this.bytes, password);
    } else {
      throw new CodeError(`export format '${format4}' is not supported`, "ERR_INVALID_EXPORT_FORMAT");
    }
  }
};
function unmarshalEd25519PrivateKey(bytes2) {
  if (bytes2.length > PRIVATE_KEY_BYTE_LENGTH) {
    bytes2 = ensureKey(bytes2, PRIVATE_KEY_BYTE_LENGTH + PUBLIC_KEY_BYTE_LENGTH);
    const privateKeyBytes2 = bytes2.subarray(0, PRIVATE_KEY_BYTE_LENGTH);
    const publicKeyBytes2 = bytes2.subarray(PRIVATE_KEY_BYTE_LENGTH, bytes2.length);
    return new Ed25519PrivateKey(privateKeyBytes2, publicKeyBytes2);
  }
  bytes2 = ensureKey(bytes2, PRIVATE_KEY_BYTE_LENGTH);
  const privateKeyBytes = bytes2.subarray(0, PRIVATE_KEY_BYTE_LENGTH);
  const publicKeyBytes = bytes2.subarray(PUBLIC_KEY_BYTE_LENGTH);
  return new Ed25519PrivateKey(privateKeyBytes, publicKeyBytes);
}
function unmarshalEd25519PublicKey(bytes2) {
  bytes2 = ensureKey(bytes2, PUBLIC_KEY_BYTE_LENGTH);
  return new Ed25519PublicKey(bytes2);
}
async function generateKeyPair2() {
  const { privateKey, publicKey } = await generateKey();
  return new Ed25519PrivateKey(privateKey, publicKey);
}
async function generateKeyPairFromSeed2(seed) {
  const { privateKey, publicKey } = await generateKeyFromSeed(seed);
  return new Ed25519PrivateKey(privateKey, publicKey);
}
function ensureKey(key, length5) {
  key = Uint8Array.from(key ?? []);
  if (key.length !== length5) {
    throw new CodeError(`Key must be a Uint8Array of length ${length5}, got ${key.length}`, "ERR_INVALID_KEY_TYPE");
  }
  return key;
}

// ../../crypto/dist/src/util.js
var import_util8 = __toESM(require_util(), 1);
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
  return toString4(buf, "base64url");
}
function base64urlToBigInteger(str) {
  const buf = base64urlToBuffer(str);
  return new import_forge.default.jsbn.BigInteger(toString4(buf, "base16"), 16);
}
function base64urlToBuffer(str, len) {
  let buf = fromString4(str, "base64urlpad");
  if (len != null) {
    if (buf.length > len)
      throw new Error("byte array longer than desired length");
    buf = concat([new Uint8Array(len - buf.length), buf]);
  }
  return buf;
}

// ../../crypto/dist/src/keys/ecdh-browser.js
var bits = {
  "P-256": 256,
  "P-384": 384,
  "P-521": 521
};
var curveTypes = Object.keys(bits);
var names2 = curveTypes.join(" / ");

// ../../crypto/dist/src/keys/rsa-class.js
var rsa_class_exports = {};
__export(rsa_class_exports, {
  RsaPrivateKey: () => RsaPrivateKey,
  RsaPublicKey: () => RsaPublicKey,
  fromJwk: () => fromJwk,
  generateKeyPair: () => generateKeyPair3,
  unmarshalRsaPrivateKey: () => unmarshalRsaPrivateKey,
  unmarshalRsaPublicKey: () => unmarshalRsaPublicKey
});
var import_forge4 = __toESM(require_forge(), 1);
var import_sha512 = __toESM(require_sha512(), 1);

// ../../../node_modules/@noble/secp256k1/lib/esm/index.js
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
    const V2 = mod3(U1 * HH);
    const X3 = mod3(r * r - HHH - _2n2 * V2);
    const Y3 = mod3(r * (V2 - X3) - S1 * HHH);
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
    let base7 = p;
    for (let window2 = 0; window2 < windows; window2++) {
      base7 = p;
      points.push(base7);
      for (let i = 1; i < 2 ** (W - 1); i++) {
        base7 = base7.add(p);
        points.push(base7);
      }
      p = base7.double();
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
  static fromCompressedHex(bytes2) {
    const isShort = bytes2.length === 32;
    const x = bytesToNumber(isShort ? bytes2 : bytes2.subarray(1));
    if (!isValidFieldElement(x))
      throw new Error("Point is not on curve");
    const y2 = weierstrass(x);
    let y = sqrtMod(y2);
    const isYOdd = (y & _1n2) === _1n2;
    if (isShort) {
      if (isYOdd)
        y = mod3(-y);
    } else {
      const isFirstByteOdd = (bytes2[0] & 1) === 1;
      if (isFirstByteOdd !== isYOdd)
        y = mod3(-y);
    }
    const point = new _Point(x, y);
    point.assertValidity();
    return point;
  }
  static fromUncompressedHex(bytes2) {
    const x = bytesToNumber(bytes2.subarray(1, fieldLen + 1));
    const y = bytesToNumber(bytes2.subarray(fieldLen + 1, fieldLen * 2 + 1));
    const point = new _Point(x, y);
    point.assertValidity();
    return point;
  }
  static fromHex(hex) {
    const bytes2 = ensureBytes2(hex);
    const len = bytes2.length;
    const header = bytes2[0];
    if (len === fieldLen)
      return this.fromCompressedHex(bytes2);
    if (len === compressedLen && (header === 2 || header === 3)) {
      return this.fromCompressedHex(bytes2);
    }
    if (len === uncompressedLen && header === 4)
      return this.fromUncompressedHex(bytes2);
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
    const name5 = "Signature.fromCompact";
    if (typeof hex !== "string" && !arr)
      throw new TypeError(`${name5}: Expected string or Uint8Array`);
    const str = arr ? bytesToHex2(hex) : hex;
    if (str.length !== 128)
      throw new Error(`${name5}: Expected 64-byte hex`);
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
    const length5 = numberToHexUnpadded(rHexL + sHexL + 4);
    return `30${length5}02${rLen}${rHex}02${sLen}${sHex}`;
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
  const length5 = arrays.reduce((a, arr) => a + arr.length, 0);
  const result = new Uint8Array(length5);
  for (let i = 0, pad = 0; i < arrays.length; i++) {
    const arr = arrays[i];
    result.set(arr, pad);
    pad += arr.length;
  }
  return result;
}
var hexes3 = Array.from({ length: 256 }, (v, i) => i.toString(16).padStart(2, "0"));
function bytesToHex2(uint8a) {
  if (!(uint8a instanceof Uint8Array))
    throw new Error("Expected Uint8Array");
  let hex = "";
  for (let i = 0; i < uint8a.length; i++) {
    hex += hexes3[uint8a[i]];
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
function bytesToNumber(bytes2) {
  return hexToNumber(bytesToHex2(bytes2));
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
function invert2(number2, modulo = CURVE2.P) {
  if (number2 === _0n2 || modulo <= _0n2) {
    throw new Error(`invert: expected positive integers, got n=${number2} mod=${modulo}`);
  }
  let a = mod3(number2, modulo);
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
function bits2int_2(bytes2) {
  const delta = bytes2.length * 8 - groupLen * 8;
  const num = bytesToNumber(bytes2);
  return delta > 0 ? num >> BigInt(delta) : num;
}
function truncateHash(hash2, truncateOnly = false) {
  const h = bits2int_2(hash2);
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
function bits2int(bytes2) {
  const slice = bytes2.length > fieldLen ? bytes2.slice(0, fieldLen) : bytes2;
  return bytesToNumber(slice);
}
function bits2octets(bytes2) {
  const z1 = bits2int(bytes2);
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
var crypto4 = {
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
  hashToPrivateKey: (hash2) => {
    hash2 = ensureBytes2(hash2);
    const minLen = groupLen + 8;
    if (hash2.length < minLen || hash2.length > 1024) {
      throw new Error(`Expected valid bytes of private key as per FIPS 186`);
    }
    const num = mod3(bytesToNumber(hash2), CURVE2.n - _1n2) + _1n2;
    return numTo32b(num);
  },
  randomBytes: (bytesLength = 32) => {
    if (crypto4.web) {
      return crypto4.web.getRandomValues(new Uint8Array(bytesLength));
    } else if (crypto4.node) {
      const { randomBytes: randomBytes2 } = crypto4.node;
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
    if (crypto4.web) {
      const buffer = await crypto4.web.subtle.digest("SHA-256", concatBytes2(...messages));
      return new Uint8Array(buffer);
    } else if (crypto4.node) {
      const { createHash } = crypto4.node;
      const hash2 = createHash("sha256");
      messages.forEach((m) => hash2.update(m));
      return Uint8Array.from(hash2.digest());
    } else {
      throw new Error("The environment doesn't have sha256 function");
    }
  },
  hmacSha256: async (key, ...messages) => {
    if (crypto4.web) {
      const ckey = await crypto4.web.subtle.importKey("raw", key, { name: "HMAC", hash: { name: "SHA-256" } }, false, ["sign"]);
      const message2 = concatBytes2(...messages);
      const buffer = await crypto4.web.subtle.sign("HMAC", ckey, message2);
      return new Uint8Array(buffer);
    } else if (crypto4.node) {
      const { createHmac } = crypto4.node;
      const hash2 = createHmac("sha256", key);
      messages.forEach((m) => hash2.update(m));
      return Uint8Array.from(hash2.digest());
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

// ../../crypto/dist/src/random-bytes.js
function randomBytes(length5) {
  if (isNaN(length5) || length5 <= 0) {
    throw new CodeError("random bytes length must be a Number bigger than 0", "ERR_INVALID_LENGTH");
  }
  return utils2.randomBytes(length5);
}

// ../../crypto/dist/src/keys/jwk2pem.js
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

// ../../crypto/dist/src/keys/rsa-utils.js
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
function pkcs1ToJwk(bytes2) {
  const asn1 = import_forge3.default.asn1.fromDer(toString4(bytes2, "ascii"));
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
  return fromString4(import_forge3.default.asn1.toDer(asn1).getBytes(), "ascii");
}
function pkixToJwk(bytes2) {
  const asn1 = import_forge3.default.asn1.fromDer(toString4(bytes2, "ascii"));
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
  return fromString4(import_forge3.default.asn1.toDer(asn1).getBytes(), "ascii");
}

// ../../crypto/dist/src/keys/rsa-browser.js
async function generateKey2(bits2) {
  const pair2 = await webcrypto_default.get().subtle.generateKey({
    name: "RSASSA-PKCS1-v1_5",
    modulusLength: bits2,
    publicExponent: new Uint8Array([1, 0, 1]),
    hash: { name: "SHA-256" }
  }, true, ["sign", "verify"]);
  const keys = await exportKey(pair2);
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
  const pair2 = [
    privateKey,
    await derivePublicFromPrivate(key)
  ];
  const keys = await exportKey({
    privateKey: pair2[0],
    publicKey: pair2[1]
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
async function exportKey(pair2) {
  if (pair2.privateKey == null || pair2.publicKey == null) {
    throw new CodeError("Private and public key are required", "ERR_INVALID_PARAMETERS");
  }
  return Promise.all([
    webcrypto_default.get().subtle.exportKey("jwk", pair2.privateKey),
    webcrypto_default.get().subtle.exportKey("jwk", pair2.publicKey)
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
  const fmsg = toString4(Uint8Array.from(msg), "ascii");
  const fomsg = handle(fmsg, fkey);
  return fromString4(fomsg, "ascii");
}
function encrypt(key, msg) {
  return convertKey(key, true, msg, (msg2, key2) => key2.encrypt(msg2));
}
function decrypt(key, msg) {
  return convertKey(key, false, msg, (msg2, key2) => key2.decrypt(msg2));
}

// ../../crypto/dist/src/keys/rsa-class.js
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
  encrypt(bytes2) {
    return encrypt(this._key, bytes2);
  }
  equals(key) {
    return equals3(this.bytes, key.bytes);
  }
  async hash() {
    const { bytes: bytes2 } = await sha256.digest(this.bytes);
    return bytes2;
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
  decrypt(bytes2) {
    return decrypt(this._key, bytes2);
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
    return equals3(this.bytes, key.bytes);
  }
  async hash() {
    const { bytes: bytes2 } = await sha256.digest(this.bytes);
    return bytes2;
  }
  /**
   * Gets the ID of the key.
   *
   * The key id is the base58 encoding of the SHA-256 multihash of its public key.
   * The public key is a protobuf encoding containing a type and the DER encoding
   * of the PKCS SubjectPublicKeyInfo.
   */
  async id() {
    const hash2 = await this.public.hash();
    return toString4(hash2, "base58btc");
  }
  /**
   * Exports the key into a password protected PEM format
   */
  async export(password, format4 = "pkcs-8") {
    if (format4 === "pkcs-8") {
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
    } else if (format4 === "libp2p-key") {
      return exporter(this.bytes, password);
    } else {
      throw new CodeError(`export format '${format4}' is not supported`, "ERR_INVALID_EXPORT_FORMAT");
    }
  }
};
async function unmarshalRsaPrivateKey(bytes2) {
  const jwk = rsa_utils_exports.pkcs1ToJwk(bytes2);
  const keys = await unmarshalPrivateKey(jwk);
  return new RsaPrivateKey(keys.privateKey, keys.publicKey);
}
function unmarshalRsaPublicKey(bytes2) {
  const jwk = rsa_utils_exports.pkixToJwk(bytes2);
  return new RsaPublicKey(jwk);
}
async function fromJwk(jwk) {
  const keys = await unmarshalPrivateKey(jwk);
  return new RsaPrivateKey(keys.privateKey, keys.publicKey);
}
async function generateKeyPair3(bits2) {
  const keys = await generateKey2(bits2);
  return new RsaPrivateKey(keys.privateKey, keys.publicKey);
}

// ../../crypto/dist/src/keys/secp256k1-class.js
var secp256k1_class_exports = {};
__export(secp256k1_class_exports, {
  Secp256k1PrivateKey: () => Secp256k1PrivateKey,
  Secp256k1PublicKey: () => Secp256k1PublicKey,
  generateKeyPair: () => generateKeyPair4,
  unmarshalSecp256k1PrivateKey: () => unmarshalSecp256k1PrivateKey,
  unmarshalSecp256k1PublicKey: () => unmarshalSecp256k1PublicKey
});

// ../../crypto/dist/src/keys/secp256k1.js
function generateKey3() {
  return utils2.randomPrivateKey();
}
async function hashAndSign3(key, msg) {
  const { digest: digest5 } = await sha256.digest(msg);
  try {
    return await sign2(digest5, key);
  } catch (err) {
    throw new CodeError(String(err), "ERR_INVALID_INPUT");
  }
}
async function hashAndVerify3(key, sig, msg) {
  try {
    const { digest: digest5 } = await sha256.digest(msg);
    return verify2(sig, digest5, key);
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

// ../../crypto/dist/src/keys/secp256k1-class.js
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
    return equals3(this.bytes, key.bytes);
  }
  async hash() {
    const { bytes: bytes2 } = await sha256.digest(this.bytes);
    return bytes2;
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
    return equals3(this.bytes, key.bytes);
  }
  async hash() {
    const { bytes: bytes2 } = await sha256.digest(this.bytes);
    return bytes2;
  }
  /**
   * Gets the ID of the key.
   *
   * The key id is the base58 encoding of the SHA-256 multihash of its public key.
   * The public key is a protobuf encoding containing a type and the DER encoding
   * of the PKCS SubjectPublicKeyInfo.
   */
  async id() {
    const hash2 = await this.public.hash();
    return toString4(hash2, "base58btc");
  }
  /**
   * Exports the key into a password protected `format`
   */
  async export(password, format4 = "libp2p-key") {
    if (format4 === "libp2p-key") {
      return exporter(this.bytes, password);
    } else {
      throw new CodeError(`export format '${format4}' is not supported`, "ERR_INVALID_EXPORT_FORMAT");
    }
  }
};
function unmarshalSecp256k1PrivateKey(bytes2) {
  return new Secp256k1PrivateKey(bytes2);
}
function unmarshalSecp256k1PublicKey(bytes2) {
  return new Secp256k1PublicKey(bytes2);
}
async function generateKeyPair4() {
  const privateKeyBytes = generateKey3();
  return new Secp256k1PrivateKey(privateKeyBytes);
}

// ../../crypto/dist/src/keys/index.js
var supportedKeys = {
  rsa: rsa_class_exports,
  ed25519: ed25519_class_exports,
  secp256k1: secp256k1_class_exports
};
function unsupportedKey(type) {
  const supported = Object.keys(supportedKeys).join(" / ");
  return new CodeError(`invalid or unsupported key type ${type}. Must be ${supported}`, "ERR_UNSUPPORTED_KEY_TYPE");
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

// ../../../node_modules/@chainsafe/libp2p-noise/dist/src/proto/payload.js
var NoiseExtensions;
(function(NoiseExtensions2) {
  let _codec;
  NoiseExtensions2.codec = () => {
    if (_codec == null) {
      _codec = message((obj, w, opts = {}) => {
        if (opts.lengthDelimited !== false) {
          w.fork();
        }
        if (obj.webtransportCerthashes != null) {
          for (const value of obj.webtransportCerthashes) {
            w.uint32(10);
            w.bytes(value);
          }
        }
        if (opts.lengthDelimited !== false) {
          w.ldelim();
        }
      }, (reader2, length5) => {
        const obj = {
          webtransportCerthashes: []
        };
        const end = length5 == null ? reader2.len : reader2.pos + length5;
        while (reader2.pos < end) {
          const tag = reader2.uint32();
          switch (tag >>> 3) {
            case 1:
              obj.webtransportCerthashes.push(reader2.bytes());
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
  NoiseExtensions2.encode = (obj) => {
    return encodeMessage(obj, NoiseExtensions2.codec());
  };
  NoiseExtensions2.decode = (buf) => {
    return decodeMessage(buf, NoiseExtensions2.codec());
  };
})(NoiseExtensions || (NoiseExtensions = {}));
var NoiseHandshakePayload;
(function(NoiseHandshakePayload2) {
  let _codec;
  NoiseHandshakePayload2.codec = () => {
    if (_codec == null) {
      _codec = message((obj, w, opts = {}) => {
        if (opts.lengthDelimited !== false) {
          w.fork();
        }
        if (opts.writeDefaults === true || obj.identityKey != null && obj.identityKey.byteLength > 0) {
          w.uint32(10);
          w.bytes(obj.identityKey ?? new Uint8Array(0));
        }
        if (opts.writeDefaults === true || obj.identitySig != null && obj.identitySig.byteLength > 0) {
          w.uint32(18);
          w.bytes(obj.identitySig ?? new Uint8Array(0));
        }
        if (obj.extensions != null) {
          w.uint32(34);
          NoiseExtensions.codec().encode(obj.extensions, w, {
            writeDefaults: false
          });
        }
        if (opts.lengthDelimited !== false) {
          w.ldelim();
        }
      }, (reader2, length5) => {
        const obj = {
          identityKey: new Uint8Array(0),
          identitySig: new Uint8Array(0)
        };
        const end = length5 == null ? reader2.len : reader2.pos + length5;
        while (reader2.pos < end) {
          const tag = reader2.uint32();
          switch (tag >>> 3) {
            case 1:
              obj.identityKey = reader2.bytes();
              break;
            case 2:
              obj.identitySig = reader2.bytes();
              break;
            case 4:
              obj.extensions = NoiseExtensions.codec().decode(reader2, reader2.uint32());
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
  NoiseHandshakePayload2.encode = (obj) => {
    return encodeMessage(obj, NoiseHandshakePayload2.codec());
  };
  NoiseHandshakePayload2.decode = (buf) => {
    return decodeMessage(buf, NoiseHandshakePayload2.codec());
  };
})(NoiseHandshakePayload || (NoiseHandshakePayload = {}));

// ../../../node_modules/@chainsafe/libp2p-noise/dist/src/utils.js
async function getPayload(localPeer, staticPublicKey, extensions) {
  const signedPayload = await signPayload(localPeer, getHandshakePayload(staticPublicKey));
  if (localPeer.publicKey == null) {
    throw new Error("PublicKey was missing from local PeerId");
  }
  return createHandshakePayload(localPeer.publicKey, signedPayload, extensions);
}
function createHandshakePayload(libp2pPublicKey, signedPayload, extensions) {
  return NoiseHandshakePayload.encode({
    identityKey: libp2pPublicKey,
    identitySig: signedPayload,
    extensions: extensions ?? { webtransportCerthashes: [] }
  }).subarray();
}
async function signPayload(peerId, payload) {
  if (peerId.privateKey == null) {
    throw new Error("PrivateKey was missing from PeerId");
  }
  const privateKey = await unmarshalPrivateKey2(peerId.privateKey);
  return privateKey.sign(payload);
}
async function getPeerIdFromPayload(payload) {
  return peerIdFromKeys(payload.identityKey);
}
function decodePayload(payload) {
  return NoiseHandshakePayload.decode(payload);
}
function getHandshakePayload(publicKey) {
  const prefix = fromString4("noise-libp2p-static-key:");
  return concat([prefix, publicKey], prefix.length + publicKey.length);
}
async function verifySignedPayload(noiseStaticKey, payload, remotePeer) {
  const payloadPeerId = await peerIdFromKeys(payload.identityKey);
  if (!payloadPeerId.equals(remotePeer)) {
    throw new Error(`Payload identity key ${payloadPeerId.toString()} does not match expected remote peer ${remotePeer.toString()}`);
  }
  const generatedPayload = getHandshakePayload(noiseStaticKey);
  if (payloadPeerId.publicKey == null) {
    throw new Error("PublicKey was missing from PeerId");
  }
  if (payload.identitySig == null) {
    throw new Error("Signature was missing from message");
  }
  const publicKey = unmarshalPublicKey(payloadPeerId.publicKey);
  const valid = await publicKey.verify(generatedPayload, payload.identitySig);
  if (!valid) {
    throw new Error("Static key doesn't match to peer that signed payload!");
  }
  return payloadPeerId;
}
function isValidPublicKey(pk) {
  if (!(pk instanceof Uint8Array)) {
    return false;
  }
  if (pk.length !== 32) {
    return false;
  }
  return true;
}

// ../../../node_modules/@chainsafe/libp2p-noise/dist/src/logger.js
var log7 = logger("libp2p:noise");
var keyLogger;
if (DUMP_SESSION_KEYS) {
  keyLogger = log7;
} else {
  keyLogger = Object.assign(() => {
  }, {
    enabled: false,
    trace: () => {
    },
    error: () => {
    }
  });
}
function logLocalStaticKeys(s) {
  keyLogger(`LOCAL_STATIC_PUBLIC_KEY ${toString4(s.publicKey, "hex")}`);
  keyLogger(`LOCAL_STATIC_PRIVATE_KEY ${toString4(s.privateKey, "hex")}`);
}
function logLocalEphemeralKeys(e) {
  if (e) {
    keyLogger(`LOCAL_PUBLIC_EPHEMERAL_KEY ${toString4(e.publicKey, "hex")}`);
    keyLogger(`LOCAL_PRIVATE_EPHEMERAL_KEY ${toString4(e.privateKey, "hex")}`);
  } else {
    keyLogger("Missing local ephemeral keys.");
  }
}
function logRemoteStaticKey(rs) {
  keyLogger(`REMOTE_STATIC_PUBLIC_KEY ${toString4(rs, "hex")}`);
}
function logRemoteEphemeralKey(re) {
  keyLogger(`REMOTE_EPHEMERAL_PUBLIC_KEY ${toString4(re, "hex")}`);
}
function logCipherState(session) {
  if (session.cs1 && session.cs2) {
    keyLogger(`CIPHER_STATE_1 ${session.cs1.n.getUint64()} ${toString4(session.cs1.k, "hex")}`);
    keyLogger(`CIPHER_STATE_2 ${session.cs2.n.getUint64()} ${toString4(session.cs2.k, "hex")}`);
  } else {
    keyLogger("Missing cipher state.");
  }
}

// ../../../node_modules/@chainsafe/libp2p-noise/dist/src/nonce.js
var MIN_NONCE = 0;
var MAX_NONCE = 4294967295;
var ERR_MAX_NONCE = "Cipherstate has reached maximum n, a new handshake must be performed";
var Nonce = class {
  n;
  bytes;
  view;
  constructor(n = MIN_NONCE) {
    this.n = n;
    this.bytes = new Uint8Array(12);
    this.view = new DataView(this.bytes.buffer, this.bytes.byteOffset, this.bytes.byteLength);
    this.view.setUint32(4, n, true);
  }
  increment() {
    this.n++;
    this.view.setUint32(4, this.n, true);
  }
  getBytes() {
    return this.bytes;
  }
  getUint64() {
    return this.n;
  }
  assertValue() {
    if (this.n > MAX_NONCE) {
      throw new Error(ERR_MAX_NONCE);
    }
  }
};

// ../../../node_modules/@chainsafe/libp2p-noise/dist/src/handshakes/abstract-handshake.js
var AbstractHandshake = class {
  crypto;
  constructor(crypto5) {
    this.crypto = crypto5;
  }
  encryptWithAd(cs, ad, plaintext) {
    const e = this.encrypt(cs.k, cs.n, ad, plaintext);
    cs.n.increment();
    return e;
  }
  decryptWithAd(cs, ad, ciphertext, dst) {
    const { plaintext, valid } = this.decrypt(cs.k, cs.n, ad, ciphertext, dst);
    if (valid)
      cs.n.increment();
    return { plaintext, valid };
  }
  // Cipher state related
  hasKey(cs) {
    return !this.isEmptyKey(cs.k);
  }
  createEmptyKey() {
    return new Uint8Array(32);
  }
  isEmptyKey(k) {
    const emptyKey = this.createEmptyKey();
    return equals3(emptyKey, k);
  }
  encrypt(k, n, ad, plaintext) {
    n.assertValue();
    return this.crypto.chaCha20Poly1305Encrypt(plaintext, n.getBytes(), ad, k);
  }
  encryptAndHash(ss, plaintext) {
    let ciphertext;
    if (this.hasKey(ss.cs)) {
      ciphertext = this.encryptWithAd(ss.cs, ss.h, plaintext);
    } else {
      ciphertext = plaintext;
    }
    this.mixHash(ss, ciphertext);
    return ciphertext;
  }
  decrypt(k, n, ad, ciphertext, dst) {
    n.assertValue();
    const encryptedMessage = this.crypto.chaCha20Poly1305Decrypt(ciphertext, n.getBytes(), ad, k, dst);
    if (encryptedMessage) {
      return {
        plaintext: encryptedMessage,
        valid: true
      };
    } else {
      return {
        plaintext: new Uint8Array(0),
        valid: false
      };
    }
  }
  decryptAndHash(ss, ciphertext) {
    let plaintext;
    let valid = true;
    if (this.hasKey(ss.cs)) {
      ({ plaintext, valid } = this.decryptWithAd(ss.cs, ss.h, ciphertext));
    } else {
      plaintext = ciphertext;
    }
    this.mixHash(ss, ciphertext);
    return { plaintext, valid };
  }
  dh(privateKey, publicKey) {
    try {
      const derivedU8 = this.crypto.generateX25519SharedKey(privateKey, publicKey);
      if (derivedU8.length === 32) {
        return derivedU8;
      }
      return derivedU8.subarray(0, 32);
    } catch (e) {
      const err = e;
      log7.error(err);
      return new Uint8Array(32);
    }
  }
  mixHash(ss, data) {
    ss.h = this.getHash(ss.h, data);
  }
  getHash(a, b) {
    const u = this.crypto.hashSHA256(concat([a, b], a.length + b.length));
    return u;
  }
  mixKey(ss, ikm) {
    const [ck, tempK] = this.crypto.getHKDF(ss.ck, ikm);
    ss.cs = this.initializeKey(tempK);
    ss.ck = ck;
  }
  initializeKey(k) {
    return { k, n: new Nonce() };
  }
  // Symmetric state related
  initializeSymmetric(protocolName) {
    const protocolNameBytes = fromString4(protocolName, "utf-8");
    const h = this.hashProtocolName(protocolNameBytes);
    const ck = h;
    const key = this.createEmptyKey();
    const cs = this.initializeKey(key);
    return { cs, ck, h };
  }
  hashProtocolName(protocolName) {
    if (protocolName.length <= 32) {
      const h = new Uint8Array(32);
      h.set(protocolName);
      return h;
    } else {
      return this.getHash(protocolName, new Uint8Array(0));
    }
  }
  split(ss) {
    const [tempk1, tempk2] = this.crypto.getHKDF(ss.ck, new Uint8Array(0));
    const cs1 = this.initializeKey(tempk1);
    const cs2 = this.initializeKey(tempk2);
    return { cs1, cs2 };
  }
  writeMessageRegular(cs, payload) {
    const ciphertext = this.encryptWithAd(cs, new Uint8Array(0), payload);
    const ne = this.createEmptyKey();
    const ns = new Uint8Array(0);
    return { ne, ns, ciphertext };
  }
  readMessageRegular(cs, message2) {
    return this.decryptWithAd(cs, new Uint8Array(0), message2.ciphertext);
  }
};

// ../../../node_modules/@chainsafe/libp2p-noise/dist/src/handshakes/xx.js
var XX = class extends AbstractHandshake {
  initializeInitiator(prologue, s, rs, psk) {
    const name5 = "Noise_XX_25519_ChaChaPoly_SHA256";
    const ss = this.initializeSymmetric(name5);
    this.mixHash(ss, prologue);
    const re = new Uint8Array(32);
    return { ss, s, rs, psk, re };
  }
  initializeResponder(prologue, s, rs, psk) {
    const name5 = "Noise_XX_25519_ChaChaPoly_SHA256";
    const ss = this.initializeSymmetric(name5);
    this.mixHash(ss, prologue);
    const re = new Uint8Array(32);
    return { ss, s, rs, psk, re };
  }
  writeMessageA(hs, payload, e) {
    const ns = new Uint8Array(0);
    if (e !== void 0) {
      hs.e = e;
    } else {
      hs.e = this.crypto.generateX25519KeyPair();
    }
    const ne = hs.e.publicKey;
    this.mixHash(hs.ss, ne);
    const ciphertext = this.encryptAndHash(hs.ss, payload);
    return { ne, ns, ciphertext };
  }
  writeMessageB(hs, payload) {
    hs.e = this.crypto.generateX25519KeyPair();
    const ne = hs.e.publicKey;
    this.mixHash(hs.ss, ne);
    this.mixKey(hs.ss, this.dh(hs.e.privateKey, hs.re));
    const spk = hs.s.publicKey;
    const ns = this.encryptAndHash(hs.ss, spk);
    this.mixKey(hs.ss, this.dh(hs.s.privateKey, hs.re));
    const ciphertext = this.encryptAndHash(hs.ss, payload);
    return { ne, ns, ciphertext };
  }
  writeMessageC(hs, payload) {
    const spk = hs.s.publicKey;
    const ns = this.encryptAndHash(hs.ss, spk);
    this.mixKey(hs.ss, this.dh(hs.s.privateKey, hs.re));
    const ciphertext = this.encryptAndHash(hs.ss, payload);
    const ne = this.createEmptyKey();
    const messageBuffer = { ne, ns, ciphertext };
    const { cs1, cs2 } = this.split(hs.ss);
    return { h: hs.ss.h, messageBuffer, cs1, cs2 };
  }
  readMessageA(hs, message2) {
    if (isValidPublicKey(message2.ne)) {
      hs.re = message2.ne;
    }
    this.mixHash(hs.ss, hs.re);
    return this.decryptAndHash(hs.ss, message2.ciphertext);
  }
  readMessageB(hs, message2) {
    if (isValidPublicKey(message2.ne)) {
      hs.re = message2.ne;
    }
    this.mixHash(hs.ss, hs.re);
    if (!hs.e) {
      throw new Error("Handshake state `e` param is missing.");
    }
    this.mixKey(hs.ss, this.dh(hs.e.privateKey, hs.re));
    const { plaintext: ns, valid: valid1 } = this.decryptAndHash(hs.ss, message2.ns);
    if (valid1 && isValidPublicKey(ns)) {
      hs.rs = ns;
    }
    this.mixKey(hs.ss, this.dh(hs.e.privateKey, hs.rs));
    const { plaintext, valid: valid2 } = this.decryptAndHash(hs.ss, message2.ciphertext);
    return { plaintext, valid: valid1 && valid2 };
  }
  readMessageC(hs, message2) {
    const { plaintext: ns, valid: valid1 } = this.decryptAndHash(hs.ss, message2.ns);
    if (valid1 && isValidPublicKey(ns)) {
      hs.rs = ns;
    }
    if (!hs.e) {
      throw new Error("Handshake state `e` param is missing.");
    }
    this.mixKey(hs.ss, this.dh(hs.e.privateKey, hs.rs));
    const { plaintext, valid: valid2 } = this.decryptAndHash(hs.ss, message2.ciphertext);
    const { cs1, cs2 } = this.split(hs.ss);
    return { h: hs.ss.h, plaintext, valid: valid1 && valid2, cs1, cs2 };
  }
  initSession(initiator, prologue, s) {
    const psk = this.createEmptyKey();
    const rs = new Uint8Array(32);
    let hs;
    if (initiator) {
      hs = this.initializeInitiator(prologue, s, rs, psk);
    } else {
      hs = this.initializeResponder(prologue, s, rs, psk);
    }
    return {
      hs,
      i: initiator,
      mc: 0
    };
  }
  sendMessage(session, message2, ephemeral) {
    let messageBuffer;
    if (session.mc === 0) {
      messageBuffer = this.writeMessageA(session.hs, message2, ephemeral);
    } else if (session.mc === 1) {
      messageBuffer = this.writeMessageB(session.hs, message2);
    } else if (session.mc === 2) {
      const { h, messageBuffer: resultingBuffer, cs1, cs2 } = this.writeMessageC(session.hs, message2);
      messageBuffer = resultingBuffer;
      session.h = h;
      session.cs1 = cs1;
      session.cs2 = cs2;
    } else if (session.mc > 2) {
      if (session.i) {
        if (!session.cs1) {
          throw new Error("CS1 (cipher state) is not defined");
        }
        messageBuffer = this.writeMessageRegular(session.cs1, message2);
      } else {
        if (!session.cs2) {
          throw new Error("CS2 (cipher state) is not defined");
        }
        messageBuffer = this.writeMessageRegular(session.cs2, message2);
      }
    } else {
      throw new Error("Session invalid.");
    }
    session.mc++;
    return messageBuffer;
  }
  recvMessage(session, message2) {
    let plaintext = new Uint8Array(0);
    let valid = false;
    if (session.mc === 0) {
      ({ plaintext, valid } = this.readMessageA(session.hs, message2));
    } else if (session.mc === 1) {
      ({ plaintext, valid } = this.readMessageB(session.hs, message2));
    } else if (session.mc === 2) {
      const { h, plaintext: resultingPlaintext, valid: resultingValid, cs1, cs2 } = this.readMessageC(session.hs, message2);
      plaintext = resultingPlaintext;
      valid = resultingValid;
      session.h = h;
      session.cs1 = cs1;
      session.cs2 = cs2;
    }
    session.mc++;
    return { plaintext, valid };
  }
};

// ../../../node_modules/@chainsafe/libp2p-noise/dist/src/handshake-xx.js
var XXHandshake = class {
  isInitiator;
  session;
  remotePeer;
  remoteExtensions = { webtransportCerthashes: [] };
  payload;
  connection;
  xx;
  staticKeypair;
  prologue;
  constructor(isInitiator, payload, prologue, crypto5, staticKeypair, connection, remotePeer, handshake) {
    this.isInitiator = isInitiator;
    this.payload = payload;
    this.prologue = prologue;
    this.staticKeypair = staticKeypair;
    this.connection = connection;
    if (remotePeer) {
      this.remotePeer = remotePeer;
    }
    this.xx = handshake ?? new XX(crypto5);
    this.session = this.xx.initSession(this.isInitiator, this.prologue, this.staticKeypair);
  }
  // stage 0
  async propose() {
    logLocalStaticKeys(this.session.hs.s);
    if (this.isInitiator) {
      log7.trace("Stage 0 - Initiator starting to send first message.");
      const messageBuffer = this.xx.sendMessage(this.session, new Uint8Array(0));
      this.connection.writeLP(encode0(messageBuffer));
      log7.trace("Stage 0 - Initiator finished sending first message.");
      logLocalEphemeralKeys(this.session.hs.e);
    } else {
      log7.trace("Stage 0 - Responder waiting to receive first message...");
      const receivedMessageBuffer = decode0((await this.connection.readLP()).subarray());
      const { valid } = this.xx.recvMessage(this.session, receivedMessageBuffer);
      if (!valid) {
        throw new InvalidCryptoExchangeError("xx handshake stage 0 validation fail");
      }
      log7.trace("Stage 0 - Responder received first message.");
      logRemoteEphemeralKey(this.session.hs.re);
    }
  }
  // stage 1
  async exchange() {
    if (this.isInitiator) {
      log7.trace("Stage 1 - Initiator waiting to receive first message from responder...");
      const receivedMessageBuffer = decode1((await this.connection.readLP()).subarray());
      const { plaintext, valid } = this.xx.recvMessage(this.session, receivedMessageBuffer);
      if (!valid) {
        throw new InvalidCryptoExchangeError("xx handshake stage 1 validation fail");
      }
      log7.trace("Stage 1 - Initiator received the message.");
      logRemoteEphemeralKey(this.session.hs.re);
      logRemoteStaticKey(this.session.hs.rs);
      log7.trace("Initiator going to check remote's signature...");
      try {
        const decodedPayload = decodePayload(plaintext);
        this.remotePeer = this.remotePeer || await getPeerIdFromPayload(decodedPayload);
        await verifySignedPayload(this.session.hs.rs, decodedPayload, this.remotePeer);
        this.setRemoteNoiseExtension(decodedPayload.extensions);
      } catch (e) {
        const err = e;
        throw new UnexpectedPeerError(`Error occurred while verifying signed payload: ${err.message}`);
      }
      log7.trace("All good with the signature!");
    } else {
      log7.trace("Stage 1 - Responder sending out first message with signed payload and static key.");
      const messageBuffer = this.xx.sendMessage(this.session, this.payload);
      this.connection.writeLP(encode1(messageBuffer));
      log7.trace("Stage 1 - Responder sent the second handshake message with signed payload.");
      logLocalEphemeralKeys(this.session.hs.e);
    }
  }
  // stage 2
  async finish() {
    if (this.isInitiator) {
      log7.trace("Stage 2 - Initiator sending third handshake message.");
      const messageBuffer = this.xx.sendMessage(this.session, this.payload);
      this.connection.writeLP(encode22(messageBuffer));
      log7.trace("Stage 2 - Initiator sent message with signed payload.");
    } else {
      log7.trace("Stage 2 - Responder waiting for third handshake message...");
      const receivedMessageBuffer = decode22((await this.connection.readLP()).subarray());
      const { plaintext, valid } = this.xx.recvMessage(this.session, receivedMessageBuffer);
      if (!valid) {
        throw new InvalidCryptoExchangeError("xx handshake stage 2 validation fail");
      }
      log7.trace("Stage 2 - Responder received the message, finished handshake.");
      try {
        const decodedPayload = decodePayload(plaintext);
        this.remotePeer = this.remotePeer || await getPeerIdFromPayload(decodedPayload);
        await verifySignedPayload(this.session.hs.rs, decodedPayload, this.remotePeer);
        this.setRemoteNoiseExtension(decodedPayload.extensions);
      } catch (e) {
        const err = e;
        throw new UnexpectedPeerError(`Error occurred while verifying signed payload: ${err.message}`);
      }
    }
    logCipherState(this.session);
  }
  encrypt(plaintext, session) {
    const cs = this.getCS(session);
    return this.xx.encryptWithAd(cs, new Uint8Array(0), plaintext);
  }
  decrypt(ciphertext, session, dst) {
    const cs = this.getCS(session, false);
    return this.xx.decryptWithAd(cs, new Uint8Array(0), ciphertext, dst);
  }
  getRemoteStaticKey() {
    return this.session.hs.rs;
  }
  getCS(session, encryption = true) {
    if (!session.cs1 || !session.cs2) {
      throw new InvalidCryptoExchangeError("Handshake not completed properly, cipher state does not exist.");
    }
    if (this.isInitiator) {
      return encryption ? session.cs1 : session.cs2;
    } else {
      return encryption ? session.cs2 : session.cs1;
    }
  }
  setRemoteNoiseExtension(e) {
    if (e) {
      this.remoteExtensions = e;
    }
  }
};

// ../../../node_modules/@chainsafe/libp2p-noise/dist/src/metrics.js
function registerMetrics(metrics) {
  return {
    xxHandshakeSuccesses: metrics.registerCounter("libp2p_noise_xxhandshake_successes_total", {
      help: "Total count of noise xxHandshakes successes_"
    }),
    xxHandshakeErrors: metrics.registerCounter("libp2p_noise_xxhandshake_error_total", {
      help: "Total count of noise xxHandshakes errors"
    }),
    encryptedPackets: metrics.registerCounter("libp2p_noise_encrypted_packets_total", {
      help: "Total count of noise encrypted packets successfully"
    }),
    decryptedPackets: metrics.registerCounter("libp2p_noise_decrypted_packets_total", {
      help: "Total count of noise decrypted packets"
    }),
    decryptErrors: metrics.registerCounter("libp2p_noise_decrypt_errors_total", {
      help: "Total count of noise decrypt errors"
    })
  };
}

// ../../../node_modules/@chainsafe/libp2p-noise/dist/src/noise.js
var Noise = class {
  protocol = "/noise";
  crypto;
  prologue;
  staticKeys;
  extensions;
  metrics;
  constructor(init = {}) {
    const { staticNoiseKey, extensions, crypto: crypto5, prologueBytes, metrics } = init;
    this.crypto = crypto5 ?? pureJsCrypto;
    this.extensions = extensions;
    this.metrics = metrics ? registerMetrics(metrics) : void 0;
    if (staticNoiseKey) {
      this.staticKeys = this.crypto.generateX25519KeyPairFromSeed(staticNoiseKey);
    } else {
      this.staticKeys = this.crypto.generateX25519KeyPair();
    }
    this.prologue = prologueBytes ?? new Uint8Array(0);
  }
  /**
   * Encrypt outgoing data to the remote party (handshake as initiator)
   *
   * @param {PeerId} localPeer - PeerId of the receiving peer
   * @param {Duplex<AsyncGenerator<Uint8Array>, AsyncIterable<Uint8Array>, Promise<void>>} connection - streaming iterable duplex that will be encrypted
   * @param {PeerId} remotePeer - PeerId of the remote peer. Used to validate the integrity of the remote peer.
   * @returns {Promise<SecuredConnection>}
   */
  async secureOutbound(localPeer, connection, remotePeer) {
    const wrappedConnection = pbStream(connection, {
      lengthEncoder: uint16BEEncode,
      lengthDecoder: uint16BEDecode,
      maxDataLength: NOISE_MSG_MAX_LENGTH_BYTES
    });
    const handshake = await this.performHandshake({
      connection: wrappedConnection,
      isInitiator: true,
      localPeer,
      remotePeer
    });
    const conn = await this.createSecureConnection(wrappedConnection, handshake);
    return {
      conn,
      remoteExtensions: handshake.remoteExtensions,
      remotePeer: handshake.remotePeer
    };
  }
  /**
   * Decrypt incoming data (handshake as responder).
   *
   * @param {PeerId} localPeer - PeerId of the receiving peer.
   * @param {Duplex<AsyncGenerator<Uint8Array>, AsyncIterable<Uint8Array>, Promise<void>>} connection - streaming iterable duplex that will be encryption.
   * @param {PeerId} remotePeer - optional PeerId of the initiating peer, if known. This may only exist during transport upgrades.
   * @returns {Promise<SecuredConnection>}
   */
  async secureInbound(localPeer, connection, remotePeer) {
    const wrappedConnection = pbStream(connection, {
      lengthEncoder: uint16BEEncode,
      lengthDecoder: uint16BEDecode,
      maxDataLength: NOISE_MSG_MAX_LENGTH_BYTES
    });
    const handshake = await this.performHandshake({
      connection: wrappedConnection,
      isInitiator: false,
      localPeer,
      remotePeer
    });
    const conn = await this.createSecureConnection(wrappedConnection, handshake);
    return {
      conn,
      remotePeer: handshake.remotePeer,
      remoteExtensions: handshake.remoteExtensions
    };
  }
  /**
   * If Noise pipes supported, tries IK handshake first with XX as fallback if it fails.
   * If noise pipes disabled or remote peer static key is unknown, use XX.
   *
   * @param {HandshakeParams} params
   */
  async performHandshake(params) {
    const payload = await getPayload(params.localPeer, this.staticKeys.publicKey, this.extensions);
    return this.performXXHandshake(params, payload);
  }
  async performXXHandshake(params, payload) {
    const { isInitiator, remotePeer, connection } = params;
    const handshake = new XXHandshake(isInitiator, payload, this.prologue, this.crypto, this.staticKeys, connection, remotePeer);
    try {
      await handshake.propose();
      await handshake.exchange();
      await handshake.finish();
      this.metrics?.xxHandshakeSuccesses.increment();
    } catch (e) {
      this.metrics?.xxHandshakeErrors.increment();
      if (e instanceof Error) {
        e.message = `Error occurred during XX handshake: ${e.message}`;
        throw e;
      }
    }
    return handshake;
  }
  async createSecureConnection(connection, handshake) {
    const [secure, user] = duplexPair();
    const network = connection.unwrap();
    await pipe(
      secure,
      // write to wrapper
      encryptStream(handshake, this.metrics),
      // encrypt data + prefix with message length
      network,
      // send to the remote peer
      (source) => decode16(source, { lengthDecoder: uint16BEDecode }),
      // read message length prefix
      decryptStream(handshake, this.metrics),
      // decrypt the incoming data
      secure
      // pipe to the wrapper
    );
    return user;
  }
};

// ../../../node_modules/@chainsafe/libp2p-noise/dist/src/index.js
function noise(init = {}) {
  return () => new Noise(init);
}

// private-to-public/transport.ts
var multihashes2 = __toESM(require_src3(), 1);

// private-to-public/sdp.ts
var multihashes = __toESM(require_src3(), 1);
var log8 = logger("libp2p:webrtc:sdp");
var mbdecoder = Object.values(bases).map((b) => b.decoder).reduce((d, b) => d.or(b));
function getLocalFingerprint(pc) {
  const localCert = pc.getConfiguration().certificates?.at(0);
  if (localCert == null || localCert.getFingerprints == null) {
    log8.trace("fetching fingerprint from local SDP");
    const localDescription = pc.localDescription;
    if (localDescription == null) {
      return void 0;
    }
    return getFingerprintFromSdp(localDescription.sdp);
  }
  log8.trace("fetching fingerprint from local certificate");
  if (localCert.getFingerprints().length === 0) {
    return void 0;
  }
  const fingerprint = localCert.getFingerprints()[0].value;
  if (fingerprint == null) {
    throw invalidFingerprint("", "no fingerprint on local certificate");
  }
  return fingerprint;
}
var fingerprintRegex = /^a=fingerprint:(?:\w+-[0-9]+)\s(?<fingerprint>(:?[0-9a-fA-F]{2})+)$/m;
function getFingerprintFromSdp(sdp) {
  const searchResult = sdp.match(fingerprintRegex);
  return searchResult?.groups?.fingerprint;
}
function ipv(ma) {
  for (const proto of ma.protoNames()) {
    if (proto.startsWith("ip")) {
      return proto.toUpperCase();
    }
  }
  log8("Warning: multiaddr does not appear to contain IP4 or IP6, defaulting to IP6", ma);
  return "IP6";
}
function certhash(ma) {
  const tups = ma.stringTuples();
  const certhash2 = tups.filter((tup) => tup[0] === CERTHASH_CODE).map((tup) => tup[1])[0];
  if (certhash2 === void 0 || certhash2 === "") {
    throw inappropriateMultiaddr(`Couldn't find a certhash component of multiaddr: ${ma.toString()}`);
  }
  return certhash2;
}
function decodeCerthash(certhash2) {
  const mbdecoded = mbdecoder.decode(certhash2);
  return multihashes.decode(mbdecoded);
}
function ma2Fingerprint(ma) {
  const mhdecoded = decodeCerthash(certhash(ma));
  const prefix = toSupportedHashFunction(mhdecoded.name);
  const fingerprint = mhdecoded.digest.reduce((str, byte) => str + byte.toString(16).padStart(2, "0"), "");
  const sdp = fingerprint.match(/.{1,2}/g);
  if (sdp == null) {
    throw invalidFingerprint(fingerprint, ma.toString());
  }
  return [`${prefix.toUpperCase()} ${sdp.join(":").toUpperCase()}`, fingerprint];
}
function toSupportedHashFunction(name5) {
  switch (name5) {
    case "sha1":
      return "sha-1";
    case "sha2-256":
      return "sha-256";
    case "sha2-512":
      return "sha-512";
    default:
      throw unsupportedHashAlgorithm(name5);
  }
}
function ma2sdp(ma, ufrag) {
  const { host, port } = ma.toOptions();
  const ipVersion = ipv(ma);
  const [CERTFP] = ma2Fingerprint(ma);
  return `v=0
o=- 0 0 IN ${ipVersion} ${host}
s=-
c=IN ${ipVersion} ${host}
t=0 0
a=ice-lite
m=application ${port} UDP/DTLS/SCTP webrtc-datachannel
a=mid:0
a=setup:passive
a=ice-ufrag:${ufrag}
a=ice-pwd:${ufrag}
a=fingerprint:${CERTFP}
a=sctp-port:5000
a=max-message-size:100000
a=candidate:1467250027 1 UDP 1467250027 ${host} ${port} typ host\r
`;
}
function fromMultiAddr(ma, ufrag) {
  return {
    type: "answer",
    sdp: ma2sdp(ma, ufrag)
  };
}
function munge(desc, ufrag) {
  if (desc.sdp === void 0) {
    throw invalidArgument("Can't munge a missing SDP");
  }
  desc.sdp = desc.sdp.replace(/\na=ice-ufrag:[^\n]*\n/, "\na=ice-ufrag:" + ufrag + "\n").replace(/\na=ice-pwd:[^\n]*\n/, "\na=ice-pwd:" + ufrag + "\n");
  return desc;
}

// private-to-public/util.ts
var charset = Array.from("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/");
var genUfrag = (len) => [...Array(len)].map(() => charset.at(Math.floor(Math.random() * charset.length))).join("");

// private-to-public/transport.ts
var log9 = logger("libp2p:webrtc:transport");
var HANDSHAKE_TIMEOUT_MS = 1e4;
var WEBRTC_CODE2 = getProtocol("webrtc-direct").code;
var CERTHASH_CODE = getProtocol("certhash").code;
var WebRTCDirectTransport = class {
  metrics;
  components;
  init;
  constructor(components, init = {}) {
    this.components = components;
    this.init = init;
    if (components.metrics != null) {
      this.metrics = {
        dialerEvents: components.metrics.registerCounterGroup("libp2p_webrtc_dialer_events_total", {
          label: "event",
          help: "Total count of WebRTC dial events by type"
        })
      };
    }
  }
  /**
   * Dial a given multiaddr
   */
  async dial(ma, options) {
    const rawConn = await this._connect(ma, options);
    log9(`dialing address - ${ma.toString()}`);
    return rawConn;
  }
  /**
   * Create transport listeners no supported by browsers
   */
  createListener(options) {
    throw unimplemented("WebRTCTransport.createListener");
  }
  /**
   * Takes a list of `Multiaddr`s and returns only valid addresses for the transport
   */
  filter(multiaddrs) {
    return multiaddrs.filter(validMa);
  }
  /**
   * Implement toString() for WebRTCTransport
   */
  [Symbol.toStringTag] = "@libp2p/webrtc-direct";
  /**
   * Symbol.for('@libp2p/transport')
   */
  [symbol] = true;
  /**
   * Connect to a peer using a multiaddr
   */
  async _connect(ma, options) {
    const controller = new AbortController();
    const signal = controller.signal;
    const remotePeerString = ma.getPeerId();
    if (remotePeerString === null) {
      throw inappropriateMultiaddr("we need to have the remote's PeerId");
    }
    const theirPeerId = peerIdFromString(remotePeerString);
    const remoteCerthash = decodeCerthash(certhash(ma));
    const certificate = await RTCPeerConnection.generateCertificate({
      name: "ECDSA",
      namedCurve: "P-256",
      hash: toSupportedHashFunction(remoteCerthash.name)
    });
    const peerConnection = new RTCPeerConnection({ certificates: [certificate] });
    const dataChannelOpenPromise = new Promise((resolve, reject) => {
      const handshakeDataChannel2 = peerConnection.createDataChannel("", { negotiated: true, id: 0 });
      const handshakeTimeout = setTimeout(() => {
        const error = `Data channel was never opened: state: ${handshakeDataChannel2.readyState}`;
        log9.error(error);
        this.metrics?.dialerEvents.increment({ open_error: true });
        reject(dataChannelError("data", error));
      }, HANDSHAKE_TIMEOUT_MS);
      handshakeDataChannel2.onopen = (_) => {
        clearTimeout(handshakeTimeout);
        resolve(handshakeDataChannel2);
      };
      handshakeDataChannel2.onerror = (event) => {
        clearTimeout(handshakeTimeout);
        const errorTarget = event.target?.toString() ?? "not specified";
        const error = `Error opening a data channel for handshaking: ${errorTarget}`;
        log9.error(error);
        this.metrics?.dialerEvents.increment({ unknown_error: true });
        reject(dataChannelError("data", error));
      };
    });
    const ufrag = "libp2p+webrtc+v1/" + genUfrag(32);
    const offerSdp = await peerConnection.createOffer();
    const mungedOfferSdp = munge(offerSdp, ufrag);
    await peerConnection.setLocalDescription(mungedOfferSdp);
    const answerSdp = fromMultiAddr(ma, ufrag);
    await peerConnection.setRemoteDescription(answerSdp);
    const handshakeDataChannel = await dataChannelOpenPromise;
    const myPeerId = this.components.peerId;
    const fingerprintsPrologue = this.generateNoisePrologue(peerConnection, remoteCerthash.code, ma);
    const noise2 = noise({ prologueBytes: fingerprintsPrologue })();
    const wrappedChannel = createStream({ channel: handshakeDataChannel, direction: "inbound", dataChannelOptions: this.init.dataChannel });
    const wrappedDuplex = {
      ...wrappedChannel,
      sink: wrappedChannel.sink.bind(wrappedChannel),
      source: async function* () {
        for await (const list of wrappedChannel.source) {
          for (const buf of list) {
            yield buf;
          }
        }
      }()
    };
    const maConn = new WebRTCMultiaddrConnection({
      peerConnection,
      remoteAddr: ma,
      timeline: {
        open: Date.now()
      },
      metrics: this.metrics?.dialerEvents
    });
    const eventListeningName = isFirefox ? "iceconnectionstatechange" : "connectionstatechange";
    peerConnection.addEventListener(eventListeningName, () => {
      switch (peerConnection.connectionState) {
        case "failed":
        case "disconnected":
        case "closed":
          maConn.close().catch((err) => {
            log9.error("error closing connection", err);
          }).finally(() => {
            controller.abort();
          });
          break;
        default:
          break;
      }
    }, { signal });
    this.metrics?.dialerEvents.increment({ peer_connection: true });
    const muxerFactory = new DataChannelMuxerFactory({ peerConnection, metrics: this.metrics?.dialerEvents, dataChannelOptions: this.init.dataChannel });
    await noise2.secureInbound(myPeerId, wrappedDuplex, theirPeerId);
    return options.upgrader.upgradeOutbound(maConn, { skipProtection: true, skipEncryption: true, muxerFactory });
  }
  /**
   * Generate a noise prologue from the peer connection's certificate.
   * noise prologue = bytes('libp2p-webrtc-noise:') + noise-responder fingerprint + noise-initiator fingerprint
   */
  generateNoisePrologue(pc, hashCode, ma) {
    if (pc.getConfiguration().certificates?.length === 0) {
      throw invalidArgument("no local certificate");
    }
    const localFingerprint = getLocalFingerprint(pc);
    if (localFingerprint == null) {
      throw invalidArgument("no local fingerprint found");
    }
    const localFpString = localFingerprint.trim().toLowerCase().replaceAll(":", "");
    const localFpArray = fromString4(localFpString, "hex");
    const local = multihashes2.encode(localFpArray, hashCode);
    const remote = mbdecoder.decode(certhash(ma));
    const prefix = fromString4("libp2p-webrtc-noise:");
    return concat([prefix, local, remote]);
  }
};
function validMa(ma) {
  const codes3 = ma.protoCodes();
  return codes3.includes(WEBRTC_CODE2) && codes3.includes(CERTHASH_CODE) && ma.getPeerId() != null && !codes3.includes(getProtocol("p2p-circuit").code);
}

// index.ts
function webRTCDirect(init) {
  return (components) => new WebRTCDirectTransport(components, init);
}
function webRTC(init) {
  return (components) => new WebRTCTransport(components, init);
}
export {
  webRTC,
  webRTCDirect
};
/*! Bundled license information:

@noble/hashes/esm/utils.js:
  (*! noble-hashes - MIT License (c) 2022 Paul Miller (paulmillr.com) *)

@noble/ed25519/lib/esm/index.js:
  (*! noble-ed25519 - MIT License (c) 2019 Paul Miller (paulmillr.com) *)

@noble/secp256k1/lib/esm/index.js:
  (*! noble-secp256k1 - MIT License (c) 2019 Paul Miller (paulmillr.com) *)
*/
