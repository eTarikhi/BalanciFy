# balancify

Windows-native multi-WAN connection aggregation: load balancing, packet bonding and seamless failover across Wi-Fi, Ethernet and cellular behind one virtual network interface.

## Technical blueprint

The engineering decision document lives in [`docs/technical-blueprint.md`](docs/technical-blueprint.md):

- Comparative analysis of aggregation methodologies (policy routing → packet bonding → multipath transports)
- Recommended architecture (user-mode Rust service + Wintun adapter + encrypted multipath tunnel to an operator-run relay; gated kernel-driver path)
- Data-driven technology-stack recommendations with verified platform/signing evidence
- GUI/UX and web-presence design
- Security, reliability and compliance plan (driver signing, HVCI/Memory Integrity, SBOM/VEX)
- Performance and validation plan with proposed acceptance criteria
- Nine-phase, eight-gate development roadmap
- Skills and tools matrix, risk register, and sourced assumptions

Supporting inputs: [`SKILL.md`](SKILL.md), [`report-outline.md`](report-outline.md), [`Windows Multi-WAN Aggregation Blueprint.docx`](Windows%20Multi-WAN%20Aggregation%20Blueprint.docx). Diagrams: [`docs/assets/`](docs/assets/).
