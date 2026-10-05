//! `dirsql context`: the agent usage guide, compiled in so it always
//! describes the binary that prints it.

// `docs/` in the crate root is a symlink to the workspace docs, and it ships in
// the published crate, so these paths resolve both in the repo and from crates.io.
const USAGE: &str = include_str!("../../docs/context/usage.md");
const RECIPES: &str = include_str!("../../docs/context/recipes.md");
const TROUBLESHOOTING: &str = include_str!("../../docs/context/troubleshooting.md");

/// The guide as printed: a `dirsql <version>` line, then each section.
pub fn guide() -> String {
    format!(
        "dirsql {}\n\n{USAGE}\n{RECIPES}\n{TROUBLESHOOTING}",
        env!("CARGO_PKG_VERSION")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_line_names_the_crate_version() {
        let guide = guide();
        assert_eq!(
            guide.lines().next(),
            Some(concat!("dirsql ", env!("CARGO_PKG_VERSION")))
        );
    }

    #[test]
    fn the_sections_follow_the_version_line_in_order() {
        let guide = guide();
        let usage = guide.find("\n## Usage\n").unwrap();
        let recipes = guide.find("\n## Recipes\n").unwrap();
        let troubleshooting = guide.find("\n## Troubleshooting\n").unwrap();
        assert!(usage < recipes && recipes < troubleshooting);
    }

    #[test]
    fn every_section_is_printed_whole() {
        let guide = guide();
        for section in [USAGE, RECIPES, TROUBLESHOOTING] {
            assert!(guide.contains(section));
        }
    }
}
