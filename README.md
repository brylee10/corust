# Corust

Corust ("Collaborative Rust") is a Rust collaborative code editor with code execution. Try it out at [corust.dev](https://www.corust.dev/) - it's more fun with friends!

<img src="https://i.imgur.com/eV9rCUP.png" alt="Corust - A collaborative code editor" style="width: 1000px; border-radius: 15px;">

Inspired by the [Rust Playground](https://play.rust-lang.org/) -- thanks to the prolific [Kirby](https://github.com/shepmaster)-- Corust allows users to quickly develop code snippets but now while collaborating live with others. 

Corust supports live collaboration, the ability to compile in debug or release mode with stable, beta, or nightly versions of Rust, editor syntax highlighting, and the top ~200+ crates.

## Architecture
Corust uses a [React (NextJS)](https://nextjs.org/) frontend, an [Axum](https://docs.rs/axum/latest/axum/) webserver, and the [CodeMirror](https://codemirror.net/) code editor (made by Marijn Haverbeke, an indispensible open source contributor). The server archives state in a [SQLite](https://sqlite.org/index.html) database for easy self hosting. The Corust UI attempts to mirror familiar designs from the Rust Playground. Code is executed inside Docker containers which support the various Rust channels and crates. Collaboration and conflict resolution is enabled by the [operational transform (OT) algorithm](https://web.archive.org/web/20120107060932/http://www.codecommit.com/blog/java/understanding-and-applying-operational-transformation). In unit tests, the [Docker](https://www.docker.com/) execution environment is replaced with a Rust project initialized in the temporary file system. Corust has a simulated "network" for unit testing the client and server under different edit sequences for text and cursor validation. This client is compiled to [WebAssembly](https://www.rust-lang.org/what/wasm) and used by the frontend.

## Execution
Corust allows one execution per session at a time and the current server code is the source of truth for the version of code that is executed. The execution logs of this run are streamed to all session participants. 

Sandboxed execution occurs in Docker images (built with `rust/Dockerfile`). Execution is time limited (see `entrypoint.sh`) and Docker containers are run with the following command:
```
docker run --cap-drop ALL --network none --memory 128m --memory-swap 128m --pids-limit 128 --oom-score-adj 1000 ...
```
Where:
- `--cap-drop ALL`: Drops all Linux capabilities for the container, meaning the container runs with the minimum set of privileges (e.g. no socket creation, changing file ownership).
- `--network none`: Disables all network interfaces except for the loopback device, effectively isolating the container from the network.
- `--memory 128m`: Limits the container's memory usage to 512 MB. This means the container can use up to 128 MB of RAM.
- `--memory-swap 128m`: Sets the total memory usage limit to 512 MB, which includes both physical RAM and swap space. 
- `--pids-limit 128`: Limits the number of process IDs (PIDs) that can be used by the container to 128. This limits the number of processes that can be run simultaneously within the container.
- `--oom-score-adj 1000`: Adjusts the OOM (Out-Of-Memory) killer score for the container. A score of 1000 sets the container to the highest priority for being killed when the system is out of memory.

The text output of a process is also limited to `STDOUT_ERR_BYTE_LIMIT` and concurrently executing containers is limited to `MAX_CONCURRENT_CONTAINERS`. The Corust sandbox has ~200+ top crates, as taken from [lib.rs/std](https://lib.rs/std). Thank you to [Kornel](https://github.com/kornelski) for responding to a request to create [the Atom feed](https://lib.rs/std.atom) this project uses. This is populated one-off with `rust/populate_crates`.

Much inspiration was taken from the Rust Playground's architecture for code execution.

## Deployment
Currently, the Corust frontend is deployed on AWS Amplify and the server and code execution environment are run on an AWS EC2 machine. Follow the below to run Corust (the frontend (`site`) and backend (`rust`)) locally.

### UI
The `site/Makefile` specifies commands to run to start the frontend, compile the required Rust components into WASM binaries, install them as a node module, and start the NextJS development server. 
```
# Front end
cd site
make dev
```
This will run the development build locally.

Configure a `.env` file in `site` with the following environment variables to send requests to the server
```
NEXT_PUBLIC_WEBSOCKET_URI=ws://127.0.0.1:8000
NEXT_PUBLIC_ENDPOINT_URI=http://127.0.0.1:8000
```

### Server
```
cd rust/app
cargo run
```
The runtime environment can be configured via a `.env` file in the `rust` directory via `dotenv`.

```
# Front end host, used for CORS whitelisting
FRONT_END_URI=http://localhost:3000
# Address for the WS server
WS_SERVER_URI=127.0.0.1
# Server port
PORT=8000
# Log level for server
RUST_LOG=INFO
# Log level for docker container the server spawns
DOCKER_LOG_LEVEL=INFO
# Path to a SQLite database
DB_PATH="/Users/bylee/code/corust/corust.db"
```

### Docker Containers
You can build the docker containers from scratch via
```
cd deployment
./build.sh
```
or you can fetch the currently used containers from Docker Hub
```
cd deployment
./fetch.sh
```

## License
Licensed under Apache License 2.0 or MIT at your selection