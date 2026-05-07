# AWP Compliance Pre-Mappings

This directory contains AWP-to-regulation pre-mapping documents. Each document maps AWP's attestation features to specific clauses of one regulation, with explicit *necessary-but-not-sufficient* flags so the reader can see where AWP supplies cryptographic evidence and where the institution must layer in additional controls. These documents are decision-support for compliance and engineering leads — **they are not legal advice**, and they are written to be read alongside the working prototype rather than as standalone interpretations.

## Current and planned mappings

- **SR 11-7** ([`SR_11_7.md`](SR_11_7.md)) — *current.* US Federal Reserve Supervisory Letter SR 11-7, "Guidance on Model Risk Management" (April 4, 2011). The de-facto US standard for model risk in regulated financial institutions, now widely applied to AI agents. Persona A (Sarah, Director of Risk at a mid-market insurer; see [`../USER_JOURNEYS.md`](../USER_JOURNEYS.md)) is the primary reader.
- **EU AI Act** — *planned* (GTM Phase 2 or later). Articles 10–15 (high-risk AI systems: data governance, technical documentation, record-keeping, transparency, human oversight, accuracy/robustness/cybersecurity) are the natural mapping target.
- **HIPAA** — *planned* (GTM Phase 2 or later). The healthcare vertical equivalent: AWP attestations as supporting evidence for the audit-control and integrity standards of the Security Rule (45 CFR §164.312(b) and §164.312(c)(1)).

New mappings will be added when a Persona A buyer in the relevant vertical engages and the legal-interpretation work pays off in design-partner conversations. Until then, additional mappings are deliberately deferred — overclaiming on regulations we have not done the work for would damage the credibility of the mappings we have done.
