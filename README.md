# Corust

Corust ("Collaborative Rust") is a Rust collaborative code editor with code execution. Try it out at [corust.dev](https://www.corust.dev/) - it's more fun with friends!

<img src="https://i.imgur.com/FvtzlpI.png" alt="Corust - A collaborative code editor" style="width: 1000px; border-radius: 15px;">

**Note: This project is still under development.** Improvements include: support for a larger number of concurrent sessions, more Rust compilation modes (beyond a release mode executable) like the Rust Playground, and database-backed session persistence.

# Overview
Inspired by the canonical [Rust Playground](https://play.rust-lang.org/) in the Rust ecosystem (thanks to the prolific [Kirby](https://github.com/shepmaster)), Corust allows users to quickly develop code snippets but now while collaborating live with others. 

Collaborative editing is enabled with operational transform (OT), a non blocking, eventually consistent, conflict resolution algorithm for text. It is implemented in this repository primarily for the author's education, but Rust has a well used [operational transform](https://docs.rs/operational-transform/latest/operational_transform/) crate. As [Marijn Haverbeke points out](https://marijnhaverbeke.nl/blog/collaborative-editing-cm.html), most theoretical work in collaborative editing is regarding truly distributed collaborative editing, but Corust is implemented more simply with a client server architecture. Using OT as a building block, Corust takes inspiration from steps outlined in [David Spiewak's overview](https://web.archive.org/web/20120107060932/http://www.codecommit.com/blog/java/understanding-and-applying-operational-transformation) of Novell Pulse's implementation to coordinate update synchronization and conflict resolution between clients and a server. Fun fact: in Corust's simulated editing unit tests, a "most formidable" test case is taken from the last example in David's aforementioned post.  

## Components
The Corust web backend uses a Warp webserver with the tokio asynchronous runtime. The frontend is built in React with Typescript and uses WebAssembly as the compile target to run the Rust client. The code editor uses [CodeMirror](https://codemirror.net/) made by Marijn Haverbeke, an indispensible contributor in open source code editing. The Corust UI attempts to mirror familiar designs from the Rust Playground. The CodeMirror React component detects certain text update operations and these are translated into Corust operations which are sent from client to server.

Corust provides both live text and cursor updates, synchronized streaming of execution logs, live user presence tracking, and shareable session IDs which are persistent* (*SQL server to be added, currently still in memory).

For testing text and cursor resolution, Corust implements a simulated message passing test framework (called a "network") in `rust/components/network`. This uses the foundational client and server building blocks which are reused in the production app (`rust/app`) but allows for precise timing and sequencing of both text and cursor updates between an arbitrary number of clients.

## Execution
Corust allows one execution per session at a time and the current server code is the source of truth for the version of code that is executed. The execution logs of this run are streamed to all session participants. 

Sandboxed execution occurs in Docker images (built with `rust/Dockerfile`). Execution is time limited (see `entrypoint.sh`) and Docker containers are run with the following command:
```
docker run --cap-drop ALL --network none --memory 512m --memory-swap 512m --pids-limit 128 --oom-score-adj 1000 ...
```
Where:
- `--cap-drop ALL`: Drops all Linux capabilities for the container, meaning the container runs with the minimum set of privileges (e.g. no socket creation, changing file ownership).
- `--network none`: Disables all network interfaces except for the loopback device, effectively isolating the container from the network.
- `--memory 512m`: Limits the container's memory usage to 512 MB. This means the container can use up to 512 MB of RAM.
- `--memory-swap 512m`: Sets the total memory usage limit to 512 MB, which includes both physical RAM and swap space. 
- `--pids-limit 128`: Limits the number of process IDs (PIDs) that can be used by the container to 128. This limits the number of processes that can be run simultaneously within the container.
- `--oom-score-adj 1000`: Adjusts the OOM (Out-Of-Memory) killer score for the container. A score of 1000 sets the container to the highest priority for being killed when the system is out of memory.

The text output of a process is also limited to `STDOUT_ERR_BYTE_LIMIT` and concurrently executing containers is limited to `MAX_CONCURRENT_CONTAINERS`. The Corust sandbox has 200 top crates, as taken from [lib.rs/std](https://lib.rs/std). Thank you to [Kornel](https://github.com/kornelski) for responding to a request to create [the Atom feed](https://lib.rs/std.atom) this project uses. This is populated one-off with `rust/populate_crates`.

In unit tests, the Docker execution environment is replaced with a Rust project initialized in the temporary file system. 

## Deployment
Currently, the Corust frontend is deployed on AWS Amplify and the server and code execution environment are run on AWS EC2 machine.  
