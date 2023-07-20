// Copyright 2023 Doug Anderson
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

import { webRTC, webRTCDirect } from "./js-deps/jslibp2p.webrtc.2.0.10.js";
import {
    DefaultUpgrader,
    DefaultRegistrar,
    DefaultTransportManager,
} from "./js-deps/jslibp2p.0.45.2.js";
import { createFromPrivKey } from "./js-deps/peer-id-factory.esm.2.0.3.js";
import { unmarshalPrivateKey } from "./js-deps/libp2p.crypto.unmarshalPrivateKey.1.0.17.js";

/**
 * @arg {Uint8array} encoded - protobuf encoded private key
 */
export const webrtc_transport = async (encoded) => {
    /**
     * @param {WebRTCTransportInit} init
     * @returns {(components: WebRTCTransportComponents) => WebRTCTransport}
     */
    const init = {};
    const transportFn = new webRTC(init);

    // create a new WebRTC transport
    /**
     * @param {WebRTCTransportComponents} components
     * @returns {Transport}
     */
    /**
     * export interface WebRTCTransportComponents {
     *         peerId: PeerId
     *         registrar: Registrar
     *         upgrader: Upgrader
     *         transportManager: TransportManager
     *         }
     */
    const privateKey = await unmarshalPrivateKey(encoded);
    const peerId = await createFromPrivKey(privateKey);

    /**
     * interface DefaultUpgraderComponents {
     *         peerId: PeerId
     *         metrics?: Metrics
     *         connectionManager: ConnectionManager
     *         connectionGater: ConnectionGater
     *         connectionProtector?: ConnectionProtector
     *         registrar: Registrar
     *         peerStore: PeerStore
     *         events: EventEmitter<Libp2pEvents>
     *         }
     */
    // Set up the Upgrader
    const upgrader = new DefaultUpgrader(
        { peerId },
        {
            connectionEncryption: [],
            muxers: [],
        }
    );

    const registrar = new DefaultRegistrar({ upgrader }); // registrar is only used in start() and stop() in js-libp2p
    const transportManager = new DefaultTransportManager({ upgrader }, {}); // transportManager is used to dial and create the connection
    const components = { peerId, upgrader, registrar, transportManager }; // todo: figure out whaere this comes from
    const transport = transportFn(components);

    // pass in WebRTCTransportComponents to the transportFn
    return {
        dial: transport.dial,
        listen_on: transport.listen,
    };
};

export const webrtc_direct_transport = async (encoded) => {
    /**
     * @param {WebRTCDirectTransportInit} init
     * @returns {(components: WebRTCDirectTransportComponents) => WebRTCDirectTransport}
     */
    const init = {};
    const transportFn = new webRTCDirect(init);

    // create a new WebRTC transport
    /**
     * @param {WebRTCDirectTransportComponents} components
     * @returns {Transport}
     */
    const privateKey = await unmarshalPrivateKey(encoded);
    const peerId = await createFromPrivKey(privateKey);
    const components = { peerId }; // todo: figure out whaere this comes from
    const transport = transportFn(components);

    // pass in WebRTCTransportComponents to the transportFn
    return {
        dial: transport.dial,
        listen_on: transport.listen,
    };
};
