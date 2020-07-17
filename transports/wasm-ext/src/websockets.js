// Copyright 2020 Parity Technologies (UK) Ltd.
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

export const websocket_transport = () => {
	return {
		dial: dial,
		listen_on: (addr) => {
			let err = new Error("Listening on WebSockets is not possible from within a browser");
			err.name = "NotSupportedError";
			throw err;
		},
	};
}

/// Turns a string multiaddress into a WebSockets string URL.
const multiaddr_to_ws = (addr) => {
	let parsed = addr.match(/^\/(ip4|ip6|dns4|dns6|dns)\/(.*?)\/tcp\/(.*?)\/(ws|wss|x-parity-ws\/(.*)|x-parity-wss\/(.*))$/);
	if (parsed != null) {
		let proto = 'wss';
		if (parsed[4] == 'ws' || parsed[4] == 'x-parity-ws') {
			proto = 'ws';
		}
		let url = decodeURIComponent(parsed[5] || parsed[6] || '');
		if (parsed[1] == 'ip6') {
			return proto + "://[" + parsed[2] + "]:" + parsed[3] + url;
		} else {
			return proto + "://" + parsed[2] + ":" + parsed[3] + url;
		}
	}

	let err = new Error("Address not supported: " + addr);
	err.name = "NotSupportedError";
	throw err;
}

// Attempt to dial a multiaddress.
const dial = (addr) => {
	let ws = new WebSocket(multiaddr_to_ws(addr));
	ws.binaryType = "arraybuffer";
	let reader = read_queue();

	return new Promise((open_resolve, open_reject) => {
		ws.onerror = (ev) => {
			// If `open_resolve` has been called earlier, calling `open_reject` seems to be
			// silently ignored. It is easier to unconditionally call `open_reject` rather than
			// check in which state the connection is, which would be error-prone.
			open_reject(ev);
			// Injecting an EOF is how we report to the reading side that the connection has been
			// closed. Injecting multiple EOFs is harmless.
			reader.inject_eof();
		};
		ws.onclose = (ev) => {
			// Same remarks as above.
			open_reject(ev);
			reader.inject_eof();
		};

		// We inject all incoming messages into the queue unconditionally. The caller isn't
		// supposed to access this queue unless the connection is open.
		ws.onmessage = (ev) => reader.inject_array_buffer(ev.data);

		ws.onopen = () => open_resolve({
			read: (function*() { while(ws.readyState == 1) { yield reader.next(); } })(),
			write: (data) => {
				if (ws.readyState == 1) {
					ws.send(data);
					return promise_when_send_finished(ws);
				} else {
					return Promise.reject("WebSocket is closed");
				}
			},
			shutdown: () => ws.close(),
			close: () => {}
		});
	});
}

// Takes a WebSocket object and returns a Promise that resolves when bufferedAmount is low enough
// to allow more data to be sent.
const promise_when_send_finished = (ws) => {
	return new Promise((resolve, reject) => {
		function check() {
			if (ws.readyState != 1) {
				reject("WebSocket is closed");
				return;
			}

			// We put an arbitrary threshold of 8 kiB of buffered data.
			if (ws.bufferedAmount < 8 * 1024) {
				resolve();
			} else {
				setTimeout(check, 100);
			}
		}

		check();
	})
}

// Creates a queue reading system.
const read_queue = () => {
	// State of the queue.
	let state = {
		// Array of promises resolving to `ArrayBuffer`s, that haven't been transmitted back with
		// `next` yet.
		queue: new Array(),
		// If `resolve` isn't null, it is a "resolve" function of a promise that has already been
		// returned by `next`. It should be called with some data.
		resolve: null,
	};

	return {
		// Inserts a new Blob in the queue.
		inject_array_buffer: (buffer) => {
			if (state.resolve != null) {
				state.resolve(buffer);
				state.resolve = null;
			} else {
				state.queue.push(Promise.resolve(buffer));
			}
		},

		// Inserts an EOF message in the queue.
		inject_eof: () => {
			if (state.resolve != null) {
				state.resolve(null);
				state.resolve = null;
			} else {
				state.queue.push(Promise.resolve(null));
			}
		},

		// Returns a Promise that yields the next entry as an ArrayBuffer.
		next: () => {
			if (state.queue.length != 0) {
				return state.queue.shift(0);
			} else {
				if (state.resolve !== null)
					throw "Internal error: already have a pending promise";
				return new Promise((resolve, reject) => {
					state.resolve = resolve;
				});
			}
		}
	};
};
