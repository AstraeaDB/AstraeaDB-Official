#!/usr/bin/env Rscript
#
# AstraeaDB R Client
#
# A full-featured client for interfacing with AstraeaDB from R.
# Provides complete feature parity with the Python client.
#
# Features:
#   - Node and Edge CRUD operations
#   - Graph traversals (BFS, shortest path)
#   - Temporal queries (time-travel)
#   - Vector/semantic search (k-NN, hybrid, semantic walk)
#   - GQL query execution
#   - GraphRAG (subgraph extraction + LLM)
#   - Batch operations (create_nodes, create_edges, delete_nodes, delete_edges)
#   - Data frame import/export (import_nodes_df, import_edges_df, export_nodes_df)
#   - Arrow Flight support (optional, for high-performance queries)
#   - Authentication support (auth_token)
#
# Client Classes:
#   - AstraeaClient: JSON/TCP client (always available)
#   - ArrowClient: Arrow Flight client (requires 'arrow' package)
#   - UnifiedClient: Auto-selects best transport
#
# Prerequisites:
#   install.packages("jsonlite")           # Required
#   install.packages("arrow")              # Optional, for Arrow Flight
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
        # Build detailed error message
        msg <- response$message
        if (!is.null(response$code)) {
          msg <- sprintf("[%s] %s", response$code, msg)
        }
        if (!is.null(response$details)) {
          msg <- sprintf("%s\nDetails: %s", msg, response$details)
        }
        stop(paste("AstraeaDB error:", msg), call. = FALSE)
      }
      response$data
    },

    check_silent = function(response) {
      "Check response status; return NULL on error instead of stopping."
      if (identical(response$status, "error")) {
        return(NULL)
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
    },

    nodes_to_dataframe = function(node_ids) {
      "Fetch multiple nodes and return as a data.frame."
      rows <- lapply(node_ids, function(nid) {
        node <- get_node(nid)
        data.frame(
          id = nid,
          labels = I(list(node$labels)),
          as.data.frame(node$properties),
          stringsAsFactors = FALSE
        )
      })
      do.call(rbind, rows)
    },

    # ══════════════════════════════════════════════════════
    # Batch Operations
    # ══════════════════════════════════════════════════════

    create_nodes = function(nodes_list) {
      "Create multiple nodes from a list. Each element should have:
       labels (character vector), properties (list), embedding (optional numeric vector).
       Returns vector of node IDs."
      vapply(nodes_list, function(node) {
        create_node(
          labels     = node$labels,
          properties = node$properties,
          embedding  = node$embedding
        )
      }, integer(1))
    },

    create_edges = function(edges_list) {
      "Create multiple edges from a list. Each element should have:
       source, target, edge_type, properties (optional), weight (optional),
       valid_from (optional), valid_to (optional).
       Returns vector of edge IDs."
      vapply(edges_list, function(edge) {
        create_edge(
          source     = edge$source,
          target     = edge$target,
          edge_type  = edge$edge_type,
          properties = if (!is.null(edge$properties)) edge$properties else list(),
          weight     = if (!is.null(edge$weight)) edge$weight else 1.0,
          valid_from = edge$valid_from,
          valid_to   = edge$valid_to
        )
      }, integer(1))
    },

    delete_nodes = function(node_ids) {
      "Delete multiple nodes. Returns number of successfully deleted nodes."
      count <- 0L
      for (nid in node_ids) {
        tryCatch({
          delete_node(nid)
          count <- count + 1L
        }, error = function(e) NULL)
      }
      count
    },

    delete_edges = function(edge_ids) {
      "Delete multiple edges. Returns number of successfully deleted edges."
      count <- 0L
      for (eid in edge_ids) {
        tryCatch({
          delete_edge(eid)
          count <- count + 1L
        }, error = function(e) NULL)
      }
      count
    },

    # ══════════════════════════════════════════════════════
    # Bulk Import from Data Frames
    # ══════════════════════════════════════════════════════

    import_nodes_df = function(df, label_col = "label", id_col = NULL,
                               embedding_cols = NULL) {
      "Import nodes from a data.frame.
       - label_col: column name containing node label(s)
       - id_col: optional column to use as external ID (stored in properties)
       - embedding_cols: optional vector of column names for embedding
       Returns vector of created node IDs."
      ids <- integer(nrow(df))
      for (i in seq_len(nrow(df))) {
        row <- df[i, , drop = FALSE]

        # Extract labels
        labels <- row[[label_col]]
        if (is.character(labels)) labels <- list(labels)

        # Extract embedding if specified
        embedding <- NULL
        if (!is.null(embedding_cols)) {
          embedding <- as.numeric(row[, embedding_cols, drop = TRUE])
        }

        # Build properties from remaining columns
        prop_cols <- setdiff(names(df), c(label_col, embedding_cols))
        properties <- as.list(row[, prop_cols, drop = FALSE])

        ids[i] <- create_node(labels, properties, embedding)
      }
      ids
    },

    import_edges_df = function(df, source_col = "source", target_col = "target",
                               type_col = "type", weight_col = NULL,
                               valid_from_col = NULL, valid_to_col = NULL) {
      "Import edges from a data.frame.
       - source_col, target_col: columns with node IDs
       - type_col: column with edge type
       - weight_col: optional column with edge weights
       - valid_from_col, valid_to_col: optional columns with temporal bounds
       Returns vector of created edge IDs."
      ids <- integer(nrow(df))
      for (i in seq_len(nrow(df))) {
        row <- df[i, , drop = FALSE]

        # Extract core fields
        source    <- row[[source_col]]
        target    <- row[[target_col]]
        edge_type <- row[[type_col]]

        # Optional fields
        weight     <- if (!is.null(weight_col)) row[[weight_col]] else 1.0
        valid_from <- if (!is.null(valid_from_col)) row[[valid_from_col]] else NULL
        valid_to   <- if (!is.null(valid_to_col)) row[[valid_to_col]] else NULL

        # Build properties from remaining columns
        exclude <- c(source_col, target_col, type_col, weight_col,
                     valid_from_col, valid_to_col)
        prop_cols <- setdiff(names(df), exclude)
        properties <- as.list(row[, prop_cols, drop = FALSE])

        ids[i] <- create_edge(source, target, edge_type, properties,
                              weight, valid_from, valid_to)
      }
      ids
    },

    # ══════════════════════════════════════════════════════
    # Export to Data Frames
    # ══════════════════════════════════════════════════════

    export_nodes_df = function(node_ids) {
      "Export nodes to a data.frame with id, labels, and flattened properties."
      if (length(node_ids) == 0) return(data.frame())
      rows <- lapply(node_ids, function(nid) {
        node <- get_node(nid)
        props <- if (length(node$properties) > 0) {
          as.data.frame(node$properties, stringsAsFactors = FALSE)
        } else {
          data.frame()
        }
        cbind(
          data.frame(
            node_id = nid,
            labels = paste(unlist(node$labels), collapse = ","),
            stringsAsFactors = FALSE
          ),
          props
        )
      })
      # Bind rows, handling differing columns
      all_cols <- unique(unlist(lapply(rows, names)))
      rows <- lapply(rows, function(r) {
        missing <- setdiff(all_cols, names(r))
        for (col in missing) r[[col]] <- NA
        r[, all_cols, drop = FALSE]
      })
      do.call(rbind, rows)
    },

    export_bfs_df = function(start, max_depth = 3L) {
      "Run BFS and return results as a data.frame with node details."
      bfs_result <- bfs(start, max_depth)
      if (length(bfs_result) == 0) return(data.frame())
      rows <- lapply(bfs_result, function(entry) {
        node <- get_node(entry$node_id)
        props <- if (length(node$properties) > 0) {
          as.data.frame(node$properties, stringsAsFactors = FALSE)
        } else {
          data.frame()
        }
        cbind(
          data.frame(
            node_id = entry$node_id,
            depth = entry$depth,
            labels = paste(unlist(node$labels), collapse = ","),
            stringsAsFactors = FALSE
          ),
          props
        )
      })
      all_cols <- unique(unlist(lapply(rows, names)))
      rows <- lapply(rows, function(r) {
        missing <- setdiff(all_cols, names(r))
        for (col in missing) r[[col]] <- NA
        r[, all_cols, drop = FALSE]
      })
      do.call(rbind, rows)
    }
  )
)


# ══════════════════════════════════════════════════════════════
# Arrow Flight Client (Optional - requires 'arrow' package)
# ══════════════════════════════════════════════════════════════

# Check if arrow package is available
.arrow_available <- function() {
  requireNamespace("arrow", quietly = TRUE)
}

#' Create an Arrow Flight client for high-performance queries
#'
#' @param uri Flight server URI (e.g., "grpc://localhost:7689")
#' @return An ArrowClient reference class instance
#' @export
ArrowClient <- setRefClass("ArrowClient",

  fields = list(
    uri    = "character",
    client = "ANY"
  ),

  methods = list(
    initialize = function(uri = "grpc://localhost:7689") {
      if (!.arrow_available()) {
        stop("Arrow package not installed. Install with: install.packages('arrow')")
      }
      uri    <<- uri
      client <<- NULL
    },

    connect = function() {
      "Connect to the Arrow Flight server."
      client <<- arrow::flight_connect(uri)
    },

    close = function() {
      "Close the Arrow Flight connection."
      if (!is.null(client)) {
        # Arrow Flight connections are managed automatically
        client <<- NULL
      }
    },

    query = function(gql) {
      "Execute a GQL query and return an Arrow Table."
      if (is.null(client)) stop("Not connected. Call $connect() first.")
      # Create a flight descriptor with the query
      descriptor <- arrow::flight_descriptor_for_command(gql)
      # Get the flight info and fetch data
      info <- client$get_flight_info(descriptor)
      reader <- client$do_get(info$endpoints[[1]]$ticket)
      reader$read_all()
    },

    query_df = function(gql) {
      "Execute a GQL query and return as a data.frame."
      tbl <- query(gql)
      as.data.frame(tbl)
    },

    query_batches = function(gql, callback) {
      "Execute a GQL query and process record batches with a callback function.
       callback receives each Arrow RecordBatch."
      if (is.null(client)) stop("Not connected. Call $connect() first.")
      descriptor <- arrow::flight_descriptor_for_command(gql)
      info <- client$get_flight_info(descriptor)
      reader <- client$do_get(info$endpoints[[1]]$ticket)
      while (TRUE) {
        batch <- reader$read_next_batch()
        if (is.null(batch)) break
        callback(batch)
      }
    },

    list_flights = function() {
      "List available flights on the server."
      if (is.null(client)) stop("Not connected. Call $connect() first.")
      client$list_flights()
    }
  )
)


#' Create a unified client that uses Arrow when available
#'
#' @param host Server host for JSON/TCP
#' @param port Server port for JSON/TCP
#' @param flight_uri Arrow Flight URI (optional)
#' @param auth_token Authentication token (optional)
#' @return A UnifiedClient that auto-selects best transport
#' @export
UnifiedClient <- setRefClass("UnifiedClient",

  fields = list(
    json_client  = "ANY",
    arrow_client = "ANY",
    use_arrow    = "logical"
  ),

  methods = list(
    initialize = function(host = "127.0.0.1", port = 7687L,
                          flight_uri = NULL, auth_token = NULL) {
      # Always create JSON client
      json_client <<- AstraeaClient$new(host, port, auth_token)

      # Try to create Arrow client if available
      arrow_client <<- NULL
      use_arrow    <<- FALSE

      if (.arrow_available()) {
        flight_uri <- flight_uri %||% sprintf("grpc://%s:7689", host)
        tryCatch({
          arrow_client <<- ArrowClient$new(flight_uri)
          use_arrow    <<- TRUE
        }, error = function(e) {
          message("Arrow Flight not available, using JSON/TCP")
        })
      }
    },

    connect = function() {
      "Connect to the server(s)."
      json_client$connect()
      if (use_arrow && !is.null(arrow_client)) {
        tryCatch({
          arrow_client$connect()
        }, error = function(e) {
          use_arrow <<- FALSE
          message("Arrow Flight connection failed, using JSON/TCP only")
        })
      }
    },

    close = function() {
      "Close all connections."
      json_client$close()
      if (!is.null(arrow_client)) {
        arrow_client$close()
      }
    },

    # Delegate to JSON client for CRUD operations
    ping = function() json_client$ping(),
    create_node = function(...) json_client$create_node(...),
    get_node = function(...) json_client$get_node(...),
    update_node = function(...) json_client$update_node(...),
    delete_node = function(...) json_client$delete_node(...),
    create_edge = function(...) json_client$create_edge(...),
    get_edge = function(...) json_client$get_edge(...),
    update_edge = function(...) json_client$update_edge(...),
    delete_edge = function(...) json_client$delete_edge(...),
    neighbors = function(...) json_client$neighbors(...),
    bfs = function(...) json_client$bfs(...),
    shortest_path = function(...) json_client$shortest_path(...),
    neighbors_at = function(...) json_client$neighbors_at(...),
    bfs_at = function(...) json_client$bfs_at(...),
    shortest_path_at = function(...) json_client$shortest_path_at(...),
    vector_search = function(...) json_client$vector_search(...),
    hybrid_search = function(...) json_client$hybrid_search(...),
    semantic_neighbors = function(...) json_client$semantic_neighbors(...),
    semantic_walk = function(...) json_client$semantic_walk(...),
    extract_subgraph = function(...) json_client$extract_subgraph(...),
    graph_rag = function(...) json_client$graph_rag(...),
    create_nodes = function(...) json_client$create_nodes(...),
    create_edges = function(...) json_client$create_edges(...),
    delete_nodes = function(...) json_client$delete_nodes(...),
    delete_edges = function(...) json_client$delete_edges(...),
    import_nodes_df = function(...) json_client$import_nodes_df(...),
    import_edges_df = function(...) json_client$import_edges_df(...),
    export_nodes_df = function(...) json_client$export_nodes_df(...),
    export_bfs_df = function(...) json_client$export_bfs_df(...),

    # Query uses Arrow if available, falls back to JSON
    query = function(gql) {
      if (use_arrow && !is.null(arrow_client)) {
        arrow_client$query(gql)
      } else {
        json_client$query(gql)
      }
    },

    query_df = function(gql) {
      if (use_arrow && !is.null(arrow_client)) {
        arrow_client$query_df(gql)
      } else {
        result <- json_client$query(gql)
        if (!is.null(result$rows)) {
          do.call(rbind, lapply(result$rows, as.data.frame))
        } else {
          data.frame()
        }
      }
    },

    is_arrow_enabled = function() {
      "Check if Arrow Flight is being used."
      use_arrow && !is.null(arrow_client)
    }
  )
)

# Null coalesce operator (if not already defined)
`%||%` <- function(a, b) if (is.null(a)) b else a


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
