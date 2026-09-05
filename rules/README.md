# WSI-DICOM rule catalogs

The source tree retains two catalogs with different active scopes:

| Catalog | Use |
| --- | --- |
| `wsi-dicom-bench-rules-2026c-v4.json` | Current 18-family core challenge and workbench runs, with `wsi-dicom-core-profile-2026c-v2.json` |
| `wsi-dicom-bench-rules-2026c-v2.json` | Current format-coverage runs using the Rust `general` profile and its 13-family inventory |

Profile v2 defines the selected core scope and grouped challenge rows. These rows do not establish
exhaustive normative DICOM coverage. Catalog and profile identifiers are stable provenance fields;
the version suffix alone does not mark a file as obsolete.

Superseded core catalogs and profile matrices are retained inside their sealed evidence packages,
rather than duplicated here. Current commands and evidence contracts are in the
[workbench guide](../docs/WORKBENCH.md).
