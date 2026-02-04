#!/usr/bin/env python3
"""
AstraeaDB Cybersecurity Use Case Demo

Demonstrates how AstraeaDB enables security analysts to investigate network
alerts by tracing connections through a graph:

    FirewallAlert -> IP Address -> (DHCP Lease) -> Laptop -> User

Scenario: Eve downloads a password cracker from a malicious website, then
attempts lateral movement to Bob's and Alice's machines via RDP and SSH.

Usage:
    # Start the server first:
    #   cargo run -p astraea-cli -- serve

    # Then run this demo:
    #   python3 examples/cybersecurity_demo.py
"""

import json
import socket
import sys
from datetime import datetime, timedelta, timezone
from typing import Any, Optional


# ---------------------------------------------------------------------------
# Client (reuses the same protocol as python_client.py)
# ---------------------------------------------------------------------------

class AstraeaClient:
    """Minimal AstraeaDB client for the cybersecurity demo."""

    def __init__(self, host: str = "127.0.0.1", port: int = 7687):
        self.host = host
        self.port = port
        self._sock: Optional[socket.socket] = None

    def connect(self):
        self._sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self._sock.connect((self.host, self.port))
        self._sock.settimeout(5.0)

    def close(self):
        if self._sock:
            self._sock.close()
            self._sock = None

    def __enter__(self):
        self.connect()
        return self

    def __exit__(self, *args):
        self.close()

    def _send(self, request: dict) -> dict:
        if not self._sock:
            raise ConnectionError("Not connected")
        line = json.dumps(request) + "\n"
        self._sock.sendall(line.encode("utf-8"))
        buf = b""
        while b"\n" not in buf:
            chunk = self._sock.recv(4096)
            if not chunk:
                raise ConnectionError("Server closed connection")
            buf += chunk
        return json.loads(buf.split(b"\n", 1)[0])

    def _check(self, response: dict) -> Any:
        if response.get("status") == "error":
            raise RuntimeError(f"AstraeaDB error: {response.get('message')}")
        return response.get("data")

    def create_node(self, labels: list[str], properties: dict) -> int:
        return self._check(self._send({
            "type": "CreateNode",
            "labels": labels,
            "properties": properties,
        }))["node_id"]

    def create_edge(
        self,
        source: int,
        target: int,
        edge_type: str,
        properties: Optional[dict] = None,
        weight: float = 1.0,
        valid_from: Optional[int] = None,
        valid_to: Optional[int] = None,
    ) -> int:
        req = {
            "type": "CreateEdge",
            "source": source,
            "target": target,
            "edge_type": edge_type,
            "properties": properties or {},
            "weight": weight,
        }
        if valid_from is not None:
            req["valid_from"] = valid_from
        if valid_to is not None:
            req["valid_to"] = valid_to
        return self._check(self._send(req))["edge_id"]

    def get_node(self, node_id: int) -> dict:
        return self._check(self._send({"type": "GetNode", "id": node_id}))

    def get_edge(self, edge_id: int) -> dict:
        return self._check(self._send({"type": "GetEdge", "id": edge_id}))

    def neighbors(
        self,
        node_id: int,
        direction: str = "outgoing",
        edge_type: Optional[str] = None,
    ) -> list[dict]:
        req: dict = {"type": "Neighbors", "id": node_id, "direction": direction}
        if edge_type is not None:
            req["edge_type"] = edge_type
        return self._check(self._send(req))["neighbors"]

    def bfs(self, start: int, max_depth: int = 3) -> list[dict]:
        return self._check(
            self._send({"type": "Bfs", "start": start, "max_depth": max_depth})
        )["nodes"]

    def shortest_path(self, from_node: int, to_node: int) -> Optional[dict]:
        return self._check(self._send({
            "type": "ShortestPath",
            "from": from_node,
            "to": to_node,
        }))

    def ping(self) -> dict:
        return self._check(self._send({"type": "Ping"}))


# ---------------------------------------------------------------------------
# Helper: epoch millis
# ---------------------------------------------------------------------------

def epoch_ms(dt: datetime) -> int:
    return int(dt.timestamp() * 1000)


# ---------------------------------------------------------------------------
# Main demo
# ---------------------------------------------------------------------------

def run_demo(client: AstraeaClient):
    print("=" * 70)
    print("  AstraeaDB Cybersecurity Demo: Threat Investigation")
    print("=" * 70)

    # -- Timestamps for the scenario --
    base = datetime(2025, 1, 15, 8, 0, 0, tzinfo=timezone.utc)
    lease_start = epoch_ms(base)
    lease_end = epoch_ms(base + timedelta(hours=2))
    attack_time_1 = epoch_ms(base + timedelta(minutes=30))
    attack_time_2 = epoch_ms(base + timedelta(minutes=45))
    attack_time_3 = epoch_ms(base + timedelta(minutes=50))

    # =====================================================================
    # PHASE 1: Load the datasets
    # =====================================================================
    print("\n--- Phase 1: Loading datasets into AstraeaDB ---\n")

    # -- 1a. Asset Management: Users and Laptops --
    print("1a. Asset management (users + laptops)...")
    alice = client.create_node(["User"], {
        "name": "Alice", "department": "Engineering", "role": "Senior Developer",
    })
    bob = client.create_node(["User"], {
        "name": "Bob", "department": "Finance", "role": "Accountant",
    })
    eve = client.create_node(["User"], {
        "name": "Eve", "department": "Marketing", "role": "Analyst",
    })

    laptop_alice = client.create_node(["Laptop"], {
        "brand": "Apple", "model": "MacBook Pro 16",
        "serial_number": "SN-APP-1001", "hostname": "ALICE-MBP01",
    })
    laptop_bob = client.create_node(["Laptop"], {
        "brand": "Lenovo", "model": "ThinkPad X1 Carbon",
        "serial_number": "SN-LEN-2001", "hostname": "BOB-TP01",
    })
    laptop_eve = client.create_node(["Laptop"], {
        "brand": "Dell", "model": "Latitude 5540",
        "serial_number": "SN-DEL-3001", "hostname": "EVE-LAT01",
    })

    # Laptop -> User assignments
    client.create_edge(laptop_alice, alice, "ASSIGNED_TO", {
        "assigned_date": "2024-03-15",
    })
    client.create_edge(laptop_bob, bob, "ASSIGNED_TO", {
        "assigned_date": "2024-06-01",
    })
    client.create_edge(laptop_eve, eve, "ASSIGNED_TO", {
        "assigned_date": "2024-09-10",
    })
    print(f"   Users: Alice(id={alice}), Bob(id={bob}), Eve(id={eve})")
    print(f"   Laptops: ALICE-MBP01(id={laptop_alice}), BOB-TP01(id={laptop_bob}), EVE-LAT01(id={laptop_eve})")

    # -- 1b. IP Addresses --
    print("\n1b. IP addresses...")
    ip_alice = client.create_node(["IPAddress"], {
        "address": "10.0.1.10", "network": "internal",
    })
    ip_bob = client.create_node(["IPAddress"], {
        "address": "10.0.1.20", "network": "internal",
    })
    ip_eve = client.create_node(["IPAddress"], {
        "address": "10.0.1.50", "network": "internal",
    })
    print(f"   10.0.1.10(id={ip_alice}), 10.0.1.20(id={ip_bob}), 10.0.1.50(id={ip_eve})")

    # -- 1c. DHCP Leases (temporal edges: IP -> Laptop) --
    print("\n1c. DHCP leases (temporal edges)...")
    dhcp_alice = client.create_edge(ip_alice, laptop_alice, "DHCP_LEASE", {
        "dhcp_server": "10.0.0.1",
    }, valid_from=lease_start, valid_to=lease_end)
    dhcp_bob = client.create_edge(ip_bob, laptop_bob, "DHCP_LEASE", {
        "dhcp_server": "10.0.0.1",
    }, valid_from=lease_start, valid_to=lease_end)
    dhcp_eve = client.create_edge(ip_eve, laptop_eve, "DHCP_LEASE", {
        "dhcp_server": "10.0.0.1",
    }, valid_from=lease_start, valid_to=lease_end)
    print(f"   10.0.1.10 -> ALICE-MBP01  [08:00-10:00 UTC]")
    print(f"   10.0.1.20 -> BOB-TP01     [08:00-10:00 UTC]")
    print(f"   10.0.1.50 -> EVE-LAT01    [08:00-10:00 UTC]")

    # -- 1d. External hosts --
    print("\n1d. External hosts...")
    darktools = client.create_node(["ExternalHost"], {
        "hostname": "darktools.example.com",
        "ip_address": "198.51.100.66",
        "category": "malware_distribution",
        "risk_level": "critical",
    })
    legit_web = client.create_node(["ExternalHost"], {
        "hostname": "docs.example.com",
        "ip_address": "203.0.113.10",
        "category": "documentation",
        "risk_level": "none",
    })
    print(f"   darktools.example.com(id={darktools}), docs.example.com(id={legit_web})")

    # -- 1e. Network traffic --
    print("\n1e. Network traffic logs...")

    # Legitimate: Alice visits docs
    client.create_edge(ip_alice, legit_web, "TRAFFIC", {
        "timestamp": attack_time_1, "dest_port": 443,
        "protocol": "TCP", "bytes_sent": 15200,
        "description": "HTTPS to documentation site",
    })

    # Eve's malicious traffic
    client.create_edge(ip_eve, darktools, "TRAFFIC", {
        "timestamp": attack_time_1, "dest_port": 443,
        "protocol": "TCP", "bytes_sent": 524288,
        "description": "Downloaded password_cracker.zip",
    })
    client.create_edge(ip_eve, ip_bob, "TRAFFIC", {
        "timestamp": attack_time_2, "dest_port": 3389,
        "protocol": "TCP", "bytes_sent": 4096,
        "description": "RDP connection attempt",
    })
    client.create_edge(ip_eve, ip_alice, "TRAFFIC", {
        "timestamp": attack_time_3, "dest_port": 22,
        "protocol": "TCP", "bytes_sent": 2048,
        "description": "SSH connection attempt",
    })
    print("   10.0.1.10 -> docs.example.com:443 (legitimate)")
    print("   10.0.1.50 -> darktools.example.com:443 (malware download)")
    print("   10.0.1.50 -> 10.0.1.20:3389 (RDP attempt)")
    print("   10.0.1.50 -> 10.0.1.10:22 (SSH attempt)")

    # -- 1f. Firewall alerts --
    print("\n1f. Firewall alerts...")
    alert_malware = client.create_node(["FirewallAlert"], {
        "alert_id": "FW-2025-0042",
        "rule": "MALWARE_DOWNLOAD",
        "severity": "critical",
        "timestamp": attack_time_1,
        "action": "logged",
        "description": "Connection to known malware distribution site",
    })
    alert_rdp = client.create_node(["FirewallAlert"], {
        "alert_id": "FW-2025-0043",
        "rule": "LATERAL_MOVEMENT_RDP",
        "severity": "high",
        "timestamp": attack_time_2,
        "action": "blocked",
        "description": "Unauthorized RDP attempt to internal host",
    })
    alert_ssh = client.create_node(["FirewallAlert"], {
        "alert_id": "FW-2025-0044",
        "rule": "UNAUTHORIZED_SSH",
        "severity": "high",
        "timestamp": attack_time_3,
        "action": "blocked",
        "description": "Unauthorized SSH attempt to internal host",
    })

    # Alert triggered by source IP
    client.create_edge(ip_eve, alert_malware, "TRIGGERED", {
        "timestamp": attack_time_1,
    })
    client.create_edge(ip_eve, alert_rdp, "TRIGGERED", {
        "timestamp": attack_time_2,
    })
    client.create_edge(ip_eve, alert_ssh, "TRIGGERED", {
        "timestamp": attack_time_3,
    })

    # Alert targets (what was the destination)
    client.create_edge(alert_malware, darktools, "TARGETS", {})
    client.create_edge(alert_rdp, ip_bob, "TARGETS", {})
    client.create_edge(alert_ssh, ip_alice, "TARGETS", {})

    print(f"   FW-2025-0042: MALWARE_DOWNLOAD (critical)")
    print(f"   FW-2025-0043: LATERAL_MOVEMENT_RDP (high)")
    print(f"   FW-2025-0044: UNAUTHORIZED_SSH (high)")
    print(f"\n   Graph loaded: {6} users/laptops, {3} IPs, {2} external hosts, {3} alerts")

    # =====================================================================
    # PHASE 2: Analyst Investigation
    # =====================================================================
    print("\n" + "=" * 70)
    print("  Phase 2: Analyst Investigation")
    print("=" * 70)

    # -- Step 1: Start from the malware alert --
    print("\n[Step 1] Analyst sees alert FW-2025-0042 (MALWARE_DOWNLOAD)")
    alert_node = client.get_node(alert_malware)
    print(f"   Alert: {alert_node['properties']['description']}")
    print(f"   Severity: {alert_node['properties']['severity']}")
    print(f"   Action: {alert_node['properties']['action']}")

    # -- Step 2: Who triggered this alert? (incoming TRIGGERED edges) --
    print("\n[Step 2] Who triggered this alert? (follow TRIGGERED edges)")
    triggerers = client.neighbors(alert_malware, "incoming", edge_type="TRIGGERED")
    for t in triggerers:
        src_node = client.get_node(t["node_id"])
        src_ip = src_node["properties"]["address"]
        print(f"   Source IP: {src_ip} (node_id={t['node_id']})")

    # -- Step 3: Trace IP -> Laptop via DHCP lease --
    source_ip_id = triggerers[0]["node_id"]
    print(f"\n[Step 3] Trace {client.get_node(source_ip_id)['properties']['address']} -> Laptop via DHCP_LEASE")
    dhcp_leases = client.neighbors(source_ip_id, "outgoing", edge_type="DHCP_LEASE")
    for lease in dhcp_leases:
        edge_data = client.get_edge(lease["edge_id"])
        laptop_node = client.get_node(lease["node_id"])
        hostname = laptop_node["properties"]["hostname"]
        # Show temporal validity
        vf = edge_data.get("valid_from")
        vt = edge_data.get("valid_to")
        if vf and vt:
            dt_from = datetime.fromtimestamp(vf / 1000, tz=timezone.utc).strftime("%H:%M")
            dt_to = datetime.fromtimestamp(vt / 1000, tz=timezone.utc).strftime("%H:%M")
            print(f"   Laptop: {hostname} (node_id={lease['node_id']})")
            print(f"   DHCP lease valid: {dt_from} - {dt_to} UTC")
        else:
            print(f"   Laptop: {hostname} (node_id={lease['node_id']})")

    # -- Step 4: Trace Laptop -> User via ASSIGNED_TO --
    laptop_id = dhcp_leases[0]["node_id"]
    print(f"\n[Step 4] Trace {client.get_node(laptop_id)['properties']['hostname']} -> User via ASSIGNED_TO")
    assignments = client.neighbors(laptop_id, "outgoing", edge_type="ASSIGNED_TO")
    for a in assignments:
        user_node = client.get_node(a["node_id"])
        name = user_node["properties"]["name"]
        dept = user_node["properties"]["department"]
        role = user_node["properties"]["role"]
        print(f"   >>> IDENTIFIED USER: {name}")
        print(f"       Department: {dept}")
        print(f"       Role: {role}")

    # -- Step 5: Pivot - what else has this IP been doing? --
    print(f"\n[Step 5] Pivot: What else has 10.0.1.50 been doing?")
    all_traffic = client.neighbors(source_ip_id, "outgoing", edge_type="TRAFFIC")
    print(f"   Found {len(all_traffic)} outbound traffic connections:")
    for t in all_traffic:
        edge_data = client.get_edge(t["edge_id"])
        dest_node = client.get_node(t["node_id"])
        props = edge_data["properties"]
        dest_name = dest_node["properties"].get("hostname") or dest_node["properties"].get("address")
        port = props.get("dest_port", "?")
        desc = props.get("description", "")
        print(f"   -> {dest_name}:{port} - {desc}")

    # -- Step 6: Who were the targets of the lateral movement? --
    print(f"\n[Step 6] Identify targets of lateral movement attempts")
    all_alerts = client.neighbors(source_ip_id, "outgoing", edge_type="TRIGGERED")
    for alert_ref in all_alerts:
        alert_data = client.get_node(alert_ref["node_id"])
        rule = alert_data["properties"]["rule"]
        severity = alert_data["properties"]["severity"]

        # Follow TARGETS edge from alert to destination
        targets = client.neighbors(alert_ref["node_id"], "outgoing", edge_type="TARGETS")
        for target_ref in targets:
            target_data = client.get_node(target_ref["node_id"])
            target_name = target_data["properties"].get("hostname") or target_data["properties"].get("address")

            # If target is an internal IP, trace it back to a user
            if "IPAddress" in target_data.get("labels", []):
                dhcp = client.neighbors(target_ref["node_id"], "outgoing", edge_type="DHCP_LEASE")
                if dhcp:
                    laptop = client.get_node(dhcp[0]["node_id"])
                    assigns = client.neighbors(dhcp[0]["node_id"], "outgoing", edge_type="ASSIGNED_TO")
                    if assigns:
                        victim = client.get_node(assigns[0]["node_id"])
                        target_name = f"{target_data['properties']['address']} -> {laptop['properties']['hostname']} -> {victim['properties']['name']}"

            print(f"   Alert {rule} ({severity}): target = {target_name}")

    # -- Step 7: BFS to see full blast radius --
    print(f"\n[Step 7] BFS from Eve's IP (depth=2) - attack blast radius")
    bfs_results = client.bfs(source_ip_id, max_depth=2)
    for entry in bfs_results:
        node = client.get_node(entry["node_id"])
        labels = ", ".join(node["labels"])
        name = (
            node["properties"].get("name")
            or node["properties"].get("hostname")
            or node["properties"].get("address")
            or node["properties"].get("alert_id")
            or "unknown"
        )
        print(f"   Depth {entry['depth']}: [{labels}] {name}")

    # -- Step 8: Shortest path between Eve and Bob --
    print(f"\n[Step 8] Shortest path: Eve's IP -> Bob's IP")
    path_result = client.shortest_path(ip_eve, ip_bob)
    if path_result and path_result.get("path"):
        names = []
        for nid in path_result["path"]:
            n = client.get_node(nid)
            name = (
                n["properties"].get("address")
                or n["properties"].get("hostname")
                or n["properties"].get("name")
            )
            names.append(f"{name}")
        print(f"   Path ({path_result['length']} hop{'s' if path_result['length'] != 1 else ''}): {' -> '.join(names)}")
    else:
        print("   No direct path found")

    # =====================================================================
    # Summary
    # =====================================================================
    print("\n" + "=" * 70)
    print("  Investigation Summary")
    print("=" * 70)
    print("""
  Alert:   FW-2025-0042 (MALWARE_DOWNLOAD, critical)
  Source:  10.0.1.50
  Laptop:  EVE-LAT01 (Dell Latitude 5540, SN-DEL-3001)
  User:    Eve (Marketing, Analyst)

  Activity from 10.0.1.50:
    1. Downloaded password cracker from darktools.example.com
    2. Attempted RDP to Bob's machine (10.0.1.20, BOB-TP01) - BLOCKED
    3. Attempted SSH to Alice's machine (10.0.1.10, ALICE-MBP01) - BLOCKED

  Recommendation: Isolate EVE-LAT01, revoke Eve's credentials,
  initiate incident response procedure.
""")
    print("=" * 70)
    print("  Demo complete.")
    print("=" * 70)


def main():
    import argparse

    parser = argparse.ArgumentParser(
        description="AstraeaDB Cybersecurity Use Case Demo"
    )
    parser.add_argument("--host", default="127.0.0.1", help="Server host")
    parser.add_argument("--port", type=int, default=7687, help="Server port")
    args = parser.parse_args()

    try:
        with AstraeaClient(args.host, args.port) as client:
            client.ping()
            run_demo(client)
    except ConnectionRefusedError:
        print(
            f"Could not connect to AstraeaDB at {args.host}:{args.port}",
            file=sys.stderr,
        )
        print(
            "Start the server first: cargo run -p astraea-cli -- serve",
            file=sys.stderr,
        )
        sys.exit(1)
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
