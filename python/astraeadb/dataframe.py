"""Optional DataFrame support for AstraeaDB.

Requires pandas: pip install pandas

Usage:
    from astraeadb import AstraeaClient
    from astraeadb.dataframe import import_nodes_df, export_nodes_df
    import pandas as pd

    df = pd.DataFrame([
        {"label": "Person", "name": "Alice", "age": 30},
        {"label": "Person", "name": "Bob", "age": 25},
    ])

    with AstraeaClient() as client:
        node_ids = import_nodes_df(client, df, label_col="label")
        result_df = export_nodes_df(client, node_ids)
"""

from __future__ import annotations

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    import pandas as pd


def _check_pandas():
    """Check that pandas is available."""
    try:
        import pandas
        return pandas
    except ImportError:
        raise ImportError(
            "pandas is required for DataFrame support. Install with: pip install pandas"
        )


def import_nodes_df(
    client,
    df: "pd.DataFrame",
    label_col: str = "label",
    embedding_cols: list[str] | None = None,
) -> list[int]:
    """Import nodes from a pandas DataFrame.

    Args:
        client: AstraeaDB client instance
        df: DataFrame with node data
        label_col: Column containing node label(s). Can be a string or list of strings.
        embedding_cols: Optional list of columns to use as embedding vector

    Returns:
        List of created node IDs

    Example:
        df = pd.DataFrame([
            {"label": "Person", "name": "Alice", "age": 30},
            {"label": "Person", "name": "Bob", "age": 25},
        ])
        node_ids = import_nodes_df(client, df, label_col="label")
    """
    _check_pandas()
    ids = []

    for _, row in df.iterrows():
        labels = row[label_col]
        if isinstance(labels, str):
            labels = [labels]

        # Extract embedding if specified
        embedding = None
        if embedding_cols:
            embedding = [float(row[col]) for col in embedding_cols]

        # Build properties from remaining columns
        exclude = {label_col} | set(embedding_cols or [])
        properties = {k: v for k, v in row.items() if k not in exclude}

        # Convert numpy types to Python native types
        properties = {k: _to_python(v) for k, v in properties.items()}

        node_id = client.create_node(list(labels), properties, embedding)
        ids.append(node_id)

    return ids


def import_edges_df(
    client,
    df: "pd.DataFrame",
    source_col: str = "source",
    target_col: str = "target",
    type_col: str = "type",
    weight_col: str | None = None,
    valid_from_col: str | None = None,
    valid_to_col: str | None = None,
) -> list[int]:
    """Import edges from a pandas DataFrame.

    Args:
        client: AstraeaDB client instance
        df: DataFrame with edge data
        source_col: Column with source node IDs
        target_col: Column with target node IDs
        type_col: Column with edge types
        weight_col: Optional column with edge weights
        valid_from_col: Optional column with validity start timestamps
        valid_to_col: Optional column with validity end timestamps

    Returns:
        List of created edge IDs

    Example:
        df = pd.DataFrame([
            {"source": 0, "target": 1, "type": "KNOWS", "since": 2020},
            {"source": 1, "target": 2, "type": "KNOWS", "since": 2021},
        ])
        edge_ids = import_edges_df(client, df)
    """
    pd = _check_pandas()
    ids = []

    exclude = {source_col, target_col, type_col, weight_col, valid_from_col, valid_to_col}
    exclude = {c for c in exclude if c is not None}

    for _, row in df.iterrows():
        properties = {k: _to_python(v) for k, v in row.items() if k not in exclude}

        edge_id = client.create_edge(
            source=int(row[source_col]),
            target=int(row[target_col]),
            edge_type=row[type_col],
            properties=properties if properties else None,
            weight=float(row[weight_col]) if weight_col else 1.0,
            valid_from=int(row[valid_from_col]) if valid_from_col and pd.notna(row[valid_from_col]) else None,
            valid_to=int(row[valid_to_col]) if valid_to_col and pd.notna(row[valid_to_col]) else None,
        )
        ids.append(edge_id)

    return ids


def export_nodes_df(client, node_ids: list[int]) -> "pd.DataFrame":
    """Export nodes to a pandas DataFrame.

    Args:
        client: AstraeaDB client instance
        node_ids: List of node IDs to export

    Returns:
        DataFrame with node_id, labels, and flattened properties

    Example:
        df = export_nodes_df(client, [0, 1, 2])
        print(df.columns)  # ['node_id', 'labels', 'name', 'age', ...]
    """
    pd = _check_pandas()
    rows = []

    for nid in node_ids:
        node = client.get_node(nid)
        row = {
            "node_id": nid,
            "labels": ",".join(node.get("labels", [])),
        }
        row.update(node.get("properties", {}))
        rows.append(row)

    return pd.DataFrame(rows)


def export_edges_df(client, edge_ids: list[int]) -> "pd.DataFrame":
    """Export edges to a pandas DataFrame.

    Args:
        client: AstraeaDB client instance
        edge_ids: List of edge IDs to export

    Returns:
        DataFrame with edge_id, source, target, type, weight, and flattened properties
    """
    pd = _check_pandas()
    rows = []

    for eid in edge_ids:
        edge = client.get_edge(eid)
        row = {
            "edge_id": eid,
            "source": edge.get("source"),
            "target": edge.get("target"),
            "edge_type": edge.get("edge_type"),
            "weight": edge.get("weight", 1.0),
        }
        row.update(edge.get("properties", {}))
        rows.append(row)

    return pd.DataFrame(rows)


def export_bfs_df(client, start: int, max_depth: int = 3) -> "pd.DataFrame":
    """Run BFS and return results as a DataFrame with node details.

    Args:
        client: AstraeaDB client instance
        start: Starting node ID
        max_depth: Maximum BFS depth

    Returns:
        DataFrame with node_id, depth, labels, and properties

    Example:
        df = export_bfs_df(client, start=0, max_depth=2)
        print(df.groupby("depth").count())  # Nodes per level
    """
    pd = _check_pandas()
    bfs_result = client.bfs(start, max_depth)
    rows = []

    for entry in bfs_result:
        node = client.get_node(entry["node_id"])
        row = {
            "node_id": entry["node_id"],
            "depth": entry["depth"],
            "labels": ",".join(node.get("labels", [])),
        }
        row.update(node.get("properties", {}))
        rows.append(row)

    return pd.DataFrame(rows)


def export_bfs_at_df(client, start: int, max_depth: int, timestamp: int) -> "pd.DataFrame":
    """Run temporal BFS and return results as a DataFrame.

    Args:
        client: AstraeaDB client instance
        start: Starting node ID
        max_depth: Maximum BFS depth
        timestamp: Unix timestamp in milliseconds

    Returns:
        DataFrame with node_id, depth, labels, and properties
    """
    pd = _check_pandas()
    bfs_result = client.bfs_at(start, max_depth, timestamp)
    rows = []

    for entry in bfs_result:
        node = client.get_node(entry["node_id"])
        row = {
            "node_id": entry["node_id"],
            "depth": entry["depth"],
            "labels": ",".join(node.get("labels", [])),
        }
        row.update(node.get("properties", {}))
        rows.append(row)

    return pd.DataFrame(rows)


def _to_python(val):
    """Convert numpy types to Python native types for JSON serialization."""
    import numpy as np
    if isinstance(val, (np.integer, np.int64, np.int32)):
        return int(val)
    elif isinstance(val, (np.floating, np.float64, np.float32)):
        return float(val)
    elif isinstance(val, np.ndarray):
        return val.tolist()
    elif isinstance(val, np.bool_):
        return bool(val)
    return val
