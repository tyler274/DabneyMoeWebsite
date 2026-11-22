//! Frame existing HTTP test responses as SSE for the streaming transport.
use serde_json::{Value, json};

pub fn stream(mut response: Value) -> String {
    let mut events = vec![json!({"type":"response.created","response":{"id":response["id"]}})];
    if response["status"] == "completed" {
        for (index, item) in response["output"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .enumerate()
        {
            if item.get("id").is_none() {
                item["id"] = json!(format!("item-{index}"));
            }
            let id = &item["id"];
            // A reasoning item is announced without its summary; the summary
            // streams as its own parts, and only the completed item holds it.
            let mut announced = item.clone();
            if item["type"] == "reasoning" {
                announced["summary"] = json!([]);
            }
            events.push(
                json!({"type":"response.output_item.added","output_index":index,"item":announced}),
            );
            if item["type"] == "reasoning" {
                for (part, summary) in item["summary"].as_array().unwrap().iter().enumerate() {
                    events.push(json!({"type":"response.reasoning_summary_part.added","output_index":index,"item_id":id,"summary_index":part,"part":{"type":"summary_text","text":""}}));
                    events.push(json!({"type":"response.reasoning_summary_text.delta","output_index":index,"item_id":id,"summary_index":part,"delta":summary["text"]}));
                    events.push(json!({"type":"response.reasoning_summary_text.done","output_index":index,"item_id":id,"summary_index":part,"text":summary["text"]}));
                    events.push(json!({"type":"response.reasoning_summary_part.done","output_index":index,"item_id":id,"summary_index":part,"part":summary}));
                }
            } else if item["type"] == "function_call" {
                events.push(json!({"type":"response.function_call_arguments.delta","output_index":index,"item_id":id,"delta":item["arguments"]}));
                events.push(json!({"type":"response.function_call_arguments.done","output_index":index,"item_id":id,"arguments":item["arguments"]}));
            } else if item["type"] == "message" {
                for (part, content) in item["content"].as_array().unwrap().iter().enumerate() {
                    events.push(json!({"type":"response.content_part.added","output_index":index,"item_id":id,"content_index":part,"part":{"type":"output_text","text":""}}));
                    events.push(json!({"type":"response.output_text.delta","output_index":index,"item_id":id,"content_index":part,"delta":content["text"]}));
                    events.push(json!({"type":"response.output_text.done","output_index":index,"item_id":id,"content_index":part,"text":content["text"]}));
                    events.push(json!({"type":"response.content_part.done","output_index":index,"item_id":id,"content_index":part,"part":content}));
                }
            }
            events
                .push(json!({"type":"response.output_item.done","output_index":index,"item":item}));
        }
    }
    events.push(json!({"type":format!("response.{}", response["status"].as_str().unwrap()),"response":response}));
    events
        .into_iter()
        .enumerate()
        .map(|(index, mut event)| {
            event["sequence_number"] = json!(index);
            format!("data: {event}\n\n")
        })
        .collect()
}
