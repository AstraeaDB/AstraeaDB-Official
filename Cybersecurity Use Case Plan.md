## Cybersecurity Use Case Plan
You are developing an example use case for AstraeaDB regarding cybersecurity. It may be helpful to create subagents that specialize in Cybersecurity and/or network technology.

## Background and Intuition
Cybersecurity tools often only know the context of network addresses. For example, a firewall may detect an attempt to access a blocked service by an IP address, but IP addresses are ephemeral, and an alert logged by the firewall doesn't tell the security analyst what machine and what user may have been involved.

Companies typically have asset management datasets that define, say, a laptop brand, serial number, hostname, and the user to which it is assigned. They also usually employ DHCP to dynamically assign IP addresses to laptops on the network, and the DHCP server logs hostname, the IP address assigned, and the length of the lease, which typically renews every 1-2 hours, or may be reassigned if the user is no longer active. Network controls like firewalls and routers monitor IP address traffic from some source IP to some destination IP. 

A Cyber analyst often has the complex job of chacing down an alert from a network appliance, trying to determine which hostname was assigned to the alerted IP address at that particular time, and then figuring out which user is assigned to the hostname.

## The Task
Create and test an example usage of AstraeaDB for documentation in the README as follows:

- Create a mock dataset for network traffic from source IP to destination IP
- Create a mock dataset simulating a firewall log which alerts on risky attempted behavior by IP addresses on the network
- create a mock dataset simulating DHCP logs showing hostnames being assigned to IP addresses
- Create a mock business dataset showing which laptops are assigned to which users, including the hostname of the laptop and the date on which it was assigned

For the datasets, assume all of the internal IP addresses are on a 10.0.x.y network.

Create, test, and document int he README an example use case showing how each of those datasets could be loaded as graphs into AstraeaDB, and an analyst could easily query AstraeaDB for an IP address and see who the user is and what activity they've been up to. Use the typical Bob-Alice-Eve scenario where Eve is trying to do something malicious to Bob and Alice. For example, Eve may visit a risky website to download a password cracker, and then use that to try to remotely log into Bob or Alice's laptops.
