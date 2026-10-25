# Research Log — Job 3

Scope: external research performed during Job 3, distinct from internal design decisions (recorded in the ADRs) and from code review (recorded in JOB3-OVERVIEW).

---

## 1. Rust 2018 edition — module declaration, not elimination

**Question**: does the module-organization proposal's "no mod.rs" claim hold up, and does it mean what it was assumed to mean?

**Checked**: the Rust Edition Guide's own Path and Module System chapter, plus the original tracking issue.

**Finding**: the common summary ("mod.rs is no longer needed") is close but imprecise. What actually changed: a module *with submodules* previously had to be named `foo/mod.rs`; as of the 2018 edition, it can instead be `foo.rs`, sitting as a sibling to a `foo/` directory holding the submodule files. The Edition Guide states it directly: the restriction that a module with submodules must be named `mod.rs` is lifted — `foo.rs` can just be `foo.rs`, and the submodule is still `foo/bar.rs`. This is a relocation of the declaration file, not its elimination — one file per module-with-submodules still has to declare which submodules exist; it's just no longer forced to sit inside the directory it's declaring.

**Fed into**: ADR-039. Confirms "no nested mod.rs per subdirectory" as stated is accurate, while correcting the stronger, informal version of the claim ("zero declaration anywhere") that the phrase alone could be misread as.

---

## 2. ext3/ext4 HTree — real precedent for the B-tree design

**Question**: is hash-keyed, per-directory namespace resolution a genuinely proven approach, or an untested design being adopted on reasoning alone?

**Checked**: the Linux kernel's own ext4 filesystem documentation, and the HTree data structure's own description.

**Findings**:

- HTree is described directly as a specialized tree data structure for directory indexing, similar to a B-tree — constant depth of one or two levels, high fanout, keyed by a hash of the filename, and requiring no rebalancing. Three properties — hash-keyed, high-fanout, per-directory (HTree indexes are built per directory inode, not globally) — matching this session's B-tree decision independently, not by design.
- The measured payoff is concrete, not theoretical: HTree indexing took ext2-family directories from a practical limit of a few thousand files up to the range of tens of millions of files per directory.
- Entry format, per the kernel's own directory documentation: each interior level stores a hash-to-block map with fixed-size entries — 8 bytes each (4-byte hash + 4-byte block number). Same fixed-slot pattern as this session's 16-byte slots, narrower only because ext4's block numbers are 32-bit where CAFS's identifiers are 64-bit.
- Lookup mechanism confirmed as structurally identical to what was designed independently: the kernel docs describe computing the hash of the target name and using it to find the leaf node whose hash range contains it — stated explicitly as working basically the same as a B-tree keyed by the hash.
- One difference worth flagging, not yet resolved: ext4's htree checksums an interior/leaf block via a `dx_tail` structure appended when metadata checksums are enabled, and explicitly documents what happens when there's no room left for it — the filesystem tells the user to run `e2fsck -D` to rebuild the directory index with room reserved. This session's node layout doesn't yet address what happens if a node's own checksum can't fit after its slot array fills — the same category of question, not yet asked here.

**Fed into**: ADR-034. Changed the decision from "reasoned to be sound" to "matches a filesystem's shipped, field-tested design directly" — the citation, not just the reasoning, is what the ADR now rests on.
