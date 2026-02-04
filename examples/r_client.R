#!/usr/bin/env Rscript
#
# AstraeaDB R Client
#
# A demonstration client showing how to interface with AstraeaDB from R.
# Connects via TCP using the newline-delimited JSON protocol.
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
    host = "character",
    port = "integer",
    con  = "ANY"
  ),
  methods = list(
    initialize = function(host = "127.0.0.1", port = 7687L) {
      host <<- host
      port <<- as.integer(port)
      con  <<- NULL
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

    # ── Health ──────────────────────────────────────────

    ping = function() {
      "Health check. Returns server info."
      check(send(list(type = "Ping")))
    },

    # ── Node Operations ────────────────────────────────

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

    # ── Edge Operations ────────────────────────────────

    create_edge = function(source, target, edge_type,
                           properties = list(), weight = 1.0) {
      "Create an edge. Returns the edge ID."
      data <- check(send(list(
        type       = "CreateEdge",
        source     = source,
        target     = target,
        edge_type  = edge_type,
        properties = properties,
        weight     = weight
      )))
      data$edge_id
    },

    get_edge = function(edge_id) {
      "Get an edge by ID."
      check(send(list(type = "GetEdge", id = edge_id)))
    },

    delete_edge = function(edge_id) {
      "Delete an edge."
      check(send(list(type = "DeleteEdge", id = edge_id)))
    },

    # ── Traversal ──────────────────────────────────────

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
    }
  )
)


# ── Demo ─────────────────────────────────────────────────────

demo_social_network <- function(client) {
  cat(strrep("=", 60), "\n")
  cat("AstraeaDB R Client Demo: Social Network\n")
  cat(strrep("=", 60), "\n")

  # ── Create people ──
  cat("\n1. Creating nodes (people)...\n")
  alice   <- client$create_node(list("Person"), list(name = "Alice",   age = 30, city = "NYC"))
  bob     <- client$create_node(list("Person"), list(name = "Bob",     age = 25, city = "London"))
  charlie <- client$create_node(list("Person"), list(name = "Charlie", age = 35, city = "Tokyo"))
  diana   <- client$create_node(list("Person"), list(name = "Diana",   age = 28, city = "Paris"))
  eve     <- client$create_node(list("Person"), list(name = "Eve",     age = 32, city = "Berlin"))
  cat(sprintf("   Created: Alice(id=%d), Bob(id=%d), Charlie(id=%d), Diana(id=%d), Eve(id=%d)\n",
              alice, bob, charlie, diana, eve))

  # ── Create relationships ──
  cat("\n2. Creating edges (relationships)...\n")
  client$create_edge(alice, bob,     "KNOWS",   list(since = 2020), weight = 0.9)
  client$create_edge(alice, charlie, "KNOWS",   list(since = 2018), weight = 0.7)
  client$create_edge(bob,   diana,   "KNOWS",   list(since = 2021), weight = 0.8)
  client$create_edge(charlie, diana, "KNOWS",   list(since = 2019), weight = 0.6)
  client$create_edge(diana, eve,     "KNOWS",   list(since = 2022), weight = 0.95)
  client$create_edge(alice, eve,     "FOLLOWS", list(since = 2023), weight = 0.3)
  cat("   Created 6 edges (5 KNOWS + 1 FOLLOWS)\n")

  # ── Read back ──
  cat("\n3. Reading nodes...\n")
  alice_data <- client$get_node(alice)
  cat(sprintf("   Alice: labels=%s, properties=%s\n",
              toJSON(alice_data$labels, auto_unbox = TRUE),
              toJSON(alice_data$properties, auto_unbox = TRUE)))

  # ── Update ──
  cat("\n4. Updating Alice's properties...\n")
  client$update_node(alice, list(city = "San Francisco", title = "Engineer"))
  alice_data <- client$get_node(alice)
  cat(sprintf("   Alice now: %s\n",
              toJSON(alice_data$properties, auto_unbox = TRUE)))

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

  incoming <- client$neighbors(diana, "incoming")
  cat(sprintf("   Who knows Diana: %d people\n", length(incoming)))
  for (n in incoming) {
    source <- client$get_node(n$node_id)
    cat(sprintf("     <- %s\n", source$properties$name))
  }

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

  # ── Delete ──
  cat("\n8. Deleting Eve...\n")
  client$delete_node(eve)
  result <- client$shortest_path(alice, eve, weighted = FALSE)
  if (is.null(result$path)) {
    cat("   No path from Alice to Eve (Eve was deleted)\n")
  }

  # ── Ping ──
  cat("\n9. Server health check...\n")
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
