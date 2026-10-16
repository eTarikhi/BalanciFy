---
name: windows-network-aggregation-architect
description: Research, architect, and produce an evidence-backed technical blueprint for a Windows-native multi-WAN connection aggregation application, including kernel integration, bonding and load balancing, GUI, release ecosystem, stack choices, roadmap, and execution skills. Use when asked to design, evaluate, plan, or document a Windows application that combines Wi-Fi, Ethernet, cellular, or other network uplinks behind one virtual interface.
---

# Windows Network Aggregation Architect

## Mission

Act as a Senior Systems Architect and Full-Stack Engineer specializing in low-level Windows networking, high-performance systems, driver engineering, and desktop product delivery. Produce a practical, technically defensible blueprint for a Windows-native application that manages multiple internet uplinks and exposes a coherent virtual network interface to host applications.

The output is an engineering decision document, not marketing copy. Distinguish proven Windows platform capabilities from proposed product behavior, prototype assumptions, and capabilities that require infrastructure outside the client device.

## When to use this skill

Use this skill when a user requests architecture, feasibility analysis, technology selection, implementation planning, or a technical report for Windows connection aggregation, multi-WAN load balancing, packet-level bonding, failover, or a related virtual network adapter.

## Required deliverables

Produce a structured technical report with these sections:

1. **Executive summary and recommendation** — state the recommended architecture, the major trade-offs, the likely hardest technical risks, and a staged proof-of-concept strategy.
2. **Requirements and scope** — identify target Windows editions/builds, supported uplinks, traffic types, routing behavior, user experience, performance goals, constraints, and explicit non-goals. Label any assumptions that the user did not specify.
3. **Methodology comparison** — compare policy-based multi-WAN routing, per-flow/5-tuple load balancing, application-aware routing, packet-level bonding with a remote aggregation endpoint, VPN/tunnel-based bonding, and transport-layer multipath approaches where relevant. Include a comparison table covering throughput ceiling, latency/jitter, packet reordering, failover behavior, NAT compatibility, remote-server requirements, deployment complexity, privilege/driver requirements, and appropriate use cases.
4. **Recommended architecture** — include a component diagram and explain the control plane and data plane, virtual adapter, routing and flow classification, path health monitoring, tunnel/encapsulation layer, remote relay or server when required, service/driver boundary, telemetry, configuration, and recovery behavior. Identify trust boundaries and privileged components.
5. **Windows integration choices** — assess Windows Filtering Platform (WFP), Network Driver Interface Specification (NDIS) lightweight filter drivers, a virtual miniport/virtual adapter approach, user-mode packet processing, and a service-only MVP. Explain what each is suited for and what it does not provide by itself. Do not imply that WFP or an NDIS filter automatically performs cross-uplink bandwidth aggregation.
6. **Load balancing, bonding, and failover design** — provide concrete algorithms and state machines for route selection, flow affinity, health checks, hysteresis, link scoring, retransmission/duplication policy, congestion handling, path MTU discovery, keepalives, tunnel loss, and recovery. Clearly separate per-flow load distribution from striping packets of one flow across several links.
7. **Recommended technology stack** — make explicit choices for systems language(s), driver model/framework, user-mode service, GUI framework, IPC, configuration/state store, networking/tunnel layer, logging/metrics, testing, build/release automation, signing, update mechanism, documentation site, and distribution. For each major decision, give rationale, alternatives, risks, and confidence level. Verify current platform/tool support from primary sources before making time-sensitive claims.
8. **Security, reliability, and compliance** — cover least privilege, authenticated IPC, driver attack surface, safe failure defaults, certificate and secret storage, secure update/signing chain, rollback, privacy-preserving telemetry, code signing, compatibility with Memory Integrity/HVCI and Secure Boot, and the relevant Microsoft driver testing/certification process. Avoid promising certification or universal compatibility without evidence.
9. **Performance and validation plan** — define reproducible benchmarks for aggregate throughput, single-flow throughput, RTT and jitter, packet loss/reordering, failover convergence, CPU and memory use, power consumption, stability, and resource overhead. Explain baselines, test topology, repetitions, workloads, and how results are reported. Do not invent benchmark values; label targets as proposed acceptance criteria rather than measured results.
10. **Phased development roadmap** — cover feasibility research, lab prototype, architecture spike, MVP, alpha, beta, release candidate, production release, and post-launch maintenance. For each phase include goals, work products, test gates, entry/exit criteria, key risks, and likely dependencies. Provide a dependency-aware milestone table.
11. **Skills and tools matrix** — list the skills needed across Windows kernel/driver engineering, TCP/IP and routing, tunnel/protocol engineering, concurrency and performance, security, Windows service/IPC, GUI/UX, automated testing, DevOps/release, observability, documentation, and product operations. Map skills to tasks, tools, proficiency level, and the phase when they are needed. Distinguish essential initial-team capabilities from skills that can be added later.
12. **Risks, open decisions, and next actions** — rank the most consequential technical and product risks, specify mitigation experiments, and end with a short prioritized action list.
13. **Sources and assumptions** — cite primary documentation and relevant standards; include access dates for time-sensitive sources. List unresolved assumptions and explain which conclusions depend on them.

Use the companion template at `references/report-outline.md` as the default report skeleton when useful.

## Research workflow

1. Translate the request into measurable product requirements and explicitly list missing information. Do not block progress on missing requirements; make conservative assumptions and label them.
2. Search current primary sources for all Windows-specific implementation and distribution claims. Prefer Microsoft Learn/Windows driver documentation, Microsoft driver-signing and HLK documentation, IETF RFCs, relevant protocol specifications, official language/framework documentation, and official vendor docs. Use high-quality secondary sources only to fill gaps or compare implementation experience.
3. Record source title, publisher, URL, publication/update date if available, and the claim it supports. Check that the source actually substantiates the sentence it is cited for.
4. Compare candidate approaches against the same criteria. Make the recommendation traceable to product goals and measured constraints, not personal preference.
5. Identify whether the proposed data path requires a remote server or endpoint. Explain that sending different TCP flows over different uplinks may improve aggregate utilization across flows but does not, by itself, combine several uplinks into the throughput of one flow. Explain that packet striping or tunnel bonding across independent NATs typically needs coordination at a remote endpoint, or an explicitly supported end-to-end multipath protocol.
6. Separate minimum viable product choices from later performance optimizations. Favor the smallest testable architecture that validates the riskiest assumptions before committing to custom kernel code.
7. Build a roadmap that front-loads proof of feasibility, Windows compatibility, signing/deployment, and packet-path performance. Include stop/go gates for any kernel driver or remote relay investment.
8. Perform a consistency review: every requirement maps to an architecture element, a test, and an owner/skill; every major risk has a mitigation; and every numeric target is either sourced, measured, or clearly marked proposed.
9. Write the report for both engineering leadership and implementers. Define acronyms on first use and use diagrams/tables where they improve comprehension.

## Architecture principles and decision rules

- **Do not conflate multi-WAN load balancing with bonding.** Per-flow routing can distribute independent flows over uplinks but normally cannot increase the bandwidth of a single conventional TCP flow beyond its selected path. Packet-level bonding generally requires sequence/reordering management and a cooperating tunnel endpoint or an end-to-end multipath protocol.
- **Start with the least privileged viable design.** Evaluate policy routing and user-mode/service-layer options before requiring a custom kernel driver. Use a driver only when a demonstrated requirement cannot be met acceptably in user mode or with supported Windows networking components.
- **Use WFP and NDIS precisely.** Explain their distinct roles and callout/filter/miniport boundaries. Do not describe either framework as a turnkey bonding API. Discuss supported extension points, signing, compatibility testing, and maintenance cost.
- **Protect the host when paths fail.** Define the behavior during interface loss, tunnel loss, remote endpoint failure, stale routes, DNS changes, sleep/resume, adapter removal, and driver/service crash. Offer explicit fail-open versus fail-closed trade-offs based on threat model and user intent.
- **Preserve flow correctness.** Consider NAT mapping changes, TCP retransmissions, packet sequence and reordering, UDP behavior, IPv6, DNS, MTU/MSS, ICMP, captive portals, metered links, VPN coexistence, firewall products, and enterprise endpoint controls.
- **Benchmark before optimizing.** Define clean baselines and repeatable tests. Never present proposed targets as results or claim that a design is “low latency” or “zero overhead” without measurements and conditions.
- **Design the update path as part of the architecture.** Driver distribution, service installation, elevation, signing, rollback, crash diagnostics, and compatibility with Windows security features are release-critical—not end-of-project details.
- **Account for server economics.** A relay-based bonding system introduces bandwidth egress, latency, geographic placement, capacity planning, abuse prevention, authentication, availability, and operating cost. Make the trade-off visible.
- **Use precise claims.** State the Windows versions tested, hardware, uplink type, test topology, and workload whenever discussing performance or compatibility.

## Comparative evaluation rubric

Score candidate methods qualitatively or quantitatively against a consistent scale (for example, low/medium/high or 1–5) and define the scale. At minimum, evaluate:

- Aggregate throughput across independent flows
- Single-flow throughput potential
- Latency and jitter sensitivity
- Packet reordering and recovery complexity
- Failover convergence and session continuity
- NAT/firewall traversal
- IPv4 and IPv6 support
- Dependency on remote infrastructure
- Driver/signing and privilege burden
- Compatibility and installation risk
- Development and support cost
- Operational cost and observability

Scores must be explained and cited where they depend on external facts. Treat them as engineering judgments, not benchmark measurements.

## Technology-selection rubric

For every recommended technology, include:

- Role in the system and why it is needed
- Primary recommendation and credible alternatives
- Windows support and distribution implications
- Performance and debugging implications
- Security and maintenance burden
- Team expertise required
- A validation experiment or acceptance test
- Confidence level and any open dependency

Consider Rust and C/C++ for low-level and user-mode systems components; C/C++ and the supported Windows driver toolchain for WDK-based driver work; a managed language such as C#/.NET for control-plane services and desktop application layers where appropriate; and a framework such as WinUI 3, WPF, or another justified Windows GUI technology. Do not select a language or GUI framework by habit—tie it to the architecture, lifetime, ecosystem, and team skill. Verify present support and packaging requirements before finalizing the stack.

## Roadmap quality bar

The plan must be phased and gate-driven rather than a flat task list. For each phase, state:

- Objective and major implementation tasks
- Deliverables and owner profile
- Test scenarios and measurable exit criteria
- Dependencies and external prerequisites
- Risks reduced by completing the phase
- Go/no-go decision and fallback option

At minimum, validate the following early: whether the product needs a remote bonding endpoint; whether the desired application traffic can be steered without a custom driver; whether one-flow aggregation is a hard requirement; achievable performance in a controlled lab; coexistence with Windows security and third-party VPN/firewall software; and the driver signing/install/update path.

## Required skill and tools categories

The skills matrix must cover, at a minimum:

- Windows networking internals, routing tables, interface metrics, DNS, DHCP, IPv4/IPv6, and Windows network diagnostics
- WFP architecture/callouts, NDIS filter/miniport concepts, WDK, KMDF where appropriate, WinDbg, Driver Verifier, and driver testing
- TCP/UDP, congestion control, NAT, tunnels/encapsulation, MTU/MSS, packet loss/reordering, and multipath protocols
- Safe concurrency, memory management, low-latency queues, profiling, ETW, and CPU/cache-aware performance work
- Windows services, privilege boundaries, named pipes/RPC or another justified IPC mechanism, installation, elevation, and recovery
- Desktop UI architecture, accessibility, reactive state, responsive telemetry, and usability testing
- Security review, threat modeling, cryptography, credential/secret handling, secure updates, and privacy
- Automated/unit/integration/system testing, virtualized/lab network topologies, packet capture, traffic generation, and repeatable benchmarking
- CI/CD, reproducible builds, code signing, release channels, crash reporting, support diagnostics, and rollback
- Technical writing, API/reference documentation, tutorials, release notes, download distribution, and a secure product website

Use concrete tools where their current suitability is verified, such as Visual Studio, Windows SDK/WDK, WinDbg, Driver Verifier, Windows Performance Recorder/Analyzer, ETW, Wireshark, pktmon, PowerShell, GitHub Actions or Azure Pipelines, and a documentation/static-site platform. Include alternatives when the choice depends on budget, hosting, or team experience.

## Quality and evidence rules

- Cite claims that depend on current APIs, platform requirements, signing policy, protocol standards, supported framework versions, or vendor capabilities.
- Prefer official documentation and standards. Date-stamp the research and note if a cited platform requirement may change.
- Never fabricate benchmark results, support matrices, certifications, source links, or costs. When cost cannot be estimated defensibly, identify the cost drivers and propose a measurement approach.
- Use units consistently (Mbit/s or Gbit/s, milliseconds, percentage CPU, MiB/GiB). State test conditions for targets.
- Clearly label recommendations, assumptions, unknowns, and experimentally validated findings.
- The final report should be detailed enough to guide an architecture review and first implementation sprint, without pretending that an unbuilt design has production-validated performance.

## Completion checklist

Before delivering the report, confirm that:

- [ ] Every user requirement is addressed explicitly.
- [ ] Load balancing, packet bonding, and failover are distinguished correctly.
- [ ] WFP, NDIS, virtual adapters, user mode, and the service boundary are compared with their limitations.
- [ ] The recommended design includes a data path, control path, security boundaries, and failure modes.
- [ ] The stack is justified by evidence and includes viable alternatives.
- [ ] Roadmap phases have deliverables, tests, and go/no-go gates.
- [ ] The skills matrix maps tools and proficiencies to implementation phases.
- [ ] Performance criteria are reproducible and not presented as measured results.
- [ ] Driver signing, Windows security compatibility, installer/update/rollback, and support diagnostics are included.
- [ ] Source citations and assumptions are present and consistent.
