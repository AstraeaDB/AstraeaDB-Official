"""Arrow Flight client for AstraeaDB.

Requires pyarrow: pip install pyarrow
"""

import json
from typing import Optional

try:
    import pyarrow as pa
    import pyarrow.flight as flight
except ImportError:
    raise ImportError(
        "pyarrow is required for ArrowClient. Install with: pip install 'astraeadb[arrow]'"
    )


class ArrowClient:
    """AstraeaDB client using Apache Arrow Flight.

    Provides high-throughput, zero-copy data transfer for query results
    and bulk data import.

    Usage:
        client = ArrowClient("grpc://localhost:50051")
        table = client.query("MATCH (n:Person) RETURN n.name, n.age")
        df = table.to_pandas()
    """

    def __init__(self, uri: str = "grpc://localhost:50051"):
        """Create a new Arrow Flight client.

        Args:
            uri: Flight server URI (e.g., "grpc://localhost:50051")
        """
        self.uri = uri
        self._client: Optional[flight.FlightClient] = None

    def connect(self) -> None:
        """Establish connection to the Flight server."""
        self._client = flight.connect(self.uri)

    def close(self) -> None:
        """Close the connection."""
        if self._client:
            self._client.close()
            self._client = None

    def __enter__(self):
        self.connect()
        return self

    def __exit__(self, *args):
        self.close()

    def _ensure_connected(self) -> flight.FlightClient:
        if self._client is None:
            raise ConnectionError("not connected; call connect() or use context manager")
        return self._client

    def query(self, gql: str) -> pa.Table:
        """Execute a GQL query and return results as an Arrow Table.

        The result can be converted to pandas with .to_pandas() or
        used directly with Polars.

        Args:
            gql: GQL query string

        Returns:
            Arrow Table with query results
        """
        client = self._ensure_connected()
        ticket = flight.Ticket(gql.encode("utf-8"))
        reader = client.do_get(ticket)
        return reader.read_all()

    def query_batches(self, gql: str):
        """Execute a GQL query and return a RecordBatch reader.

        Useful for streaming large result sets without loading
        everything into memory.

        Args:
            gql: GQL query string

        Returns:
            FlightStreamReader for iterating over RecordBatches
        """
        client = self._ensure_connected()
        ticket = flight.Ticket(gql.encode("utf-8"))
        return client.do_get(ticket)

    def bulk_insert_nodes(
        self,
        table: pa.Table,
    ) -> dict:
        """Bulk insert nodes from an Arrow Table.

        Expected schema:
            - labels: string (JSON array of labels)
            - properties: string (JSON object)
            - has_embedding: bool (optional)

        Args:
            table: Arrow Table with node data

        Returns:
            dict with import statistics (nodes_created, edges_created)
        """
        client = self._ensure_connected()

        # Create a FlightDescriptor for the upload
        descriptor = flight.FlightDescriptor.for_command(b"import_nodes")

        # Start the upload
        writer, reader = client.do_put(descriptor, table.schema)
        writer.write_table(table)
        writer.close()

        # Read the result metadata
        result = reader.read()
        metadata = json.loads(result.to_pybytes())
        return metadata

    def bulk_insert_edges(
        self,
        table: pa.Table,
    ) -> dict:
        """Bulk insert edges from an Arrow Table.

        Expected schema:
            - source: uint64
            - target: uint64
            - edge_type: string
            - properties: string (JSON object)
            - weight: float64
            - valid_from: int64 (nullable, epoch ms)
            - valid_to: int64 (nullable, epoch ms)

        Args:
            table: Arrow Table with edge data

        Returns:
            dict with import statistics (nodes_created, edges_created)
        """
        client = self._ensure_connected()
        descriptor = flight.FlightDescriptor.for_command(b"import_edges")
        writer, reader = client.do_put(descriptor, table.schema)
        writer.write_table(table)
        writer.close()
        result = reader.read()
        metadata = json.loads(result.to_pybytes())
        return metadata

    def query_to_pandas(self, gql: str):
        """Execute a GQL query and return a pandas DataFrame.

        Convenience method that combines query() with .to_pandas().

        Args:
            gql: GQL query string

        Returns:
            pandas DataFrame with query results
        """
        table = self.query(gql)
        return table.to_pandas()
