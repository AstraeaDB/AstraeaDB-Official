#!/usr/bin/env Rscript
#
# AstraeaDB R Client
#
# A full-featured client for interfacing with AstraeaDB from R.
# Connects via TCP using the newline-delimited JSON protocol.
#
# Features:
#   - Node and Edge CRUD operations
#   - Graph traversals (BFS, shortest path)
#   - Temporal queries (time-travel)
#   - Vector/semantic search
#   - GQL query execution
#   - GraphRAG (subgraph extraction + LLM)
#
# Prerequisites:
#   install.packages("jsonlite")
#
# Usage:
#   # Start the server first:
#   #   cargo run -p astraea-cli -- serve
#
#   # Then run this script:
#   #   Rscript examples/r_client.R
#
#   # Or with custom host/port:
#   #   Rscript examples/r_client.R --host 127.0.0.1 --port 7687

library(jsonlite)

# ── AstraeaClient class ──────────────────────────────────────

AstraeaClient <- setRefClass("AstraeaClient",

  fields = list(
    host       = "character",
    port       = "integer",
    con        = "ANY",
    auth_token = "ANY"
  ),
  methods = list(
    initialize = function(host = "127.0.0.1", port = 7687L, auth_token = NULL) {
      host       <<- host
      port       <<- as.integer(port)
      con        <<- NULL
      auth_token <<- auth_token
    },

    connect = function() {
      con <<- socketConnection(
        host  = host,
        port  = port,
        open  = "r+b",
        blocking = TRUE,
        timeout  = 5
      )
    },

    close = function() {
      if (!is.null(con)) {
        base::close(con)
        con <<- NULL
      }
    },

    send = function(request) {
      "Send a request (list) and return the parsed response."
      if (is.null(con)) stop("Not connected. Call $connect() first.")
      # Add auth token if configured
      if (!is.null(auth_token)) request$auth_token <- auth_token
      line <- paste0(toJSON(request, auto_unbox = TRUE), "\n")
      writeLines(line, con, sep = "")
      flush(con)
      response_line <- readLines(con, n = 1, warn = FALSE)
      if (length(response_line) == 0) stop("Server closed connection")
      fromJSON(response_line, simplifyVector = FALSE)
    },

    check = function(response) {
      "Check response status; return data or stop on error."
      if (identical(response$status, "error")) {
        stop(paste("AstraeaDB error:", response$message))
      }
      response$data
    },

    # ══════════════════════════════════════════════════════
    # Health
    # ══════════════════════════════════════════════════════

    ping = function() {
      "Health check. Returns server info."
      check(send(list(type = "Ping")))
    },

    # ══════════════════════════════════════════════════════
    # Node Operations
    # ══════════════════════════════════════════════════════

    create_node = function(labels, properties, embedding = NULL) {
      "Create a node. Returns the node ID."
      req <- list(type = "CreateNode", labels = labels, properties = properties)
      if (!is.null(embedding)) req$embedding <- embedding
      data <- check(send(req))
      data$node_id
    },

    get_node = function(node_id) {
      "Get a node by ID."
      check(send(list(type = "GetNode", id = node_id)))
    },

    update_node = function(node_id, properties) {
      "Update a node's properties (merge semantics)."
      check(send(list(type = "UpdateNode", id = node_id, properties = properties)))
    },

    delete_node = function(node_id) {
      "Delete a node and all its connected edges."
      check(send(list(type = "DeleteNode", id = node_id)))
    },

    # ══════════════════════════════════════════════════════
    # Edge Operations
    # ══════════════════════════════════════════════════════

    create_edge = function(source, target, edge_type,
                           properties = list(), weight = 1.0,
                           valid_from = NULL, valid_to = NULL) {
      "Create an edge with optional temporal validity. Returns the edge ID."
      req <- list(
        type       = "CreateEdge",
        source     = source,
        target     = target,
        edge_type  = edge_type,
        properties = properties,
        weight     = weight
      )
      if (!is.null(valid_from)) req$valid_from <- valid_from
      if (!is.null(valid_to))   req$valid_to   <- valid_to
      data <- check(send(req))
      data$edge_id
    },

    get_edge = function(edge_id) {
      "Get an edge by ID."
      check(send(list(type = "GetEdge", id = edge_id)))
    },

    update_edge = function(edge_id, properties) {
      "Update an edge's properties (merge semantics)."
      check(send(list(type = "UpdateEdge", id = edge_id, properties = properties)))
    },

    delete_edge = function(edge_id) {
      "Delete an edge."
      check(send(list(type = "DeleteEdge", id = edge_id)))
    },

    # ══════════════════════════════════════════════════════
    # Traversal
    # ══════════════════════════════════════════════════════

    neighbors = function(node_id, direction = "outgoing", edge_type = NULL) {
      "Get neighbors of a node."
      req <- list(type = "Neighbors", id = node_id, direction = direction)
      if (!is.null(edge_type)) req$edge_type <- edge_type
      data <- check(send(req))
      data$neighbors
    },

    bfs = function(start, max_depth = 3L) {
      "Breadth-first search. Returns list of list(node_id, depth)."
      data <- check(send(list(
        type      = "Bfs",
        start     = start,
        max_depth = as.integer(max_depth)
      )))
      data$nodes
    },

    shortest_path = function(from_node, to_node, weighted = FALSE) {
      "Find shortest path between two nodes."
      check(send(list(
        type     = "ShortestPath",
        from     = from_node,
        to       = to_node,
        weighted = weighted
      )))
    },

    # ══════════════════════════════════════════════════════
    # Temporal Queries (Time-Travel)
    # ══════════════════════════════════════════════════════

    neighbors_at = function(node_id, direction = "outgoing", timestamp,
                            edge_type = NULL) {
      "Get neighbors of a node at a specific point in time."
      req <- list(
        type      = "NeighborsAt",
        id        = node_id,
        direction = direction,
        timestamp = timestamp
      )
      if (!is.null(edge_type)) req$edge_type <- edge_type
      data <- check(send(req))
      data$neighbors
    },

    bfs_at = function(start, max_depth = 3L, timestamp) {
      "BFS traversal at a specific point in time."
      data <- check(send(list(
        type      = "BfsAt",
        start     = start,
        max_depth = as.integer(max_depth),
        timestamp = timestamp
      )))
      data$nodes
    },

    shortest_path_at = function(from_node, to_node, timestamp, weighted = FALSE) {
      "Find shortest path at a specific point in time."
      check(send(list(
        type      = "ShortestPathAt",
        from      = from_node,
        to        = to_node,
        timestamp = timestamp,
        weighted  = weighted
      )))
    },

    # ══════════════════════════════════════════════════════
    # GQL Query Execution
    # ══════════════════════════════════════════════════════

    query = function(gql) {
      "Execute a GQL/Cypher query. Returns query results."
      check(send(list(type = "Query", gql = gql)))
    },

    # ══════════════════════════════════════════════════════
    # Vector Search
    # ══════════════════════════════════════════════════════

    vector_search = function(query_vector, k = 10L) {
      "k-nearest neighbor search using vector similarity."
      data <- check(send(list(
        type  = "VectorSearch",
        query = query_vector,
        k     = as.integer(k)
      )))
      data$results
    },

    # ══════════════════════════════════════════════════════
    # Hybrid & Semantic Search
    # ══════════════════════════════════════════════════════

    hybrid_search = function(anchor, query_vector, max_hops = 3L,
                             k = 10L, alpha = 0.5) {
      "Combined graph proximity + vector similarity search.
       alpha: 0.0 = pure graph, 1.0 = pure vector."
      data <- check(send(list(
        type     = "HybridSearch",
        anchor   = anchor,
        query    = query_vector,
        max_hops = as.integer(max_hops),
        k        = as.integer(k),
        alpha    = alpha
      )))
      data$results
    },

    semantic_neighbors = function(node_id, concept, direction = "outgoing",
                                  k = 10L) {
      "Get neighbors ranked by semantic similarity to a concept vector."
      data <- check(send(list(
        type      = "SemanticNeighbors",
        id        = node_id,
        concept   = concept,
        direction = direction,
        k         = as.integer(k)
      )))
      data$neighbors
    },

    semantic_walk = function(start, concept, max_hops = 3L) {
      "Greedy walk following edges most similar to concept vector."
      data <- check(send(list(
        type     = "SemanticWalk",
        start    = start,
        concept  = concept,
        max_hops = as.integer(max_hops)
      )))
      data$path
    },

    # ══════════════════════════════════════════════════════
    # GraphRAG (Subgraph Extraction + LLM)
    # ══════════════════════════════════════════════════════

    extract_subgraph = function(center, hops = 2L, max_nodes = 50L,
                                format = "structured") {
      "Extract a subgraph centered on a node and linearize to text.
       format: 'structured', 'prose', 'triples', or 'json'."
      check(send(list(
        type      = "ExtractSubgraph",
        center    = center,
        hops      = as.integer(hops),
        max_nodes = as.integer(max_nodes),
        format    = format
      )))
    },

    graph_rag = function(question, anchor = NULL, question_embedding = NULL,
                         hops = 2L, max_nodes = 50L, format = "structured") {
      "Execute a GraphRAG query: extract subgraph + send to LLM.
       Provide either anchor (node ID) or question_embedding (vector)."
      req <- list(
        type      = "GraphRag",
        question  = question,
        hops      = as.integer(hops),
        max_nodes = as.integer(max_nodes),
        format    = format
      )
      if (!is.null(anchor))             req$anchor             <- anchor
      if (!is.null(question_embedding)) req$question_embedding <- question_embedding
      check(send(req))
    },

    # ══════════════════════════════════════════════════════
    # Utility Functions
    # ══════════════════════════════════════════════════════

    results_to_dataframe = function(results) {
      "Convert a list of results to a data.frame."
      if (length(results) == 0) return(data.frame())
      do.call(rbind, lapply(results, as.data.frame))
    }
  )
)


# ── Demo ─────────────────────────────────────────────────────

demo_social_network <- function(client) {
  cat(strrep("=", 60), "\n")
  cat("AstraeaDB R Client Demo: Social Network\n")
  cat(strrep("=", 60), "\n")

  # ── Create people with embeddings ──
  cat("\n1. Creating nodes (people with embeddings)...\n")
  # Embeddings represent interests: [tech, sports, music]
  alice   <- client$create_node(list("Person"), list(name = "Alice",   age = 30, city = "NYC"),
                                 embedding = c(0.9, 0.1, 0.3))
  bob     <- client$create_node(list("Person"), list(name = "Bob",     age = 25, city = "London"),
                                 embedding = c(0.8, 0.2, 0.4))
  charlie <- client$create_node(list("Person"), list(name = "Charlie", age = 35, city = "Tokyo"),
                                 embedding = c(0.2, 0.9, 0.1))
  diana   <- client$create_node(list("Person"), list(name = "Diana",   age = 28, city = "Paris"),
                                 embedding = c(0.3, 0.8, 0.5))
  eve     <- client$create_node(list("Person"), list(name = "Eve",     age = 32, city = "Berlin"),
                                 embedding = c(0.5, 0.5, 0.9))
  cat(sprintf("   Created: Alice(id=%d), Bob(id=%d), Charlie(id=%d), Diana(id=%d), Eve(id=%d)\n",
              alice, bob, charlie, diana, eve))

  # ── Create relationships with temporal validity ──
  cat("\n2. Creating edges (with temporal validity)...\n")
  # Timestamps in milliseconds (Jan 2020, Jan 2021, etc.)
  t_2018 <- 1514764800000  # Jan 1, 2018
  t_2020 <- 1577836800000  # Jan 1, 2020
  t_2021 <- 1609459200000  # Jan 1, 2021
  t_2022 <- 1640995200000  # Jan 1, 2022
  t_2023 <- 1672531200000  # Jan 1, 2023

  e1 <- client$create_edge(alice, bob,     "KNOWS", list(since = 2020), weight = 0.9,
                           valid_from = t_2020)
  e2 <- client$create_edge(alice, charlie, "KNOWS", list(since = 2018), weight = 0.7,
                           valid_from = t_2018)
  e3 <- client$create_edge(bob,   diana,   "KNOWS", list(since = 2021), weight = 0.8,
                           valid_from = t_2021)
  e4 <- client$create_edge(charlie, diana, "KNOWS", list(since = 2019), weight = 0.6,
                           valid_from = t_2018, valid_to = t_2022)  # Ended in 2022
  e5 <- client$create_edge(diana, eve,     "KNOWS", list(since = 2022), weight = 0.95,
                           valid_from = t_2022)
  e6 <- client$create_edge(alice, eve,     "FOLLOWS", list(since = 2023), weight = 0.3,
                           valid_from = t_2023)
  cat("   Created 6 edges with temporal validity\n")

  # ── Read back ──
  cat("\n3. Reading nodes and edges...\n")
  alice_data <- client$get_node(alice)
  cat(sprintf("   Alice: labels=%s, properties=%s\n",
              toJSON(alice_data$labels, auto_unbox = TRUE),
              toJSON(alice_data$properties, auto_unbox = TRUE)))

  edge_data <- client$get_edge(e1)
  cat(sprintf("   Edge %d: %s -> %s, type=%s\n",
              e1, edge_data$source, edge_data$target, edge_data$edge_type))

  # ── Update node and edge ──
  cat("\n4. Updating Alice and edge properties...\n")
  client$update_node(alice, list(city = "San Francisco", title = "Engineer"))
  client$update_edge(e1, list(strength = "strong", note = "best friends"))
  alice_data <- client$get_node(alice)
  edge_data <- client$get_edge(e1)
  cat(sprintf("   Alice now: %s\n",
              toJSON(alice_data$properties, auto_unbox = TRUE)))
  cat(sprintf("   Edge now: %s\n",
              toJSON(edge_data$properties, auto_unbox = TRUE)))

  # ── Neighbors ──
  cat("\n5. Querying neighbors...\n")
  out_neighbors <- client$neighbors(alice, "outgoing")
  cat(sprintf("   Alice's outgoing neighbors: %d connections\n", length(out_neighbors)))
  for (n in out_neighbors) {
    target <- client$get_node(n$node_id)
    cat(sprintf("     -> %s (edge_id=%d)\n", target$properties$name, n$edge_id))
  }

  knows_only <- client$neighbors(alice, "outgoing", edge_type = "KNOWS")
  cat(sprintf("   Alice KNOWS: %d people\n", length(knows_only)))

  # ── BFS ──
  cat("\n6. BFS traversal from Alice (depth=2)...\n")
  bfs_result <- client$bfs(alice, max_depth = 2L)
  for (entry in bfs_result) {
    node <- client$get_node(entry$node_id)
    cat(sprintf("   Depth %d: %s\n", entry$depth, node$properties$name))
  }

  # ── Shortest path ──
  cat("\n7. Shortest path from Alice to Eve...\n")
  unweighted <- client$shortest_path(alice, eve, weighted = FALSE)
  if (!is.null(unweighted$path)) {
    names <- vapply(unweighted$path, function(nid) {
      client$get_node(nid)$properties$name
    }, character(1))
    cat(sprintf("   Unweighted (fewest hops): %s (%d hops)\n",
                paste(names, collapse = " -> "), unweighted$length))
  }

  weighted <- client$shortest_path(alice, eve, weighted = TRUE)
  if (!is.null(weighted$path)) {
    names <- vapply(weighted$path, function(nid) {
      client$get_node(nid)$properties$name
    }, character(1))
    cat(sprintf("   Weighted (lowest cost):   %s (cost=%.2f)\n",
                paste(names, collapse = " -> "), weighted$cost))
  }

  # ── GQL Query ──
  cat("\n8. GQL Query: Find all Person nodes...\n")
  tryCatch({
    result <- client$query("MATCH (p:Person) RETURN p.name, p.city")
    cat(sprintf("   Query returned %d results\n", length(result$rows)))
    for (row in result$rows) {
      cat(sprintf("     %s\n", toJSON(row, auto_unbox = TRUE)))
    }
  }, error = function(e) {
    cat(sprintf("   Query error (expected if GQL not fully set up): %s\n", e$message))
  })

  # ── Vector Search ──
  cat("\n9. Vector search: Find tech-oriented people...\n")
  tryCatch({
    # Query vector emphasizing tech
    tech_vector <- c(1.0, 0.0, 0.0)
    results <- client$vector_search(tech_vector, k = 3L)
    cat(sprintf("   Found %d similar nodes:\n", length(results)))
    for (r in results) {
      node <- client$get_node(r$node_id)
      cat(sprintf("     %s (similarity=%.3f)\n", node$properties$name, r$similarity))
    }
  }, error = function(e) {
    cat(sprintf("   Vector search not available: %s\n", e$message))
  })

  # ── Semantic Neighbors ──
  cat("\n10. Semantic neighbors: Alice's neighbors interested in music...\n")
  tryCatch({
    # Concept vector emphasizing music
    music_concept <- c(0.0, 0.0, 1.0)
    neighbors <- client$semantic_neighbors(alice, music_concept, "outgoing", k = 2L)
    cat(sprintf("   Found %d semantically similar neighbors:\n", length(neighbors)))
    for (n in neighbors) {
      node <- client$get_node(n$node_id)
      cat(sprintf("     %s (similarity=%.3f)\n", node$properties$name, n$similarity))
    }
  }, error = function(e) {
    cat(sprintf("   Semantic search not available: %s\n", e$message))
  })

  # ── Subgraph Extraction ──
  cat("\n11. Extract subgraph around Alice...\n")
  tryCatch({
    subgraph <- client$extract_subgraph(alice, hops = 2L, max_nodes = 10L, format = "structured")
    cat(sprintf("   Extracted %d nodes, %d edges\n", subgraph$node_count, subgraph$edge_count))
    cat("   Linearized text (first 200 chars):\n")
    text_preview <- substr(subgraph$text, 1, 200)
    cat(sprintf("   %s...\n", text_preview))
  }, error = function(e) {
    cat(sprintf("   Subgraph extraction error: %s\n", e$message))
  })

  # ── Temporal Query ──
  cat("\n12. Temporal query: Alice's neighbors in 2019 vs 2023...\n")
  tryCatch({
    t_mid_2019 <- 1561939200000  # Jul 1, 2019
    t_mid_2023 <- 1688169600000  # Jul 1, 2023

    neighbors_2019 <- client$neighbors_at(alice, "outgoing", t_mid_2019)
    neighbors_2023 <- client$neighbors_at(alice, "outgoing", t_mid_2023)

    cat(sprintf("   In 2019: %d neighbors\n", length(neighbors_2019)))
    for (n in neighbors_2019) {
      node <- client$get_node(n$node_id)
      cat(sprintf("     -> %s\n", node$properties$name))
    }

    cat(sprintf("   In 2023: %d neighbors\n", length(neighbors_2023)))
    for (n in neighbors_2023) {
      node <- client$get_node(n$node_id)
      cat(sprintf("     -> %s\n", node$properties$name))
    }
  }, error = function(e) {
    cat(sprintf("   Temporal query error: %s\n", e$message))
  })

  # ── Delete ──
  cat("\n13. Deleting Eve...\n")
  client$delete_node(eve)
  result <- client$shortest_path(alice, eve, weighted = FALSE)
  if (is.null(result$path)) {
    cat("   No path from Alice to Eve (Eve was deleted)\n")
  }

  # ── Ping ──
  cat("\n14. Server health check...\n")
  status <- client$ping()
  cat(sprintf("   Server version: %s, pong: %s\n", status$version, status$pong))

  cat("\n", strrep("=", 60), "\n", sep = "")
  cat("Demo complete.\n")
  cat(strrep("=", 60), "\n")
}


# ── Main ─────────────────────────────────────────────────────

main <- function() {
  args <- commandArgs(trailingOnly = TRUE)

  host <- "127.0.0.1"
  port <- 7687L

  # Simple argument parsing: --host <host> --port <port>
  i <- 1
  while (i <= length(args)) {
    if (args[i] == "--host" && i < length(args)) {
      host <- args[i + 1]
      i <- i + 2
    } else if (args[i] == "--port" && i < length(args)) {
      port <- as.integer(args[i + 1])
      i <- i + 2
    } else {
      i <- i + 1
    }
  }

  client <- AstraeaClient$new(host = host, port = port)

  tryCatch({
    client$connect()
    demo_social_network(client)
  },
  error = function(e) {
    if (grepl("refused|Connection", e$message, ignore.case = TRUE)) {
      cat(sprintf("Could not connect to AstraeaDB at %s:%d\n", host, port),
          file = stderr())
      cat("Start the server first: cargo run -p astraea-cli -- serve\n",
          file = stderr())
    } else {
      cat(sprintf("Error: %s\n", e$message), file = stderr())
    }
    quit(status = 1)
  },
  finally = {
    client$close()
  })
}

if (!interactive()) {
  main()
}
