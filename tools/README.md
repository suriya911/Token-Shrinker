# Tool descriptors

Optional-tool support is data-driven. A future descriptor lives at `tools/<tool-id>/tool.toml` and must validate against the decoded structure in `schemas/tool-descriptor.schema.json`.

Descriptors declare identity, authoritative release source, ownership, capabilities, typed release discovery, and a typed health probe. They cannot contain arbitrary shell scripts. Adding activation or installation primitives is post-v1 work and requires security review.

No external tool is registered during M0. Real descriptors are added with their provider adapter and contract fixtures.

