//! The pre-online builds seeded these packages into the default theme
//! directory under their bare IDs. Explicit downloads have source-qualified paths.
use std::path::Path;

use resume_one_core::ThemeManifest;

pub(super) fn is_previous_default(path: &Path, manifest: &ThemeManifest) -> bool {
    let provenance = &manifest.normalized["provenance"];
    path.file_name().is_some_and(|name| name == "theme.json")
        && path
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == manifest.id.as_str())
        && IDS.binary_search(&manifest.id.as_str()).is_ok()
        && provenance["fidelity"] == "adapted"
        && matches!(
            provenance["migratedBy"].as_str(),
            Some("Resume One contributors" | "resume-one-theme-migrate --adapt")
        )
}

const IDS: &[&str] = &[
    "community-academic-cv-lite",
    "community-architects-portfolio",
    "community-art-deco",
    "community-art-school-modern",
    "community-asymmetric-timeline",
    "community-berlin-grid",
    "community-bold-header-statement",
    "community-brutalist",
    "community-californian-warm",
    "community-claude",
    "community-clinical-precision",
    "community-coastal-creative",
    "community-colophon",
    "community-community-garden",
    "community-consultant-polished",
    "community-creative-confidence",
    "community-creative-studio",
    "community-data-driven",
    "community-desert-modern",
    "community-developer-mono",
    "community-diagonal-accent-bar",
    "community-elegant",
    "community-elegant-pink",
    "community-even",
    "community-executive-slate",
    "community-field-researcher",
    "community-flat",
    "community-french-atelier",
    "community-government-standard",
    "community-graph-paper-grid",
    "community-industrial-engineer",
    "community-investor-brief",
    "community-jacrys",
    "community-kendall",
    "community-london-bureau",
    "community-lucide",
    "community-macchiato",
    "community-marketing-narrative",
    "community-mid-century-resume",
    "community-minimalist-grid",
    "community-minyma",
    "community-modern-classic",
    "community-monochrome-noir",
    "community-new-york-editorial",
    "community-nordic-minimal",
    "community-operations-precision",
    "community-pacific-horizon",
    "community-paper-plus-plus",
    "community-pro",
    "community-product-manager-canvas",
    "community-professional",
    "community-pumpkin",
    "community-reference",
    "community-rickosborne",
    "community-sales-hunter",
    "community-sidebar",
    "community-sidebar-photo-strip",
    "community-stackoverflow",
    "community-tailwind",
    "community-tan-responsive",
    "community-tokyo-modernist",
    "community-two-column",
    "community-two-column-modernist",
    "community-typewriter-modern",
    "community-university-first",
    "community-urban-techno",
    "community-writers-portfolio",
];
