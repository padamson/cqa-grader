//! The roadmap as data, in `public/roadmap.json`: Now (the next release,
//! with each item's status), Next and Later. Trunk copies the file into
//! every build, so main's copy is published at `/cqa-grader/dev/roadmap.json`.
//! The dev build shows its own copy, which is main's; every release build
//! fetches the dev one, so whichever version a visitor views, the roadmap
//! is the current one, and shows the copy compiled in here only if that
//! fetch fails. Moving an item is an edit to the file, made in the commit
//! that does the work.
//!
//! Each item says what done means as a list of acceptance criteria, and
//! each criterion names the test that proves it, as `<path>::<function>`
//! from the repository root (`src/grade.rs::two_attempts_at_one_question_are_an_error`).
//! An item marked done must name tests that exist; the host tests below
//! hold the file to that.
//!
//! When a release ships, delete its done items (the changelog keeps
//! them), move `milestone` and `next_milestone` on a version, promote
//! from Next, and add the few words the new `next_milestone` is about to
//! `themes` (the shipped release's entry can go).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::version::snapshot_url;

/// The roadmap this build was compiled with.
pub const BUILT: &str = include_str!("../public/roadmap.json");

/// Where main's roadmap is published, which every release build fetches.
pub fn current_url() -> String {
    format!("{}roadmap.json", snapshot_url("dev"))
}

/// Unknown keys, statuses and columns are tolerated rather than refused,
/// so an older snapshot can still read a roadmap a newer main publishes;
/// the host tests hold this repo's own file to exactly these fields and
/// values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Roadmap {
    /// The release the Now column is building toward.
    pub milestone: String,
    /// The release after it, which the Next column is planned for.
    pub next_milestone: String,
    /// What each release is about, in a few words, by version: Now and
    /// Next show their milestone's.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub themes: BTreeMap<String, String>,
    pub items: Vec<Item>,
}

impl Roadmap {
    /// The roadmap compiled into this build.
    pub fn built() -> Self {
        serde_json::from_str(BUILT).expect("public/roadmap.json parses; the host tests hold it")
    }

    /// The items in one column, in roadmap order.
    pub fn column(&self, horizon: Horizon) -> impl Iterator<Item = &Item> {
        self.items
            .iter()
            .filter(move |item| item.horizon == horizon)
    }

    /// What the release a column is building toward is about: the theme
    /// of Now's or Next's milestone, none for Later, a blank theme, or a
    /// milestone without one.
    pub fn theme(&self, horizon: Horizon) -> Option<&str> {
        let version = match horizon {
            Horizon::Now => &self.milestone,
            Horizon::Next => &self.next_milestone,
            Horizon::Later | Horizon::Unknown => return None,
        };
        self.themes
            .get(version)
            .map(String::as_str)
            .filter(|theme| !theme.trim().is_empty())
    }

    /// Done and total for the Now column.
    pub fn progress(&self) -> (usize, usize) {
        let done = self
            .column(Horizon::Now)
            .filter(|item| item.status == Status::Done)
            .count();
        (done, self.column(Horizon::Now).count())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Horizon {
    Now,
    Next,
    Later,
    /// A column a newer main added; its items are not shown here.
    #[serde(other)]
    Unknown,
}

impl Horizon {
    pub const ALL: [Horizon; 3] = [Horizon::Now, Horizon::Next, Horizon::Later];

    pub fn label(self) -> &'static str {
        match self {
            Horizon::Now => "Now",
            Horizon::Next => "Next",
            Horizon::Later => "Later",
            Horizon::Unknown => "",
        }
    }

    /// The column's id, and the prefix of its items' ids.
    pub fn id(self) -> &'static str {
        match self {
            Horizon::Now => "roadmap-now",
            Horizon::Next => "roadmap-next",
            Horizon::Later => "roadmap-later",
            Horizon::Unknown => "roadmap-unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    #[default]
    Planned,
    InProgress,
    Done,
    /// A status a newer main added; the item shows without one.
    #[serde(other)]
    Unknown,
}

impl Status {
    /// The badge text, or `None` for a status this build does not know.
    pub fn label(self) -> Option<&'static str> {
        match self {
            Status::Planned => Some("Planned"),
            Status::InProgress => Some("In progress"),
            Status::Done => Some("Done"),
            Status::Unknown => None,
        }
    }

    fn is_planned(&self) -> bool {
        *self == Status::Planned
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    /// Stable, so a test or a link can find the item: `<column id>-<slug>`.
    pub id: String,
    pub title: String,
    pub blurb: String,
    pub horizon: Horizon,
    /// Only Now items are anything but planned, which is the default.
    #[serde(default, skip_serializing_if = "Status::is_planned")]
    pub status: Status,
    /// The GitHub issue that tracks it, once there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue: Option<u32>,
    /// What done means. Required for Now and Next; Later may leave it out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub acceptance: Vec<Criterion>,
}

/// One acceptance criterion and the test that proves it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Criterion {
    /// What must be true, in a sentence.
    pub criterion: String,
    /// The proving test as `<path>::<function>`, the path from the
    /// repository root.
    pub test: String,
}

impl Criterion {
    /// The test's file and function, or `None` when `test` is not of the
    /// form `<path>.rs::<function>` with a path inside the repository.
    pub fn test_location(&self) -> Option<(&str, &str)> {
        let (path, function) = self.test.rsplit_once("::")?;
        let is_ident = !function.is_empty()
            && function
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_');
        let in_repo = !path.starts_with('/') && !path.split('/').any(|s| s == "..");
        (path.ends_with(".rs") && is_ident && in_repo).then_some((path, function))
    }
}

/// Whether `source` declares an active test named `name`: a `fn name(`
/// line, not commented out, whose attributes include a test attribute
/// and no `#[ignore`.
#[cfg(test)]
fn has_active_test(source: &str, name: &str) -> bool {
    let lines: Vec<&str> = source.lines().map(str::trim).collect();
    let declares = [format!("fn {name}("), format!("async fn {name}(")];
    lines.iter().enumerate().any(|(i, line)| {
        if !declares.iter().any(|d| line.starts_with(d.as_str())) {
            return false;
        }
        let attrs: Vec<&&str> = lines[..i]
            .iter()
            .rev()
            .take_while(|l| l.starts_with("#[") || l.starts_with("//"))
            .filter(|l| l.starts_with("#["))
            .collect();
        attrs
            .iter()
            .any(|a| a.starts_with("#[test]") || a.contains("::test"))
            && !attrs.iter().any(|a| a.starts_with("#[ignore"))
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::Path;

    use super::*;

    #[test]
    fn the_file_holds_exactly_the_roadmap_s_fields() {
        // An unknown or misspelled key parses (and is dropped), so the file
        // must survive a round trip through the types unchanged.
        let file: serde_json::Value = serde_json::from_str(BUILT).unwrap();
        assert_eq!(
            serde_json::to_value(Roadmap::built()).unwrap(),
            file,
            "public/roadmap.json has a key or value the types drop: a misspelling, \
             an unknown status or column, or a planned status, a null issue or an \
             empty acceptance list written out (leave those out)"
        );
    }

    #[test]
    fn main_s_roadmap_is_fetched_from_the_dev_build() {
        assert_eq!(current_url(), "/cqa-grader/dev/roadmap.json");
    }

    #[test]
    fn every_item_id_is_unique() {
        let roadmap = Roadmap::built();
        let ids: BTreeSet<&str> = roadmap.items.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(ids.len(), roadmap.items.len());
    }

    #[test]
    fn every_item_id_starts_with_its_column_id() {
        let strays: Vec<String> = Roadmap::built()
            .items
            .into_iter()
            .filter(|item| !item.id.starts_with(&format!("{}-", item.horizon.id())))
            .map(|item| item.id)
            .collect();
        assert_eq!(strays, Vec::<String>::new());
    }

    #[test]
    fn only_now_items_are_started_or_done() {
        let strays: Vec<String> = Roadmap::built()
            .items
            .into_iter()
            .filter(|item| item.horizon != Horizon::Now && item.status != Status::Planned)
            .map(|item| item.id)
            .collect();
        assert_eq!(strays, Vec::<String>::new());
    }

    #[test]
    fn every_now_and_next_item_says_what_done_means() {
        let vague: Vec<String> = Roadmap::built()
            .items
            .into_iter()
            .filter(|item| item.horizon != Horizon::Later && item.acceptance.is_empty())
            .map(|item| item.id)
            .collect();
        assert_eq!(vague, Vec::<String>::new());
    }

    #[test]
    fn every_criterion_names_its_test_as_a_path_and_a_function() {
        let malformed: Vec<String> = Roadmap::built()
            .items
            .iter()
            .flat_map(|item| &item.acceptance)
            .filter(|c| c.test_location().is_none())
            .map(|c| c.test.clone())
            .collect();
        assert_eq!(malformed, Vec::<String>::new());
    }

    #[test]
    fn every_done_criterion_names_a_test_that_exists() {
        // "Done" is the claim the page's popup makes; the test it names has
        // to be in the tree, or the claim is a promise.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let missing: Vec<String> = Roadmap::built()
            .items
            .iter()
            .filter(|item| item.status == Status::Done)
            .flat_map(|item| &item.acceptance)
            .filter(|c| {
                let Some((path, function)) = c.test_location() else {
                    return true;
                };
                let source = std::fs::read_to_string(root.join(path)).unwrap_or_default();
                !has_active_test(&source, function)
            })
            .map(|c| c.test.clone())
            .collect();
        assert_eq!(missing, Vec::<String>::new());
    }

    #[test]
    fn a_test_location_splits_at_the_last_separator() {
        let c = Criterion {
            criterion: String::new(),
            test: "crates/site/src/roadmap.rs::a_name".into(),
        };
        assert_eq!(
            c.test_location(),
            Some(("crates/site/src/roadmap.rs", "a_name"))
        );
    }

    #[test]
    fn a_test_outside_the_repository_is_not_a_location() {
        for test in [
            "/tmp/x.rs::f",
            "../other/src/lib.rs::f",
            "src/../../x.rs::f",
        ] {
            let c = Criterion {
                criterion: String::new(),
                test: test.into(),
            };
            assert_eq!(c.test_location(), None, "{test}");
        }
    }

    #[test]
    fn an_active_test_counts() {
        let src = "#[test]\nfn proves_it() {}\n";
        assert!(has_active_test(src, "proves_it"));
    }

    #[test]
    fn an_async_test_with_a_qualified_attribute_counts() {
        let src = "#[tokio::test]\n// why\nasync fn proves_it() {}\n";
        assert!(has_active_test(src, "proves_it"));
    }

    #[test]
    fn an_ignored_test_does_not_count() {
        let src = "#[test]\n#[ignore = \"needs a browser\"]\nfn proves_it() {}\n";
        assert!(!has_active_test(src, "proves_it"));
    }

    #[test]
    fn a_helper_that_is_not_a_test_does_not_count() {
        let src = "#[test]\nfn other() {}\n\nfn proves_it() {}\n";
        assert!(!has_active_test(src, "proves_it"));
    }

    #[test]
    fn a_commented_out_test_does_not_count() {
        let src = "#[test]\n// fn proves_it() {}\n";
        assert!(!has_active_test(src, "proves_it"));
    }

    #[test]
    fn a_test_named_by_module_path_is_not_a_location() {
        // `grade::tests::name` names no file to look in.
        for test in [
            "grade::tests::name",
            "src/grade.rs::",
            "src/grade.rs::a b",
            "name",
        ] {
            let c = Criterion {
                criterion: String::new(),
                test: test.into(),
            };
            assert_eq!(c.test_location(), None, "{test}");
        }
    }

    fn item(id: &str, horizon: Horizon, status: Status) -> Item {
        Item {
            id: id.into(),
            title: String::new(),
            blurb: String::new(),
            horizon,
            status,
            issue: None,
            acceptance: Vec::new(),
        }
    }

    #[test]
    fn progress_counts_done_items_in_now_and_ignores_the_other_columns() {
        let roadmap = Roadmap {
            milestone: "0.1.0".into(),
            next_milestone: "0.2.0".into(),
            themes: BTreeMap::new(),
            items: vec![
                item("roadmap-now-a", Horizon::Now, Status::Done),
                item("roadmap-now-b", Horizon::Now, Status::InProgress),
                item("roadmap-now-c", Horizon::Now, Status::Planned),
                item("roadmap-next-d", Horizon::Next, Status::Done),
            ],
        };
        assert_eq!(roadmap.progress(), (1, 3));
    }

    #[test]
    fn every_column_has_an_item() {
        let roadmap = Roadmap::built();
        for horizon in Horizon::ALL {
            assert!(
                roadmap.column(horizon).next().is_some(),
                "{horizon:?} is empty"
            );
        }
    }

    #[test]
    fn a_roadmap_with_a_key_this_build_does_not_know_still_reads() {
        let newer = r#"{"milestone": "9.0.0", "next_milestone": "9.1.0", "owner": "x",
            "items": [{"id": "roadmap-now-a", "title": "A", "blurb": "", "horizon": "now", "eta": "soon"}]}"#;
        assert_eq!(
            serde_json::from_str::<Roadmap>(newer)
                .map(|r| r.milestone)
                .ok(),
            Some("9.0.0".to_string())
        );
    }

    #[test]
    fn an_item_with_a_status_this_build_does_not_know_still_reads() {
        let newer = r#"{"milestone": "9.0.0", "next_milestone": "9.1.0", "items": [
            {"id": "roadmap-now-a", "title": "A", "blurb": "", "horizon": "now", "status": "blocked"}]}"#;
        assert_eq!(
            serde_json::from_str::<Roadmap>(newer)
                .map(|r| r.items[0].status)
                .ok(),
            Some(Status::Unknown)
        );
    }

    #[test]
    fn an_item_in_a_column_this_build_does_not_know_is_in_none_it_shows() {
        let newer = r#"{"milestone": "9.0.0", "next_milestone": "9.1.0", "items": [
            {"id": "roadmap-someday-a", "title": "A", "blurb": "", "horizon": "someday"}]}"#;
        let roadmap: Roadmap = serde_json::from_str(newer).unwrap();
        assert_eq!(
            Horizon::ALL.map(|horizon| roadmap.column(horizon).count()),
            [0, 0, 0]
        );
    }

    fn themed(themes: &[(&str, &str)]) -> Roadmap {
        Roadmap {
            milestone: "0.4.0".into(),
            next_milestone: "0.5.0".into(),
            themes: themes
                .iter()
                .map(|(v, t)| (v.to_string(), t.to_string()))
                .collect(),
            ..Roadmap::built()
        }
    }

    #[test]
    fn now_shows_its_milestone_s_theme() {
        assert_eq!(
            themed(&[("0.4.0", "Graphs as they are published")]).theme(Horizon::Now),
            Some("Graphs as they are published")
        );
    }

    #[test]
    fn next_shows_its_milestone_s_theme() {
        assert_eq!(
            themed(&[("0.5.0", "A pass that means something")]).theme(Horizon::Next),
            Some("A pass that means something")
        );
    }

    #[test]
    fn a_theme_for_a_release_no_column_is_building_toward_is_not_shown() {
        assert_eq!(themed(&[("0.3.0", "Old")]).theme(Horizon::Now), None);
    }

    #[test]
    fn a_blank_theme_is_none() {
        assert_eq!(themed(&[("0.4.0", "  ")]).theme(Horizon::Now), None);
    }

    #[test]
    fn later_has_no_theme() {
        assert_eq!(themed(&[("0.4.0", "x")]).theme(Horizon::Later), None);
    }

    #[test]
    fn the_built_roadmap_s_now_and_next_have_themes() {
        let roadmap = Roadmap::built();
        assert_eq!(
            [Horizon::Now, Horizon::Next].map(|h| roadmap.theme(h).is_some()),
            [true, true]
        );
    }
}
