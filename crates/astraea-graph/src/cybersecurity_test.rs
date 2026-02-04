//! Integration test: Cybersecurity threat investigation scenario.
//!
//! Models a corporate network where an attacker (Eve) downloads a password
//! cracker and attempts lateral movement to other users' machines. Verifies
//! that graph traversals enable an analyst to trace from a firewall alert
//! back to the responsible user and discover all related activity.

#[cfg(test)]
mod tests {
    use astraea_core::traits::GraphOps;
    use astraea_core::types::*;

    use crate::graph::Graph;
    use crate::test_utils::InMemoryStorage;

    /// Build the full cybersecurity graph and return (graph, node_ids).
    #[allow(dead_code)]
    struct CyberGraph {
        graph: Graph,
        // Users
        alice: NodeId,
        bob: NodeId,
        eve: NodeId,
        // Laptops
        laptop_alice: NodeId,
        laptop_bob: NodeId,
        laptop_eve: NodeId,
        // Internal IPs
        ip_alice: NodeId,
        ip_bob: NodeId,
        ip_eve: NodeId,
        // External hosts
        darktools: NodeId,
        legit_docs: NodeId,
        // Firewall alerts
        alert_malware: NodeId,
        alert_rdp: NodeId,
        alert_ssh: NodeId,
    }

    /// Timestamps (epoch millis) for the scenario.
    const LEASE_START: i64 = 1_736_928_000_000; // 2025-01-15 08:00 UTC
    const LEASE_END: i64 = 1_736_935_200_000; // 2025-01-15 10:00 UTC
    const ATTACK_TIME_1: i64 = 1_736_929_800_000; // 08:30
    const ATTACK_TIME_2: i64 = 1_736_930_700_000; // 08:45
    const ATTACK_TIME_3: i64 = 1_736_931_000_000; // 08:50

    fn build_cyber_graph() -> CyberGraph {
        let graph = Graph::new(Box::new(InMemoryStorage::new()));

        // -- Users --
        let alice = graph
            .create_node(
                vec!["User".into()],
                serde_json::json!({
                    "name": "Alice",
                    "department": "Engineering",
                    "role": "Senior Developer"
                }),
                None,
            )
            .unwrap();
        let bob = graph
            .create_node(
                vec!["User".into()],
                serde_json::json!({
                    "name": "Bob",
                    "department": "Finance",
                    "role": "Accountant"
                }),
                None,
            )
            .unwrap();
        let eve = graph
            .create_node(
                vec!["User".into()],
                serde_json::json!({
                    "name": "Eve",
                    "department": "Marketing",
                    "role": "Analyst"
                }),
                None,
            )
            .unwrap();

        // -- Laptops --
        let laptop_alice = graph
            .create_node(
                vec!["Laptop".into()],
                serde_json::json!({
                    "brand": "Apple",
                    "model": "MacBook Pro 16",
                    "serial_number": "SN-APP-1001",
                    "hostname": "ALICE-MBP01"
                }),
                None,
            )
            .unwrap();
        let laptop_bob = graph
            .create_node(
                vec!["Laptop".into()],
                serde_json::json!({
                    "brand": "Lenovo",
                    "model": "ThinkPad X1 Carbon",
                    "serial_number": "SN-LEN-2001",
                    "hostname": "BOB-TP01"
                }),
                None,
            )
            .unwrap();
        let laptop_eve = graph
            .create_node(
                vec!["Laptop".into()],
                serde_json::json!({
                    "brand": "Dell",
                    "model": "Latitude 5540",
                    "serial_number": "SN-DEL-3001",
                    "hostname": "EVE-LAT01"
                }),
                None,
            )
            .unwrap();

        // Laptop -> User assignments
        graph
            .create_edge(
                laptop_alice,
                alice,
                "ASSIGNED_TO".into(),
                serde_json::json!({"assigned_date": "2024-03-15"}),
                1.0,
                None,
                None,
            )
            .unwrap();
        graph
            .create_edge(
                laptop_bob,
                bob,
                "ASSIGNED_TO".into(),
                serde_json::json!({"assigned_date": "2024-06-01"}),
                1.0,
                None,
                None,
            )
            .unwrap();
        graph
            .create_edge(
                laptop_eve,
                eve,
                "ASSIGNED_TO".into(),
                serde_json::json!({"assigned_date": "2024-09-10"}),
                1.0,
                None,
                None,
            )
            .unwrap();

        // -- IP Addresses --
        let ip_alice = graph
            .create_node(
                vec!["IPAddress".into()],
                serde_json::json!({"address": "10.0.1.10", "network": "internal"}),
                None,
            )
            .unwrap();
        let ip_bob = graph
            .create_node(
                vec!["IPAddress".into()],
                serde_json::json!({"address": "10.0.1.20", "network": "internal"}),
                None,
            )
            .unwrap();
        let ip_eve = graph
            .create_node(
                vec!["IPAddress".into()],
                serde_json::json!({"address": "10.0.1.50", "network": "internal"}),
                None,
            )
            .unwrap();

        // -- DHCP Leases (temporal edges: IP -> Laptop) --
        graph
            .create_edge(
                ip_alice,
                laptop_alice,
                "DHCP_LEASE".into(),
                serde_json::json!({"dhcp_server": "10.0.0.1"}),
                1.0,
                Some(LEASE_START),
                Some(LEASE_END),
            )
            .unwrap();
        graph
            .create_edge(
                ip_bob,
                laptop_bob,
                "DHCP_LEASE".into(),
                serde_json::json!({"dhcp_server": "10.0.0.1"}),
                1.0,
                Some(LEASE_START),
                Some(LEASE_END),
            )
            .unwrap();
        graph
            .create_edge(
                ip_eve,
                laptop_eve,
                "DHCP_LEASE".into(),
                serde_json::json!({"dhcp_server": "10.0.0.1"}),
                1.0,
                Some(LEASE_START),
                Some(LEASE_END),
            )
            .unwrap();

        // -- External hosts --
        let darktools = graph
            .create_node(
                vec!["ExternalHost".into()],
                serde_json::json!({
                    "hostname": "darktools.example.com",
                    "ip_address": "198.51.100.66",
                    "category": "malware_distribution",
                    "risk_level": "critical"
                }),
                None,
            )
            .unwrap();
        let legit_docs = graph
            .create_node(
                vec!["ExternalHost".into()],
                serde_json::json!({
                    "hostname": "docs.example.com",
                    "ip_address": "203.0.113.10",
                    "category": "documentation",
                    "risk_level": "none"
                }),
                None,
            )
            .unwrap();

        // -- Network traffic --
        // Legitimate: Alice -> docs
        graph
            .create_edge(
                ip_alice,
                legit_docs,
                "TRAFFIC".into(),
                serde_json::json!({
                    "timestamp": ATTACK_TIME_1,
                    "dest_port": 443,
                    "protocol": "TCP",
                    "bytes_sent": 15200
                }),
                1.0,
                None,
                None,
            )
            .unwrap();

        // Eve -> malware site
        graph
            .create_edge(
                ip_eve,
                darktools,
                "TRAFFIC".into(),
                serde_json::json!({
                    "timestamp": ATTACK_TIME_1,
                    "dest_port": 443,
                    "protocol": "TCP",
                    "bytes_sent": 524288
                }),
                1.0,
                None,
                None,
            )
            .unwrap();
        // Eve -> Bob (RDP)
        graph
            .create_edge(
                ip_eve,
                ip_bob,
                "TRAFFIC".into(),
                serde_json::json!({
                    "timestamp": ATTACK_TIME_2,
                    "dest_port": 3389,
                    "protocol": "TCP",
                    "bytes_sent": 4096
                }),
                1.0,
                None,
                None,
            )
            .unwrap();
        // Eve -> Alice (SSH)
        graph
            .create_edge(
                ip_eve,
                ip_alice,
                "TRAFFIC".into(),
                serde_json::json!({
                    "timestamp": ATTACK_TIME_3,
                    "dest_port": 22,
                    "protocol": "TCP",
                    "bytes_sent": 2048
                }),
                1.0,
                None,
                None,
            )
            .unwrap();

        // -- Firewall alerts --
        let alert_malware = graph
            .create_node(
                vec!["FirewallAlert".into()],
                serde_json::json!({
                    "alert_id": "FW-2025-0042",
                    "rule": "MALWARE_DOWNLOAD",
                    "severity": "critical",
                    "timestamp": ATTACK_TIME_1,
                    "action": "logged"
                }),
                None,
            )
            .unwrap();
        let alert_rdp = graph
            .create_node(
                vec!["FirewallAlert".into()],
                serde_json::json!({
                    "alert_id": "FW-2025-0043",
                    "rule": "LATERAL_MOVEMENT_RDP",
                    "severity": "high",
                    "timestamp": ATTACK_TIME_2,
                    "action": "blocked"
                }),
                None,
            )
            .unwrap();
        let alert_ssh = graph
            .create_node(
                vec!["FirewallAlert".into()],
                serde_json::json!({
                    "alert_id": "FW-2025-0044",
                    "rule": "UNAUTHORIZED_SSH",
                    "severity": "high",
                    "timestamp": ATTACK_TIME_3,
                    "action": "blocked"
                }),
                None,
            )
            .unwrap();

        // IP -> Alert (TRIGGERED)
        graph
            .create_edge(
                ip_eve,
                alert_malware,
                "TRIGGERED".into(),
                serde_json::json!({"timestamp": ATTACK_TIME_1}),
                1.0,
                None,
                None,
            )
            .unwrap();
        graph
            .create_edge(
                ip_eve,
                alert_rdp,
                "TRIGGERED".into(),
                serde_json::json!({"timestamp": ATTACK_TIME_2}),
                1.0,
                None,
                None,
            )
            .unwrap();
        graph
            .create_edge(
                ip_eve,
                alert_ssh,
                "TRIGGERED".into(),
                serde_json::json!({"timestamp": ATTACK_TIME_3}),
                1.0,
                None,
                None,
            )
            .unwrap();

        // Alert -> Target (TARGETS)
        graph
            .create_edge(
                alert_malware,
                darktools,
                "TARGETS".into(),
                serde_json::json!({}),
                1.0,
                None,
                None,
            )
            .unwrap();
        graph
            .create_edge(
                alert_rdp,
                ip_bob,
                "TARGETS".into(),
                serde_json::json!({}),
                1.0,
                None,
                None,
            )
            .unwrap();
        graph
            .create_edge(
                alert_ssh,
                ip_alice,
                "TARGETS".into(),
                serde_json::json!({}),
                1.0,
                None,
                None,
            )
            .unwrap();

        CyberGraph {
            graph,
            alice,
            bob,
            eve,
            laptop_alice,
            laptop_bob,
            laptop_eve,
            ip_alice,
            ip_bob,
            ip_eve,
            darktools,
            legit_docs,
            alert_malware,
            alert_rdp,
            alert_ssh,
        }
    }

    // -- Investigation tests --

    #[test]
    fn trace_alert_to_source_ip() {
        let cg = build_cyber_graph();

        // The alert should have an incoming TRIGGERED edge from Eve's IP.
        let triggerers = cg
            .graph
            .neighbors_filtered(cg.alert_malware, Direction::Incoming, "TRIGGERED")
            .unwrap();
        assert_eq!(triggerers.len(), 1);
        assert_eq!(triggerers[0].1, cg.ip_eve);
    }

    #[test]
    fn trace_ip_to_laptop_via_dhcp() {
        let cg = build_cyber_graph();

        // Eve's IP -> Laptop via DHCP_LEASE
        let leases = cg
            .graph
            .neighbors_filtered(cg.ip_eve, Direction::Outgoing, "DHCP_LEASE")
            .unwrap();
        assert_eq!(leases.len(), 1);
        assert_eq!(leases[0].1, cg.laptop_eve);
    }

    #[test]
    fn trace_laptop_to_user_via_assignment() {
        let cg = build_cyber_graph();

        // Eve's laptop -> User via ASSIGNED_TO
        let assignments = cg
            .graph
            .neighbors_filtered(cg.laptop_eve, Direction::Outgoing, "ASSIGNED_TO")
            .unwrap();
        assert_eq!(assignments.len(), 1);
        assert_eq!(assignments[0].1, cg.eve);

        // Verify it's actually Eve
        let user = cg.graph.get_node(assignments[0].1).unwrap().unwrap();
        assert_eq!(user.properties["name"], "Eve");
    }

    #[test]
    fn full_investigation_alert_to_user() {
        let cg = build_cyber_graph();

        // Step 1: Alert -> Source IP (incoming TRIGGERED)
        let source_ips = cg
            .graph
            .neighbors_filtered(cg.alert_malware, Direction::Incoming, "TRIGGERED")
            .unwrap();
        assert_eq!(source_ips.len(), 1);
        let source_ip = source_ips[0].1;

        // Verify IP address
        let ip_node = cg.graph.get_node(source_ip).unwrap().unwrap();
        assert_eq!(ip_node.properties["address"], "10.0.1.50");

        // Step 2: IP -> Laptop (DHCP_LEASE)
        let laptops = cg
            .graph
            .neighbors_filtered(source_ip, Direction::Outgoing, "DHCP_LEASE")
            .unwrap();
        assert_eq!(laptops.len(), 1);
        let laptop = laptops[0].1;

        let laptop_node = cg.graph.get_node(laptop).unwrap().unwrap();
        assert_eq!(laptop_node.properties["hostname"], "EVE-LAT01");

        // Step 3: Laptop -> User (ASSIGNED_TO)
        let users = cg
            .graph
            .neighbors_filtered(laptop, Direction::Outgoing, "ASSIGNED_TO")
            .unwrap();
        assert_eq!(users.len(), 1);

        let user = cg.graph.get_node(users[0].1).unwrap().unwrap();
        assert_eq!(user.properties["name"], "Eve");
        assert_eq!(user.properties["department"], "Marketing");
    }

    #[test]
    fn dhcp_lease_temporal_validity() {
        let cg = build_cyber_graph();

        // Get the DHCP lease edge for Eve's IP
        let leases = cg
            .graph
            .neighbors_filtered(cg.ip_eve, Direction::Outgoing, "DHCP_LEASE")
            .unwrap();
        let edge = cg.graph.get_edge(leases[0].0).unwrap().unwrap();

        // Verify temporal bounds
        assert_eq!(edge.validity.valid_from, Some(LEASE_START));
        assert_eq!(edge.validity.valid_to, Some(LEASE_END));

        // Attack happened during the lease window
        assert!(edge.validity.contains(ATTACK_TIME_1));
        assert!(edge.validity.contains(ATTACK_TIME_2));
        assert!(edge.validity.contains(ATTACK_TIME_3));

        // Before and after lease should not match
        assert!(!edge.validity.contains(LEASE_START - 1));
        assert!(!edge.validity.contains(LEASE_END));
    }

    #[test]
    fn eve_outbound_traffic() {
        let cg = build_cyber_graph();

        // Eve's IP should have 3 outbound TRAFFIC edges
        let traffic = cg
            .graph
            .neighbors_filtered(cg.ip_eve, Direction::Outgoing, "TRAFFIC")
            .unwrap();
        assert_eq!(traffic.len(), 3);

        // Destinations should be: darktools, Bob's IP, Alice's IP
        let dest_ids: Vec<NodeId> = traffic.iter().map(|(_, nid)| *nid).collect();
        assert!(dest_ids.contains(&cg.darktools));
        assert!(dest_ids.contains(&cg.ip_bob));
        assert!(dest_ids.contains(&cg.ip_alice));
    }

    #[test]
    fn eve_triggered_all_alerts() {
        let cg = build_cyber_graph();

        // Eve's IP should have triggered 3 alerts
        let alerts = cg
            .graph
            .neighbors_filtered(cg.ip_eve, Direction::Outgoing, "TRIGGERED")
            .unwrap();
        assert_eq!(alerts.len(), 3);

        let alert_ids: Vec<NodeId> = alerts.iter().map(|(_, nid)| *nid).collect();
        assert!(alert_ids.contains(&cg.alert_malware));
        assert!(alert_ids.contains(&cg.alert_rdp));
        assert!(alert_ids.contains(&cg.alert_ssh));
    }

    #[test]
    fn bfs_from_eve_ip_discovers_attack_surface() {
        let cg = build_cyber_graph();

        // BFS depth 1 from Eve's IP: should reach DHCP target, traffic targets, alerts
        let bfs_d1 = cg.graph.bfs(cg.ip_eve, 1).unwrap();
        let found: Vec<NodeId> = bfs_d1.iter().map(|(nid, _)| *nid).collect();

        // Should find Eve's IP itself at depth 0
        assert!(found.contains(&cg.ip_eve));
        // Depth 1: laptop (DHCP), darktools (TRAFFIC), Bob's IP, Alice's IP, 3 alerts
        assert!(found.contains(&cg.laptop_eve));
        assert!(found.contains(&cg.darktools));
        assert!(found.contains(&cg.ip_bob));
        assert!(found.contains(&cg.ip_alice));
        assert!(found.contains(&cg.alert_malware));

        // BFS depth 2: should also reach Eve (via laptop->ASSIGNED_TO)
        let bfs_d2 = cg.graph.bfs(cg.ip_eve, 2).unwrap();
        let found_d2: Vec<NodeId> = bfs_d2.iter().map(|(nid, _)| *nid).collect();
        assert!(found_d2.contains(&cg.eve));
    }

    #[test]
    fn shortest_path_eve_ip_to_bob_ip() {
        let cg = build_cyber_graph();

        // There should be a direct TRAFFIC edge from Eve's IP to Bob's IP
        let path = cg
            .graph
            .shortest_path(cg.ip_eve, cg.ip_bob)
            .unwrap()
            .expect("path should exist");
        assert_eq!(path.len(), 1); // direct connection
        assert_eq!(path.start, cg.ip_eve);
        assert_eq!(path.end(), cg.ip_bob);
    }

    #[test]
    fn alice_traffic_is_legitimate() {
        let cg = build_cyber_graph();

        // Alice's IP should only have traffic to the legitimate docs site
        let traffic = cg
            .graph
            .neighbors_filtered(cg.ip_alice, Direction::Outgoing, "TRAFFIC")
            .unwrap();
        assert_eq!(traffic.len(), 1);
        assert_eq!(traffic[0].1, cg.legit_docs);
    }

    #[test]
    fn alert_targets_correct_destinations() {
        let cg = build_cyber_graph();

        // Malware alert targets darktools
        let targets = cg
            .graph
            .neighbors_filtered(cg.alert_malware, Direction::Outgoing, "TARGETS")
            .unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].1, cg.darktools);

        // RDP alert targets Bob's IP
        let targets = cg
            .graph
            .neighbors_filtered(cg.alert_rdp, Direction::Outgoing, "TARGETS")
            .unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].1, cg.ip_bob);

        // SSH alert targets Alice's IP
        let targets = cg
            .graph
            .neighbors_filtered(cg.alert_ssh, Direction::Outgoing, "TARGETS")
            .unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].1, cg.ip_alice);
    }

    #[test]
    fn bob_is_not_attacker() {
        let cg = build_cyber_graph();

        // Bob's IP should have no outbound TRAFFIC and no TRIGGERED alerts
        let traffic = cg
            .graph
            .neighbors_filtered(cg.ip_bob, Direction::Outgoing, "TRAFFIC")
            .unwrap();
        assert_eq!(traffic.len(), 0);

        let alerts = cg
            .graph
            .neighbors_filtered(cg.ip_bob, Direction::Outgoing, "TRIGGERED")
            .unwrap();
        assert_eq!(alerts.len(), 0);
    }

    #[test]
    fn node_counts_and_labels() {
        let cg = build_cyber_graph();

        // Verify key nodes have correct labels
        let eve_node = cg.graph.get_node(cg.eve).unwrap().unwrap();
        assert_eq!(eve_node.labels, vec!["User"]);

        let laptop = cg.graph.get_node(cg.laptop_eve).unwrap().unwrap();
        assert_eq!(laptop.labels, vec!["Laptop"]);

        let ip = cg.graph.get_node(cg.ip_eve).unwrap().unwrap();
        assert_eq!(ip.labels, vec!["IPAddress"]);

        let alert = cg.graph.get_node(cg.alert_malware).unwrap().unwrap();
        assert_eq!(alert.labels, vec!["FirewallAlert"]);

        let ext = cg.graph.get_node(cg.darktools).unwrap().unwrap();
        assert_eq!(ext.labels, vec!["ExternalHost"]);
    }
}
