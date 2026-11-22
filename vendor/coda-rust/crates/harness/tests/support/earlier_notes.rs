use engine::recipe::{Observer, Provider, Request};
use harness::engine::{self, Invocation, Item, Turn};
use serde_json::json;

#[derive(Default)]
pub struct AcrossRuns {
    pub calls: usize,
    reference: String,
}
impl Provider for AcrossRuns {
    async fn exchange(
        &mut self,
        request: &Request,
        _: &engine::Cancellation,
        _: &engine::RunLimits,
        _: &mut dyn Observer,
    ) -> Result<Turn, engine::Cause> {
        let call = |id: &str, name: &str, arguments| {
            Item::Call(Invocation {
                id: id.into(),
                name: name.into(),
                arguments: serde_json::to_string(&arguments).unwrap(),
            })
        };
        let result = |id: &str| {
            request
                .items
                .iter()
                .find_map(|item| match item {
                    Item::MaterializedResult { call_id, output } if call_id == id => Some(output),
                    _ => None,
                })
                .expect("actual tool result reached the provider")
        };
        let items = match self.calls {
            0 => vec![call(
                "earlier-note",
                "save_working_note",
                json!({"text":"Exact progress from the earlier run.","fresh_window":false}),
            )],
            1 => {
                let value: serde_json::Value =
                    serde_json::from_str(result("earlier-note")).unwrap();
                self.reference = value["reference"].as_str().unwrap().into();
                vec![Item::assistant("First work completed. ".repeat(3000))]
            }
            2 => {
                assert!(request.items.iter().any(|item| matches!(item, Item::Message { text, .. } if text.contains("Earlier working notes") && text.contains(&self.reference))), "start-of-run fresh window must name the earlier run's working note");
                vec![call(
                    "open-earlier-note",
                    "read_working_note",
                    json!({"reference":self.reference}),
                )]
            }
            3 => {
                let value: serde_json::Value =
                    serde_json::from_str(result("open-earlier-note")).unwrap();
                assert_eq!(value["text"], "Exact progress from the earlier run.");
                vec![Item::assistant(
                    "Continued from the earlier run's exact note.",
                )]
            }
            _ => panic!("unexpected provider dispatch"),
        };
        self.calls += 1;
        Ok(Turn {
            items,
            usage: Default::default(),
            receipt: json!({"fixture":"earlier-notes","step":self.calls}),
        })
    }
}
