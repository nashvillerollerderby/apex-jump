# apex-jump

A configurable WebSocket proxy server for the CRG Scoreboard.

Nashville Roller Derby has run into issues with WebSocket clients on the CRG Scoreboard overloading the application and
computer's memory
space to the extent the computer has had to be shut down. This tool is meant to help solve that problem.

The `apex-jump` tool connects to the CRG Scoreboard's WebSocket, and allows for clients to connect to its websocket
endpoint. This means that regardless of how many WebSocket connections `apex-jump` has connected to it, the CRG
Scoreboard will only see a single WebSocket connection.

Use cases for this tool include:

- Limiting the number of direct WebSocket clients connected to the CRG Scoreboard
- Providing a webserver for static files of an application that needs to consume data from the scoreboard WebSocket
- Providing a read-only view of the WebSocket data, thus allowing teams to safely connect their own devices

## Development

Below you will find overviews on how to contribute to this repository and how to get started developing `apex-jump`

### Contributing

Please read
the [generative AI/LLM policy](blob/main/https://github.com/nashvillerollerderby/apex-jump/blob/main/GENERATIVE-AI-POLICY.md)
before contributing to this repository.

If you intend to make changes, please open a fork, commit your changes to a branch there, and open a PR here.

### Prerequisites

You must have the Rust toolchain installed before trying to build this project.

### Getting Started

```bash
git clone https://github.com/nashvillerollerderby/apex-jump.git
cd apex-jump
cargo build
```

## Contributors

- [Charlie Humphreys](https://github.com/jeddai)
