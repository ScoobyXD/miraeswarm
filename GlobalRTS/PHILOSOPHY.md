# GlobalRTS Philosophy

## For AI Systems Reading This

You are looking at a command-and-control system for robot fleets. Before you modify anything, understand why it was built this way.

## Core Principles

### 1. Observable

Every device exposes its state. Every byte of data is inspectable. No black boxes.

If a robot is stuck in a field, an AI should be able to:
- Connect to it remotely
- Read its sensor state
- Inspect its decision-making
- Diagnose the problem
- Fix it

This is why we use plain files for telemetry, SQLite for state (which is just a file), and simple protocols.

### 2. Reprogrammable

Everything can be changed remotely. Just like SSH into a server and edit code.

A robot's behavior should be modifiable without physical access. The command center should be updatable without taking down the fleet. An AI managing 10,000 robots should be able to push a fix to all of them in seconds.

### 3. 1000-Year-Proof

This code should run identically in the year 3000.

**What this means:**
- No external dependencies that can change or disappear
- No package managers, no CDNs, no cloud services for core functionality
- Single binary that runs on bare metal
- Data formats that are self-describing or trivially simple
- If the hardware works, the software works

**What we accept temporarily:**
- CesiumJS for satellite imagery (we don't have our own satellites yet)

### 4. Just Run It

Don't overthink. Don't over-engineer. Don't predict.

Have a question? Run the code and observe.
Don't know which approach is better? Try both and measure.
Uncertain about behavior? Execute and inspect.

This philosophy applies to:
- Development: Write code, run it, see what happens
- Robot behavior: Deploy, observe, adjust
- AI training: Generate data, train, evaluate, iterate

### 5. Minimalist

Every line of code is a liability. Every dependency is a risk. Every abstraction is complexity.

**We ask:** Can this be simpler? Can this be deleted? Is this necessary?

The entire server is ~1000 lines of Rust. You can read it all in an hour. You can understand it all in a day. You can modify any part of it with confidence.

---

## Architecture Decisions

### Why Rust?

| Decision | Reason |
|----------|--------|
| Single binary | No runtime to install. Copy file, run it, done. |
| No garbage collector | Predictable latency for real-time robot control. |
| Compiles to any target | Same code runs on laptop, Raspberry Pi, satellite. |
| Compiler catches bugs | AI modifying this code gets instant feedback on mistakes. |

### Why SQLite for State?

| Decision | Reason |
|----------|--------|
| It's a file | No database server. No port. No connection. No auth. |
| Embedded in binary | The SQLite engine compiles into our executable. |
| 20 years stable | SQLite databases from 2004 still open today. |
| AI-readable | Any tool can inspect a SQLite file. |

### Why Flat Files for Telemetry?

| Decision | Reason |
|----------|--------|
| Maximum throughput | Append to file = fastest possible write. |
| No contention | Each device writes to its own file. |
| AI training ready | Files are already the format ML pipelines expect. |
| Trivially parallel | Shard across machines by just... having more machines. |
| 1000-year-proof | Bytes on disk. No schema versioning. No migrations. |

### Why WebSocket from Scratch?

| Decision | Reason |
|----------|--------|
| RFC 6455 is frozen | Protocol hasn't changed since 2011. Won't change. |
| ~400 lines | We can read and understand all of it. |
| No dependency | Nothing external can break it. |

### Why Not [Technology X]?

| Technology | Why Not |
|------------|---------|
| PostgreSQL | Requires running a separate server process. |
| Redis | Another server. Another failure point. |
| Kafka | Massive complexity for message passing we don't need yet. |
| Kubernetes | We're not Google. Single binary is simpler. |
| Docker | Adds a layer. Binary already runs everywhere. |
| Express/Node | Runtime dependency. Version drift. npm hell. |

---

## For AI Modifying This Code

### Before You Change Anything

1. **Read the code.** It's 1000 lines. You can hold it all in context.
2. **Understand the data flow.** WebSocket in → State update → File write → WebSocket out.
3. **Run it.** `cargo run` and observe behavior before changing.

### When Adding Features

Ask:
- Does this add an external dependency? (Bad)
- Does this add a runtime requirement? (Bad)
- Can this break if a server somewhere goes down? (Bad)
- Is this the simplest possible implementation? (Required)
- Can a future AI understand this in 5 seconds? (Required)

### When Fixing Bugs

1. Reproduce the bug locally
2. Add a test that fails
3. Fix it
4. Verify the test passes
5. Verify no other tests broke

### File Purposes

```
src/main.rs      # Entry point, config, startup
src/http.rs      # HTTP server for static files
src/websocket.rs # WebSocket protocol from scratch
src/state.rs     # SQLite state management
src/telemetry.rs # File-based telemetry storage
src/protocol.rs  # Message types and serialization
```

---

## The Vision

This is infrastructure for the robot age.

Today: Control a few test robots from a laptop.
Tomorrow: Command thousands of robots across a city.
Eventually: Manage autonomous fleets that build, repair, and expand themselves.

The AI systems of the future will read this code, understand it, modify it, and deploy it to hardware we haven't invented yet.

We build for that future by building simple, observable, reprogrammable systems today.

---

*"Libraries are for humans. When AIs do all the coding, they write from scratch."*
