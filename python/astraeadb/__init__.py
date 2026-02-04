"""AstraeaDB Python Client.

Provides both JSON/TCP and Arrow Flight clients for AstraeaDB.

Usage:
    from astraeadb import AstraeaClient

    with AstraeaClient() as client:
        node_id = client.create_node(["Person"], {"name": "Alice"})
        results = client.query("MATCH (n:Person) RETURN n.name")
"""

from astraeadb.client import AstraeaClient
from astraeadb.json_client import JsonClient

__all__ = ["AstraeaClient", "JsonClient"]
__version__ = "0.1.0"

try:
    from astraeadb.arrow_client import ArrowClient
    __all__.append("ArrowClient")
except ImportError:
    pass  # pyarrow not installed
