const Socket = require('node:net').Socket;
const createServer = require('node:net').createServer;

module.exports.tcp_transport = () => {
    return {
        dial: dial,
        listen_on: listen_on
    }
}

/// Convert a string multiaddress into a host/port tuple.
const multiaddr_to_tcp_host_and_port = (addr) => {
    let parsed = addr.match(/^\/(ip4|ip6|dns4|dns6|dns)\/(.*?)\/tcp\/(\d+)(.*?)$/);
    if (parsed != null) {
        return {
            host: parsed[2],
            port: Number(parsed[3]),
        };
    }

    let err = new Error("Address not supported: " + addr);
    err.name = "NotSupportedError";
    throw err;
}

/// Convert a host/port/family tuple into a multiaddr
const tcp_host_family_port_to_multiaddr = (host, port, family) => {
    if (family != 'IPv4' && family != 'IPv6') {
        let err = new Error("Address family not supported: " + family);
        err.name = "NotSupportedError";
        throw err;
    }

    const family_ma = (family == 'IPv4') ? 'ip4' : 'ip6'
    return `/${family_ma}/${host}/tcp/${port}`
}

/// Create a connection object.
let to_connection = (socket, reader) => {
    return {
        read: (function* () { while (socket.readyState == 'open') { yield reader.next(); } })(),
        write: (data) => {
            if (socket.readyState == 'open') {
                let state = { done: false }
                socket.write(data.slice(0), () => state.done = true);

                return new Promise((resolve, reject) => {
                    function check() {
                        if (state.done) {
                            resolve();
                            return;
                        }
                        
                        if (socket.readyState != 'open') {
                            reject("Socket is not open");
                            return;
                        }
                        
                        setTimeout(check, 1);
                    }

                    check();
                })
            } else {
                return Promise.reject("Socket is closed");
            }
        },
        shutdown: () => socket.destroy(),
        close: () => socket.end()
    }
}

/// Attempt to dial a multiaddress.
const dial = (addr) => {
    return new Promise((resolve, reject) => {
        let reader = read_queue();
        const target = multiaddr_to_tcp_host_and_port(addr);
        let socket = new Socket();

        socket.on('error', (ev) => {
            // If `resolve` has been called earlier, calling `reject` seems to be
            // silently ignored. It is easier to unconditionally call `reject` rather than
            // check in which state the connection is, which would be error-prone.
            reject(ev);
            // Injecting an EOF is how we report to the reading side that the connection has been
            // closed. Injecting multiple EOFs is harmless.
            reader.inject_eof();
        })

        socket.on('close', (ev) => {
            // Same remarks as above.
            reject(ev);
            reader.inject_eof();
        })

        socket.on('end', (ev) => {
            // Same remarks as above.
            reject(ev);
            reader.inject_eof();
        })

        socket.on('data', (ev) => reader.inject_array_buffer(ev));
        socket.connect(target, () => resolve(to_connection(socket, reader)))
    });
}

/// Attempt to listen on a multiaddress
const listen_on = (addr) => {
    let listen_state = {
        events: [],
        resolve: null
    }

    const push_event = (event) => {
        if (listen_state.resolve == null) {
            listen_state.events.push(event)
        } else {
            listen_state.resolve(event)
            listen_state.resolve = null
        }
    }

    const listen_event = (new_addreses, exp_addrs, new_conns) => {
        return {
            new_addrs: new_addreses,
            expired_addrs: exp_addrs,
            new_connections: new_conns,
            // NOTE: after going thought the libp2p-wasm-ext source, this does not 
            // appear to be used anywhere, but instead the iterator is used to extract
            // the next ListenEvent
            next_event: Promise.resolve(),
        }
    };

    const connection_event = (socket) => {
        let reader = read_queue();

        socket.on('error', (ev) => {
            reader.inject_eof();
            socket.destroy();
        })
        socket.on('close', (ev) => {
            reader.inject_eof();
            
        })

        // We inject all incoming messages into the queue unconditionally. The caller isn't
        // supposed to access this queue unless the connection is open.
        socket.on('data', (ev) => reader.inject_array_buffer(ev));

        return {
            connection: to_connection(socket, reader),
            observed_addr: tcp_host_family_port_to_multiaddr(socket.remoteAddress, socket.remotePort, socket.remoteFamily),
            local_addr: addr
        }
    }

    // initiate the socket
    const port = multiaddr_to_tcp_host_and_port(addr).port
    let server = createServer()
    server.on('listening', () => {
        push_event(listen_event([addr], undefined, undefined))
    })
    server.on('connection', (socket) => {
        push_event(listen_event(undefined, undefined, [connection_event(socket)]))
    })
    server.on('error', (e) => { server.destroy(); })
    server.on('close', () => {
        push_event(listen_event(undefined, [addr], undefined))
    })
    server.on('drop', (data) => {
        console.error("Connection dropped due to incoming connection limit")
    })

    server.listen(port)

    const iterator = {
        next() {
            if (listen_state.events.length > 0) {
                return {
                    value: Promise.resolve(listen_state.events.shift(0)),
                    done: false
                }
            } else {
                if (listen_state.resolve !== null)
                    throw new Error('Internal error: already have a pending promise');

                if (server.listening) {
                    return {
                        value: new Promise((resolve, reject) => {
                            listen_state.resolve = resolve;
                        }),
                        done: false
                    };
                } else {
                    return {
                        value: Promise.resolve(),
                        done: true
                    };
                }
            }
        },
    };

    return {
        events: iterator,
        finalizer: () => { server.close() }
    }
}


/// Creates a queue reading system.
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
                state.queue.push(buffer);
            }
        },

        // Inserts an EOF message in the queue.
        inject_eof: () => {
            if (state.resolve != null) {
                state.resolve(null);
                state.resolve = null;
            } else {
                state.queue.push(null);
            }
        },

        // Returns a Promise that yields the next entry as an ArrayBuffer.
        next: () => {
            if (state.queue.length != 0) {
                return Promise.resolve(state.queue.shift(0));
            } else {
                if (state.resolve !== null)
                    throw new Error('Internal error: already have a pending promise');
                return new Promise((resolve, reject) => {
                    state.resolve = resolve;
                });
            }
        }
    };
};
