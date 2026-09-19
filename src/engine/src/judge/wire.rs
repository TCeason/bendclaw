//! Questions <-> the tool schema llmproxy's Jev channel understands.
//!
//! On the wire a decision is one tool whose object schema lists the
//! questions: `boolean` is a noul, a string `enum` a choice, `x-levels` a
//! score. The answer is a `tool_use` whose input holds the decided values and
//! a text block holding the raw answers (probabilities, confidence). The raw
//! block is what callers want; the tool input is a fallback for a relay that
//! dropped the text.

use std::collections::HashMap;

use serde_json::json;
use serde_json::Value;

use super::Answer;
use super::JudgeError;
use super::Question;
use super::QuestionKind;

pub(super) const TOOL_NAME: &str = "judge";

pub(super) fn tool_schema(questions: &[Question]) -> Value {
    let mut properties = serde_json::Map::new();
    for question in questions {
        let mut property = serde_json::Map::new();
        property.insert("description".into(), json!(question.instructions));
        match &question.kind {
            QuestionKind::Noul { yes, no } => {
                property.insert("type".into(), json!("boolean"));
                if yes.is_some() || no.is_some() {
                    let mut criteria = serde_json::Map::new();
                    if let Some(yes) = yes {
                        criteria.insert("true".into(), json!(yes));
                    }
                    if let Some(no) = no {
                        criteria.insert("false".into(), json!(no));
                    }
                    property.insert("x-criteria".into(), Value::Object(criteria));
                }
            }
            QuestionKind::Choice(options) => {
                property.insert("type".into(), json!("string"));
                property.insert(
                    "enum".into(),
                    json!(options.iter().map(|(name, _)| name).collect::<Vec<_>>()),
                );
                let described: Vec<Value> = options
                    .iter()
                    .filter_map(|(name, rubric)| {
                        rubric
                            .as_ref()
                            .map(|text| json!({"const": name, "description": text}))
                    })
                    .collect();
                if !described.is_empty() {
                    property.insert("oneOf".into(), Value::Array(described));
                }
            }
            QuestionKind::Score(levels) => {
                property.insert("type".into(), json!("integer"));
                property.insert("x-levels".into(), json!(levels));
            }
        }
        properties.insert(question.id.clone(), Value::Object(property));
    }
    json!({
        "type": "object",
        "properties": properties,
        "required": questions.iter().map(|q| q.id.as_str()).collect::<Vec<_>>(),
    })
}

/// Read answers from the assistant reply: the raw-answers text block first,
/// the tool input as a fallback.
pub(super) fn parse_answers(
    questions: &[Question],
    text: Option<&str>,
    tool_input: Option<&Value>,
) -> Result<HashMap<String, Answer>, JudgeError> {
    if let Some(raw) = text.and_then(|t| serde_json::from_str::<Value>(t.trim()).ok()) {
        if let Some(answers) = raw.as_object() {
            let parsed = questions
                .iter()
                .filter_map(|q| {
                    answers
                        .get(&q.id)
                        .and_then(|a| raw_answer(&q.kind, a))
                        .map(|a| (q.id.clone(), a))
                })
                .collect::<HashMap<_, _>>();
            if parsed.len() == questions.len() {
                return Ok(parsed);
            }
        }
    }
    let input = tool_input
        .and_then(Value::as_object)
        .ok_or_else(|| JudgeError::Malformed("no answers in reply".into()))?;
    questions
        .iter()
        .map(|q| {
            let value = input
                .get(&q.id)
                .ok_or_else(|| JudgeError::Malformed(format!("missing answer for {}", q.id)))?;
            decided_answer(&q.kind, value)
                .map(|a| (q.id.clone(), a))
                .ok_or_else(|| JudgeError::Malformed(format!("unreadable answer for {}", q.id)))
        })
        .collect()
}

fn raw_answer(kind: &QuestionKind, raw: &Value) -> Option<Answer> {
    let number = |key: &str| raw.get(key).and_then(Value::as_f64);
    match kind {
        QuestionKind::Noul { .. } => Some(Answer::Noul {
            probability: number("noul")?,
        }),
        QuestionKind::Choice(_) => Some(Answer::Choice {
            choice: raw.get("choice")?.as_str()?.to_string(),
            probabilities: probabilities_map(raw.get("probabilities")?)?,
            confidence: number("confidence").unwrap_or(0.0),
        }),
        QuestionKind::Score(levels) => {
            let map = probabilities_map(raw.get("probabilities")?)?;
            Some(Answer::Score {
                score: number("score")?,
                probabilities: (0..levels.len())
                    .map(|i| map.get(&i.to_string()).copied().unwrap_or(0.0))
                    .collect(),
                confidence: number("confidence").unwrap_or(0.0),
            })
        }
    }
}

/// Without the raw block only the decided value survives: a boolean becomes
/// a probability of 0 or 1, a choice a certain one, a score its level.
fn decided_answer(kind: &QuestionKind, value: &Value) -> Option<Answer> {
    match kind {
        QuestionKind::Noul { .. } => Some(Answer::Noul {
            probability: if value.as_bool()? { 1.0 } else { 0.0 },
        }),
        QuestionKind::Choice(options) => {
            let choice = value.as_str()?.to_string();
            let probabilities = options
                .iter()
                .map(|(name, _)| (name.clone(), if *name == choice { 1.0 } else { 0.0 }))
                .collect();
            Some(Answer::Choice {
                choice,
                probabilities,
                confidence: 1.0,
            })
        }
        QuestionKind::Score(levels) => {
            let index = levels
                .iter()
                .position(|level| Some(level.as_str()) == value.as_str())?;
            Some(Answer::Score {
                score: index as f64,
                probabilities: (0..levels.len())
                    .map(|i| if i == index { 1.0 } else { 0.0 })
                    .collect(),
                confidence: 1.0,
            })
        }
    }
}

fn probabilities_map(value: &Value) -> Option<HashMap<String, f64>> {
    value
        .as_object()?
        .iter()
        .map(|(k, v)| v.as_f64().map(|p| (k.clone(), p)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn questions() -> Vec<Question> {
        vec![
            Question::noul("keep", "Still needed?").with_criteria("needed", "stale"),
            Question {
                id: "kind".into(),
                instructions: "What is it?".into(),
                kind: QuestionKind::Choice(vec![
                    ("a".into(), Some("first".into())),
                    ("b".into(), None),
                ]),
            },
            Question {
                id: "risk".into(),
                instructions: "How risky?".into(),
                kind: QuestionKind::Score(vec!["low".into(), "high".into()]),
            },
        ]
    }

    #[test]
    fn schema_maps_each_kind_to_the_channel_convention() {
        let schema = tool_schema(&questions());
        let props = &schema["properties"];
        assert_eq!(props["keep"]["type"], "boolean");
        assert_eq!(props["keep"]["x-criteria"]["true"], "needed");
        assert_eq!(props["kind"]["enum"], json!(["a", "b"]));
        assert_eq!(props["kind"]["oneOf"][0]["const"], "a");
        assert_eq!(props["risk"]["x-levels"], json!(["low", "high"]));
        assert_eq!(schema["required"], json!(["keep", "kind", "risk"]));
    }

    #[test]
    fn raw_answers_win_over_the_decided_values() {
        let raw = r#"{"keep":{"noul":0.83},"kind":{"choice":"a","probabilities":{"a":0.7,"b":0.3},"confidence":0.6},
                      "risk":{"score":0.9,"probabilities":{"0":0.1,"1":0.9},"confidence":0.8}}"#;
        let answers = parse_answers(&questions(), Some(raw), Some(&json!({"keep": true}))).unwrap();
        assert_eq!(answers["keep"].probability(), Some(0.83));
        assert!(matches!(&answers["kind"], Answer::Choice { choice, .. } if choice == "a"));
        assert!(
            matches!(&answers["risk"], Answer::Score { probabilities, .. } if probabilities == &vec![0.1, 0.9])
        );
    }

    #[test]
    fn decided_values_are_the_fallback() {
        let input = json!({"keep": false, "kind": "b", "risk": "high"});
        let answers = parse_answers(&questions(), Some("not json"), Some(&input)).unwrap();
        assert_eq!(answers["keep"].probability(), Some(0.0));
        assert!(matches!(&answers["risk"], Answer::Score { score, .. } if *score == 1.0));
        assert!(parse_answers(&questions(), None, Some(&json!({"keep": true}))).is_err());
    }
}
