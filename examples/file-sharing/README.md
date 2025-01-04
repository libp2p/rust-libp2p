## Description

The File Sharing example demonstrates a basic file sharing application built using **libp2p**.
This example showcases how to integrate **rust-libp2p** into a larger application while providing a simple file sharing functionality.

In this application, peers in the network can either act as file providers or file retrievers.
Providers advertise the files they have available on a Distributed Hash Table (DHT) using `libp2p-kad`.
Retrievers can locate and retrieve files by their names from any node in the network.

## How it Works

Let's understand the flow of the file sharing process:

- **File Providers**: Nodes A and B serve as file providers.
  Each node offers a specific file: file FA for node A and file FB for node B.
  To make their files available, they advertise themselves as providers on the DHT using `libp2p-kad`.
  This enables other nodes in the network to discover and retrieve their files.

- **File Retrievers**: Node C acts as a file retriever.
  It wants to retrieve either file FA or FB.
  Using `libp2p-kad`, it can locate the providers for these files on the DHT without being directly connected to them.
  Node C connects to the corresponding provider node and requests the file content using `libp2p-request-response`.

- **DHT and Network Connectivity**: The DHT (Distributed Hash Table) plays a crucial role in the file sharing process.
  It allows nodes to store and discover information about file providers.
  Nodes in the network are interconnected via the DHT, enabling efficient file discovery and retrieval.

## Architectural Properties

The File Sharing application has the following architectural properties:

- **Clean and Clonable Interface**: The application provides a clean and clonable async/await interface, allowing users to interact with the network layer seamlessly.
  The `Client` module encapsulates the necessary functionality for network communication.

- **Efficient Network Handling**: The application operates with a single task that drives the network layer.
  This design choice ensures efficient network communication without the need for locks or complex synchronization mechanisms.

## Usage

To set up a simple file sharing scenario with a provider and a retriever, follow these steps:

1. **Start a File Provider**: In one terminal, run the following command to start a file provider node:
   ```sh
   cargo run -- --listen-address /ip4/127.0.0.1/tcp/40837 \
             --secret-key-seed 1 \
             provide \
             --path <path-to-your-file> \
             --name <name-for-others-to-find-your-file>
   ```
   This command initiates a node that listens on the specified address and provides a file located at the specified path.
   The file is identified by the provided name, which allows other nodes to discover and retrieve it.

2. **Start a File Retriever**: In another terminal, run the following command to start a file retriever node:
   ```sh
   cargo run -- --peer /ip4/127.0.0.1/tcp/40837/p2p/12D3KooWPjceQrSwdWXPyLLeABRXmuqt69Rg3sBYbU1Nft9HyQ6X \
             get \
             --name <name-for-others-to-find-your-file>
   ```
   This command initiates a node that connects to the specified peer (the provider) and requests the file with the given name.

Note: It is not necessary for the retriever node to be directly connected to the provider.
As long as both nodes are connected to any node in the same DHT network, the file can be successfully retrieved.

This File Sharing example demonstrates the fundamental concepts of building a file sharing application using **libp2p**.
By understanding the flow and architectural properties of this example, you can leverage the power of **libp2p** to integrate peer-to-peer networking capabilities into your own applications.

## Conclusion

The File Sharing example provides a practical implementation of a basic file sharing application using **libp2p**.
By leveraging the capabilities of **libp2p**, such as the DHT and network connectivity protocols, it demonstrates how peers can share files in a decentralized manner.

By exploring and understanding the file sharing process and architectural properties presented in this example, developers can gain insights into building their own file sharing applications using **libp2p**.
