# Binary declarations

The shared observer retains typed mapping, code/data and relocation declarations from the held inspection bytes. These records flow through ordinary evidence queries, native views and reviewed exports.

Mapping rows identify file offsets, declared source coordinates, file/memory extents, executable flags and captured prefix lengths. ELF mapping symbols describe code/data intervals and their agreement with mappings. ELF relocation rows retain source-table positions, type codes, symbol declarations and explicit signed addends.

PE base-relocation rows retain block/slot, source entry offset, raw type, page/offset RVA, padding classification, HIGHADJ paired words with separate source offsets, known target widths and captured unambiguous target mappings. Source RVAs and inventory coverage remain explicit. Machine-specific types retain their raw classification. Other PE machine classes retain structural metadata alongside their machine identity.

Observation budgets are32PEblocks/128slots,64retained rows per typed group,32KiB of typed declarations and256KiB per saved snapshot. Retained/omitted counts and completeness fields accompany the records. Original encoding nodes and relationships keep their space before optional projections.

Verification uses original compiled ELF/Arm64 PE fixtures with LLVM metadata/disassembly comparisons, actual inspection callbacks, retained model queries, native views and guarded create/readback exports. Format reference: Microsoft [PE Format](https://learn.microsoft.com/en-us/windows/win32/debug/pe-format#the-reloc-section-image-only).
