//! Stage progress reports completed work, never an estimated overall duration.
use serde::Deserialize;

pub(crate) const CONSTRUCTION_TIMEOUT_MS: u32 = 30 * 60 * 1000;

#[derive(Deserialize)]
pub(crate) struct ReadStatus {
    pub revision: String,
    pub phase: Option<String>,
    #[serde(default)]
    pub progress: Option<ReadWork>,
    #[serde(default)]
    pub preview: Option<PreviewBatch>,
}

#[derive(Deserialize)]
pub(crate) struct PreviewBatch {
    pub after: usize,
    pub next: usize,
    pub bricks: Vec<ergo_world::Brick>,
    pub meshes: std::collections::BTreeMap<String, ergo_world::geometry::Mesh>,
}
impl PreviewBatch {
    pub fn valid_for(&self, count: usize, has_mesh: impl Fn(&str) -> bool) -> bool {
        self.after == count
            && !self.bricks.is_empty()
            && self.next == count + self.bricks.len()
            && self.next <= 100_000
            && self
                .meshes
                .iter()
                .all(|(part, mesh)| part == &mesh.part && !has_mesh(part))
            && self.bricks.iter().enumerate().all(|(index, brick)| {
                brick.id as usize == count + index + 1
                    && (has_mesh(&brick.part) || self.meshes.contains_key(&brick.part))
            })
    }
}

#[derive(Deserialize)]
pub(crate) struct ReadWork {
    stage: String,
    completed: usize,
    total: usize,
    found_pieces: Option<usize>,
}

#[derive(Clone)]
pub(crate) struct Loading {
    pub title: String,
    pub detail: String,
    pub percent: Option<u8>,
    pub elapsed_s: u64,
    pub waiting_for_server: bool,
    pub preview_pieces: usize,
    started_ms: f64,
}

impl Loading {
    pub fn new(now_ms: f64) -> Self {
        Self {
            title: "Loading construction".into(),
            detail: "Connecting to the shared world…".into(),
            percent: None,
            elapsed_s: 0,
            waiting_for_server: true,
            preview_pieces: 0,
            started_ms: now_ms,
        }
    }
    pub fn tick(&mut self, now_ms: f64) {
        self.elapsed_s = ((now_ms - self.started_ms).max(0.) / 1000.) as u64;
    }
    pub fn elapsed(&self) -> String {
        if self.elapsed_s < 60 {
            format!("{}s elapsed", self.elapsed_s)
        } else {
            format!(
                "{}m {:02}s elapsed",
                self.elapsed_s / 60,
                self.elapsed_s % 60
            )
        }
    }
    pub fn client_stage(&mut self, title: &str, detail: impl Into<String>) {
        self.title = title.into();
        self.detail = detail.into();
        self.percent = None;
        self.waiting_for_server = false;
    }
    pub fn server_status(&mut self, status: ReadStatus) {
        if !self.waiting_for_server {
            return;
        }
        if let Some(work) = status.progress {
            let completed = work.completed.min(work.total);
            let percentage = if work.total == 0 {
                None
            } else {
                Some(((completed as u128 * 100) / work.total as u128) as u8)
            };
            match work.stage.as_str() {
                "discovering" => {
                    self.title = "Finding nearby bricks".into();
                    self.detail = format!(
                        "{} bricks found · {completed}/{} cells checked",
                        number(work.found_pieces.unwrap_or(0)),
                        work.total
                    );
                    // Cell count is not the unknown number of index rows or time remaining.
                    self.percent = None;
                }
                "pieces" => {
                    self.title = "Loading bricks".into();
                    self.detail = format!("{} of {} bricks", number(completed), number(work.total));
                    self.percent = percentage;
                }
                "membership" => {
                    self.title = "Checking constructions".into();
                    self.detail = format!("{completed} of {} constructions", work.total);
                    self.percent = percentage;
                }
                _ => self.preparing(status.phase.as_deref()),
            }
        } else {
            self.preparing(status.phase.as_deref());
        }
    }
    fn preparing(&mut self, phase: Option<&str>) {
        self.percent = None;
        match phase {
            Some("persistent cache lookup" | "nearby cache lookup") => {
                self.title = "Opening cached region".into();
                self.detail = "Checking the saved region for this revision…".into();
            }
            Some(phase) if phase.starts_with("encoding") => {
                self.title = "Preparing download".into();
                self.detail = "Compressing the construction for your browser…".into();
            }
            _ => {
                self.title = "Waiting for the server".into();
                self.detail = "Preparing the nearby region…".into();
            }
        }
    }
}

fn number(value: usize) -> String {
    let digits = value.to_string();
    let mut text = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            text.push(',');
        }
        text.push(digit);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_batches_require_contiguous_occurrences_and_previously_defined_prototypes() {
        let mut demo = ergo_world::demo::Demo::new();
        demo.add("3003", 4, 0., 0., 0.);
        demo.add("3003", 1, 4., 0., 0.);
        let part = demo.bricks[0].part.clone();
        let mut batch = PreviewBatch {
            after: 0,
            next: 1,
            bricks: demo.bricks[..1].to_vec(),
            meshes: demo.meshes,
        };
        assert!(batch.valid_for(0, |_| false));
        assert!(!batch.valid_for(1, |_| true));
        let meshes = std::mem::take(&mut batch.meshes);
        assert!(!batch.valid_for(0, |_| false));
        batch.after = 1;
        batch.next = 2;
        batch.bricks = demo.bricks[1..].to_vec();
        assert!(batch.valid_for(1, |id| id == part));
        batch.bricks[0].id = 1;
        assert!(!batch.valid_for(1, |id| meshes.contains_key(id)));
        let status: ReadStatus =
            serde_json::from_value(serde_json::json!({"revision":"accepted","preview":null}))
                .unwrap();
        assert!(status.preview.is_none());
        let loading = Loading::new(0.);
        assert_eq!(loading.preview_pieces, 0);
        assert!(CONSTRUCTION_TIMEOUT_MS > 180_000 && CONSTRUCTION_TIMEOUT_MS <= i32::MAX as u32);
    }
    fn status(stage: &str, completed: usize, total: usize) -> ReadStatus {
        serde_json::from_value(serde_json::json!({"revision":"accepted","phase":"working","progress":{"stage":stage,"completed":completed,"total":total,"found_pieces":91265}})).unwrap()
    }
    #[test]
    fn discovery_is_indeterminate_and_known_totals_are_stage_percentages() {
        let mut loading = Loading::new(0.);
        loading.server_status(status("discovering", 12, 27));
        assert_eq!(loading.percent, None);
        assert!(loading.detail.contains("91,265 bricks found"));
        loading.server_status(status("pieces", 2000, 6000));
        assert_eq!(loading.percent, Some(33));
        assert_eq!(loading.detail, "2,000 of 6,000 bricks");
        loading.server_status(status("pieces", 7000, 6000));
        assert_eq!(loading.percent, Some(100));
        loading.server_status(status("membership", 0, 0));
        assert_eq!(loading.percent, None);
    }
    #[test]
    fn delayed_server_reports_cannot_replace_download_or_render_stages() {
        let mut loading = Loading::new(1000.);
        loading.client_stage("Preparing display", "Building the 3D scene…");
        loading.server_status(status("pieces", 20, 100));
        assert_eq!(loading.title, "Preparing display");
        loading.tick(73000.);
        assert_eq!(loading.elapsed(), "1m 12s elapsed");
    }
    #[test]
    fn legacy_status_without_counters_remains_indeterminate() {
        let mut loading = Loading::new(0.);
        let status: ReadStatus = serde_json::from_value(
            serde_json::json!({"revision":"old","phase":"persistent cache lookup"}),
        )
        .unwrap();
        assert_eq!(status.revision, "old");
        loading.server_status(status);
        assert_eq!(loading.title, "Opening cached region");
        assert_eq!(loading.percent, None);
    }
}
