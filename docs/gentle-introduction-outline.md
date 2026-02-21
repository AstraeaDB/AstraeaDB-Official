# A Gentle Introduction to AstraeaDB — Outline

## Purpose
A comprehensive, beginner-friendly resource that progressively educates users from graph database fundamentals through advanced AstraeaDB features. Each chapter is a standalone HTML page, interlinked for sequential reading or topic-based browsing. Code examples are provided in Python, R, Go, and Java throughout.

---

## Part I: Foundations — What Are Graph Databases?

### Chapter 1: Why Graphs?
**Introduction:** Before diving into AstraeaDB, we build intuition for *why* graph databases exist and when they outshine relational databases.
- **1.1 The Limits of Tables** — A motivating example (social network, fraud ring, or supply chain) that's awkward in SQL but natural as a graph.
- **1.2 Nodes, Edges, and Properties** — The basic vocabulary: what is a node, what is an edge, what are properties? Visual diagrams.
- **1.3 Thinking in Connections** — How graph traversals differ from JOINs. The O(k) vs O(log N) insight (index-free adjacency).
- **1.4 Real-World Use Cases** — Brief survey: social networks, recommendation engines, fraud detection, cybersecurity, knowledge graphs, supply chain, life sciences.

### Chapter 2: The Graph Database Landscape
**Introduction:** A brief tour of the graph database ecosystem to contextualize where AstraeaDB fits.
- **2.1 Property Graphs vs RDF** — Two major models, their trade-offs, and why AstraeaDB chose the property graph model.
- **2.2 Query Languages** — Cypher, Gremlin, SPARQL, and the new ISO GQL standard.
- **2.3 What Makes AstraeaDB Different** — The "Vector-Property Graph" model, AI-first design, Rust performance, and unified architecture.

---

## Part II: Getting Started with AstraeaDB

### Chapter 3: Installation and Setup
**Introduction:** Get AstraeaDB running on your machine in minutes.
- **3.1 Prerequisites** — Rust toolchain, supported platforms (Linux, macOS).
- **3.2 Building from Source** — Clone, `cargo build --release`, verify with `cargo test`.
- **3.3 Starting the Server** — `astraea-cli serve` and configuration options (ports, bind address, TLS).
- **3.4 Your First Connection** — Connecting with each client library:
  - Python: `pip install ./python` and `AstraeaClient("localhost", 7687)`
  - R: Sourcing `r_client.R` and `AstraeaClient$new("localhost", 7687)`
  - Go: `go get` and `astraeadb.NewClient("localhost:7687")`
  - Java: Gradle dependency and `AstraeaClient.builder().host("localhost").build()`
- **3.5 The Interactive Shell** — Using `astraea-cli shell` for ad-hoc queries.

### Chapter 4: Your First Graph
**Introduction:** A hands-on walkthrough creating a small graph, querying it, and understanding the results.
- **4.1 Creating Nodes** — Creating nodes with labels and properties. Examples: a small movie database (movies, actors, directors).
- **4.2 Creating Edges** — Connecting nodes with typed, directed edges (ACTED_IN, DIRECTED).
- **4.3 Retrieving Data** — Getting nodes and edges by ID. Understanding the response format.
- **4.4 Your First Query (GQL)** — `MATCH (a:Actor)-[:ACTED_IN]->(m:Movie) RETURN a.name, m.title`. Explanation of pattern matching.
- **4.5 Updating and Deleting** — Modifying properties, removing nodes and edges.
- **4.6 Bulk Import and Export** — Using `astraea-cli import` / `astraea-cli export` for JSON data.

---

## Part III: Intermediate — Querying and Traversals

### Chapter 5: The GQL Query Language
**Introduction:** A thorough guide to AstraeaDB's GQL implementation — the ISO-standard query language for property graphs.
- **5.1 Pattern Matching Deep Dive** — Node patterns `(n:Label)`, edge patterns `-[:TYPE]->`, variable-length paths.
- **5.2 Filtering with WHERE** — Comparison operators, boolean logic, property access, string matching.
- **5.3 RETURN, ORDER BY, SKIP, LIMIT** — Projections, sorting, pagination.
- **5.4 Aggregation Functions** — `count()`, `sum()`, `avg()`, `min()`, `max()` with grouping.
- **5.5 CREATE and DELETE in GQL** — Mutating the graph through queries.
- **5.6 Built-in Functions** — `id()`, `labels()`, `type()`, `toString()`, `toInteger()`, `DISTINCT`.

### Chapter 6: Graph Traversals
**Introduction:** Understanding and using AstraeaDB's traversal algorithms for pathfinding and exploration.
- **6.1 Breadth-First Search (BFS)** — When to use BFS, API usage, interpreting results.
- **6.2 Depth-First Search (DFS)** — When to use DFS, comparison with BFS.
- **6.3 Shortest Path** — Unweighted shortest path between two nodes.
- **6.4 Weighted Shortest Path (Dijkstra)** — Using edge weights for cost-optimized routing.
- **6.5 Neighbor Queries** — Filtering by direction (incoming, outgoing, both) and edge type.

### Chapter 7: Transport Protocols
**Introduction:** AstraeaDB offers three ways to communicate — choose the right one for your workload.
- **7.1 JSON over TCP** — Simple, human-readable, zero-dependency. Best for getting started and light workloads.
- **7.2 gRPC** — Strongly typed, efficient binary protocol. Best for microservices and production.
- **7.3 Apache Arrow Flight** — Zero-copy columnar data transfer. Best for analytics and DataFrame workflows.
- **7.4 Choosing the Right Protocol** — Decision matrix with trade-offs.

---

## Part IV: Advanced Features

### Chapter 8: Vector Search and Semantic Queries
**Introduction:** AstraeaDB's killer feature — vectors and graphs unified in a single data model.
- **8.1 What Are Vector Embeddings?** — Gentle introduction to embeddings, similarity, and why they matter for AI.
- **8.2 Adding Embeddings to Nodes** — Storing float32 arrays alongside properties.
- **8.3 Vector Search (k-NN)** — Finding the k most similar nodes by cosine/euclidean/dot-product distance.
- **8.4 Hybrid Search** — Combining vector similarity with graph proximity for more relevant results.
- **8.5 Semantic Neighbors and Semantic Walk** — Traversing the graph guided by semantic similarity.

### Chapter 9: Temporal Graphs (Time-Travel Queries)
**Introduction:** Edges in AstraeaDB can carry validity intervals, enabling queries across time.
- **9.1 Validity Intervals** — Setting `[t_start, t_end)` on edges.
- **9.2 Point-in-Time Queries** — "Who were Alice's coworkers on Jan 1, 2024?"
- **9.3 Temporal BFS and Shortest Path** — Traversals restricted to a moment in time.
- **9.4 Use Cases** — Organizational history, evolving networks, audit trails.

### Chapter 10: Graph Algorithms
**Introduction:** Built-in analytical algorithms for understanding graph structure at scale.
- **10.1 PageRank** — Identifying influential nodes. Configuration: damping factor, convergence.
- **10.2 Connected and Strongly Connected Components** — Finding clusters and isolated subgraphs.
- **10.3 Centrality (Degree and Betweenness)** — Measuring node importance by connections and bridging.
- **10.4 Community Detection (Louvain)** — Discovering natural groupings via modularity optimization.

### Chapter 11: GraphRAG — Graph-Powered AI
**Introduction:** Retrieval-Augmented Generation using graph context for more accurate, grounded LLM responses.
- **11.1 What Is GraphRAG?** — How graph context improves LLM answers vs. flat document retrieval.
- **11.2 Subgraph Extraction** — BFS-based and semantic subgraph retrieval.
- **11.3 Linearization Formats** — Structured, Prose, Triples, JSON — turning graphs into text.
- **11.4 LLM Integration** — Connecting to OpenAI, Anthropic (Claude), or Ollama.
- **11.5 End-to-End Example** — Building a knowledge-graph-powered Q&A system.

### Chapter 12: Graph Neural Networks (GNNs)
**Introduction:** AstraeaDB is the first graph database with built-in differentiable traversal and GNN training.
- **12.1 What Are GNNs?** — Message passing, node classification, and why databases should care.
- **12.2 Setting Up Training Data** — Labeling nodes, configuring message passing layers.
- **12.3 Training a Model** — Running `train_node_classification` and interpreting loss/accuracy.
- **12.4 Differentiable Edge Weights** — How backpropagation updates edge weights inside the database.

---

## Part V: Production and Operations

### Chapter 13: Security
**Introduction:** Securing your AstraeaDB deployment for production use.
- **13.1 Authentication (API Keys & RBAC)** — Creating users, assigning roles (Reader, Writer, Admin).
- **13.2 TLS and Mutual TLS (mTLS)** — Encrypting connections, client certificate authentication.
- **13.3 Homomorphic Encryption** — Querying encrypted data without exposing plaintext (banking/healthcare).
- **13.4 Audit Logging** — Tracking who did what and when.

### Chapter 14: Performance and Scaling
**Introduction:** Tuning AstraeaDB for high-throughput and large-scale deployments.
- **14.1 The Three-Tier Storage Architecture** — Understanding cold (S3/Parquet), warm (NVMe buffer pool), and hot (pointer swizzling) tiers.
- **14.2 Connection Management** — Configuring connection limits, backpressure, and timeouts.
- **14.3 Monitoring with Prometheus** — Metrics endpoint, key counters, and percentiles.
- **14.4 GPU Acceleration** — Offloading heavy algorithms (PageRank, BFS) to GPU/CPU backend.
- **14.5 Sharding and Distributed Processing** — Hash and range partitioning across a cluster.

### Chapter 15: Real-World Scenario — Cybersecurity Threat Investigation
**Introduction:** A capstone example tying together multiple features in a realistic cybersecurity use case.
- **15.1 Building the Threat Graph** — Modeling hosts, IPs, vulnerabilities, alerts, and attack patterns.
- **15.2 Investigating an Alert** — Using traversals to trace attack paths.
- **15.3 Enriching with AI** — Vector search for similar threats, GraphRAG for analyst briefings.
- **15.4 Temporal Analysis** — Understanding how the threat evolved over time.

---

## Appendices

### Appendix A: GQL Quick Reference
A one-page cheat sheet of GQL syntax supported by AstraeaDB.

### Appendix B: Client API Reference
Summary tables of all operations across Python, R, Go, and Java clients.

### Appendix C: Configuration Reference
Server configuration options (TOML format), CLI flags, and environment variables.

---

## Page Structure and Navigation

Each chapter will be a standalone HTML page (`gentle-intro-ch01.html`, `gentle-intro-ch02.html`, etc.) with:
- Consistent navigation bar matching the main site
- Previous/Next chapter links at top and bottom
- A sidebar table of contents for within-chapter navigation
- Code examples in tabbed panels (Python | R | Go | Java)
- Diagrams and visual aids where appropriate
- A central table of contents page (`gentle-intro.html`) linking to all chapters

Total: ~18 HTML pages (1 TOC + 15 chapters + 3 appendices)
