var __create = Object.create;
var __defProp = Object.defineProperty;
var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __getProtoOf = Object.getPrototypeOf;
var __hasOwnProp = Object.prototype.hasOwnProperty;
var __commonJS = (cb, mod) => function __require() {
  return mod || (0, cb[__getOwnPropNames(cb)[0]])((mod = { exports: {} }).exports, mod), mod.exports;
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
var __toESM = (mod, isNodeMode, target) => (target = mod != null ? __create(__getProtoOf(mod)) : {}, __copyProps(
  // If the importer is in node compatibility mode or this is not an ESM
  // file that has been converted to a CommonJS file using a Babel-
  // compatible transform (i.e. "__esModule" has not been set), then set
  // "default" to the CommonJS "module.exports" for node compatibility.
  isNodeMode || !mod || !mod.__esModule ? __defProp(target, "default", { value: mod, enumerable: true }) : target,
  mod
));

// ../../../node_modules/ms/index.js
var require_ms = __commonJS({
  "../../../node_modules/ms/index.js"(exports, module) {
    var s = 1e3;
    var m = s * 60;
    var h = m * 60;
    var d = h * 24;
    var w = d * 7;
    var y = d * 365.25;
    module.exports = function(val, options) {
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
    function plural(ms, msAbs, n, name3) {
      var isPlural = msAbs >= n * 1.5;
      return Math.round(ms / n) + " " + name3 + (isPlural ? "s" : "");
    }
  }
});

// ../../../node_modules/debug/src/common.js
var require_common = __commonJS({
  "../../../node_modules/debug/src/common.js"(exports, module) {
    function setup(env) {
      createDebug.debug = createDebug;
      createDebug.default = createDebug;
      createDebug.coerce = coerce3;
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
        let hash = 0;
        for (let i = 0; i < namespace.length; i++) {
          hash = (hash << 5) - hash + namespace.charCodeAt(i);
          hash |= 0;
        }
        return createDebug.colors[Math.abs(hash) % createDebug.colors.length];
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
          const self = debug2;
          const curr = Number(/* @__PURE__ */ new Date());
          const ms = curr - (prevTime || curr);
          self.diff = ms;
          self.prev = prevTime;
          self.curr = curr;
          prevTime = curr;
          args[0] = createDebug.coerce(args[0]);
          if (typeof args[0] !== "string") {
            args.unshift("%O");
          }
          let index = 0;
          args[0] = args[0].replace(/%([a-zA-Z%])/g, (match, format3) => {
            if (match === "%%") {
              return "%";
            }
            index++;
            const formatter = createDebug.formatters[format3];
            if (typeof formatter === "function") {
              const val = args[index];
              match = formatter.call(self, val);
              args.splice(index, 1);
              index--;
            }
            return match;
          });
          createDebug.formatArgs.call(self, args);
          const logFn = self.log || createDebug.log;
          logFn.apply(self, args);
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
      function enabled(name3) {
        if (name3[name3.length - 1] === "*") {
          return true;
        }
        let i;
        let len;
        for (i = 0, len = createDebug.skips.length; i < len; i++) {
          if (createDebug.skips[i].test(name3)) {
            return false;
          }
        }
        for (i = 0, len = createDebug.names.length; i < len; i++) {
          if (createDebug.names[i].test(name3)) {
            return true;
          }
        }
        return false;
      }
      function toNamespace(regexp) {
        return regexp.toString().substring(2, regexp.toString().length - 2).replace(/\.\*\?$/, "*");
      }
      function coerce3(val) {
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
    module.exports = setup;
  }
});

// ../../../node_modules/debug/src/browser.js
var require_browser = __commonJS({
  "../../../node_modules/debug/src/browser.js"(exports, module) {
    exports.formatArgs = formatArgs;
    exports.save = save;
    exports.load = load;
    exports.useColors = useColors;
    exports.storage = localstorage();
    exports.destroy = (() => {
      let warned = false;
      return () => {
        if (!warned) {
          warned = true;
          console.warn("Instance method `debug.destroy()` is deprecated and no longer does anything. It will be removed in the next major version of `debug`.");
        }
      };
    })();
    exports.colors = [
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
      args[0] = (this.useColors ? "%c" : "") + this.namespace + (this.useColors ? " %c" : " ") + args[0] + (this.useColors ? "%c " : " ") + "+" + module.exports.humanize(this.diff);
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
    exports.log = console.debug || console.log || (() => {
    });
    function save(namespaces) {
      try {
        if (namespaces) {
          exports.storage.setItem("debug", namespaces);
        } else {
          exports.storage.removeItem("debug");
        }
      } catch (error) {
      }
    }
    function load() {
      let r;
      try {
        r = exports.storage.getItem("debug");
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
    module.exports = require_common()(exports);
    var { formatters } = module.exports;
    formatters.j = function(v) {
      try {
        return JSON.stringify(v);
      } catch (error) {
        return "[UnexpectedJSONParseError]: " + error.message;
      }
    };
  }
});

// ../../../node_modules/is-plain-obj/index.js
var require_is_plain_obj = __commonJS({
  "../../../node_modules/is-plain-obj/index.js"(exports, module) {
    "use strict";
    module.exports = (value) => {
      if (Object.prototype.toString.call(value) !== "[object Object]") {
        return false;
      }
      const prototype = Object.getPrototypeOf(value);
      return prototype === null || prototype === Object.prototype;
    };
  }
});

// ../../../node_modules/merge-options/index.js
var require_merge_options = __commonJS({
  "../../../node_modules/merge-options/index.js"(exports, module) {
    "use strict";
    var isOptionObject = require_is_plain_obj();
    var { hasOwnProperty } = Object.prototype;
    var { propertyIsEnumerable } = Object;
    var defineProperty = (object, name3, value) => Object.defineProperty(object, name3, {
      value,
      writable: true,
      enumerable: true,
      configurable: true
    });
    var globalThis2 = exports;
    var defaultMergeOptions = {
      concatArrays: false,
      ignoreUndefined: false
    };
    var getEnumerableOwnPropertyKeys = (value) => {
      const keys = [];
      for (const key in value) {
        if (hasOwnProperty.call(value, key)) {
          keys.push(key);
        }
      }
      if (Object.getOwnPropertySymbols) {
        const symbols = Object.getOwnPropertySymbols(value);
        for (const symbol5 of symbols) {
          if (propertyIsEnumerable.call(value, symbol5)) {
            keys.push(symbol5);
          }
        }
      }
      return keys;
    };
    function clone(value) {
      if (Array.isArray(value)) {
        return cloneArray(value);
      }
      if (isOptionObject(value)) {
        return cloneOptionObject(value);
      }
      return value;
    }
    function cloneArray(array) {
      const result = array.slice(0, 0);
      getEnumerableOwnPropertyKeys(array).forEach((key) => {
        defineProperty(result, key, clone(array[key]));
      });
      return result;
    }
    function cloneOptionObject(object) {
      const result = Object.getPrototypeOf(object) === null ? /* @__PURE__ */ Object.create(null) : {};
      getEnumerableOwnPropertyKeys(object).forEach((key) => {
        defineProperty(result, key, clone(object[key]));
      });
      return result;
    }
    var mergeKeys = (merged, source, keys, config) => {
      keys.forEach((key) => {
        if (typeof source[key] === "undefined" && config.ignoreUndefined) {
          return;
        }
        if (key in merged && merged[key] !== Object.getPrototypeOf(merged)) {
          defineProperty(merged, key, merge2(merged[key], source[key], config));
        } else {
          defineProperty(merged, key, clone(source[key]));
        }
      });
      return merged;
    };
    var concatArrays = (merged, source, config) => {
      let result = merged.slice(0, 0);
      let resultIndex = 0;
      [merged, source].forEach((array) => {
        const indices = [];
        for (let k = 0; k < array.length; k++) {
          if (!hasOwnProperty.call(array, k)) {
            continue;
          }
          indices.push(String(k));
          if (array === merged) {
            defineProperty(result, resultIndex++, array[k]);
          } else {
            defineProperty(result, resultIndex++, clone(array[k]));
          }
        }
        result = mergeKeys(result, array, getEnumerableOwnPropertyKeys(array).filter((key) => !indices.includes(key)), config);
      });
      return result;
    };
    function merge2(merged, source, config) {
      if (config.concatArrays && Array.isArray(merged) && Array.isArray(source)) {
        return concatArrays(merged, source, config);
      }
      if (!isOptionObject(source) || !isOptionObject(merged)) {
        return clone(source);
      }
      return mergeKeys(merged, source, getEnumerableOwnPropertyKeys(source), config);
    }
    module.exports = function(...options) {
      const config = merge2(clone(defaultMergeOptions), this !== globalThis2 && this || {}, defaultMergeOptions);
      let merged = { _: {} };
      for (const option of options) {
        if (option === void 0) {
          continue;
        }
        if (!isOptionObject(option)) {
          throw new TypeError("`" + option + "` is not an Option Object");
        }
        merged = merge2(merged, { _: option }, config);
      }
      return merged._;
    };
  }
});

// ../../../node_modules/events/events.js
var require_events = __commonJS({
  "../../../node_modules/events/events.js"(exports, module) {
    "use strict";
    var R = typeof Reflect === "object" ? Reflect : null;
    var ReflectApply = R && typeof R.apply === "function" ? R.apply : function ReflectApply2(target, receiver, args) {
      return Function.prototype.apply.call(target, receiver, args);
    };
    var ReflectOwnKeys;
    if (R && typeof R.ownKeys === "function") {
      ReflectOwnKeys = R.ownKeys;
    } else if (Object.getOwnPropertySymbols) {
      ReflectOwnKeys = function ReflectOwnKeys2(target) {
        return Object.getOwnPropertyNames(target).concat(Object.getOwnPropertySymbols(target));
      };
    } else {
      ReflectOwnKeys = function ReflectOwnKeys2(target) {
        return Object.getOwnPropertyNames(target);
      };
    }
    function ProcessEmitWarning(warning) {
      if (console && console.warn)
        console.warn(warning);
    }
    var NumberIsNaN = Number.isNaN || function NumberIsNaN2(value) {
      return value !== value;
    };
    function EventEmitter() {
      EventEmitter.init.call(this);
    }
    module.exports = EventEmitter;
    module.exports.once = once;
    EventEmitter.EventEmitter = EventEmitter;
    EventEmitter.prototype._events = void 0;
    EventEmitter.prototype._eventsCount = 0;
    EventEmitter.prototype._maxListeners = void 0;
    var defaultMaxListeners = 10;
    function checkListener(listener) {
      if (typeof listener !== "function") {
        throw new TypeError('The "listener" argument must be of type Function. Received type ' + typeof listener);
      }
    }
    Object.defineProperty(EventEmitter, "defaultMaxListeners", {
      enumerable: true,
      get: function() {
        return defaultMaxListeners;
      },
      set: function(arg) {
        if (typeof arg !== "number" || arg < 0 || NumberIsNaN(arg)) {
          throw new RangeError('The value of "defaultMaxListeners" is out of range. It must be a non-negative number. Received ' + arg + ".");
        }
        defaultMaxListeners = arg;
      }
    });
    EventEmitter.init = function() {
      if (this._events === void 0 || this._events === Object.getPrototypeOf(this)._events) {
        this._events = /* @__PURE__ */ Object.create(null);
        this._eventsCount = 0;
      }
      this._maxListeners = this._maxListeners || void 0;
    };
    EventEmitter.prototype.setMaxListeners = function setMaxListeners2(n) {
      if (typeof n !== "number" || n < 0 || NumberIsNaN(n)) {
        throw new RangeError('The value of "n" is out of range. It must be a non-negative number. Received ' + n + ".");
      }
      this._maxListeners = n;
      return this;
    };
    function _getMaxListeners(that) {
      if (that._maxListeners === void 0)
        return EventEmitter.defaultMaxListeners;
      return that._maxListeners;
    }
    EventEmitter.prototype.getMaxListeners = function getMaxListeners() {
      return _getMaxListeners(this);
    };
    EventEmitter.prototype.emit = function emit(type) {
      var args = [];
      for (var i = 1; i < arguments.length; i++)
        args.push(arguments[i]);
      var doError = type === "error";
      var events = this._events;
      if (events !== void 0)
        doError = doError && events.error === void 0;
      else if (!doError)
        return false;
      if (doError) {
        var er;
        if (args.length > 0)
          er = args[0];
        if (er instanceof Error) {
          throw er;
        }
        var err = new Error("Unhandled error." + (er ? " (" + er.message + ")" : ""));
        err.context = er;
        throw err;
      }
      var handler = events[type];
      if (handler === void 0)
        return false;
      if (typeof handler === "function") {
        ReflectApply(handler, this, args);
      } else {
        var len = handler.length;
        var listeners = arrayClone(handler, len);
        for (var i = 0; i < len; ++i)
          ReflectApply(listeners[i], this, args);
      }
      return true;
    };
    function _addListener(target, type, listener, prepend) {
      var m;
      var events;
      var existing;
      checkListener(listener);
      events = target._events;
      if (events === void 0) {
        events = target._events = /* @__PURE__ */ Object.create(null);
        target._eventsCount = 0;
      } else {
        if (events.newListener !== void 0) {
          target.emit(
            "newListener",
            type,
            listener.listener ? listener.listener : listener
          );
          events = target._events;
        }
        existing = events[type];
      }
      if (existing === void 0) {
        existing = events[type] = listener;
        ++target._eventsCount;
      } else {
        if (typeof existing === "function") {
          existing = events[type] = prepend ? [listener, existing] : [existing, listener];
        } else if (prepend) {
          existing.unshift(listener);
        } else {
          existing.push(listener);
        }
        m = _getMaxListeners(target);
        if (m > 0 && existing.length > m && !existing.warned) {
          existing.warned = true;
          var w = new Error("Possible EventEmitter memory leak detected. " + existing.length + " " + String(type) + " listeners added. Use emitter.setMaxListeners() to increase limit");
          w.name = "MaxListenersExceededWarning";
          w.emitter = target;
          w.type = type;
          w.count = existing.length;
          ProcessEmitWarning(w);
        }
      }
      return target;
    }
    EventEmitter.prototype.addListener = function addListener(type, listener) {
      return _addListener(this, type, listener, false);
    };
    EventEmitter.prototype.on = EventEmitter.prototype.addListener;
    EventEmitter.prototype.prependListener = function prependListener(type, listener) {
      return _addListener(this, type, listener, true);
    };
    function onceWrapper() {
      if (!this.fired) {
        this.target.removeListener(this.type, this.wrapFn);
        this.fired = true;
        if (arguments.length === 0)
          return this.listener.call(this.target);
        return this.listener.apply(this.target, arguments);
      }
    }
    function _onceWrap(target, type, listener) {
      var state = { fired: false, wrapFn: void 0, target, type, listener };
      var wrapped = onceWrapper.bind(state);
      wrapped.listener = listener;
      state.wrapFn = wrapped;
      return wrapped;
    }
    EventEmitter.prototype.once = function once2(type, listener) {
      checkListener(listener);
      this.on(type, _onceWrap(this, type, listener));
      return this;
    };
    EventEmitter.prototype.prependOnceListener = function prependOnceListener(type, listener) {
      checkListener(listener);
      this.prependListener(type, _onceWrap(this, type, listener));
      return this;
    };
    EventEmitter.prototype.removeListener = function removeListener(type, listener) {
      var list, events, position, i, originalListener;
      checkListener(listener);
      events = this._events;
      if (events === void 0)
        return this;
      list = events[type];
      if (list === void 0)
        return this;
      if (list === listener || list.listener === listener) {
        if (--this._eventsCount === 0)
          this._events = /* @__PURE__ */ Object.create(null);
        else {
          delete events[type];
          if (events.removeListener)
            this.emit("removeListener", type, list.listener || listener);
        }
      } else if (typeof list !== "function") {
        position = -1;
        for (i = list.length - 1; i >= 0; i--) {
          if (list[i] === listener || list[i].listener === listener) {
            originalListener = list[i].listener;
            position = i;
            break;
          }
        }
        if (position < 0)
          return this;
        if (position === 0)
          list.shift();
        else {
          spliceOne(list, position);
        }
        if (list.length === 1)
          events[type] = list[0];
        if (events.removeListener !== void 0)
          this.emit("removeListener", type, originalListener || listener);
      }
      return this;
    };
    EventEmitter.prototype.off = EventEmitter.prototype.removeListener;
    EventEmitter.prototype.removeAllListeners = function removeAllListeners(type) {
      var listeners, events, i;
      events = this._events;
      if (events === void 0)
        return this;
      if (events.removeListener === void 0) {
        if (arguments.length === 0) {
          this._events = /* @__PURE__ */ Object.create(null);
          this._eventsCount = 0;
        } else if (events[type] !== void 0) {
          if (--this._eventsCount === 0)
            this._events = /* @__PURE__ */ Object.create(null);
          else
            delete events[type];
        }
        return this;
      }
      if (arguments.length === 0) {
        var keys = Object.keys(events);
        var key;
        for (i = 0; i < keys.length; ++i) {
          key = keys[i];
          if (key === "removeListener")
            continue;
          this.removeAllListeners(key);
        }
        this.removeAllListeners("removeListener");
        this._events = /* @__PURE__ */ Object.create(null);
        this._eventsCount = 0;
        return this;
      }
      listeners = events[type];
      if (typeof listeners === "function") {
        this.removeListener(type, listeners);
      } else if (listeners !== void 0) {
        for (i = listeners.length - 1; i >= 0; i--) {
          this.removeListener(type, listeners[i]);
        }
      }
      return this;
    };
    function _listeners(target, type, unwrap) {
      var events = target._events;
      if (events === void 0)
        return [];
      var evlistener = events[type];
      if (evlistener === void 0)
        return [];
      if (typeof evlistener === "function")
        return unwrap ? [evlistener.listener || evlistener] : [evlistener];
      return unwrap ? unwrapListeners(evlistener) : arrayClone(evlistener, evlistener.length);
    }
    EventEmitter.prototype.listeners = function listeners(type) {
      return _listeners(this, type, true);
    };
    EventEmitter.prototype.rawListeners = function rawListeners(type) {
      return _listeners(this, type, false);
    };
    EventEmitter.listenerCount = function(emitter, type) {
      if (typeof emitter.listenerCount === "function") {
        return emitter.listenerCount(type);
      } else {
        return listenerCount.call(emitter, type);
      }
    };
    EventEmitter.prototype.listenerCount = listenerCount;
    function listenerCount(type) {
      var events = this._events;
      if (events !== void 0) {
        var evlistener = events[type];
        if (typeof evlistener === "function") {
          return 1;
        } else if (evlistener !== void 0) {
          return evlistener.length;
        }
      }
      return 0;
    }
    EventEmitter.prototype.eventNames = function eventNames() {
      return this._eventsCount > 0 ? ReflectOwnKeys(this._events) : [];
    };
    function arrayClone(arr, n) {
      var copy = new Array(n);
      for (var i = 0; i < n; ++i)
        copy[i] = arr[i];
      return copy;
    }
    function spliceOne(list, index) {
      for (; index + 1 < list.length; index++)
        list[index] = list[index + 1];
      list.pop();
    }
    function unwrapListeners(arr) {
      var ret = new Array(arr.length);
      for (var i = 0; i < ret.length; ++i) {
        ret[i] = arr[i].listener || arr[i];
      }
      return ret;
    }
    function once(emitter, name3) {
      return new Promise(function(resolve, reject) {
        function errorListener(err) {
          emitter.removeListener(name3, resolver);
          reject(err);
        }
        function resolver() {
          if (typeof emitter.removeListener === "function") {
            emitter.removeListener("error", errorListener);
          }
          resolve([].slice.call(arguments));
        }
        ;
        eventTargetAgnosticAddListener(emitter, name3, resolver, { once: true });
        if (name3 !== "error") {
          addErrorHandlerIfEventEmitter(emitter, errorListener, { once: true });
        }
      });
    }
    function addErrorHandlerIfEventEmitter(emitter, handler, flags) {
      if (typeof emitter.on === "function") {
        eventTargetAgnosticAddListener(emitter, "error", handler, flags);
      }
    }
    function eventTargetAgnosticAddListener(emitter, name3, listener, flags) {
      if (typeof emitter.on === "function") {
        if (flags.once) {
          emitter.once(name3, listener);
        } else {
          emitter.on(name3, listener);
        }
      } else if (typeof emitter.addEventListener === "function") {
        emitter.addEventListener(name3, function wrapListener(arg) {
          if (flags.once) {
            emitter.removeEventListener(name3, wrapListener);
          }
          listener(arg);
        });
      } else {
        throw new TypeError('The "emitter" argument must be of type EventEmitter. Received type ' + typeof emitter);
      }
    }
  }
});

// ../../../node_modules/err-code/index.js
var require_err_code = __commonJS({
  "../../../node_modules/err-code/index.js"(exports, module) {
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
    function createError(err, code3, props) {
      if (!err || typeof err === "string") {
        throw new TypeError("Please pass an Error to err-code");
      }
      if (!props) {
        props = {};
      }
      if (typeof code3 === "object") {
        props = code3;
        code3 = "";
      }
      if (code3) {
        props.code = code3;
      }
      try {
        return assign(err, props);
      } catch (_) {
        props.message = err.message;
        props.stack = err.stack;
        const ErrClass = function() {
        };
        ErrClass.prototype = Object.create(Object.getPrototypeOf(err));
        const output = assign(new ErrClass(), props);
        return output;
      }
    }
    module.exports = createError;
  }
});

// ../../interface/dist/src/errors.js
var CodeError = class extends Error {
  code;
  props;
  constructor(message, code3, props) {
    super(message);
    this.code = code3;
    this.name = props?.name ?? "CodeError";
    this.props = props ?? {};
  }
};

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
  function encode11(source) {
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
  function decode12(string2) {
    var buffer = decodeUnsafe(string2);
    if (buffer) {
      return buffer;
    }
    throw new Error(`Non-${name3} character`);
  }
  return {
    encode: encode11,
    decodeUnsafe,
    decode: decode12
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
var from = ({ name: name3, prefix, encode: encode11, decode: decode12 }) => new Codec(name3, prefix, encode11, decode12);
var baseX = ({ prefix, name: name3, alphabet: alphabet3 }) => {
  const { encode: encode11, decode: decode12 } = base_x_default(alphabet3, name3);
  return from({
    prefix,
    name: name3,
    encode: encode11,
    /**
     * @param {string} text
     */
    decode: (text) => coerce(decode12(text))
  });
};
var decode = (string2, alphabet3, bitsPerChar, name3) => {
  const codes2 = {};
  for (let i = 0; i < alphabet3.length; ++i) {
    codes2[alphabet3[i]] = i;
  }
  let end = string2.length;
  while (string2[end - 1] === "=") {
    --end;
  }
  const out = new Uint8Array(end * bitsPerChar / 8 | 0);
  let bits = 0;
  let buffer = 0;
  let written = 0;
  for (let i = 0; i < end; ++i) {
    const value = codes2[string2[i]];
    if (value === void 0) {
      throw new SyntaxError(`Non-${name3} character`);
    }
    buffer = buffer << bitsPerChar | value;
    bits += bitsPerChar;
    if (bits >= 8) {
      bits -= 8;
      out[written++] = 255 & buffer >> bits;
    }
  }
  if (bits >= bitsPerChar || 255 & buffer << 8 - bits) {
    throw new SyntaxError("Unexpected end of data");
  }
  return out;
};
var encode = (data, alphabet3, bitsPerChar) => {
  const pad = alphabet3[alphabet3.length - 1] === "=";
  const mask = (1 << bitsPerChar) - 1;
  let out = "";
  let bits = 0;
  let buffer = 0;
  for (let i = 0; i < data.length; ++i) {
    buffer = buffer << 8 | data[i];
    bits += 8;
    while (bits > bitsPerChar) {
      bits -= bitsPerChar;
      out += alphabet3[mask & buffer >> bits];
    }
  }
  if (bits) {
    out += alphabet3[mask & buffer << bitsPerChar - bits];
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
function logger(name3) {
  let trace = createDisabledLogger(`${name3}:trace`);
  if (import_debug.default.enabled(`${name3}:trace`) && import_debug.default.names.map((r) => r.toString()).find((n) => n.includes(":trace")) != null) {
    trace = (0, import_debug.default)(`${name3}:trace`);
  }
  return Object.assign((0, import_debug.default)(name3), {
    error: (0, import_debug.default)(`${name3}:error`),
    trace
  });
}

// ../../../node_modules/merge-options/index.mjs
var import_index = __toESM(require_merge_options(), 1);
var merge_options_default = import_index.default;

// registrar.ts
var log = logger("libp2p:registrar");
var DEFAULT_MAX_INBOUND_STREAMS = 32;
var DEFAULT_MAX_OUTBOUND_STREAMS = 64;
var DefaultRegistrar = class {
  topologies;
  handlers;
  components;
  constructor(components) {
    this.topologies = /* @__PURE__ */ new Map();
    this.handlers = /* @__PURE__ */ new Map();
    this.components = components;
    this._onDisconnect = this._onDisconnect.bind(this);
    this._onPeerUpdate = this._onPeerUpdate.bind(this);
    this._onConnect = this._onConnect.bind(this);
    this.components.events.addEventListener("peer:disconnect", this._onDisconnect);
    this.components.events.addEventListener("peer:connect", this._onConnect);
    this.components.events.addEventListener("peer:update", this._onPeerUpdate);
  }
  getProtocols() {
    return Array.from(/* @__PURE__ */ new Set([
      ...this.handlers.keys()
    ])).sort();
  }
  getHandler(protocol) {
    const handler = this.handlers.get(protocol);
    if (handler == null) {
      throw new CodeError(`No handler registered for protocol ${protocol}`, "ERR_NO_HANDLER_FOR_PROTOCOL" /* ERR_NO_HANDLER_FOR_PROTOCOL */);
    }
    return handler;
  }
  getTopologies(protocol) {
    const topologies = this.topologies.get(protocol);
    if (topologies == null) {
      return [];
    }
    return [
      ...topologies.values()
    ];
  }
  /**
   * Registers the `handler` for each protocol
   */
  async handle(protocol, handler, opts) {
    if (this.handlers.has(protocol)) {
      throw new CodeError(`Handler already registered for protocol ${protocol}`, "ERR_PROTOCOL_HANDLER_ALREADY_REGISTERED" /* ERR_PROTOCOL_HANDLER_ALREADY_REGISTERED */);
    }
    const options = merge_options_default.bind({ ignoreUndefined: true })({
      maxInboundStreams: DEFAULT_MAX_INBOUND_STREAMS,
      maxOutboundStreams: DEFAULT_MAX_OUTBOUND_STREAMS
    }, opts);
    this.handlers.set(protocol, {
      handler,
      options
    });
    await this.components.peerStore.merge(this.components.peerId, {
      protocols: [protocol]
    });
  }
  /**
   * Removes the handler for each protocol. The protocol
   * will no longer be supported on streams.
   */
  async unhandle(protocols) {
    const protocolList = Array.isArray(protocols) ? protocols : [protocols];
    protocolList.forEach((protocol) => {
      this.handlers.delete(protocol);
    });
    await this.components.peerStore.patch(this.components.peerId, {
      protocols: protocolList
    });
  }
  /**
   * Register handlers for a set of multicodecs given
   */
  async register(protocol, topology) {
    if (topology == null) {
      throw new CodeError("invalid topology", "ERR_INVALID_PARAMETERS" /* ERR_INVALID_PARAMETERS */);
    }
    const id = `${(Math.random() * 1e9).toString(36)}${Date.now()}`;
    let topologies = this.topologies.get(protocol);
    if (topologies == null) {
      topologies = /* @__PURE__ */ new Map();
      this.topologies.set(protocol, topologies);
    }
    topologies.set(id, topology);
    return id;
  }
  /**
   * Unregister topology
   */
  unregister(id) {
    for (const [protocol, topologies] of this.topologies.entries()) {
      if (topologies.has(id)) {
        topologies.delete(id);
        if (topologies.size === 0) {
          this.topologies.delete(protocol);
        }
      }
    }
  }
  /**
   * Remove a disconnected peer from the record
   */
  _onDisconnect(evt) {
    const remotePeer = evt.detail;
    void this.components.peerStore.get(remotePeer).then((peer) => {
      for (const protocol of peer.protocols) {
        const topologies = this.topologies.get(protocol);
        if (topologies == null) {
          continue;
        }
        for (const topology of topologies.values()) {
          topology.onDisconnect?.(remotePeer);
        }
      }
    }).catch((err) => {
      if (err.code === "ERR_NOT_FOUND" /* ERR_NOT_FOUND */) {
        return;
      }
      log.error("could not inform topologies of disconnecting peer %p", remotePeer, err);
    });
  }
  /**
   * On peer connected if we already have their protocols. Usually used for reconnects
   * as change:protocols event won't be emitted due to identical protocols.
   */
  _onConnect(evt) {
    const remotePeer = evt.detail;
    void this.components.peerStore.get(remotePeer).then((peer) => {
      const connection = this.components.connectionManager.getConnections(peer.id)[0];
      if (connection == null) {
        log("peer %p connected but the connection manager did not have a connection", peer);
        return;
      }
      for (const protocol of peer.protocols) {
        const topologies = this.topologies.get(protocol);
        if (topologies == null) {
          continue;
        }
        for (const topology of topologies.values()) {
          topology.onConnect?.(remotePeer, connection);
        }
      }
    }).catch((err) => {
      if (err.code === "ERR_NOT_FOUND" /* ERR_NOT_FOUND */) {
        return;
      }
      log.error("could not inform topologies of connecting peer %p", remotePeer, err);
    });
  }
  /**
   * Check if a new peer support the multicodecs for this topology
   */
  _onPeerUpdate(evt) {
    const { peer, previous } = evt.detail;
    const removed = (previous?.protocols ?? []).filter((protocol) => !peer.protocols.includes(protocol));
    const added = peer.protocols.filter((protocol) => !(previous?.protocols ?? []).includes(protocol));
    for (const protocol of removed) {
      const topologies = this.topologies.get(protocol);
      if (topologies == null) {
        continue;
      }
      for (const topology of topologies.values()) {
        topology.onDisconnect?.(peer.id);
      }
    }
    for (const protocol of added) {
      const topologies = this.topologies.get(protocol);
      if (topologies == null) {
        continue;
      }
      for (const topology of topologies.values()) {
        const connection = this.components.connectionManager.getConnections(peer.id)[0];
        if (connection == null) {
          continue;
        }
        topology.onConnect?.(peer.id, connection);
      }
    }
  }
};

// ../../interface/dist/src/metrics/tracked-map.js
var TrackedMap = class extends Map {
  metric;
  constructor(init) {
    super();
    const { name: name3, metrics } = init;
    this.metric = metrics.registerMetric(name3);
    this.updateComponentMetric();
  }
  set(key, value) {
    super.set(key, value);
    this.updateComponentMetric();
    return this;
  }
  delete(key) {
    const deleted = super.delete(key);
    this.updateComponentMetric();
    return deleted;
  }
  clear() {
    super.clear();
    this.updateComponentMetric();
  }
  updateComponentMetric() {
    this.metric.update(this.size);
  }
};
function trackedMap(config) {
  const { name: name3, metrics } = config;
  let map;
  if (metrics != null) {
    map = new TrackedMap({ name: name3, metrics });
  } else {
    map = /* @__PURE__ */ new Map();
  }
  return map;
}

// ../../interface/dist/src/transport/index.js
var symbol = Symbol.for("@libp2p/transport");
var FaultTolerance;
(function(FaultTolerance2) {
  FaultTolerance2[FaultTolerance2["FATAL_ALL"] = 0] = "FATAL_ALL";
  FaultTolerance2[FaultTolerance2["NO_FATAL"] = 1] = "NO_FATAL";
})(FaultTolerance || (FaultTolerance = {}));

// transport-manager.ts
var log2 = logger("libp2p:transports");
var DefaultTransportManager = class {
  components;
  transports;
  listeners;
  faultTolerance;
  started;
  constructor(components, init = {}) {
    this.components = components;
    this.started = false;
    this.transports = /* @__PURE__ */ new Map();
    this.listeners = trackedMap({
      name: "libp2p_transport_manager_listeners",
      metrics: this.components.metrics
    });
    this.faultTolerance = init.faultTolerance ?? FaultTolerance.FATAL_ALL;
  }
  /**
   * Adds a `Transport` to the manager
   */
  add(transport) {
    const tag = transport[Symbol.toStringTag];
    if (tag == null) {
      throw new CodeError("Transport must have a valid tag", "ERR_INVALID_KEY" /* ERR_INVALID_KEY */);
    }
    if (this.transports.has(tag)) {
      throw new CodeError(`There is already a transport with the tag ${tag}`, "ERR_DUPLICATE_TRANSPORT" /* ERR_DUPLICATE_TRANSPORT */);
    }
    log2("adding transport %s", tag);
    this.transports.set(tag, transport);
    if (!this.listeners.has(tag)) {
      this.listeners.set(tag, []);
    }
  }
  isStarted() {
    return this.started;
  }
  start() {
    this.started = true;
  }
  async afterStart() {
    const addrs = this.components.addressManager.getListenAddrs();
    await this.listen(addrs);
  }
  /**
   * Stops all listeners
   */
  async stop() {
    const tasks = [];
    for (const [key, listeners] of this.listeners) {
      log2("closing listeners for %s", key);
      while (listeners.length > 0) {
        const listener = listeners.pop();
        if (listener == null) {
          continue;
        }
        tasks.push(listener.close());
      }
    }
    await Promise.all(tasks);
    log2("all listeners closed");
    for (const key of this.listeners.keys()) {
      this.listeners.set(key, []);
    }
    this.started = false;
  }
  /**
   * Dials the given Multiaddr over it's supported transport
   */
  async dial(ma, options) {
    const transport = this.transportForMultiaddr(ma);
    if (transport == null) {
      throw new CodeError(`No transport available for address ${String(ma)}`, "ERR_TRANSPORT_UNAVAILABLE" /* ERR_TRANSPORT_UNAVAILABLE */);
    }
    try {
      return await transport.dial(ma, {
        ...options,
        upgrader: this.components.upgrader
      });
    } catch (err) {
      if (err.code == null) {
        err.code = "ERR_TRANSPORT_DIAL_FAILED" /* ERR_TRANSPORT_DIAL_FAILED */;
      }
      throw err;
    }
  }
  /**
   * Returns all Multiaddr's the listeners are using
   */
  getAddrs() {
    let addrs = [];
    for (const listeners of this.listeners.values()) {
      for (const listener of listeners) {
        addrs = [...addrs, ...listener.getAddrs()];
      }
    }
    return addrs;
  }
  /**
   * Returns all the transports instances
   */
  getTransports() {
    return Array.of(...this.transports.values());
  }
  /**
   * Returns all the listener instances
   */
  getListeners() {
    return Array.of(...this.listeners.values()).flat();
  }
  /**
   * Finds a transport that matches the given Multiaddr
   */
  transportForMultiaddr(ma) {
    for (const transport of this.transports.values()) {
      const addrs = transport.filter([ma]);
      if (addrs.length > 0) {
        return transport;
      }
    }
  }
  /**
   * Starts listeners for each listen Multiaddr
   */
  async listen(addrs) {
    if (addrs == null || addrs.length === 0) {
      log2("no addresses were provided for listening, this node is dial only");
      return;
    }
    const couldNotListen = [];
    for (const [key, transport] of this.transports.entries()) {
      const supportedAddrs = transport.filter(addrs);
      const tasks = [];
      for (const addr of supportedAddrs) {
        log2("creating listener for %s on %a", key, addr);
        const listener = transport.createListener({
          upgrader: this.components.upgrader
        });
        let listeners = this.listeners.get(key) ?? [];
        if (listeners == null) {
          listeners = [];
          this.listeners.set(key, listeners);
        }
        listeners.push(listener);
        listener.addEventListener("listening", () => {
          this.components.events.safeDispatchEvent("transport:listening", {
            detail: listener
          });
        });
        listener.addEventListener("close", () => {
          const index = listeners.findIndex((l) => l === listener);
          listeners.splice(index, 1);
          this.components.events.safeDispatchEvent("transport:close", {
            detail: listener
          });
        });
        tasks.push(listener.listen(addr));
      }
      if (tasks.length === 0) {
        couldNotListen.push(key);
        continue;
      }
      const results = await Promise.allSettled(tasks);
      const isListening = results.find((r) => r.status === "fulfilled");
      if (isListening == null && this.faultTolerance !== FaultTolerance.NO_FATAL) {
        throw new CodeError(`Transport (${key}) could not listen on any available address`, "ERR_NO_VALID_ADDRESSES" /* ERR_NO_VALID_ADDRESSES */);
      }
    }
    if (couldNotListen.length === this.transports.size) {
      const message = `no valid addresses were provided for transports [${couldNotListen.join(", ")}]`;
      if (this.faultTolerance === FaultTolerance.FATAL_ALL) {
        throw new CodeError(message, "ERR_NO_VALID_ADDRESSES" /* ERR_NO_VALID_ADDRESSES */);
      }
      log2(`libp2p in dial mode only: ${message}`);
    }
  }
  /**
   * Removes the given transport from the manager.
   * If a transport has any running listeners, they will be closed.
   */
  async remove(key) {
    log2("removing %s", key);
    for (const listener of this.listeners.get(key) ?? []) {
      await listener.close();
    }
    this.transports.delete(key);
    this.listeners.delete(key);
  }
  /**
   * Removes all transports from the manager.
   * If any listeners are running, they will be closed.
   *
   * @async
   */
  async removeAll() {
    const tasks = [];
    for (const key of this.transports.keys()) {
      tasks.push(this.remove(key));
    }
    await Promise.all(tasks);
  }
};

// upgrader.ts
var import_events = __toESM(require_events(), 1);

// ../../multistream-select/dist/src/constants.js
var PROTOCOL_ID = "/multistream/1.0.0";
var MAX_PROTOCOL_LENGTH = 1024;

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

// ../../../node_modules/uint8arrays/dist/src/concat.js
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

// ../../../node_modules/uint8arrays/dist/src/equals.js
function equals2(a, b) {
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

// ../../../node_modules/uint8arraylist/dist/src/index.js
var symbol2 = Symbol.for("@achingbrain/uint8arraylist");
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
  return Boolean(value?.[symbol2]);
}
var Uint8ArrayList = class _Uint8ArrayList {
  constructor(...data) {
    Object.defineProperty(this, symbol2, { value: true });
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
    let length3 = 0;
    for (const buf of bufs) {
      if (buf instanceof Uint8Array) {
        length3 += buf.byteLength;
        this.bufs.push(buf);
      } else if (isUint8ArrayList(buf)) {
        length3 += buf.byteLength;
        this.bufs.push(...buf.bufs);
      } else {
        throw new Error("Could not append value, must be an Uint8Array or a Uint8ArrayList");
      }
    }
    this.length += length3;
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
    let length3 = 0;
    for (const buf of bufs.reverse()) {
      if (buf instanceof Uint8Array) {
        length3 += buf.byteLength;
        this.bufs.unshift(buf);
      } else if (isUint8ArrayList(buf)) {
        length3 += buf.byteLength;
        this.bufs.unshift(...buf.bufs);
      } else {
        throw new Error("Could not prepend value, must be an Uint8Array or a Uint8ArrayList");
      }
    }
    this.length += length3;
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
  consume(bytes) {
    bytes = Math.trunc(bytes);
    if (Number.isNaN(bytes) || bytes <= 0) {
      return;
    }
    if (bytes === this.byteLength) {
      this.bufs = [];
      this.length = 0;
      return;
    }
    while (this.bufs.length > 0) {
      if (bytes >= this.bufs[0].byteLength) {
        bytes -= this.bufs[0].byteLength;
        this.length -= this.bufs[0].byteLength;
        this.bufs.shift();
      } else {
        this.bufs[0] = this.bufs[0].subarray(bytes);
        this.length -= bytes;
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
    const { bufs, length: length3 } = this._subList(beginInclusive, endExclusive);
    return concat(bufs, length3);
  }
  /**
   * Returns a alloc from the given start and end element index.
   *
   * In the best case where the data extracted comes from a single Uint8Array
   * internally this is a no-copy operation otherwise it is a copy operation.
   */
  subarray(beginInclusive, endExclusive) {
    const { bufs, length: length3 } = this._subList(beginInclusive, endExclusive);
    if (bufs.length === 1) {
      return bufs[0];
    }
    return concat(bufs, length3);
  }
  /**
   * Returns a allocList from the given start and end element index.
   *
   * This is a no-copy operation.
   */
  sublist(beginInclusive, endExclusive) {
    const { bufs, length: length3 } = this._subList(beginInclusive, endExclusive);
    const list = new _Uint8ArrayList();
    list.length = length3;
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
      if (!equals2(this.bufs[i], other.bufs[i])) {
        return false;
      }
    }
    return true;
  }
  /**
   * Create a Uint8ArrayList from a pre-existing list of Uint8Arrays.  Use this
   * method if you know the total size of all the Uint8Arrays ahead of time.
   */
  static fromUint8Arrays(bufs, length3) {
    const list = new _Uint8ArrayList();
    list.bufs = bufs;
    if (length3 == null) {
      length3 = bufs.reduce((acc, curr) => acc + curr.byteLength, 0);
    }
    list.length = length3;
    return list;
  }
};

// ../../../node_modules/it-reader/dist/src/index.js
function reader(source) {
  const reader2 = async function* () {
    let bytes = yield;
    let bl = new Uint8ArrayList();
    for await (const chunk of source) {
      if (bytes == null) {
        bl.append(chunk);
        bytes = yield bl;
        bl = new Uint8ArrayList();
        continue;
      }
      bl.append(chunk);
      while (bl.length >= bytes) {
        const data = bl.sublist(0, bytes);
        bl.consume(bytes);
        bytes = yield data;
        if (bytes == null) {
          if (bl.length > 0) {
            bytes = yield bl;
            bl = new Uint8ArrayList();
          }
          break;
        }
      }
    }
    if (bytes != null) {
      throw Object.assign(new Error(`stream ended before ${bytes} bytes became available`), { code: "ERR_UNDER_READ", buffer: bl });
    }
  }();
  void reader2.next();
  return reader2;
}

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
var AbortError = class extends Error {
  type;
  code;
  constructor(message, code3) {
    super(message ?? "The operation was aborted");
    this.type = "aborted";
    this.code = code3 ?? "ABORT_ERR";
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
            reject(new AbortError());
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

// ../../../node_modules/it-handshake/dist/src/index.js
function handshake(stream) {
  const writer = pushable();
  const source = reader(stream.source);
  const sourcePromise = pDefer();
  let sinkErr;
  const sinkPromise = stream.sink(async function* () {
    yield* writer;
    const source2 = await sourcePromise.promise;
    yield* source2;
  }());
  sinkPromise.catch((err) => {
    sinkErr = err;
  });
  const rest = {
    sink: async (source2) => {
      if (sinkErr != null) {
        await Promise.reject(sinkErr);
        return;
      }
      sourcePromise.resolve(source2);
      await sinkPromise;
    },
    source
  };
  return {
    reader: source,
    writer,
    stream: rest,
    rest: () => writer.end(),
    write: writer.push,
    read: async () => {
      const res = await source.next();
      if (res.value != null) {
        return res.value;
      }
    }
  };
}

// ../../../node_modules/it-merge/dist/src/index.js
function isAsyncIterable(thing) {
  return thing[Symbol.asyncIterator] != null;
}
function merge(...sources) {
  const syncSources = [];
  for (const source of sources) {
    if (!isAsyncIterable(source)) {
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
    const output = pushable({
      objectMode: true
    });
    void Promise.resolve().then(async () => {
      try {
        await Promise.all(sources.map(async (source) => {
          for await (const item of source) {
            output.push(item);
          }
        }));
        output.end();
      } catch (err) {
        output.end(err);
      }
    });
    yield* output;
  }();
}
var src_default = merge;

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/bases/identity.js
var identity_exports = {};
__export(identity_exports, {
  identity: () => identity
});

// ../../../node_modules/uint8arrays/node_modules/multiformats/vendor/base-x.js
function base2(ALPHABET, name3) {
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
  function encode11(source) {
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
  function decode12(string2) {
    var buffer = decodeUnsafe(string2);
    if (buffer) {
      return buffer;
    }
    throw new Error(`Non-${name3} character`);
  }
  return {
    encode: encode11,
    decodeUnsafe,
    decode: decode12
  };
}
var src2 = base2;
var _brrp__multiformats_scope_baseX2 = src2;
var base_x_default2 = _brrp__multiformats_scope_baseX2;

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/bytes.js
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
var fromString2 = (str) => new TextEncoder().encode(str);
var toString2 = (b) => new TextDecoder().decode(b);

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/bases/base.js
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
var from2 = ({ name: name3, prefix, encode: encode11, decode: decode12 }) => new Codec2(name3, prefix, encode11, decode12);
var baseX2 = ({ prefix, name: name3, alphabet: alphabet3 }) => {
  const { encode: encode11, decode: decode12 } = base_x_default2(alphabet3, name3);
  return from2({
    prefix,
    name: name3,
    encode: encode11,
    /**
     * @param {string} text
     */
    decode: (text) => coerce2(decode12(text))
  });
};
var decode2 = (string2, alphabet3, bitsPerChar, name3) => {
  const codes2 = {};
  for (let i = 0; i < alphabet3.length; ++i) {
    codes2[alphabet3[i]] = i;
  }
  let end = string2.length;
  while (string2[end - 1] === "=") {
    --end;
  }
  const out = new Uint8Array(end * bitsPerChar / 8 | 0);
  let bits = 0;
  let buffer = 0;
  let written = 0;
  for (let i = 0; i < end; ++i) {
    const value = codes2[string2[i]];
    if (value === void 0) {
      throw new SyntaxError(`Non-${name3} character`);
    }
    buffer = buffer << bitsPerChar | value;
    bits += bitsPerChar;
    if (bits >= 8) {
      bits -= 8;
      out[written++] = 255 & buffer >> bits;
    }
  }
  if (bits >= bitsPerChar || 255 & buffer << 8 - bits) {
    throw new SyntaxError("Unexpected end of data");
  }
  return out;
};
var encode2 = (data, alphabet3, bitsPerChar) => {
  const pad = alphabet3[alphabet3.length - 1] === "=";
  const mask = (1 << bitsPerChar) - 1;
  let out = "";
  let bits = 0;
  let buffer = 0;
  for (let i = 0; i < data.length; ++i) {
    buffer = buffer << 8 | data[i];
    bits += 8;
    while (bits > bitsPerChar) {
      bits -= bitsPerChar;
      out += alphabet3[mask & buffer >> bits];
    }
  }
  if (bits) {
    out += alphabet3[mask & buffer << bitsPerChar - bits];
  }
  if (pad) {
    while (out.length * bitsPerChar & 7) {
      out += "=";
    }
  }
  return out;
};
var rfc46482 = ({ name: name3, prefix, bitsPerChar, alphabet: alphabet3 }) => {
  return from2({
    prefix,
    name: name3,
    encode(input) {
      return encode2(input, alphabet3, bitsPerChar);
    },
    decode(input) {
      return decode2(input, alphabet3, bitsPerChar, name3);
    }
  });
};

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/bases/identity.js
var identity = from2({
  prefix: "\0",
  name: "identity",
  encode: (buf) => toString2(buf),
  decode: (str) => fromString2(str)
});

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/bases/base2.js
var base2_exports = {};
__export(base2_exports, {
  base2: () => base22
});
var base22 = rfc46482({
  prefix: "0",
  name: "base2",
  alphabet: "01",
  bitsPerChar: 1
});

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/bases/base8.js
var base8_exports = {};
__export(base8_exports, {
  base8: () => base8
});
var base8 = rfc46482({
  prefix: "7",
  name: "base8",
  alphabet: "01234567",
  bitsPerChar: 3
});

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/bases/base10.js
var base10_exports = {};
__export(base10_exports, {
  base10: () => base10
});
var base10 = baseX2({
  prefix: "9",
  name: "base10",
  alphabet: "0123456789"
});

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/bases/base16.js
var base16_exports = {};
__export(base16_exports, {
  base16: () => base16,
  base16upper: () => base16upper
});
var base16 = rfc46482({
  prefix: "f",
  name: "base16",
  alphabet: "0123456789abcdef",
  bitsPerChar: 4
});
var base16upper = rfc46482({
  prefix: "F",
  name: "base16upper",
  alphabet: "0123456789ABCDEF",
  bitsPerChar: 4
});

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/bases/base32.js
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

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/bases/base36.js
var base36_exports = {};
__export(base36_exports, {
  base36: () => base36,
  base36upper: () => base36upper
});
var base36 = baseX2({
  prefix: "k",
  name: "base36",
  alphabet: "0123456789abcdefghijklmnopqrstuvwxyz"
});
var base36upper = baseX2({
  prefix: "K",
  name: "base36upper",
  alphabet: "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ"
});

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/bases/base58.js
var base58_exports2 = {};
__export(base58_exports2, {
  base58btc: () => base58btc2,
  base58flickr: () => base58flickr2
});
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

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/bases/base64.js
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

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/bases/base256emoji.js
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
function encode3(data) {
  return data.reduce((p, c) => {
    p += alphabetBytesToChars[c];
    return p;
  }, "");
}
function decode3(str) {
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
var base256emoji = from2({
  prefix: "\u{1F680}",
  name: "base256emoji",
  encode: encode3,
  decode: decode3
});

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/hashes/sha2-browser.js
var sha2_browser_exports = {};
__export(sha2_browser_exports, {
  sha256: () => sha256,
  sha512: () => sha512
});

// ../../../node_modules/uint8arrays/node_modules/multiformats/vendor/varint.js
var encode_1 = encode4;
var MSB = 128;
var REST = 127;
var MSBALL = ~REST;
var INT = Math.pow(2, 31);
function encode4(num, out, offset) {
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
  encode4.bytes = offset - oldOffset + 1;
  return out;
}
var decode4 = read;
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
  decode: decode4,
  encodingLength: length
};
var _brrp_varint = varint;
var varint_default = _brrp_varint;

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/varint.js
var decode5 = (data, offset = 0) => {
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

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/hashes/digest.js
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
var decode6 = (multihash) => {
  const bytes = coerce2(multihash);
  const [code3, sizeOffset] = decode5(bytes);
  const [size, digestOffset] = decode5(bytes.subarray(sizeOffset));
  const digest3 = bytes.subarray(sizeOffset + digestOffset);
  if (digest3.byteLength !== size) {
    throw new Error("Incorrect length");
  }
  return new Digest(code3, size, digest3, bytes);
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

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/hashes/hasher.js
var from3 = ({ name: name3, code: code3, encode: encode11 }) => new Hasher(name3, code3, encode11);
var Hasher = class {
  /**
   *
   * @param {Name} name
   * @param {Code} code
   * @param {(input: Uint8Array) => Await<Uint8Array>} encode
   */
  constructor(name3, code3, encode11) {
    this.name = name3;
    this.code = code3;
    this.encode = encode11;
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

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/hashes/sha2-browser.js
var sha = (name3) => (
  /**
   * @param {Uint8Array} data
   */
  async (data) => new Uint8Array(await crypto.subtle.digest(name3, data))
);
var sha256 = from3({
  name: "sha2-256",
  code: 18,
  encode: sha("SHA-256")
});
var sha512 = from3({
  name: "sha2-512",
  code: 19,
  encode: sha("SHA-512")
});

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/hashes/identity.js
var identity_exports2 = {};
__export(identity_exports2, {
  identity: () => identity2
});
var code = 0;
var name = "identity";
var encode5 = coerce2;
var digest = (input) => create(code, encode5(input));
var identity2 = { code, name, encode: encode5, digest };

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/codecs/json.js
var textEncoder = new TextEncoder();
var textDecoder = new TextDecoder();

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/cid.js
var format = (link, base3) => {
  const { bytes, version } = link;
  switch (version) {
    case 0:
      return toStringV0(
        bytes,
        baseCache(link),
        /** @type {API.MultibaseEncoder<"z">} */
        base3 || base58btc2.encoder
      );
    default:
      return toStringV1(
        bytes,
        baseCache(link),
        /** @type {API.MultibaseEncoder<Prefix>} */
        base3 || base322.encoder
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
  static equals(self, other) {
    const unknown = (
      /** @type {{code?:unknown, version?:unknown, multihash?:unknown}} */
      other
    );
    return unknown && self.code === unknown.code && self.version === unknown.version && equals4(self.multihash, unknown.multihash);
  }
  /**
   * @param {API.MultibaseEncoder<string>} [base]
   * @returns {string}
   */
  toString(base3) {
    return format(this, base3);
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
        decode6(multihash)
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
    const multihashBytes = coerce2(
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
      const [i, length3] = decode5(initialBytes.subarray(offset));
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
  static parse(source, base3) {
    const [prefix, bytes] = parseCIDtoBytes(source, base3);
    const cid = _CID.decode(bytes);
    if (cid.version === 0 && source[0] !== "Q") {
      throw Error("Version 0 CID string must not include multibase prefix");
    }
    baseCache(cid).set(prefix, source);
    return cid;
  }
};
var parseCIDtoBytes = (source, base3) => {
  switch (source[0]) {
    case "Q": {
      const decoder = base3 || base58btc2;
      return [
        /** @type {Prefix} */
        base58btc2.prefix,
        decoder.decode(`${base58btc2.prefix}${source}`)
      ];
    }
    case base58btc2.prefix: {
      const decoder = base3 || base58btc2;
      return [
        /** @type {Prefix} */
        base58btc2.prefix,
        decoder.decode(source)
      ];
    }
    case base322.prefix: {
      const decoder = base3 || base322;
      return [
        /** @type {Prefix} */
        base322.prefix,
        decoder.decode(source)
      ];
    }
    default: {
      if (base3 == null) {
        throw Error(
          "To parse non base32 or base58btc encoded CID multibase decoder must be provided"
        );
      }
      return [
        /** @type {Prefix} */
        source[0],
        base3.decode(source)
      ];
    }
  }
};
var toStringV0 = (bytes, cache3, base3) => {
  const { prefix } = base3;
  if (prefix !== base58btc2.prefix) {
    throw Error(`Cannot string encode V0 in ${base3.name} encoding`);
  }
  const cid = cache3.get(prefix);
  if (cid == null) {
    const cid2 = base3.encode(bytes).slice(1);
    cache3.set(prefix, cid2);
    return cid2;
  } else {
    return cid;
  }
};
var toStringV1 = (bytes, cache3, base3) => {
  const { prefix } = base3;
  const cid = cache3.get(prefix);
  if (cid == null) {
    const cid2 = base3.encode(bytes);
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

// ../../../node_modules/uint8arrays/node_modules/multiformats/src/basics.js
var bases = { ...identity_exports, ...base2_exports, ...base8_exports, ...base10_exports, ...base16_exports, ...base32_exports2, ...base36_exports, ...base58_exports2, ...base64_exports2, ...base256emoji_exports };
var hashes = { ...sha2_browser_exports, ...identity_exports2 };

// ../../../node_modules/uint8arrays/dist/src/util/bases.js
function createCodec(name3, prefix, encode11, decode12) {
  return {
    name: name3,
    prefix,
    encoder: {
      name: name3,
      prefix,
      encode: encode11
    },
    decoder: {
      decode: decode12
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

// ../../../node_modules/uint8arrays/dist/src/from-string.js
function fromString3(string2, encoding = "utf8") {
  const base3 = bases_default[encoding];
  if (base3 == null) {
    throw new Error(`Unsupported encoding "${encoding}"`);
  }
  if ((encoding === "utf8" || encoding === "utf-8") && globalThis.Buffer != null && globalThis.Buffer.from != null) {
    return asUint8Array(globalThis.Buffer.from(string2, "utf-8"));
  }
  return base3.decoder.decode(`${base3.prefix}${string2}`);
}

// ../../../node_modules/abortable-iterator/dist/src/abort-error.js
var AbortError2 = class extends Error {
  constructor(message, code3) {
    super(message ?? "The operation was aborted");
    this.type = "aborted";
    this.code = code3 ?? "ABORT_ERR";
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
          throw new AbortError2(abortMessage, abortCode);
        }
        const abort = new Promise((resolve, reject) => {
          nextAbortHandler = () => {
            const { abortMessage, abortCode } = opts;
            reject(new AbortError2(abortMessage, abortCode));
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

// ../../../node_modules/it-first/dist/src/index.js
function isAsyncIterable2(thing) {
  return thing[Symbol.asyncIterator] != null;
}
function first(source) {
  if (isAsyncIterable2(source)) {
    return (async () => {
      for await (const entry of source) {
        return entry;
      }
      return void 0;
    })();
  }
  for (const entry of source) {
    return entry;
  }
  return void 0;
}
var src_default2 = first;

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
    const sign = value < 0;
    if (sign) {
      value = -value;
    }
    let lo = value >>> 0;
    let hi = (value - lo) / 4294967296 >>> 0;
    if (sign) {
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
    const bits = new _LongBits();
    let i = 0;
    if (buf.length - offset > 4) {
      for (; i < 4; ++i) {
        bits.lo = (bits.lo | (access.get(offset) & 127) << i * 7) >>> 0;
        if (access.get(offset++) < 128) {
          return bits;
        }
      }
      bits.lo = (bits.lo | (access.get(offset) & 127) << 28) >>> 0;
      bits.hi = (bits.hi | (access.get(offset) & 127) >> 4) >>> 0;
      if (access.get(offset++) < 128) {
        return bits;
      }
      i = 0;
    } else {
      for (; i < 4; ++i) {
        if (offset >= buf.length) {
          throw RangeError(`index out of range: ${offset} > ${buf.length}`);
        }
        bits.lo = (bits.lo | (access.get(offset) & 127) << i * 7) >>> 0;
        if (access.get(offset++) < 128) {
          return bits;
        }
      }
    }
    if (buf.length - offset > 4) {
      for (; i < 5; ++i) {
        bits.hi = (bits.hi | (access.get(offset) & 127) << i * 7 + 3) >>> 0;
        if (access.get(offset++) < 128) {
          return bits;
        }
      }
    } else if (offset < buf.byteLength) {
      for (; i < 5; ++i) {
        if (offset >= buf.length) {
          throw RangeError(`index out of range: ${offset} > ${buf.length}`);
        }
        bits.hi = (bits.hi | (access.get(offset) & 127) << i * 7 + 3) >>> 0;
        if (access.get(offset++) < 128) {
          return bits;
        }
      }
    }
    throw RangeError("invalid varint encoding");
  }
};

// ../../../node_modules/uint8-varint/dist/src/index.js
var N12 = Math.pow(2, 7);
var N22 = Math.pow(2, 14);
var N32 = Math.pow(2, 21);
var N42 = Math.pow(2, 28);
var N52 = Math.pow(2, 35);
var N62 = Math.pow(2, 42);
var N72 = Math.pow(2, 49);
var N82 = Math.pow(2, 56);
var N92 = Math.pow(2, 63);
var unsigned = {
  encodingLength(value) {
    if (value < N12) {
      return 1;
    }
    if (value < N22) {
      return 2;
    }
    if (value < N32) {
      return 3;
    }
    if (value < N42) {
      return 4;
    }
    if (value < N52) {
      return 5;
    }
    if (value < N62) {
      return 6;
    }
    if (value < N72) {
      return 7;
    }
    if (value < N82) {
      return 8;
    }
    if (value < N92) {
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
function isAsyncIterable3(thing) {
  return thing[Symbol.asyncIterator] != null;
}

// ../../../node_modules/it-length-prefixed/dist/src/encode.js
var defaultEncoder = (length3) => {
  const lengthLength = unsigned.encodingLength(length3);
  const lengthBuf = allocUnsafe(lengthLength);
  unsigned.encode(length3, lengthBuf);
  defaultEncoder.bytes = lengthLength;
  return lengthBuf;
};
defaultEncoder.bytes = 0;
function encode6(source, options) {
  options = options ?? {};
  const encodeLength = options.lengthEncoder ?? defaultEncoder;
  function* maybeYield(chunk) {
    const length3 = encodeLength(chunk.byteLength);
    if (length3 instanceof Uint8Array) {
      yield length3;
    } else {
      yield* length3;
    }
    if (chunk instanceof Uint8Array) {
      yield chunk;
    } else {
      yield* chunk;
    }
  }
  if (isAsyncIterable3(source)) {
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
encode6.single = (chunk, options) => {
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
  const length3 = unsigned.decode(buf);
  defaultDecoder.bytes = unsigned.encodingLength(length3);
  return length3;
};
defaultDecoder.bytes = 0;
function decode7(source, options) {
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
  if (isAsyncIterable3(source)) {
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
decode7.fromReader = (reader2, options) => {
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
  return decode7(varByteSource, {
    ...options ?? {},
    onLength
  });
};

// ../../../node_modules/it-pipe/dist/src/index.js
function pipe(first2, ...rest) {
  if (first2 == null) {
    throw new Error("Empty pipeline");
  }
  if (isDuplex(first2)) {
    const duplex = first2;
    first2 = () => duplex.source;
  } else if (isIterable(first2) || isAsyncIterable4(first2)) {
    const source = first2;
    first2 = () => source;
  }
  const fns = [first2, ...rest];
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
var isAsyncIterable4 = (obj) => {
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
      if (isAsyncIterable4(source2)) {
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

// ../../../node_modules/uint8arrays/dist/src/to-string.js
function toString3(array, encoding = "utf8") {
  const base3 = bases_default[encoding];
  if (base3 == null) {
    throw new Error(`Unsupported encoding "${encoding}"`);
  }
  if ((encoding === "utf8" || encoding === "utf-8") && globalThis.Buffer != null && globalThis.Buffer.from != null) {
    return globalThis.Buffer.from(array.buffer, array.byteOffset, array.byteLength).toString("utf8");
  }
  return base3.encoder.encode(array).substring(1);
}

// ../../multistream-select/dist/src/multistream.js
var log3 = logger("libp2p:mss");
var NewLine = fromString3("\n");
function encode7(buffer) {
  const list = new Uint8ArrayList(buffer, NewLine);
  return encode6.single(list);
}
function write(writer, buffer, options = {}) {
  const encoded = encode7(buffer);
  if (options.writeBytes === true) {
    writer.push(encoded.subarray());
  } else {
    writer.push(encoded);
  }
}
function writeAll(writer, buffers, options = {}) {
  const list = new Uint8ArrayList();
  for (const buf of buffers) {
    list.append(encode7(buf));
  }
  if (options.writeBytes === true) {
    writer.push(list.subarray());
  } else {
    writer.push(list);
  }
}
async function read2(reader2, options) {
  let byteLength = 1;
  const varByteSource = {
    [Symbol.asyncIterator]: () => varByteSource,
    next: async () => reader2.next(byteLength)
  };
  let input = varByteSource;
  if (options?.signal != null) {
    input = abortableSource(varByteSource, options.signal);
  }
  const onLength = (l) => {
    byteLength = l;
  };
  const buf = await pipe(input, (source) => decode7(source, { onLength, maxDataLength: MAX_PROTOCOL_LENGTH }), async (source) => src_default2(source));
  if (buf == null || buf.length === 0) {
    throw new CodeError("no buffer returned", "ERR_INVALID_MULTISTREAM_SELECT_MESSAGE");
  }
  if (buf.get(buf.byteLength - 1) !== NewLine[0]) {
    log3.error("Invalid mss message - missing newline - %s", buf.subarray());
    throw new CodeError("missing newline", "ERR_INVALID_MULTISTREAM_SELECT_MESSAGE");
  }
  return buf.sublist(0, -1);
}
async function readString(reader2, options) {
  const buf = await read2(reader2, options);
  return toString3(buf.subarray());
}

// ../../multistream-select/dist/src/select.js
var log4 = logger("libp2p:mss:select");
async function select(stream, protocols, options = {}) {
  protocols = Array.isArray(protocols) ? [...protocols] : [protocols];
  const { reader: reader2, writer, rest, stream: shakeStream } = handshake(stream);
  const protocol = protocols.shift();
  if (protocol == null) {
    throw new Error("At least one protocol must be specified");
  }
  log4.trace('select: write ["%s", "%s"]', PROTOCOL_ID, protocol);
  const p1 = fromString3(PROTOCOL_ID);
  const p2 = fromString3(protocol);
  writeAll(writer, [p1, p2], options);
  let response = await readString(reader2, options);
  log4.trace('select: read "%s"', response);
  if (response === PROTOCOL_ID) {
    response = await readString(reader2, options);
    log4.trace('select: read "%s"', response);
  }
  if (response === protocol) {
    rest();
    return { stream: shakeStream, protocol };
  }
  for (const protocol2 of protocols) {
    log4.trace('select: write "%s"', protocol2);
    write(writer, fromString3(protocol2), options);
    const response2 = await readString(reader2, options);
    log4.trace('select: read "%s" for "%s"', response2, protocol2);
    if (response2 === protocol2) {
      rest();
      return { stream: shakeStream, protocol: protocol2 };
    }
  }
  rest();
  throw new CodeError("protocol selection failed", "ERR_UNSUPPORTED_PROTOCOL");
}

// ../../multistream-select/dist/src/handle.js
var log5 = logger("libp2p:mss:handle");
async function handle(stream, protocols, options) {
  protocols = Array.isArray(protocols) ? protocols : [protocols];
  const { writer, reader: reader2, rest, stream: shakeStream } = handshake(stream);
  while (true) {
    const protocol = await readString(reader2, options);
    log5.trace('read "%s"', protocol);
    if (protocol === PROTOCOL_ID) {
      log5.trace('respond with "%s" for "%s"', PROTOCOL_ID, protocol);
      write(writer, fromString3(PROTOCOL_ID), options);
      continue;
    }
    if (protocols.includes(protocol)) {
      write(writer, fromString3(protocol), options);
      log5.trace('respond with "%s" for "%s"', protocol, protocol);
      rest();
      return { stream: shakeStream, protocol };
    }
    if (protocol === "ls") {
      write(writer, new Uint8ArrayList(...protocols.map((p) => encode7(fromString3(p)))), options);
      log5.trace('respond with "%s" for %s', protocols, protocol);
      continue;
    }
    write(writer, fromString3("na"), options);
    log5('respond with "na" for "%s"', protocol);
  }
}

// ../../interface/dist/src/peer-id/index.js
var symbol3 = Symbol.for("@libp2p/peer-id");

// ../../../node_modules/multiformats/src/bases/identity.js
var identity_exports3 = {};
__export(identity_exports3, {
  identity: () => identity3
});
var identity3 = from({
  prefix: "\0",
  name: "identity",
  encode: (buf) => toString(buf),
  decode: (str) => fromString(str)
});

// ../../../node_modules/multiformats/src/bases/base2.js
var base2_exports2 = {};
__export(base2_exports2, {
  base2: () => base23
});
var base23 = rfc4648({
  prefix: "0",
  name: "base2",
  alphabet: "01",
  bitsPerChar: 1
});

// ../../../node_modules/multiformats/src/bases/base8.js
var base8_exports2 = {};
__export(base8_exports2, {
  base8: () => base82
});
var base82 = rfc4648({
  prefix: "7",
  name: "base8",
  alphabet: "01234567",
  bitsPerChar: 3
});

// ../../../node_modules/multiformats/src/bases/base10.js
var base10_exports2 = {};
__export(base10_exports2, {
  base10: () => base102
});
var base102 = baseX({
  prefix: "9",
  name: "base10",
  alphabet: "0123456789"
});

// ../../../node_modules/multiformats/src/bases/base16.js
var base16_exports2 = {};
__export(base16_exports2, {
  base16: () => base162,
  base16upper: () => base16upper2
});
var base162 = rfc4648({
  prefix: "f",
  name: "base16",
  alphabet: "0123456789abcdef",
  bitsPerChar: 4
});
var base16upper2 = rfc4648({
  prefix: "F",
  name: "base16upper",
  alphabet: "0123456789ABCDEF",
  bitsPerChar: 4
});

// ../../../node_modules/multiformats/src/bases/base36.js
var base36_exports2 = {};
__export(base36_exports2, {
  base36: () => base362,
  base36upper: () => base36upper2
});
var base362 = baseX({
  prefix: "k",
  name: "base36",
  alphabet: "0123456789abcdefghijklmnopqrstuvwxyz"
});
var base36upper2 = baseX({
  prefix: "K",
  name: "base36upper",
  alphabet: "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ"
});

// ../../../node_modules/multiformats/src/bases/base256emoji.js
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
function decode8(str) {
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
var base256emoji2 = from({
  prefix: "\u{1F680}",
  name: "base256emoji",
  encode: encode8,
  decode: decode8
});

// ../../../node_modules/multiformats/src/hashes/sha2-browser.js
var sha2_browser_exports2 = {};
__export(sha2_browser_exports2, {
  sha256: () => sha2562,
  sha512: () => sha5122
});

// ../../../node_modules/multiformats/vendor/varint.js
var encode_12 = encode9;
var MSB2 = 128;
var REST2 = 127;
var MSBALL2 = ~REST2;
var INT2 = Math.pow(2, 31);
function encode9(num, out, offset) {
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
  encode9.bytes = offset - oldOffset + 1;
  return out;
}
var decode9 = read3;
var MSB$12 = 128;
var REST$12 = 127;
function read3(buf, offset) {
  var res = 0, offset = offset || 0, shift = 0, counter = offset, b, l = buf.length;
  do {
    if (counter >= l) {
      read3.bytes = 0;
      throw new RangeError("Could not decode varint");
    }
    b = buf[counter++];
    res += shift < 28 ? (b & REST$12) << shift : (b & REST$12) * Math.pow(2, shift);
    shift += 7;
  } while (b >= MSB$12);
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
var length2 = function(value) {
  return value < N13 ? 1 : value < N23 ? 2 : value < N33 ? 3 : value < N43 ? 4 : value < N53 ? 5 : value < N63 ? 6 : value < N73 ? 7 : value < N83 ? 8 : value < N93 ? 9 : 10;
};
var varint2 = {
  encode: encode_12,
  decode: decode9,
  encodingLength: length2
};
var _brrp_varint2 = varint2;
var varint_default2 = _brrp_varint2;

// ../../../node_modules/multiformats/src/varint.js
var decode10 = (data, offset = 0) => {
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

// ../../../node_modules/multiformats/src/hashes/digest.js
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
var decode11 = (multihash) => {
  const bytes = coerce(multihash);
  const [code3, sizeOffset] = decode10(bytes);
  const [size, digestOffset] = decode10(bytes.subarray(sizeOffset));
  const digest3 = bytes.subarray(sizeOffset + digestOffset);
  if (digest3.byteLength !== size) {
    throw new Error("Incorrect length");
  }
  return new Digest2(code3, size, digest3, bytes);
};
var equals5 = (a, b) => {
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

// ../../../node_modules/multiformats/src/hashes/hasher.js
var from4 = ({ name: name3, code: code3, encode: encode11 }) => new Hasher2(name3, code3, encode11);
var Hasher2 = class {
  /**
   *
   * @param {Name} name
   * @param {Code} code
   * @param {(input: Uint8Array) => Await<Uint8Array>} encode
   */
  constructor(name3, code3, encode11) {
    this.name = name3;
    this.code = code3;
    this.encode = encode11;
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

// ../../../node_modules/multiformats/src/hashes/sha2-browser.js
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

// ../../../node_modules/multiformats/src/hashes/identity.js
var identity_exports4 = {};
__export(identity_exports4, {
  identity: () => identity4
});
var code2 = 0;
var name2 = "identity";
var encode10 = coerce;
var digest2 = (input) => create2(code2, encode10(input));
var identity4 = { code: code2, name: name2, encode: encode10, digest: digest2 };

// ../../../node_modules/multiformats/src/codecs/json.js
var textEncoder2 = new TextEncoder();
var textDecoder2 = new TextDecoder();

// ../../../node_modules/multiformats/src/cid.js
var format2 = (link, base3) => {
  const { bytes, version } = link;
  switch (version) {
    case 0:
      return toStringV02(
        bytes,
        baseCache2(link),
        /** @type {API.MultibaseEncoder<"z">} */
        base3 || base58btc.encoder
      );
    default:
      return toStringV12(
        bytes,
        baseCache2(link),
        /** @type {API.MultibaseEncoder<Prefix>} */
        base3 || base32.encoder
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
  static equals(self, other) {
    const unknown = (
      /** @type {{code?:unknown, version?:unknown, multihash?:unknown}} */
      other
    );
    return unknown && self.code === unknown.code && self.version === unknown.version && equals5(self.multihash, unknown.multihash);
  }
  /**
   * @param {API.MultibaseEncoder<string>} [base]
   * @returns {string}
   */
  toString(base3) {
    return format2(this, base3);
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
        decode11(multihash)
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
    const multihashBytes = coerce(
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
      const [i, length3] = decode10(initialBytes.subarray(offset));
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
  static parse(source, base3) {
    const [prefix, bytes] = parseCIDtoBytes2(source, base3);
    const cid = _CID.decode(bytes);
    if (cid.version === 0 && source[0] !== "Q") {
      throw Error("Version 0 CID string must not include multibase prefix");
    }
    baseCache2(cid).set(prefix, source);
    return cid;
  }
};
var parseCIDtoBytes2 = (source, base3) => {
  switch (source[0]) {
    case "Q": {
      const decoder = base3 || base58btc;
      return [
        /** @type {Prefix} */
        base58btc.prefix,
        decoder.decode(`${base58btc.prefix}${source}`)
      ];
    }
    case base58btc.prefix: {
      const decoder = base3 || base58btc;
      return [
        /** @type {Prefix} */
        base58btc.prefix,
        decoder.decode(source)
      ];
    }
    case base32.prefix: {
      const decoder = base3 || base32;
      return [
        /** @type {Prefix} */
        base32.prefix,
        decoder.decode(source)
      ];
    }
    default: {
      if (base3 == null) {
        throw Error(
          "To parse non base32 or base58btc encoded CID multibase decoder must be provided"
        );
      }
      return [
        /** @type {Prefix} */
        source[0],
        base3.decode(source)
      ];
    }
  }
};
var toStringV02 = (bytes, cache3, base3) => {
  const { prefix } = base3;
  if (prefix !== base58btc.prefix) {
    throw Error(`Cannot string encode V0 in ${base3.name} encoding`);
  }
  const cid = cache3.get(prefix);
  if (cid == null) {
    const cid2 = base3.encode(bytes).slice(1);
    cache3.set(prefix, cid2);
    return cid2;
  } else {
    return cid;
  }
};
var toStringV12 = (bytes, cache3, base3) => {
  const { prefix } = base3;
  const cid = cache3.get(prefix);
  if (cid == null) {
    const cid2 = base3.encode(bytes);
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

// ../../../node_modules/multiformats/src/basics.js
var bases2 = { ...identity_exports3, ...base2_exports2, ...base8_exports2, ...base10_exports2, ...base16_exports2, ...base32_exports, ...base36_exports2, ...base58_exports, ...base64_exports, ...base256emoji_exports2 };
var hashes2 = { ...sha2_browser_exports2, ...identity_exports4 };

// ../../peer-id/dist/src/index.js
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
  [symbol3] = true;
  toString() {
    if (this.string == null) {
      this.string = base58btc.encode(this.multihash.bytes).slice(1);
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
      return equals2(this.multihash.bytes, id);
    } else if (typeof id === "string") {
      return peerIdFromString(id).equals(this);
    } else if (id?.multihash?.bytes != null) {
      return equals2(this.multihash.bytes, id.multihash.bytes);
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
    const multihash = decode11(base58btc.decode(`z${str}`));
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
    const multihash = decode11(buf);
    if (multihash.code === identity4.code) {
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
  } else if (multihash.code === identity4.code) {
    if (multihash.digest.length === MARSHALLED_ED225519_PUBLIC_KEY_LENGTH) {
      return new Ed25519PeerIdImpl({ multihash: cid.multihash });
    } else if (multihash.digest.length === MARSHALLED_SECP256K1_PUBLIC_KEY_LENGTH) {
      return new Secp256k1PeerIdImpl({ multihash: cid.multihash });
    }
  }
  throw new Error("Supplied PeerID CID is invalid");
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

// ../../interface/dist/src/connection/index.js
var symbol4 = Symbol.for("@libp2p/connection");

// ../../interface/dist/src/connection/status.js
var OPEN = "OPEN";
var CLOSING = "CLOSING";
var CLOSED = "CLOSED";

// connection/index.ts
var log6 = logger("libp2p:connection");
var ConnectionImpl = class {
  /**
   * Connection identifier.
   */
  id;
  /**
   * Observed multiaddr of the remote peer
   */
  remoteAddr;
  /**
   * Remote peer id
   */
  remotePeer;
  /**
   * Connection metadata
   */
  stat;
  /**
   * User provided tags
   *
   */
  tags;
  /**
   * Reference to the new stream function of the multiplexer
   */
  _newStream;
  /**
   * Reference to the close function of the raw connection
   */
  _close;
  /**
   * Reference to the getStreams function of the muxer
   */
  _getStreams;
  _closing;
  /**
   * An implementation of the js-libp2p connection.
   * Any libp2p transport should use an upgrader to return this connection.
   */
  constructor(init) {
    const { remoteAddr, remotePeer, newStream, close, getStreams, stat } = init;
    this.id = `${parseInt(String(Math.random() * 1e9)).toString(36)}${Date.now()}`;
    this.remoteAddr = remoteAddr;
    this.remotePeer = remotePeer;
    this.stat = {
      ...stat,
      status: OPEN
    };
    this._newStream = newStream;
    this._close = close;
    this._getStreams = getStreams;
    this.tags = [];
    this._closing = false;
  }
  [Symbol.toStringTag] = "Connection";
  [symbol4] = true;
  /**
   * Get all the streams of the muxer
   */
  get streams() {
    return this._getStreams();
  }
  /**
   * Create a new stream from this connection
   */
  async newStream(protocols, options) {
    if (this.stat.status === CLOSING) {
      throw new CodeError("the connection is being closed", "ERR_CONNECTION_BEING_CLOSED");
    }
    if (this.stat.status === CLOSED) {
      throw new CodeError("the connection is closed", "ERR_CONNECTION_CLOSED");
    }
    if (!Array.isArray(protocols)) {
      protocols = [protocols];
    }
    const stream = await this._newStream(protocols, options);
    stream.stat.direction = "outbound";
    return stream;
  }
  /**
   * Add a stream when it is opened to the registry
   */
  addStream(stream) {
    stream.stat.direction = "inbound";
  }
  /**
   * Remove stream registry after it is closed
   */
  removeStream(id) {
  }
  /**
   * Close the connection
   */
  async close() {
    if (this.stat.status === CLOSED || this._closing) {
      return;
    }
    this.stat.status = CLOSING;
    try {
      this.streams.forEach((s) => {
        s.close();
      });
    } catch (err) {
      log6.error(err);
    }
    this._closing = true;
    await this._close();
    this._closing = false;
    this.stat.timeline.close = Date.now();
    this.stat.status = CLOSED;
  }
};
function createConnection(init) {
  return new ConnectionImpl(init);
}

// connection-manager/constants.ts
var INBOUND_UPGRADE_TIMEOUT = 3e4;

// upgrader.ts
var log7 = logger("libp2p:upgrader");
function findIncomingStreamLimit(protocol, registrar) {
  try {
    const { options } = registrar.getHandler(protocol);
    return options.maxInboundStreams;
  } catch (err) {
    if (err.code !== "ERR_NO_HANDLER_FOR_PROTOCOL" /* ERR_NO_HANDLER_FOR_PROTOCOL */) {
      throw err;
    }
  }
  return DEFAULT_MAX_INBOUND_STREAMS;
}
function findOutgoingStreamLimit(protocol, registrar, options = {}) {
  try {
    const { options: options2 } = registrar.getHandler(protocol);
    if (options2.maxOutboundStreams != null) {
      return options2.maxOutboundStreams;
    }
  } catch (err) {
    if (err.code !== "ERR_NO_HANDLER_FOR_PROTOCOL" /* ERR_NO_HANDLER_FOR_PROTOCOL */) {
      throw err;
    }
  }
  return options.maxOutboundStreams ?? DEFAULT_MAX_OUTBOUND_STREAMS;
}
function countStreams(protocol, direction, connection) {
  let streamCount = 0;
  connection.streams.forEach((stream) => {
    if (stream.stat.direction === direction && stream.stat.protocol === protocol) {
      streamCount++;
    }
  });
  return streamCount;
}
var DefaultUpgrader = class {
  components;
  connectionEncryption;
  muxers;
  inboundUpgradeTimeout;
  events;
  constructor(components, init) {
    this.components = components;
    this.connectionEncryption = /* @__PURE__ */ new Map();
    init.connectionEncryption.forEach((encrypter) => {
      this.connectionEncryption.set(encrypter.protocol, encrypter);
    });
    this.muxers = /* @__PURE__ */ new Map();
    init.muxers.forEach((muxer) => {
      this.muxers.set(muxer.protocol, muxer);
    });
    this.inboundUpgradeTimeout = init.inboundUpgradeTimeout ?? INBOUND_UPGRADE_TIMEOUT;
    this.events = components.events;
  }
  async shouldBlockConnection(remotePeer, maConn, connectionType) {
    const connectionGater = this.components.connectionGater[connectionType];
    if (connectionGater !== void 0) {
      if (await connectionGater(remotePeer, maConn)) {
        throw new CodeError(`The multiaddr connection is blocked by gater.${connectionType}`, "ERR_CONNECTION_INTERCEPTED" /* ERR_CONNECTION_INTERCEPTED */);
      }
    }
  }
  /**
   * Upgrades an inbound connection
   */
  async upgradeInbound(maConn, opts) {
    const accept = await this.components.connectionManager.acceptIncomingConnection(maConn);
    if (!accept) {
      throw new CodeError("connection denied", "ERR_CONNECTION_DENIED" /* ERR_CONNECTION_DENIED */);
    }
    let encryptedConn;
    let remotePeer;
    let upgradedConn;
    let muxerFactory;
    let cryptoProtocol;
    const signal = anySignal([AbortSignal.timeout(this.inboundUpgradeTimeout)]);
    try {
      (0, import_events.setMaxListeners)?.(Infinity, signal);
    } catch {
    }
    try {
      const abortableStream = abortableDuplex(maConn, signal);
      maConn.source = abortableStream.source;
      maConn.sink = abortableStream.sink;
      if (await this.components.connectionGater.denyInboundConnection?.(maConn) === true) {
        throw new CodeError("The multiaddr connection is blocked by gater.acceptConnection", "ERR_CONNECTION_INTERCEPTED" /* ERR_CONNECTION_INTERCEPTED */);
      }
      this.components.metrics?.trackMultiaddrConnection(maConn);
      log7("starting the inbound connection upgrade");
      let protectedConn = maConn;
      if (opts?.skipProtection !== true) {
        const protector = this.components.connectionProtector;
        if (protector != null) {
          log7("protecting the inbound connection");
          protectedConn = await protector.protect(maConn);
        }
      }
      try {
        encryptedConn = protectedConn;
        if (opts?.skipEncryption !== true) {
          ({
            conn: encryptedConn,
            remotePeer,
            protocol: cryptoProtocol
          } = await this._encryptInbound(protectedConn));
          const maConn2 = {
            ...protectedConn,
            ...encryptedConn
          };
          await this.shouldBlockConnection(remotePeer, maConn2, "denyInboundEncryptedConnection");
        } else {
          const idStr = maConn.remoteAddr.getPeerId();
          if (idStr == null) {
            throw new CodeError("inbound connection that skipped encryption must have a peer id", "ERR_INVALID_MULTIADDR" /* ERR_INVALID_MULTIADDR */);
          }
          const remotePeerId = peerIdFromString(idStr);
          cryptoProtocol = "native";
          remotePeer = remotePeerId;
        }
        upgradedConn = encryptedConn;
        if (opts?.muxerFactory != null) {
          muxerFactory = opts.muxerFactory;
        } else if (this.muxers.size > 0) {
          const multiplexed = await this._multiplexInbound({
            ...protectedConn,
            ...encryptedConn
          }, this.muxers);
          muxerFactory = multiplexed.muxerFactory;
          upgradedConn = multiplexed.stream;
        }
      } catch (err) {
        log7.error("Failed to upgrade inbound connection", err);
        throw err;
      }
      await this.shouldBlockConnection(remotePeer, maConn, "denyInboundUpgradedConnection");
      log7("Successfully upgraded inbound connection");
      return this._createConnection({
        cryptoProtocol,
        direction: "inbound",
        maConn,
        upgradedConn,
        muxerFactory,
        remotePeer
      });
    } finally {
      this.components.connectionManager.afterUpgradeInbound();
      signal.clear();
    }
  }
  /**
   * Upgrades an outbound connection
   */
  async upgradeOutbound(maConn, opts) {
    const idStr = maConn.remoteAddr.getPeerId();
    let remotePeerId;
    if (idStr != null) {
      remotePeerId = peerIdFromString(idStr);
      await this.shouldBlockConnection(remotePeerId, maConn, "denyOutboundConnection");
    }
    let encryptedConn;
    let remotePeer;
    let upgradedConn;
    let cryptoProtocol;
    let muxerFactory;
    this.components.metrics?.trackMultiaddrConnection(maConn);
    log7("Starting the outbound connection upgrade");
    let protectedConn = maConn;
    if (opts?.skipProtection !== true) {
      const protector = this.components.connectionProtector;
      if (protector != null) {
        protectedConn = await protector.protect(maConn);
      }
    }
    try {
      encryptedConn = protectedConn;
      if (opts?.skipEncryption !== true) {
        ({
          conn: encryptedConn,
          remotePeer,
          protocol: cryptoProtocol
        } = await this._encryptOutbound(protectedConn, remotePeerId));
        const maConn2 = {
          ...protectedConn,
          ...encryptedConn
        };
        await this.shouldBlockConnection(remotePeer, maConn2, "denyOutboundEncryptedConnection");
      } else {
        if (remotePeerId == null) {
          throw new CodeError("Encryption was skipped but no peer id was passed", "ERR_INVALID_PEER" /* ERR_INVALID_PEER */);
        }
        cryptoProtocol = "native";
        remotePeer = remotePeerId;
      }
      upgradedConn = encryptedConn;
      if (opts?.muxerFactory != null) {
        muxerFactory = opts.muxerFactory;
      } else if (this.muxers.size > 0) {
        const multiplexed = await this._multiplexOutbound({
          ...protectedConn,
          ...encryptedConn
        }, this.muxers);
        muxerFactory = multiplexed.muxerFactory;
        upgradedConn = multiplexed.stream;
      }
    } catch (err) {
      log7.error("Failed to upgrade outbound connection", err);
      await maConn.close(err);
      throw err;
    }
    await this.shouldBlockConnection(remotePeer, maConn, "denyOutboundUpgradedConnection");
    log7("Successfully upgraded outbound connection");
    return this._createConnection({
      cryptoProtocol,
      direction: "outbound",
      maConn,
      upgradedConn,
      muxerFactory,
      remotePeer
    });
  }
  /**
   * A convenience method for generating a new `Connection`
   */
  _createConnection(opts) {
    const {
      cryptoProtocol,
      direction,
      maConn,
      upgradedConn,
      remotePeer,
      muxerFactory
    } = opts;
    let muxer;
    let newStream;
    let connection;
    if (muxerFactory != null) {
      muxer = muxerFactory.createStreamMuxer({
        direction,
        // Run anytime a remote stream is created
        onIncomingStream: (muxedStream) => {
          if (connection == null) {
            return;
          }
          void Promise.resolve().then(async () => {
            const protocols = this.components.registrar.getProtocols();
            const { stream, protocol } = await handle(muxedStream, protocols);
            log7("%s: incoming stream opened on %s", direction, protocol);
            if (connection == null) {
              return;
            }
            const incomingLimit = findIncomingStreamLimit(protocol, this.components.registrar);
            const streamCount = countStreams(protocol, "inbound", connection);
            if (streamCount === incomingLimit) {
              const err = new CodeError(`Too many inbound protocol streams for protocol "${protocol}" - limit ${incomingLimit}`, "ERR_TOO_MANY_INBOUND_PROTOCOL_STREAMS" /* ERR_TOO_MANY_INBOUND_PROTOCOL_STREAMS */);
              muxedStream.abort(err);
              throw err;
            }
            muxedStream.source = stream.source;
            muxedStream.sink = stream.sink;
            muxedStream.stat.protocol = protocol;
            await this.components.peerStore.merge(remotePeer, {
              protocols: [protocol]
            });
            connection.addStream(muxedStream);
            this.components.metrics?.trackProtocolStream(muxedStream, connection);
            this._onStream({ connection, stream: muxedStream, protocol });
          }).catch((err) => {
            log7.error(err);
            if (muxedStream.stat.timeline.close == null) {
              muxedStream.close();
            }
          });
        },
        // Run anytime a stream closes
        onStreamEnd: (muxedStream) => {
          connection?.removeStream(muxedStream.id);
        }
      });
      newStream = async (protocols, options = {}) => {
        if (muxer == null) {
          throw new CodeError("Stream is not multiplexed", "ERR_MUXER_UNAVAILABLE" /* ERR_MUXER_UNAVAILABLE */);
        }
        log7("%s: starting new stream on %s", direction, protocols);
        const muxedStream = await muxer.newStream();
        try {
          if (options.signal == null) {
            log7("No abort signal was passed while trying to negotiate protocols %s falling back to default timeout", protocols);
            options.signal = AbortSignal.timeout(3e4);
            try {
              (0, import_events.setMaxListeners)?.(Infinity, options.signal);
            } catch {
            }
          }
          const { stream, protocol } = await select(muxedStream, protocols, options);
          const outgoingLimit = findOutgoingStreamLimit(protocol, this.components.registrar, options);
          const streamCount = countStreams(protocol, "outbound", connection);
          if (streamCount >= outgoingLimit) {
            const err = new CodeError(`Too many outbound protocol streams for protocol "${protocol}" - limit ${outgoingLimit}`, "ERR_TOO_MANY_OUTBOUND_PROTOCOL_STREAMS" /* ERR_TOO_MANY_OUTBOUND_PROTOCOL_STREAMS */);
            muxedStream.abort(err);
            throw err;
          }
          await this.components.peerStore.merge(remotePeer, {
            protocols: [protocol]
          });
          muxedStream.source = stream.source;
          muxedStream.sink = stream.sink;
          muxedStream.stat.protocol = protocol;
          this.components.metrics?.trackProtocolStream(muxedStream, connection);
          return muxedStream;
        } catch (err) {
          log7.error("could not create new stream", err);
          if (muxedStream.stat.timeline.close == null) {
            muxedStream.close();
          }
          if (err.code != null) {
            throw err;
          }
          throw new CodeError(String(err), "ERR_UNSUPPORTED_PROTOCOL" /* ERR_UNSUPPORTED_PROTOCOL */);
        }
      };
      void Promise.all([
        muxer.sink(upgradedConn.source),
        upgradedConn.sink(muxer.source)
      ]).catch((err) => {
        log7.error(err);
      });
    }
    const _timeline = maConn.timeline;
    maConn.timeline = new Proxy(_timeline, {
      set: (...args) => {
        if (connection != null && args[1] === "close" && args[2] != null && _timeline.close == null) {
          (async () => {
            try {
              if (connection.stat.status === "OPEN") {
                await connection.close();
              }
            } catch (err) {
              log7.error(err);
            } finally {
              this.events.safeDispatchEvent("connection:close", {
                detail: connection
              });
            }
          })().catch((err) => {
            log7.error(err);
          });
        }
        return Reflect.set(...args);
      }
    });
    maConn.timeline.upgraded = Date.now();
    const errConnectionNotMultiplexed = () => {
      throw new CodeError("connection is not multiplexed", "ERR_CONNECTION_NOT_MULTIPLEXED" /* ERR_CONNECTION_NOT_MULTIPLEXED */);
    };
    connection = createConnection({
      remoteAddr: maConn.remoteAddr,
      remotePeer,
      stat: {
        status: "OPEN",
        direction,
        timeline: maConn.timeline,
        multiplexer: muxer?.protocol,
        encryption: cryptoProtocol
      },
      newStream: newStream ?? errConnectionNotMultiplexed,
      getStreams: () => {
        if (muxer != null) {
          return muxer.streams;
        } else {
          return errConnectionNotMultiplexed();
        }
      },
      close: async () => {
        await maConn.close();
        if (muxer != null) {
          muxer.close();
        }
      }
    });
    this.events.safeDispatchEvent("connection:open", {
      detail: connection
    });
    return connection;
  }
  /**
   * Routes incoming streams to the correct handler
   */
  _onStream(opts) {
    const { connection, stream, protocol } = opts;
    const { handler } = this.components.registrar.getHandler(protocol);
    handler({ connection, stream });
  }
  /**
   * Attempts to encrypt the incoming `connection` with the provided `cryptos`
   */
  async _encryptInbound(connection) {
    const protocols = Array.from(this.connectionEncryption.keys());
    log7("handling inbound crypto protocol selection", protocols);
    try {
      const { stream, protocol } = await handle(connection, protocols, {
        writeBytes: true
      });
      const encrypter = this.connectionEncryption.get(protocol);
      if (encrypter == null) {
        throw new Error(`no crypto module found for ${protocol}`);
      }
      log7("encrypting inbound connection...");
      return {
        ...await encrypter.secureInbound(this.components.peerId, stream),
        protocol
      };
    } catch (err) {
      throw new CodeError(String(err), "ERR_ENCRYPTION_FAILED" /* ERR_ENCRYPTION_FAILED */);
    }
  }
  /**
   * Attempts to encrypt the given `connection` with the provided connection encrypters.
   * The first `ConnectionEncrypter` module to succeed will be used
   */
  async _encryptOutbound(connection, remotePeerId) {
    const protocols = Array.from(this.connectionEncryption.keys());
    log7("selecting outbound crypto protocol", protocols);
    try {
      const { stream, protocol } = await select(connection, protocols, {
        writeBytes: true
      });
      const encrypter = this.connectionEncryption.get(protocol);
      if (encrypter == null) {
        throw new Error(`no crypto module found for ${protocol}`);
      }
      log7("encrypting outbound connection to %p", remotePeerId);
      return {
        ...await encrypter.secureOutbound(this.components.peerId, stream, remotePeerId),
        protocol
      };
    } catch (err) {
      throw new CodeError(String(err), "ERR_ENCRYPTION_FAILED" /* ERR_ENCRYPTION_FAILED */);
    }
  }
  /**
   * Selects one of the given muxers via multistream-select. That
   * muxer will be used for all future streams on the connection.
   */
  async _multiplexOutbound(connection, muxers) {
    const protocols = Array.from(muxers.keys());
    log7("outbound selecting muxer %s", protocols);
    try {
      const { stream, protocol } = await select(connection, protocols, {
        writeBytes: true
      });
      log7("%s selected as muxer protocol", protocol);
      const muxerFactory = muxers.get(protocol);
      return { stream, muxerFactory };
    } catch (err) {
      log7.error("error multiplexing outbound stream", err);
      throw new CodeError(String(err), "ERR_MUXER_UNAVAILABLE" /* ERR_MUXER_UNAVAILABLE */);
    }
  }
  /**
   * Registers support for one of the given muxers via multistream-select. The
   * selected muxer will be used for all future streams on the connection.
   */
  async _multiplexInbound(connection, muxers) {
    const protocols = Array.from(muxers.keys());
    log7("inbound handling muxers %s", protocols);
    try {
      const { stream, protocol } = await handle(connection, protocols, {
        writeBytes: true
      });
      const muxerFactory = muxers.get(protocol);
      return { stream, muxerFactory };
    } catch (err) {
      log7.error("error multiplexing inbound stream", err);
      throw new CodeError(String(err), "ERR_MUXER_UNAVAILABLE" /* ERR_MUXER_UNAVAILABLE */);
    }
  }
};
export {
  DefaultRegistrar,
  DefaultTransportManager,
  DefaultUpgrader
};
